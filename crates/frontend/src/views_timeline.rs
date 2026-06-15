use crate::*;

pub(crate) fn calendar_view(
    boot: BootstrapDto,
    lang: ReadSignal<Lang>,
    set_nav: WriteSignal<NavView>,
    set_open_task: WriteSignal<Option<String>>,
) -> View {
    let all_tasks = boot.tasks;
    let all_milestones = boot.milestones;
    let statuses = boot.statuses;
    let (today_year, today_month, today_day) = now_date();
    let today = today_iso();
    // Displayed month (defaults to the current one). Local to the calendar so
    // month navigation does not thread a signal through the whole dashboard.
    let (cursor, set_cursor) = create_signal((today_year, today_month));
    // Day (as a civil day-number) whose cell is expanded to show all items.
    let (expanded, set_expanded) = create_signal::<Option<i64>>(None);
    view! {
        <div class="calendar-panel">
            <div class="calendar-head">
                <button
                    class="cal-nav"
                    title=move || lang.get().tr("Voriger Monat", "Previous month")
                    aria-label=move || lang.get().tr("Voriger Monat", "Previous month")
                    on:click=move |_| {
                        set_expanded.set(None);
                        set_cursor.update(|c| *c = prev_month(c.0, c.1));
                    }
                >"\u{2039}"</button>
                <strong class="cal-month">
                    {move || {
                        let (y, m) = cursor.get();
                        format!("{} {y}", month_full(m, lang.get()))
                    }}
                </strong>
                <button
                    class="cal-nav"
                    title=move || lang.get().tr("Nächster Monat", "Next month")
                    aria-label=move || lang.get().tr("Nächster Monat", "Next month")
                    on:click=move |_| {
                        set_expanded.set(None);
                        set_cursor.update(|c| *c = next_month(c.0, c.1));
                    }
                >"\u{203A}"</button>
                <button
                    class="cal-today"
                    disabled=move || cursor.get() == (today_year, today_month)
                    on:click=move |_| {
                        set_expanded.set(None);
                        set_cursor.set((today_year, today_month));
                    }
                >{move || lang.get().tr("Heute", "Today")}</button>
            </div>
            <div class="calendar-weekdays">
                {move || {
                    let (y, m) = cursor.get();
                    let today_col = if y == today_year && m == today_month {
                        Some(calendar_weekday_index(days_from_civil(y, m, today_day)))
                    } else {
                        None
                    };
                    calendar_weekday_labels(lang.get())
                        .into_iter()
                        .enumerate()
                        .map(|(i, label)| view! { <span class:today=move || today_col == Some(i)>{label}</span> })
                        .collect_view()
                }}
            </div>
            {move || {
                let (year, month) = cursor.get();
                let exp = expanded.get();
                let lang_now = lang.get();
                let prefix = format!("{year:04}-{month:02}-");
                let month_has_items = all_tasks
                    .iter()
                    .filter_map(|t| t.due_date.as_deref())
                    .any(|d| d.starts_with(&prefix))
                    || all_milestones.iter().any(|m| m.due_date.starts_with(&prefix));
                let note = if month_has_items {
                    ().into_view()
                } else {
                    view! {
                        <div class="empty-state compact">
                            <span>
                                {if lang_now == Lang::De { "Keine Termine in diesem Monat." } else { "No events this month." }}
                            </span>
                        </div>
                    }.into_view()
                };
                let mut cells: Vec<View> = Vec::new();
                for _ in 0..calendar_month_offset(year, month) {
                    cells.push(view! { <span class="day-cell calendar-empty" aria-hidden="true"></span> }.into_view());
                }
                for day in 1..=days_in_month(year, month) {
                    let iso = format!("{year:04}-{month:02}-{day:02}");
                    let day_number = days_from_civil(year, month, day);
                    let items = calendar_items_for_day(&iso, &all_tasks, &all_milestones);
                    let total = items.len();
                    let is_empty = items.is_empty();
                    let is_today = day == today_day && year == today_year && month == today_month;
                    let is_expanded = exp == Some(day_number);
                    let visible = if is_expanded { total } else { CALENDAR_VISIBLE_ITEMS };
                    let hidden_count = total.saturating_sub(CALENDAR_VISIBLE_ITEMS);
                    let chips = items
                        .into_iter()
                        .take(visible)
                        .map(|item| match item {
                            CalendarItem::Task(task) => {
                                let task_id = task.id.clone();
                                let label = task_title(&task, lang_now);
                                let overdue = !task.status_is_done && iso.as_str() < today.as_str();
                                let color = if overdue {
                                    "var(--bad)".to_string()
                                } else {
                                    status_color(&statuses, &task.status_id)
                                };
                                let mut cls = String::from("cal-chip");
                                if task.status_is_done {
                                    cls.push_str(" done");
                                }
                                if overdue {
                                    cls.push_str(" overdue");
                                }
                                view! {
                                    <button class=cls style=format!("--cal-color:{color}") title=label.clone() on:click=move |_| set_open_task.set(Some(task_id.clone()))>
                                        <span>{label}</span>
                                    </button>
                                }.into_view()
                            }
                            CalendarItem::Milestone(milestone) => {
                                let label = title_for(milestone.title, milestone.title_en, lang_now);
                                let mut cls = String::from("cal-chip milestone");
                                if milestone.done {
                                    cls.push_str(" done");
                                }
                                let aria = if lang_now == Lang::De {
                                    format!("Meilenstein {label}, im Gantt öffnen")
                                } else {
                                    format!("Milestone {label}, open in Gantt")
                                };
                                view! {
                                    <button class=cls title=label.clone() aria-label=aria on:click=move |_| set_nav.set(NavView::Gantt)>
                                        <b>"\u{25C7}"</b>
                                        <span>{label}</span>
                                    </button>
                                }.into_view()
                            }
                        })
                        .collect_view();
                    let overflow = if total <= CALENDAR_VISIBLE_ITEMS {
                        ().into_view()
                    } else if is_expanded {
                        view! {
                            <button class="cal-more" on:click=move |_| set_expanded.set(None)>
                                {if lang_now == Lang::De { "weniger" } else { "less" }}
                            </button>
                        }.into_view()
                    } else {
                        view! {
                            <button class="cal-more" on:click=move |_| set_expanded.set(Some(day_number))>
                                "+" {hidden_count} " " {if lang_now == Lang::De { "weitere" } else { "more" }}
                            </button>
                        }.into_view()
                    };
                    let mut cls = String::from("day-cell");
                    if is_today {
                        cls.push_str(" today");
                    }
                    if calendar_is_weekend(day_number) {
                        cls.push_str(" weekend");
                    }
                    if is_empty {
                        cls.push_str(" is-empty");
                    }
                    let head_today = if is_today {
                        view! { <small>{if lang_now == Lang::De { "Heute" } else { "Today" }}</small> }.into_view()
                    } else {
                        ().into_view()
                    };
                    cells.push(view! {
                        <div class=cls>
                            <header class="day-cell-head">
                                <strong>{day}</strong>
                                {head_today}
                            </header>
                            <div class="calendar-items">
                                {chips}
                                {overflow}
                            </div>
                        </div>
                    }.into_view());
                }
                view! { {note} <div class="calendar-grid">{cells}</div> }.into_view()
            }}
        </div>
    }.into_view()
}

const CALENDAR_VISIBLE_ITEMS: usize = 4;

/// Previous calendar month, wrapping the year at January.
pub(crate) fn prev_month(year: i32, month: u32) -> (i32, u32) {
    if month == 1 {
        (year - 1, 12)
    } else {
        (year, month - 1)
    }
}

/// Next calendar month, wrapping the year at December.
pub(crate) fn next_month(year: i32, month: u32) -> (i32, u32) {
    if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    }
}

fn month_full(month: u32, lang: Lang) -> &'static str {
    let i = (month - 1) as usize;
    if lang.is_de() {
        MONTHS_DE_FULL[i]
    } else {
        MONTHS_EN_FULL[i]
    }
}

#[derive(Debug, Clone)]
enum CalendarItem {
    Task(Box<TaskDto>),
    Milestone(Box<MilestoneDto>),
}

fn calendar_items_for_day(
    iso: &str,
    tasks: &[TaskDto],
    milestones: &[MilestoneDto],
) -> Vec<CalendarItem> {
    let mut items = Vec::new();
    items.extend(
        tasks
            .iter()
            .filter(|task| task.due_date.as_deref() == Some(iso))
            .cloned()
            .map(Box::new)
            .map(CalendarItem::Task),
    );
    items.extend(
        milestones
            .iter()
            .filter(|milestone| milestone.due_date == iso)
            .cloned()
            .map(Box::new)
            .map(CalendarItem::Milestone),
    );
    items.sort_by_key(calendar_item_sort_key);
    items
}

fn calendar_item_sort_key(item: &CalendarItem) -> (u8, bool, String) {
    match item {
        CalendarItem::Milestone(milestone) => (
            0,
            milestone.done,
            title_for(
                milestone.title.clone(),
                milestone.title_en.clone(),
                Lang::De,
            ),
        ),
        CalendarItem::Task(task) => (1, task.status_is_done, task.key.clone()),
    }
}

pub(crate) fn calendar_month_offset(year: i32, month: u32) -> usize {
    calendar_weekday_index(days_from_civil(year, month, 1))
}

pub(crate) fn calendar_weekday_index(day: i64) -> usize {
    (day + 3).rem_euclid(7) as usize
}

pub(crate) fn calendar_is_weekend(day: i64) -> bool {
    calendar_weekday_index(day) >= 5
}

fn calendar_weekday_labels(lang: Lang) -> [&'static str; 7] {
    if lang.is_de() {
        ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"]
    } else {
        ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
    }
}
pub(crate) fn roadmap_view(
    boot: BootstrapDto,
    lang: ReadSignal<Lang>,
    set_open_task: WriteSignal<Option<String>>,
) -> View {
    let lang_now = lang.get();
    let all_tasks = boot.tasks;
    view! {
        <div class="roadmap-grid">
            {PHASES.into_iter().map(|(phase, de, en)| {
                let label = if lang_now.is_de() { de } else { en };
                let tasks = all_tasks.iter().filter(|t| t.phase == phase).cloned().collect::<Vec<_>>();
                let done = tasks.iter().filter(|t| t.status_is_done).count();
                let pct = if tasks.is_empty() { 0 } else { done * 100 / tasks.len() };
                view! {
                    <section class="road-card">
                        <header><h3>{label}</h3><small>{pct}"%"</small></header>
                        <span class="bar"><i style=format!("width:{pct}%")></i></span>
                        {tasks.into_iter().map(|task| {
                            let task_id = task.id.clone();
                            let title = task_title(&task, lang.get());
                            view! { <button on:click=move |_| set_open_task.set(Some(task_id.clone()))>{title}</button> }
                        }).collect_view()}
                    </section>
                }
            }).collect_view()}
        </div>
    }.into_view()
}
pub(crate) fn team_view(boot: BootstrapDto, lang: ReadSignal<Lang>) -> View {
    view! {
        <div class="team-grid">
            {boot.members.iter().map(|m| view! {
                <article class="member-card">
                    <span class="avatar large">{m.initials.clone()}</span>
                    <div>
                        <h3>{m.name.clone()}</h3>
                        <p>{role_label(&m.role, lang.get())}</p>
                        <small>
                            <strong>{m.open_tasks}</strong>
                            {move || lang.get().tr(" offen", " open")}
                            " / "
                            <strong>{m.done_tasks}</strong>
                            {move || lang.get().tr(" fertig", " done")}
                        </small>
                    </div>
                </article>
            }).collect_view()}
        </div>
    }
    .into_view()
}
