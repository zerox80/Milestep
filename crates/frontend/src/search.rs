use crate::*;

pub(crate) fn search_is_active(query: &str) -> bool {
    !query.trim().is_empty()
}

pub(crate) fn search_subtitle(boot: &BootstrapDto, lang: Lang, query: &str) -> String {
    let task_count = matching_tasks(boot, lang, query).len();
    let ticket_count = matching_tickets(boot, lang, query).len();
    let needle = query.trim();
    if lang.is_de() {
        format!("{task_count} Aufgaben und {ticket_count} Tickets für \"{needle}\"")
    } else {
        format!("{task_count} tasks and {ticket_count} tickets for \"{needle}\"")
    }
}

pub(crate) fn search_results_view(
    boot: BootstrapDto,
    lang: ReadSignal<Lang>,
    set_open_task: WriteSignal<Option<String>>,
    set_open_ticket: WriteSignal<Option<String>>,
    set_search_query: WriteSignal<String>,
    query: String,
) -> View {
    let lang_now = lang.get();
    let tasks = matching_tasks(&boot, lang_now, &query);
    let tickets = matching_tickets(&boot, lang_now, &query);
    let total = tasks.len() + tickets.len();
    let task_count = tasks.len();
    let ticket_count = tickets.len();
    let members = boot.members;

    view! {
        <div class="search-results">
            <div class="search-results-head">
                <strong>{move || lang.get().tr("Suchergebnisse", "Search results")}</strong>
                <span>{move || {
                    if lang.get().is_de() {
                        format!("{total} Treffer")
                    } else {
                        format!("{total} results")
                    }
                }}</span>
                <button class="btn ghost" on:click=move |_| set_search_query.set(String::new())>
                    {move || lang.get().tr("Suche leeren", "Clear search")}
                </button>
            </div>
            {if total == 0 {
                view! {
                    <div class="empty-state">
                        <strong>{move || lang.get().tr("Keine Treffer gefunden", "No results found")}</strong>
                        <span>{move || lang.get().tr("Versuche eine Aufgabe, ein Ticket, ein Teammitglied oder ein Datum.", "Try a task, ticket, teammate or date.")}</span>
                    </div>
                }.into_view()
            } else {
                view! {
                    <div class="search-result-grid">
                        <section class="panel search-panel">
                            <div class="panel-head">
                                <h3>{move || lang.get().tr("Aufgaben", "Tasks")}</h3>
                                <small>{task_count}</small>
                            </div>
                            {if tasks.is_empty() {
                                view! {
                                    <div class="empty-state compact">
                                        <span>{move || lang.get().tr("Keine passenden Aufgaben.", "No matching tasks.")}</span>
                                    </div>
                                }.into_view()
                            } else {
                                view! {
                                    <div class="row-list">
                                        {tasks.into_iter().map(|task| task_row(task, members.clone(), lang, set_open_task)).collect_view()}
                                    </div>
                                }.into_view()
                            }}
                        </section>
                        <section class="panel search-panel">
                            <div class="panel-head">
                                <h3>{move || lang.get().tr("Tickets", "Tickets")}</h3>
                                <small>{ticket_count}</small>
                            </div>
                            {if tickets.is_empty() {
                                view! {
                                    <div class="empty-state compact">
                                        <span>{move || lang.get().tr("Keine passenden Tickets.", "No matching tickets.")}</span>
                                    </div>
                                }.into_view()
                            } else {
                                view! {
                                    <div class="row-list">
                                        {tickets.into_iter().map(|ticket| search_ticket_row(ticket, lang, set_open_ticket)).collect_view()}
                                    </div>
                                }.into_view()
                            }}
                        </section>
                    </div>
                }.into_view()
            }}
        </div>
    }
    .into_view()
}

fn matching_tasks(boot: &BootstrapDto, lang: Lang, query: &str) -> Vec<TaskDto> {
    let terms = search_terms(query);
    boot.tasks
        .iter()
        .filter(|task| task_matches(task, &boot.statuses, &boot.members, lang, &terms))
        .cloned()
        .collect()
}

fn matching_tickets(boot: &BootstrapDto, lang: Lang, query: &str) -> Vec<TicketDto> {
    let terms = search_terms(query);
    boot.tickets
        .iter()
        .filter(|ticket| ticket_matches(ticket, lang, &terms))
        .cloned()
        .collect()
}

fn search_terms(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
}

fn task_matches(
    task: &TaskDto,
    statuses: &[StatusDto],
    members: &[MemberDto],
    lang: Lang,
    terms: &[String],
) -> bool {
    let status = statuses
        .iter()
        .find(|status| status.id == task.status_id)
        .map(|status| status_name(status, lang))
        .unwrap_or_default();
    let assignees = task
        .assignee_ids
        .iter()
        .filter_map(|id| members.iter().find(|member| &member.user_id == id))
        .map(|member| member.name.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let haystack = format!(
        "{} {} {} {} {} {} {} {} {} {} {} {}",
        task.key,
        task.title,
        task.title_en.as_deref().unwrap_or_default(),
        task.description,
        task.description_en.as_deref().unwrap_or_default(),
        task.tag,
        task.phase,
        task.start_date.as_deref().unwrap_or_default(),
        task.due_date.as_deref().unwrap_or_default(),
        priority_label(&task.priority, lang),
        status,
        assignees
    )
    .to_lowercase();
    terms.iter().all(|term| haystack.contains(term))
}

fn ticket_matches(ticket: &TicketDto, lang: Lang, terms: &[String]) -> bool {
    let haystack = format!(
        "{} {} {} {} {} {} {} {}",
        ticket.key,
        ticket.title,
        ticket.description,
        ticket_status_label(&ticket.status, lang),
        priority_label(&ticket.priority, lang),
        ticket.requester_name,
        ticket.assignee_name.as_deref().unwrap_or_default(),
        ticket.created_by_name.as_deref().unwrap_or_default()
    )
    .to_lowercase();
    terms.iter().all(|term| haystack.contains(term))
}

fn search_ticket_row(
    ticket: TicketDto,
    lang: ReadSignal<Lang>,
    set_open_ticket: WriteSignal<Option<String>>,
) -> View {
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

    view! {
        <button class="search-ticket-row" on:click=move |_| set_open_ticket.set(Some(ticket_id.clone()))>
            <span><small>{ticket.key}</small><strong>{ticket.title}</strong><em>{ticket.description}</em></span>
            <span><b class=status_class>{status}</b></span>
            <span>{priority}</span>
            <span>{requester}</span>
            <span>{assignee}</span>
        </button>
    }
    .into_view()
}
