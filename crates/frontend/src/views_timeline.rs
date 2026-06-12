use crate::*;

pub(crate) fn calendar_view(
    boot: BootstrapDto,
    lang: ReadSignal<Lang>,
    set_open_task: WriteSignal<Option<String>>,
) -> View {
    let all_tasks = boot.tasks;
    let (year, month, today_day) = now_date();
    view! {
        <div class="calendar-grid">
            {(1..=days_in_month(year, month)).map(|day| {
                let iso = format!("{year:04}-{month:02}-{day:02}");
                let tasks = all_tasks.iter().filter(|t| t.due_date.as_deref() == Some(iso.as_str())).cloned();
                view! {
                    <div class="day-cell" class:today=move || day == today_day>
                        <strong>{day}</strong>
                        {tasks.take(3).map(|task| {
                            let task_id = task.id.clone();
                            let label = task_title(&task, lang.get());
                            view! { <button class="cal-chip" on:click=move |_| set_open_task.set(Some(task_id.clone()))>{label}</button> }
                        }).collect_view()}
                    </div>
                }
            }).collect_view()}
        </div>
    }.into_view()
}
pub(crate) fn gantt_view(
    boot: BootstrapDto,
    lang: ReadSignal<Lang>,
    set_open_task: WriteSignal<Option<String>>,
) -> View {
    let statuses = boot.statuses.clone();
    let tasks: Vec<TaskDto> = boot
        .tasks
        .into_iter()
        .filter(|t| {
            t.start_date.as_deref().and_then(iso_day_number).is_some()
                && t.due_date.as_deref().and_then(iso_day_number).is_some()
        })
        .collect();
    // Day window spanning all scheduled tasks; positions are day offsets from its start.
    let min_day = tasks
        .iter()
        .filter_map(|t| t.start_date.as_deref().and_then(iso_day_number))
        .min()
        .unwrap_or_else(|| iso_day_number(&today_iso()).unwrap_or(0));
    let max_day = tasks
        .iter()
        .filter_map(|t| t.due_date.as_deref().and_then(iso_day_number))
        .max()
        .unwrap_or(min_day);
    let range = (max_day - min_day + 1).max(1) as usize;
    let row_width = 70 + range * 44;
    view! {
        <div class="gantt-panel">
            <div class="gantt-scale" style=format!("grid-template-columns: repeat({range}, 44px)")>
                {(0..range).map(|i| {
                    let (_, _, d) = civil_from_days(min_day + i as i64);
                    view! { <span>{d}</span> }
                }).collect_view()}
            </div>
            {tasks.into_iter().map(|task| {
                let start = task.start_date.as_deref().and_then(iso_day_number).unwrap_or(min_day);
                let due = task.due_date.as_deref().and_then(iso_day_number).unwrap_or(start);
                // 70px label gutter, matching the scale's margin-left.
                let left = 70 + (start - min_day).max(0) * 44;
                let width = ((due - start + 1).max(1) * 44).max(44);
                let task_id = task.id.clone();
                let key = task.key.clone();
                let title = task_title(&task, lang.get());
                let color = status_color(&statuses, &task.status_id);
                view! {
                    <button class="gantt-row" style=format!("width:{row_width}px") on:click=move |_| set_open_task.set(Some(task_id.clone()))>
                        <span>{key}</span>
                        <i style=format!("left:{left}px;width:{width}px;background:{color}")>{title}</i>
                    </button>
                }
            }).collect_view()}
        </div>
    }.into_view()
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
