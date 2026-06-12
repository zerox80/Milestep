use crate::*;

pub(crate) fn ticket_detail(
    ticket: TicketDto,
    boot: BootstrapDto,
    lang: ReadSignal<Lang>,
    set_open_ticket: WriteSignal<Option<String>>,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) -> View {
    let (title, set_title) = create_signal(ticket.title.clone());
    let (description, set_description) = create_signal(ticket.description.clone());
    let (requester_name, set_requester_name) = create_signal(ticket.requester_name.clone());
    let (status, set_status) = create_signal(ticket.status.clone());
    let (priority, set_priority) = create_signal(ticket.priority.clone());
    let (assignee_id, set_assignee_id) =
        create_signal(ticket.assignee_id.clone().unwrap_or_default());
    let (busy, set_busy) = create_signal(false);
    let (local_error, set_local_error) = create_signal::<Option<String>>(None);
    // The whole ticket drawer is an edit form; a background refetch would
    // re-render it and discard in-progress changes.
    hold_realtime_while(|| true);

    let ticket_id_for_save = ticket.id.clone();
    let save = move |_| {
        if title.get_untracked().trim().is_empty() {
            set_local_error.set(Some(if lang.get_untracked() == Lang::De {
                "Bitte gib zuerst einen Tickettitel ein.".into()
            } else {
                "Add a ticket title first.".into()
            }));
            return;
        }
        set_local_error.set(None);
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
                Err(err) => {
                    set_local_error.set(Some(err.message.clone()));
                    set_error.set(Some(err.message));
                }
            }
            set_busy.set(false);
        });
    };

    let ticket_id_for_delete = ticket.id.clone();
    let ticket_title_for_delete = ticket.title.clone();
    let delete = move |_| {
        let confirm_text = if lang.get_untracked() == Lang::De {
            format!("{ticket_title_for_delete} wirklich loeschen?")
        } else {
            format!("Delete {ticket_title_for_delete}?")
        };
        let confirmed = web_sys::window()
            .and_then(|w| w.confirm_with_message(&confirm_text).ok())
            .unwrap_or(false);
        if !confirmed {
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
                    <span>{move || if lang.get() == Lang::De { "Titel" } else { "Title" }}</span>
                    <input class="title-input" prop:value=title on:input=move |ev| {
                        set_title.set(event_target_value(&ev));
                        set_local_error.set(None);
                    }/>
                </label>
                {move || local_error.get().map(|err| view! {
                    <div class="modal-error">{err}</div>
                })}
                <label class="modal-field">
                    <span>{move || if lang.get() == Lang::De { "Beschreibung" } else { "Description" }}</span>
                    <textarea prop:value=description on:input=move |ev| set_description.set(textarea_value(&ev))></textarea>
                </label>
                <div class="modal-meta ticket-meta">
                    <input placeholder=move || if lang.get() == Lang::De { "Melder / Kontakt" } else { "Requester / contact" } prop:value=requester_name on:input=move |ev| set_requester_name.set(event_target_value(&ev))/>
                    <select on:change=move |ev| set_status.set(ticket_status_from_value(&select_value(&ev)))>
                        <option value="open" selected=current_status == TicketStatus::Open>{move || if lang.get() == Lang::De { "Offen" } else { "Open" }}</option>
                        <option value="in_progress" selected=current_status == TicketStatus::InProgress>{move || if lang.get() == Lang::De { "In Arbeit" } else { "In progress" }}</option>
                        <option value="resolved" selected=current_status == TicketStatus::Resolved>{move || if lang.get() == Lang::De { "Geloest" } else { "Resolved" }}</option>
                        <option value="closed" selected=current_status == TicketStatus::Closed>{move || if lang.get() == Lang::De { "Geschlossen" } else { "Closed" }}</option>
                    </select>
                    <select on:change=move |ev| set_priority.set(priority_from_value(&select_value(&ev)))>
                        <option value="urgent" selected=current_priority == Priority::Urgent>{move || if lang.get() == Lang::De { "Dringend" } else { "Urgent" }}</option>
                        <option value="high" selected=current_priority == Priority::High>{move || if lang.get() == Lang::De { "Hoch" } else { "High" }}</option>
                        <option value="medium" selected=current_priority == Priority::Medium>{move || if lang.get() == Lang::De { "Mittel" } else { "Medium" }}</option>
                        <option value="low" selected=current_priority == Priority::Low>{move || if lang.get() == Lang::De { "Niedrig" } else { "Low" }}</option>
                    </select>
                    <select on:change=move |ev| set_assignee_id.set(select_value(&ev))>
                        <option value="" selected=current_assignee.is_empty()>{move || if lang.get() == Lang::De { "Nicht zugewiesen" } else { "Unassigned" }}</option>
                        {boot.members.into_iter().map(|m| {
                            let selected = current_assignee == m.user_id;
                            view! { <option value=m.user_id selected=selected>{m.name}</option> }
                        }).collect_view()}
                    </select>
                </div>
                <footer>
                    <button class="danger-link danger-action" on:click=delete>{move || if lang.get() == Lang::De { "Loeschen" } else { "Delete" }}</button>
                    <button class="btn ghost" on:click=move |_| set_open_ticket.set(None)>{move || if lang.get() == Lang::De { "Abbrechen" } else { "Cancel" }}</button>
                    <button class="btn primary" disabled=move || busy.get() on:click=save>{move || if lang.get() == Lang::De { "Speichern" } else { "Save" }}</button>
                </footer>
            </section>
        </div>
    }.into_view()
}
