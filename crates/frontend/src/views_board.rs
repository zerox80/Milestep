use crate::*;

pub(crate) fn overview_view(
    boot: BootstrapDto,
    lang: ReadSignal<Lang>,
    set_open_task: WriteSignal<Option<String>>,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) -> View {
    let (show_create_milestone, set_show_create_milestone) = create_signal(false);
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
        .cloned()
        .collect::<Vec<_>>();
    let statuses_for_legend = boot.statuses.clone();
    let tasks_for_legend = boot.tasks.clone();
    let milestones = boot.milestones.clone();
    let can_edit = boot.current_role.can_edit();
    let boot_for_milestone_create = boot.clone();

    view! {
        <div class="overview-grid">
            <div class="stats-row">
                {stat(AppIcon::Kanban, open, lang.get().tr("Offene Aufgaben", "Open tasks"), "cool")}
                {stat(AppIcon::Clock, today, lang.get().tr("Heute fällig", "Due today"), "accent")}
                {stat(AppIcon::Flag, overdue, lang.get().tr("Überfällig", "Overdue"), "warm")}
                {stat(AppIcon::CheckCircle, done, lang.get().tr("Diese Woche fertig", "Done this week"), "good")}
            </div>
            <div class="two-col">
                <div class="panel">
                    <h3>{move || lang.get().tr("Heute fällig", "Due today")}</h3>
                    <div class="row-list">
                        {if today_tasks.is_empty() {
                            view! {
                                <div class="empty-state compact">
                                    <strong>{move || lang.get().tr("Nichts fällig", "Nothing due")}</strong>
                                    <span>{move || lang.get().tr("Heute ist frei von überfälligen Aufgaben.", "No due or overdue tasks today.")}</span>
                                </div>
                            }.into_view()
                        } else {
                            today_tasks.into_iter().map(|task| task_row(task, boot.members.clone(), lang, set_open_task)).collect_view().into_view()
                        }}
                    </div>
                </div>
                <div class="panel">
                    <h3>{move || lang.get().tr("Projekt-Fortschritt", "Project progress")}</h3>
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
                    <div class="panel-head">
                        <h3>{move || lang.get().tr("Anstehende Meilensteine", "Upcoming milestones")}</h3>
                        {if can_edit {
                            view! {
                                <button class="icon-button small" title=move || lang.get().tr("Meilenstein erstellen", "Create milestone") on:click=move |_| set_show_create_milestone.set(true)>"+ "</button>
                            }.into_view()
                        } else {
                            empty_view()
                        }}
                    </div>
                    {if milestones.is_empty() {
                        view! {
                            <div class="empty-state compact">
                                <strong>{move || lang.get().tr("Keine Meilensteine geplant", "No milestones planned")}</strong>
                                <span>{move || lang.get().tr("Sobald Termine angelegt sind, erscheinen sie hier.", "Scheduled milestones will appear here.")}</span>
                                {if can_edit {
                                    view! {
                                        <button class="btn primary" on:click=move |_| set_show_create_milestone.set(true)>{move || lang.get().tr("Meilenstein anlegen", "Create milestone")}</button>
                                    }.into_view()
                                } else {
                                    empty_view()
                                }}
                            </div>
                        }.into_view()
                    } else {
                        milestones.iter().map(|m| view! {
                            <div class="milestone-row">
                                <span>"◇"</span>
                                <strong>{title_for(m.title.clone(), m.title_en.clone(), lang.get())}</strong>
                                <small>{fmt_date(m.due_date.as_str(), lang.get())}</small>
                                {if can_edit {
                                    let milestone_id = m.id.clone();
                                    let milestone_title = title_for(m.title.clone(), m.title_en.clone(), lang.get());
                                    view! {
                                        <button class="danger-icon" title=move || lang.get().tr("Meilenstein löschen", "Delete milestone") on:click=move |_| {
                                            delete_milestone(milestone_id.clone(), milestone_title.clone(), lang, set_data, set_error);
                                        }>"x"</button>
                                    }.into_view()
                                } else {
                                    empty_view()
                                }}
                            </div>
                        }).collect_view().into_view()
                    }}
                </div>
                <div class="panel">
                    <h3>{move || lang.get().tr("Aktivität", "Activity")}</h3>
                    {if boot.audit_events.is_empty() {
                        view! {
                            <div class="empty-state compact">
                                <span>{move || lang.get().tr("Noch keine Aktivität.", "No activity yet.")}</span>
                            </div>
                        }.into_view()
                    } else {
                        boot.audit_events.iter().take(6).map(|a| view! {
                            <div class="activity-row"><span class="avatar tiny">{a.actor_name.as_deref().map_or_else(|| "S".into(), initials)}</span><span>{a.actor_name.clone().unwrap_or_else(|| "System".into())}" · "{a.action.clone()}</span><small>{if lang.get().is_de() { a.created_label_de.clone() } else { a.created_label_en.clone() }}</small></div>
                        }).collect_view().into_view()
                    }}
                </div>
            </div>
            {move || if can_edit && show_create_milestone.get() {
                create_milestone_modal(boot_for_milestone_create.clone(), lang, set_show_create_milestone, set_data, set_error).into_view()
            } else {
                empty_view()
            }}
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
    let can_edit = boot.current_role.can_edit();
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
                            if can_edit {
                                if let Some(task_id) = drag_task.get_untracked() {
                                    optimistic_move(task_id, status_id.clone(), set_data, set_error);
                                    set_drag_task.set(None);
                                }
                            }
                        }>
                        <header>
                            <b style=format!("background:{}", status_color)></b>
                            <strong>{status_label}</strong>
                            <small>{task_count}</small>
                            {if can_edit {
                                view! { <button on:click=move |_| set_show_create.set(true)>"+ "</button> }.into_view()
                            } else {
                                empty_view()
                            }}
                        </header>
                        {if tasks.is_empty() {
                            view! {
                                <div class="board-empty">
                                    <strong>{move || lang.get().tr("Keine Aufgaben", "No tasks")}</strong>
                                    <span>{move || lang.get().tr("Ziehe Aufgaben hierher oder erstelle eine neue.", "Drop tasks here or create a new one.")}</span>
                                    {if can_edit {
                                        view! {
                                            <button class="btn ghost" on:click=move |_| set_show_create.set(true)>{move || lang.get().tr("Aufgabe erstellen", "Create task")}</button>
                                        }.into_view()
                                    } else {
                                        empty_view()
                                    }}
                                </div>
                            }.into_view()
                        } else {
                            tasks.into_iter().map(|task| task_card(task, boot.members.clone(), lang, set_open_task, set_drag_task, can_edit)).collect_view().into_view()
                        }}
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
            {if boot.tasks.is_empty() {
                view! {
                    <div class="empty-state">
                        <strong>{move || lang.get().tr("Noch keine Aufgaben", "No tasks yet")}</strong>
                        <span>{move || lang.get().tr("Neue Aufgaben erscheinen in dieser Liste.", "New tasks will appear in this list.")}</span>
                    </div>
                }.into_view()
            } else {
                boot.tasks.into_iter().map(|task| {
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
                }).collect_view().into_view()
            }}
        </div>
    }.into_view()
}

pub(crate) fn ticket_view(
    boot: BootstrapDto,
    lang: ReadSignal<Lang>,
    set_show_create_ticket: WriteSignal<bool>,
    set_open_ticket: WriteSignal<Option<String>>,
) -> View {
    let can_edit = boot.current_role.can_edit();
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
                {stat(AppIcon::Ticket, boot.tickets.len(), lang.get().tr("Tickets gesamt", "Total tickets"), "cool")}
                {stat(AppIcon::Alert, open, lang.get().tr("Offen", "Open"), "accent")}
                {stat(AppIcon::Timeline, active, lang.get().tr("In Arbeit", "In progress"), "warm")}
                {stat(AppIcon::CheckCircle, done, lang.get().tr("Erledigt", "Done"), "good")}
            </div>
            <div class="table-panel">
                <div class="ticket-head">
                    <span>"Ticket"</span>
                    <span>"Status"</span>
                    <span>{move || lang.get().tr("Priorität", "Priority")}</span>
                    <span>{move || lang.get().tr("Melder", "Requester")}</span>
                    <span>{move || lang.get().tr("Zuweisung", "Assignee")}</span>
                    <span>{move || lang.get().tr("Aktualisiert", "Updated")}</span>
                </div>
                {if boot.tickets.is_empty() {
                    view! {
                        <div class="empty-state">
                            <strong>{move || lang.get().tr("Noch keine Tickets", "No tickets yet")}</strong>
                            {if can_edit {
                                view! {
                                    <button class="btn primary" on:click=move |_| set_show_create_ticket.set(true)>{move || lang.get().tr("Ticket erstellen", "Create ticket")}</button>
                                }.into_view()
                            } else {
                                empty_view()
                            }}
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
                        let updated = if lang.get().is_de() { ticket.updated_label_de } else { ticket.updated_label_en };
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
