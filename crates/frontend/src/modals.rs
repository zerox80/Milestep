use crate::*;

pub(crate) fn create_task_modal(
    boot: BootstrapDto,
    lang: ReadSignal<Lang>,
    set_show_create: WriteSignal<bool>,
    set_open_task: WriteSignal<Option<String>>,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) -> View {
    let (title, set_title) = create_signal(String::new());
    let (description, set_description) = create_signal(String::new());
    let (due_date, set_due_date) = create_signal(iso_in_days(5));
    let (priority, set_priority) = create_signal(Priority::Medium);
    let (phase, set_phase) = create_signal("ausfuehrung".to_string());
    let (status_id, set_status_id) = create_signal(
        boot.statuses
            .first()
            .map(|s| s.id.clone())
            .unwrap_or_default(),
    );
    let (assignee_id, set_assignee_id) = create_signal(
        boot.members
            .first()
            .map(|m| m.user_id.clone())
            .unwrap_or_default(),
    );
    let (recurrence, set_recurrence) = create_signal::<Option<Recurrence>>(None);
    let (busy, set_busy) = create_signal(false);
    let (local_error, set_local_error) = create_signal::<Option<String>>(None);
    hold_realtime_while(|| true);

    let create = move |_| {
        if !require_nonempty(
            &title.get_untracked(),
            lang.get_untracked(),
            "Bitte gib zuerst einen Aufgabentitel ein.",
            "Add a task title first.",
            set_local_error,
        ) {
            return;
        }
        set_busy.set(true);
        let payload = CreateTaskRequest {
            project_id: boot.project.id.clone(),
            title: title.get_untracked(),
            description: description.get_untracked(),
            tag: "Ausführung".into(),
            tag_color: "accent".into(),
            priority: priority.get_untracked(),
            status_id: status_id.get_untracked(),
            start_date: Some(today_iso()),
            due_date: Some(due_date.get_untracked()),
            phase: phase.get_untracked(),
            recurrence: recurrence.get_untracked(),
            assignee_ids: vec![assignee_id.get_untracked()],
            subtasks: vec![],
        };
        spawn_local(async move {
            match api_post::<_, TaskDto>("/api/tasks", &payload).await {
                Ok(task) => {
                    set_open_task.set(Some(task.id.clone()));
                    set_data.update(|data| {
                        if let Some(data) = data {
                            data.tasks.push(task);
                        }
                    });
                    set_show_create.set(false);
                    set_error.set(None);
                }
                Err(err) => report_submit_error(err, set_local_error, set_error),
            }
            set_busy.set(false);
        });
    };

    view! {
        <div class="modal-backdrop">
            <section class="create-modal">
                <header>
                    <strong>"+ "</strong>
                    <h2>{move || if lang.get() == Lang::De { "Neue Aufgabe" } else { "New task" }}</h2>
                    <button on:click=move |_| set_show_create.set(false)>"×"</button>
                </header>
                <label class="modal-field title-field">
                    <span>{move || if lang.get() == Lang::De { "Titel" } else { "Title" }}</span>
                    <input class="title-input" placeholder=move || if lang.get() == Lang::De { "Woran wird gearbeitet?" } else { "What are we working on?" } prop:value=title on:input=move |ev| {
                        set_title.set(event_target_value(&ev));
                        set_local_error.set(None);
                    }/>
                </label>
                {move || local_error.get().map(|err| view! {
                    <div class="modal-error">{err}</div>
                })}
                <label class="modal-field">
                    <span>{move || if lang.get() == Lang::De { "Beschreibung" } else { "Description" }}</span>
                    <textarea placeholder=move || if lang.get() == Lang::De { "Beschreibung hinzufügen..." } else { "Add description..." } prop:value=description on:input=move |ev| set_description.set(textarea_value(&ev))></textarea>
                </label>
                <div class="modal-meta">
                    <select on:change=move |ev| set_assignee_id.set(select_value(&ev))>
                        {boot.members.clone().into_iter().map(|m| view! { <option value=m.user_id>{m.name}</option> }).collect_view()}
                    </select>
                    <input type="date" prop:value=due_date on:input=move |ev| set_due_date.set(event_target_value(&ev))/>
                    <select on:change=move |ev| set_priority.set(priority_from_value(&select_value(&ev)))>
                        <option value="urgent">"Dringend"</option>
                        <option value="high">"Hoch"</option>
                        <option value="medium" selected>"Mittel"</option>
                        <option value="low">"Niedrig"</option>
                    </select>
                    <select on:change=move |ev| set_status_id.set(select_value(&ev))>
                        {boot.statuses.into_iter().map(|s| { let label = status_name(&s, lang.get()).to_string(); view! { <option value=s.id>{label}</option> } }).collect_view()}
                    </select>
                    <select on:change=move |ev| set_phase.set(select_value(&ev))>
                        <option value="planung">{move || if lang.get() == Lang::De { "Planung" } else { "Planning" }}</option>
                        <option value="vergabe">{move || if lang.get() == Lang::De { "Vergabe" } else { "Tendering" }}</option>
                        <option value="ausfuehrung" selected>{move || if lang.get() == Lang::De { "Ausführung" } else { "Execution" }}</option>
                        <option value="abnahme">{move || if lang.get() == Lang::De { "Abnahme" } else { "Handover" }}</option>
                    </select>
                    <select on:change=move |ev| set_recurrence.set(recurrence_from_value(&select_value(&ev)))>
                        {recurrence_options(None, lang)}
                    </select>
                </div>
                <footer>
                    <button class="btn ghost" on:click=move |_| set_show_create.set(false)>{move || if lang.get() == Lang::De { "Abbrechen" } else { "Cancel" }}</button>
                    <button class="btn primary" disabled=move || busy.get() on:click=create>{move || if lang.get() == Lang::De { "Aufgabe erstellen" } else { "Create task" }}</button>
                </footer>
            </section>
        </div>
    }.into_view()
}

pub(crate) fn create_ticket_modal(
    boot: BootstrapDto,
    lang: ReadSignal<Lang>,
    set_show_create_ticket: WriteSignal<bool>,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) -> View {
    let (title, set_title) = create_signal(String::new());
    let (description, set_description) = create_signal(String::new());
    let (requester_name, set_requester_name) = create_signal(String::new());
    let (status, set_status) = create_signal(TicketStatus::Open);
    let (priority, set_priority) = create_signal(Priority::Medium);
    let (assignee_id, set_assignee_id) = create_signal(String::new());
    let (busy, set_busy) = create_signal(false);
    let (local_error, set_local_error) = create_signal::<Option<String>>(None);
    hold_realtime_while(|| true);

    let create = move |_| {
        if !require_nonempty(
            &title.get_untracked(),
            lang.get_untracked(),
            "Bitte gib zuerst einen Tickettitel ein.",
            "Add a ticket title first.",
            set_local_error,
        ) {
            return;
        }
        set_busy.set(true);
        let assignee = assignee_id.get_untracked();
        let payload = CreateTicketRequest {
            project_id: boot.project.id.clone(),
            title: title.get_untracked(),
            description: description.get_untracked(),
            status: status.get_untracked(),
            priority: priority.get_untracked(),
            requester_name: requester_name.get_untracked(),
            assignee_id: (!assignee.trim().is_empty()).then_some(assignee),
        };
        spawn_local(async move {
            match api_post::<_, TicketDto>("/api/tickets", &payload).await {
                Ok(ticket) => {
                    set_data.update(|data| {
                        if let Some(data) = data {
                            data.tickets.insert(0, ticket);
                        }
                    });
                    set_show_create_ticket.set(false);
                    set_error.set(None);
                }
                Err(err) => report_submit_error(err, set_local_error, set_error),
            }
            set_busy.set(false);
        });
    };

    view! {
        <div class="modal-backdrop">
            <section class="create-modal">
                <header>
                    <strong>"T"</strong>
                    <h2>{move || if lang.get() == Lang::De { "Neues Ticket" } else { "New ticket" }}</h2>
                    <button on:click=move |_| set_show_create_ticket.set(false)>"x"</button>
                </header>
                <label class="modal-field title-field">
                    <span>{move || if lang.get() == Lang::De { "Titel" } else { "Title" }}</span>
                    <input class="title-input" placeholder=move || if lang.get() == Lang::De { "Was ist passiert?" } else { "What happened?" } prop:value=title on:input=move |ev| {
                        set_title.set(event_target_value(&ev));
                        set_local_error.set(None);
                    }/>
                </label>
                {move || local_error.get().map(|err| view! {
                    <div class="modal-error">{err}</div>
                })}
                <label class="modal-field">
                    <span>{move || if lang.get() == Lang::De { "Beschreibung" } else { "Description" }}</span>
                    <textarea placeholder=move || if lang.get() == Lang::De { "Details, Kontext, betroffene Wohnung..." } else { "Details, context, affected unit..." } prop:value=description on:input=move |ev| set_description.set(textarea_value(&ev))></textarea>
                </label>
                <div class="modal-meta ticket-meta">
                    <input placeholder=move || if lang.get() == Lang::De { "Melder / Kontakt" } else { "Requester / contact" } prop:value=requester_name on:input=move |ev| set_requester_name.set(event_target_value(&ev))/>
                    <select on:change=move |ev| set_status.set(ticket_status_from_value(&select_value(&ev)))>
                        <option value="open" selected>{move || if lang.get() == Lang::De { "Offen" } else { "Open" }}</option>
                        <option value="in_progress">{move || if lang.get() == Lang::De { "In Arbeit" } else { "In progress" }}</option>
                        <option value="resolved">{move || if lang.get() == Lang::De { "Geloest" } else { "Resolved" }}</option>
                        <option value="closed">{move || if lang.get() == Lang::De { "Geschlossen" } else { "Closed" }}</option>
                    </select>
                    <select on:change=move |ev| set_priority.set(priority_from_value(&select_value(&ev)))>
                        <option value="urgent">"Dringend"</option>
                        <option value="high">"Hoch"</option>
                        <option value="medium" selected>"Mittel"</option>
                        <option value="low">"Niedrig"</option>
                    </select>
                    <select on:change=move |ev| set_assignee_id.set(select_value(&ev))>
                        <option value="">{move || if lang.get() == Lang::De { "Nicht zugewiesen" } else { "Unassigned" }}</option>
                        {boot.members.into_iter().map(|m| view! { <option value=m.user_id>{m.name}</option> }).collect_view()}
                    </select>
                </div>
                <footer>
                    <button class="btn ghost" on:click=move |_| set_show_create_ticket.set(false)>{move || if lang.get() == Lang::De { "Abbrechen" } else { "Cancel" }}</button>
                    <button class="btn primary" disabled=move || busy.get() on:click=create>{move || if lang.get() == Lang::De { "Ticket erstellen" } else { "Create ticket" }}</button>
                </footer>
            </section>
        </div>
    }.into_view()
}

pub(crate) fn create_milestone_modal(
    boot: BootstrapDto,
    lang: ReadSignal<Lang>,
    set_show_create_milestone: WriteSignal<bool>,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) -> View {
    let (title, set_title) = create_signal(String::new());
    let (due_date, set_due_date) = create_signal(iso_in_days(7));
    let (phase, set_phase) = create_signal("planung".to_string());
    let (busy, set_busy) = create_signal(false);
    let (local_error, set_local_error) = create_signal::<Option<String>>(None);
    hold_realtime_while(|| true);

    let create = move |_| {
        if !require_nonempty(
            &title.get_untracked(),
            lang.get_untracked(),
            "Bitte gib zuerst einen Meilenstein-Titel ein.",
            "Add a milestone title first.",
            set_local_error,
        ) {
            return;
        }
        set_busy.set(true);
        let payload = CreateMilestoneRequest {
            project_id: boot.project.id.clone(),
            title: title.get_untracked(),
            title_en: None,
            due_date: due_date.get_untracked(),
            phase: phase.get_untracked(),
        };
        spawn_local(async move {
            match api_post::<_, MilestoneDto>("/api/milestones", &payload).await {
                Ok(milestone) => {
                    set_data.update(|data| {
                        if let Some(data) = data {
                            data.milestones.push(milestone);
                            data.milestones.sort_by(|a, b| a.due_date.cmp(&b.due_date));
                        }
                    });
                    set_show_create_milestone.set(false);
                    set_error.set(None);
                }
                Err(err) => report_submit_error(err, set_local_error, set_error),
            }
            set_busy.set(false);
        });
    };

    view! {
        <div class="modal-backdrop">
            <section class="create-modal">
                <header>
                    <strong>"◇"</strong>
                    <h2>{move || if lang.get() == Lang::De { "Neuer Meilenstein" } else { "New milestone" }}</h2>
                    <button on:click=move |_| set_show_create_milestone.set(false)>"x"</button>
                </header>
                <label class="modal-field title-field">
                    <span>{move || if lang.get() == Lang::De { "Titel" } else { "Title" }}</span>
                    <input class="title-input" placeholder=move || if lang.get() == Lang::De { "Was soll erreicht werden?" } else { "What should be reached?" } prop:value=title on:input=move |ev| {
                        set_title.set(event_target_value(&ev));
                        set_local_error.set(None);
                    }/>
                </label>
                {move || local_error.get().map(|err| view! {
                    <div class="modal-error">{err}</div>
                })}
                <div class="modal-meta milestone-meta">
                    <input type="date" prop:value=due_date on:input=move |ev| set_due_date.set(event_target_value(&ev))/>
                    <select on:change=move |ev| set_phase.set(select_value(&ev))>
                        <option value="planung" selected>{move || if lang.get() == Lang::De { "Planung" } else { "Planning" }}</option>
                        <option value="vergabe">{move || if lang.get() == Lang::De { "Vergabe" } else { "Tendering" }}</option>
                        <option value="ausfuehrung">{move || if lang.get() == Lang::De { "Ausfuehrung" } else { "Execution" }}</option>
                        <option value="abnahme">{move || if lang.get() == Lang::De { "Abnahme" } else { "Handover" }}</option>
                    </select>
                </div>
                <footer>
                    <button class="btn ghost" on:click=move |_| set_show_create_milestone.set(false)>{move || if lang.get() == Lang::De { "Abbrechen" } else { "Cancel" }}</button>
                    <button class="btn primary" disabled=move || busy.get() on:click=create>{move || if lang.get() == Lang::De { "Meilenstein erstellen" } else { "Create milestone" }}</button>
                </footer>
            </section>
        </div>
    }.into_view()
}
