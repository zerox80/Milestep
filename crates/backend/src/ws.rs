use crate::*;

pub(crate) async fn read_notification(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let (_, user_id) = require_user(&state, &headers).await?;
    let notification_id = uuid_from_str(&id)?;
    sqlx::query("UPDATE notifications SET unread = false WHERE id = $1 AND user_id = $2")
        .bind(notification_id)
        .bind(user_id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn read_all_notifications(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<WorkspaceQuery>,
) -> Result<StatusCode, AppError> {
    let (_, user_id) = require_user(&state, &headers).await?;
    let (workspace_id,): (Uuid,) = sqlx::query_as(
        "SELECT workspace_id FROM memberships \
         WHERE user_id = $1 AND status = 'active' \
         AND ($2::uuid IS NULL OR workspace_id = $2) \
         ORDER BY created_at ASC LIMIT 1",
    )
    .bind(user_id)
    .bind(query.workspace_uuid()?)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::Forbidden)?;
    sqlx::query("UPDATE notifications SET unread = false WHERE user_id = $1 AND workspace_id = $2")
        .bind(user_id)
        .bind(workspace_id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Fire-and-forget realtime fanout to the workspace. The originating tab
/// identifies itself via the X-Client-Id header so it can skip refetching its
/// own (already locally applied) mutation. `send()` only errs when no socket is
/// connected, which is fine to ignore.
pub(crate) fn notify_workspace(
    state: &AppState,
    ctx: &AuthContext,
    headers: &HeaderMap,
    workspace_id: Uuid,
    topic: &str,
) {
    // The id is namespaced with the server-side session id, so one session
    // cannot replay another tab's id to suppress its realtime refetches.
    let client_id = headers
        .get("x-client-id")
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.is_empty() && v.len() <= 64)
        .map(|v| format!("{}:{v}", ctx.session_id));
    let _ = state.events.send(WorkspaceEventDto {
        workspace_id: workspace_id.to_string(),
        topic: topic.to_string(),
        client_id,
    });
}

/// Like `notify_workspace`, but without echo suppression: every tab refetches,
/// including the one that caused the change (used when the server created
/// additional data the originator does not know about, e.g. a recurring
/// follow-up task).
pub(crate) fn notify_workspace_all(state: &AppState, workspace_id: Uuid, topic: &str) {
    let _ = state.events.send(WorkspaceEventDto {
        workspace_id: workspace_id.to_string(),
        topic: topic.to_string(),
        client_id: None,
    });
}

#[derive(Debug, Deserialize)]
pub(crate) struct WsQuery {
    #[serde(default)]
    pub(crate) client_id: Option<String>,
    #[serde(default)]
    pub(crate) workspace_id: Option<String>,
}

// The slot-reservation lock is already confined to the smallest sensible block;
// the lint's suggested restructuring cannot chain the entry/insert through it.
#[allow(clippy::significant_drop_tightening)]
pub(crate) async fn ws_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, AppError> {
    // The handshake is a GET, so enforce_same_origin skips it — but browsers
    // do not apply CORS to WebSockets, so a foreign page could otherwise open
    // a cookie-authenticated socket (cross-site WebSocket hijacking).
    if !same_origin(&state.cfg, &headers) {
        return Err(AppError::Forbidden);
    }
    let (ctx, user_id) = require_user(&state, &headers).await?;
    let selected_workspace = query
        .workspace_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
        .map(uuid_from_str)
        .transpose()?;
    // Same scoping rule as fetch_bootstrap: an explicit workspace may be
    // selected, otherwise the first active membership is used.
    let membership: Option<(Uuid,)> = sqlx::query_as(
        "SELECT workspace_id FROM memberships \
         WHERE user_id = $1 AND status = 'active' \
         AND ($2::uuid IS NULL OR workspace_id = $2) \
         ORDER BY created_at ASC LIMIT 1",
    )
    .bind(user_id)
    .bind(selected_workspace)
    .fetch_optional(&state.db)
    .await?;
    let (workspace_id,) = membership.ok_or(AppError::Forbidden)?;

    // Reserve a connection slot before upgrading so an over-limit client gets a
    // plain HTTP error instead of a socket. The guard releases the slot on every
    // exit path (including a failed upgrade) via Drop.
    {
        let mut conns = state.ws_conns.lock().expect("ws_conns lock poisoned");
        let entry = conns.entry(user_id).or_insert(0);
        if *entry >= MAX_WS_CONNECTIONS_PER_USER {
            return Err(AppError::TooManyRequests);
        }
        *entry += 1;
    }
    let guard = WsConnGuard {
        conns: state.ws_conns.clone(),
        user_id,
    };

    // Same session-id namespacing as notify_workspace; the raw client id is
    // useless to anyone without this connection's session cookie.
    let client_id = query
        .client_id
        .filter(|v| !v.is_empty() && v.len() <= 64)
        .map(|v| format!("{}:{v}", ctx.session_id));
    Ok(ws.on_upgrade(move |socket| ws_loop(socket, state, workspace_id, client_id, guard)))
}

/// Releases this connection's per-user socket slot when dropped, so the count
/// stays correct even if the upgrade future is dropped before `ws_loop` ends.
pub(crate) struct WsConnGuard {
    conns: Arc<Mutex<HashMap<Uuid, usize>>>,
    user_id: Uuid,
}

impl Drop for WsConnGuard {
    fn drop(&mut self) {
        let mut conns = self.conns.lock().expect("ws_conns lock poisoned");
        if let Some(count) = conns.get_mut(&self.user_id) {
            *count -= 1;
            if *count == 0 {
                conns.remove(&self.user_id);
            }
        }
    }
}

pub(crate) async fn ws_loop(
    socket: WebSocket,
    state: AppState,
    workspace_id: Uuid,
    client_id: Option<String>,
    // Held for the lifetime of the connection; releases the per-user slot on drop.
    _guard: WsConnGuard,
) {
    use futures_util::{SinkExt, StreamExt};

    let (mut sink, mut stream) = socket.split();
    let mut events = state.events.subscribe();
    let workspace = workspace_id.to_string();
    // Keeps the connection alive through nginx's proxy_read_timeout.
    let mut ping = tokio::time::interval(StdDuration::from_secs(30));
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ping.tick().await; // the first tick fires immediately

    loop {
        tokio::select! {
            _ = ping.tick() => {
                if sink.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
            incoming = stream.next() => {
                match incoming {
                    None | Some(Err(_) | Ok(Message::Close(_))) => break,
                    Some(Ok(_)) => {}
                }
            }
            event = events.recv() => {
                let event = match event {
                    Ok(event) => {
                        if event.workspace_id != workspace {
                            continue;
                        }
                        // Skip the echo of this tab's own mutation.
                        if event.client_id.is_some() && event.client_id == client_id {
                            continue;
                        }
                        event
                    }
                    // This receiver fell behind and missed events; tell the
                    // client to refetch instead of dropping the connection.
                    Err(broadcast::error::RecvError::Lagged(_)) => WorkspaceEventDto {
                        workspace_id: workspace.clone(),
                        topic: "resync".to_string(),
                        client_id: None,
                    },
                    Err(broadcast::error::RecvError::Closed) => break,
                };
                let Ok(json) = serde_json::to_string(&event) else {
                    continue;
                };
                if sink.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        }
    }
}
