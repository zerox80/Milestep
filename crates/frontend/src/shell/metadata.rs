use crate::*;

/// Returns the current page title for the dashboard chrome.
pub(super) fn header_title(boot: &BootstrapDto, nav: NavView, lang: Lang) -> String {
    match (nav, lang) {
        (NavView::Overview, Lang::De) => {
            format!("Guten Morgen, {}", first_name(&boot.current_user.name))
        }
        (NavView::Overview, Lang::En) => {
            format!("Good morning, {}", first_name(&boot.current_user.name))
        }
        (NavView::Board, Lang::De) => "Aufgaben-Board".into(),
        (NavView::Board, Lang::En) => "Task board".into(),
        (NavView::Tickets, Lang::De) => "Tickets".into(),
        (NavView::Tickets, Lang::En) => "Tickets".into(),
        (NavView::Calendar, Lang::De) => "Kalender".into(),
        (NavView::Calendar, Lang::En) => "Calendar".into(),
        (NavView::Gantt, Lang::De) => "Gantt-Diagramm".into(),
        (NavView::Gantt, Lang::En) => "Gantt chart".into(),
        (NavView::Roadmap, Lang::De) => "Bau-Roadmap".into(),
        (NavView::Roadmap, Lang::En) => "Project roadmap".into(),
        (NavView::Team, Lang::De) => "Team".into(),
        (NavView::Team, Lang::En) => "Team".into(),
        (NavView::Admin, Lang::De) => "Administration".into(),
        (NavView::Admin, Lang::En) => "Administration".into(),
        (NavView::Settings, Lang::De) => "Einstellungen".into(),
        (NavView::Settings, Lang::En) => "Settings".into(),
    }
}

/// Returns the compact context text shown beneath the current page title.
pub(super) fn header_subtitle(boot: &BootstrapDto, nav: NavView, lang: Lang) -> String {
    let today = today_iso();
    let due_today = boot
        .tasks
        .iter()
        .filter(|t| !t.status_is_done && t.due_date.as_deref() == Some(today.as_str()))
        .count();
    match (nav, lang) {
        (NavView::Overview, Lang::De) => match due_today {
            1 => "Du hast 1 Aufgabe heute fÃ¤llig.".into(),
            n => format!("Du hast {n} Aufgaben heute fÃ¤llig."),
        },
        (NavView::Overview, Lang::En) => match due_today {
            1 => "You have 1 task due today.".into(),
            n => format!("You have {n} tasks due today."),
        },
        (NavView::Board, Lang::De) => format!(
            "{} Aufgaben Â· {} Spalten",
            boot.tasks.len(),
            boot.statuses.len()
        ),
        (NavView::Board, Lang::En) => format!(
            "{} tasks Â· {} columns",
            boot.tasks.len(),
            boot.statuses.len()
        ),
        (NavView::Tickets, Lang::De) => format!(
            "{} Tickets Â· {} offen",
            boot.tickets.len(),
            boot.tickets
                .iter()
                .filter(|t| matches!(t.status, TicketStatus::Open | TicketStatus::InProgress))
                .count()
        ),
        (NavView::Tickets, Lang::En) => format!(
            "{} tickets Â· {} open",
            boot.tickets.len(),
            boot.tickets
                .iter()
                .filter(|t| matches!(t.status, TicketStatus::Open | TicketStatus::InProgress))
                .count()
        ),
        (NavView::Calendar, Lang::De) => "FÃ¤lligkeiten und Meilensteine".into(),
        (NavView::Calendar, Lang::En) => "Due dates and milestones".into(),
        (NavView::Gantt, Lang::De) => "Zeitplan, AbhÃ¤ngigkeiten und Meilensteine".into(),
        (NavView::Gantt, Lang::En) => "Schedule, dependencies and milestones".into(),
        (NavView::Roadmap, Lang::De) => "Initiativen nach Zeithorizont".into(),
        (NavView::Roadmap, Lang::En) => "Initiatives by horizon".into(),
        (NavView::Team, Lang::De) => {
            format!("{} Mitglieder Â· {}", boot.members.len(), boot.project.name)
        }
        (NavView::Team, Lang::En) => {
            format!("{} members Â· {}", boot.members.len(), boot.project.name)
        }
        (NavView::Admin, Lang::De) => "Mitglieder, Rollen, System und Sicherheit".into(),
        (NavView::Admin, Lang::En) => "Members, roles, system and security".into(),
        (NavView::Settings, Lang::De) => "Design und persÃ¶nliche Einstellungen".into(),
        (NavView::Settings, Lang::En) => "Appearance and personal preferences".into(),
    }
}

pub(super) fn nav_icon(view: NavView) -> AppIcon {
    match view {
        NavView::Overview => AppIcon::Dashboard,
        NavView::Board => AppIcon::Kanban,
        NavView::Tickets => AppIcon::Ticket,
        NavView::Calendar => AppIcon::Calendar,
        NavView::Gantt => AppIcon::Timeline,
        NavView::Roadmap => AppIcon::Roadmap,
        NavView::Team => AppIcon::Users,
        NavView::Admin => AppIcon::Settings,
        NavView::Settings => AppIcon::Sliders,
    }
}
