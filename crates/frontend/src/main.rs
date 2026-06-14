pub(crate) use gloo_net::http::Request;
pub(crate) use kowobau_shared::*;
pub(crate) use leptos::*;
pub(crate) use leptos_router::*;
pub(crate) use serde::{de::DeserializeOwned, Serialize};
pub(crate) use wasm_bindgen_futures::spawn_local;
pub(crate) use web_sys::{DragEvent, HtmlSelectElement, HtmlTextAreaElement, RequestCredentials};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lang {
    De,
    En,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NavView {
    Overview,
    Board,
    Tickets,
    Calendar,
    Gantt,
    Roadmap,
    Team,
    Admin,
    Settings,
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
            (Self::Settings, Lang::De) => "Einstellungen",
            (Self::Settings, Lang::En) => "Settings",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthMode {
    Login,
    Register,
}

fn boot_splash() -> View {
    view! {
        <main class="boot-page" aria-busy="true">
            {logo()}
            <span>"KoWoBau-Planner wird geladen..."</span>
        </main>
    }
    .into_view()
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
pub(crate) fn AppRoot() -> impl IntoView {
    let (lang, set_lang) = create_signal(Lang::De);
    let (theme, set_theme) = create_signal(load_theme());
    // Reflect the selected theme onto <html data-theme>. Runs once on boot
    // (applying the stored choice) and on every later change. Persistence only
    // happens on actual user changes (prev.is_some()); the boot value was just
    // read back from localStorage, so writing it again would be redundant.
    create_effect(move |prev| {
        let theme = theme.get();
        apply_theme(theme);
        if prev.is_some() {
            persist_theme(theme);
        }
        theme
    });
    provide_context((theme, set_theme));
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
            match api_get::<BootstrapDto>(&bootstrap_url()).await {
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
                    &AppSignals {
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
                    },
                ).into_view(),
                None if loading.get() => boot_splash().into_view(),
                None => auth_shell(lang, set_lang, reload, error, loading).into_view(),
            }}
        </div>
    }
}

mod actions;
mod admin;
mod api;
mod attachments_ui;
mod auth_view;
mod cards;
mod i18n;
mod modals;
mod realtime;
mod settings;
mod shell;
mod task_detail;
mod task_edit;
#[cfg(test)]
mod tests;
mod theme;
mod ticket_detail;
mod views_board;
mod views_gantt;
mod views_timeline;

pub(crate) use actions::*;
pub(crate) use admin::*;
pub(crate) use api::*;
pub(crate) use attachments_ui::*;
pub(crate) use auth_view::*;
pub(crate) use cards::*;
pub(crate) use i18n::*;
pub(crate) use modals::*;
pub(crate) use realtime::*;
pub(crate) use settings::*;
pub(crate) use shell::*;
pub(crate) use task_detail::*;
pub(crate) use task_edit::*;
pub(crate) use theme::*;
pub(crate) use ticket_detail::*;
pub(crate) use views_board::*;
pub(crate) use views_gantt::*;
pub(crate) use views_timeline::*;
