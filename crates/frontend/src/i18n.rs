use crate::*;

pub(crate) fn task_title(task: &TaskDto, lang: Lang) -> String {
    title_for(task.title.clone(), task.title_en.clone(), lang)
}

pub(crate) fn description_for(task: &TaskDto, lang: Lang) -> String {
    title_for(task.description.clone(), task.description_en.clone(), lang)
}

pub(crate) fn title_for(de: String, en: Option<String>, lang: Lang) -> String {
    if lang == Lang::En {
        en.unwrap_or(de)
    } else {
        de
    }
}

pub(crate) fn status_name(status: &StatusDto, lang: Lang) -> &'_ str {
    if lang.is_de() {
        &status.name_de
    } else {
        &status.name_en
    }
}

pub(crate) fn status_color(statuses: &[StatusDto], status_id: &str) -> String {
    statuses
        .iter()
        .find(|s| s.id == status_id)
        .map_or_else(|| "#6b8aa6".into(), |s| s.color.clone())
}

pub(crate) fn priority_label(priority: &Priority, lang: Lang) -> &'static str {
    match (priority, lang) {
        (Priority::Urgent, Lang::De) => "Dringend",
        (Priority::Urgent, Lang::En) => "Urgent",
        (Priority::High, Lang::De) => "Hoch",
        (Priority::High, Lang::En) => "High",
        (Priority::Medium, Lang::De) => "Mittel",
        (Priority::Medium, Lang::En) => "Medium",
        (Priority::Low, Lang::De) => "Niedrig",
        (Priority::Low, Lang::En) => "Low",
    }
}

pub(crate) fn priority_class(priority: &Priority) -> &'static str {
    match priority {
        Priority::Urgent => "urgent",
        Priority::High => "high",
        Priority::Medium => "medium",
        Priority::Low => "low",
    }
}

pub(crate) fn priority_from_value(value: &str) -> Priority {
    match value {
        "urgent" => Priority::Urgent,
        "high" => Priority::High,
        "low" => Priority::Low,
        _ => Priority::Medium,
    }
}

pub(crate) fn recurrence_from_value(value: &str) -> Option<Recurrence> {
    match value {
        "daily" => Some(Recurrence::Daily),
        "weekly" => Some(Recurrence::Weekly),
        "biweekly" => Some(Recurrence::Biweekly),
        "monthly" => Some(Recurrence::Monthly),
        _ => None,
    }
}

pub(crate) fn recurrence_value(recurrence: Option<&Recurrence>) -> &'static str {
    match recurrence {
        Some(Recurrence::Daily) => "daily",
        Some(Recurrence::Weekly) => "weekly",
        Some(Recurrence::Biweekly) => "biweekly",
        Some(Recurrence::Monthly) => "monthly",
        None => "",
    }
}

pub(crate) fn recurrence_label(recurrence: Option<&Recurrence>, lang: Lang) -> &'static str {
    match (recurrence, lang) {
        (Some(Recurrence::Daily), Lang::De) => "Täglich",
        (Some(Recurrence::Daily), Lang::En) => "Daily",
        (Some(Recurrence::Weekly), Lang::De) => "Wöchentlich",
        (Some(Recurrence::Weekly), Lang::En) => "Weekly",
        (Some(Recurrence::Biweekly), Lang::De) => "Alle 2 Wochen",
        (Some(Recurrence::Biweekly), Lang::En) => "Every 2 weeks",
        (Some(Recurrence::Monthly), Lang::De) => "Monatlich",
        (Some(Recurrence::Monthly), Lang::En) => "Monthly",
        (None, Lang::De) => "Keine Wiederholung",
        (None, Lang::En) => "No repeat",
    }
}

/// The recurrence options for a task select, with `current` preselected.
pub(crate) fn recurrence_options(current: Option<Recurrence>, lang: ReadSignal<Lang>) -> View {
    [
        None,
        Some(Recurrence::Daily),
        Some(Recurrence::Weekly),
        Some(Recurrence::Biweekly),
        Some(Recurrence::Monthly),
    ]
    .into_iter()
    .map(|option| {
        let selected = option == current;
        let value = recurrence_value(option.as_ref());
        view! {
            <option value=value selected=selected>
                {move || recurrence_label(option.as_ref(), lang.get())}
            </option>
        }
    })
    .collect_view()
}

pub(crate) fn priority_value(priority: &Priority) -> &'static str {
    match priority {
        Priority::Urgent => "urgent",
        Priority::High => "high",
        Priority::Medium => "medium",
        Priority::Low => "low",
    }
}

/// Priority `<option>`s with `current` preselected, labels following `lang`.
pub(crate) fn priority_options(current: Priority, lang: ReadSignal<Lang>) -> View {
    [
        Priority::Urgent,
        Priority::High,
        Priority::Medium,
        Priority::Low,
    ]
    .into_iter()
    .map(|option| {
        let selected = option == current;
        let value = priority_value(&option);
        view! {
            <option value=value selected=selected>
                {move || priority_label(&option, lang.get())}
            </option>
        }
    })
    .collect_view()
}

pub(crate) fn ticket_status_value(status: &TicketStatus) -> &'static str {
    match status {
        TicketStatus::Open => "open",
        TicketStatus::InProgress => "in_progress",
        TicketStatus::Resolved => "resolved",
        TicketStatus::Closed => "closed",
    }
}

/// Ticket-status `<option>`s with `current` preselected, labels following `lang`.
pub(crate) fn ticket_status_options(current: TicketStatus, lang: ReadSignal<Lang>) -> View {
    [
        TicketStatus::Open,
        TicketStatus::InProgress,
        TicketStatus::Resolved,
        TicketStatus::Closed,
    ]
    .into_iter()
    .map(|option| {
        let selected = option == current;
        let value = ticket_status_value(&option);
        view! {
            <option value=value selected=selected>
                {move || ticket_status_label(&option, lang.get())}
            </option>
        }
    })
    .collect_view()
}

/// Canonical project phases in roadmap order, with their `(value, DE, EN)` labels.
pub(crate) const PHASES: [(&str, &str, &str); 4] = [
    ("planung", "Planung", "Planning"),
    ("vergabe", "Vergabe", "Tendering"),
    ("ausfuehrung", "Ausführung", "Execution"),
    ("abnahme", "Abnahme", "Handover"),
];

/// Phase `<option>`s with `current` preselected, labels following `lang`.
pub(crate) fn phase_options(current: String, lang: ReadSignal<Lang>) -> View {
    PHASES
        .into_iter()
        .map(|(value, de, en)| {
            let selected = value == current;
            view! {
                <option value=value selected=selected>
                    {move || if lang.get().is_de() { de } else { en }}
                </option>
            }
        })
        .collect_view()
}

pub(crate) fn role_from_value(value: &str) -> Role {
    match value {
        "owner" => Role::Owner,
        "admin" => Role::Admin,
        "viewer" => Role::Viewer,
        _ => Role::Member,
    }
}

pub(crate) fn ticket_status_from_value(value: &str) -> TicketStatus {
    match value {
        "in_progress" => TicketStatus::InProgress,
        "resolved" => TicketStatus::Resolved,
        "closed" => TicketStatus::Closed,
        _ => TicketStatus::Open,
    }
}

pub(crate) fn ticket_status_label(status: &TicketStatus, lang: Lang) -> &'static str {
    match (status, lang) {
        (TicketStatus::Open, Lang::De) => "Offen",
        (TicketStatus::Open, Lang::En) => "Open",
        (TicketStatus::InProgress, Lang::De) => "In Arbeit",
        (TicketStatus::InProgress, Lang::En) => "In progress",
        (TicketStatus::Resolved, Lang::De) => "Geloest",
        (TicketStatus::Resolved, Lang::En) => "Resolved",
        (TicketStatus::Closed, Lang::De) => "Geschlossen",
        (TicketStatus::Closed, Lang::En) => "Closed",
    }
}

pub(crate) fn ticket_status_class(status: &TicketStatus) -> &'static str {
    match status {
        TicketStatus::Open => "open",
        TicketStatus::InProgress => "active",
        TicketStatus::Resolved => "resolved",
        TicketStatus::Closed => "closed",
    }
}

pub(crate) fn role_label(role: &Role, lang: Lang) -> &'static str {
    match (role, lang) {
        (Role::Owner, Lang::De) => "Owner",
        (Role::Owner, Lang::En) => "Owner",
        (Role::Admin, Lang::De) => "Admin",
        (Role::Admin, Lang::En) => "Admin",
        (Role::Member, Lang::De) => "Mitglied",
        (Role::Member, Lang::En) => "Member",
        (Role::Viewer, Lang::De) => "Betrachter",
        (Role::Viewer, Lang::En) => "Viewer",
    }
}

pub(crate) fn notif_text(n: &NotificationDto, lang: Lang) -> String {
    if lang == Lang::En {
        n.text_en.clone().unwrap_or_else(|| "updated".into())
    } else {
        n.text.clone().unwrap_or_else(|| "hat aktualisiert".into())
    }
}

pub(crate) fn first_name(name: &str) -> &str {
    name.split_whitespace().next().unwrap_or(name)
}

pub(crate) fn initials(name: &str) -> String {
    display_initials(name)
}

/// Local current date as (year, month 1-12, day 1-31).
pub(crate) fn now_date() -> (i32, u32, u32) {
    let d = js_sys::Date::new_0();
    (d.get_full_year() as i32, d.get_month() + 1, d.get_date())
}

pub(crate) fn today_iso() -> String {
    let (y, m, d) = now_date();
    format!("{y:04}-{m:02}-{d:02}")
}

pub(crate) fn iso_in_days(days: i64) -> String {
    let ms = (days as f64).mul_add(86_400_000.0, js_sys::Date::now());
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ms));
    format!(
        "{:04}-{:02}-{:02}",
        d.get_full_year(),
        d.get_month() + 1,
        d.get_date()
    )
}

pub(crate) fn parse_iso(iso: &str) -> Option<(i32, u32, u32)> {
    let mut parts = iso.split('-');
    let y = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    let d: u32 = parts.next()?.parse().ok()?;
    ((1..=12).contains(&m) && (1..=31).contains(&d)).then_some((y, m, d))
}

pub(crate) fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => 31,
    }
}

/// Days since 1970-01-01 (Howard Hinnant's civil-calendar algorithm).
pub(crate) fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = i64::from(y) - i64::from(m <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * ((i64::from(m) + 9) % 12) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Inverse of `days_from_civil`.
pub(crate) fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    ((if m <= 2 { y + 1 } else { y }) as i32, m, d)
}

pub(crate) fn iso_day_number(iso: &str) -> Option<i64> {
    parse_iso(iso).map(|(y, m, d)| days_from_civil(y, m, d))
}

pub(crate) const MONTHS_DE: [&str; 12] = [
    "Jan", "Feb", "Mär", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez",
];
pub(crate) const MONTHS_EN: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
pub(crate) const MONTHS_DE_FULL: [&str; 12] = [
    "Januar",
    "Februar",
    "März",
    "April",
    "Mai",
    "Juni",
    "Juli",
    "August",
    "September",
    "Oktober",
    "November",
    "Dezember",
];
pub(crate) const MONTHS_EN_FULL: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

pub(crate) fn fmt_date(iso: &str, lang: Lang) -> String {
    let Some((_, m, d)) = parse_iso(iso) else {
        return iso.to_string();
    };
    let month = if lang.is_de() {
        MONTHS_DE[(m - 1) as usize]
    } else {
        MONTHS_EN[(m - 1) as usize]
    };
    if lang.is_de() {
        format!("{d}. {month}")
    } else {
        format!("{month} {d}")
    }
}

pub(crate) fn select_value(ev: &web_sys::Event) -> String {
    event_target::<HtmlSelectElement>(ev).value()
}

pub(crate) fn textarea_value(ev: &web_sys::Event) -> String {
    event_target::<HtmlTextAreaElement>(ev).value()
}
