use crate::*;

pub(crate) async fn list_milestones(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<WorkspaceQuery>,
) -> Result<Json<Vec<MilestoneDto>>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let project_id = active_project_id(
        &state.db,
        uuid_from_str(&ctx.user.id)?,
        query.workspace_uuid()?,
    )
    .await?;
    Ok(Json(fetch_milestones(&state.db, project_id).await?))
}

pub(crate) async fn create_milestone(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateMilestoneRequest>,
) -> Result<Json<MilestoneDto>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    let project_id = uuid_from_str(&payload.project_id)?;
    let workspace_id = assert_project_edit(&state.db, user_id, project_id).await?;

    if payload.title.trim().is_empty() {
        return Err(AppError::BadRequest("milestone title is required".into()));
    }
    let due_date = parse_optional_date(Some(payload.due_date.as_str()))?
        .ok_or_else(|| AppError::BadRequest("milestone due date is required".into()))?;
    let phase = payload.phase.trim();
    if phase.is_empty() {
        return Err(AppError::BadRequest("milestone phase is required".into()));
    }
    let title_en = payload
        .title_en
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let milestone_id = Uuid::new_v4();
    let mut tx = state.db.begin().await?;
    sqlx::query(
        "INSERT INTO milestones (id, project_id, title, title_en, due_date, done, phase) \
         VALUES ($1, $2, $3, $4, $5, false, $6)",
    )
    .bind(milestone_id)
    .bind(project_id)
    .bind(payload.title.trim())
    .bind(title_en)
    .bind(due_date)
    .bind(phase)
    .execute(&mut *tx)
    .await?;

    record_audit(
        &mut *tx,
        workspace_id,
        user_id,
        "created milestone",
        "milestone",
        Some(milestone_id),
    )
    .await?;
    tx.commit().await?;
    notify_workspace(&state, &ctx, &headers, workspace_id, "milestone");
    Ok(Json(fetch_milestone(&state.db, milestone_id).await?))
}

pub(crate) async fn delete_milestone(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    let milestone_id = uuid_from_str(&id)?;
    let workspace_id = assert_milestone_edit(&state.db, user_id, milestone_id).await?;
    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM milestones WHERE id = $1")
        .bind(milestone_id)
        .execute(&mut *tx)
        .await?;
    record_audit(
        &mut *tx,
        workspace_id,
        user_id,
        "deleted milestone",
        "milestone",
        Some(milestone_id),
    )
    .await?;
    tx.commit().await?;
    notify_workspace(&state, &ctx, &headers, workspace_id, "milestone");
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn fetch_milestone(
    db: &PgPool,
    milestone_id: Uuid,
) -> Result<MilestoneDto, AppError> {
    let row: MilestoneRow = sqlx::query_as(
        "SELECT id, project_id, title, title_en, due_date, done, phase \
         FROM milestones WHERE id = $1",
    )
    .bind(milestone_id)
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(milestone_from_row(row))
}

pub(crate) fn milestone_from_row(row: MilestoneRow) -> MilestoneDto {
    MilestoneDto {
        id: row.id.to_string(),
        project_id: row.project_id.to_string(),
        title: row.title,
        title_en: row.title_en,
        due_date: row.due_date.to_string(),
        done: row.done,
        phase: row.phase,
    }
}
