use crate::*;

pub(crate) fn overview_view(
    boot: BootstrapDto,
    lang: ReadSignal<Lang>,
    set_open_task: WriteSignal<Option<String>>,
) -> View {
    let today_str = today_iso();
    let open = boot.tasks.iter().filter(|t| !t.status_is_done).count();
    let today = boot
        .tasks
        .iter()
        .filter(|t| t.due_date.as_deref() == Some(today_str.as_str()) && !t.status_is_done)
        .count();
    let overdue = boot
        .tasks
        .iter()
        .filter(|t| {
            t.due_date
                .as_deref()
                .is_some_and(|d| d < today_str.as_str())
                && !t.status_is_done
        })
        .count();
    let done = boot.tasks.iter().filter(|t| t.status_is_done).count();
    let progress = if boot.tasks.is_empty() {
        0
    } else {
        (done * 100) / boot.tasks.len()
    };
    let today_tasks = boot
        .tasks
        .iter()
        .filter(|t| {
            !t.status_is_done
                && t.due_date
                    .as_deref()
                    .is_some_and(|d| d <= today_str.as_str())
        })
        .cloned();
    let statuses_for_legend = boot.statuses.clone();
    let tasks_for_legend = boot.tasks.clone();

    view! {
        <div class="overview-grid">
            <div class="stats-row">
                {stat("□", open, if lang.get() == Lang::De { "Offene Aufgaben" } else { "Open tasks" }, "cool")}
                {stat("◷", today, if lang.get() == Lang::De { "Heute fällig" } else { "Due today" }, "accent")}
                {stat("⚑", overdue, if lang.get() == Lang::De { "Überfällig" } else { "Overdue" }, "warm")}
                {stat("✓", done, if lang.get() == Lang::De { "Diese Woche fertig" } else { "Done this week" }, "good")}
            </div>
            <div class="two-col">
                <div class="panel">
                    <h3>{move || if lang.get() == Lang::De { "Heute fällig" } else { "Due today" }}</h3>
                    <div class="row-list">
                        {today_tasks.map(|task| task_row(task, boot.members.clone(), lang, set_open_task)).collect_view()}
                    </div>
                </div>
                <div class="panel">
                    <h3>{move || if lang.get() == Lang::De { "Projekt-Fortschritt" } else { "Project progress" }}</h3>
                    <div class="progress-big">
                        <strong>{format!("{progress}%")}</strong>
                        <span><i style=format!("width:{progress}%")></i></span>
                    </div>
                    <div class="status-legend">
                        {statuses_for_legend.into_iter().map(|s| {
                            let count = tasks_for_legend.iter().filter(|t| t.status_id == s.id).count();
                            let color = s.color.clone();
                            let label = status_name(&s, lang.get()).to_string();
                            view! { <small><b style=format!("background:{}", color)></b>{label}" "{count}</small> }
                        }).collect_view()}
                    </div>
                </div>
            </div>
            <div class="two-col">
                <div class="panel">
                    <h3>{move || if lang.get() == Lang::De { "Anstehende Meilensteine" } else { "Upcoming milestones" }}</h3>
                    {boot.milestones.iter().map(|m| view! {
                        <div class="milestone-row"><span>"◇"</span><strong>{title_for(m.title.clone(), m.title_en.clone(), lang.get())}</strong><small>{fmt_date(m.due_date.as_str(), lang.get())}</small></div>
                    }).collect_view()}
                </div>
                <div class="panel">
                    <h3>{move || if lang.get() == Lang::De { "Aktivität" } else { "Activity" }}</h3>
                    {boot.audit_events.iter().take(6).map(|a| view! {
                        <div class="activity-row"><span class="avatar tiny">{a.actor_name.as_deref().map_or_else(|| "S".into(), initials)}</span><span>{a.actor_name.clone().unwrap_or_else(|| "System".into())}" · "{a.action.clone()}</span><small>{if lang.get() == Lang::De { a.created_label_de.clone() } else { a.created_label_en.clone() }}</small></div>
                    }).collect_view()}
                </div>
            </div>
        </div>
    }.into_view()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn board_view(
    boot: BootstrapDto,
    lang: ReadSignal<Lang>,
    set_open_task: WriteSignal<Option<String>>,
    drag_task: ReadSignal<Option<String>>,
    set_drag_task: WriteSignal<Option<String>>,
    set_show_create: WriteSignal<bool>,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) -> View {
    view! {
        <div class="board-grid">
            {boot.statuses.clone().into_iter().map(|status| {
                let status_id = status.id.clone();
                let status_color = status.color.clone();
                let status_label = status_name(&status, lang.get()).to_string();
                let tasks = boot.tasks.iter().filter(|t| t.status_id == status.id).cloned().collect::<Vec<_>>();
                let task_count = tasks.len();
                view! {
                    <section class="board-col"
                        on:dragover=move |ev: DragEvent| ev.prevent_default()
                        on:drop=move |ev: DragEvent| {
                            ev.prevent_default();
                            if let Some(task_id) = drag_task.get_untracked() {
                                optimistic_move(task_id, status_id.clone(), set_data, set_error);
                                set_drag_task.set(None);
                            }
                        }>
                        <header><b style=format!("background:{}", status_color)></b><strong>{status_label}</strong><small>{task_count}</small><button on:click=move |_| set_show_create.set(true)>"+ "</button></header>
                        {tasks.into_iter().map(|task| task_card(task, boot.members.clone(), lang, set_open_task, set_drag_task)).collect_view()}
                    </section>
                }
            }).collect_view()}
        </div>
    }.into_view()
}

pub(crate) fn list_view(
    boot: BootstrapDto,
    lang: ReadSignal<Lang>,
    set_open_task: WriteSignal<Option<String>>,
) -> View {
    view! {
        <div class="table-panel">
            <div class="table-head"><span>"Aufgabe"</span><span>"Status"</span><span>"Priorität"</span><span>"Fällig"</span><span>"Team"</span></div>
            {boot.tasks.into_iter().map(|task| {
                let task_id = task.id.clone();
                let key = task.key.clone();
                let title = task_title(&task, lang.get());
                let status_label = boot.statuses.iter().find(|s| s.id == task.status_id).map(|s| status_name(s, lang.get()).to_string()).unwrap_or_default();
                let priority = priority_label(&task.priority, lang.get()).to_string();
                let due = task.due_date.as_deref().map_or_else(|| "-".into(), |d| fmt_date(d, lang.get()));
                let assignees = task.assignee_ids;
                view! {
                    <button class="task-line" on:click=move |_| set_open_task.set(Some(task_id.clone()))>
                        <span><small>{key}</small><strong>{title}</strong></span>
                        <span>{status_label}</span>
                        <span>{priority}</span>
                        <span>{due}</span>
                        <span>{assignee_avatars(&assignees, &boot.members)}</span>
                    </button>
                }
            }).collect_view()}
        </div>
    }.into_view()
}

pub(crate) fn ticket_view(
    boot: BootstrapDto,
    lang: ReadSignal<Lang>,
    set_show_create_ticket: WriteSignal<bool>,
    set_open_ticket: WriteSignal<Option<String>>,
) -> View {
    let open = boot
        .tickets
        .iter()
        .filter(|t| matches!(t.status, TicketStatus::Open))
        .count();
    let active = boot
        .tickets
        .iter()
        .filter(|t| matches!(t.status, TicketStatus::InProgress))
        .count();
    let done = boot
        .tickets
        .iter()
        .filter(|t| matches!(t.status, TicketStatus::Resolved | TicketStatus::Closed))
        .count();
    view! {
        <div class="ticket-grid">
            <div class="stats-row">
                {stat("T", boot.tickets.len(), if lang.get() == Lang::De { "Tickets gesamt" } else { "Total tickets" }, "cool")}
                {stat("!", open, if lang.get() == Lang::De { "Offen" } else { "Open" }, "accent")}
                {stat(">", active, if lang.get() == Lang::De { "In Arbeit" } else { "In progress" }, "warm")}
                {stat("✓", done, if lang.get() == Lang::De { "Erledigt" } else { "Done" }, "good")}
            </div>
            <div class="table-panel">
                <div class="ticket-head">
                    <span>"Ticket"</span>
                    <span>"Status"</span>
                    <span>{move || if lang.get() == Lang::De { "Prioritaet" } else { "Priority" }}</span>
                    <span>{move || if lang.get() == Lang::De { "Melder" } else { "Requester" }}</span>
                    <span>{move || if lang.get() == Lang::De { "Zuweisung" } else { "Assignee" }}</span>
                    <span>{move || if lang.get() == Lang::De { "Aktualisiert" } else { "Updated" }}</span>
                </div>
                {if boot.tickets.is_empty() {
                    view! {
                        <div class="empty-state">
                            <strong>{move || if lang.get() == Lang::De { "Noch keine Tickets" } else { "No tickets yet" }}</strong>
                            <button class="btn primary" on:click=move |_| set_show_create_ticket.set(true)>{move || if lang.get() == Lang::De { "Ticket erstellen" } else { "Create ticket" }}</button>
                        </div>
                    }.into_view()
                } else {
                    boot.tickets.into_iter().map(|ticket| {
                        let ticket_id = ticket.id.clone();
                        let status = ticket_status_label(&ticket.status, lang.get()).to_string();
                        let status_class = format!("ticket-status {}", ticket_status_class(&ticket.status));
                        let priority = priority_label(&ticket.priority, lang.get()).to_string();
                        let assignee = ticket.assignee_name.unwrap_or_else(|| "-".into());
                        let requester = if ticket.requester_name.trim().is_empty() {
                            ticket.created_by_name.unwrap_or_else(|| "-".into())
                        } else {
                            ticket.requester_name
                        };
                        let updated = if lang.get() == Lang::De { ticket.updated_label_de } else { ticket.updated_label_en };
                        view! {
                            <button class="ticket-row" on:click=move |_| set_open_ticket.set(Some(ticket_id.clone()))>
                                <span><small>{ticket.key}</small><strong>{ticket.title}</strong><em>{ticket.description}</em></span>
                                <span><b class=status_class>{status}</b></span>
                                <span>{priority}</span>
                                <span>{requester}</span>
                                <span>{assignee}</span>
                                <span>{updated}</span>
                            </button>
                        }
                    }).collect_view().into_view()
                }}
            </div>
        </div>
    }.into_view()
}
