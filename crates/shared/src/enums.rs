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
    pub const fn can_admin(&self) -> bool {
        matches!(self, Self::Owner | Self::Admin)
    }

    pub const fn can_edit(&self) -> bool {
        matches!(self, Self::Owner | Self::Admin | Self::Member)
    }

    pub const fn as_db(&self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::Member => "member",
            Self::Viewer => "viewer",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "owner" => Some(Self::Owner),
            "admin" => Some(Self::Admin),
            "member" => Some(Self::Member),
            "viewer" => Some(Self::Viewer),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemberStatus {
    Active,
    Invited,
}

impl MemberStatus {
    pub const fn as_db(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Invited => "invited",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "invited" => Some(Self::Invited),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Urgent,
    High,
    Medium,
    Low,
}

impl Priority {
    pub const fn as_db(&self) -> &'static str {
        match self {
            Self::Urgent => "urgent",
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "urgent" => Some(Self::Urgent),
            "high" => Some(Self::High),
            "medium" => Some(Self::Medium),
            "low" => Some(Self::Low),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TicketStatus {
    Open,
    InProgress,
    Resolved,
    Closed,
}

impl TicketStatus {
    pub const fn as_db(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::InProgress => "in_progress",
            Self::Resolved => "resolved",
            Self::Closed => "closed",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "open" => Some(Self::Open),
            "in_progress" => Some(Self::InProgress),
            "resolved" => Some(Self::Resolved),
            "closed" => Some(Self::Closed),
            _ => None,
        }
    }
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

impl NotificationKind {
    pub const fn as_db(&self) -> &'static str {
        match self {
            Self::Assigned => "assigned",
            Self::Mention => "mention",
            Self::Due => "due",
            Self::Comment => "comment",
            Self::Done => "done",
            Self::Milestone => "milestone",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "assigned" => Some(Self::Assigned),
            "mention" => Some(Self::Mention),
            "due" => Some(Self::Due),
            "comment" => Some(Self::Comment),
            "done" => Some(Self::Done),
            "milestone" => Some(Self::Milestone),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentKind {
    File,
    Image,
}

impl AttachmentKind {
    pub const fn as_db(&self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Image => "image",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "file" => Some(Self::File),
            "image" => Some(Self::Image),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Recurrence {
    Daily,
    Weekly,
    Biweekly,
    Monthly,
}

impl Recurrence {
    pub const fn as_db(&self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Biweekly => "biweekly",
            Self::Monthly => "monthly",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "daily" => Some(Self::Daily),
            "weekly" => Some(Self::Weekly),
            "biweekly" => Some(Self::Biweekly),
            "monthly" => Some(Self::Monthly),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The backend persists `as_db` to the database and the frontend parses it
    // back via `from_db`; both also (de)serialize these enums in DTOs. This
    // guards the single mapping all three rely on: every variant round-trips,
    // and `as_db` matches the serde snake_case wire form exactly.
    #[test]
    fn enum_db_strings_round_trip_and_match_serde() {
        macro_rules! assert_round_trip {
            ($ty:ident, [$($variant:ident),+ $(,)?]) => {
                $(
                    let v = $ty::$variant;
                    assert_eq!(serde_json::to_string(&v).unwrap(), format!("\"{}\"", v.as_db()));
                    assert_eq!($ty::from_db(v.as_db()), Some(v));
                )+
            };
        }
        assert_round_trip!(Role, [Owner, Admin, Member, Viewer]);
        assert_round_trip!(MemberStatus, [Active, Invited]);
        assert_round_trip!(Priority, [Urgent, High, Medium, Low]);
        assert_round_trip!(TicketStatus, [Open, InProgress, Resolved, Closed]);
        assert_round_trip!(
            NotificationKind,
            [Assigned, Mention, Due, Comment, Done, Milestone]
        );
        assert_round_trip!(AttachmentKind, [File, Image]);
        assert_round_trip!(Recurrence, [Daily, Weekly, Biweekly, Monthly]);
    }

    #[test]
    fn enum_from_db_rejects_unknown_values() {
        assert_eq!(Role::from_db("superuser"), None);
        assert_eq!(MemberStatus::from_db("banned"), None);
        assert_eq!(Priority::from_db(""), None);
        assert_eq!(TicketStatus::from_db("done"), None);
        assert_eq!(NotificationKind::from_db("ping"), None);
        assert_eq!(AttachmentKind::from_db("video"), None);
        assert_eq!(Recurrence::from_db("yearly"), None);
    }
}
