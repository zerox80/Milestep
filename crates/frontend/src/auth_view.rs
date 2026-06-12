use crate::*;

pub(crate) fn auth_shell(
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
pub(crate) fn LangToggle(lang: ReadSignal<Lang>, set_lang: WriteSignal<Lang>) -> impl IntoView {
    view! {
        <div class="lang-toggle">
            <button class:active=move || lang.get() == Lang::De on:click=move |_| set_lang.set(Lang::De)>"DE"</button>
            <button class:active=move || lang.get() == Lang::En on:click=move |_| set_lang.set(Lang::En)>"EN"</button>
        </div>
    }
}
