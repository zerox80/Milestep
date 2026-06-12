use crate::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn dashboard(
    boot: BootstrapDto,
    lang: ReadSignal<Lang>,
    set_lang: WriteSignal<Lang>,
    nav: ReadSignal<NavView>,
    set_nav: WriteSignal<NavView>,
    board_mode: ReadSignal<String>,
    set_board_mode: WriteSignal<String>,
    open_task: ReadSignal<Option<String>>,
    set_open_task: WriteSignal<Option<String>>,
    open_ticket: ReadSignal<Option<String>>,
    set_open_ticket: WriteSignal<Option<String>>,
    show_create: ReadSignal<bool>,
    set_show_create: WriteSignal<bool>,
    show_create_ticket: ReadSignal<bool>,
    set_show_create_ticket: WriteSignal<bool>,
    show_notifications: ReadSignal<bool>,
    set_show_notifications: WriteSignal<bool>,
    drag_task: ReadSignal<Option<String>>,
    set_drag_task: WriteSignal<Option<String>>,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) -> View {
    let unread = boot.notifications.iter().filter(|n| n.unread).count();
    let title = header_title(&boot, nav.get(), lang.get());
    let subtitle = header_subtitle(&boot, nav.get(), lang.get());
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
                    {main_view(
                        boot_for_main,
                        lang,
                        nav,
                        board_mode,
                        set_open_task,
                        drag_task,
                        set_drag_task,
                        set_show_create,
                        set_show_create_ticket,
                        set_open_ticket,
                        set_data,
                        set_error,
                    )}
                </section>
            </main>

            {move || if show_create.get() {
                create_task_modal(boot_for_create.clone(), lang, set_show_create, set_open_task, set_data, set_error).into_view()
            } else {
                view! { <span/> }.into_view()
            }}

            {move || if show_create_ticket.get() {
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
            <span class="side-icon">{nav_icon(view)}</span>
            <span>{move || view.label(lang.get())}</span>
            {badge.map(|b| view! { <small>{b}</small> })}
        </button>
    }.into_view()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn main_view(
    boot: BootstrapDto,
    lang: ReadSignal<Lang>,
    nav: ReadSignal<NavView>,
    board_mode: ReadSignal<String>,
    set_open_task: WriteSignal<Option<String>>,
    drag_task: ReadSignal<Option<String>>,
    set_drag_task: WriteSignal<Option<String>>,
    set_show_create: WriteSignal<bool>,
    set_show_create_ticket: WriteSignal<bool>,
    set_open_ticket: WriteSignal<Option<String>>,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) -> View {
    match nav.get() {
        NavView::Overview => overview_view(boot, lang, set_open_task),
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
        NavView::Calendar => calendar_view(boot, lang, set_open_task),
        NavView::Gantt => gantt_view(boot, lang, set_open_task),
        NavView::Roadmap => roadmap_view(boot, lang, set_open_task),
        NavView::Team => team_view(boot, lang),
        NavView::Admin => admin_view(boot, lang, set_data, set_error),
    }
}

pub(crate) fn stat(
    icon: &'static str,
    value: usize,
    label: &'static str,
    tone: &'static str,
) -> View {
    view! {
        <article class=format!("stat-card {tone}")><span>{icon}</span><strong>{value}</strong><small>{label}</small></article>
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
        (NavView::Calendar, Lang::De) => {
            let (_, m, _) = now_date();
            format!(
                "Fälligkeiten und Meilensteine im {}",
                MONTHS_DE_FULL[(m - 1) as usize]
            )
        }
        (NavView::Calendar, Lang::En) => {
            let (_, m, _) = now_date();
            format!(
                "Due dates and milestones in {}",
                MONTHS_EN_FULL[(m - 1) as usize]
            )
        }
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
    }
}

pub(crate) fn nav_icon(view: NavView) -> &'static str {
    match view {
        NavView::Overview => "□",
        NavView::Board => "▤",
        NavView::Tickets => "T",
        NavView::Calendar => "◫",
        NavView::Gantt => "≋",
        NavView::Roadmap => "◇",
        NavView::Team => "♙",
        NavView::Admin => "⚙",
    }
}
