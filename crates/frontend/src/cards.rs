use crate::*;

#[allow(dead_code)]
pub(crate) fn task_detail_readonly(
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
        .map_or_else(|| "-".into(), |d| fmt_date(d, lang.get()));
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
pub(crate) fn notifications_panel(
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
pub(crate) fn task_card(
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
        .map_or_else(|| "-".into(), |d| fmt_date(d, lang.get()));
    let assignees = task.assignee_ids;
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
pub(crate) fn task_row(
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
        .map_or_else(|| "-".into(), |d| fmt_date(d, lang.get()));
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
pub(crate) fn subtask_pct(task: &TaskDto) -> usize {
    if task.subtasks.is_empty() {
        0
    } else {
        task.subtasks.iter().filter(|s| s.done).count() * 100 / task.subtasks.len()
    }
}

pub(crate) fn assignee_avatars(ids: &[String], members: &[MemberDto]) -> View {
    view! {
        <span class="avatars">
            {ids.iter().filter_map(|id| members.iter().find(|m| &m.user_id == id)).map(|m| view! {
                <i>{m.initials.clone()}</i>
            }).collect_view()}
        </span>
    }
    .into_view()
}
