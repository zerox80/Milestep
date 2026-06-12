use crate::*;

pub(crate) async fn fetch_user(db: &PgPool, id: Uuid) -> Result<UserDto, AppError> {
    let row: UserRow = sqlx::query_as("SELECT id, email, name FROM users WHERE id = $1")
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(row.into())
}

pub(crate) async fn fetch_workspace(db: &PgPool, id: Uuid) -> Result<WorkspaceDto, AppError> {
    let row: WorkspaceRow =
        sqlx::query_as("SELECT id, name, url_slug, default_lang FROM workspaces WHERE id = $1")
            .bind(id)
            .fetch_optional(db)
            .await?
            .ok_or(AppError::NotFound)?;
    Ok(row.into())
}

pub(crate) async fn fetch_bootstrap(db: &PgPool, user_id: Uuid) -> Result<BootstrapDto, AppError> {
    let user = fetch_user(db, user_id).await?;
    let membership: MembershipWorkspaceRow = sqlx::query_as(
        "SELECT workspace_id, user_id, role \
         FROM memberships WHERE user_id = $1 AND status = 'active' ORDER BY created_at ASC LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)?;

    sqlx::query(
        "UPDATE memberships SET last_active_at = now() WHERE user_id = $1 AND workspace_id = $2",
    )
    .bind(user_id)
    .bind(membership.workspace_id)
    .execute(db)
    .await?;

    let workspace = fetch_workspace(db, membership.workspace_id).await?;
    let project_row: ProjectRow = sqlx::query_as(
        "SELECT id, workspace_id, name, key FROM projects WHERE workspace_id = $1 ORDER BY created_at ASC LIMIT 1",
    )
    .bind(membership.workspace_id)
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)?;
    let project: ProjectDto = project_row.into();
    let project_id = uuid_from_str(&project.id)?;

    let statuses = fetch_statuses(db, project_id).await?;
    let members = fetch_members(db, membership.workspace_id).await?;
    let tasks = fetch_tasks(db, project_id).await?;
    let tickets = fetch_tickets(db, project_id).await?;
    let milestones = fetch_milestones(db, project_id).await?;
    let notifications = fetch_notifications(db, user_id).await?;
    let audit_events = fetch_audit_events(db, membership.workspace_id).await?;
    let current_role = role_from_db(&membership.role)?;
    let registered_users = if current_role.can_admin() {
        fetch_registered_users(db, membership.workspace_id).await?
    } else {
        Vec::new()
    };

    Ok(BootstrapDto {
        current_user: user,
        workspace,
        project,
        current_role,
        members,
        registered_users,
        statuses,
        tasks,
        tickets,
        milestones,
        notifications,
        audit_events,
    })
}

pub(crate) async fn fetch_statuses(
    db: &PgPool,
    project_id: Uuid,
) -> Result<Vec<StatusDto>, AppError> {
    let rows: Vec<StatusRow> = sqlx::query_as(
        "SELECT id, project_id, name_de, name_en, position, is_done, color \
         FROM project_statuses WHERE project_id = $1 ORDER BY position",
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub(crate) async fn fetch_members(
    db: &PgPool,
    workspace_id: Uuid,
) -> Result<Vec<MemberDto>, AppError> {
    let rows: Vec<MemberRow> = sqlx::query_as(
        "SELECT m.id, m.user_id, m.workspace_id, u.name, u.email, m.role, m.status, m.last_active_at \
         FROM memberships m JOIN users u ON u.id = m.user_id \
         WHERE m.workspace_id = $1 ORDER BY u.name",
    )
    .bind(workspace_id)
    .fetch_all(db)
    .await?;

    // One aggregate query for the whole workspace instead of two counts per member.
    let count_rows: Vec<(Uuid, i64, i64)> = sqlx::query_as(
        "SELECT ta.user_id, \
                COUNT(*) FILTER (WHERE NOT s.is_done), \
                COUNT(*) FILTER (WHERE s.is_done) \
         FROM task_assignees ta \
         JOIN tasks t ON t.id = ta.task_id \
         JOIN projects p ON p.id = t.project_id \
         JOIN project_statuses s ON s.id = t.status_id \
         WHERE p.workspace_id = $1 GROUP BY ta.user_id",
    )
    .bind(workspace_id)
    .fetch_all(db)
    .await?;
    let counts: HashMap<Uuid, (i64, i64)> = count_rows
        .into_iter()
        .map(|(user_id, open, done)| (user_id, (open, done)))
        .collect();

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let (open_tasks, done_tasks) = counts.get(&row.user_id).copied().unwrap_or((0, 0));

        out.push(MemberDto {
            id: row.id.to_string(),
            user_id: row.user_id.to_string(),
            workspace_id: row.workspace_id.to_string(),
            initials: initials(&row.name),
            name: row.name,
            email: row.email,
            role: role_from_db(&row.role)?,
            status: member_status_from_db(&row.status)?,
            last_active_label_de: row
                .last_active_at
                .map_or_else(|| "nie".to_string(), |t| relative_label(t, "de")),
            last_active_label_en: row
                .last_active_at
                .map_or_else(|| "never".to_string(), |t| relative_label(t, "en")),
            open_tasks,
            done_tasks,
        });
    }
    Ok(out)
}

pub(crate) async fn fetch_member(db: &PgPool, membership_id: Uuid) -> Result<MemberDto, AppError> {
    let row: MemberRow = sqlx::query_as(
        "SELECT m.id, m.user_id, m.workspace_id, u.name, u.email, m.role, m.status, m.last_active_at \
         FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.id = $1",
    )
    .bind(membership_id)
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)?;
    let members = fetch_members(db, row.workspace_id).await?;
    members
        .into_iter()
        .find(|m| m.id == membership_id.to_string())
        .ok_or(AppError::NotFound)
}

pub(crate) async fn fetch_registered_users(
    db: &PgPool,
    workspace_id: Uuid,
) -> Result<Vec<RegisteredUserDto>, AppError> {
    let rows: Vec<RegisteredUserRow> = sqlx::query_as(
        "SELECT u.id, u.email, u.name, u.created_at, \
                m.id AS membership_id, m.role \
         FROM users u \
         JOIN memberships m ON m.user_id = u.id \
         WHERE m.workspace_id = $1 AND m.status = 'active' \
         ORDER BY u.created_at DESC, u.email",
    )
    .bind(workspace_id)
    .fetch_all(db)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(RegisteredUserDto {
                id: row.id.to_string(),
                email: row.email,
                initials: initials(&row.name),
                name: row.name,
                membership_id: row.membership_id.map(|id| id.to_string()),
                role: row.role.as_deref().map(role_from_db).transpose()?,
                created_label_de: relative_label(row.created_at, "de"),
                created_label_en: relative_label(row.created_at, "en"),
            })
        })
        .collect()
}

pub(crate) const TASK_SELECT: &str =
    "SELECT t.id, t.project_id, t.key, t.title, t.title_en, t.description, t.description_en, \
            t.tag, t.tag_color, t.priority, t.status_id, s.position AS status_position, \
            s.is_done AS status_is_done, \
            t.start_date, t.due_date, t.phase, t.recurrence, t.comments_count, \
            t.created_at, t.updated_at \
     FROM tasks t JOIN project_statuses s ON s.id = t.status_id";

pub(crate) async fn fetch_tasks(db: &PgPool, project_id: Uuid) -> Result<Vec<TaskDto>, AppError> {
    let rows: Vec<TaskRow> = sqlx::query_as(&format!(
        "{TASK_SELECT} WHERE t.project_id = $1 ORDER BY s.position, t.due_date NULLS LAST, t.key"
    ))
    .bind(project_id)
    .fetch_all(db)
    .await?;
    assemble_tasks(db, rows).await
}

pub(crate) async fn fetch_task(db: &PgPool, task_id: Uuid) -> Result<TaskDto, AppError> {
    let row: TaskRow = sqlx::query_as(&format!("{TASK_SELECT} WHERE t.id = $1"))
        .bind(task_id)
        .fetch_optional(db)
        .await?
        .ok_or(AppError::NotFound)?;
    assemble_tasks(db, vec![row])
        .await?
        .pop()
        .ok_or(AppError::NotFound)
}

pub(crate) const TICKET_SELECT: &str =
    "SELECT t.id, t.project_id, t.key, t.title, t.description, t.status, t.priority, \
            t.requester_name, t.assignee_id, au.name AS assignee_name, \
            cu.name AS created_by_name, t.created_at, t.updated_at \
     FROM tickets t \
     LEFT JOIN users au ON au.id = t.assignee_id \
     LEFT JOIN users cu ON cu.id = t.created_by";

pub(crate) async fn fetch_tickets(
    db: &PgPool,
    project_id: Uuid,
) -> Result<Vec<TicketDto>, AppError> {
    let rows: Vec<TicketRow> = sqlx::query_as(&format!(
        "{TICKET_SELECT} WHERE t.project_id = $1 ORDER BY t.updated_at DESC, t.key DESC"
    ))
    .bind(project_id)
    .fetch_all(db)
    .await?;
    rows.into_iter().map(ticket_from_row).collect()
}

pub(crate) async fn fetch_ticket(db: &PgPool, ticket_id: Uuid) -> Result<TicketDto, AppError> {
    let row: TicketRow = sqlx::query_as(&format!("{TICKET_SELECT} WHERE t.id = $1"))
        .bind(ticket_id)
        .fetch_optional(db)
        .await?
        .ok_or(AppError::NotFound)?;
    ticket_from_row(row)
}

pub(crate) fn ticket_from_row(row: TicketRow) -> Result<TicketDto, AppError> {
    Ok(TicketDto {
        id: row.id.to_string(),
        project_id: row.project_id.to_string(),
        key: row.key,
        title: row.title,
        description: row.description,
        status: ticket_status_from_db(&row.status)?,
        priority: priority_from_db(&row.priority)?,
        requester_name: row.requester_name,
        assignee_id: row.assignee_id.map(|id| id.to_string()),
        assignee_name: row.assignee_name,
        created_by_name: row.created_by_name,
        created_label_de: relative_label(row.created_at, "de"),
        created_label_en: relative_label(row.created_at, "en"),
        updated_label_de: relative_label(row.updated_at, "de"),
        updated_label_en: relative_label(row.updated_at, "en"),
    })
}
