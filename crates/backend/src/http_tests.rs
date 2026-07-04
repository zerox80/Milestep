//! Handler-level HTTP tests: drive the real router (middleware chain, JSON
//! extractors, cookies) with `tower::ServiceExt::oneshot` against a real
//! Postgres. They complement `db_tests.rs`, which exercises the access-control
//! layer directly.
//!
//! Like `db_tests.rs` they skip gracefully when `DATABASE_URL` is unset and
//! do not clean up after themselves — point the variable at a disposable
//! database only (CI uses an ephemeral `kowobau_test`).

use crate::*;
use serde::de::DeserializeOwned;
use tower::util::ServiceExt;

/// Connects, migrates and builds an `AppState` with test config. Returns
/// `None` only when `DATABASE_URL` is unset; a set-but-broken URL panics
/// loudly instead of silently skipping (same contract as `db_tests`).
async fn test_state() -> Option<AppState> {
    let Ok(url) = env::var("DATABASE_URL") else {
        return None;
    };
    let db = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("DATABASE_URL is set but the database connection failed");
    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .expect("migrations failed to run on the test database");
    let cfg = AppConfig {
        bind: "127.0.0.1:0".into(),
        static_dir: PathBuf::from("."),
        upload_dir: env::temp_dir().join("kowobau-http-tests"),
        session_secret: "http-test-secret-with-at-least-32-chars".into(),
        cookie_secure: false,
        seed_demo: false,
        registration_enabled: true,
        max_workspace_storage_bytes: MAX_WORKSPACE_STORAGE_BYTES,
        trust_proxy: false,
        trusted_proxies: default_trusted_proxies(),
        public_origin: None,
    };
    Some(build_state(db, cfg))
}

async fn send(state: &AppState, req: Request) -> Response {
    build_router(state.clone())
        .oneshot(req)
        .await
        .expect("router is infallible")
}

/// The auth rate limiter reads the peer address from the `ConnectInfo`
/// extension; `oneshot` bypasses a real listener, so it is injected here.
fn connect_info() -> ConnectInfo<SocketAddr> {
    ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 39999)))
}

fn json_request<B: Serialize>(
    method: Method,
    uri: &str,
    body: &B,
    cookie: Option<&str>,
) -> Request {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(CONTENT_TYPE, "application/json")
        .extension(connect_info());
    if let Some(cookie) = cookie {
        builder = builder.header(COOKIE, cookie);
    }
    builder
        .body(Body::from(
            serde_json::to_vec(body).expect("body serializes"),
        ))
        .expect("request builds")
}

fn get_request(uri: &str, cookie: Option<&str>) -> Request {
    let mut builder = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .extension(connect_info());
    if let Some(cookie) = cookie {
        builder = builder.header(COOKIE, cookie);
    }
    builder.body(Body::empty()).expect("request builds")
}

/// The `name=value` pair of the session cookie a response set.
fn session_cookie(res: &Response) -> String {
    res.headers()
        .get(SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .expect("response sets a session cookie")
        .to_string()
}

async fn body_json<T: DeserializeOwned>(res: Response) -> T {
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .expect("body reads");
    serde_json::from_slice(&bytes).expect("body decodes")
}

/// Registers a fresh user and returns its session cookie and email.
async fn register_user(state: &AppState) -> (String, String) {
    let email = format!("{}@http.test", Uuid::new_v4());
    let res = send(
        state,
        json_request(
            Method::POST,
            "/api/auth/register",
            &json!({ "name": "Http Tester", "email": email, "password": "password-123" }),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let cookie = session_cookie(&res);
    (cookie, email)
}

async fn audit_count(db: &PgPool, workspace_id: Uuid) -> i64 {
    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM audit_events WHERE workspace_id = $1")
            .bind(workspace_id)
            .fetch_one(db)
            .await
            .expect("audit count");
    count
}

#[tokio::test]
async fn register_sets_a_session_and_login_works() {
    let Some(state) = test_state().await else {
        return;
    };
    let (cookie, email) = register_user(&state).await;

    let res = send(&state, get_request("/api/auth/me", Some(&cookie))).await;
    assert_eq!(res.status(), StatusCode::OK);
    let me: AuthResponse = body_json(res).await;
    assert_eq!(me.user.email, email);

    let res = send(
        &state,
        json_request(
            Method::POST,
            "/api/auth/login",
            &json!({ "email": email, "password": "password-123" }),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(res.headers().contains_key(SET_COOKIE));
}

#[tokio::test]
async fn wrong_password_and_missing_session_are_unauthorized() {
    let Some(state) = test_state().await else {
        return;
    };
    let (_, email) = register_user(&state).await;

    let res = send(
        &state,
        json_request(
            Method::POST,
            "/api/auth/login",
            &json!({ "email": email, "password": "wrong-password" }),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    let res = send(&state, get_request("/api/bootstrap", None)).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn cross_origin_mutations_are_forbidden() {
    let Some(state) = test_state().await else {
        return;
    };
    let (cookie, _) = register_user(&state).await;

    let mut req = json_request(Method::POST, "/api/tasks", &json!({}), Some(&cookie));
    req.headers_mut()
        .insert(ORIGIN, HeaderValue::from_static("https://evil.test"));
    req.headers_mut()
        .insert(HOST, HeaderValue::from_static("kowobau.test"));
    let res = send(&state, req).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn empty_task_patch_is_a_noop_and_dates_clear_via_null() {
    let Some(state) = test_state().await else {
        return;
    };
    let (cookie, _) = register_user(&state).await;

    let res = send(&state, get_request("/api/bootstrap", Some(&cookie))).await;
    assert_eq!(res.status(), StatusCode::OK);
    let boot: BootstrapDto = body_json(res).await;
    let workspace_id = uuid_from_str(&boot.workspace.id).expect("workspace id");

    let res = send(
        &state,
        json_request(
            Method::POST,
            "/api/tasks",
            &json!({
                "project_id": boot.project.id,
                "title": "Putz prüfen",
                "description": "",
                "tag": "",
                "tag_color": "",
                "priority": "medium",
                "status_id": boot.statuses[0].id,
                "start_date": null,
                "due_date": null,
                "phase": "rohbau",
                "assignee_ids": [],
                "subtasks": [],
            }),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let task: TaskDto = body_json(res).await;
    let task_uri = format!("/api/tasks/{}", task.id);
    let before = audit_count(&state.db, workspace_id).await;

    // An empty PATCH changes nothing and must not create an audit entry.
    let res = send(
        &state,
        json_request(Method::PATCH, &task_uri, &json!({}), Some(&cookie)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let unchanged: TaskDto = body_json(res).await;
    assert_eq!(unchanged.title, "Putz prüfen");
    assert_eq!(audit_count(&state.db, workspace_id).await, before);

    // A real change lands, is audited, and an explicit null clears a date
    // (double_option semantics through the consolidated UPDATE).
    let res = send(
        &state,
        json_request(
            Method::PATCH,
            &task_uri,
            &json!({ "title": "Umbenannt", "due_date": "2026-07-10" }),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let updated: TaskDto = body_json(res).await;
    assert_eq!(updated.title, "Umbenannt");
    assert_eq!(updated.due_date.as_deref(), Some("2026-07-10"));
    assert_eq!(audit_count(&state.db, workspace_id).await, before + 1);

    let res = send(
        &state,
        json_request(
            Method::PATCH,
            &task_uri,
            &json!({ "due_date": null }),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let cleared: TaskDto = body_json(res).await;
    assert_eq!(cleared.due_date, None);
    assert_eq!(audit_count(&state.db, workspace_id).await, before + 2);
}

#[tokio::test]
async fn empty_ticket_patch_is_a_noop() {
    let Some(state) = test_state().await else {
        return;
    };
    let (cookie, _) = register_user(&state).await;

    let res = send(&state, get_request("/api/bootstrap", Some(&cookie))).await;
    assert_eq!(res.status(), StatusCode::OK);
    let boot: BootstrapDto = body_json(res).await;
    let workspace_id = uuid_from_str(&boot.workspace.id).expect("workspace id");

    let res = send(
        &state,
        json_request(
            Method::POST,
            "/api/tickets",
            &json!({
                "project_id": boot.project.id,
                "title": "Fenster klemmt",
                "description": "",
                "status": "open",
                "priority": "medium",
                "requester_name": "",
                "assignee_id": null,
            }),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let ticket: TicketDto = body_json(res).await;
    let ticket_uri = format!("/api/tickets/{}", ticket.id);
    let before = audit_count(&state.db, workspace_id).await;

    let res = send(
        &state,
        json_request(Method::PATCH, &ticket_uri, &json!({}), Some(&cookie)),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(audit_count(&state.db, workspace_id).await, before);

    let res = send(
        &state,
        json_request(
            Method::PATCH,
            &ticket_uri,
            &json!({ "status": "resolved" }),
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let updated: TicketDto = body_json(res).await;
    assert_eq!(updated.status, TicketStatus::Resolved);
    assert_eq!(audit_count(&state.db, workspace_id).await, before + 1);
}
