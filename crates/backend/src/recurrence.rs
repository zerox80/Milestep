use crate::*;

#[derive(Debug, FromRow)]
pub(crate) struct RecurrenceSourceRow {
    pub(crate) recurrence: Option<String>,
    pub(crate) is_done: bool,
    pub(crate) project_id: Uuid,
    pub(crate) title: String,
    pub(crate) title_en: Option<String>,
    pub(crate) description: String,
    pub(crate) description_en: Option<String>,
    pub(crate) tag: String,
    pub(crate) tag_color: String,
    pub(crate) priority: String,
    pub(crate) start_date: Option<NaiveDate>,
    pub(crate) due_date: Option<NaiveDate>,
    pub(crate) phase: String,
    pub(crate) created_by: Option<Uuid>,
}

/// If `task_id` just transitioned from a not-done into a done status and
/// carries a recurrence, creates the next instance (dates shifted, subtasks
/// reset, assignees copied) and moves the recurrence marker onto it. Moving
/// the marker makes repeated spawning from the same task impossible: the
/// chain continues through the new instance. Returns the new task id.
pub(crate) async fn spawn_recurrence_if_completed(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    task_id: Uuid,
    was_done: bool,
) -> Result<Option<Uuid>, AppError> {
    if was_done {
        return Ok(None);
    }
    let source: RecurrenceSourceRow = sqlx::query_as(
        "SELECT t.recurrence, s.is_done, t.project_id, t.title, t.title_en, t.description, \
                t.description_en, t.tag, t.tag_color, t.priority, t.start_date, t.due_date, \
                t.phase, t.created_by \
         FROM tasks t JOIN project_statuses s ON s.id = t.status_id WHERE t.id = $1",
    )
    .bind(task_id)
    .fetch_one(&mut **tx)
    .await?;
    let Some(recurrence) = source.recurrence.as_deref() else {
        return Ok(None);
    };
    if !source.is_done {
        return Ok(None);
    }
    let recurrence = recurrence_from_db(recurrence)?;

    // The follow-up starts in the first open status of the project; a project
    // without any open status cannot host a follow-up.
    let target_status: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM project_statuses WHERE project_id = $1 AND NOT is_done \
         ORDER BY position LIMIT 1",
    )
    .bind(source.project_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some((status_id,)) = target_status else {
        return Ok(None);
    };

    // Same advisory-lock pattern as create_task so concurrent key generation
    // cannot collide.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(source.project_id.to_string())
        .execute(&mut **tx)
        .await?;
    let (project_key,): (String,) = sqlx::query_as("SELECT key FROM projects WHERE id = $1")
        .bind(source.project_id)
        .fetch_one(&mut **tx)
        .await?;
    let next: (i32,) = sqlx::query_as(
        "SELECT COALESCE(MAX(split_part(key, '-', 2)::INT), 100) + 1 \
         FROM tasks WHERE project_id = $1 AND key LIKE $2 || '-%' \
         AND split_part(key, '-', 2) ~ '^[0-9]+$'",
    )
    .bind(source.project_id)
    .bind(&project_key)
    .fetch_one(&mut **tx)
    .await?;
    let key = format!("{}-{}", project_key, next.0);

    let new_task_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO tasks \
         (id, project_id, key, title, title_en, description, description_en, tag, tag_color, \
          priority, status_id, start_date, due_date, phase, recurrence, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)",
    )
    .bind(new_task_id)
    .bind(source.project_id)
    .bind(&key)
    .bind(&source.title)
    .bind(&source.title_en)
    .bind(&source.description)
    .bind(&source.description_en)
    .bind(&source.tag)
    .bind(&source.tag_color)
    .bind(&source.priority)
    .bind(status_id)
    .bind(shifted_start_date(source.start_date, recurrence))
    .bind(shifted_due_date(
        source.start_date,
        source.due_date,
        recurrence,
    ))
    .bind(&source.phase)
    .bind(recurrence_to_db(recurrence))
    .bind(source.created_by)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        "INSERT INTO task_assignees (task_id, user_id) \
         SELECT $1, user_id FROM task_assignees WHERE task_id = $2",
    )
    .bind(new_task_id)
    .bind(task_id)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "INSERT INTO subtasks (id, task_id, title, title_en, done, position) \
         SELECT gen_random_uuid(), $1, title, title_en, false, position \
         FROM subtasks WHERE task_id = $2",
    )
    .bind(new_task_id)
    .bind(task_id)
    .execute(&mut **tx)
    .await?;

    // The completed instance stops recurring; the new one carries the marker.
    sqlx::query("UPDATE tasks SET recurrence = NULL, updated_at = now() WHERE id = $1")
        .bind(task_id)
        .execute(&mut **tx)
        .await?;

    Ok(Some(new_task_id))
}

pub(crate) fn shift_date(date: NaiveDate, recurrence: Recurrence) -> NaiveDate {
    match recurrence {
        Recurrence::Daily => date + Duration::days(1),
        Recurrence::Weekly => date + Duration::days(7),
        Recurrence::Biweekly => date + Duration::days(14),
        // checked_add_months clamps (Jan 31 -> Feb 28/29) and only fails far
        // outside any plannable date range.
        Recurrence::Monthly => date
            .checked_add_months(chrono::Months::new(1))
            .unwrap_or(date),
    }
}

/// Shifts the start date by the recurrence step. For monthly recurrences this
/// is the same calendar day (clamped to month-end), matching `shift_date`.
pub(crate) fn shifted_start_date(
    start_date: Option<NaiveDate>,
    recurrence: Recurrence,
) -> Option<NaiveDate> {
    start_date.map(|d| shift_date(d, recurrence))
}

/// Shifts the due date so the task duration is preserved. For fixed-step
/// recurrences the due date moves by the same step as the start date. For
/// monthly recurrences the offset in days between start and due is added to
/// the shifted start date, so a task that spans e.g. Jan 1 – Jan 31 still
/// spans 31 days in the next instance.
pub(crate) fn shifted_due_date(
    start_date: Option<NaiveDate>,
    due_date: Option<NaiveDate>,
    recurrence: Recurrence,
) -> Option<NaiveDate> {
    match (start_date, due_date, recurrence) {
        (Some(start), Some(due), Recurrence::Monthly) => {
            let duration_days = due.signed_duration_since(start).num_days();
            let new_start = shift_date(start, recurrence);
            Some(new_start + Duration::days(duration_days))
        }
        (Some(_), Some(due), _) => Some(shift_date(due, recurrence)),
        (None, Some(due), _) => Some(shift_date(due, recurrence)),
        (_, None, _) => None,
    }
}
