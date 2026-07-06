use crate::*;

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
                <h3>{move || lang.get().tr("Benachrichtigungen", "Notifications")}</h3>
                <button on:click=move |_| read_all_notifications(set_data, set_error)>{move || lang.get().tr("Alle als gelesen markieren", "Mark all read")}</button>
            </header>
            {if notifications.is_empty() {
                view! {
                    <div class="empty-state compact">
                        <strong>{move || lang.get().tr("Alles gelesen", "All caught up")}</strong>
                        <span>{move || lang.get().tr("Neue Hinweise erscheinen hier.", "New updates will appear here.")}</span>
                    </div>
                }.into_view()
            } else {
                notifications.into_iter().map(|n| {
                    let id = n.id.clone();
                    let unread = n.unread;
                    let actor_initials = n.actor_initials.clone().unwrap_or_else(|| "•".into());
                    let actor_name = n.actor_name.clone().unwrap_or_else(|| "System".into());
                    let text = notif_text(&n, lang.get());
                    let created = if lang.get().is_de() { n.created_label_de.clone() } else { n.created_label_en.clone() };
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
                }).collect_view().into_view()
            }}
        </div>
    }.into_view()
}
pub(crate) fn task_card(
    task: TaskDto,
    members: Vec<MemberDto>,
    lang: ReadSignal<Lang>,
    set_open_task: WriteSignal<Option<String>>,
    set_drag_task: WriteSignal<Option<String>>,
    can_edit: bool,
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
        <article class="task-card" draggable=if can_edit { "true" } else { "false" }
            on:dragstart=move |_| {
                if can_edit {
                    set_drag_task.set(Some(drag_id.clone()));
                }
            }
            on:click=move |_| set_open_task.set(Some(open_id.clone()))>
            <div class="task-tags">
                <span class=tag_class>{tag}</span>
                {recurring.then(|| view! {
                    <span class="recur-mark" title=move || lang.get().tr("Wiederkehrende Aufgabe", "Recurring task")>"↻"</span>
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
