use crate::*;

#[derive(Clone, Copy)]
pub(crate) struct AppSignals {
    pub(crate) lang: ReadSignal<Lang>,
    pub(crate) set_lang: WriteSignal<Lang>,
    pub(crate) nav: ReadSignal<NavView>,
    pub(crate) set_nav: WriteSignal<NavView>,
    pub(crate) board_mode: ReadSignal<String>,
    pub(crate) set_board_mode: WriteSignal<String>,
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
    let lang = signals.lang;
    let set_lang = signals.set_lang;
    let nav = signals.nav;
    let set_nav = signals.set_nav;
    let board_mode = signals.board_mode;
    let set_board_mode = signals.set_board_mode;
    let open_task = signals.open_task;
    let set_open_task = signals.set_open_task;
    let open_ticket = signals.open_ticket;
    let set_open_ticket = signals.set_open_ticket;
    let show_create = signals.show_create;
    let set_show_create = signals.set_show_create;
    let show_create_ticket = signals.show_create_ticket;
    let set_show_create_ticket = signals.set_show_create_ticket;
    let show_notifications = signals.show_notifications;
    let set_show_notifications = signals.set_show_notifications;
    let set_data = signals.set_data;
    let set_error = signals.set_error;
    let unread = boot.notifications.iter().filter(|n| n.unread).count();
    let can_edit = boot.current_role.can_edit();
    let title = header_title(&boot, nav.get(), lang.get());
    let subtitle = header_subtitle(&boot, nav.get(), lang.get());
    let current_workspace_id = boot.workspace.id.clone();
    let workspaces = boot.workspaces.clone();
    let boot_for_main = boot.clone();
    let boot_for_open = boot.clone();
    let boot_for_ticket_open = boot.clone();
    let boot_for_notifications = boot.clone();
    let boot_for_create = boot.clone();
    let boot_for_ticket_create = boot.clone();
    let logout_action = move |_| {
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
                        view! { <span/> }.into_view()
                    }}
                </div>

                <nav class="side-nav">
                    <span class="side-label">{move || if lang.get() == Lang::De { "Arbeitsbereich" } else { "Workspace" }}</span>
                    {nav_button(NavView::Overview, nav, set_nav, lang, None)}
                    {nav_button(NavView::Board, nav, set_nav, lang, Some(boot.tasks.iter().filter(|t| !t.status_is_done).count()))}
                    {nav_button(NavView::Tickets, nav, set_nav, lang, Some(boot.tickets.iter().filter(|t| !matches!(t.status, TicketStatus::Resolved | TicketStatus::Closed)).count()))}
                    {nav_button(NavView::Calendar, nav, set_nav, lang, None)}
                    <span class="side-label">{move || if lang.get() == Lang::De { "Planung" } else { "Planning" }}</span>
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
                    <div class="search">"⌕" <input placeholder=move || if lang.get() == Lang::De { "Suchen..." } else { "Search..." }/></div>
                    <span class="demo-pill">{move || if lang.get() == Lang::De { "Demo-Vorschau" } else { "Demo preview" }}</span>
                    <LangToggle lang set_lang/>
                    <span class="notif-wrap">
                        <button class="icon-button" on:click=move |_| set_show_notifications.update(|v| *v = !*v)>
                            "◌"
                            {move || if unread > 0 { view! { <b class="dot"></b> }.into_view() } else { view! { <span/> }.into_view() }}
                        </button>
                        {move || if show_notifications.get() {
                            notifications_panel(boot_for_notifications.notifications.clone(), boot_for_notifications.tasks.clone(), lang, set_show_notifications, set_data, set_error).into_view()
                        } else {
                            view! { <span/> }.into_view()
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
                        view! { <span/> }.into_view()
                    }}
                </header>

                <section class="page-head">
                    <div>
                        <h1>{title}</h1>
                        <p>{subtitle}</p>
                    </div>
                    {move || if nav.get() == NavView::Board {
                        view! {
                            <div class="segmented">
                                <button class:active=move || board_mode.get() == "board" on:click=move |_| set_board_mode.set("board".to_string())>"Board"</button>
                                <button class:active=move || board_mode.get() == "list" on:click=move |_| set_board_mode.set("list".to_string())>{move || if lang.get() == Lang::De { "Liste" } else { "List" }}</button>
                            </div>
                        }.into_view()
                    } else {
                        view! { <span/> }.into_view()
                    }}
                </section>

                <section class="content">
                    {main_view(boot_for_main, signals)}
                </section>
            </main>

            {move || if can_edit && show_create.get() {
                create_task_modal(boot_for_create.clone(), lang, set_show_create, set_open_task, set_data, set_error).into_view()
            } else {
                view! { <span/> }.into_view()
            }}

            {move || if can_edit && show_create_ticket.get() {
                create_ticket_modal(boot_for_ticket_create.clone(), lang, set_show_create_ticket, set_data, set_error).into_view()
            } else {
                view! { <span/> }.into_view()
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
    let lang = signals.lang;
    let nav = signals.nav;
    let set_nav = signals.set_nav;
    let board_mode = signals.board_mode;
    let set_open_task = signals.set_open_task;
    let drag_task = signals.drag_task;
    let set_drag_task = signals.set_drag_task;
    let set_show_create = signals.set_show_create;
    let set_show_create_ticket = signals.set_show_create_ticket;
    let set_open_ticket = signals.set_open_ticket;
    let set_data = signals.set_data;
    let set_error = signals.set_error;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AppIcon {
    Alert,
    Calendar,
    CheckCircle,
    Clock,
    Dashboard,
    Flag,
    Kanban,
    Roadmap,
    Settings,
    Sliders,
    Ticket,
    Timeline,
    Users,
}

pub(crate) fn app_icon(icon: AppIcon) -> View {
    match icon {
        AppIcon::Alert => view! {
            <svg class="app-icon" aria-hidden="true" focusable="false" viewBox="0 0 24 24">
                <path d="M12 8v5"></path>
                <path d="M12 17h.01"></path>
                <path d="M10.3 4.9 3.1 17.3A2 2 0 0 0 4.8 20h14.4a2 2 0 0 0 1.7-2.7L13.7 4.9a2 2 0 0 0-3.4 0Z"></path>
            </svg>
        }.into_view(),
        AppIcon::Calendar => view! {
            <svg class="app-icon" aria-hidden="true" focusable="false" viewBox="0 0 24 24">
                <path d="M8 3v4"></path>
                <path d="M16 3v4"></path>
                <path d="M4 9h16"></path>
                <rect x="4" y="5" width="16" height="16" rx="3"></rect>
                <path d="M8 13h.01"></path>
                <path d="M12 13h.01"></path>
                <path d="M16 13h.01"></path>
            </svg>
        }.into_view(),
        AppIcon::CheckCircle => view! {
            <svg class="app-icon" aria-hidden="true" focusable="false" viewBox="0 0 24 24">
                <circle cx="12" cy="12" r="8"></circle>
                <path d="m8.7 12.2 2.1 2.1 4.6-4.9"></path>
            </svg>
        }.into_view(),
        AppIcon::Clock => view! {
            <svg class="app-icon" aria-hidden="true" focusable="false" viewBox="0 0 24 24">
                <circle cx="12" cy="12" r="8"></circle>
                <path d="M12 8v4l3 2"></path>
            </svg>
        }.into_view(),
        AppIcon::Dashboard => view! {
            <svg class="app-icon" aria-hidden="true" focusable="false" viewBox="0 0 24 24">
                <rect x="4" y="4" width="7" height="7" rx="2"></rect>
                <rect x="13" y="4" width="7" height="5" rx="2"></rect>
                <rect x="13" y="11" width="7" height="9" rx="2"></rect>
                <rect x="4" y="13" width="7" height="7" rx="2"></rect>
            </svg>
        }.into_view(),
        AppIcon::Flag => view! {
            <svg class="app-icon" aria-hidden="true" focusable="false" viewBox="0 0 24 24">
                <path d="M6 21V4"></path>
                <path d="M6 5h10l-1.4 4L16 13H6"></path>
            </svg>
        }.into_view(),
        AppIcon::Kanban => view! {
            <svg class="app-icon" aria-hidden="true" focusable="false" viewBox="0 0 24 24">
                <rect x="4" y="4" width="5" height="16" rx="2"></rect>
                <rect x="10.5" y="4" width="5" height="10" rx="2"></rect>
                <rect x="17" y="4" width="3" height="13" rx="1.5"></rect>
            </svg>
        }.into_view(),
        AppIcon::Roadmap => view! {
            <svg class="app-icon" aria-hidden="true" focusable="false" viewBox="0 0 24 24">
                <path d="M5 19c2.8 0 2.8-4 5.6-4h2.8c2.8 0 2.8-4 5.6-4"></path>
                <circle cx="5" cy="19" r="2"></circle>
                <circle cx="12" cy="15" r="2"></circle>
                <path d="M17 4v8"></path>
                <path d="M17 5h4l-1 2 1 2h-4"></path>
            </svg>
        }.into_view(),
        AppIcon::Settings => view! {
            <svg class="app-icon" aria-hidden="true" focusable="false" viewBox="0 0 24 24">
                <circle cx="12" cy="12" r="3"></circle>
                <path d="M12 3v3"></path>
                <path d="M12 18v3"></path>
                <path d="M3 12h3"></path>
                <path d="M18 12h3"></path>
                <path d="m5.6 5.6 2.1 2.1"></path>
                <path d="m16.3 16.3 2.1 2.1"></path>
                <path d="m18.4 5.6-2.1 2.1"></path>
                <path d="m7.7 16.3-2.1 2.1"></path>
            </svg>
        }.into_view(),
        AppIcon::Sliders => view! {
            <svg class="app-icon" aria-hidden="true" focusable="false" viewBox="0 0 24 24">
                <path d="M4 6h16"></path>
                <path d="M4 12h16"></path>
                <path d="M4 18h16"></path>
                <circle cx="9" cy="6" r="2"></circle>
                <circle cx="15" cy="12" r="2"></circle>
                <circle cx="8" cy="18" r="2"></circle>
            </svg>
        }.into_view(),
        AppIcon::Ticket => view! {
            <svg class="app-icon" aria-hidden="true" focusable="false" viewBox="0 0 24 24">
                <path d="M4 8a2 2 0 0 1 2-2h12a2 2 0 0 1 2 2v2.2a2 2 0 0 0 0 3.6V16a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2v-2.2a2 2 0 0 0 0-3.6V8Z"></path>
                <path d="M9 8v8"></path>
            </svg>
        }.into_view(),
        AppIcon::Timeline => view! {
            <svg class="app-icon" aria-hidden="true" focusable="false" viewBox="0 0 24 24">
                <path d="M5 7h7"></path>
                <path d="M5 12h14"></path>
                <path d="M5 17h10"></path>
                <path d="M4 5v14"></path>
            </svg>
        }.into_view(),
        AppIcon::Users => view! {
            <svg class="app-icon" aria-hidden="true" focusable="false" viewBox="0 0 24 24">
                <circle cx="9" cy="8" r="3"></circle>
                <path d="M4 19a5 5 0 0 1 10 0"></path>
                <path d="M16 11a2.5 2.5 0 0 0 0-5"></path>
                <path d="M17 19a4 4 0 0 0-3-3.8"></path>
            </svg>
        }.into_view(),
    }
}

pub(crate) fn stat(icon: AppIcon, value: usize, label: &'static str, tone: &'static str) -> View {
    view! {
        <article class=format!("stat-card {tone}")><span>{app_icon(icon)}</span><strong>{value}</strong><small>{label}</small></article>
    }.into_view()
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
