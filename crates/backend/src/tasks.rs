use crate::*;

pub(crate) async fn list_tasks(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<WorkspaceQuery>,
) -> Result<Json<Vec<TaskDto>>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let project_id = active_project_id(
        &state.db,
        uuid_from_str(&ctx.user.id)?,
        query.workspace_uuid()?,
    )
    .await?;
    Ok(Json(fetch_tasks(&state.db, project_id).await?))
}

pub(crate) async fn get_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<TaskDto>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let task_id = uuid_from_str(&id)?;
    assert_task_read(&state.db, uuid_from_str(&ctx.user.id)?, task_id).await?;
    Ok(Json(fetch_task(&state.db, task_id).await?))
}

pub(crate) async fn create_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateTaskRequest>,
) -> Result<Json<TaskDto>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    let project_id = uuid_from_str(&payload.project_id)?;
    let workspace_id = assert_project_edit(&state.db, user_id, project_id).await?;

    if payload.title.trim().is_empty() {
        return Err(AppError::BadRequest("task title is required".into()));
    }

    let status_id = uuid_from_str(&payload.status_id)?;
    assert_status_in_project(&state.db, project_id, status_id).await?;

    let mut tx = state.db.begin().await?;
    // Serializes key generation per project so concurrent creates cannot collide.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(project_id.to_string())
        .execute(&mut *tx)
        .await?;
    let (project_key,): (String,) = sqlx::query_as("SELECT key FROM projects WHERE id = $1")
        .bind(project_id)
        .fetch_one(&mut *tx)
        .await?;
    let next: (i32,) = sqlx::query_as(
        "SELECT COALESCE(MAX(split_part(key, '-', 2)::INT), 100) + 1 \
         FROM tasks WHERE project_id = $1 AND key LIKE $2 || '-%' \
         AND split_part(key, '-', 2) ~ '^[0-9]+$'",
    )
    .bind(project_id)
    .bind(&project_key)
    .fetch_one(&mut *tx)
    .await?;
    let key = format!("{}-{}", project_key, next.0);

    let task_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO tasks \
         (id, project_id, key, title, description, tag, tag_color, priority, status_id, start_date, due_date, phase, recurrence, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
    )
    .bind(task_id)
    .bind(project_id)
    .bind(&key)
    .bind(payload.title.trim())
    .bind(payload.description.trim())
    .bind(payload.tag.trim())
    .bind(payload.tag_color.trim())
    .bind(priority_to_db(&payload.priority))
    .bind(status_id)
    .bind(parse_optional_date(payload.start_date.as_deref())?)
    .bind(parse_optional_date(payload.due_date.as_deref())?)
    .bind(payload.phase.trim())
    .bind(payload.recurrence.map(recurrence_to_db))
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    replace_assignees(&mut tx, task_id, &payload.assignee_ids).await?;
    for (idx, title) in payload.subtasks.iter().enumerate() {
        if !title.trim().is_empty() {
            sqlx::query(
                "INSERT INTO subtasks (id, task_id, title, position) VALUES ($1, $2, $3, $4)",
            )
            .bind(Uuid::new_v4())
            .bind(task_id)
            .bind(title.trim())
            .bind(idx as i32)
            .execute(&mut *tx)
            .await?;
        }
    }

    record_audit(
        &mut *tx,
        workspace_id,
        user_id,
        "created task",
        "task",
        Some(task_id),
    )
    .await?;
    tx.commit().await?;
    notify_workspace(&state, &ctx, &headers, workspace_id, "task");
    Ok(Json(fetch_task(&state.db, task_id).await?))
}

pub(crate) async fn update_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<UpdateTaskRequest>,
) -> Result<Json<TaskDto>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    let task_id = uuid_from_str(&id)?;
    let workspace_id = assert_task_edit(&state.db, user_id, task_id).await?;

    let mut tx = state.db.begin().await?;
    if let Some(title) = payload.title {
        if title.trim().is_empty() {
            return Err(AppError::BadRequest("task title is required".into()));
        }
        sqlx::query("UPDATE tasks SET title = $1, updated_at = now() WHERE id = $2")
            .bind(title.trim())
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(description) = payload.description {
        sqlx::query("UPDATE tasks SET description = $1, updated_at = now() WHERE id = $2")
            .bind(description.trim())
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(tag) = payload.tag {
        sqlx::query("UPDATE tasks SET tag = $1, updated_at = now() WHERE id = $2")
            .bind(tag.trim())
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(tag_color) = payload.tag_color {
        sqlx::query("UPDATE tasks SET tag_color = $1, updated_at = now() WHERE id = $2")
            .bind(tag_color.trim())
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(priority) = payload.priority {
        sqlx::query("UPDATE tasks SET priority = $1, updated_at = now() WHERE id = $2")
            .bind(priority_to_db(&priority))
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
    }
    let mut was_done_before_status_change: Option<bool> = None;
    if let Some(status_id) = payload.status_id {
        let status_id = uuid_from_str(&status_id)?;
        let project_id: (Uuid,) = sqlx::query_as("SELECT project_id FROM tasks WHERE id = $1")
            .bind(task_id)
            .fetch_one(&mut *tx)
            .await?;
        assert_status_in_project(&mut *tx, project_id.0, status_id).await?;
        was_done_before_status_change = Some(task_status_is_done(&mut *tx, task_id).await?);
        sqlx::query("UPDATE tasks SET status_id = $1, updated_at = now() WHERE id = $2")
            .bind(status_id)
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(start_date) = payload.start_date {
        sqlx::query("UPDATE tasks SET start_date = $1, updated_at = now() WHERE id = $2")
            .bind(parse_optional_date(start_date.as_deref())?)
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(due_date) = payload.due_date {
        sqlx::query("UPDATE tasks SET due_date = $1, updated_at = now() WHERE id = $2")
            .bind(parse_optional_date(due_date.as_deref())?)
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(phase) = payload.phase {
        sqlx::query("UPDATE tasks SET phase = $1, updated_at = now() WHERE id = $2")
            .bind(phase.trim())
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(recurrence) = payload.recurrence {
        sqlx::query("UPDATE tasks SET recurrence = $1, updated_at = now() WHERE id = $2")
            .bind(recurrence.map(recurrence_to_db))
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(assignee_ids) = payload.assignee_ids {
        replace_assignees(&mut tx, task_id, &assignee_ids).await?;
    }

    // After all field updates so a recurrence change in the same PATCH counts.
    let spawned_follow_up = if let Some(was_done) = was_done_before_status_change {
        spawn_recurrence_if_completed(&mut tx, task_id, was_done)
            .await?
            .is_some()
    } else {
        false
    };

    record_audit(
        &mut *tx,
        workspace_id,
        user_id,
        "updated task",
        "task",
        Some(task_id),
    )
    .await?;
    tx.commit().await?;
    notify_workspace(&state, &ctx, &headers, workspace_id, "task");
    if spawned_follow_up {
        // Without a client id even the originating tab refetches and sees the
        // spawned follow-up task.
        notify_workspace_all(&state, workspace_id, "task");
    }
    Ok(Json(fetch_task(&state.db, task_id).await?))
}

pub(crate) async fn move_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<MoveTaskRequest>,
) -> Result<Json<TaskDto>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    let task_id = uuid_from_str(&id)?;
    let workspace_id = assert_task_edit(&state.db, user_id, task_id).await?;
    let status_id = uuid_from_str(&payload.status_id)?;
    let mut tx = state.db.begin().await?;
    let project_id: (Uuid,) = sqlx::query_as("SELECT project_id FROM tasks WHERE id = $1")
        .bind(task_id)
        .fetch_one(&mut *tx)
        .await?;
    assert_status_in_project(&mut *tx, project_id.0, status_id).await?;
    let was_done = task_status_is_done(&mut *tx, task_id).await?;

    sqlx::query("UPDATE tasks SET status_id = $1, updated_at = now() WHERE id = $2")
        .bind(status_id)
        .bind(task_id)
        .execute(&mut *tx)
        .await?;
    let spawned_follow_up = spawn_recurrence_if_completed(&mut tx, task_id, was_done)
        .await?
        .is_some();
    record_audit(
        &mut *tx,
        workspace_id,
        user_id,
        "moved task",
        "task",
        Some(task_id),
    )
    .await?;
    tx.commit().await?;
    notify_workspace(&state, &ctx, &headers, workspace_id, "task");
    if spawned_follow_up {
        // Without a client id even the originating tab refetches and sees the
        // spawned follow-up task (drag&drop only patches the moved task locally).
        notify_workspace_all(&state, workspace_id, "task");
    }
    Ok(Json(fetch_task(&state.db, task_id).await?))
}

pub(crate) async fn delete_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    let task_id = uuid_from_str(&id)?;
    let workspace_id = assert_task_edit(&state.db, user_id, task_id).await?;
    let mut tx = state.db.begin().await?;
    // Capture file paths before the cascade deletes the attachment rows, so
    // the files can be removed from disk and don't leak storage forever.
    let storage_paths: Vec<(String,)> =
        sqlx::query_as("SELECT storage_path FROM attachments WHERE task_id = $1")
            .bind(task_id)
            .fetch_all(&mut *tx)
            .await?;
    sqlx::query("DELETE FROM tasks WHERE id = $1")
        .bind(task_id)
        .execute(&mut *tx)
        .await?;
    record_audit(
        &mut *tx,
        workspace_id,
        user_id,
        "deleted task",
        "task",
        Some(task_id),
    )
    .await?;
    tx.commit().await?;
    notify_workspace(&state, &ctx, &headers, workspace_id, "task");
    for (path,) in storage_paths {
        if let Err(err) = fs::remove_file(&path).await {
            tracing::warn!(%path, %err, "could not remove attachment file of deleted task");
        }
    }
    Ok(StatusCode::NO_CONTENT)
}
