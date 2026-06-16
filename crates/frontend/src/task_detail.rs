use crate::*;

pub(crate) fn task_detail(
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
    let (recurrence_changed, set_recurrence_changed) = create_signal(false);
    let (busy, set_busy) = create_signal(false);
    let (local_error, set_local_error) = create_signal::<Option<String>>(None);
    let (uploading, set_uploading) = create_signal(false);
    let (mention_open, set_mention_open) = create_signal(false);
    let (mention_index, set_mention_index) = create_signal(0usize);
    let mention_members = store_value(boot.members.clone());
    let can_edit = boot.current_role.can_edit();
    // Editing or a half-typed comment must not be wiped by a background refetch.
    hold_realtime_while(move || (can_edit && editing.get()) || !comment.get().trim().is_empty());

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
        .map_or_else(|| "-".into(), |d| fmt_date(d, lang.get()));
    let priority = priority_label(&task.priority, lang.get()).to_string();
    let task_recurrence = task.recurrence;
    let project_line = format!("{} / {}", task.tag, boot.project.name);
    let pct = subtask_pct(&task);
    let subtasks = task.subtasks.clone();
    let attachments = task.attachments.clone();
    let comments = task.comments.clone();
    let task_id_base = task.id.clone();
    let task_id_for_upload = task.id.clone();
    let statuses_for_status_options = boot.statuses.clone();
    let members_for_display = boot.members.clone();
    let members_for_assign = boot.members.clone();
    let member_names: Vec<String> = boot.members.iter().map(|m| m.name.clone()).collect();

    let delete_attachment = Callback::new(move |attachment_id: String| {
        spawn_local(async move {
            match api_delete::<TaskDto>(&format!("/api/attachments/{attachment_id}")).await {
                Ok(task) => {
                    replace_task(set_data, task);
                    set_error.set(None);
                }
                Err(err) => set_error.set(Some(err.message)),
            }
        });
    });

    let task_id_for_save = task.id.clone();
    let assignees_for_save = assignees.clone();
    let save = move |_| {
        if !require_title(
            &title_edit.get_untracked(),
            "Bitte gib zuerst einen Aufgabentitel ein.",
            "Add a task title first.",
            lang.get_untracked(),
            set_local_error,
        ) {
            return;
        }
        set_busy.set(true);
        let payload = task_update_payload(TaskEditSnapshot {
            title: title_edit.get_untracked(),
            description: description_edit.get_untracked(),
            priority: priority_edit.get_untracked(),
            status_id: status_edit.get_untracked(),
            due_date: due_date_edit.get_untracked(),
            phase: phase_edit.get_untracked(),
            recurrence: recurrence_edit.get_untracked(),
            recurrence_changed: recurrence_changed.get_untracked(),
            assignee_id: assignee_edit.get_untracked(),
            assignee_ids: assignees_for_save.clone(),
        });
        let task_id = task_id_for_save.clone();
        spawn_local(async move {
            match api_patch::<_, TaskDto>(&format!("/api/tasks/{task_id}"), &payload).await {
                Ok(task) => {
                    replace_task(set_data, task);
                    set_editing.set(false);
                    set_error.set(None);
                }
                Err(err) => report_api_error(&err, set_local_error, set_error),
            }
            set_busy.set(false);
        });
    };

    let task_id_for_delete = task.id.clone();
    let title_for_delete = title.clone();
    let delete = move |_| {
        if !confirm_delete(&title_for_delete, lang.get_untracked()) {
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
    let reset_assignee_ids = task.assignee_ids.clone();
    let reset_recurrence = task.recurrence;
    let reset_recurrence_changed = false;

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
            add_comment(
                task_id_for_comment_submit.clone(),
                body,
                set_data,
                set_error,
            );
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
                {move || if can_edit && editing.get() {
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
                    {move || if !can_edit || editing.get() {
                        empty_view()
                    } else {
                        view! {
                            <button class="link-button" on:click=move |_| set_editing.set(true)>
                                {move || lang.get().tr("Bearbeiten", "Edit")}
                            </button>
                        }.into_view()
                    }}
                    {if can_edit {
                        view! {
                            <button class="danger-link" on:click=delete>{move || lang.get().tr("Loeschen", "Delete")}</button>
                        }.into_view()
                    } else {
                        empty_view()
                    }}
                    <button class="drawer-close" on:click=move |_| set_open_task.set(None)>"x"</button>
                </span>
            </header>
            {move || if can_edit && editing.get() {
                view! {
                    <label class="drawer-field title-field">
                        <span>{move || lang.get().tr("Titel", "Title")}</span>
                        <input class="title-input" prop:value=title_edit on:input=move |ev| {
                            set_title_edit.set(event_target_value(&ev));
                            set_local_error.set(None);
                        }/>
                    </label>
                }.into_view()
            } else {
                view! { <h2>{title.clone()}</h2> }.into_view()
            }}
            {move || if can_edit && editing.get() {
                let current_assignee = assignee_edit.get_untracked();
                let current_priority = priority_edit.get_untracked();
                let current_phase = phase_edit.get_untracked();
                let current_recurrence = recurrence_edit.get_untracked();
                view! {
                    <div class="detail-meta">
                        <span>
                            <small>{move || lang.get().tr("Zuweisen", "Assign")}</small>
                            <select on:change=move |ev| set_assignee_edit.set(select_value(&ev))>
                                <option value="" selected=current_assignee.is_empty()>{move || lang.get().tr("Nicht zugewiesen", "Unassigned")}</option>
                                {members_for_assign.clone().into_iter().map(|m| {
                                    let selected = current_assignee == m.user_id;
                                    view! { <option value=m.user_id selected=selected>{m.name}</option> }
                                }).collect_view()}
                            </select>
                        </span>
                        <span>
                            <small>{move || lang.get().tr("Faelligkeit", "Due date")}</small>
                            <input type="date" prop:value=due_date_edit on:input=move |ev| set_due_date_edit.set(event_target_value(&ev))/>
                        </span>
                        <span>
                            <small>{move || lang.get().tr("Prioritaet", "Priority")}</small>
                            <select on:change=move |ev| set_priority_edit.set(priority_from_value(&select_value(&ev)))>
                                {priority_options(current_priority, lang)}
                            </select>
                        </span>
                        <span>
                            <small>"Phase"</small>
                            <select on:change=move |ev| set_phase_edit.set(select_value(&ev))>
                                {phase_options(current_phase, lang)}
                            </select>
                        </span>
                        <span>
                            <small>{move || lang.get().tr("Wiederholung", "Repeat")}</small>
                        <select on:change=move |ev| {
                            set_recurrence_edit.set(recurrence_from_value(&select_value(&ev)));
                            set_recurrence_changed.set(true);
                        }>
                            {recurrence_options(current_recurrence, lang)}
                        </select>
                        </span>
                    </div>
                }.into_view()
            } else {
                view! {
                    <div class="detail-meta">
                        <span><small>{move || lang.get().tr("Zuweisen", "Assign")}</small>{assignee_avatars(&assignees, &members_for_display)}</span>
                        <span><small>{move || lang.get().tr("Faelligkeit", "Due date")}</small><b>{due.clone()}</b></span>
                        <span><small>{move || lang.get().tr("Prioritaet", "Priority")}</small><b>{priority.clone()}</b></span>
                        <span><small>{move || lang.get().tr("Wiederholung", "Repeat")}</small><b>{move || recurrence_label(task_recurrence.as_ref(), lang.get())}</b></span>
                        <span><small>{move || lang.get().tr("Projekt", "Project")}</small><b>{project_line.clone()}</b></span>
                    </div>
                }.into_view()
            }}
            <section>
                <h3>{move || lang.get().tr("Beschreibung", "Description")}</h3>
                {move || if can_edit && editing.get() {
                    view! {
                        <textarea class="drawer-textarea" prop:value=description_edit on:input=move |ev| set_description_edit.set(textarea_value(&ev))></textarea>
                    }.into_view()
                } else {
                    view! { <p>{description.clone()}</p> }.into_view()
                }}
            </section>
            <section>
                <h3>{move || lang.get().tr("Unteraufgaben", "Subtasks")}</h3>
                <div class="progress-line"><i style=format!("width:{pct}%")></i></div>
                {subtasks.into_iter().map(|sub| {
                    let task_id = task_id_base.clone();
                    let sub_id = sub.id.clone();
                    let done = sub.done;
                    let label = title_for(sub.title, sub.title_en, lang.get());
                    view! {
                        <label class="subtask">
                            {if can_edit {
                                view! {
                                    <input type="checkbox" checked=done on:change=move |ev| {
                                        let checked = event_target::<web_sys::HtmlInputElement>(&ev).checked();
                                        toggle_subtask(task_id.clone(), sub_id.clone(), checked, set_data, set_error);
                                    }/>
                                }.into_view()
                            } else {
                                view! { <input type="checkbox" checked=done disabled/> }.into_view()
                            }}
                            <span>{label}</span>
                        </label>
                    }
                }).collect_view()}
            </section>
            <section>
                <h3>{move || lang.get().tr("Anhaenge", "Attachments")}</h3>
                <div class="attachments">
                    {attachments.into_iter().map(|a| attachment_view(a, lang, can_edit.then_some((editing, delete_attachment)))).collect_view()}
                </div>
                {move || (can_edit && editing.get()).then(|| {
                    let task_id = task_id_for_upload.clone();
                    view! {
                    <div class="upload-row">
                        // A label wrapping the input opens the file dialog natively.
                        // Calling input.click() from a Leptos click handler re-enters
                        // the delegated handler and panics the wasm app.
                        <label class="btn ghost upload-btn" class:disabled=uploading>
                            <input type="file" multiple accept=".pdf,.png,.jpg,.jpeg,.webp,.svg,.csv,.xlsx,.docx,.txt,.json,.zip,.dwg,.ifc" style="display:none" disabled=uploading on:change=move |ev| {
                                let input = event_target::<web_sys::HtmlInputElement>(&ev);
                                if let Some(files) = input.files() {
                                    if files.length() > 0 {
                                        upload_attachments(task_id.clone(), files, set_uploading, set_data, set_error);
                                    }
                                }
                                input.set_value("");
                            }/>
                            {move || match (uploading.get(), lang.get().is_de()) {
                                (true, true) => "Lädt hoch...",
                                (true, false) => "Uploading...",
                                (false, true) => "+ Datei hochladen",
                                (false, false) => "+ Upload file",
                            }}
                        </label>
                    </div>
                    }
                })}
            </section>
            <section>
                <h3>{move || lang.get().tr("Kommentare", "Comments")}</h3>
                {comments.into_iter().map(|c| {
                    let created = if lang.get().is_de() { c.created_label_de } else { c.created_label_en };
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
                        placeholder=move || lang.get().tr("Kommentar schreiben... (@ erwähnt)", "Write a comment... (@ mentions)")
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
            <section class="drawer-edit-actions" style=move || if can_edit && editing.get() { String::new() } else { "display:none".to_string() }>
                {move || local_error.get().map(|err| view! { <div class="modal-error inline">{err}</div> })}
                <button class="btn ghost" on:click=move |_| {
                    reset_task_edit(
                        TaskEditSetters {
                            title: set_title_edit,
                            description: set_description_edit,
                            status: set_status_edit,
                            priority: set_priority_edit,
                            due_date: set_due_date_edit,
                            phase: set_phase_edit,
                            assignee: set_assignee_edit,
                            recurrence: set_recurrence_edit,
                            recurrence_changed: set_recurrence_changed,
                        },
                        TaskEditSnapshot {
                            title: reset_title.clone(),
                            description: reset_description.clone(),
                            priority: reset_priority.clone(),
                            status_id: reset_status.clone(),
                            due_date: reset_due.clone(),
                            phase: reset_phase.clone(),
                            recurrence: reset_recurrence,
                            recurrence_changed: reset_recurrence_changed,
                            assignee_id: reset_assignee.clone(),
                            assignee_ids: reset_assignee_ids.clone(),
                        },
                    );
                    set_local_error.set(None);
                    set_editing.set(false);
                }>{move || lang.get().tr("Abbrechen", "Cancel")}</button>
                <button class="btn primary" disabled=move || busy.get() on:click=save>{move || lang.get().tr("Speichern", "Save")}</button>
            </section>
        </aside>
    }.into_view()
}
