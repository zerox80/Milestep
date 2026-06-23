use crate::*;

#[derive(Clone, Copy)]
pub(crate) struct AppSignals {
    pub(crate) lang: ReadSignal<Lang>,
    pub(crate) set_lang: WriteSignal<Lang>,
    pub(crate) nav: ReadSignal<NavView>,
    pub(crate) set_nav: WriteSignal<NavView>,
    pub(crate) board_mode: ReadSignal<String>,
    pub(crate) set_board_mode: WriteSignal<String>,
    pub(crate) search_query: ReadSignal<String>,
    pub(crate) set_search_query: WriteSignal<String>,
    pub(crate) open_task: ReadSignal<Option<String>>,
    pub(crate) set_open_task: WriteSignal<Option<String>>,
    pub(crate) open_ticket: ReadSignal<Option<String>>,
    pub(crate) set_open_ticket: WriteSignal<Option<String>>,
    pub(crate) show_create: ReadSignal<bool>,
    pub(crate) set_show_create: WriteSignal<bool>,
    pub(crate) show_create_ticket: ReadSignal<bool>,
    pub(crate) set_show_create_ticket: WriteSignal<bool>,
    pub(crate) show_notifications: ReadSignal<bool>,
    pub(crate) set_show_notifications: WriteSignal<bool>,
    pub(crate) drag_task: ReadSignal<Option<String>>,
    pub(crate) set_drag_task: WriteSignal<Option<String>>,
    pub(crate) set_data: WriteSignal<Option<BootstrapDto>>,
    pub(crate) set_error: WriteSignal<Option<String>>,
}

pub(crate) fn dashboard(boot: BootstrapDto, signals: &AppSignals) -> View {
    // `AppSignals` is `Copy`; `drag_task`/`set_drag_task` are only used by
    // `main_view`, which receives `signals` directly below.
    let AppSignals {
        lang,
        set_lang,
        nav,
        set_nav,
        board_mode,
        set_board_mode,
        search_query,
        set_search_query,
        open_task,
        set_open_task,
        open_ticket,
        set_open_ticket,
        show_create,
        set_show_create,
        show_create_ticket,
        set_show_create_ticket,
        show_notifications,
        set_show_notifications,
        set_data,
        set_error,
        ..
    } = *signals;
    let unread = boot.notifications.iter().filter(|n| n.unread).count();
    let can_edit = boot.current_role.can_edit();
    let current_workspace_id = boot.workspace.id.clone();
    let workspaces = boot.workspaces.clone();
    let boot_for_title = boot.clone();
    let boot_for_subtitle = boot.clone();
    let boot_for_main = boot.clone();
    let signals_for_main = *signals;
    let boot_for_open = boot.clone();
    let boot_for_ticket_open = boot.clone();
    let boot_for_notifications = boot.clone();
    let boot_for_create = boot.clone();
    let boot_for_ticket_create = boot.clone();
    let logout_action = move |_| {
        set_search_query.set(String::new());
        spawn_local(async move {
            let _ = api_empty("/api/auth/logout").await;
            set_data.set(None);
        });
    };

    view! {
        <div class="app-shell">
            <aside class="sidebar">
                <button class="logo-button">{logo()}</button>
                <div class="workspace-switcher">
                    <span class="workspace-mark">"K"</span>
                    <span>
                        <strong>{boot.workspace.name.clone()}</strong>
                        <small>{format!("{} Mitglieder", boot.members.len())}</small>
                    </span>
                    {if workspaces.len() > 1 {
                        view! {
                            <select class="workspace-select" on:change=move |ev| {
                                switch_workspace(&select_value(&ev));
                            }>
                                {workspaces.into_iter().map(|workspace| {
                                    let selected = workspace.id == current_workspace_id;
                                    view! { <option value=workspace.id selected=selected>{workspace.name}</option> }
                                }).collect_view()}
                            </select>
                        }.into_view()
                    } else {
                        empty_view()
                    }}
                </div>

                <nav class="side-nav">
                    <span class="side-label">{move || lang.get().tr("Arbeitsbereich", "Workspace")}</span>
                    {nav_button(NavView::Overview, nav, set_nav, lang, None)}
                    {nav_button(NavView::Board, nav, set_nav, lang, Some(boot.tasks.iter().filter(|t| !t.status_is_done).count()))}
                    {nav_button(NavView::Tickets, nav, set_nav, lang, Some(boot.tickets.iter().filter(|t| !matches!(t.status, TicketStatus::Resolved | TicketStatus::Closed)).count()))}
                    {nav_button(NavView::Calendar, nav, set_nav, lang, None)}
                    <span class="side-label">{move || lang.get().tr("Planung", "Planning")}</span>
                    {nav_button(NavView::Gantt, nav, set_nav, lang, None)}
                    {nav_button(NavView::Roadmap, nav, set_nav, lang, None)}
                    {nav_button(NavView::Team, nav, set_nav, lang, None)}
                    {nav_button(NavView::Admin, nav, set_nav, lang, None)}
                    {nav_button(NavView::Settings, nav, set_nav, lang, None)}
                </nav>

                <div class="user-card">
                    <span class="avatar">{initials(&boot.current_user.name)}</span>
                    <span>
                        <strong>{boot.current_user.name.clone()}</strong>
                        <small>{boot.current_user.email.clone()}</small>
                    </span>
                    <button title="Logout" on:click=logout_action>"↗"</button>
                </div>
            </aside>

            <main class="main">
                <header class="topbar">
                    <div class="search" class:active=move || search_is_active(&search_query.get())>
                        {app_icon(AppIcon::Search)}
                        <input
                            aria-label=move || lang.get().tr("Aufgaben und Tickets suchen", "Search tasks and tickets")
                            placeholder=move || lang.get().tr("Aufgaben und Tickets suchen...", "Search tasks and tickets...")
                            prop:value=search_query
                            on:input=move |ev| set_search_query.set(event_target_value(&ev))
                        />
                        {move || if search_is_active(&search_query.get()) {
                            view! {
                                <button class="search-clear" aria-label=move || lang.get().tr("Suche leeren", "Clear search") on:click=move |_| set_search_query.set(String::new())>"x"</button>
                            }.into_view()
                        } else {
                            empty_view()
                        }}
                    </div>
                    <span class="demo-pill">{move || lang.get().tr("Demo-Vorschau", "Demo preview")}</span>
                    <LangToggle lang set_lang/>
                    <span class="notif-wrap">
                        <button class="icon-button" aria-label=move || lang.get().tr("Benachrichtigungen", "Notifications") on:click=move |_| set_show_notifications.update(|v| *v = !*v)>
                            {app_icon(AppIcon::Bell)}
                            {move || if unread > 0 { view! { <b class="dot"></b> }.into_view() } else { empty_view() }}
                        </button>
                        {move || if show_notifications.get() {
                            notifications_panel(boot_for_notifications.notifications.clone(), boot_for_notifications.tasks.clone(), lang, set_show_notifications, set_data, set_error).into_view()
                        } else {
                            empty_view()
                        }}
                    </span>
                    {move || if can_edit {
                        view! {
                            <button class="btn primary" on:click=move |_| {
                                if nav.get_untracked() == NavView::Tickets {
                                    set_show_create_ticket.set(true);
                                } else {
                                    set_show_create.set(true);
                                }
                            }>
                                "+ "
                                {move || match (nav.get(), lang.get()) {
                                    (NavView::Tickets, Lang::De) => "Neues Ticket",
                                    (NavView::Tickets, Lang::En) => "New ticket",
                                    (_, Lang::De) => "Neue Aufgabe",
                                    (_, Lang::En) => "New task",
                                }}
                            </button>
                        }.into_view()
                    } else {
                        empty_view()
                    }}
                </header>

                <section class="page-head">
                    <div>
                        <h1>{move || {
                            let query = search_query.get();
                            if search_is_active(&query) {
                                lang.get().tr("Suche", "Search").to_string()
                            } else {
                                header_title(&boot_for_title, nav.get(), lang.get())
                            }
                        }}</h1>
                        <p>{move || {
                            let query = search_query.get();
                            if search_is_active(&query) {
                                search_subtitle(&boot_for_subtitle, lang.get(), &query)
                            } else {
                                header_subtitle(&boot_for_subtitle, nav.get(), lang.get())
                            }
                        }}</p>
                    </div>
                    {move || if nav.get() == NavView::Board && !search_is_active(&search_query.get()) {
                        view! {
                            <div class="segmented">
                                <button class:active=move || board_mode.get() == "board" on:click=move |_| set_board_mode.set("board".to_string())>"Board"</button>
                                <button class:active=move || board_mode.get() == "list" on:click=move |_| set_board_mode.set("list".to_string())>{move || lang.get().tr("Liste", "List")}</button>
                            </div>
                        }.into_view()
                    } else {
                        empty_view()
                    }}
                </section>

                <section class="content">
                    {move || main_view(boot_for_main.clone(), &signals_for_main)}
                </section>
            </main>

            {move || if can_edit && show_create.get() {
                create_task_modal(boot_for_create.clone(), lang, set_show_create, set_open_task, set_data, set_error).into_view()
            } else {
                empty_view()
            }}

            {move || if can_edit && show_create_ticket.get() {
                create_ticket_modal(boot_for_ticket_create.clone(), lang, set_show_create_ticket, set_data, set_error).into_view()
            } else {
                empty_view()
            }}

            {move || open_task.get().and_then(|id| boot_for_open.tasks.iter().find(|t| t.id == id).cloned()).map(|task| {
                task_detail(task, boot_for_open.clone(), lang, set_open_task, set_data, set_error)
            })}

            {move || open_ticket.get().and_then(|id| boot_for_ticket_open.tickets.iter().find(|t| t.id == id).cloned()).map(|ticket| {
                ticket_detail(ticket, boot_for_ticket_open.clone(), lang, set_open_ticket, set_data, set_error)
            })}
        </div>
    }.into_view()
}

pub(crate) fn nav_button(
    view: NavView,
    nav: ReadSignal<NavView>,
    set_nav: WriteSignal<NavView>,
    lang: ReadSignal<Lang>,
    badge: Option<usize>,
) -> View {
    view! {
        <button class="side-item" class:active=move || nav.get() == view on:click=move |_| set_nav.set(view)>
            <span class="side-icon">{app_icon(nav_icon(view))}</span>
            <span>{move || view.label(lang.get())}</span>
            {badge.map(|b| view! { <small>{b}</small> })}
        </button>
    }.into_view()
}

pub(crate) fn main_view(boot: BootstrapDto, signals: &AppSignals) -> View {
    let AppSignals {
        lang,
        nav,
        set_nav,
        board_mode,
        search_query,
        set_search_query,
        set_open_task,
        drag_task,
        set_drag_task,
        set_show_create,
        set_show_create_ticket,
        set_open_ticket,
        set_data,
        set_error,
        ..
    } = *signals;

    let query = search_query.get();
    if search_is_active(&query) {
        return search_results_view(
            boot,
            lang,
            set_open_task,
            set_open_ticket,
            set_search_query,
            query,
        );
    }

    match nav.get() {
        NavView::Overview => overview_view(boot, lang, set_open_task, set_data, set_error),
        NavView::Board if board_mode.get() == "list" => list_view(boot, lang, set_open_task),
        NavView::Board => board_view(
            boot,
            lang,
            set_open_task,
            drag_task,
            set_drag_task,
            set_show_create,
            set_data,
            set_error,
        ),
        NavView::Tickets => ticket_view(boot, lang, set_show_create_ticket, set_open_ticket),
        NavView::Calendar => calendar_view(boot, lang, set_nav, set_open_task),
        NavView::Gantt => gantt_view(boot, lang, set_open_task),
        NavView::Roadmap => roadmap_view(boot, lang, set_open_task),
        NavView::Team => team_view(boot, lang),
        NavView::Admin => admin_view(boot, lang, set_data, set_error),
        NavView::Settings => settings_view(lang),
    }
}

pub(crate) fn stat(icon: AppIcon, value: usize, label: &'static str, tone: &'static str) -> View {
    view! {
        <article class=format!("stat-card {tone}")><span>{app_icon(icon)}</span><strong>{value}</strong><small>{label}</small></article>
    }.into_view()
}

/// Empty placeholder rendered where a conditional branch contributes nothing.
/// Leptos still needs a node, so this stands in for the `else` of inline `if`s.
pub(crate) fn empty_view() -> View {
    view! { <span/> }.into_view()
}

/// Native `window.confirm` dialog. Returns false when the browser has no
/// window or blocks the prompt, so callers treat "no answer" as "cancel".
pub(crate) fn confirm(message: &str) -> bool {
    web_sys::window()
        .and_then(|w| w.confirm_with_message(message).ok())
        .unwrap_or(false)
}

/// Native confirm for a plain "delete &lt;name&gt;?" action. Returns false on cancel.
pub(crate) fn confirm_delete(name: &str, lang: Lang) -> bool {
    let message = if lang.is_de() {
        format!("{name} wirklich löschen?")
    } else {
        format!("Delete {name}?")
    };
    confirm(&message)
}

/// Confirm removing a member from the workspace.
pub(crate) fn confirm_remove_member(name: &str, lang: Lang) -> bool {
    let message = if lang.is_de() {
        format!("{name} wirklich aus dem Workspace entfernen?")
    } else {
        format!("Remove {name} from the workspace?")
    };
    confirm(&message)
}

/// Confirm deleting an attachment.
pub(crate) fn confirm_delete_attachment(name: &str, lang: Lang) -> bool {
    let message = if lang.is_de() {
        format!("Anhang {name} wirklich löschen?")
    } else {
        format!("Delete attachment {name}?")
    };
    confirm(&message)
}

/// Validates a required title field. When empty, sets the in-form error to the
/// localized message and returns false so the caller bails out; otherwise
/// clears the error and returns true.
pub(crate) fn require_title(
    value: &str,
    de: &'static str,
    en: &'static str,
    lang: Lang,
    set_local_error: WriteSignal<Option<String>>,
) -> bool {
    if value.trim().is_empty() {
        set_local_error.set(Some(lang.tr(de, en).to_string()));
        return false;
    }
    set_local_error.set(None);
    true
}

/// Mirrors an API failure into both the in-form and the global error signals.
pub(crate) fn report_api_error(
    err: &ApiError,
    set_local_error: WriteSignal<Option<String>>,
    set_error: WriteSignal<Option<String>>,
) {
    set_local_error.set(Some(err.message.clone()));
    set_error.set(Some(err.message.clone()));
}

pub(crate) fn logo() -> View {
    view! {
        <span class="logo">
            <i><b></b><b></b><b></b></i>
            <span>"KoWoBau-Planner"</span>
        </span>
    }
    .into_view()
}

pub(crate) fn header_title(boot: &BootstrapDto, nav: NavView, lang: Lang) -> String {
    match (nav, lang) {
        (NavView::Overview, Lang::De) => {
            format!("Guten Morgen, {}", first_name(&boot.current_user.name))
        }
        (NavView::Overview, Lang::En) => {
            format!("Good morning, {}", first_name(&boot.current_user.name))
        }
        (NavView::Board, Lang::De) => "Aufgaben-Board".into(),
        (NavView::Board, Lang::En) => "Task board".into(),
        (NavView::Tickets, Lang::De) => "Tickets".into(),
        (NavView::Tickets, Lang::En) => "Tickets".into(),
        (NavView::Calendar, Lang::De) => "Kalender".into(),
        (NavView::Calendar, Lang::En) => "Calendar".into(),
        (NavView::Gantt, Lang::De) => "Gantt-Diagramm".into(),
        (NavView::Gantt, Lang::En) => "Gantt chart".into(),
        (NavView::Roadmap, Lang::De) => "Bau-Roadmap".into(),
        (NavView::Roadmap, Lang::En) => "Project roadmap".into(),
        (NavView::Team, Lang::De) => "Team".into(),
        (NavView::Team, Lang::En) => "Team".into(),
        (NavView::Admin, Lang::De) => "Administration".into(),
        (NavView::Admin, Lang::En) => "Administration".into(),
        (NavView::Settings, Lang::De) => "Einstellungen".into(),
        (NavView::Settings, Lang::En) => "Settings".into(),
    }
}

pub(crate) fn header_subtitle(boot: &BootstrapDto, nav: NavView, lang: Lang) -> String {
    let today = today_iso();
    let due_today = boot
        .tasks
        .iter()
        .filter(|t| !t.status_is_done && t.due_date.as_deref() == Some(today.as_str()))
        .count();
    match (nav, lang) {
        (NavView::Overview, Lang::De) => match due_today {
            1 => "Du hast 1 Aufgabe heute fällig.".into(),
            n => format!("Du hast {n} Aufgaben heute fällig."),
        },
        (NavView::Overview, Lang::En) => match due_today {
            1 => "You have 1 task due today.".into(),
            n => format!("You have {n} tasks due today."),
        },
        (NavView::Board, Lang::De) => format!(
            "{} Aufgaben · {} Spalten",
            boot.tasks.len(),
            boot.statuses.len()
        ),
        (NavView::Board, Lang::En) => format!(
            "{} tasks · {} columns",
            boot.tasks.len(),
            boot.statuses.len()
        ),
        (NavView::Tickets, Lang::De) => format!(
            "{} Tickets · {} offen",
            boot.tickets.len(),
            boot.tickets
                .iter()
                .filter(|t| matches!(t.status, TicketStatus::Open | TicketStatus::InProgress))
                .count()
        ),
        (NavView::Tickets, Lang::En) => format!(
            "{} tickets · {} open",
            boot.tickets.len(),
            boot.tickets
                .iter()
                .filter(|t| matches!(t.status, TicketStatus::Open | TicketStatus::InProgress))
                .count()
        ),
        (NavView::Calendar, Lang::De) => "Fälligkeiten und Meilensteine".into(),
        (NavView::Calendar, Lang::En) => "Due dates and milestones".into(),
        (NavView::Gantt, Lang::De) => "Zeitplan, Abhängigkeiten und Meilensteine".into(),
        (NavView::Gantt, Lang::En) => "Schedule, dependencies and milestones".into(),
        (NavView::Roadmap, Lang::De) => "Initiativen nach Zeithorizont".into(),
        (NavView::Roadmap, Lang::En) => "Initiatives by horizon".into(),
        (NavView::Team, Lang::De) => {
            format!("{} Mitglieder · {}", boot.members.len(), boot.project.name)
        }
        (NavView::Team, Lang::En) => {
            format!("{} members · {}", boot.members.len(), boot.project.name)
        }
        (NavView::Admin, Lang::De) => "Mitglieder, Rollen, System und Sicherheit".into(),
        (NavView::Admin, Lang::En) => "Members, roles, system and security".into(),
        (NavView::Settings, Lang::De) => "Design und persönliche Einstellungen".into(),
        (NavView::Settings, Lang::En) => "Appearance and personal preferences".into(),
    }
}

pub(crate) fn nav_icon(view: NavView) -> AppIcon {
    match view {
        NavView::Overview => AppIcon::Dashboard,
        NavView::Board => AppIcon::Kanban,
        NavView::Tickets => AppIcon::Ticket,
        NavView::Calendar => AppIcon::Calendar,
        NavView::Gantt => AppIcon::Timeline,
        NavView::Roadmap => AppIcon::Roadmap,
        NavView::Team => AppIcon::Users,
        NavView::Admin => AppIcon::Settings,
        NavView::Settings => AppIcon::Sliders,
    }
}
