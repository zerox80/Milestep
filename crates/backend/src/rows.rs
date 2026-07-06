use crate::*;

#[derive(Debug, FromRow)]
pub(crate) struct UserAuthRow {
    pub(crate) id: Uuid,
    pub(crate) email: String,
    pub(crate) name: String,
    pub(crate) password_hash: String,
}

#[derive(Debug, FromRow)]
pub(crate) struct UserRow {
    pub(crate) id: Uuid,
    pub(crate) email: String,
    pub(crate) name: String,
}

#[derive(Debug, FromRow)]
pub(crate) struct RegisteredUserRow {
    pub(crate) id: Uuid,
    pub(crate) email: String,
    pub(crate) name: String,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) membership_id: Option<Uuid>,
    pub(crate) role: Option<String>,
}

impl From<UserRow> for UserDto {
    fn from(row: UserRow) -> Self {
        Self {
            id: row.id.to_string(),
            email: row.email,
            name: row.name,
        }
    }
}

#[derive(Debug, FromRow)]
pub(crate) struct WorkspaceRow {
    pub(crate) id: Uuid,
    pub(crate) name: String,
    pub(crate) url_slug: String,
    pub(crate) default_lang: String,
}

impl From<WorkspaceRow> for WorkspaceDto {
    fn from(row: WorkspaceRow) -> Self {
        Self {
            id: row.id.to_string(),
            name: row.name,
            url_slug: row.url_slug,
            default_lang: row.default_lang,
        }
    }
}

#[derive(Debug, FromRow)]
pub(crate) struct ProjectRow {
    pub(crate) id: Uuid,
    pub(crate) workspace_id: Uuid,
    pub(crate) name: String,
    pub(crate) key: String,
}

impl From<ProjectRow> for ProjectDto {
    fn from(row: ProjectRow) -> Self {
        Self {
            id: row.id.to_string(),
            workspace_id: row.workspace_id.to_string(),
            name: row.name,
            key: row.key,
        }
    }
}

#[derive(Debug, FromRow)]
pub(crate) struct StatusRow {
    pub(crate) id: Uuid,
    pub(crate) project_id: Uuid,
    pub(crate) name_de: String,
    pub(crate) name_en: String,
    pub(crate) position: i32,
    pub(crate) is_done: bool,
    pub(crate) color: String,
}

impl From<StatusRow> for StatusDto {
    fn from(row: StatusRow) -> Self {
        Self {
            id: row.id.to_string(),
            project_id: row.project_id.to_string(),
            name_de: row.name_de,
            name_en: row.name_en,
            position: row.position,
            is_done: row.is_done,
            color: row.color,
        }
    }
}

#[derive(Debug, FromRow)]
pub(crate) struct TaskRow {
    pub(crate) id: Uuid,
    pub(crate) project_id: Uuid,
    pub(crate) key: String,
    pub(crate) title: String,
    pub(crate) title_en: Option<String>,
    pub(crate) description: String,
    pub(crate) description_en: Option<String>,
    pub(crate) tag: String,
    pub(crate) tag_color: String,
    pub(crate) priority: String,
    pub(crate) status_id: Uuid,
    pub(crate) status_position: i32,
    pub(crate) status_is_done: bool,
    pub(crate) start_date: Option<NaiveDate>,
    pub(crate) due_date: Option<NaiveDate>,
    pub(crate) phase: String,
    pub(crate) recurrence: Option<String>,
    pub(crate) comments_count: i64,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
pub(crate) struct TicketRow {
    pub(crate) id: Uuid,
    pub(crate) project_id: Uuid,
    pub(crate) key: String,
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) status: String,
    pub(crate) priority: String,
    pub(crate) requester_name: String,
    pub(crate) assignee_id: Option<Uuid>,
    pub(crate) assignee_name: Option<String>,
    pub(crate) created_by_name: Option<String>,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
pub(crate) struct SubtaskRow {
    pub(crate) id: Uuid,
    pub(crate) task_id: Uuid,
    pub(crate) title: String,
    pub(crate) title_en: Option<String>,
    pub(crate) done: bool,
    pub(crate) position: i32,
}

#[derive(Debug, FromRow)]
pub(crate) struct CommentRow {
    pub(crate) id: Uuid,
    pub(crate) task_id: Uuid,
    pub(crate) user_id: Uuid,
    pub(crate) author_name: String,
    pub(crate) body: String,
    pub(crate) created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
pub(crate) struct AttachmentRow {
    pub(crate) id: Uuid,
    pub(crate) task_id: Uuid,
    pub(crate) file_name: String,
    pub(crate) kind: String,
    pub(crate) size_bytes: i64,
}

#[derive(Debug, FromRow)]
pub(crate) struct MembershipWorkspaceRow {
    pub(crate) workspace_id: Uuid,
    pub(crate) user_id: Uuid,
    pub(crate) role: String,
    pub(crate) status: String,
}

#[derive(Debug, FromRow)]
pub(crate) struct MemberRow {
    pub(crate) id: Uuid,
    pub(crate) user_id: Uuid,
    pub(crate) workspace_id: Uuid,
    pub(crate) name: String,
    pub(crate) email: String,
    pub(crate) role: String,
    pub(crate) status: String,
    pub(crate) last_active_at: Option<DateTime<Utc>>,
}

#[derive(Debug, FromRow)]
pub(crate) struct MilestoneRow {
    pub(crate) id: Uuid,
    pub(crate) project_id: Uuid,
    pub(crate) title: String,
    pub(crate) title_en: Option<String>,
    pub(crate) due_date: NaiveDate,
    pub(crate) done: bool,
    pub(crate) phase: String,
}

#[derive(Debug, FromRow)]
pub(crate) struct NotificationRow {
    pub(crate) id: Uuid,
    pub(crate) kind: String,
    pub(crate) actor_id: Option<Uuid>,
    pub(crate) actor_name: Option<String>,
    pub(crate) task_id: Option<Uuid>,
    pub(crate) milestone_id: Option<Uuid>,
    pub(crate) text: Option<String>,
    pub(crate) text_en: Option<String>,
    pub(crate) unread: bool,
    pub(crate) created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
pub(crate) struct AuditRow {
    pub(crate) id: Uuid,
    pub(crate) actor_name: Option<String>,
    pub(crate) action: String,
    pub(crate) entity: String,
    pub(crate) created_at: DateTime<Utc>,
}
