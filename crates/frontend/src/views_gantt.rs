use crate::*;

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
                    <strong>{move || lang.get().tr("Keine Termine geplant", "No scheduled items")}</strong>
                    <span>{move || lang.get().tr("Aufgaben mit Start- oder Fälligkeitsdatum erscheinen hier.", "Tasks with a start or due date will appear here.")}</span>
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
        Some(gantt_span_left_percent(today, min_day, range))
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
                        {move || lang.get().tr("Aufgaben", "tasks")}
                        " · "
                        {milestone_count}
                        " "
                        {move || lang.get().tr("Meilensteine", "milestones")}
                    </span>
                </div>
                <span class="gantt-hint">{move || lang.get().tr("Balken anklicken zum Öffnen", "Click bars to open")}</span>
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
                    <span>{move || lang.get().tr("Zeitachse", "Timeline")}</span>
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
                    <span class="gantt-lane-label">{move || lang.get().tr("Meilensteine", "Milestones")}</span>
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
                        || if lang.get().is_de() { "ohne Fälligkeitsdatum".into() } else { "no due date".into() },
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
                                    view! { <small title=move || lang.get().tr("Hat Abhängigkeiten", "Has dependencies")>{dep_count}</small> }.into_view()
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
    let month_label = if lang.is_de() {
        MONTHS_DE[(month - 1) as usize]
    } else {
        MONTHS_EN[(month - 1) as usize]
    };
    if lang.is_de() {
        format!("{date}. {month_label} {year}")
    } else {
        format!("{month_label} {date}, {year}")
    }
}

fn gantt_month_label(year: i32, month: u32, lang: Lang) -> String {
    let month_label = if lang.is_de() {
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
    if lang.is_de() {
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

pub(crate) fn is_weekend(day: i64) -> bool {
    let weekday = (day + 3).rem_euclid(7);
    weekday >= 5
}
