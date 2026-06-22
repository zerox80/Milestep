use crate::*;

pub(crate) async fn create_subtask(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<CreateSubtaskRequest>,
) -> Result<Json<TaskDto>, AppError> {
    let (ctx, user_id) = require_user(&state, &headers).await?;
    let task_id = uuid_from_str(&id)?;
    let workspace_id = assert_task_edit(&state.db, user_id, task_id).await?;
    let title = required_capped(&payload.title, MAX_TITLE_LEN, "subtask title")?;
    let mut tx = state.db.begin().await?;
    // Serialize position generation per task so concurrent creates cannot
    // assign the same position.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(format!("subtasks:{task_id}"))
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO subtasks (id, task_id, title, position) \
         SELECT $1, $2, $3, COALESCE(MAX(position), -1) + 1 FROM subtasks WHERE task_id = $2",
    )
    .bind(Uuid::new_v4())
    .bind(task_id)
    .bind(title)
    .execute(&mut *tx)
    .await?;
    touch_task(&mut *tx, task_id).await?;
    record_audit(
        &mut *tx,
        workspace_id,
        user_id,
        "created subtask",
        "subtask",
        Some(task_id),
    )
    .await?;
    tx.commit().await?;
    notify_workspace(&state, &ctx, &headers, workspace_id, "task");
    Ok(Json(fetch_task(&state.db, task_id).await?))
}

pub(crate) async fn update_subtask(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, subtask_id)): Path<(String, String)>,
    Json(payload): Json<UpdateSubtaskRequest>,
) -> Result<Json<TaskDto>, AppError> {
    let (ctx, user_id) = require_user(&state, &headers).await?;
    let task_id = uuid_from_str(&id)?;
    let subtask_id = uuid_from_str(&subtask_id)?;
    let workspace_id = assert_task_edit(&state.db, user_id, task_id).await?;

    if let Some(title) = &payload.title {
        required_capped(title, MAX_TITLE_LEN, "subtask title")?;
    }
    let mut tx = state.db.begin().await?;
    let exists: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM subtasks WHERE id = $1 AND task_id = $2")
            .bind(subtask_id)
            .bind(task_id)
            .fetch_optional(&mut *tx)
            .await?;
    if exists.is_none() {
        return Err(AppError::NotFound);
    }
    if let Some(title) = payload.title {
        sqlx::query("UPDATE subtasks SET title = $1 WHERE id = $2 AND task_id = $3")
            .bind(title.trim())
            .bind(subtask_id)
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(done) = payload.done {
        sqlx::query("UPDATE subtasks SET done = $1 WHERE id = $2 AND task_id = $3")
            .bind(done)
            .bind(subtask_id)
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
    }
    touch_task(&mut *tx, task_id).await?;
    record_audit(
        &mut *tx,
        workspace_id,
        user_id,
        "updated subtask",
        "subtask",
        Some(subtask_id),
    )
    .await?;
    tx.commit().await?;
    notify_workspace(&state, &ctx, &headers, workspace_id, "task");
    Ok(Json(fetch_task(&state.db, task_id).await?))
}

pub(crate) async fn delete_subtask(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, subtask_id)): Path<(String, String)>,
) -> Result<Json<TaskDto>, AppError> {
    let (ctx, user_id) = require_user(&state, &headers).await?;
    let task_id = uuid_from_str(&id)?;
    let subtask_id = uuid_from_str(&subtask_id)?;
    let workspace_id = assert_task_edit(&state.db, user_id, task_id).await?;
    let mut tx = state.db.begin().await?;
    let deleted = sqlx::query("DELETE FROM subtasks WHERE id = $1 AND task_id = $2")
        .bind(subtask_id)
        .bind(task_id)
        .execute(&mut *tx)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    touch_task(&mut *tx, task_id).await?;
    record_audit(
        &mut *tx,
        workspace_id,
        user_id,
        "deleted subtask",
        "subtask",
        Some(subtask_id),
    )
    .await?;
    tx.commit().await?;
    notify_workspace(&state, &ctx, &headers, workspace_id, "task");
    Ok(Json(fetch_task(&state.db, task_id).await?))
}

pub(crate) async fn create_comment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<CreateCommentRequest>,
) -> Result<Json<TaskDto>, AppError> {
    let (ctx, user_id) = require_user(&state, &headers).await?;
    let task_id = uuid_from_str(&id)?;
    // Commenting is intentionally open to viewers; only read access is required.
    let workspace_id = assert_task_read(&state.db, user_id, task_id).await?;
    let body = required_capped(&payload.body, MAX_COMMENT_LEN, "comment body")?.to_string();
    let mut tx = state.db.begin().await?;
    sqlx::query("INSERT INTO comments (id, task_id, user_id, body) VALUES ($1, $2, $3, $4)")
        .bind(Uuid::new_v4())
        .bind(task_id)
        .bind(user_id)
        .bind(&body)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE tasks SET comments_count = comments_count + 1, updated_at = now() WHERE id = $1",
    )
    .bind(task_id)
    .execute(&mut *tx)
    .await?;

    let (task_key,): (String,) = sqlx::query_as("SELECT key FROM tasks WHERE id = $1")
        .bind(task_id)
        .fetch_one(&mut *tx)
        .await?;
    let members: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT m.user_id, u.name FROM memberships m JOIN users u ON u.id = m.user_id \
         WHERE m.workspace_id = $1 AND m.status = 'active'",
    )
    .bind(workspace_id)
    .fetch_all(&mut *tx)
    .await?;
    let mentioned = mentioned_user_ids(&body, &members);
    for &target in mentioned.iter().filter(|&&id| id != user_id) {
        insert_notification(
            &mut *tx,
            workspace_id,
            target,
            &NotificationKind::Mention,
            user_id,
            Some(task_id),
            None,
            &format!("hat dich in {task_key} erwähnt"),
            &format!("mentioned you in {task_key}"),
        )
        .await?;
    }
    // Everyone else involved with the task (assignees and earlier commenters)
    // gets a plain comment notification instead.
    let participants: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT user_id FROM task_assignees WHERE task_id = $1 \
         UNION SELECT user_id FROM comments WHERE task_id = $1",
    )
    .bind(task_id)
    .fetch_all(&mut *tx)
    .await?;
    for (target,) in participants {
        if target == user_id || mentioned.contains(&target) {
            continue;
        }
        // Old assignees/commenters may have left the workspace since;
        // `members` holds the active memberships.
        if !members.iter().any(|(id, _)| *id == target) {
            continue;
        }
        insert_notification(
            &mut *tx,
            workspace_id,
            target,
            &NotificationKind::Comment,
            user_id,
            Some(task_id),
            None,
            &format!("hat {task_key} kommentiert"),
            &format!("commented on {task_key}"),
        )
        .await?;
    }

    record_audit(
        &mut *tx,
        workspace_id,
        user_id,
        "commented",
        "task",
        Some(task_id),
    )
    .await?;
    tx.commit().await?;
    notify_workspace(&state, &ctx, &headers, workspace_id, "comment");
    Ok(Json(fetch_task(&state.db, task_id).await?))
}

/// User ids of members whose exact name appears as "@Name" in `body`,
/// followed by a non-alphanumeric boundary (or end of input). Matching is
/// case-sensitive against the canonical member names the autocomplete
/// inserts, checked longest-first so "@Anna Schmidt" wins over a member
/// literally named "Anna".
pub(crate) fn mentioned_user_ids(body: &str, members: &[(Uuid, String)]) -> Vec<Uuid> {
    let mut by_length: Vec<&(Uuid, String)> = members
        .iter()
        .filter(|(_, name)| !name.trim().is_empty())
        .collect();
    by_length.sort_by_key(|(_, name)| std::cmp::Reverse(name.len()));

    // Byte ranges already claimed by a (longer) name, so "@Anna Schmidt"
    // does not additionally mention a member named "Anna".
    let mut claimed: Vec<(usize, usize)> = Vec::new();
    let mut out: Vec<Uuid> = Vec::new();
    for (user_id, name) in by_length {
        let pattern = format!("@{name}");
        for (start, _) in body.match_indices(&pattern) {
            let end = start + pattern.len();
            let boundary_ok = body[end..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_alphanumeric());
            let overlaps = claimed.iter().any(|&(s, e)| start < e && end > s);
            if boundary_ok && !overlaps {
                claimed.push((start, end));
                if !out.contains(user_id) {
                    out.push(*user_id);
                }
            }
        }
    }
    out
}
