use crate::*;

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
        format!("{name} wirklich lÃ¶schen?")
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
        format!("Anhang {name} wirklich lÃ¶schen?")
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
            <span>"Milestep"</span>
        </span>
    }
    .into_view()
}
