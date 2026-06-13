use crate::*;

pub(crate) fn admin_view(
    boot: BootstrapDto,
    lang: ReadSignal<Lang>,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) -> View {
    let can_admin = boot.current_role.can_admin();
    let can_owner = boot.current_role == Role::Owner;
    let workspace_id = boot.workspace.id.clone();
    let current_user_id = boot.current_user.id.clone();
    let member_count = boot.members.len();
    let admin_count = boot
        .members
        .iter()
        .filter(|m| matches!(m.role, Role::Owner | Role::Admin))
        .count();
    let registered_count = boot.registered_users.len();
    let latest_activity = boot.audit_events.first().map_or_else(
        || "-".into(),
        |a| {
            if lang.get_untracked() == Lang::De {
                a.created_label_de.clone()
            } else {
                a.created_label_en.clone()
            }
        },
    );

    let (search, set_search) = create_signal(String::new());
    let (member_role_filter, set_member_role_filter) = create_signal("all".to_string());
    let (account_role_filter, set_account_role_filter) = create_signal("all".to_string());
    let (invite_email, set_invite_email) = create_signal(String::new());
    let (invite_role, set_invite_role) = create_signal(Role::Member);
    let (invite_result, set_invite_result) = create_signal::<Option<String>>(None);
    let (local_error, set_local_error) = create_signal::<Option<String>>(None);

    let members_for_list = boot.members.clone();
    let accounts_for_list = boot.registered_users.clone();
    let audit_events = boot.audit_events;
    let workspace_id_for_invite = workspace_id.clone();
    let workspace_id_for_accounts = workspace_id;
    let current_user_id_for_members = current_user_id;

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
                                    "Bestehender User wurde direkt hinzugefuegt.".into()
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
        <div class="admin-space">
            <section class="admin-summary">
                {admin_metric("Mitglieder", "Members", member_count.to_string(), if lang.get() == Lang::De { "aktive Workspace-Zugaenge" } else { "active workspace access" }, "cool", lang)}
                {admin_metric("Owner/Admins", "Owners/admins", admin_count.to_string(), if lang.get() == Lang::De { "koennen verwalten" } else { "can manage" }, "accent", lang)}
                {admin_metric("Accounts", "Accounts", registered_count.to_string(), if lang.get() == Lang::De { "registriert" } else { "registered" }, "good", lang)}
                {admin_metric("Letzte Aktivitaet", "Latest activity", latest_activity, if lang.get() == Lang::De { "Audit-Log" } else { "audit log" }, "warm", lang)}
            </section>

            <section class="panel admin-toolbar">
                <div>
                    <h3>{move || if lang.get() == Lang::De { "Verwaltung" } else { "Management" }}</h3>
                    <p class="muted">{move || if lang.get() == Lang::De { "Mitglieder, Accounts und Rollen durchsuchen." } else { "Search members, accounts and roles." }}</p>
                </div>
                <label class="admin-search">
                    <span>"⌕"</span>
                    <input placeholder=move || if lang.get() == Lang::De { "Name oder E-Mail suchen..." } else { "Search name or email..." } prop:value=search on:input=move |ev| set_search.set(event_target_value(&ev))/>
                </label>
                <div class="admin-filters">
                    <select on:change=move |ev| set_member_role_filter.set(select_value(&ev))>
                        <option value="all">{move || if lang.get() == Lang::De { "Alle Mitglieder" } else { "All members" }}</option>
                        {role_filter_options(lang)}
                    </select>
                    <select on:change=move |ev| set_account_role_filter.set(select_value(&ev))>
                        <option value="all">{move || if lang.get() == Lang::De { "Alle Accounts" } else { "All accounts" }}</option>
                        {role_filter_options(lang)}
                        <option value="none">{move || if lang.get() == Lang::De { "Ohne Workspace" } else { "No workspace" }}</option>
                    </select>
                </div>
            </section>

            <div class="admin-layout">
                <section class="panel admin-members-panel">
                    <div class="panel-head">
                        <h3>{move || if lang.get() == Lang::De { "Mitglieder" } else { "Members" }}</h3>
                        <span class="admin-count">{member_count}</span>
                    </div>
                    {move || {
                        let query = search.get().trim().to_lowercase();
                        let role_filter = member_role_filter.get();
                        let rows = members_for_list
                            .iter()
                            .filter(|m| admin_text_matches(&query, &m.name, &m.email))
                            .filter(|m| role_filter_matches(&role_filter, Some(&m.role)))
                            .cloned()
                            .collect::<Vec<_>>();
                        if rows.is_empty() {
                            admin_empty(lang, "Keine passenden Mitglieder", "No matching members").into_view()
                        } else {
                            rows.into_iter().map(|m| {
                                let membership_id = m.id.clone();
                                let remove_id = m.id.clone();
                                let current_role = m.role.clone();
                                let is_current_user = m.user_id == current_user_id_for_members;
                                let member_name = m.name.clone();
                                let member_name_for_remove = m.name.clone();
                                let can_change_owner_target = can_owner || current_role != Role::Owner;
                                view! {
                                    <div class="admin-member-card">
                                        <span class="avatar tiny">{m.initials.clone()}</span>
                                        <span class="admin-person">
                                            <strong>{m.name.clone()}</strong>
                                            <small>{m.email.clone()}</small>
                                        </span>
                                        <span class="admin-workload">
                                            <b>{m.open_tasks}</b>
                                            <small>{move || if lang.get() == Lang::De { "offen" } else { "open" }}</small>
                                            <b>{m.done_tasks}</b>
                                            <small>{move || if lang.get() == Lang::De { "fertig" } else { "done" }}</small>
                                        </span>
                                        <span>
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
                                                view! { <b class="role-pill">{role_label(&m.role, lang.get())}</b> }.into_view()
                                            }}
                                        </span>
                                        <span class="admin-actions">
                                            {if can_admin && !is_current_user && can_change_owner_target {
                                                view! {
                                                    <button class="danger-link" title=format!("Remove {member_name}") on:click=move |_| {
                                                        remove_member(remove_id.clone(), member_name_for_remove.clone(), lang, set_data, set_error);
                                                    }>{move || if lang.get() == Lang::De { "Entfernen" } else { "Remove" }}</button>
                                                }.into_view()
                                            } else {
                                                view! { <span class="muted">"-"</span> }.into_view()
                                            }}
                                        </span>
                                    </div>
                                }
                            }).collect_view().into_view()
                        }
                    }}
                </section>

                <aside class="admin-side">
                    <section class="panel">
                        <h3>{move || if lang.get() == Lang::De { "Einladen" } else { "Invite" }}</h3>
                        {if can_admin {
                            view! {
                                <div class="invite-card">
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
                        <h3>{move || if lang.get() == Lang::De { "System & Hosting" } else { "System & hosting" }}</h3>
                        <div class="sys-grid compact">
                            <span><small>"Version"</small><strong>"0.1.0"</strong></span>
                            <span><small>"Runtime"</small><strong>"Rust / Axum"</strong></span>
                            <span><small>"Frontend"</small><strong>"WASM / Leptos"</strong></span>
                            <span><small>"Datenbank"</small><strong>"PostgreSQL"</strong></span>
                        </div>
                    </section>
                </aside>
            </div>

            <section class="panel admin-accounts-panel">
                <div class="panel-head">
                    <h3>{move || if lang.get() == Lang::De { "Registrierte Accounts" } else { "Registered accounts" }}</h3>
                    <span class="admin-count">{registered_count}</span>
                </div>
                {if can_admin {
                    view! {
                        {move || {
                            let query = search.get().trim().to_lowercase();
                            let role_filter = account_role_filter.get();
                            let rows = accounts_for_list
                                .iter()
                                .filter(|user| admin_text_matches(&query, &user.name, &user.email))
                                .filter(|user| role_filter_matches(&role_filter, user.role.as_ref()))
                                .cloned()
                                .collect::<Vec<_>>();
                            if rows.is_empty() {
                                admin_empty(lang, "Keine passenden Accounts", "No matching accounts").into_view()
                            } else {
                                rows.into_iter().map(|user| {
                                    let email = user.email.clone();
                                    let email_for_add = user.email.clone();
                                    let workspace_id_for_add = workspace_id_for_accounts.clone();
                                    let membership_id = user.membership_id.clone();
                                    let current_account_role = user.role.clone();
                                    let is_member = user.membership_id.is_some();
                                    let (add_role, set_add_role) = create_signal(Role::Member);
                                    let can_change_account_owner = can_owner || current_account_role != Some(Role::Owner);
                                    let created = if lang.get() == Lang::De { user.created_label_de.clone() } else { user.created_label_en.clone() };
                                    view! {
                                        <div class="registered-row">
                                            <span class="avatar tiny">{user.initials.clone()}</span>
                                            <span><strong>{user.name}</strong><small>{email}</small></span>
                                            <span>
                                                {if let Some(member_id) = membership_id {
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
                                                        view! { <b class="role-pill">{role_label(current_account_role.as_ref().unwrap_or(&Role::Member), lang.get())}</b> }.into_view()
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
                                                {if is_member {
                                                    view! { <b class="role-pill good">{move || if lang.get() == Lang::De { "Im Workspace" } else { "In workspace" }}</b> }.into_view()
                                                } else {
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
                                                }}
                                            </span>
                                        </div>
                                    }
                                }).collect_view().into_view()
                            }
                        }}
                    }.into_view()
                } else {
                    view! { <p class="muted">{move || if lang.get() == Lang::De { "Nur Admins sehen registrierte Accounts." } else { "Only admins can view registered accounts." }}</p> }.into_view()
                }}
            </section>

            <section class="panel admin-audit-panel">
                <div class="panel-head">
                    <h3>{move || if lang.get() == Lang::De { "Audit-Log" } else { "Audit log" }}</h3>
                    <span class="admin-count">{audit_events.len()}</span>
                </div>
                <div class="audit-list">
                    {audit_events.iter().map(|a| view! {
                        <div class="activity-row">
                            <span>{a.actor_name.clone().unwrap_or_else(|| "System".into())}</span>
                            <strong>{a.action.clone()}</strong>
                            <small>{if lang.get() == Lang::De { a.created_label_de.clone() } else { a.created_label_en.clone() }}</small>
                        </div>
                    }).collect_view()}
                </div>
            </section>
        </div>
    }.into_view()
}

fn admin_metric(
    label_de: &'static str,
    label_en: &'static str,
    value: String,
    detail: &'static str,
    tone: &'static str,
    lang: ReadSignal<Lang>,
) -> View {
    view! {
        <article class=format!("admin-metric {tone}")>
            <small>{move || if lang.get() == Lang::De { label_de } else { label_en }}</small>
            <strong>{value}</strong>
            <span>{detail}</span>
        </article>
    }
    .into_view()
}

fn role_filter_options(lang: ReadSignal<Lang>) -> View {
    [
        ("owner", "Owner", "Owner"),
        ("admin", "Admin", "Admin"),
        ("member", "Mitglied", "Member"),
        ("viewer", "Betrachter", "Viewer"),
    ]
    .into_iter()
    .map(|(value, de, en)| {
        view! {
            <option value=value>{move || if lang.get() == Lang::De { de } else { en }}</option>
        }
    })
    .collect_view()
}

fn admin_text_matches(query: &str, name: &str, email: &str) -> bool {
    query.is_empty() || name.to_lowercase().contains(query) || email.to_lowercase().contains(query)
}

fn role_filter_matches(filter: &str, role: Option<&Role>) -> bool {
    matches!(
        (filter, role),
        ("all", _)
            | ("none", None)
            | ("owner", Some(Role::Owner))
            | ("admin", Some(Role::Admin))
            | ("member", Some(Role::Member))
            | ("viewer", Some(Role::Viewer))
    )
}

fn admin_empty(lang: ReadSignal<Lang>, de: &'static str, en: &'static str) -> View {
    view! {
        <div class="empty-state compact">
            <strong>{move || if lang.get() == Lang::De { de } else { en }}</strong>
        </div>
    }
    .into_view()
}
