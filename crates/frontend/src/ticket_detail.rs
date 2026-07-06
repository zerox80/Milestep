use crate::*;

pub(crate) fn ticket_detail(
    ticket: TicketDto,
    boot: BootstrapDto,
    lang: ReadSignal<Lang>,
    set_open_ticket: WriteSignal<Option<String>>,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) -> View {
    let can_edit = boot.current_role.can_edit();
    let (title, set_title) = create_signal(ticket.title.clone());
    let (description, set_description) = create_signal(ticket.description.clone());
    let (requester_name, set_requester_name) = create_signal(ticket.requester_name.clone());
    let (status, set_status) = create_signal(ticket.status.clone());
    let (priority, set_priority) = create_signal(ticket.priority.clone());
    let (assignee_id, set_assignee_id) =
        create_signal(ticket.assignee_id.clone().unwrap_or_default());
    let (busy, set_busy) = create_signal(false);
    let (local_error, set_local_error) = create_signal::<Option<String>>(None);
    let initial_title = ticket.title.clone();
    let initial_description = ticket.description.clone();
    let initial_requester_name = ticket.requester_name.clone();
    let initial_status = ticket.status.clone();
    let initial_priority = ticket.priority.clone();
    let initial_assignee_id = ticket.assignee_id.clone().unwrap_or_default();
    // Hold realtime only while there are unsaved local changes, so simply
    // viewing an editable ticket does not block collaborator updates.
    hold_realtime_while(move || {
        can_edit
            && (busy.get()
                || title.get() != initial_title
                || description.get() != initial_description
                || requester_name.get() != initial_requester_name
                || status.get() != initial_status
                || priority.get() != initial_priority
                || assignee_id.get() != initial_assignee_id)
    });

    let ticket_id_for_save = ticket.id.clone();
    let save = move |_| {
        if !require_title(
            &title.get_untracked(),
            "Bitte gib zuerst einen Tickettitel ein.",
            "Add a ticket title first.",
            lang.get_untracked(),
            set_local_error,
        ) {
            return;
        }
        set_busy.set(true);
        let assignee = assignee_id.get_untracked();
        let payload = UpdateTicketRequest {
            title: Some(title.get_untracked()),
            description: Some(description.get_untracked()),
            status: Some(status.get_untracked()),
            priority: Some(priority.get_untracked()),
            requester_name: Some(requester_name.get_untracked()),
            assignee_id: Some((!assignee.trim().is_empty()).then_some(assignee)),
        };
        let ticket_id = ticket_id_for_save.clone();
        spawn_local(async move {
            match api_patch::<_, TicketDto>(&format!("/api/tickets/{ticket_id}"), &payload).await {
                Ok(ticket) => {
                    replace_ticket(set_data, ticket);
                    set_error.set(None);
                    set_open_ticket.set(None);
                }
                Err(err) => report_api_error(&err, set_local_error, set_error),
            }
            set_busy.set(false);
        });
    };

    let ticket_id_for_delete = ticket.id.clone();
    let ticket_title_for_delete = ticket.title.clone();
    let delete = move |_| {
        if !confirm_delete(&ticket_title_for_delete, lang.get_untracked()) {
            return;
        }
        let ticket_id = ticket_id_for_delete.clone();
        spawn_local(async move {
            match api_delete_empty(&format!("/api/tickets/{ticket_id}")).await {
                Ok(()) => {
                    remove_ticket(set_data, &ticket_id);
                    set_open_ticket.set(None);
                    set_error.set(None);
                }
                Err(err) => set_error.set(Some(err.message)),
            }
        });
    };

    let current_status = ticket.status.clone();
    let current_priority = ticket.priority.clone();
    let current_assignee = ticket.assignee_id.clone().unwrap_or_default();

    view! {
        <div class="modal-backdrop">
            <section class="create-modal">
                <header>
                    <strong>"T"</strong>
                    <h2>{ticket.key}</h2>
                    <button on:click=move |_| set_open_ticket.set(None)>"x"</button>
                </header>
                <label class="modal-field title-field">
                    <span>{move || lang.get().tr("Titel", "Title")}</span>
                    <input class="title-input" prop:value=title on:input=move |ev| {
                        set_title.set(event_target_value(&ev));
                        set_local_error.set(None);
                    } disabled=!can_edit/>
                </label>
                {move || local_error.get().map(|err| view! {
                    <div class="modal-error">{err}</div>
                })}
                <label class="modal-field">
                    <span>{move || lang.get().tr("Beschreibung", "Description")}</span>
                    <textarea prop:value=description on:input=move |ev| set_description.set(textarea_value(&ev)) disabled=!can_edit></textarea>
                </label>
                <div class="modal-meta ticket-meta">
                    <input placeholder=move || lang.get().tr("Melder / Kontakt", "Requester / contact") prop:value=requester_name on:input=move |ev| set_requester_name.set(event_target_value(&ev)) disabled=!can_edit/>
                    <select prop:value=move || ticket_status_value(&status.get()) on:change=move |ev| set_status.set(ticket_status_from_value(&select_value(&ev))) disabled=!can_edit>
                        {ticket_status_options(current_status, lang)}
                    </select>
                    <select prop:value=move || priority_value(&priority.get()) on:change=move |ev| set_priority.set(priority_from_value(&select_value(&ev))) disabled=!can_edit>
                        {priority_options(current_priority, lang)}
                    </select>
                    <select prop:value=assignee_id on:change=move |ev| set_assignee_id.set(select_value(&ev)) disabled=!can_edit>
                        <option value="" selected=current_assignee.is_empty()>{move || lang.get().tr("Nicht zugewiesen", "Unassigned")}</option>
                        {boot.members.into_iter().map(|m| {
                            let selected = current_assignee == m.user_id;
                            view! { <option value=m.user_id selected=selected>{m.name}</option> }
                        }).collect_view()}
                    </select>
                </div>
                <footer>
                    {if can_edit {
                        view! {
                            <button class="danger-link danger-action" on:click=delete>{move || lang.get().tr("Löschen", "Delete")}</button>
                        }.into_view()
                    } else {
                        empty_view()
                    }}
                    <button class="btn ghost" on:click=move |_| set_open_ticket.set(None)>{move || lang.get().tr("Abbrechen", "Cancel")}</button>
                    {if can_edit {
                        view! {
                            <button class="btn primary" disabled=move || busy.get() on:click=save>{move || lang.get().tr("Speichern", "Save")}</button>
                        }.into_view()
                    } else {
                        empty_view()
                    }}
                </footer>
            </section>
        </div>
    }.into_view()
}
