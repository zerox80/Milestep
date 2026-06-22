use crate::*;

pub(crate) async fn project_access(
    db: &PgPool,
    user_id: Uuid,
    project_id: Uuid,
) -> Result<(Uuid, Role), AppError> {
    let row: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT p.workspace_id, m.role \
         FROM projects p JOIN memberships m ON m.workspace_id = p.workspace_id \
         WHERE p.id = $1 AND m.user_id = $2 AND m.status = 'active'",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(db)
    .await?;
    let Some((workspace_id, role)) = row else {
        return Err(AppError::Forbidden);
    };
    Ok((workspace_id, role_from_db(&role)?))
}

pub(crate) async fn assert_project_edit(
    db: &PgPool,
    user_id: Uuid,
    project_id: Uuid,
) -> Result<Uuid, AppError> {
    let (workspace_id, role) = project_access(db, user_id, project_id).await?;
    if !role.can_edit() {
        return Err(AppError::Forbidden);
    }
    Ok(workspace_id)
}

pub(crate) async fn milestone_access(
    db: &PgPool,
    user_id: Uuid,
    milestone_id: Uuid,
) -> Result<(Uuid, Role), AppError> {
    let row: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT p.workspace_id, m.role \
         FROM milestones ms JOIN projects p ON p.id = ms.project_id \
         JOIN memberships m ON m.workspace_id = p.workspace_id \
         WHERE ms.id = $1 AND m.user_id = $2 AND m.status = 'active'",
    )
    .bind(milestone_id)
    .bind(user_id)
    .fetch_optional(db)
    .await?;
    let Some((workspace_id, role)) = row else {
        return Err(AppError::Forbidden);
    };
    Ok((workspace_id, role_from_db(&role)?))
}

pub(crate) async fn assert_milestone_edit(
    db: &PgPool,
    user_id: Uuid,
    milestone_id: Uuid,
) -> Result<Uuid, AppError> {
    let (workspace_id, role) = milestone_access(db, user_id, milestone_id).await?;
    if !role.can_edit() {
        return Err(AppError::Forbidden);
    }
    Ok(workspace_id)
}

pub(crate) async fn task_access(
    db: &PgPool,
    user_id: Uuid,
    task_id: Uuid,
) -> Result<(Uuid, Role), AppError> {
    let row: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT p.workspace_id, m.role \
         FROM tasks t JOIN projects p ON p.id = t.project_id \
         JOIN memberships m ON m.workspace_id = p.workspace_id \
         WHERE t.id = $1 AND m.user_id = $2 AND m.status = 'active'",
    )
    .bind(task_id)
    .bind(user_id)
    .fetch_optional(db)
    .await?;
    let Some((workspace_id, role)) = row else {
        return Err(AppError::Forbidden);
    };
    Ok((workspace_id, role_from_db(&role)?))
}

pub(crate) async fn assert_task_read(
    db: &PgPool,
    user_id: Uuid,
    task_id: Uuid,
) -> Result<Uuid, AppError> {
    let (workspace_id, _) = task_access(db, user_id, task_id).await?;
    Ok(workspace_id)
}

pub(crate) async fn assert_task_edit(
    db: &PgPool,
    user_id: Uuid,
    task_id: Uuid,
) -> Result<Uuid, AppError> {
    let (workspace_id, role) = task_access(db, user_id, task_id).await?;
    if !role.can_edit() {
        return Err(AppError::Forbidden);
    }
    Ok(workspace_id)
}

pub(crate) async fn ticket_access(
    db: &PgPool,
    user_id: Uuid,
    ticket_id: Uuid,
) -> Result<(Uuid, Role), AppError> {
    let row: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT p.workspace_id, m.role \
         FROM tickets t JOIN projects p ON p.id = t.project_id \
         JOIN memberships m ON m.workspace_id = p.workspace_id \
         WHERE t.id = $1 AND m.user_id = $2 AND m.status = 'active'",
    )
    .bind(ticket_id)
    .bind(user_id)
    .fetch_optional(db)
    .await?;
    let Some((workspace_id, role)) = row else {
        return Err(AppError::Forbidden);
    };
    Ok((workspace_id, role_from_db(&role)?))
}

pub(crate) async fn assert_ticket_read(
    db: &PgPool,
    user_id: Uuid,
    ticket_id: Uuid,
) -> Result<Uuid, AppError> {
    let (workspace_id, _) = ticket_access(db, user_id, ticket_id).await?;
    Ok(workspace_id)
}

pub(crate) async fn assert_ticket_edit(
    db: &PgPool,
    user_id: Uuid,
    ticket_id: Uuid,
) -> Result<Uuid, AppError> {
    let (workspace_id, role) = ticket_access(db, user_id, ticket_id).await?;
    if !role.can_edit() {
        return Err(AppError::Forbidden);
    }
    Ok(workspace_id)
}

pub(crate) async fn assert_status_in_project(
    exec: impl sqlx::PgExecutor<'_>,
    project_id: Uuid,
    status_id: Uuid,
) -> Result<(), AppError> {
    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM project_statuses WHERE id = $1 AND project_id = $2")
            .bind(status_id)
            .bind(project_id)
            .fetch_one(exec)
            .await?;
    if count.0 == 0 {
        return Err(AppError::BadRequest(
            "status does not belong to project".into(),
        ));
    }
    Ok(())
}

pub(crate) async fn assert_user_in_project(
    exec: impl sqlx::PgExecutor<'_>,
    project_id: Uuid,
    user_id: Uuid,
) -> Result<(), AppError> {
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM memberships m \
         JOIN projects p ON p.workspace_id = m.workspace_id \
         WHERE p.id = $1 AND m.user_id = $2 AND m.status = 'active'",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_one(exec)
    .await?;
    if count.0 == 0 {
        return Err(AppError::BadRequest(
            "assignee is not an active workspace member".into(),
        ));
    }
    Ok(())
}

pub(crate) async fn workspace_role(
    exec: impl sqlx::PgExecutor<'_>,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<Option<Role>, AppError> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT role FROM memberships WHERE user_id = $1 AND workspace_id = $2 AND status = 'active'")
            .bind(user_id)
            .bind(workspace_id)
            .fetch_optional(exec)
            .await?;
    row.map(|(role,)| role_from_db(&role)).transpose()
}

pub(crate) async fn assert_workspace_admin(
    db: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<(), AppError> {
    let role = workspace_role(db, user_id, workspace_id)
        .await?
        .ok_or(AppError::Forbidden)?;
    if !role.can_admin() {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

pub(crate) async fn replace_assignees(
    conn: &mut PgConnection,
    task_id: Uuid,
    assignee_ids: &[String],
) -> Result<(), AppError> {
    let mut ids = assignee_ids
        .iter()
        .map(|id| uuid_from_str(id))
        .collect::<Result<Vec<_>, _>>()?;
    ids.sort_unstable();
    ids.dedup();

    if !ids.is_empty() {
        let valid: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM memberships m \
             JOIN projects p ON p.workspace_id = m.workspace_id \
             JOIN tasks t ON t.project_id = p.id \
             WHERE t.id = $1 AND m.status = 'active' AND m.user_id = ANY($2)",
        )
        .bind(task_id)
        .bind(&ids)
        .fetch_one(&mut *conn)
        .await?;
        if valid.0 != ids.len() as i64 {
            return Err(AppError::BadRequest(
                "assignee is not an active workspace member".into(),
            ));
        }
    }

    sqlx::query("DELETE FROM task_assignees WHERE task_id = $1")
        .bind(task_id)
        .execute(&mut *conn)
        .await?;
    for user_id in &ids {
        sqlx::query("INSERT INTO task_assignees (task_id, user_id) VALUES ($1, $2)")
            .bind(task_id)
            .bind(user_id)
            .execute(&mut *conn)
            .await?;
    }
    touch_task(&mut *conn, task_id).await?;
    Ok(())
}

pub(crate) async fn touch_task(
    exec: impl sqlx::PgExecutor<'_>,
    task_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query("UPDATE tasks SET updated_at = now() WHERE id = $1")
        .bind(task_id)
        .execute(exec)
        .await?;
    Ok(())
}

pub(crate) async fn record_audit(
    exec: impl sqlx::PgExecutor<'_>,
    workspace_id: Uuid,
    actor_id: Uuid,
    action: &str,
    entity: &str,
    entity_id: Option<Uuid>,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO audit_events (id, workspace_id, actor_id, action, entity, entity_id, metadata) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(Uuid::new_v4())
    .bind(workspace_id)
    .bind(actor_id)
    .bind(action)
    .bind(entity)
    .bind(entity_id)
    .bind(json!({}))
    .execute(exec)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn insert_notification(
    exec: impl sqlx::PgExecutor<'_>,
    workspace_id: Uuid,
    user_id: Uuid,
    kind: &NotificationKind,
    actor_id: Uuid,
    task_id: Option<Uuid>,
    milestone_id: Option<Uuid>,
    text_de: &str,
    text_en: &str,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO notifications (id, workspace_id, user_id, kind, actor_id, task_id, milestone_id, text, text_en, unread) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, true)",
    )
    .bind(Uuid::new_v4())
    .bind(workspace_id)
    .bind(user_id)
    .bind(notification_kind_to_db(kind))
    .bind(actor_id)
    .bind(task_id)
    .bind(milestone_id)
    .bind(text_de)
    .bind(text_en)
    .execute(exec)
    .await?;
    Ok(())
}

/// Notifies every other active workspace member that a milestone was created.
/// Mirrors the comment-notification fan-out and is the one place that uses the
/// `milestone` notification kind and the `milestone_id` column.
pub(crate) async fn notify_milestone_created(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    workspace_id: Uuid,
    actor_id: Uuid,
    milestone_id: Uuid,
    title: &str,
) -> Result<(), AppError> {
    let members: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT user_id FROM memberships WHERE workspace_id = $1 AND status = 'active'",
    )
    .bind(workspace_id)
    .fetch_all(&mut **tx)
    .await?;
    for (target,) in members {
        if target == actor_id {
            continue;
        }
        insert_notification(
            &mut **tx,
            workspace_id,
            target,
            &NotificationKind::Milestone,
            actor_id,
            None,
            Some(milestone_id),
            &format!("hat einen Meilenstein erstellt: {title}"),
            &format!("created a milestone: {title}"),
        )
        .await?;
    }
    Ok(())
}

pub(crate) async fn task_status_is_done(
    exec: impl sqlx::PgExecutor<'_>,
    task_id: Uuid,
) -> Result<bool, AppError> {
    let (is_done,): (bool,) = sqlx::query_as(
        "SELECT s.is_done FROM tasks t JOIN project_statuses s ON s.id = t.status_id WHERE t.id = $1",
    )
    .bind(task_id)
    .fetch_one(exec)
    .await?;
    Ok(is_done)
}
