use crate::*;

use super::AppSignals;

/// Chooses the active content view for the current shell state.
pub(super) fn main_view(boot: BootstrapDto, signals: &AppSignals) -> View {
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
