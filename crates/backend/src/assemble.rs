use crate::*;

/// Loads all task children (assignees, dependencies, subtasks, comments,
/// attachments) with one batched query each instead of one set per task.
pub(crate) async fn assemble_tasks(
    db: &PgPool,
    rows: Vec<TaskRow>,
    include_details: bool,
) -> Result<Vec<TaskDto>, AppError> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();

    let assignee_rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT task_id, user_id FROM task_assignees WHERE task_id = ANY($1) ORDER BY user_id",
    )
    .bind(&ids)
    .fetch_all(db)
    .await?;
    let mut assignees: HashMap<Uuid, Vec<String>> = HashMap::new();
    for (task_id, user_id) in assignee_rows {
        assignees
            .entry(task_id)
            .or_default()
            .push(user_id.to_string());
    }

    let dependency_rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT task_id, depends_on_task_id FROM task_dependencies \
         WHERE task_id = ANY($1) ORDER BY depends_on_task_id",
    )
    .bind(&ids)
    .fetch_all(db)
    .await?;
    let mut dependencies: HashMap<Uuid, Vec<String>> = HashMap::new();
    for (task_id, dep_id) in dependency_rows {
        dependencies
            .entry(task_id)
            .or_default()
            .push(dep_id.to_string());
    }

    let subtask_rows: Vec<SubtaskRow> = sqlx::query_as(
        "SELECT id, task_id, title, title_en, done, position FROM subtasks \
         WHERE task_id = ANY($1) ORDER BY position",
    )
    .bind(&ids)
    .fetch_all(db)
    .await?;
    let mut subtasks: HashMap<Uuid, Vec<SubtaskDto>> = HashMap::new();
    for s in subtask_rows {
        subtasks.entry(s.task_id).or_default().push(SubtaskDto {
            id: s.id.to_string(),
            title: s.title,
            title_en: s.title_en,
            done: s.done,
            position: s.position,
        });
    }

    let comment_rows: Vec<CommentRow> = if include_details {
        sqlx::query_as(
            "SELECT c.id, c.task_id, c.user_id, u.name AS author_name, c.body, c.created_at \
             FROM comments c JOIN users u ON u.id = c.user_id \
             WHERE c.task_id = ANY($1) ORDER BY c.created_at DESC",
        )
        .bind(&ids)
        .fetch_all(db)
        .await?
    } else {
        Vec::new()
    };
    let mut comments: HashMap<Uuid, Vec<CommentDto>> = HashMap::new();
    for c in comment_rows {
        comments.entry(c.task_id).or_default().push(CommentDto {
            id: c.id.to_string(),
            task_id: c.task_id.to_string(),
            user_id: c.user_id.to_string(),
            author_initials: initials(&c.author_name),
            author_name: c.author_name,
            body: c.body,
            created_label_de: relative_label(c.created_at, "de"),
            created_label_en: relative_label(c.created_at, "en"),
        });
    }

    let attachment_rows: Vec<AttachmentRow> = if include_details {
        sqlx::query_as(
            "SELECT id, task_id, file_name, kind, size_bytes FROM attachments \
             WHERE task_id = ANY($1) ORDER BY created_at DESC",
        )
        .bind(&ids)
        .fetch_all(db)
        .await?
    } else {
        Vec::new()
    };
    let mut attachments: HashMap<Uuid, Vec<AttachmentDto>> = HashMap::new();
    for a in attachment_rows {
        attachments
            .entry(a.task_id)
            .or_default()
            .push(AttachmentDto {
                id: a.id.to_string(),
                task_id: a.task_id.to_string(),
                file_name: a.file_name,
                kind: attachment_kind_from_db(&a.kind).unwrap_or(AttachmentKind::File),
                size_label: size_label(a.size_bytes),
            });
    }

    rows.into_iter()
        .map(|row| {
            Ok(TaskDto {
                id: row.id.to_string(),
                project_id: row.project_id.to_string(),
                key: row.key,
                title: row.title,
                title_en: row.title_en,
                description: row.description,
                description_en: row.description_en,
                tag: row.tag,
                tag_color: row.tag_color,
                priority: priority_from_db(&row.priority)?,
                status_id: row.status_id.to_string(),
                status_position: row.status_position,
                status_is_done: row.status_is_done,
                start_date: row.start_date.map(|d| d.to_string()),
                due_date: row.due_date.map(|d| d.to_string()),
                phase: row.phase,
                recurrence: row
                    .recurrence
                    .as_deref()
                    .map(recurrence_from_db)
                    .transpose()?,
                assignee_ids: assignees.remove(&row.id).unwrap_or_default(),
                dependency_ids: dependencies.remove(&row.id).unwrap_or_default(),
                subtasks: subtasks.remove(&row.id).unwrap_or_default(),
                comments: comments.remove(&row.id).unwrap_or_default(),
                attachments: attachments.remove(&row.id).unwrap_or_default(),
                comments_count: row.comments_count,
                created_label_de: relative_label(row.created_at, "de"),
                created_label_en: relative_label(row.created_at, "en"),
                updated_label_de: relative_label(row.updated_at, "de"),
                updated_label_en: relative_label(row.updated_at, "en"),
                details_loaded: include_details,
            })
        })
        .collect()
}

pub(crate) async fn fetch_milestones(
    db: &PgPool,
    project_id: Uuid,
) -> Result<Vec<MilestoneDto>, AppError> {
    let rows: Vec<MilestoneRow> = sqlx::query_as(
        "SELECT id, project_id, title, title_en, due_date, done, phase FROM milestones WHERE project_id = $1 ORDER BY due_date",
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|m| MilestoneDto {
            id: m.id.to_string(),
            project_id: m.project_id.to_string(),
            title: m.title,
            title_en: m.title_en,
            due_date: m.due_date.to_string(),
            done: m.done,
            phase: m.phase,
        })
        .collect())
}

pub(crate) async fn fetch_notifications(
    db: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<Vec<NotificationDto>, AppError> {
    // Scoped to the active workspace: a user who belongs to several workspaces
    // must not see notifications referencing tasks the bootstrap never loaded.
    let rows: Vec<NotificationRow> = sqlx::query_as(
        "SELECT n.id, n.kind, n.actor_id, u.name AS actor_name, n.task_id, n.milestone_id, \
                n.text, n.text_en, n.unread, n.created_at \
         FROM notifications n LEFT JOIN users u ON u.id = n.actor_id \
         WHERE n.user_id = $1 AND n.workspace_id = $2 ORDER BY n.created_at DESC LIMIT 30",
    )
    .bind(user_id)
    .bind(workspace_id)
    .fetch_all(db)
    .await?;

    rows.into_iter()
        .map(|n| {
            Ok(NotificationDto {
                id: n.id.to_string(),
                kind: notification_kind_from_db(&n.kind)?,
                actor_id: n.actor_id.map(|id| id.to_string()),
                actor_initials: n.actor_name.as_deref().map(initials),
                actor_name: n.actor_name,
                task_id: n.task_id.map(|id| id.to_string()),
                milestone_id: n.milestone_id.map(|id| id.to_string()),
                text: n.text,
                text_en: n.text_en,
                unread: n.unread,
                created_label_de: relative_label(n.created_at, "de"),
                created_label_en: relative_label(n.created_at, "en"),
            })
        })
        .collect()
}

pub(crate) async fn fetch_audit_events(
    db: &PgPool,
    workspace_id: Uuid,
) -> Result<Vec<AuditEventDto>, AppError> {
    let rows: Vec<AuditRow> = sqlx::query_as(
        "SELECT a.id, u.name AS actor_name, a.action, a.entity, a.created_at \
         FROM audit_events a LEFT JOIN users u ON u.id = a.actor_id \
         WHERE a.workspace_id = $1 ORDER BY a.created_at DESC LIMIT 20",
    )
    .bind(workspace_id)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|a| AuditEventDto {
            id: a.id.to_string(),
            actor_name: a.actor_name,
            action: a.action,
            entity: a.entity,
            created_label_de: relative_label(a.created_at, "de"),
            created_label_en: relative_label(a.created_at, "en"),
        })
        .collect())
}
