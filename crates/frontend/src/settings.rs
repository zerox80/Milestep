use crate::*;

/// Personal settings page. Currently exposes the visual theme picker; the
/// selected theme is held in a context signal (provided by `AppRoot`) and
/// persisted to `localStorage` via `apply_theme`.
pub(crate) fn settings_view(lang: ReadSignal<Lang>) -> View {
    let (theme, set_theme) = use_context::<(ReadSignal<Theme>, WriteSignal<Theme>)>()
        .expect("theme context provided by AppRoot");

    let cards = Theme::ALL
        .into_iter()
        .map(move |option| {
            let swatches = option.swatches();
            view! {
                <button
                    class="theme-card"
                    class:active=move || theme.get() == option
                    on:click=move |_| set_theme.set(option)
                >
                    <span class="theme-swatches">
                        {swatches.into_iter().map(|c| view! {
                            <i class="theme-swatch" style=format!("background:{c}")></i>
                        }).collect_view()}
                    </span>
                    <strong>{move || option.label(lang.get())}</strong>
                    <small>{move || option.description(lang.get())}</small>
                    <span class="theme-check">{move || if theme.get() == option {
                        lang.get().tr("Aktiv", "Active")
                    } else {
                        lang.get().tr("Auswählen", "Select")
                    }}</span>
                </button>
            }
        })
        .collect_view();

    view! {
        <div class="settings-space">
            <section class="panel">
                <div class="panel-head">
                    <h3>{move || lang.get().tr("Design / Theme", "Appearance / theme")}</h3>
                </div>
                <p class="settings-hint">
                    {move || if lang.get().is_de() {
                        "Wähle das Erscheinungsbild der App. Die Auswahl wird in diesem Browser gespeichert."
                    } else {
                        "Choose how the app looks. Your choice is saved in this browser."
                    }}
                </p>
                <div class="theme-grid">
                    {cards}
                </div>
            </section>
        </div>
    }
    .into_view()
}
