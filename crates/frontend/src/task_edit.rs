use crate::*;

#[derive(Clone)]
pub(crate) struct TaskEditSnapshot {
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) priority: Priority,
    pub(crate) status_id: String,
    pub(crate) due_date: String,
    pub(crate) phase: String,
    pub(crate) recurrence: Option<Recurrence>,
    pub(crate) recurrence_changed: bool,
    pub(crate) assignee_id: String,
    /// Full original assignee list so edits that do not touch the assignee
    /// dropdown do not silently reduce multiple assignees to one.
    pub(crate) assignee_ids: Vec<String>,
}

#[derive(Clone, Copy)]
pub(crate) struct TaskEditSetters {
    pub(crate) title: WriteSignal<String>,
    pub(crate) description: WriteSignal<String>,
    pub(crate) status: WriteSignal<String>,
    pub(crate) priority: WriteSignal<Priority>,
    pub(crate) due_date: WriteSignal<String>,
    pub(crate) phase: WriteSignal<String>,
    pub(crate) assignee: WriteSignal<String>,
    pub(crate) recurrence: WriteSignal<Option<Recurrence>>,
    pub(crate) recurrence_changed: WriteSignal<bool>,
}

pub(crate) fn task_update_payload(edit: TaskEditSnapshot) -> UpdateTaskRequest {
    let assignee_ids = if edit.assignee_id.trim().is_empty() {
        Vec::new()
    } else if edit.assignee_ids.len() > 1 && edit.assignee_ids.first() == Some(&edit.assignee_id) {
        // The dropdown only shows the first assignee. If it was not changed,
        // preserve the full original list instead of reducing it to one.
        edit.assignee_ids
    } else {
        vec![edit.assignee_id]
    };

    UpdateTaskRequest {
        title: Some(edit.title),
        description: Some(edit.description),
        tag: None,
        tag_color: None,
        priority: Some(edit.priority),
        status_id: Some(edit.status_id),
        start_date: None,
        due_date: Some((!edit.due_date.trim().is_empty()).then_some(edit.due_date)),
        phase: Some(edit.phase),
        recurrence: edit.recurrence_changed.then_some(edit.recurrence),
        assignee_ids: Some(assignee_ids),
    }
}

pub(crate) fn reset_task_edit(setters: TaskEditSetters, values: TaskEditSnapshot) {
    setters.title.set(values.title);
    setters.description.set(values.description);
    setters.status.set(values.status_id);
    setters.priority.set(values.priority);
    setters.due_date.set(values.due_date);
    setters.phase.set(values.phase);
    setters.assignee.set(values.assignee_id);
    setters.recurrence.set(values.recurrence);
    setters.recurrence_changed.set(values.recurrence_changed);
}
