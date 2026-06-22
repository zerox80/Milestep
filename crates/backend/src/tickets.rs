use crate::*;

pub(crate) async fn list_tickets(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<WorkspaceQuery>,
) -> Result<Json<Vec<TicketDto>>, AppError> {
    let (_, user_id) = require_user(&state, &headers).await?;
    let project_id = active_project_id(&state.db, user_id, query.workspace_uuid()?).await?;
    Ok(Json(fetch_tickets(&state.db, project_id).await?))
}

pub(crate) async fn get_ticket(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<TicketDto>, AppError> {
    let (_, user_id) = require_user(&state, &headers).await?;
    let ticket_id = uuid_from_str(&id)?;
    assert_ticket_read(&state.db, user_id, ticket_id).await?;
    Ok(Json(fetch_ticket(&state.db, ticket_id).await?))
}

pub(crate) async fn create_ticket(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateTicketRequest>,
) -> Result<Json<TicketDto>, AppError> {
    let (ctx, user_id) = require_user(&state, &headers).await?;
    let project_id = uuid_from_str(&payload.project_id)?;
    let workspace_id = assert_project_edit(&state.db, user_id, project_id).await?;

    let title = required_capped(&payload.title, MAX_TITLE_LEN, "ticket title")?;
    let description = optional_capped(&payload.description, MAX_TEXT_LEN, "ticket description")?;
    let requester_name = optional_capped(
        &payload.requester_name,
        MAX_LABEL_LEN,
        "ticket requester name",
    )?;

    let assignee_id = optional_uuid(payload.assignee_id.as_deref())?;

    let mut tx = state.db.begin().await?;
    if let Some(assignee_id) = assignee_id {
        assert_user_in_project(&mut *tx, project_id, assignee_id).await?;
    }

    let key = next_ticket_key(&mut tx, project_id).await?;

    let ticket_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO tickets \
         (id, project_id, key, title, description, status, priority, requester_name, assignee_id, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(ticket_id)
    .bind(project_id)
    .bind(&key)
    .bind(title)
    .bind(description)
    .bind(ticket_status_to_db(&payload.status))
    .bind(priority_to_db(&payload.priority))
    .bind(requester_name)
    .bind(assignee_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    record_audit(
        &mut *tx,
        workspace_id,
        user_id,
        "created ticket",
        "ticket",
        Some(ticket_id),
    )
    .await?;
    tx.commit().await?;
    notify_workspace(&state, &ctx, &headers, workspace_id, "ticket");
    Ok(Json(fetch_ticket(&state.db, ticket_id).await?))
}

pub(crate) async fn update_ticket(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<UpdateTicketRequest>,
) -> Result<Json<TicketDto>, AppError> {
    let (ctx, user_id) = require_user(&state, &headers).await?;
    let ticket_id = uuid_from_str(&id)?;
    let workspace_id = assert_ticket_edit(&state.db, user_id, ticket_id).await?;

    if let Some(title) = &payload.title {
        required_capped(title, MAX_TITLE_LEN, "ticket title")?;
    }

    let mut tx = state.db.begin().await?;
    if let Some(title) = payload.title {
        sqlx::query("UPDATE tickets SET title = $1, updated_at = now() WHERE id = $2")
            .bind(title.trim())
            .bind(ticket_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(description) = payload.description {
        sqlx::query("UPDATE tickets SET description = $1, updated_at = now() WHERE id = $2")
            .bind(optional_capped(
                &description,
                MAX_TEXT_LEN,
                "ticket description",
            )?)
            .bind(ticket_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(status) = payload.status {
        sqlx::query("UPDATE tickets SET status = $1, updated_at = now() WHERE id = $2")
            .bind(ticket_status_to_db(&status))
            .bind(ticket_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(priority) = payload.priority {
        sqlx::query("UPDATE tickets SET priority = $1, updated_at = now() WHERE id = $2")
            .bind(priority_to_db(&priority))
            .bind(ticket_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(requester_name) = payload.requester_name {
        sqlx::query("UPDATE tickets SET requester_name = $1, updated_at = now() WHERE id = $2")
            .bind(optional_capped(
                &requester_name,
                MAX_LABEL_LEN,
                "ticket requester name",
            )?)
            .bind(ticket_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(assignee_id) = payload.assignee_id {
        let assignee_id = optional_uuid(assignee_id.as_deref())?;
        if let Some(assignee_id) = assignee_id {
            let (project_id,): (Uuid,) =
                sqlx::query_as("SELECT project_id FROM tickets WHERE id = $1")
                    .bind(ticket_id)
                    .fetch_one(&mut *tx)
                    .await?;
            assert_user_in_project(&mut *tx, project_id, assignee_id).await?;
        }
        sqlx::query("UPDATE tickets SET assignee_id = $1, updated_at = now() WHERE id = $2")
            .bind(assignee_id)
            .bind(ticket_id)
            .execute(&mut *tx)
            .await?;
    }

    record_audit(
        &mut *tx,
        workspace_id,
        user_id,
        "updated ticket",
        "ticket",
        Some(ticket_id),
    )
    .await?;
    tx.commit().await?;
    notify_workspace(&state, &ctx, &headers, workspace_id, "ticket");
    Ok(Json(fetch_ticket(&state.db, ticket_id).await?))
}

pub(crate) async fn delete_ticket(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let (ctx, user_id) = require_user(&state, &headers).await?;
    let ticket_id = uuid_from_str(&id)?;
    let workspace_id = assert_ticket_edit(&state.db, user_id, ticket_id).await?;
    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM tickets WHERE id = $1")
        .bind(ticket_id)
        .execute(&mut *tx)
        .await?;
    record_audit(
        &mut *tx,
        workspace_id,
        user_id,
        "deleted ticket",
        "ticket",
        Some(ticket_id),
    )
    .await?;
    tx.commit().await?;
    notify_workspace(&state, &ctx, &headers, workspace_id, "ticket");
    Ok(StatusCode::NO_CONTENT)
}

async fn next_ticket_key(conn: &mut PgConnection, project_id: Uuid) -> Result<String, AppError> {
    // Serializes key generation per project so concurrent creates cannot collide.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(format!("tickets:{project_id}"))
        .execute(&mut *conn)
        .await?;
    let (project_key,): (String,) = sqlx::query_as("SELECT key FROM projects WHERE id = $1")
        .bind(project_id)
        .fetch_one(&mut *conn)
        .await?;
    let next: (i32,) = sqlx::query_as(
        "SELECT COALESCE(MAX(split_part(key, '-', 3)::INT), 0) + 1 \
         FROM tickets WHERE project_id = $1 AND key LIKE $2 || '-T-%' \
         AND split_part(key, '-', 3) ~ '^[0-9]+$'",
    )
    .bind(project_id)
    .bind(&project_key)
    .fetch_one(&mut *conn)
    .await?;
    Ok(format!("{}-T-{}", project_key, next.0))
}
