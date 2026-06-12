use gloo_net::http::Request;
use kowobau_shared::*;
use leptos::*;
use leptos_router::*;
use serde::{de::DeserializeOwned, Serialize};
use wasm_bindgen_futures::spawn_local;
use web_sys::{DragEvent, HtmlSelectElement, HtmlTextAreaElement, RequestCredentials};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lang {
    De,
    En,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NavView {
    Overview,
    Board,
    Tickets,
    Calendar,
    Gantt,
    Roadmap,
    Team,
    Admin,
}

impl NavView {
    fn label(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Overview, Lang::De) => "Übersicht",
            (Self::Overview, Lang::En) => "Overview",
            (Self::Board, Lang::De) => "Board",
            (Self::Board, Lang::En) => "Board",
            (Self::Tickets, Lang::De) => "Tickets",
            (Self::Tickets, Lang::En) => "Tickets",
            (Self::Calendar, Lang::De) => "Kalender",
            (Self::Calendar, Lang::En) => "Calendar",
            (Self::Gantt, Lang::De) => "Gantt",
            (Self::Gantt, Lang::En) => "Gantt",
            (Self::Roadmap, Lang::De) => "Roadmap",
            (Self::Roadmap, Lang::En) => "Roadmap",
            (Self::Team, Lang::De) => "Team",
            (Self::Team, Lang::En) => "Team",
            (Self::Admin, Lang::De) => "Admin",
            (Self::Admin, Lang::En) => "Admin",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthMode {
    Login,
    Register,
}

fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(|| {
        view! {
            <Router>
                <Routes>
                    <Route path="/" view=|| view! { <AppRoot/> }/>
                    <Route path="/app" view=|| view! { <AppRoot/> }/>
                </Routes>
            </Router>
        }
    });
}

#[component]
fn AppRoot() -> impl IntoView {
    let (lang, set_lang) = create_signal(Lang::De);
    let (data, set_data) = create_signal::<Option<BootstrapDto>>(None);
    let (nav, set_nav) = create_signal(NavView::Overview);
    let (board_mode, set_board_mode) = create_signal("board".to_string());
    let (open_task, set_open_task) = create_signal::<Option<String>>(None);
    let (open_ticket, set_open_ticket) = create_signal::<Option<String>>(None);
    let (show_create, set_show_create) = create_signal(false);
    let (show_create_ticket, set_show_create_ticket) = create_signal(false);
    let (show_notifications, set_show_notifications) = create_signal(false);
    let (drag_task, set_drag_task) = create_signal::<Option<String>>(None);
    let (loading, set_loading) = create_signal(true);
    let (error, set_error) = create_signal::<Option<String>>(None);
    // Counts open editors (modals, edit mode, comment drafts). Realtime
    // refetches wait until it drops to zero so a background set_data does not
    // re-render the dashboard and wipe in-progress input.
    let realtime_hold = create_rw_signal(0i32);
    provide_context(RealtimeHold(realtime_hold));

    let reload = move || {
        set_loading.set(true);
        spawn_local(async move {
            match api_get::<BootstrapDto>("/api/bootstrap").await {
                Ok(next) => {
                    set_data.set(Some(next));
                    set_error.set(None);
                }
                Err(err) if err.status == 401 => {
                    // Not logged in: show the auth screen without an error.
                    set_data.set(None);
                    set_error.set(None);
                }
                Err(err) => {
                    set_data.set(None);
                    set_error.set(Some(err.message));
                }
            }
            set_loading.set(false);
        });
    };

    create_effect(move |_| {
        if loading.get_untracked() && data.get_untracked().is_none() {
            reload();
        }
    });

    // Live updates: one WebSocket connection per login. The loop ends itself
    // on logout (data becomes None) and is restarted here after the next
    // successful bootstrap.
    let realtime_running = store_value(false);
    create_effect(move |_| {
        if data.get().is_some() && !realtime_running.get_value() {
            realtime_running.set_value(true);
            start_realtime(data, realtime_hold, realtime_running, set_data, set_error);
        }
    });

    view! {
        <div>
            {move || match data.get() {
                Some(boot) => dashboard(
                    boot,
                    lang,
                    set_lang,
                    nav,
                    set_nav,
                    board_mode,
                    set_board_mode,
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
                    drag_task,
                    set_drag_task,
                    set_data,
                    set_error,
                ).into_view(),
                None => auth_shell(lang, set_lang, reload, error, loading).into_view(),
            }}
        </div>
    }
}

fn auth_shell(
    lang: ReadSignal<Lang>,
    set_lang: WriteSignal<Lang>,
    reload: impl Fn() + Copy + 'static,
    error: ReadSignal<Option<String>>,
    loading: ReadSignal<bool>,
) -> View {
    // Invite tokens arrive as /?invite=<token>; pre-fill the code and open the
    // register form directly. The token is URL-safe base64, no decoding needed.
    let invite_from_url = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .and_then(|search| {
            search
                .trim_start_matches('?')
                .split('&')
                .find_map(|pair| pair.strip_prefix("invite=").map(str::to_string))
        })
        .filter(|t| !t.is_empty());

    let (mode, set_mode) = create_signal(if invite_from_url.is_some() {
        AuthMode::Register
    } else {
        AuthMode::Login
    });
    let (name, set_name) = create_signal(String::new());
    let (email, set_email) = create_signal(String::new());
    let (password, set_password) = create_signal(String::new());
    let (invite, set_invite) = create_signal(invite_from_url.unwrap_or_default());
    let (busy, set_busy) = create_signal(false);
    let (local_error, set_local_error) = create_signal::<Option<String>>(None);

    let submit = move || {
        set_busy.set(true);
        set_local_error.set(None);
        let mode_now = mode.get_untracked();
        let name_now = name.get_untracked();
        let email_now = email.get_untracked();
        let password_now = password.get_untracked();
        let invite_now = invite.get_untracked();
        spawn_local(async move {
            let result = match mode_now {
                AuthMode::Login => {
                    api_post::<_, AuthResponse>(
                        "/api/auth/login",
                        &AuthRequest {
                            email: email_now,
                            password: password_now,
                        },
                    )
                    .await
                }
                AuthMode::Register => {
                    api_post::<_, AuthResponse>(
                        "/api/auth/register",
                        &RegisterRequest {
                            name: name_now,
                            email: email_now,
                            password: password_now,
                            invite_token: Some(invite_now.trim().to_string())
                                .filter(|t| !t.is_empty()),
                        },
                    )
                    .await
                }
            };

            match result {
                Ok(_) => reload(),
                Err(err) => set_local_error.set(Some(err.message)),
            }
            set_busy.set(false);
        });
    };

    let demo_login = move |_| {
        set_email.set("alex@firma.com".to_string());
        set_password.set("password123".to_string());
        submit();
    };

    view! {
        <main class="auth-page">
            <section class="auth-brand">
                <div class="brand-row">
                    {logo()}
                    <LangToggle lang set_lang/>
                </div>
                <div class="hero-copy">
                    <span class="eyebrow">"OPEN SOURCE · RUST · WASM"</span>
                    <h1>"KoWoBau-Planner"</h1>
                    <p>{move || if lang.get() == Lang::De {
                        "Projektmanagement für Bau- und Modernisierungsteams: Aufgaben, Termine, Meilensteine und Teamverantwortung in einem schnellen Self-Hosting-Tool."
                    } else {
                        "Project management for construction and modernization teams: tasks, dates, milestones and ownership in one fast self-hosted tool."
                    }}</p>
                    <div class="hero-actions">
                        <button class="btn primary" on:click=demo_login disabled=move || busy.get()>
                            {move || if lang.get() == Lang::De { "Demo öffnen" } else { "Open demo" }}
                        </button>
                        <button class="btn ghost" on:click=move |_| set_mode.set(AuthMode::Register)>
                            {move || if lang.get() == Lang::De { "Arbeitsbereich anlegen" } else { "Create workspace" }}
                        </button>
                    </div>
                </div>
                <div class="mini-board">
                    <div class="mini-col">
                        <span>"Geplant"</span>
                        <div>"Bemusterung vorbereiten"</div>
                        <div>"Gewerkefreigabe dokumentieren"</div>
                    </div>
                    <div class="mini-col active">
                        <span>"In Arbeit"</span>
                        <div>"Mängelaufnahme koordinieren"</div>
                        <div>"Terminplan aktualisieren"</div>
                    </div>
                    <div class="mini-col">
                        <span>"Review"</span>
                        <div>"Abnahmeprotokoll prüfen"</div>
                    </div>
                </div>
            </section>

            <section class="auth-card">
                <div class="auth-card-head">
                    <h2>{move || match (mode.get(), lang.get()) {
                        (AuthMode::Login, Lang::De) => "Willkommen zurück",
                        (AuthMode::Login, Lang::En) => "Welcome back",
                        (AuthMode::Register, Lang::De) => "Arbeitsbereich starten",
                        (AuthMode::Register, Lang::En) => "Start workspace",
                    }}</h2>
                    <p>{move || if lang.get() == Lang::De {
                        "Mit dem Demo-Konto kannst du sofort in den Planner springen."
                    } else {
                        "Use the demo account to jump straight into the planner."
                    }}</p>
                </div>

                {move || if mode.get() == AuthMode::Register {
                    view! {
                        <label class="field">
                            <span>"Name"</span>
                            <input prop:value=name on:input=move |ev| set_name.set(event_target_value(&ev))/>
                        </label>
                        <label class="field">
                            <span>{move || if lang.get() == Lang::De { "Einladungscode (optional)" } else { "Invite code (optional)" }}</span>
                            <input prop:value=invite on:input=move |ev| set_invite.set(event_target_value(&ev))/>
                        </label>
                    }.into_view()
                } else {
                    view! { <div/> }.into_view()
                }}

                <label class="field">
                    <span>"E-Mail"</span>
                    <input type="email" prop:value=email on:input=move |ev| set_email.set(event_target_value(&ev))/>
                </label>
                <label class="field">
                    <span>{move || if lang.get() == Lang::De { "Passwort" } else { "Password" }}</span>
                    <input type="password" prop:value=password on:input=move |ev| set_password.set(event_target_value(&ev))/>
                </label>

                {move || local_error.get().or_else(|| error.get()).map(|err| view! {
                    <div class="error-line">{err}</div>
                })}

                <button class="btn primary full" on:click=move |_| submit() disabled=move || busy.get() || loading.get()>
                    {move || match (mode.get(), lang.get(), busy.get()) {
                        (_, Lang::De, true) => "Bitte warten...",
                        (_, Lang::En, true) => "Please wait...",
                        (AuthMode::Login, Lang::De, false) => "Anmelden",
                        (AuthMode::Login, Lang::En, false) => "Log in",
                        (AuthMode::Register, Lang::De, false) => "Konto erstellen",
                        (AuthMode::Register, Lang::En, false) => "Create account",
                    }}
                </button>

                <button class="link-button" on:click=move |_| {
                    set_mode.set(if mode.get_untracked() == AuthMode::Login { AuthMode::Register } else { AuthMode::Login });
                }>
                    {move || match (mode.get(), lang.get()) {
                        (AuthMode::Login, Lang::De) => "Noch kein Konto? Registrieren",
                        (AuthMode::Login, Lang::En) => "No account yet? Sign up",
                        (AuthMode::Register, Lang::De) => "Schon ein Konto? Anmelden",
                        (AuthMode::Register, Lang::En) => "Already have an account? Log in",
                    }}
                </button>
            </section>
        </main>
    }.into_view()
}

#[component]
fn LangToggle(lang: ReadSignal<Lang>, set_lang: WriteSignal<Lang>) -> impl IntoView {
    view! {
        <div class="lang-toggle">
            <button class:active=move || lang.get() == Lang::De on:click=move |_| set_lang.set(Lang::De)>"DE"</button>
            <button class:active=move || lang.get() == Lang::En on:click=move |_| set_lang.set(Lang::En)>"EN"</button>
        </div>
    }
}

#[allow(clippy::too_many_arguments)]
fn dashboard(
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

fn nav_button(
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
fn main_view(
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

fn overview_view(
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
        .cloned()
        .collect::<Vec<_>>();
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
                        {today_tasks.into_iter().map(|task| task_row(task, boot.members.clone(), lang, set_open_task)).collect_view()}
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
                        <div class="activity-row"><span class="avatar tiny">{a.actor_name.as_deref().map(initials).unwrap_or_else(|| "S".into())}</span><span>{a.actor_name.clone().unwrap_or_else(|| "System".into())}" · "{a.action.clone()}</span><small>{if lang.get() == Lang::De { a.created_label_de.clone() } else { a.created_label_en.clone() }}</small></div>
                    }).collect_view()}
                </div>
            </div>
        </div>
    }.into_view()
}

#[allow(clippy::too_many_arguments)]
fn board_view(
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

fn list_view(
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
                let due = task.due_date.as_deref().map(|d| fmt_date(d, lang.get())).unwrap_or_else(|| "-".into());
                let assignees = task.assignee_ids.clone();
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

fn ticket_view(
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

fn calendar_view(
    boot: BootstrapDto,
    lang: ReadSignal<Lang>,
    set_open_task: WriteSignal<Option<String>>,
) -> View {
    let all_tasks = boot.tasks.clone();
    let (year, month, today_day) = now_date();
    view! {
        <div class="calendar-grid">
            {(1..=days_in_month(year, month)).map(|day| {
                let iso = format!("{year:04}-{month:02}-{day:02}");
                let tasks = all_tasks.iter().filter(|t| t.due_date.as_deref() == Some(iso.as_str())).cloned().collect::<Vec<_>>();
                view! {
                    <div class="day-cell" class:today=move || day == today_day>
                        <strong>{day}</strong>
                        {tasks.into_iter().take(3).map(|task| {
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
fn gantt_view(
    boot: BootstrapDto,
    lang: ReadSignal<Lang>,
    set_open_task: WriteSignal<Option<String>>,
) -> View {
    let statuses = boot.statuses.clone();
    let tasks: Vec<TaskDto> = boot
        .tasks
        .clone()
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
fn roadmap_view(
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
    let all_tasks = boot.tasks.clone();
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
fn team_view(boot: BootstrapDto, lang: ReadSignal<Lang>) -> View {
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

fn admin_view(
    boot: BootstrapDto,
    lang: ReadSignal<Lang>,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) -> View {
    let can_admin = boot.current_role.can_admin();
    let can_owner = boot.current_role == Role::Owner;
    let workspace_id = boot.workspace.id.clone();
    let current_user_id = boot.current_user.id.clone();
    let (invite_email, set_invite_email) = create_signal(String::new());
    let (invite_role, set_invite_role) = create_signal(Role::Member);
    let (invite_result, set_invite_result) = create_signal::<Option<String>>(None);
    let (local_error, set_local_error) = create_signal::<Option<String>>(None);
    let workspace_id_for_invite = workspace_id.clone();
    let invite = move |_| {
        if !can_admin {
            return;
        }
        let email = invite_email.get_untracked();
        if email.trim().is_empty() {
            set_local_error.set(Some(if lang.get_untracked() == Lang::De {
                "Bitte gib eine E-Mail ein.".into()
            } else {
                "Enter an email first.".into()
            }));
            return;
        }
        set_local_error.set(None);
        set_invite_result.set(None);
        let role = invite_role.get_untracked();
        let workspace_id = workspace_id_for_invite.clone();
        spawn_local(async move {
            match api_post::<_, InviteMemberResponse>(
                &format!("/api/workspaces/{workspace_id}/invites"),
                &InviteMemberRequest { email, role },
            )
            .await
            {
                Ok(res) => {
                    if let Some(path) = res.invite_path {
                        let origin = web_sys::window()
                            .and_then(|w| w.location().origin().ok())
                            .unwrap_or_default();
                        set_invite_result.set(Some(format!("{origin}{path}")));
                    } else {
                        match api_get::<BootstrapDto>("/api/bootstrap").await {
                            Ok(next) => {
                                set_data.set(Some(next));
                                set_invite_email.set(String::new());
                                set_invite_result.set(Some(if lang.get_untracked() == Lang::De {
                                    "Bestehender User wurde direkt hinzugefügt.".into()
                                } else {
                                    "Existing user was added directly.".into()
                                }));
                            }
                            Err(err) => set_error.set(Some(err.message)),
                        }
                    }
                }
                Err(err) => {
                    set_local_error.set(Some(err.message.clone()));
                    set_error.set(Some(err.message));
                }
            }
        });
    };

    view! {
        <div class="admin-grid">
            <section class="panel">
                <h3>{move || if lang.get() == Lang::De { "Mitglieder" } else { "Members" }}</h3>
                {boot.members.iter().map(|m| {
                    let membership_id = m.id.clone();
                    let remove_id = m.id.clone();
                    let current_role = m.role.clone();
                    let is_current_user = m.user_id == current_user_id;
                    let member_name = m.name.clone();
                    let member_name_for_remove = m.name.clone();
                    let can_change_owner_target = can_owner || current_role != Role::Owner;
                    view! {
                    <div class="admin-row">
                        <span class="avatar tiny">{m.initials.clone()}</span>
                        <strong>{m.name.clone()}</strong>
                        <small>{m.email.clone()}</small>
                        {if can_admin && can_change_owner_target {
                            view! {
                                <select class="role-select" on:change=move |ev| {
                                    update_member_role(
                                        membership_id.clone(),
                                        role_from_value(&select_value(&ev)),
                                        set_data,
                                        set_error,
                                    );
                                }>
                                    <option value="owner" selected=current_role == Role::Owner disabled=!can_owner>"Owner"</option>
                                    <option value="admin" selected=current_role == Role::Admin>"Admin"</option>
                                    <option value="member" selected=current_role == Role::Member>{move || if lang.get() == Lang::De { "Mitglied" } else { "Member" }}</option>
                                    <option value="viewer" selected=current_role == Role::Viewer>{move || if lang.get() == Lang::De { "Betrachter" } else { "Viewer" }}</option>
                                </select>
                            }.into_view()
                        } else {
                            view! { <b>{role_label(&m.role, lang.get())}</b> }.into_view()
                        }}
                        {if can_admin && !is_current_user && can_change_owner_target {
                            view! {
                                <button class="danger-link" title=format!("Remove {member_name}") on:click=move |_| {
                                    remove_member(remove_id.clone(), member_name_for_remove.clone(), lang, set_data, set_error);
                                }>{move || if lang.get() == Lang::De { "Entfernen" } else { "Remove" }}</button>
                            }.into_view()
                        } else {
                            view! { <span/> }.into_view()
                        }}
                    </div>
                }}).collect_view()}
            </section>
            <section class="panel">
                <h3>{move || if lang.get() == Lang::De { "Einladen" } else { "Invite" }}</h3>
                {if can_admin {
                    view! {
                        <div class="invite-box">
                            <input type="email" placeholder="name@example.com" prop:value=invite_email on:input=move |ev| set_invite_email.set(event_target_value(&ev))/>
                            <select on:change=move |ev| set_invite_role.set(role_from_value(&select_value(&ev)))>
                                <option value="admin">"Admin"</option>
                                <option value="member" selected>{move || if lang.get() == Lang::De { "Mitglied" } else { "Member" }}</option>
                                <option value="viewer">{move || if lang.get() == Lang::De { "Betrachter" } else { "Viewer" }}</option>
                            </select>
                            <button class="btn primary" on:click=invite>{move || if lang.get() == Lang::De { "Einladen" } else { "Invite" }}</button>
                        </div>
                        {move || local_error.get().map(|err| view! { <div class="error-line">{err}</div> })}
                        {move || invite_result.get().map(|text| view! { <div class="invite-result">{text}</div> })}
                    }.into_view()
                } else {
                    view! { <p class="muted">{move || if lang.get() == Lang::De { "Nur Admins koennen Mitglieder verwalten." } else { "Only admins can manage members." }}</p> }.into_view()
                }}
            </section>
            <section class="panel">
                <h3>{move || if lang.get() == Lang::De { "Registrierte Accounts" } else { "Registered accounts" }}</h3>
                {if can_admin {
                    view! {
                        {boot.registered_users.iter().map(|user| {
                            let email = user.email.clone();
                            let email_for_add = user.email.clone();
                            let workspace_id_for_add = workspace_id.clone();
                            let user_id = user.id.clone();
                            let user_name = user.name.clone();
                            let user_name_for_delete = user.name.clone();
                            let membership_id = user.membership_id.clone();
                            let current_account_role = user.role.clone();
                            let is_member = user.membership_id.is_some();
                            let (add_role, set_add_role) = create_signal(Role::Member);
                            let can_change_account_owner = can_owner || current_account_role != Some(Role::Owner);
                            let created = if lang.get() == Lang::De { user.created_label_de.clone() } else { user.created_label_en.clone() };
                            view! {
                                <div class="registered-row">
                                    <span class="avatar tiny">{user.initials.clone()}</span>
                                    <span><strong>{user.name.clone()}</strong><small>{email}</small></span>
                                    <span>
                                        {if let Some(member_id) = membership_id.clone() {
                                            if can_change_account_owner {
                                                view! {
                                                    <select class="role-select" on:change=move |ev| {
                                                        update_member_role(
                                                            member_id.clone(),
                                                            role_from_value(&select_value(&ev)),
                                                            set_data,
                                                            set_error,
                                                        );
                                                    }>
                                                        <option value="owner" selected=current_account_role == Some(Role::Owner) disabled=!can_owner>"Owner"</option>
                                                        <option value="admin" selected=current_account_role == Some(Role::Admin)>"Admin"</option>
                                                        <option value="member" selected=current_account_role == Some(Role::Member)>{move || if lang.get() == Lang::De { "Mitglied" } else { "Member" }}</option>
                                                        <option value="viewer" selected=current_account_role == Some(Role::Viewer)>{move || if lang.get() == Lang::De { "Betrachter" } else { "Viewer" }}</option>
                                                    </select>
                                                }.into_view()
                                            } else {
                                                view! { <b>{role_label(current_account_role.as_ref().unwrap_or(&Role::Member), lang.get())}</b> }.into_view()
                                            }
                                        } else {
                                            view! {
                                                <select class="role-select" on:change=move |ev| set_add_role.set(role_from_value(&select_value(&ev)))>
                                                    <option value="admin">"Admin"</option>
                                                    <option value="member" selected>{move || if lang.get() == Lang::De { "Mitglied" } else { "Member" }}</option>
                                                    <option value="viewer">{move || if lang.get() == Lang::De { "Betrachter" } else { "Viewer" }}</option>
                                                </select>
                                            }.into_view()
                                        }}
                                    </span>
                                    <small>{created}</small>
                                    <span class="admin-actions">
                                        {if !is_member {
                                            view! {
                                                <button class="link-button" on:click=move |_| {
                                                    add_existing_user_to_workspace(
                                                        workspace_id_for_add.clone(),
                                                        email_for_add.clone(),
                                                        add_role.get_untracked(),
                                                        lang,
                                                        set_data,
                                                        set_error,
                                                    );
                                                }>{move || if lang.get() == Lang::De { "Hinzufuegen" } else { "Add" }}</button>
                                            }.into_view()
                                        } else {
                                            view! { <b>{move || if lang.get() == Lang::De { "Im Workspace" } else { "In workspace" }}</b> }.into_view()
                                        }}
                                        {if can_owner && user_id != current_user_id {
                                            view! {
                                                <button class="danger-link" title=format!("Delete {user_name}") on:click=move |_| {
                                                    delete_registered_user(user_id.clone(), user_name_for_delete.clone(), lang, set_data, set_error);
                                                }>{move || if lang.get() == Lang::De { "Loeschen" } else { "Delete" }}</button>
                                            }.into_view()
                                        } else {
                                            view! { <span/> }.into_view()
                                        }}
                                    </span>
                                </div>
                            }
                        }).collect_view()}
                    }.into_view()
                } else {
                    view! { <p class="muted">{move || if lang.get() == Lang::De { "Nur Admins sehen registrierte Accounts." } else { "Only admins can view registered accounts." }}</p> }.into_view()
                }}
            </section>
            <section class="panel">
                <h3>{move || if lang.get() == Lang::De { "System & Hosting" } else { "System & hosting" }}</h3>
                <div class="sys-grid">
                    <span><small>"Version"</small><strong>"0.1.0"</strong></span>
                    <span><small>"Runtime"</small><strong>"Rust · Axum"</strong></span>
                    <span><small>"Frontend"</small><strong>"WASM · Leptos"</strong></span>
                    <span><small>"Datenbank"</small><strong>"PostgreSQL"</strong></span>
                </div>
            </section>
            <section class="panel">
                <h3>{move || if lang.get() == Lang::De { "Audit-Log" } else { "Audit log" }}</h3>
                {boot.audit_events.iter().map(|a| view! {
                    <div class="activity-row"><span>{a.actor_name.clone().unwrap_or_else(|| "System".into())}</span><strong>{a.action.clone()}</strong><small>{if lang.get() == Lang::De { a.created_label_de.clone() } else { a.created_label_en.clone() }}</small></div>
                }).collect_view()}
            </section>
        </div>
    }.into_view()
}

fn create_task_modal(
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
        if title.get_untracked().trim().is_empty() {
            set_local_error.set(Some(if lang.get_untracked() == Lang::De {
                "Bitte gib zuerst einen Aufgabentitel ein.".into()
            } else {
                "Add a task title first.".into()
            }));
            return;
        }
        set_local_error.set(None);
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
                Err(err) => {
                    set_local_error.set(Some(err.message.clone()));
                    set_error.set(Some(err.message));
                }
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
                        {boot.statuses.clone().into_iter().map(|s| { let label = status_name(&s, lang.get()).to_string(); view! { <option value=s.id>{label}</option> } }).collect_view()}
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

fn create_ticket_modal(
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
                Err(err) => {
                    set_local_error.set(Some(err.message.clone()));
                    set_error.set(Some(err.message));
                }
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
                        {boot.members.clone().into_iter().map(|m| view! { <option value=m.user_id>{m.name}</option> }).collect_view()}
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

fn ticket_detail(
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
                    <h2>{ticket.key.clone()}</h2>
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
                        {boot.members.clone().into_iter().map(|m| {
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

fn task_detail(
    task: TaskDto,
    boot: BootstrapDto,
    lang: ReadSignal<Lang>,
    set_open_task: WriteSignal<Option<String>>,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) -> View {
    let (comment, set_comment) = create_signal(String::new());
    let (editing, set_editing) = create_signal(false);
    let (title_edit, set_title_edit) = create_signal(task.title.clone());
    let (description_edit, set_description_edit) = create_signal(task.description.clone());
    let (status_edit, set_status_edit) = create_signal(task.status_id.clone());
    let (priority_edit, set_priority_edit) = create_signal(task.priority.clone());
    let (due_date_edit, set_due_date_edit) =
        create_signal(task.due_date.clone().unwrap_or_default());
    let (phase_edit, set_phase_edit) = create_signal(task.phase.clone());
    let (assignee_edit, set_assignee_edit) =
        create_signal(task.assignee_ids.first().cloned().unwrap_or_default());
    let (recurrence_edit, set_recurrence_edit) = create_signal(task.recurrence);
    let (busy, set_busy) = create_signal(false);
    let (local_error, set_local_error) = create_signal::<Option<String>>(None);
    let (uploading, set_uploading) = create_signal(false);
    let file_input = create_node_ref::<html::Input>();
    let (mention_open, set_mention_open) = create_signal(false);
    let (mention_index, set_mention_index) = create_signal(0usize);
    let mention_members = store_value(boot.members.clone());
    // Editing or a half-typed comment must not be wiped by a background refetch.
    hold_realtime_while(move || editing.get() || !comment.get().trim().is_empty());

    let status_label = boot
        .statuses
        .iter()
        .find(|s| s.id == task.status_id)
        .map(|s| status_name(s, lang.get()).to_string())
        .unwrap_or_default();
    let title = task_title(&task, lang.get());
    let description = description_for(&task, lang.get());
    let assignees = task.assignee_ids.clone();
    let due = task
        .due_date
        .as_deref()
        .map(|d| fmt_date(d, lang.get()))
        .unwrap_or_else(|| "-".into());
    let priority = priority_label(&task.priority, lang.get()).to_string();
    let task_recurrence = task.recurrence;
    let project_line = format!("{} / {}", task.tag, boot.project.name);
    let pct = subtask_pct(&task);
    let subtasks = task.subtasks.clone();
    let attachments = task.attachments.clone();
    let comments = task.comments.clone();
    let can_edit = boot.current_role.can_edit();
    let task_id_base = task.id.clone();
    let task_id_for_upload = task.id.clone();
    let statuses_for_status_options = boot.statuses.clone();
    let members_for_display = boot.members.clone();
    let members_for_assign = boot.members.clone();
    let member_names: Vec<String> = boot.members.iter().map(|m| m.name.clone()).collect();

    let task_id_for_save = task.id.clone();
    let save = move |_| {
        if title_edit.get_untracked().trim().is_empty() {
            set_local_error.set(Some(if lang.get_untracked() == Lang::De {
                "Bitte gib zuerst einen Aufgabentitel ein.".into()
            } else {
                "Add a task title first.".into()
            }));
            return;
        }
        set_local_error.set(None);
        set_busy.set(true);
        let assignee = assignee_edit.get_untracked();
        let assignee_ids = if assignee.trim().is_empty() {
            Vec::new()
        } else {
            vec![assignee]
        };
        let due_date = due_date_edit.get_untracked();
        let payload = UpdateTaskRequest {
            title: Some(title_edit.get_untracked()),
            description: Some(description_edit.get_untracked()),
            tag: None,
            tag_color: None,
            priority: Some(priority_edit.get_untracked()),
            status_id: Some(status_edit.get_untracked()),
            start_date: None,
            due_date: Some((!due_date.trim().is_empty()).then_some(due_date)),
            phase: Some(phase_edit.get_untracked()),
            recurrence: Some(recurrence_edit.get_untracked()),
            assignee_ids: Some(assignee_ids),
        };
        let task_id = task_id_for_save.clone();
        spawn_local(async move {
            match api_patch::<_, TaskDto>(&format!("/api/tasks/{task_id}"), &payload).await {
                Ok(task) => {
                    replace_task(set_data, task);
                    set_editing.set(false);
                    set_error.set(None);
                }
                Err(err) => {
                    set_local_error.set(Some(err.message.clone()));
                    set_error.set(Some(err.message));
                }
            }
            set_busy.set(false);
        });
    };

    let task_id_for_delete = task.id.clone();
    let title_for_delete = title.clone();
    let delete = move |_| {
        let confirm_text = if lang.get_untracked() == Lang::De {
            format!("{title_for_delete} wirklich loeschen?")
        } else {
            format!("Delete {title_for_delete}?")
        };
        let confirmed = web_sys::window()
            .and_then(|w| w.confirm_with_message(&confirm_text).ok())
            .unwrap_or(false);
        if !confirmed {
            return;
        }
        let task_id = task_id_for_delete.clone();
        spawn_local(async move {
            match api_delete_empty(&format!("/api/tasks/{task_id}")).await {
                Ok(()) => {
                    remove_task(set_data, &task_id);
                    set_open_task.set(None);
                    set_error.set(None);
                }
                Err(err) => set_error.set(Some(err.message)),
            }
        });
    };

    let reset_title = task.title.clone();
    let reset_description = task.description.clone();
    let reset_status = task.status_id.clone();
    let reset_priority = task.priority.clone();
    let reset_due = task.due_date.clone().unwrap_or_default();
    let reset_phase = task.phase.clone();
    let reset_assignee = task.assignee_ids.first().cloned().unwrap_or_default();
    let reset_recurrence = task.recurrence;

    let mention_candidates = move || -> Vec<MemberDto> {
        let value = comment.get();
        let Some((_, query)) = mention_query(&value) else {
            return Vec::new();
        };
        let query = query.to_lowercase();
        mention_members.with_value(|members| {
            members
                .iter()
                .filter(|m| m.name.to_lowercase().contains(&query))
                .cloned()
                .collect()
        })
    };
    let pick_mention = move |name: String| {
        let value = comment.get_untracked();
        if let Some((at, _)) = mention_query(&value) {
            set_comment.set(format!("{}@{name} ", &value[..at]));
        }
        set_mention_open.set(false);
        set_mention_index.set(0);
    };
    let task_id_for_comment_submit = task.id.clone();
    let submit_comment = move || {
        let body = comment.get_untracked();
        if !body.trim().is_empty() {
            add_comment(task_id_for_comment_submit.clone(), body, set_data, set_error);
            set_comment.set(String::new());
            set_mention_open.set(false);
        }
    };
    let submit_comment_for_button = submit_comment.clone();

    view! {
        <div class="drawer-backdrop" on:click=move |_| set_open_task.set(None)></div>
        <aside class="task-drawer">
            <header>
                <span>{task.key.clone()}</span>
                {move || if editing.get() {
                    let current = status_edit.get_untracked();
                    view! {
                        <select class="compact-select" on:change=move |ev| set_status_edit.set(select_value(&ev))>
                            {statuses_for_status_options.clone().into_iter().map(|s| {
                                let selected = current == s.id;
                                let label = status_name(&s, lang.get()).to_string();
                                view! { <option value=s.id selected=selected>{label}</option> }
                            }).collect_view()}
                        </select>
                    }.into_view()
                } else {
                    view! { <b>{status_label.clone()}</b> }.into_view()
                }}
                <span class="drawer-actions">
                    {move || if editing.get() {
                        view! { <span/> }.into_view()
                    } else {
                        view! {
                            <button class="link-button" on:click=move |_| set_editing.set(true)>
                                {move || if lang.get() == Lang::De { "Bearbeiten" } else { "Edit" }}
                            </button>
                        }.into_view()
                    }}
                    <button class="danger-link" on:click=delete>{move || if lang.get() == Lang::De { "Loeschen" } else { "Delete" }}</button>
                    <button class="drawer-close" on:click=move |_| set_open_task.set(None)>"x"</button>
                </span>
            </header>
            {move || if editing.get() {
                view! {
                    <label class="drawer-field title-field">
                        <span>{move || if lang.get() == Lang::De { "Titel" } else { "Title" }}</span>
                        <input class="title-input" prop:value=title_edit on:input=move |ev| {
                            set_title_edit.set(event_target_value(&ev));
                            set_local_error.set(None);
                        }/>
                    </label>
                }.into_view()
            } else {
                view! { <h2>{title.clone()}</h2> }.into_view()
            }}
            {move || if editing.get() {
                let current_assignee = assignee_edit.get_untracked();
                let current_priority = priority_edit.get_untracked();
                let current_phase = phase_edit.get_untracked();
                let current_recurrence = recurrence_edit.get_untracked();
                view! {
                    <div class="detail-meta">
                        <span>
                            <small>{move || if lang.get() == Lang::De { "Zuweisen" } else { "Assign" }}</small>
                            <select on:change=move |ev| set_assignee_edit.set(select_value(&ev))>
                                <option value="" selected=current_assignee.is_empty()>{move || if lang.get() == Lang::De { "Nicht zugewiesen" } else { "Unassigned" }}</option>
                                {members_for_assign.clone().into_iter().map(|m| {
                                    let selected = current_assignee == m.user_id;
                                    view! { <option value=m.user_id selected=selected>{m.name}</option> }
                                }).collect_view()}
                            </select>
                        </span>
                        <span>
                            <small>{move || if lang.get() == Lang::De { "Faelligkeit" } else { "Due date" }}</small>
                            <input type="date" prop:value=due_date_edit on:input=move |ev| set_due_date_edit.set(event_target_value(&ev))/>
                        </span>
                        <span>
                            <small>{move || if lang.get() == Lang::De { "Prioritaet" } else { "Priority" }}</small>
                            <select on:change=move |ev| set_priority_edit.set(priority_from_value(&select_value(&ev)))>
                                <option value="urgent" selected=current_priority == Priority::Urgent>{move || if lang.get() == Lang::De { "Dringend" } else { "Urgent" }}</option>
                                <option value="high" selected=current_priority == Priority::High>{move || if lang.get() == Lang::De { "Hoch" } else { "High" }}</option>
                                <option value="medium" selected=current_priority == Priority::Medium>{move || if lang.get() == Lang::De { "Mittel" } else { "Medium" }}</option>
                                <option value="low" selected=current_priority == Priority::Low>{move || if lang.get() == Lang::De { "Niedrig" } else { "Low" }}</option>
                            </select>
                        </span>
                        <span>
                            <small>"Phase"</small>
                            <select on:change=move |ev| set_phase_edit.set(select_value(&ev))>
                                <option value="planung" selected=current_phase == "planung">{move || if lang.get() == Lang::De { "Planung" } else { "Planning" }}</option>
                                <option value="vergabe" selected=current_phase == "vergabe">{move || if lang.get() == Lang::De { "Vergabe" } else { "Tendering" }}</option>
                                <option value="ausfuehrung" selected=current_phase == "ausfuehrung">{move || if lang.get() == Lang::De { "Ausfuehrung" } else { "Execution" }}</option>
                                <option value="abnahme" selected=current_phase == "abnahme">{move || if lang.get() == Lang::De { "Abnahme" } else { "Handover" }}</option>
                            </select>
                        </span>
                        <span>
                            <small>{move || if lang.get() == Lang::De { "Wiederholung" } else { "Repeat" }}</small>
                            <select on:change=move |ev| set_recurrence_edit.set(recurrence_from_value(&select_value(&ev)))>
                                {recurrence_options(current_recurrence, lang)}
                            </select>
                        </span>
                    </div>
                }.into_view()
            } else {
                view! {
                    <div class="detail-meta">
                        <span><small>{move || if lang.get() == Lang::De { "Zuweisen" } else { "Assign" }}</small>{assignee_avatars(&assignees, &members_for_display)}</span>
                        <span><small>{move || if lang.get() == Lang::De { "Faelligkeit" } else { "Due date" }}</small><b>{due.clone()}</b></span>
                        <span><small>{move || if lang.get() == Lang::De { "Prioritaet" } else { "Priority" }}</small><b>{priority.clone()}</b></span>
                        <span><small>{move || if lang.get() == Lang::De { "Wiederholung" } else { "Repeat" }}</small><b>{move || recurrence_label(task_recurrence.as_ref(), lang.get())}</b></span>
                        <span><small>{move || if lang.get() == Lang::De { "Projekt" } else { "Project" }}</small><b>{project_line.clone()}</b></span>
                    </div>
                }.into_view()
            }}
            <section>
                <h3>{move || if lang.get() == Lang::De { "Beschreibung" } else { "Description" }}</h3>
                {move || if editing.get() {
                    view! {
                        <textarea class="drawer-textarea" prop:value=description_edit on:input=move |ev| set_description_edit.set(textarea_value(&ev))></textarea>
                    }.into_view()
                } else {
                    view! { <p>{description.clone()}</p> }.into_view()
                }}
            </section>
            <section>
                <h3>{move || if lang.get() == Lang::De { "Unteraufgaben" } else { "Subtasks" }}</h3>
                <div class="progress-line"><i style=format!("width:{pct}%")></i></div>
                {subtasks.into_iter().map(|sub| {
                    let task_id = task_id_base.clone();
                    let sub_id = sub.id.clone();
                    let done = sub.done;
                    let label = title_for(sub.title, sub.title_en, lang.get());
                    view! {
                        <label class="subtask">
                            <input type="checkbox" checked=done on:change=move |_| toggle_subtask(task_id.clone(), sub_id.clone(), !done, set_data, set_error)/>
                            <span>{label}</span>
                        </label>
                    }
                }).collect_view()}
            </section>
            <section>
                <h3>{move || if lang.get() == Lang::De { "Anhaenge" } else { "Attachments" }}</h3>
                <div class="attachments">
                    {attachments.into_iter().map(|a| attachment_view(a, lang)).collect_view()}
                </div>
                {can_edit.then(|| view! {
                    <div class="upload-row">
                        <input type="file" multiple style="display:none" node_ref=file_input on:change=move |ev| {
                            let input = event_target::<web_sys::HtmlInputElement>(&ev);
                            if let Some(files) = input.files() {
                                if files.length() > 0 {
                                    upload_attachments(task_id_for_upload.clone(), files, set_uploading, set_data, set_error);
                                }
                            }
                            input.set_value("");
                        }/>
                        <button class="btn ghost" disabled=uploading on:click=move |_| {
                            if let Some(input) = file_input.get_untracked() {
                                input.click();
                            }
                        }>
                            {move || match (uploading.get(), lang.get() == Lang::De) {
                                (true, true) => "Lädt hoch...",
                                (true, false) => "Uploading...",
                                (false, true) => "+ Datei hochladen",
                                (false, false) => "+ Upload file",
                            }}
                        </button>
                    </div>
                })}
            </section>
            <section>
                <h3>{move || if lang.get() == Lang::De { "Kommentare" } else { "Comments" }}</h3>
                {comments.into_iter().map(|c| {
                    let created = if lang.get() == Lang::De { c.created_label_de } else { c.created_label_en };
                    let body = comment_body_view(&c.body, &member_names);
                    view! { <div class="comment"><span class="avatar tiny">{c.author_initials}</span><p><strong>{c.author_name}</strong><br/>{body}</p><small>{created}</small></div> }
                }).collect_view()}
                <div class="comment-box">
                    {move || {
                        let candidates = mention_candidates();
                        (mention_open.get() && !candidates.is_empty()).then(|| view! {
                            <div class="mention-pop">
                                {candidates.into_iter().enumerate().map(|(i, m)| {
                                    let name = m.name.clone();
                                    view! {
                                        <button type="button" class="mention-item" class:active=move || mention_index.get() == i
                                            on:mousedown=move |ev| {
                                                // Pick before the input loses focus.
                                                ev.prevent_default();
                                                pick_mention(name.clone());
                                            }>
                                            <span class="avatar tiny">{m.initials}</span>
                                            <span class="mention-name">{m.name}</span>
                                            <small>{m.email}</small>
                                        </button>
                                    }
                                }).collect_view()}
                            </div>
                        })
                    }}
                    <input
                        placeholder=move || if lang.get() == Lang::De { "Kommentar schreiben... (@ erwähnt)" } else { "Write a comment... (@ mentions)" }
                        prop:value=comment
                        on:input=move |ev| {
                            let value = event_target_value(&ev);
                            set_mention_open.set(mention_query(&value).is_some());
                            set_mention_index.set(0);
                            set_comment.set(value);
                        }
                        on:keydown=move |ev| {
                            // The popup only counts as active while it has
                            // candidates; a query without matches must not
                            // swallow Enter (the user wants to submit).
                            let candidates = if mention_open.get_untracked() {
                                mention_candidates()
                            } else {
                                Vec::new()
                            };
                            if !candidates.is_empty() {
                                match ev.key().as_str() {
                                    "ArrowDown" => {
                                        ev.prevent_default();
                                        set_mention_index.update(|i| *i = (*i + 1) % candidates.len());
                                    }
                                    "ArrowUp" => {
                                        ev.prevent_default();
                                        set_mention_index.update(|i| *i = (*i + candidates.len() - 1) % candidates.len());
                                    }
                                    "Enter" | "Tab" => {
                                        ev.prevent_default();
                                        let index = mention_index.get_untracked().min(candidates.len() - 1);
                                        pick_mention(candidates[index].name.clone());
                                    }
                                    "Escape" => set_mention_open.set(false),
                                    _ => {}
                                }
                            } else if ev.key() == "Enter" {
                                submit_comment();
                            }
                        }
                    />
                    <button on:click=move |_| submit_comment_for_button()>"Enter"</button>
                </div>
            </section>
            <section class="drawer-edit-actions" style=move || if editing.get() { "".to_string() } else { "display:none".to_string() }>
                {move || local_error.get().map(|err| view! { <div class="modal-error inline">{err}</div> })}
                <button class="btn ghost" on:click=move |_| {
                    set_title_edit.set(reset_title.clone());
                    set_description_edit.set(reset_description.clone());
                    set_status_edit.set(reset_status.clone());
                    set_priority_edit.set(reset_priority.clone());
                    set_due_date_edit.set(reset_due.clone());
                    set_phase_edit.set(reset_phase.clone());
                    set_assignee_edit.set(reset_assignee.clone());
                    set_recurrence_edit.set(reset_recurrence);
                    set_local_error.set(None);
                    set_editing.set(false);
                }>{move || if lang.get() == Lang::De { "Abbrechen" } else { "Cancel" }}</button>
                <button class="btn primary" disabled=move || busy.get() on:click=save>{move || if lang.get() == Lang::De { "Speichern" } else { "Save" }}</button>
            </section>
        </aside>
    }.into_view()
}

#[allow(dead_code)]
fn task_detail_readonly(
    task: TaskDto,
    boot: BootstrapDto,
    lang: ReadSignal<Lang>,
    set_open_task: WriteSignal<Option<String>>,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) -> View {
    let (comment, set_comment) = create_signal(String::new());
    let status_label = boot
        .statuses
        .iter()
        .find(|s| s.id == task.status_id)
        .map(|s| status_name(s, lang.get()).to_string())
        .unwrap_or_default();
    let title = task_title(&task, lang.get());
    let description = description_for(&task, lang.get());
    let assignees = task.assignee_ids.clone();
    let due = task
        .due_date
        .as_deref()
        .map(|d| fmt_date(d, lang.get()))
        .unwrap_or_else(|| "-".into());
    let priority = priority_label(&task.priority, lang.get()).to_string();
    let project_line = format!("{} / {}", task.tag, boot.project.name);
    let pct = subtask_pct(&task);
    let subtasks = task.subtasks.clone();
    let attachments = task.attachments.clone();
    let comments = task.comments.clone();
    let task_id_for_comment = task.id.clone();

    view! {
        <div class="drawer-backdrop" on:click=move |_| set_open_task.set(None)></div>
        <aside class="task-drawer">
            <header>
                <span>{task.key.clone()}</span>
                <b>{status_label}</b>
                <button on:click=move |_| set_open_task.set(None)>"x"</button>
            </header>
            <h2>{title}</h2>
            <div class="detail-meta">
                <span><small>{move || if lang.get() == Lang::De { "Zuweisen" } else { "Assign" }}</small>{assignee_avatars(&assignees, &boot.members)}</span>
                <span><small>{move || if lang.get() == Lang::De { "Fälligkeit" } else { "Due date" }}</small><b>{due}</b></span>
                <span><small>{move || if lang.get() == Lang::De { "Priorität" } else { "Priority" }}</small><b>{priority}</b></span>
                <span><small>{move || if lang.get() == Lang::De { "Projekt" } else { "Project" }}</small><b>{project_line}</b></span>
            </div>
            <section>
                <h3>{move || if lang.get() == Lang::De { "Beschreibung" } else { "Description" }}</h3>
                <p>{description}</p>
            </section>
            <section>
                <h3>{move || if lang.get() == Lang::De { "Unteraufgaben" } else { "Subtasks" }}</h3>
                <div class="progress-line"><i style=format!("width:{pct}%")></i></div>
                {subtasks.into_iter().map(|sub| {
                    let task_id = task.id.clone();
                    let sub_id = sub.id.clone();
                    let done = sub.done;
                    let label = title_for(sub.title, sub.title_en, lang.get());
                    view! {
                        <label class="subtask">
                            <input type="checkbox" checked=done on:change=move |_| toggle_subtask(task_id.clone(), sub_id.clone(), !done, set_data, set_error)/>
                            <span>{label}</span>
                        </label>
                    }
                }).collect_view()}
            </section>
            <section>
                <h3>{move || if lang.get() == Lang::De { "Anhänge" } else { "Attachments" }}</h3>
                <div class="chips">
                    {attachments.into_iter().map(|a| view! { <a class="file-chip" href=format!("/api/attachments/{}", a.id) download>"Datei "{a.file_name}<small>{a.size_label}</small></a> }).collect_view()}
                </div>
            </section>
            <section>
                <h3>{move || if lang.get() == Lang::De { "Kommentare" } else { "Comments" }}</h3>
                {comments.into_iter().map(|c| {
                    let created = if lang.get() == Lang::De { c.created_label_de } else { c.created_label_en };
                    view! { <div class="comment"><span class="avatar tiny">{c.author_initials}</span><p><strong>{c.author_name}</strong><br/>{c.body}</p><small>{created}</small></div> }
                }).collect_view()}
                <div class="comment-box">
                    <input placeholder=move || if lang.get() == Lang::De { "Kommentar schreiben..." } else { "Write a comment..." } prop:value=comment on:input=move |ev| set_comment.set(event_target_value(&ev))/>
                    <button on:click=move |_| {
                        let body = comment.get_untracked();
                        if !body.trim().is_empty() {
                            add_comment(task_id_for_comment.clone(), body, set_data, set_error);
                            set_comment.set(String::new());
                        }
                    }>"↵"</button>
                </div>
            </section>
        </aside>
    }.into_view()
}
fn notifications_panel(
    notifications: Vec<NotificationDto>,
    tasks: Vec<TaskDto>,
    lang: ReadSignal<Lang>,
    set_show_notifications: WriteSignal<bool>,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) -> View {
    view! {
        <div class="notifications">
            <header>
                <h3>{move || if lang.get() == Lang::De { "Benachrichtigungen" } else { "Notifications" }}</h3>
                <button on:click=move |_| read_all_notifications(set_data, set_error)>{move || if lang.get() == Lang::De { "Alle als gelesen markieren" } else { "Mark all read" }}</button>
            </header>
            {notifications.into_iter().map(|n| {
                let id = n.id.clone();
                let unread = n.unread;
                let actor_initials = n.actor_initials.clone().unwrap_or_else(|| "•".into());
                let actor_name = n.actor_name.clone().unwrap_or_else(|| "System".into());
                let text = notif_text(&n, lang.get());
                let created = if lang.get() == Lang::De { n.created_label_de.clone() } else { n.created_label_en.clone() };
                let related_title = n.task_id.as_ref().and_then(|id| tasks.iter().find(|t| &t.id == id)).map(|t| task_title(t, lang.get())).unwrap_or_default();
                view! {
                    <button class="notif-row" class:unread=unread on:click=move |_| {
                        read_notification(id.clone(), set_data, set_error);
                        set_show_notifications.set(false);
                    }>
                        <span class="avatar tiny">{actor_initials}</span>
                        <span><strong>{actor_name}</strong>" "{text}<em>{related_title}</em><small>{created}</small></span>
                        {if unread { view! { <b></b> }.into_view() } else { view! { <i></i> }.into_view() }}
                    </button>
                }
            }).collect_view()}
        </div>
    }.into_view()
}
fn task_card(
    task: TaskDto,
    members: Vec<MemberDto>,
    lang: ReadSignal<Lang>,
    set_open_task: WriteSignal<Option<String>>,
    set_drag_task: WriteSignal<Option<String>>,
) -> View {
    let pct = subtask_pct(&task);
    let drag_id = task.id.clone();
    let open_id = task.id.clone();
    let tag_class = format!("tag {}", task.tag_color);
    let tag = task.tag.clone();
    let prio_class = format!("prio {}", priority_class(&task.priority));
    let title = task_title(&task, lang.get());
    let recurring = task.recurrence.is_some();
    let due = task
        .due_date
        .as_deref()
        .map(|d| fmt_date(d, lang.get()))
        .unwrap_or_else(|| "-".into());
    let assignees = task.assignee_ids.clone();
    view! {
        <article class="task-card" draggable="true"
            on:dragstart=move |_| set_drag_task.set(Some(drag_id.clone()))
            on:click=move |_| set_open_task.set(Some(open_id.clone()))>
            <div class="task-tags">
                <span class=tag_class>{tag}</span>
                {recurring.then(|| view! {
                    <span class="recur-mark" title=move || if lang.get() == Lang::De { "Wiederkehrende Aufgabe" } else { "Recurring task" }>"↻"</span>
                })}
                <b class=prio_class></b>
            </div>
            <h3>{title}</h3>
            <div class="mini-progress"><i style=format!("width:{pct}%")></i></div>
            <footer>
                <small>{due}</small>
                <span>{assignee_avatars(&assignees, &members)}</span>
            </footer>
        </article>
    }
    .into_view()
}
fn task_row(
    task: TaskDto,
    members: Vec<MemberDto>,
    lang: ReadSignal<Lang>,
    set_open_task: WriteSignal<Option<String>>,
) -> View {
    let task_id = task.id.clone();
    let title = task_title(&task, lang.get());
    let tag = task.tag.clone();
    let due = task
        .due_date
        .as_deref()
        .map(|d| fmt_date(d, lang.get()))
        .unwrap_or_else(|| "-".into());
    let assignees = task.assignee_ids.clone();
    let prio_class = format!("prio {}", priority_class(&task.priority));
    view! {
        <button class="today-row" on:click=move |_| set_open_task.set(Some(task_id.clone()))>
            <b class=prio_class></b>
            <span><strong>{title}</strong><small>{tag}" / "{due}</small></span>
            {assignee_avatars(&assignees, &members)}
        </button>
    }
    .into_view()
}
fn stat(icon: &'static str, value: usize, label: &'static str, tone: &'static str) -> View {
    view! {
        <article class=format!("stat-card {tone}")><span>{icon}</span><strong>{value}</strong><small>{label}</small></article>
    }.into_view()
}

fn logo() -> View {
    view! {
        <span class="logo">
            <i><b></b><b></b><b></b></i>
            <span>"KoWoBau-Planner"</span>
        </span>
    }
    .into_view()
}

fn optimistic_move(
    task_id: String,
    status_id: String,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) {
    // Remembered so the card can snap back if the server rejects the move.
    let mut previous: Option<(String, i32)> = None;
    set_data.update(|data| {
        if let Some(data) = data {
            if let Some(status) = data.statuses.iter().find(|s| s.id == status_id) {
                if let Some(task) = data.tasks.iter_mut().find(|t| t.id == task_id) {
                    previous = Some((task.status_id.clone(), task.status_position));
                    task.status_id = status_id.clone();
                    task.status_position = status.position;
                }
            }
        }
    });
    let revert_task_id = task_id.clone();
    spawn_local(async move {
        match api_post::<_, TaskDto>(
            &format!("/api/tasks/{task_id}/move"),
            &MoveTaskRequest { status_id },
        )
        .await
        {
            Ok(task) => {
                set_data.update(|data| {
                    if let Some(data) = data {
                        if let Some(current) = data.tasks.iter_mut().find(|t| t.id == task.id) {
                            *current = task;
                        }
                    }
                });
                set_error.set(None);
            }
            Err(err) => {
                if let Some((prev_status_id, prev_position)) = previous {
                    set_data.update(|data| {
                        if let Some(data) = data {
                            if let Some(task) =
                                data.tasks.iter_mut().find(|t| t.id == revert_task_id)
                            {
                                task.status_id = prev_status_id;
                                task.status_position = prev_position;
                            }
                        }
                    });
                }
                set_error.set(Some(err.message));
            }
        }
    });
}

fn toggle_subtask(
    task_id: String,
    subtask_id: String,
    done: bool,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) {
    set_data.update(|data| {
        if let Some(data) = data {
            if let Some(task) = data.tasks.iter_mut().find(|t| t.id == task_id) {
                if let Some(sub) = task.subtasks.iter_mut().find(|s| s.id == subtask_id) {
                    sub.done = done;
                }
            }
        }
    });
    spawn_local(async move {
        let body = UpdateSubtaskRequest {
            title: None,
            done: Some(done),
        };
        match api_patch::<_, TaskDto>(
            &format!("/api/tasks/{task_id}/subtasks/{subtask_id}"),
            &body,
        )
        .await
        {
            Ok(task) => replace_task(set_data, task),
            Err(err) => {
                set_error.set(Some(err.message));
            }
        }
    });
}

fn add_comment(
    task_id: String,
    body: String,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) {
    spawn_local(async move {
        match api_post::<_, TaskDto>(
            &format!("/api/tasks/{task_id}/comments"),
            &CreateCommentRequest { body },
        )
        .await
        {
            Ok(task) => replace_task(set_data, task),
            Err(err) => set_error.set(Some(err.message)),
        }
    });
}

fn read_notification(
    id: String,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) {
    set_data.update(|data| {
        if let Some(data) = data {
            if let Some(n) = data.notifications.iter_mut().find(|n| n.id == id) {
                n.unread = false;
            }
        }
    });
    spawn_local(async move {
        if let Err(err) = api_empty(&format!("/api/notifications/{id}/read")).await {
            set_error.set(Some(err.message));
        }
    });
}

fn read_all_notifications(
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) {
    set_data.update(|data| {
        if let Some(data) = data {
            for n in &mut data.notifications {
                n.unread = false;
            }
        }
    });
    spawn_local(async move {
        if let Err(err) = api_empty("/api/notifications/read-all").await {
            set_error.set(Some(err.message));
        }
    });
}

fn update_member_role(
    membership_id: String,
    role: Role,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) {
    spawn_local(async move {
        match api_patch::<_, MemberDto>(
            &format!("/api/memberships/{membership_id}"),
            &UpdateMembershipRequest { role },
        )
        .await
        {
            Ok(_) => refresh_bootstrap(set_data, set_error).await,
            Err(err) => set_error.set(Some(err.message)),
        }
    });
}

fn add_existing_user_to_workspace(
    workspace_id: String,
    email: String,
    role: Role,
    lang: ReadSignal<Lang>,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) {
    spawn_local(async move {
        match api_post::<_, InviteMemberResponse>(
            &format!("/api/workspaces/{workspace_id}/invites"),
            &InviteMemberRequest { email, role },
        )
        .await
        {
            Ok(_) => refresh_bootstrap(set_data, set_error).await,
            Err(err) => {
                let prefix = if lang.get_untracked() == Lang::De {
                    "Konnte User nicht hinzufuegen"
                } else {
                    "Could not add user"
                };
                set_error.set(Some(format!("{prefix}: {}", err.message)));
            }
        }
    });
}

fn remove_member(
    membership_id: String,
    member_name: String,
    lang: ReadSignal<Lang>,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) {
    let confirm_text = if lang.get_untracked() == Lang::De {
        format!("{member_name} wirklich aus dem Workspace entfernen?")
    } else {
        format!("Remove {member_name} from the workspace?")
    };
    let confirmed = web_sys::window()
        .and_then(|w| w.confirm_with_message(&confirm_text).ok())
        .unwrap_or(false);
    if !confirmed {
        return;
    }
    spawn_local(async move {
        match api_delete_empty(&format!("/api/memberships/{membership_id}")).await {
            Ok(()) => refresh_bootstrap(set_data, set_error).await,
            Err(err) => set_error.set(Some(err.message)),
        }
    });
}

fn delete_registered_user(
    user_id: String,
    user_name: String,
    lang: ReadSignal<Lang>,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) {
    let confirm_text = if lang.get_untracked() == Lang::De {
        format!("{user_name} wirklich komplett loeschen? Das entfernt den Account und eigene Solo-Workspaces.")
    } else {
        format!("Delete {user_name} completely? This removes the account and own solo workspaces.")
    };
    let confirmed = web_sys::window()
        .and_then(|w| w.confirm_with_message(&confirm_text).ok())
        .unwrap_or(false);
    if !confirmed {
        return;
    }
    spawn_local(async move {
        match api_delete_empty(&format!("/api/users/{user_id}")).await {
            Ok(()) => refresh_bootstrap(set_data, set_error).await,
            Err(err) => set_error.set(Some(err.message)),
        }
    });
}

async fn refresh_bootstrap(
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) {
    match api_get::<BootstrapDto>("/api/bootstrap").await {
        Ok(next) => {
            set_data.set(Some(next));
            set_error.set(None);
        }
        Err(err) => set_error.set(Some(err.message)),
    }
}

fn replace_task(set_data: WriteSignal<Option<BootstrapDto>>, task: TaskDto) {
    set_data.update(|data| {
        if let Some(data) = data {
            if let Some(current) = data.tasks.iter_mut().find(|t| t.id == task.id) {
                *current = task;
            }
        }
    });
}

fn remove_task(set_data: WriteSignal<Option<BootstrapDto>>, task_id: &str) {
    set_data.update(|data| {
        if let Some(data) = data {
            data.tasks.retain(|task| task.id != task_id);
        }
    });
}

fn replace_ticket(set_data: WriteSignal<Option<BootstrapDto>>, ticket: TicketDto) {
    set_data.update(|data| {
        if let Some(data) = data {
            if let Some(current) = data.tickets.iter_mut().find(|t| t.id == ticket.id) {
                *current = ticket;
            }
        }
    });
}

fn remove_ticket(set_data: WriteSignal<Option<BootstrapDto>>, ticket_id: &str) {
    set_data.update(|data| {
        if let Some(data) = data {
            data.tickets.retain(|ticket| ticket.id != ticket_id);
        }
    });
}

/// API failure carrying the HTTP status so callers can tell "not logged in"
/// (401) apart from real errors. Network failures use status 0.
#[derive(Debug, Clone)]
struct ApiError {
    status: u16,
    message: String,
}

impl ApiError {
    fn network(message: impl ToString) -> Self {
        Self {
            status: 0,
            message: message.to_string(),
        }
    }
}

async fn api_get<T: DeserializeOwned>(url: &str) -> Result<T, ApiError> {
    let response = Request::get(url)
        .credentials(RequestCredentials::SameOrigin)
        .send()
        .await
        .map_err(ApiError::network)?;
    decode_response(response).await
}

async fn api_post<B: Serialize, T: DeserializeOwned>(url: &str, body: &B) -> Result<T, ApiError> {
    let response = Request::post(url)
        .credentials(RequestCredentials::SameOrigin)
        .header("x-client-id", &client_id())
        .json(body)
        .map_err(ApiError::network)?
        .send()
        .await
        .map_err(ApiError::network)?;
    decode_response(response).await
}

async fn api_post_form<T: DeserializeOwned>(
    url: &str,
    form: web_sys::FormData,
) -> Result<T, ApiError> {
    // No explicit content type: the browser sets multipart/form-data with the
    // correct boundary itself.
    let response = Request::post(url)
        .credentials(RequestCredentials::SameOrigin)
        .header("x-client-id", &client_id())
        .body(form)
        .map_err(ApiError::network)?
        .send()
        .await
        .map_err(ApiError::network)?;
    decode_response(response).await
}

async fn api_patch<B: Serialize, T: DeserializeOwned>(url: &str, body: &B) -> Result<T, ApiError> {
    let response = Request::patch(url)
        .credentials(RequestCredentials::SameOrigin)
        .header("x-client-id", &client_id())
        .json(body)
        .map_err(ApiError::network)?
        .send()
        .await
        .map_err(ApiError::network)?;
    decode_response(response).await
}

async fn api_empty(url: &str) -> Result<(), ApiError> {
    let response = Request::post(url)
        .credentials(RequestCredentials::SameOrigin)
        .header("x-client-id", &client_id())
        .send()
        .await
        .map_err(ApiError::network)?;
    if response.ok() {
        Ok(())
    } else {
        Err(error_from_body(&response, response.text().await.ok()))
    }
}

async fn api_delete_empty(url: &str) -> Result<(), ApiError> {
    let response = Request::delete(url)
        .credentials(RequestCredentials::SameOrigin)
        .header("x-client-id", &client_id())
        .send()
        .await
        .map_err(ApiError::network)?;
    if response.ok() {
        Ok(())
    } else {
        Err(error_from_body(&response, response.text().await.ok()))
    }
}

async fn decode_response<T: DeserializeOwned>(
    response: gloo_net::http::Response,
) -> Result<T, ApiError> {
    if response.ok() {
        response.json::<T>().await.map_err(ApiError::network)
    } else {
        Err(error_from_body(&response, response.text().await.ok()))
    }
}

fn error_from_body(response: &gloo_net::http::Response, text: Option<String>) -> ApiError {
    let text = text.unwrap_or_else(|| "request failed".into());
    ApiError {
        status: response.status(),
        message: serde_json::from_str::<ApiErrorDto>(&text)
            .map(|e| e.error)
            .unwrap_or(text),
    }
}

thread_local! {
    // Random per-tab id. Sent with every mutation (X-Client-Id) and the WS
    // handshake so this tab can skip refetching for its own changes.
    static CLIENT_ID: String = format!(
        "{:08x}{:08x}",
        (js_sys::Math::random() * f64::from(u32::MAX)) as u32,
        (js_sys::Math::random() * f64::from(u32::MAX)) as u32,
    );
    static REFETCH_PENDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn client_id() -> String {
    CLIENT_ID.with(Clone::clone)
}

/// Context wrapper around the editor-hold counter (see AppRoot).
#[derive(Clone, Copy)]
struct RealtimeHold(RwSignal<i32>);

/// Keeps realtime refetches paused while `active()` is true. Call once per
/// component; releases automatically when the component is removed.
fn hold_realtime_while(active: impl Fn() -> bool + 'static) {
    let Some(RealtimeHold(hold)) = use_context::<RealtimeHold>() else {
        return;
    };
    let held = store_value(false);
    create_effect(move |_| {
        let active = active();
        if active && !held.get_value() {
            held.set_value(true);
            hold.update(|v| *v += 1);
        } else if !active && held.get_value() {
            held.set_value(false);
            hold.update(|v| *v -= 1);
        }
    });
    on_cleanup(move || {
        if held.get_value() {
            held.set_value(false);
            hold.update(|v| *v -= 1);
        }
    });
}

/// Coalesces bursts of realtime events into a single background bootstrap
/// refetch, deferred while any editor is open.
fn schedule_refetch(
    data: ReadSignal<Option<BootstrapDto>>,
    hold: RwSignal<i32>,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) {
    if REFETCH_PENDING.with(|p| p.replace(true)) {
        return;
    }
    spawn_local(async move {
        gloo_timers::future::TimeoutFuture::new(400).await;
        while hold.get_untracked() > 0 {
            gloo_timers::future::TimeoutFuture::new(1_000).await;
        }
        REFETCH_PENDING.with(|p| p.set(false));
        if data.get_untracked().is_some() {
            refresh_bootstrap(set_data, set_error).await;
        }
    });
}

/// Connects to /api/ws and triggers a debounced refetch whenever another
/// client changes something in the workspace. Reconnects with exponential
/// backoff and stops once the user is logged out.
fn start_realtime(
    data: ReadSignal<Option<BootstrapDto>>,
    hold: RwSignal<i32>,
    running: StoredValue<bool>,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) {
    spawn_local(async move {
        use futures_util::StreamExt;

        let mut backoff_ms = 1_000u32;
        loop {
            if data.try_get_untracked().flatten().is_none() {
                break;
            }
            let url = match websocket_url() {
                Some(url) => url,
                None => break,
            };
            let connected_at = js_sys::Date::now();
            if let Ok(mut socket) = gloo_net::websocket::futures::WebSocket::open(&url) {
                while let Some(Ok(message)) = socket.next().await {
                    backoff_ms = 1_000;
                    if let gloo_net::websocket::Message::Text(text) = message {
                        if let Ok(event) = serde_json::from_str::<WorkspaceEventDto>(&text) {
                            if event.client_id.as_deref() == Some(client_id().as_str()) {
                                continue;
                            }
                            schedule_refetch(data, hold, set_data, set_error);
                        }
                    }
                }
            }
            // A connection that lived for a while may have missed events when
            // it dropped; sync up once before reconnecting.
            if js_sys::Date::now() - connected_at > 5_000.0 {
                backoff_ms = 1_000;
                if data.try_get_untracked().flatten().is_some() {
                    schedule_refetch(data, hold, set_data, set_error);
                }
            }
            gloo_timers::future::TimeoutFuture::new(backoff_ms).await;
            backoff_ms = (backoff_ms * 2).min(30_000);
        }
        running.set_value(false);
    });
}

fn websocket_url() -> Option<String> {
    let location = web_sys::window()?.location();
    let protocol = if location.protocol().ok()? == "https:" {
        "wss"
    } else {
        "ws"
    };
    let host = location.host().ok()?;
    Some(format!("{protocol}://{host}/api/ws?client_id={}", client_id()))
}

fn upload_attachments(
    task_id: String,
    files: web_sys::FileList,
    set_uploading: WriteSignal<bool>,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) {
    let Ok(form) = web_sys::FormData::new() else {
        return;
    };
    let mut any = false;
    for i in 0..files.length() {
        if let Some(file) = files.get(i) {
            if form.append_with_blob("files", &file).is_ok() {
                any = true;
            }
        }
    }
    if !any {
        return;
    }
    set_uploading.set(true);
    spawn_local(async move {
        match api_post_form::<TaskDto>(&format!("/api/tasks/{task_id}/attachments"), form).await {
            Ok(task) => {
                replace_task(set_data, task);
                set_error.set(None);
            }
            Err(err) => set_error.set(Some(err.message)),
        }
        set_uploading.set(false);
    });
}

fn attachment_ext(file_name: &str) -> String {
    file_name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default()
}

/// Attachment chip plus an inline preview: images render directly, PDFs get
/// a toggleable iframe, everything else stays a plain download link.
fn attachment_view(a: AttachmentDto, lang: ReadSignal<Lang>) -> View {
    let ext = attachment_ext(&a.file_name);
    let inline_url = format!("/api/attachments/{}?inline=1", a.id);
    let download_url = format!("/api/attachments/{}", a.id);
    let chip = view! {
        <a class="file-chip" href=download_url download>"Datei "{a.file_name.clone()}<small>{a.size_label.clone()}</small></a>
    };
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "webp" => {
            let alt = a.file_name.clone();
            view! {
                <div class="attachment">
                    {chip}
                    <a href=inline_url.clone() target="_blank" rel="noopener">
                        <img class="attach-preview" src=inline_url loading="lazy" alt=alt/>
                    </a>
                </div>
            }
            .into_view()
        }
        "pdf" => {
            let (preview, set_preview) = create_signal(false);
            let title = a.file_name.clone();
            view! {
                <div class="attachment">
                    {chip}
                    <button class="link-button" on:click=move |_| set_preview.update(|p| *p = !*p)>
                        {move || match (preview.get(), lang.get() == Lang::De) {
                            (true, true) => "Vorschau ausblenden",
                            (true, false) => "Hide preview",
                            (false, true) => "Vorschau anzeigen",
                            (false, false) => "Show preview",
                        }}
                    </button>
                    {move || preview.get().then(|| view! {
                        <iframe class="attach-pdf" src=inline_url.clone() title=title.clone()></iframe>
                    })}
                </div>
            }
            .into_view()
        }
        _ => view! { <div class="attachment">{chip}</div> }.into_view(),
    }
}

/// The trailing "@query" fragment of the comment draft, if the cursor sits in
/// one: returns the byte offset of the '@' and the query after it. The '@'
/// must start the input or follow whitespace.
fn mention_query(value: &str) -> Option<(usize, String)> {
    let at = value.rfind('@')?;
    if value[..at]
        .chars()
        .next_back()
        .is_some_and(|c| !c.is_whitespace())
    {
        return None;
    }
    let query = &value[at + 1..];
    if query.len() > 30 {
        return None;
    }
    Some((at, query.to_string()))
}

/// Splits a comment body into (text, is_mention) segments. Uses the same
/// rule as the backend parser: exact member names, longest first, followed by
/// a non-alphanumeric boundary.
fn mention_segments(body: &str, names: &[String]) -> Vec<(String, bool)> {
    let mut by_length: Vec<&String> = names.iter().filter(|n| !n.trim().is_empty()).collect();
    by_length.sort_by_key(|name| std::cmp::Reverse(name.len()));

    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for name in by_length {
        let pattern = format!("@{name}");
        for (start, _) in body.match_indices(&pattern) {
            let end = start + pattern.len();
            let boundary_ok = body[end..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_alphanumeric());
            let overlaps = ranges.iter().any(|&(s, e)| start < e && end > s);
            if boundary_ok && !overlaps {
                ranges.push((start, end));
            }
        }
    }
    ranges.sort_unstable();

    let mut out = Vec::new();
    let mut cursor = 0;
    for (start, end) in ranges {
        if start > cursor {
            out.push((body[cursor..start].to_string(), false));
        }
        out.push((body[start..end].to_string(), true));
        cursor = end;
    }
    if cursor < body.len() {
        out.push((body[cursor..].to_string(), false));
    }
    out
}

/// Renders a comment body with member mentions highlighted. Builds views from
/// plain segments (never raw HTML), so bodies stay XSS-safe.
fn comment_body_view(body: &str, member_names: &[String]) -> View {
    mention_segments(body, member_names)
        .into_iter()
        .map(|(text, is_mention)| {
            if is_mention {
                view! { <span class="mention">{text}</span> }.into_view()
            } else {
                text.into_view()
            }
        })
        .collect_view()
}

fn task_title(task: &TaskDto, lang: Lang) -> String {
    title_for(task.title.clone(), task.title_en.clone(), lang)
}

fn description_for(task: &TaskDto, lang: Lang) -> String {
    title_for(task.description.clone(), task.description_en.clone(), lang)
}

fn title_for(de: String, en: Option<String>, lang: Lang) -> String {
    if lang == Lang::En {
        en.unwrap_or(de)
    } else {
        de
    }
}

fn status_name(status: &StatusDto, lang: Lang) -> &'_ str {
    if lang == Lang::De {
        &status.name_de
    } else {
        &status.name_en
    }
}

fn status_color(statuses: &[StatusDto], status_id: &str) -> String {
    statuses
        .iter()
        .find(|s| s.id == status_id)
        .map(|s| s.color.clone())
        .unwrap_or_else(|| "#6b8aa6".into())
}

fn priority_label(priority: &Priority, lang: Lang) -> &'static str {
    match (priority, lang) {
        (Priority::Urgent, Lang::De) => "Dringend",
        (Priority::Urgent, Lang::En) => "Urgent",
        (Priority::High, Lang::De) => "Hoch",
        (Priority::High, Lang::En) => "High",
        (Priority::Medium, Lang::De) => "Mittel",
        (Priority::Medium, Lang::En) => "Medium",
        (Priority::Low, Lang::De) => "Niedrig",
        (Priority::Low, Lang::En) => "Low",
    }
}

fn priority_class(priority: &Priority) -> &'static str {
    match priority {
        Priority::Urgent => "urgent",
        Priority::High => "high",
        Priority::Medium => "medium",
        Priority::Low => "low",
    }
}

fn priority_from_value(value: &str) -> Priority {
    match value {
        "urgent" => Priority::Urgent,
        "high" => Priority::High,
        "low" => Priority::Low,
        _ => Priority::Medium,
    }
}

fn recurrence_from_value(value: &str) -> Option<Recurrence> {
    match value {
        "daily" => Some(Recurrence::Daily),
        "weekly" => Some(Recurrence::Weekly),
        "biweekly" => Some(Recurrence::Biweekly),
        "monthly" => Some(Recurrence::Monthly),
        _ => None,
    }
}

fn recurrence_value(recurrence: Option<&Recurrence>) -> &'static str {
    match recurrence {
        Some(Recurrence::Daily) => "daily",
        Some(Recurrence::Weekly) => "weekly",
        Some(Recurrence::Biweekly) => "biweekly",
        Some(Recurrence::Monthly) => "monthly",
        None => "",
    }
}

fn recurrence_label(recurrence: Option<&Recurrence>, lang: Lang) -> &'static str {
    match (recurrence, lang) {
        (Some(Recurrence::Daily), Lang::De) => "Täglich",
        (Some(Recurrence::Daily), Lang::En) => "Daily",
        (Some(Recurrence::Weekly), Lang::De) => "Wöchentlich",
        (Some(Recurrence::Weekly), Lang::En) => "Weekly",
        (Some(Recurrence::Biweekly), Lang::De) => "Alle 2 Wochen",
        (Some(Recurrence::Biweekly), Lang::En) => "Every 2 weeks",
        (Some(Recurrence::Monthly), Lang::De) => "Monatlich",
        (Some(Recurrence::Monthly), Lang::En) => "Monthly",
        (None, Lang::De) => "Keine Wiederholung",
        (None, Lang::En) => "No repeat",
    }
}

/// The recurrence options for a task select, with `current` preselected.
fn recurrence_options(current: Option<Recurrence>, lang: ReadSignal<Lang>) -> View {
    [
        None,
        Some(Recurrence::Daily),
        Some(Recurrence::Weekly),
        Some(Recurrence::Biweekly),
        Some(Recurrence::Monthly),
    ]
    .into_iter()
    .map(|option| {
        let selected = option == current;
        let value = recurrence_value(option.as_ref());
        view! {
            <option value=value selected=selected>
                {move || recurrence_label(option.as_ref(), lang.get())}
            </option>
        }
    })
    .collect_view()
}

fn role_from_value(value: &str) -> Role {
    match value {
        "owner" => Role::Owner,
        "admin" => Role::Admin,
        "viewer" => Role::Viewer,
        _ => Role::Member,
    }
}

fn ticket_status_from_value(value: &str) -> TicketStatus {
    match value {
        "in_progress" => TicketStatus::InProgress,
        "resolved" => TicketStatus::Resolved,
        "closed" => TicketStatus::Closed,
        _ => TicketStatus::Open,
    }
}

fn ticket_status_label(status: &TicketStatus, lang: Lang) -> &'static str {
    match (status, lang) {
        (TicketStatus::Open, Lang::De) => "Offen",
        (TicketStatus::Open, Lang::En) => "Open",
        (TicketStatus::InProgress, Lang::De) => "In Arbeit",
        (TicketStatus::InProgress, Lang::En) => "In progress",
        (TicketStatus::Resolved, Lang::De) => "Geloest",
        (TicketStatus::Resolved, Lang::En) => "Resolved",
        (TicketStatus::Closed, Lang::De) => "Geschlossen",
        (TicketStatus::Closed, Lang::En) => "Closed",
    }
}

fn ticket_status_class(status: &TicketStatus) -> &'static str {
    match status {
        TicketStatus::Open => "open",
        TicketStatus::InProgress => "active",
        TicketStatus::Resolved => "resolved",
        TicketStatus::Closed => "closed",
    }
}

fn role_label(role: &Role, lang: Lang) -> &'static str {
    match (role, lang) {
        (Role::Owner, Lang::De) => "Owner",
        (Role::Owner, Lang::En) => "Owner",
        (Role::Admin, Lang::De) => "Admin",
        (Role::Admin, Lang::En) => "Admin",
        (Role::Member, Lang::De) => "Mitglied",
        (Role::Member, Lang::En) => "Member",
        (Role::Viewer, Lang::De) => "Betrachter",
        (Role::Viewer, Lang::En) => "Viewer",
    }
}

fn notif_text(n: &NotificationDto, lang: Lang) -> String {
    if lang == Lang::En {
        n.text_en.clone().unwrap_or_else(|| "updated".into())
    } else {
        n.text.clone().unwrap_or_else(|| "hat aktualisiert".into())
    }
}

fn subtask_pct(task: &TaskDto) -> usize {
    if task.subtasks.is_empty() {
        0
    } else {
        task.subtasks.iter().filter(|s| s.done).count() * 100 / task.subtasks.len()
    }
}

fn assignee_avatars(ids: &[String], members: &[MemberDto]) -> View {
    view! {
        <span class="avatars">
            {ids.iter().filter_map(|id| members.iter().find(|m| &m.user_id == id)).map(|m| view! {
                <i>{m.initials.clone()}</i>
            }).collect_view()}
        </span>
    }
    .into_view()
}

fn header_title(boot: &BootstrapDto, nav: NavView, lang: Lang) -> String {
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

fn header_subtitle(boot: &BootstrapDto, nav: NavView, lang: Lang) -> String {
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

fn first_name(name: &str) -> &str {
    name.split_whitespace().next().unwrap_or(name)
}

fn initials(name: &str) -> String {
    let mut chars = name
        .split_whitespace()
        .filter_map(|part| part.chars().next())
        .take(2)
        .collect::<String>();
    if chars.is_empty() {
        chars = "?".into();
    }
    chars.to_uppercase()
}

fn nav_icon(view: NavView) -> &'static str {
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

/// Local current date as (year, month 1-12, day 1-31).
fn now_date() -> (i32, u32, u32) {
    let d = js_sys::Date::new_0();
    (d.get_full_year() as i32, d.get_month() + 1, d.get_date())
}

fn today_iso() -> String {
    let (y, m, d) = now_date();
    format!("{y:04}-{m:02}-{d:02}")
}

fn iso_in_days(days: i64) -> String {
    let ms = js_sys::Date::now() + days as f64 * 86_400_000.0;
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ms));
    format!(
        "{:04}-{:02}-{:02}",
        d.get_full_year(),
        d.get_month() + 1,
        d.get_date()
    )
}

fn parse_iso(iso: &str) -> Option<(i32, u32, u32)> {
    let mut parts = iso.split('-');
    let y = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    let d: u32 = parts.next()?.parse().ok()?;
    ((1..=12).contains(&m) && (1..=31).contains(&d)).then_some((y, m, d))
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => 31,
    }
}

/// Days since 1970-01-01 (Howard Hinnant's civil-calendar algorithm).
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = y as i64 - if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * ((m as i64 + 9) % 12) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719_468
}

/// Inverse of `days_from_civil`.
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    ((if m <= 2 { y + 1 } else { y }) as i32, m, d)
}

fn iso_day_number(iso: &str) -> Option<i64> {
    parse_iso(iso).map(|(y, m, d)| days_from_civil(y, m, d))
}

const MONTHS_DE: [&str; 12] = [
    "Jan", "Feb", "Mär", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez",
];
const MONTHS_EN: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTHS_DE_FULL: [&str; 12] = [
    "Januar",
    "Februar",
    "März",
    "April",
    "Mai",
    "Juni",
    "Juli",
    "August",
    "September",
    "Oktober",
    "November",
    "Dezember",
];
const MONTHS_EN_FULL: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

fn fmt_date(iso: &str, lang: Lang) -> String {
    let Some((_, m, d)) = parse_iso(iso) else {
        return iso.to_string();
    };
    let month = if lang == Lang::De {
        MONTHS_DE[(m - 1) as usize]
    } else {
        MONTHS_EN[(m - 1) as usize]
    };
    if lang == Lang::De {
        format!("{d}. {month}")
    } else {
        format!("{month} {d}")
    }
}

fn select_value(ev: &web_sys::Event) -> String {
    event_target::<HtmlSelectElement>(ev).value()
}

fn textarea_value(ev: &web_sys::Event) -> String {
    event_target::<HtmlTextAreaElement>(ev).value()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_day_roundtrip() {
        for iso in ["2026-06-11", "2024-02-29", "1999-12-31", "2026-01-01"] {
            let n = iso_day_number(iso).expect("parses");
            let (y, m, d) = civil_from_days(n);
            assert_eq!(format!("{y:04}-{m:02}-{d:02}"), iso);
        }
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(
            days_from_civil(2026, 6, 12) - days_from_civil(2026, 6, 11),
            1
        );
    }

    #[test]
    fn month_lengths() {
        assert_eq!(days_in_month(2026, 6), 30);
        assert_eq!(days_in_month(2026, 7), 31);
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29);
        assert_eq!(days_in_month(1900, 2), 28);
    }

    #[test]
    fn dates_format_with_real_month_names() {
        assert_eq!(fmt_date("2026-03-05", Lang::De), "5. Mär");
        assert_eq!(fmt_date("2026-03-05", Lang::En), "Mar 5");
        assert_eq!(fmt_date("2026-12-24", Lang::De), "24. Dez");
        assert_eq!(fmt_date("not-a-date", Lang::De), "not-a-date");
    }

    #[test]
    fn mention_query_finds_trailing_fragment() {
        assert_eq!(mention_query("hallo @An"), Some((6, "An".to_string())));
        assert_eq!(mention_query("@"), Some((0, String::new())));
        assert_eq!(
            mention_query("@Anna Sch"),
            Some((0, "Anna Sch".to_string()))
        );
        // '@' glued to a word (e-mail address) is not a mention trigger.
        assert_eq!(mention_query("mail an anna@web.de"), None);
        assert_eq!(mention_query("kein at"), None);
    }

    #[test]
    fn mention_segments_highlight_known_names() {
        let names = vec!["Anna".to_string(), "Anna Schmidt".to_string()];
        assert_eq!(
            mention_segments("ping @Anna Schmidt jetzt", &names),
            vec![
                ("ping ".to_string(), false),
                ("@Anna Schmidt".to_string(), true),
                (" jetzt".to_string(), false),
            ]
        );
        // Boundary: longer words never match a shorter member name.
        assert_eq!(
            mention_segments("@Annabelle hi", &names),
            vec![("@Annabelle hi".to_string(), false)]
        );
        assert_eq!(
            mention_segments("ohne mention", &names),
            vec![("ohne mention".to_string(), false)]
        );
    }

    #[test]
    fn attachment_extensions_are_lowercased() {
        assert_eq!(attachment_ext("Plan.PDF"), "pdf");
        assert_eq!(attachment_ext("foto.jpeg"), "jpeg");
        assert_eq!(attachment_ext("noext"), "");
    }
}
