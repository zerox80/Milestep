use crate::*;

pub(crate) fn optimistic_move(
    task_id: String,
    status_id: String,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) {
    // Remembered so the card can snap back if the server rejects the move.
    let mut previous: Option<(String, i32)> = None;
    set_data.update(|data| {
        if let Some(data) = data {
            if let Some(status) = data.statuses.iter().find(|s| s.id == status_id) {
                if let Some(task) = data.tasks.iter_mut().find(|t| t.id == task_id) {
                    previous = Some((task.status_id.clone(), task.status_position));
                    task.status_id.clone_from(&status_id);
                    task.status_position = status.position;
                }
            }
        }
    });
    let revert_task_id = task_id.clone();
    spawn_local(async move {
        match api_post::<_, TaskDto>(
            &format!("/api/tasks/{task_id}/move"),
            &MoveTaskRequest { status_id },
        )
        .await
        {
            Ok(task) => {
                replace_task(set_data, task);
                set_error.set(None);
            }
            Err(err) => {
                if let Some((prev_status_id, prev_position)) = previous {
                    set_data.update(|data| {
                        if let Some(data) = data {
                            if let Some(task) =
                                data.tasks.iter_mut().find(|t| t.id == revert_task_id)
                            {
                                task.status_id = prev_status_id;
                                task.status_position = prev_position;
                            }
                        }
                    });
                }
                set_error.set(Some(err.message));
            }
        }
    });
}

pub(crate) fn toggle_subtask(
    task_id: String,
    subtask_id: String,
    done: bool,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) {
    set_data.update(|data| {
        if let Some(data) = data {
            if let Some(task) = data.tasks.iter_mut().find(|t| t.id == task_id) {
                if let Some(sub) = task.subtasks.iter_mut().find(|s| s.id == subtask_id) {
                    sub.done = done;
                }
            }
        }
    });
    spawn_local(async move {
        let body = UpdateSubtaskRequest {
            title: None,
            done: Some(done),
        };
        match api_patch::<_, TaskDto>(
            &format!("/api/tasks/{task_id}/subtasks/{subtask_id}"),
            &body,
        )
        .await
        {
            Ok(task) => {
                replace_task(set_data, task);
                set_error.set(None);
            }
            Err(err) => {
                set_error.set(Some(err.message));
            }
        }
    });
}

pub(crate) fn add_comment(
    task_id: String,
    body: String,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) {
    spawn_local(async move {
        match api_post::<_, TaskDto>(
            &format!("/api/tasks/{task_id}/comments"),
            &CreateCommentRequest { body },
        )
        .await
        {
            Ok(task) => {
                replace_task(set_data, task);
                set_error.set(None);
            }
            Err(err) => set_error.set(Some(err.message)),
        }
    });
}

pub(crate) fn read_notification(
    id: String,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) {
    set_data.update(|data| {
        if let Some(data) = data {
            if let Some(n) = data.notifications.iter_mut().find(|n| n.id == id) {
                n.unread = false;
            }
        }
    });
    spawn_local(async move {
        match api_empty(&format!("/api/notifications/{id}/read")).await {
            Ok(()) => set_error.set(None),
            Err(err) => set_error.set(Some(err.message)),
        }
    });
}

pub(crate) fn read_all_notifications(
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) {
    set_data.update(|data| {
        if let Some(data) = data {
            for n in &mut data.notifications {
                n.unread = false;
            }
        }
    });
    spawn_local(async move {
        match api_empty(&read_all_notifications_url()).await {
            Ok(()) => set_error.set(None),
            Err(err) => set_error.set(Some(err.message)),
        }
    });
}

pub(crate) fn update_member_role(
    membership_id: String,
    role: Role,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) {
    spawn_local(async move {
        match api_patch::<_, MemberDto>(
            &format!("/api/memberships/{membership_id}"),
            &UpdateMembershipRequest { role },
        )
        .await
        {
            Ok(_) => refresh_bootstrap(set_data, set_error).await,
            Err(err) => set_error.set(Some(err.message)),
        }
    });
}

pub(crate) fn add_existing_user_to_workspace(
    workspace_id: String,
    email: String,
    role: Role,
    lang: ReadSignal<Lang>,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) {
    spawn_local(async move {
        match api_post::<_, InviteMemberResponse>(
            &format!("/api/workspaces/{workspace_id}/invites"),
            &InviteMemberRequest { email, role },
        )
        .await
        {
            Ok(_) => refresh_bootstrap(set_data, set_error).await,
            Err(err) => {
                let prefix = if lang.get_untracked().is_de() {
                    "Konnte User nicht hinzufügen"
                } else {
                    "Could not add user"
                };
                set_error.set(Some(format!("{prefix}: {}", err.message)));
            }
        }
    });
}

pub(crate) fn remove_member(
    membership_id: String,
    member_name: String,
    lang: ReadSignal<Lang>,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) {
    if !confirm_remove_member(&member_name, lang.get_untracked()) {
        return;
    }
    spawn_local(async move {
        match api_delete_empty(&format!("/api/memberships/{membership_id}")).await {
            Ok(()) => refresh_bootstrap(set_data, set_error).await,
            Err(err) => set_error.set(Some(err.message)),
        }
    });
}

pub(crate) async fn refresh_bootstrap(
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) {
    match api_get::<BootstrapDto>(&bootstrap_url()).await {
        Ok(next) => {
            set_data.set(Some(next));
            set_error.set(None);
        }
        Err(err) => set_error.set(Some(err.message)),
    }
}

pub(crate) fn replace_task(set_data: WriteSignal<Option<BootstrapDto>>, task: TaskDto) {
    update_bootstrap(set_data, |data| {
        if let Some(current) = data.tasks.iter_mut().find(|t| t.id == task.id) {
            *current = task;
        }
    });
}

pub(crate) fn remove_task(set_data: WriteSignal<Option<BootstrapDto>>, task_id: &str) {
    update_bootstrap(set_data, |data| {
        data.tasks.retain(|task| task.id != task_id);
    });
}

pub(crate) fn replace_ticket(set_data: WriteSignal<Option<BootstrapDto>>, ticket: TicketDto) {
    update_bootstrap(set_data, |data| {
        if let Some(current) = data.tickets.iter_mut().find(|t| t.id == ticket.id) {
            *current = ticket;
        }
    });
}

pub(crate) fn remove_ticket(set_data: WriteSignal<Option<BootstrapDto>>, ticket_id: &str) {
    update_bootstrap(set_data, |data| {
        data.tickets.retain(|ticket| ticket.id != ticket_id);
    });
}

pub(crate) fn delete_milestone(
    milestone_id: String,
    milestone_title: String,
    lang: ReadSignal<Lang>,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) {
    if !confirm_delete(&milestone_title, lang.get_untracked()) {
        return;
    }
    spawn_local(async move {
        match api_delete_empty(&format!("/api/milestones/{milestone_id}")).await {
            Ok(()) => {
                remove_milestone(set_data, &milestone_id);
                set_error.set(None);
            }
            Err(err) => set_error.set(Some(err.message)),
        }
    });
}

pub(crate) fn remove_milestone(set_data: WriteSignal<Option<BootstrapDto>>, milestone_id: &str) {
    update_bootstrap(set_data, |data| {
        data.milestones
            .retain(|milestone| milestone.id != milestone_id);
    });
}

fn update_bootstrap(
    set_data: WriteSignal<Option<BootstrapDto>>,
    update: impl FnOnce(&mut BootstrapDto),
) {
    set_data.update(|data| {
        if let Some(data) = data {
            update(data);
        }
    });
}
