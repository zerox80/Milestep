use crate::*;

// String <-> enum mapping lives once on the enums in `kowobau_shared`
// (`as_db` / `from_db`). These thin wrappers keep the backend's strict policy:
// an unrecognized value is a `BadRequest` rather than a silent fallback.

pub(crate) fn role_to_db(role: &Role) -> &'static str {
    role.as_db()
}

pub(crate) fn role_from_db(value: &str) -> Result<Role, AppError> {
    Role::from_db(value).ok_or_else(|| AppError::BadRequest(format!("unknown role {value}")))
}

pub(crate) fn member_status_from_db(value: &str) -> Result<MemberStatus, AppError> {
    MemberStatus::from_db(value)
        .ok_or_else(|| AppError::BadRequest(format!("unknown member status {value}")))
}

pub(crate) fn priority_to_db(priority: &Priority) -> &'static str {
    priority.as_db()
}

pub(crate) fn priority_from_db(value: &str) -> Result<Priority, AppError> {
    Priority::from_db(value)
        .ok_or_else(|| AppError::BadRequest(format!("unknown priority {value}")))
}

pub(crate) fn ticket_status_to_db(status: &TicketStatus) -> &'static str {
    status.as_db()
}

pub(crate) fn ticket_status_from_db(value: &str) -> Result<TicketStatus, AppError> {
    TicketStatus::from_db(value)
        .ok_or_else(|| AppError::BadRequest(format!("unknown ticket status {value}")))
}

pub(crate) fn recurrence_to_db(recurrence: Recurrence) -> &'static str {
    recurrence.as_db()
}

pub(crate) fn recurrence_from_db(value: &str) -> Result<Recurrence, AppError> {
    Recurrence::from_db(value)
        .ok_or_else(|| AppError::BadRequest(format!("unknown recurrence {value}")))
}

pub(crate) fn notification_kind_to_db(kind: &NotificationKind) -> &'static str {
    kind.as_db()
}

pub(crate) fn notification_kind_from_db(value: &str) -> Result<NotificationKind, AppError> {
    NotificationKind::from_db(value)
        .ok_or_else(|| AppError::BadRequest(format!("unknown notification kind {value}")))
}

pub(crate) fn attachment_kind_to_db(kind: &AttachmentKind) -> &'static str {
    kind.as_db()
}

pub(crate) fn attachment_kind_from_db(value: &str) -> Result<AttachmentKind, AppError> {
    AttachmentKind::from_db(value)
        .ok_or_else(|| AppError::BadRequest(format!("unknown attachment kind {value}")))
}

pub(crate) fn uuid_from_str(value: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(value).map_err(|_| AppError::BadRequest("invalid id".into()))
}

pub(crate) fn required_trimmed<'a>(
    value: &'a str,
    message: &'static str,
) -> Result<&'a str, AppError> {
    let value = value.trim();
    if value.is_empty() {
        Err(AppError::BadRequest(message.into()))
    } else {
        Ok(value)
    }
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
    let mut chars = name
        .split_whitespace()
        .filter_map(|part| part.chars().next())
        .take(2)
        .collect::<String>();
    if chars.is_empty() {
        chars = "?".to_string();
    }
    chars.to_uppercase()
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
