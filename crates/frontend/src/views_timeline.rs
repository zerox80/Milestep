use crate::*;

pub(crate) fn calendar_view(
    boot: BootstrapDto,
    lang: ReadSignal<Lang>,
    set_open_task: WriteSignal<Option<String>>,
) -> View {
    let all_tasks = boot.tasks;
    let all_milestones = boot.milestones;
    let statuses = boot.statuses;
    let (year, month, today_day) = now_date();
    let first_day_offset = calendar_month_offset(year, month);
    let weekday_labels = calendar_weekday_labels(lang.get());
    view! {
        <div class="calendar-panel">
            <div class="calendar-weekdays">
                {weekday_labels.into_iter().map(|label| view! { <span>{label}</span> }).collect_view()}
            </div>
            <div class="calendar-grid">
                {(0..first_day_offset).map(|_| view! { <span class="day-cell calendar-empty" aria-hidden="true"></span> }).collect_view()}
            {(1..=days_in_month(year, month)).map(|day| {
                let iso = format!("{year:04}-{month:02}-{day:02}");
                let day_number = days_from_civil(year, month, day);
                let items = calendar_items_for_day(&iso, &all_tasks, &all_milestones);
                let hidden_count = items.len().saturating_sub(CALENDAR_VISIBLE_ITEMS);
                view! {
                    <div class="day-cell" class:today=move || day == today_day class:weekend=move || calendar_is_weekend(day_number)>
                        <header class="day-cell-head">
                            <strong>{day}</strong>
                            {if day == today_day {
                                view! { <small>{move || if lang.get() == Lang::De { "Heute" } else { "Today" }}</small> }.into_view()
                            } else {
                                ().into_view()
                            }}
                        </header>
                        <div class="calendar-items">
                            {items.into_iter().take(CALENDAR_VISIBLE_ITEMS).map(|item| {
                                match item {
                                    CalendarItem::Task(task) => {
                                        let task_id = task.id.clone();
                                        let label = task_title(&task, lang.get());
                                        let color = status_color(&statuses, &task.status_id);
                                        let class_name = if task.status_is_done { "cal-chip done" } else { "cal-chip" };
                                        view! {
                                            <button class=class_name style=format!("--cal-color:{color}") title=label.clone() on:click=move |_| set_open_task.set(Some(task_id.clone()))>
                                                <span>{label}</span>
                                            </button>
                                        }.into_view()
                                    }
                                    CalendarItem::Milestone(milestone) => {
                                        let label = title_for(milestone.title, milestone.title_en, lang.get());
                                        let class_name = if milestone.done { "cal-chip milestone done" } else { "cal-chip milestone" };
                                        view! {
                                            <span class=class_name title=label.clone()>
                                                <b>"\u{25C7}"</b>
                                                <span>{label}</span>
                                            </span>
                                        }.into_view()
                                    }
                                }
                            }).collect_view()}
                            {if hidden_count > 0 {
                                view! {
                                    <span class="cal-more">
                                        "+"
                                        {hidden_count}
                                        " "
                                        {move || if lang.get() == Lang::De { "weitere" } else { "more" }}
                                    </span>
                                }.into_view()
                            } else {
                                ().into_view()
                            }}
                        </div>
                    </div>
                }
            }).collect_view()}
            </div>
        </div>
    }.into_view()
}

const CALENDAR_VISIBLE_ITEMS: usize = 4;

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
    if lang == Lang::De {
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
    let phases = [
        (
            "planung",
            if lang.get() == Lang::De {
                "Planung"
            } else {
                "Planning"
            },
        ),
        (
            "vergabe",
            if lang.get() == Lang::De {
                "Vergabe"
            } else {
                "Tendering"
            },
        ),
        (
            "ausfuehrung",
            if lang.get() == Lang::De {
                "Ausführung"
            } else {
                "Execution"
            },
        ),
        (
            "abnahme",
            if lang.get() == Lang::De {
                "Abnahme"
            } else {
                "Handover"
            },
        ),
    ];
    let all_tasks = boot.tasks;
    view! {
        <div class="roadmap-grid">
            {phases.into_iter().map(|(phase, label)| {
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
                            {move || if lang.get() == Lang::De { " offen" } else { " open" }}
                            " / "
                            <strong>{m.done_tasks}</strong>
                            {move || if lang.get() == Lang::De { " fertig" } else { " done" }}
                        </small>
                    </div>
                </article>
            }).collect_view()}
        </div>
    }
    .into_view()
}
