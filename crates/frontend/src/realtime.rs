use crate::*;

/// Context wrapper around the editor-hold counter (see `AppRoot`).
#[derive(Clone, Copy)]
pub(crate) struct RealtimeHold(pub(crate) RwSignal<i32>);

/// Keeps realtime refetches paused while `active()` is true. Call once per
/// component; releases automatically when the component is removed.
pub(crate) fn hold_realtime_while(active: impl Fn() -> bool + 'static) {
    let Some(RealtimeHold(hold)) = use_context::<RealtimeHold>() else {
        return;
    };
    let held = store_value(false);
    create_effect(move |_| {
        let active = active();
        if active && !held.get_value() {
            held.set_value(true);
            hold.update(|v| *v += 1);
        } else if !active && held.get_value() {
            held.set_value(false);
            hold.update(|v| *v -= 1);
        }
    });
    on_cleanup(move || {
        if held.get_value() {
            held.set_value(false);
            hold.update(|v| *v -= 1);
        }
    });
}

/// Leading-edge debounce: events arriving within this window collapse into the
/// one already-scheduled refetch, so a lone edit still lands quickly.
const REFETCH_DEBOUNCE_MS: u32 = 400;
/// Trailing throttle: under a sustained stream (many collaborators editing at
/// once) keep at least this gap between full bootstrap reloads, so the server
/// is not hit with one reload per event per connected client.
const REFETCH_MIN_INTERVAL_MS: f64 = 1_500.0;

/// Coalesces bursts of realtime events into a single background bootstrap
/// refetch, deferred while any editor is open and rate-limited under churn.
pub(crate) fn schedule_refetch(
    data: ReadSignal<Option<BootstrapDto>>,
    hold: RwSignal<i32>,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) {
    if REFETCH_PENDING.with(|p| p.replace(true)) {
        return;
    }
    spawn_local(async move {
        gloo_timers::future::TimeoutFuture::new(REFETCH_DEBOUNCE_MS).await;
        while hold.get_untracked() > 0 {
            gloo_timers::future::TimeoutFuture::new(1_000).await;
        }
        // Only wait out the remainder of the min interval since the last
        // reload; an isolated edit (last reload long ago) is not delayed.
        let elapsed = js_sys::Date::now() - LAST_REFETCH_AT.with(std::cell::Cell::get);
        if elapsed < REFETCH_MIN_INTERVAL_MS {
            let wait = (REFETCH_MIN_INTERVAL_MS - elapsed) as u32;
            gloo_timers::future::TimeoutFuture::new(wait).await;
        }
        REFETCH_PENDING.with(|p| p.set(false));
        if data.get_untracked().is_some() {
            refresh_bootstrap(set_data, set_error).await;
            LAST_REFETCH_AT.with(|t| t.set(js_sys::Date::now()));
        }
    });
}

/// Connects to /api/ws and triggers a debounced refetch whenever another
/// client changes something in the workspace. Reconnects with exponential
/// backoff and stops once the user is logged out.
pub(crate) fn start_realtime(
    data: ReadSignal<Option<BootstrapDto>>,
    hold: RwSignal<i32>,
    running: StoredValue<bool>,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) {
    spawn_local(async move {
        use futures_util::StreamExt;

        let mut backoff_ms = 1_000u32;
        loop {
            if data.try_get_untracked().flatten().is_none() {
                break;
            }
            let Some(url) = websocket_url() else { break };
            let connected_at = js_sys::Date::now();
            if let Ok(mut socket) = gloo_net::websocket::futures::WebSocket::open(&url) {
                while let Some(Ok(message)) = socket.next().await {
                    backoff_ms = 1_000;
                    if let gloo_net::websocket::Message::Text(text) = message {
                        if let Ok(event) = serde_json::from_str::<WorkspaceEventDto>(&text) {
                            if event.client_id.as_deref() == Some(client_id().as_str()) {
                                continue;
                            }
                            schedule_refetch(data, hold, set_data, set_error);
                        }
                    }
                }
            }
            // A connection that lived for a while may have missed events when
            // it dropped; sync up once before reconnecting.
            if js_sys::Date::now() - connected_at > 5_000.0 {
                backoff_ms = 1_000;
                if data.try_get_untracked().flatten().is_some() {
                    schedule_refetch(data, hold, set_data, set_error);
                }
            }
            gloo_timers::future::TimeoutFuture::new(backoff_ms).await;
            backoff_ms = (backoff_ms * 2).min(30_000);
        }
        running.set_value(false);
    });
}

pub(crate) fn websocket_url() -> Option<String> {
    let location = web_sys::window()?.location();
    let protocol = if location.protocol().ok()? == "https:" {
        "wss"
    } else {
        "ws"
    };
    let host = location.host().ok()?;
    Some(format!(
        "{protocol}://{host}/api/ws?client_id={}",
        client_id()
    ))
}
