use crate::*;

pub(crate) fn build_router(state: AppState) -> Router {
    let auth_rate_limit = axum::middleware::from_fn_with_state(state.clone(), rate_limit_auth);
    let api = Router::new()
        .route("/health", get(health))
        .route(
            "/auth/register",
            post(register).route_layer(auth_rate_limit.clone()),
        )
        .route("/auth/login", post(login).route_layer(auth_rate_limit))
        .route("/auth/logout", post(logout))
        .route("/auth/logout-all", post(logout_all))
        .route("/auth/me", get(me))
        .route("/bootstrap", get(bootstrap))
        .route("/ws", get(ws_handler))
        .route("/tasks", get(list_tasks).post(create_task))
        .route("/tickets", get(list_tickets).post(create_ticket))
        .route("/milestones", get(list_milestones).post(create_milestone))
        .route("/milestones/{id}", delete(delete_milestone))
        .route(
            "/tickets/{id}",
            get(get_ticket).patch(update_ticket).delete(delete_ticket),
        )
        .route(
            "/tasks/{id}",
            get(get_task).patch(update_task).delete(delete_task),
        )
        .route("/tasks/{id}/move", post(move_task))
        .route("/tasks/{id}/subtasks", post(create_subtask))
        .route(
            "/tasks/{id}/subtasks/{subtask_id}",
            patch(update_subtask).delete(delete_subtask),
        )
        .route("/tasks/{id}/comments", post(create_comment))
        .route(
            "/tasks/{id}/attachments",
            post(upload_attachment).route_layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES)),
        )
        .route(
            "/attachments/{id}",
            get(download_attachment).delete(delete_attachment),
        )
        .route("/notifications/{id}/read", post(read_notification))
        .route("/notifications/read-all", post(read_all_notifications))
        .route("/workspaces/{id}", patch(update_workspace))
        .route("/workspaces/{id}/invites", post(invite_member))
        .route(
            "/memberships/{id}",
            patch(update_membership).delete(remove_membership),
        )
        .layer(DefaultBodyLimit::max(MAX_JSON_BODY_BYTES))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            enforce_same_origin,
        ))
        .with_state(state.clone());

    let index = state.cfg.static_dir.join("index.html");
    let spa = ServeDir::new(&state.cfg.static_dir).not_found_service(ServeFile::new(index));

    Router::new()
        .nest("/api", api)
        .fallback_service(spa)
        .layer(axum::middleware::from_fn(security_headers))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
}
