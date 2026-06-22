use crate::*;

pub(crate) const fn role_to_db(role: &Role) -> &'static str {
    match role {
        Role::Owner => "owner",
        Role::Admin => "admin",
        Role::Member => "member",
        Role::Viewer => "viewer",
    }
}

pub(crate) fn role_from_db(value: &str) -> Result<Role, AppError> {
    match value {
        "owner" => Ok(Role::Owner),
        "admin" => Ok(Role::Admin),
        "member" => Ok(Role::Member),
        "viewer" => Ok(Role::Viewer),
        _ => Err(AppError::BadRequest(format!("unknown role {value}"))),
    }
}

pub(crate) fn member_status_from_db(value: &str) -> Result<MemberStatus, AppError> {
    match value {
        "active" => Ok(MemberStatus::Active),
        "invited" => Ok(MemberStatus::Invited),
        _ => Err(AppError::BadRequest(format!(
            "unknown member status {value}"
        ))),
    }
}

pub(crate) const fn priority_to_db(priority: &Priority) -> &'static str {
    match priority {
        Priority::Urgent => "urgent",
        Priority::High => "high",
        Priority::Medium => "medium",
        Priority::Low => "low",
    }
}

pub(crate) fn priority_from_db(value: &str) -> Result<Priority, AppError> {
    match value {
        "urgent" => Ok(Priority::Urgent),
        "high" => Ok(Priority::High),
        "medium" => Ok(Priority::Medium),
        "low" => Ok(Priority::Low),
        _ => Err(AppError::BadRequest(format!("unknown priority {value}"))),
    }
}

pub(crate) const fn ticket_status_to_db(status: &TicketStatus) -> &'static str {
    match status {
        TicketStatus::Open => "open",
        TicketStatus::InProgress => "in_progress",
        TicketStatus::Resolved => "resolved",
        TicketStatus::Closed => "closed",
    }
}

pub(crate) fn ticket_status_from_db(value: &str) -> Result<TicketStatus, AppError> {
    match value {
        "open" => Ok(TicketStatus::Open),
        "in_progress" => Ok(TicketStatus::InProgress),
        "resolved" => Ok(TicketStatus::Resolved),
        "closed" => Ok(TicketStatus::Closed),
        _ => Err(AppError::BadRequest(format!(
            "unknown ticket status {value}"
        ))),
    }
}

pub(crate) const fn recurrence_to_db(recurrence: Recurrence) -> &'static str {
    match recurrence {
        Recurrence::Daily => "daily",
        Recurrence::Weekly => "weekly",
        Recurrence::Biweekly => "biweekly",
        Recurrence::Monthly => "monthly",
    }
}

pub(crate) fn recurrence_from_db(value: &str) -> Result<Recurrence, AppError> {
    match value {
        "daily" => Ok(Recurrence::Daily),
        "weekly" => Ok(Recurrence::Weekly),
        "biweekly" => Ok(Recurrence::Biweekly),
        "monthly" => Ok(Recurrence::Monthly),
        _ => Err(AppError::BadRequest(format!("unknown recurrence {value}"))),
    }
}

pub(crate) const fn notification_kind_to_db(kind: &NotificationKind) -> &'static str {
    match kind {
        NotificationKind::Assigned => "assigned",
        NotificationKind::Mention => "mention",
        NotificationKind::Due => "due",
        NotificationKind::Comment => "comment",
        NotificationKind::Done => "done",
        NotificationKind::Milestone => "milestone",
    }
}

pub(crate) fn notification_kind_from_db(value: &str) -> Result<NotificationKind, AppError> {
    match value {
        "assigned" => Ok(NotificationKind::Assigned),
        "mention" => Ok(NotificationKind::Mention),
        "due" => Ok(NotificationKind::Due),
        "comment" => Ok(NotificationKind::Comment),
        "done" => Ok(NotificationKind::Done),
        "milestone" => Ok(NotificationKind::Milestone),
        _ => Err(AppError::BadRequest(format!(
            "unknown notification kind {value}"
        ))),
    }
}

pub(crate) const fn attachment_kind_to_db(kind: &AttachmentKind) -> &'static str {
    match kind {
        AttachmentKind::File => "file",
        AttachmentKind::Image => "image",
    }
}

pub(crate) fn attachment_kind_from_db(value: &str) -> Result<AttachmentKind, AppError> {
    match value {
        "file" => Ok(AttachmentKind::File),
        "image" => Ok(AttachmentKind::Image),
        _ => Err(AppError::BadRequest(format!(
            "unknown attachment kind {value}"
        ))),
    }
}

pub(crate) fn uuid_from_str(value: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(value).map_err(|_| AppError::BadRequest("invalid id".into()))
}

pub(crate) fn optional_uuid(value: Option<&str>) -> Result<Option<Uuid>, AppError> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(uuid_from_str)
        .transpose()
}

/// Rejects free text longer than `max` characters. Returns the input unchanged
/// so it can be chained where a value is already trimmed/validated.
pub(crate) fn capped<'a>(value: &'a str, max: usize, field: &str) -> Result<&'a str, AppError> {
    if value.chars().count() > max {
        return Err(AppError::BadRequest(format!(
            "{field} is too long (max {max} characters)"
        )));
    }
    Ok(value)
}

/// Trims, then enforces both non-emptiness and the length cap for a required
/// field. The error names the field so the client can point at it.
pub(crate) fn required_capped<'a>(
    value: &'a str,
    max: usize,
    field: &str,
) -> Result<&'a str, AppError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(AppError::BadRequest(format!("{field} is required")));
    }
    capped(value, max, field)
}

/// Trims an optional free-text field and enforces the length cap. Returns the
/// trimmed value (possibly empty) so callers can bind it directly.
pub(crate) fn optional_capped<'a>(
    value: &'a str,
    max: usize,
    field: &str,
) -> Result<&'a str, AppError> {
    let value = value.trim();
    capped(value, max, field)
}

pub(crate) fn fixed_uuid(value: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(value).map_err(|e| AppError::BadRequest(e.to_string()))
}

pub(crate) fn parse_optional_date(value: Option<&str>) -> Result<Option<NaiveDate>, AppError> {
    value
        .filter(|v| !v.trim().is_empty())
        .map(|v| {
            NaiveDate::parse_from_str(v, "%Y-%m-%d")
                .map_err(|_| AppError::BadRequest("date must be YYYY-MM-DD".into()))
        })
        .transpose()
}

pub(crate) fn initials(name: &str) -> String {
    display_initials(name)
}

pub(crate) fn relative_label(ts: DateTime<Utc>, lang: &str) -> String {
    let delta = Utc::now().signed_duration_since(ts);
    if delta.num_minutes() < 1 {
        return if lang == "de" {
            "gerade eben"
        } else {
            "just now"
        }
        .to_string();
    }
    if delta.num_minutes() < 60 {
        return if lang == "de" {
            format!("vor {} Min", delta.num_minutes())
        } else {
            format!("{} min ago", delta.num_minutes())
        };
    }
    if delta.num_hours() < 24 {
        return if lang == "de" {
            format!("vor {} Std", delta.num_hours())
        } else {
            format!("{} h ago", delta.num_hours())
        };
    }
    if delta.num_days() == 1 {
        return if lang == "de" { "Gestern" } else { "Yesterday" }.to_string();
    }
    if lang == "de" {
        format!("vor {} Tagen", delta.num_days())
    } else {
        format!("{} days ago", delta.num_days())
    }
}
