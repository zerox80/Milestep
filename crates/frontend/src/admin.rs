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
                                        {if is_member {
                                            view! { <b>{move || if lang.get() == Lang::De { "Im Workspace" } else { "In workspace" }}</b> }.into_view()
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
