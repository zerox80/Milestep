use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Owner,
    Admin,
    Member,
    Viewer,
}

impl Role {
    pub fn can_admin(&self) -> bool {
        matches!(self, Self::Owner | Self::Admin)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemberStatus {
    Active,
    Invited,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Urgent,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind {
    Assigned,
    Mention,
    Due,
    Comment,
    Done,
    Milestone,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentKind {
    File,
    Image,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserDto {
    pub id: String,
    pub email: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceDto {
    pub id: String,
    pub name: String,
    pub url_slug: String,
    pub default_lang: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDto {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberDto {
    pub id: String,
    pub user_id: String,
    pub workspace_id: String,
    pub name: String,
    pub email: String,
    pub initials: String,
    pub role: Role,
    pub status: MemberStatus,
    pub last_active_label_de: String,
    pub last_active_label_en: String,
    pub open_tasks: i64,
    pub done_tasks: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusDto {
    pub id: String,
    pub project_id: String,
    pub name_de: String,
    pub name_en: String,
    pub position: i32,
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtaskDto {
    pub id: String,
    pub title: String,
    pub title_en: Option<String>,
    pub done: bool,
    pub position: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommentDto {
    pub id: String,
    pub task_id: String,
    pub user_id: String,
    pub author_name: String,
    pub author_initials: String,
    pub body: String,
    pub created_label_de: String,
    pub created_label_en: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentDto {
    pub id: String,
    pub task_id: String,
    pub file_name: String,
    pub kind: AttachmentKind,
    pub size_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDto {
    pub id: String,
    pub project_id: String,
    pub key: String,
    pub title: String,
    pub title_en: Option<String>,
    pub description: String,
    pub description_en: Option<String>,
    pub tag: String,
    pub tag_color: String,
    pub priority: Priority,
    pub status_id: String,
    pub status_position: i32,
    pub start_date: Option<String>,
    pub due_date: Option<String>,
    pub phase: String,
    pub assignee_ids: Vec<String>,
    pub dependency_ids: Vec<String>,
    pub subtasks: Vec<SubtaskDto>,
    pub comments: Vec<CommentDto>,
    pub attachments: Vec<AttachmentDto>,
    pub comments_count: i64,
    pub created_label_de: String,
    pub created_label_en: String,
    pub updated_label_de: String,
    pub updated_label_en: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MilestoneDto {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub title_en: Option<String>,
    pub due_date: String,
    pub done: bool,
    pub phase: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationDto {
    pub id: String,
    pub kind: NotificationKind,
    pub actor_id: Option<String>,
    pub actor_name: Option<String>,
    pub actor_initials: Option<String>,
    pub task_id: Option<String>,
    pub milestone_id: Option<String>,
    pub text: Option<String>,
    pub text_en: Option<String>,
    pub unread: bool,
    pub created_label_de: String,
    pub created_label_en: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEventDto {
    pub id: String,
    pub actor_name: Option<String>,
    pub action: String,
    pub entity: String,
    pub created_label_de: String,
    pub created_label_en: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapDto {
    pub current_user: UserDto,
    pub workspace: WorkspaceDto,
    pub project: ProjectDto,
    pub current_role: Role,
    pub members: Vec<MemberDto>,
    pub statuses: Vec<StatusDto>,
    pub tasks: Vec<TaskDto>,
    pub milestones: Vec<MilestoneDto>,
    pub notifications: Vec<NotificationDto>,
    pub audit_events: Vec<AuditEventDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub name: String,
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    pub user: UserDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTaskRequest {
    pub project_id: String,
    pub title: String,
    pub description: String,
    pub tag: String,
    pub tag_color: String,
    pub priority: Priority,
    pub status_id: String,
    pub start_date: Option<String>,
    pub due_date: Option<String>,
    pub phase: String,
    pub assignee_ids: Vec<String>,
    pub subtasks: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateTaskRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub tag: Option<String>,
    pub tag_color: Option<String>,
    pub priority: Option<Priority>,
    pub status_id: Option<String>,
    pub start_date: Option<Option<String>>,
    pub due_date: Option<Option<String>>,
    pub phase: Option<String>,
    pub assignee_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveTaskRequest {
    pub status_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSubtaskRequest {
    pub title: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateSubtaskRequest {
    pub title: Option<String>,
    pub done: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCommentRequest {
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateWorkspaceRequest {
    pub name: Option<String>,
    pub default_lang: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InviteMemberRequest {
    pub email: String,
    pub role: Role,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateMembershipRequest {
    pub role: Role,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiErrorDto {
    pub error: String,
}
