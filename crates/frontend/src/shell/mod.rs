mod chrome;
mod content;
mod helpers;
mod metadata;

use crate::*;

pub(crate) use chrome::dashboard;
pub(crate) use helpers::{
    confirm, confirm_delete, confirm_delete_attachment, confirm_remove_member, empty_view, logo,
    report_api_error, require_title, stat,
};

/// Signals shared by the application shell and its content views.
///
/// Keeping the shell state in one value makes the dashboard boundary explicit
/// and avoids passing a growing list of signals through every view.
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
