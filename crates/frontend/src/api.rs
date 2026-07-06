use crate::*;

/// API failure carrying the HTTP status so callers can tell "not logged in"
/// (401) apart from real errors. Network failures use status 0.
#[derive(Debug, Clone)]
pub(crate) struct ApiError {
    pub(crate) status: u16,
    pub(crate) message: String,
}

impl ApiError {
    fn network(message: impl ToString) -> Self {
        Self {
            status: 0,
            message: message.to_string(),
        }
    }
}

pub(crate) async fn api_get<T: DeserializeOwned>(url: &str) -> Result<T, ApiError> {
    let response = Request::get(url)
        .credentials(RequestCredentials::SameOrigin)
        .send()
        .await
        .map_err(ApiError::network)?;
    decode_response(response).await
}

pub(crate) fn selected_workspace_id_from_url() -> Option<String> {
    web_sys::window()
        .and_then(|w| w.location().search().ok())
        .and_then(|search| {
            search
                .trim_start_matches('?')
                .split('&')
                .find_map(|pair| pair.strip_prefix("workspace=").map(str::to_string))
        })
        .filter(|id| !id.trim().is_empty())
}

fn workspace_scoped_url(base: &str) -> String {
    selected_workspace_id_from_url().map_or_else(
        || base.to_string(),
        |id| format!("{base}?workspace_id={id}"),
    )
}

pub(crate) fn bootstrap_url() -> String {
    workspace_scoped_url("/api/bootstrap")
}

pub(crate) fn read_all_notifications_url() -> String {
    workspace_scoped_url("/api/notifications/read-all")
}

pub(crate) fn switch_workspace(workspace_id: &str) {
    if workspace_id.trim().is_empty() {
        return;
    }
    if let Some(window) = web_sys::window() {
        let _ = window
            .location()
            .set_search(&format!("?workspace={workspace_id}"));
    }
}

pub(crate) async fn api_post<B: Serialize, T: DeserializeOwned>(
    url: &str,
    body: &B,
) -> Result<T, ApiError> {
    let response = Request::post(url)
        .credentials(RequestCredentials::SameOrigin)
        .header("x-client-id", &client_id())
        .json(body)
        .map_err(ApiError::network)?
        .send()
        .await
        .map_err(ApiError::network)?;
    decode_response(response).await
}

pub(crate) async fn api_post_form<T: DeserializeOwned>(
    url: &str,
    form: web_sys::FormData,
) -> Result<T, ApiError> {
    // No explicit content type: the browser sets multipart/form-data with the
    // correct boundary itself.
    let response = Request::post(url)
        .credentials(RequestCredentials::SameOrigin)
        .header("x-client-id", &client_id())
        .body(form)
        .map_err(ApiError::network)?
        .send()
        .await
        .map_err(ApiError::network)?;
    decode_response(response).await
}

pub(crate) async fn api_patch<B: Serialize, T: DeserializeOwned>(
    url: &str,
    body: &B,
) -> Result<T, ApiError> {
    let response = Request::patch(url)
        .credentials(RequestCredentials::SameOrigin)
        .header("x-client-id", &client_id())
        .json(body)
        .map_err(ApiError::network)?
        .send()
        .await
        .map_err(ApiError::network)?;
    decode_response(response).await
}

pub(crate) async fn api_empty(url: &str) -> Result<(), ApiError> {
    let response = Request::post(url)
        .credentials(RequestCredentials::SameOrigin)
        .header("x-client-id", &client_id())
        .send()
        .await
        .map_err(ApiError::network)?;
    decode_empty_response(response).await
}

pub(crate) async fn api_delete<T: DeserializeOwned>(url: &str) -> Result<T, ApiError> {
    let response = Request::delete(url)
        .credentials(RequestCredentials::SameOrigin)
        .header("x-client-id", &client_id())
        .send()
        .await
        .map_err(ApiError::network)?;
    decode_response(response).await
}

pub(crate) async fn api_delete_empty(url: &str) -> Result<(), ApiError> {
    let response = Request::delete(url)
        .credentials(RequestCredentials::SameOrigin)
        .header("x-client-id", &client_id())
        .send()
        .await
        .map_err(ApiError::network)?;
    decode_empty_response(response).await
}

pub(crate) async fn decode_response<T: DeserializeOwned>(
    response: gloo_net::http::Response,
) -> Result<T, ApiError> {
    if response.ok() {
        response.json::<T>().await.map_err(ApiError::network)
    } else {
        Err(error_from_body(&response, response.text().await.ok()))
    }
}

pub(crate) async fn decode_empty_response(
    response: gloo_net::http::Response,
) -> Result<(), ApiError> {
    if response.ok() {
        Ok(())
    } else {
        Err(error_from_body(&response, response.text().await.ok()))
    }
}

pub(crate) fn error_from_body(
    response: &gloo_net::http::Response,
    text: Option<String>,
) -> ApiError {
    let text = text.unwrap_or_else(|| "request failed".into());
    ApiError {
        status: response.status(),
        message: serde_json::from_str::<ApiErrorDto>(&text)
            .map(|e| e.error)
            .unwrap_or(text),
    }
}

thread_local! {
    // Random per-tab id. Sent with every mutation (X-Client-Id) and the WS
    // handshake so this tab can skip refetching for its own changes.
    static CLIENT_ID: String = format!(
        "{:08x}{:08x}",
        (js_sys::Math::random() * f64::from(u32::MAX)) as u32,
        (js_sys::Math::random() * f64::from(u32::MAX)) as u32,
    );
    // Used by realtime::schedule_refetch to coalesce bursts of WS events.
    pub(crate) static REFETCH_PENDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    // Wall-clock (ms, js_sys::Date::now) of the last completed bootstrap
    // refetch, so schedule_refetch can throttle a sustained event stream.
    pub(crate) static LAST_REFETCH_AT: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) };
}

pub(crate) fn client_id() -> String {
    CLIENT_ID.with(Clone::clone)
}
