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
pub(crate) fn gantt_view(
    boot: BootstrapDto,
    lang: ReadSignal<Lang>,
    set_open_task: WriteSignal<Option<String>>,
) -> View {
    let statuses = boot.statuses.clone();
    let mut tasks: Vec<ScheduledTask> = boot
        .tasks
        .into_iter()
        .filter_map(|task| {
            let (start, due) =
                scheduled_task_days(task.start_date.as_deref(), task.due_date.as_deref())?;
            Some(ScheduledTask { task, start, due })
        })
        .collect();
    let mut milestones: Vec<ScheduledMilestone> = boot
        .milestones
        .into_iter()
        .filter_map(|milestone| {
            let day = iso_day_number(&milestone.due_date)?;
            Some(ScheduledMilestone { milestone, day })
        })
        .collect();
    tasks.sort_by(|a, b| {
        a.start
            .cmp(&b.start)
            .then(a.due.cmp(&b.due))
            .then(a.task.key.cmp(&b.task.key))
    });
    milestones.sort_by(|a, b| {
        a.day
            .cmp(&b.day)
            .then(a.milestone.due_date.cmp(&b.milestone.due_date))
    });

    if tasks.is_empty() && milestones.is_empty() {
        return view! {
            <div class="gantt-panel">
                <div class="empty-state compact">
                    <strong>{move || if lang.get() == Lang::De { "Keine Termine geplant" } else { "No scheduled items" }}</strong>
                    <span>{move || if lang.get() == Lang::De { "Aufgaben mit Start- oder Fälligkeitsdatum erscheinen hier." } else { "Tasks with a start or due date will appear here." }}</span>
                </div>
            </div>
        }.into_view();
    }

    let (min_day, max_day) = timeline_bounds(
        tasks.iter().map(|task| (task.start, task.due)),
        milestones.iter().map(|milestone| milestone.day),
    )
    .unwrap_or_else(|| {
        let today = iso_day_number(&today_iso()).unwrap_or(0);
        (today, today)
    });
    let range = (max_day - min_day + 1).max(1) as usize;
    let chart_min_width = range * GANTT_DAY_MIN_WIDTH;
    let row_min_width = GANTT_LABEL_WIDTH + chart_min_width;
    let scroll_min_width = row_min_width + (GANTT_SCROLL_PADDING_X * 2);
    let chart_columns = format!("minmax({chart_min_width}px, 1fr)");
    let day_columns = format!("repeat({range}, minmax({GANTT_DAY_MIN_WIDTH}px, 1fr))");
    let month_segments = gantt_month_segments(min_day, range);
    let month_columns = month_segments
        .iter()
        .map(|segment| format!("{}fr", segment.days))
        .collect::<Vec<_>>()
        .join(" ");
    let today = iso_day_number(&today_iso()).unwrap_or(i64::MIN);
    let today_left = if (min_day..=max_day).contains(&today) {
        Some(gantt_day_center_percent(today, min_day, range))
    } else {
        None
    };
    let range_label = format!(
        "{} - {}",
        gantt_day_label(min_day, lang.get()),
        gantt_day_label(max_day, lang.get())
    );
    let task_count = tasks.len();
    let milestone_count = milestones.len();
    view! {
        <div class="gantt-panel">
            <div class="gantt-toolbar">
                <div>
                    <strong>{range_label}</strong>
                    <span>
                        {task_count}
                        " "
                        {move || if lang.get() == Lang::De { "Aufgaben" } else { "tasks" }}
                        " · "
                        {milestone_count}
                        " "
                        {move || if lang.get() == Lang::De { "Meilensteine" } else { "milestones" }}
                    </span>
                </div>
                <span class="gantt-hint">{move || if lang.get() == Lang::De { "Balken anklicken zum Öffnen" } else { "Click bars to open" }}</span>
            </div>
            <div class="gantt-scroll" style=format!("--gantt-range:{range};min-width:{scroll_min_width}px")>
                <div class="gantt-months" style=format!("grid-template-columns:{GANTT_LABEL_WIDTH}px {chart_columns}")>
                    <span></span>
                    <div class="gantt-month-track" style=format!("grid-template-columns:{month_columns}")>
                        {month_segments.iter().map(|segment| {
                            let label = gantt_month_label(segment.year, segment.month, lang.get());
                            view! { <span>{label}</span> }
                        }).collect_view()}
                    </div>
                </div>
                <div class="gantt-scale" style=format!("grid-template-columns:{GANTT_LABEL_WIDTH}px {day_columns}")>
                    <span>{move || if lang.get() == Lang::De { "Zeitachse" } else { "Timeline" }}</span>
                    {(0..range).map(|i| {
                        let day = min_day + i as i64;
                        let (_, _, d) = civil_from_days(day);
                        let weekday = gantt_weekday_label(day, lang.get());
                        let class_name = match (day == today, is_weekend(day)) {
                            (true, true) => "today weekend",
                            (true, false) => "today",
                            (false, true) => "weekend",
                            (false, false) => "",
                        };
                        let date_title = gantt_day_label(day, lang.get());
                        view! { <span class=class_name title=date_title><b>{d}</b><small>{weekday}</small></span> }
                    }).collect_view()}
                </div>
                <div class="gantt-milestones" style=format!("grid-template-columns:{GANTT_LABEL_WIDTH}px {chart_columns}")>
                    <span class="gantt-lane-label">{move || if lang.get() == Lang::De { "Meilensteine" } else { "Milestones" }}</span>
                    <div class="gantt-track">
                        {today_left.map(|left| view! { <span class="gantt-today" style=format!("left:{left:.4}%")></span> })}
                        {milestones.into_iter().map(|scheduled| {
                            let left = gantt_day_center_percent(scheduled.day, min_day, range);
                            let title = title_for(scheduled.milestone.title, scheduled.milestone.title_en, lang.get());
                            let date = fmt_date(&scheduled.milestone.due_date, lang.get());
                            let class_name = match (scheduled.milestone.done, scheduled.day + 2 >= max_day) {
                                (true, true) => "gantt-milestone done edge",
                                (true, false) => "gantt-milestone done",
                                (false, true) => "gantt-milestone edge",
                                (false, false) => "gantt-milestone",
                            };
                            view! {
                                <span class=class_name style=format!("left:{left:.4}%") title=format!("{title} - {date}")>
                                    <i></i>
                                    <b>{title}</b>
                                    <small>{date}</small>
                                </span>
                            }
                        }).collect_view()}
                    </div>
                </div>
                {tasks.into_iter().map(|scheduled| {
                    let start = scheduled.start;
                    let due = scheduled.due;
                    let left = gantt_span_left_percent(start, min_day, range);
                    let width = gantt_span_width_percent(start, due, range);
                    let duration_days = (due - start + 1).max(1) as usize;
                    let task = scheduled.task;
                    let task_id = task.id.clone();
                    let key = task.key.clone();
                    let compact_key = compact_task_key(&key);
                    let title = task_title(&task, lang.get());
                    let color = status_color(&statuses, &task.status_id);
                    let status_label = statuses.iter().find(|s| s.id == task.status_id).map(|s| status_name(s, lang.get()).to_string()).unwrap_or_default();
                    let date_label = task.due_date.as_deref().map_or_else(
                        || if lang.get() == Lang::De { "ohne Fälligkeitsdatum".into() } else { "no due date".into() },
                        |date| fmt_date(date, lang.get()),
                    );
                    let bar_class = if duration_days <= 1 { "gantt-bar compact" } else { "gantt-bar" };
                    let dep_count = task.dependency_ids.len();
                    view! {
                        <button class="gantt-row" style=format!("grid-template-columns:{GANTT_LABEL_WIDTH}px {chart_columns}") on:click=move |_| set_open_task.set(Some(task_id.clone()))>
                            <span class="gantt-key">
                                <span class="gantt-key-main">
                                    <i style=format!("background:{color}")></i>
                                    <b>{key}</b>
                                    <strong>{title.clone()}</strong>
                                </span>
                                <span class="gantt-key-meta">
                                    <em>{status_label}</em>
                                    <em>{date_label}</em>
                                </span>
                                {if dep_count > 0 {
                                    view! { <small title=move || if lang.get() == Lang::De { "Hat Abhängigkeiten" } else { "Has dependencies" }>{dep_count}</small> }.into_view()
                                } else {
                                    ().into_view()
                                }}
                            </span>
                            <span class="gantt-track">
                                {today_left.map(|left| view! { <span class="gantt-today" style=format!("left:{left:.4}%")></span> })}
                                <i class=bar_class style=format!("left:{left:.4}%;width:{width:.4}%;background:{color}") title=title.clone()>
                                    <b>{title}</b>
                                    <small>{compact_key}</small>
                                </i>
                            </span>
                        </button>
                    }
                }).collect_view()}
            </div>
        </div>
    }.into_view()
}

const GANTT_DAY_MIN_WIDTH: usize = 54;
const GANTT_LABEL_WIDTH: usize = 320;
const GANTT_SCROLL_PADDING_X: usize = 22;

#[derive(Debug, Clone)]
struct ScheduledTask {
    task: TaskDto,
    start: i64,
    due: i64,
}

#[derive(Debug, Clone)]
struct ScheduledMilestone {
    milestone: MilestoneDto,
    day: i64,
}

pub(crate) fn scheduled_task_days(start: Option<&str>, due: Option<&str>) -> Option<(i64, i64)> {
    let start = start.and_then(iso_day_number);
    let due = due.and_then(iso_day_number);
    match (start, due) {
        (Some(start), Some(due)) => Some((start.min(due), start.max(due))),
        (Some(day), None) | (None, Some(day)) => Some((day, day)),
        (None, None) => None,
    }
}

pub(crate) fn timeline_bounds(
    task_ranges: impl IntoIterator<Item = (i64, i64)>,
    milestone_days: impl IntoIterator<Item = i64>,
) -> Option<(i64, i64)> {
    let mut bounds: Option<(i64, i64)> = None;
    for (start, due) in task_ranges {
        bounds = Some(match bounds {
            Some((min, max)) => (min.min(start), max.max(due)),
            None => (start, due),
        });
    }
    for day in milestone_days {
        bounds = Some(match bounds {
            Some((min, max)) => (min.min(day), max.max(day)),
            None => (day, day),
        });
    }
    bounds
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GanttMonthSegment {
    pub year: i32,
    pub month: u32,
    pub days: usize,
}

pub(crate) fn gantt_month_segments(min_day: i64, range: usize) -> Vec<GanttMonthSegment> {
    let mut segments: Vec<GanttMonthSegment> = Vec::new();
    for offset in 0..range {
        let day = min_day + offset as i64;
        let (year, month, _) = civil_from_days(day);
        match segments.last_mut() {
            Some(segment) if segment.year == year && segment.month == month => {
                segment.days += 1;
            }
            _ => segments.push(GanttMonthSegment {
                year,
                month,
                days: 1,
            }),
        }
    }
    segments
}

fn gantt_day_label(day: i64, lang: Lang) -> String {
    let (year, month, date) = civil_from_days(day);
    let month_label = if lang == Lang::De {
        MONTHS_DE[(month - 1) as usize]
    } else {
        MONTHS_EN[(month - 1) as usize]
    };
    if lang == Lang::De {
        format!("{date}. {month_label} {year}")
    } else {
        format!("{month_label} {date}, {year}")
    }
}

fn gantt_month_label(year: i32, month: u32, lang: Lang) -> String {
    let month_label = if lang == Lang::De {
        MONTHS_DE_FULL[(month - 1) as usize]
    } else {
        MONTHS_EN_FULL[(month - 1) as usize]
    };
    format!("{month_label} {year}")
}

fn gantt_weekday_label(day: i64, lang: Lang) -> &'static str {
    const WEEKDAYS_DE: [&str; 7] = ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"];
    const WEEKDAYS_EN: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    let index = (day + 3).rem_euclid(7) as usize;
    if lang == Lang::De {
        WEEKDAYS_DE[index]
    } else {
        WEEKDAYS_EN[index]
    }
}

fn gantt_day_center_percent(day: i64, min_day: i64, range: usize) -> f64 {
    ((((day - min_day).max(0) as f64) + 0.5) / range.max(1) as f64) * 100.0
}

fn gantt_span_left_percent(start: i64, min_day: i64, range: usize) -> f64 {
    ((start - min_day).max(0) as f64 / range.max(1) as f64) * 100.0
}

fn gantt_span_width_percent(start: i64, due: i64, range: usize) -> f64 {
    ((due - start + 1).max(1) as f64 / range.max(1) as f64) * 100.0
}

fn compact_task_key(key: &str) -> String {
    if let Some((_, suffix)) = key.rsplit_once('-') {
        if !suffix.is_empty() {
            return suffix.to_string();
        }
    }
    key.to_string()
}

fn is_weekend(day: i64) -> bool {
    let weekday = (day + 4).rem_euclid(7);
    weekday >= 5
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
