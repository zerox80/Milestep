use crate::*;

pub(crate) async fn seed_demo(db: &PgPool, upload_dir: &FsPath) -> Result<(), AppError> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(db)
        .await?;
    if count.0 > 0 {
        return Ok(());
    }

    let workspace_id = fixed_uuid("10000000-0000-4000-8000-000000000001")?;
    let project_id = fixed_uuid("10000000-0000-4000-8000-000000000002")?;
    let status_ids = [
        fixed_uuid("10000000-0000-4000-8000-000000000101")?,
        fixed_uuid("10000000-0000-4000-8000-000000000102")?,
        fixed_uuid("10000000-0000-4000-8000-000000000103")?,
        fixed_uuid("10000000-0000-4000-8000-000000000104")?,
    ];

    let people = [
        (
            fixed_uuid("20000000-0000-4000-8000-000000000001")?,
            "alex@firma.com",
            "Alex Lindner",
            Role::Owner,
        ),
        (
            fixed_uuid("20000000-0000-4000-8000-000000000002")?,
            "anna@firma.com",
            "Anna Krause",
            Role::Admin,
        ),
        (
            fixed_uuid("20000000-0000-4000-8000-000000000003")?,
            "mira@firma.com",
            "Mira Roth",
            Role::Member,
        ),
        (
            fixed_uuid("20000000-0000-4000-8000-000000000004")?,
            "jonas@firma.com",
            "Jonas Schmidt",
            Role::Member,
        ),
        (
            fixed_uuid("20000000-0000-4000-8000-000000000005")?,
            "tom@firma.com",
            "Tom Lang",
            Role::Member,
        ),
        (
            fixed_uuid("20000000-0000-4000-8000-000000000006")?,
            "sara@firma.com",
            "Sara Bauer",
            Role::Member,
        ),
        (
            fixed_uuid("20000000-0000-4000-8000-000000000007")?,
            "david@firma.com",
            "David König",
            Role::Viewer,
        ),
    ];

    for (id, email, name, _) in &people {
        sqlx::query("INSERT INTO users (id, email, name, password_hash) VALUES ($1, $2, $3, $4)")
            .bind(*id)
            .bind(*email)
            .bind(*name)
            .bind(hash_password("password123")?)
            .execute(db)
            .await?;
    }

    sqlx::query("INSERT INTO workspaces (id, name, url_slug, default_lang) VALUES ($1, 'KoWoBau Demo', 'kowobau-demo', 'de')")
        .bind(workspace_id)
        .execute(db)
        .await?;
    sqlx::query("INSERT INTO projects (id, workspace_id, name, key) VALUES ($1, $2, 'Wohnquartier Nord', 'KWB')")
        .bind(project_id)
        .bind(workspace_id)
        .execute(db)
        .await?;

    let last_active = [
        "now()",
        "now() - interval '25 minutes'",
        "now() - interval '1 hour'",
        "now() - interval '3 hours'",
        "now() - interval '8 minutes'",
        "now() - interval '1 day'",
        "now() - interval '6 days'",
    ];
    for (idx, (user_id, _, _, role)) in people.into_iter().enumerate() {
        let sql = format!(
            "INSERT INTO memberships (id, workspace_id, user_id, role, status, last_active_at) \
             VALUES ($1, $2, $3, $4, 'active', {})",
            last_active[idx]
        );
        sqlx::query(&sql)
            .bind(Uuid::new_v4())
            .bind(workspace_id)
            .bind(user_id)
            .bind(role_to_db(&role))
            .execute(db)
            .await?;
    }

    let statuses = [
        ("Geplant", "Planned", "#8c867b", false),
        ("In Arbeit", "In progress", "#6b8aa6", false),
        ("Review", "Review", "#c98a3a", false),
        ("Fertig", "Done", "#5f8d6a", true),
    ];
    for (idx, (de, en, color, is_done)) in statuses.into_iter().enumerate() {
        sqlx::query(
            "INSERT INTO project_statuses (id, project_id, name_de, name_en, position, color, is_done) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(status_ids[idx])
        .bind(project_id)
        .bind(de)
        .bind(en)
        .bind(idx as i32)
        .bind(color)
        .bind(is_done)
        .execute(db)
        .await?;
    }

    let today = Utc::now().date_naive();
    let tasks = seed_tasks(project_id, status_ids);
    let mut task_ids = HashMap::new();
    for task in &tasks {
        task_ids.insert(task.key, task.id);
        sqlx::query(
            "INSERT INTO tasks \
             (id, project_id, key, title, title_en, description, description_en, tag, tag_color, priority, status_id, start_date, due_date, phase, created_by, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, now() - interval '3 days', now() - interval '25 minutes')",
        )
        .bind(task.id)
        .bind(project_id)
        .bind(task.key)
        .bind(task.title)
        .bind(task.title_en)
        .bind(task.description)
        .bind(task.description_en)
        .bind(task.tag)
        .bind(task.tag_color)
        .bind(priority_to_db(&task.priority))
        .bind(task.status_id)
        .bind(today + Duration::days(task.start_offset))
        .bind(today + Duration::days(task.due_offset))
        .bind(task.phase)
        .bind(people_by_initial(task.assignees[0])?)
        .execute(db)
        .await?;

        for initials in task.assignees {
            sqlx::query("INSERT INTO task_assignees (task_id, user_id) VALUES ($1, $2)")
                .bind(task.id)
                .bind(people_by_initial(initials)?)
                .execute(db)
                .await?;
        }
        for (idx, (title, title_en, done)) in task.subtasks.iter().enumerate() {
            sqlx::query(
                "INSERT INTO subtasks (id, task_id, title, title_en, done, position) VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(Uuid::new_v4())
            .bind(task.id)
            .bind(title)
            .bind(title_en)
            .bind(done)
            .bind(idx as i32)
            .execute(db)
            .await?;
        }
    }

    let dependencies = [
        ("KWB-104", "KWB-101"),
        ("KWB-107", "KWB-104"),
        ("KWB-103", "KWB-102"),
        ("KWB-105", "KWB-101"),
        ("KWB-106", "KWB-102"),
        ("KWB-108", "KWB-106"),
        ("KWB-110", "KWB-104"),
        ("KWB-110", "KWB-107"),
    ];
    for (task, dep) in dependencies {
        sqlx::query("INSERT INTO task_dependencies (task_id, depends_on_task_id) VALUES ($1, $2)")
            .bind(task_ids[task])
            .bind(task_ids[dep])
            .execute(db)
            .await?;
    }

    let comments = [
        (
            "KWB-104",
            "TL",
            "Fotodokumentation ist vollständig, die offenen Punkte sind markiert.",
        ),
        (
            "KWB-104",
            "JS",
            "Bitte die Nachfrist für Elektro mit der Bauleitung abstimmen.",
        ),
        (
            "KWB-107",
            "MR",
            "Der Terminplan enthält jetzt die neuen Lieferzeiten für Fenster.",
        ),
        ("KWB-101", "JS", "Die Brandschutzfreigabe ist abgelegt."),
    ];
    for (task_key, who, body) in comments {
        sqlx::query(
            "INSERT INTO comments (id, task_id, user_id, body, created_at) \
             VALUES ($1, $2, $3, $4, now() - interval '40 minutes')",
        )
        .bind(Uuid::new_v4())
        .bind(task_ids[task_key])
        .bind(people_by_initial(who)?)
        .bind(body)
        .execute(db)
        .await?;
    }
    // Keep the denormalized counter in sync with the comments actually seeded.
    sqlx::query(
        "UPDATE tasks SET comments_count = \
         (SELECT COUNT(*) FROM comments c WHERE c.task_id = tasks.id) WHERE project_id = $1",
    )
    .bind(project_id)
    .execute(db)
    .await?;

    seed_attachment(db, upload_dir, task_ids["KWB-104"], "maengelprotokoll.pdf").await?;
    seed_attachment(db, upload_dir, task_ids["KWB-104"], "fotoanhang-liste.json").await?;
    seed_attachment(db, upload_dir, task_ids["KWB-107"], "terminplan.png").await?;
    seed_attachment(
        db,
        upload_dir,
        task_ids["KWB-108"],
        "abnahme-checkliste.csv",
    )
    .await?;

    let milestones = [
        ("Planungsfreigabe", "Planning approval", -6, true, "planung"),
        (
            "Gewerke koordiniert",
            "Trades coordinated",
            1,
            false,
            "ausfuehrung",
        ),
        (
            "Abnahme Bauabschnitt A",
            "Construction phase A handover",
            7,
            false,
            "abnahme",
        ),
    ];
    for (title, title_en, due_offset, done, phase) in milestones {
        sqlx::query(
            "INSERT INTO milestones (id, project_id, title, title_en, due_date, done, phase) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(Uuid::new_v4())
        .bind(project_id)
        .bind(title)
        .bind(title_en)
        .bind(today + Duration::days(due_offset))
        .bind(done)
        .bind(phase)
        .execute(db)
        .await?;
    }

    let alex = people_by_initial("AL")?;
    let notifs = [
        (
            NotificationKind::Assigned,
            Some("TL"),
            Some("KWB-104"),
            Some("hat dir eine Aufgabe zugewiesen"),
            Some("assigned you a task"),
            true,
            "8 minutes",
        ),
        (
            NotificationKind::Mention,
            Some("MR"),
            Some("KWB-107"),
            Some("hat dich in einem Kommentar erwähnt"),
            Some("mentioned you in a comment"),
            true,
            "25 minutes",
        ),
        (
            NotificationKind::Due,
            None,
            Some("KWB-104"),
            Some("ist heute fällig"),
            Some("is due today"),
            true,
            "1 hour",
        ),
        (
            NotificationKind::Comment,
            Some("JS"),
            Some("KWB-101"),
            Some("hat kommentiert"),
            Some("commented"),
            false,
            "3 hours",
        ),
        (
            NotificationKind::Done,
            Some("SB"),
            Some("KWB-108"),
            Some("hat eine Aufgabe abgeschlossen"),
            Some("completed a task"),
            false,
            "1 day",
        ),
    ];
    for (kind, actor, task_key, text, text_en, unread, age) in notifs {
        let sql = format!(
            "INSERT INTO notifications \
             (id, workspace_id, user_id, kind, actor_id, task_id, text, text_en, unread, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, now() - interval '{age}')"
        );
        sqlx::query(&sql)
            .bind(Uuid::new_v4())
            .bind(workspace_id)
            .bind(alex)
            .bind(notification_kind_to_db(&kind))
            .bind(actor.map(people_by_initial).transpose()?)
            .bind(task_key.map(|key| task_ids[key]))
            .bind(text)
            .bind(text_en)
            .bind(unread)
            .execute(db)
            .await?;
    }

    for (action, entity, actor) in [
        ("completed task", "task", "SB"),
        ("commented", "task", "TL"),
        ("moved task", "task", "AK"),
        ("created task", "task", "MR"),
    ] {
        sqlx::query(
            "INSERT INTO audit_events (id, workspace_id, actor_id, action, entity, metadata, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, now() - interval '2 hours')",
        )
        .bind(Uuid::new_v4())
        .bind(workspace_id)
        .bind(people_by_initial(actor)?)
        .bind(action)
        .bind(entity)
        .bind(json!({}))
        .execute(db)
        .await?;
    }

    Ok(())
}

pub(crate) async fn insert_default_statuses(
    conn: &mut PgConnection,
    project_id: Uuid,
) -> Result<(), AppError> {
    for (idx, (de, en, color, is_done)) in [
        ("Geplant", "Planned", "#8c867b", false),
        ("In Arbeit", "In progress", "#6b8aa6", false),
        ("Review", "Review", "#c98a3a", false),
        ("Fertig", "Done", "#5f8d6a", true),
    ]
    .into_iter()
    .enumerate()
    {
        sqlx::query(
            "INSERT INTO project_statuses (id, project_id, name_de, name_en, position, color, is_done) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(Uuid::new_v4())
        .bind(project_id)
        .bind(de)
        .bind(en)
        .bind(idx as i32)
        .bind(color)
        .bind(is_done)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

pub(crate) async fn seed_attachment(
    db: &PgPool,
    upload_dir: &FsPath,
    task_id: Uuid,
    file_name: &str,
) -> Result<(), AppError> {
    let seed_dir = upload_dir.join("seed");
    fs::create_dir_all(&seed_dir).await?;
    let storage_path = seed_dir.join(file_name);
    let content = seed_attachment_bytes(file_name);
    fs::write(&storage_path, &content).await?;

    let ext = FsPath::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    let kind = if matches!(ext.as_deref(), Some("png" | "jpg")) {
        "image"
    } else {
        "file"
    };
    sqlx::query(
        "INSERT INTO attachments (id, task_id, file_name, kind, size_bytes, storage_path) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(Uuid::new_v4())
    .bind(task_id)
    .bind(file_name)
    .bind(kind)
    .bind(content.len() as i64)
    .bind(storage_path.to_string_lossy().to_string())
    .execute(db)
    .await?;
    Ok(())
}

pub(crate) fn seed_attachment_bytes(file_name: &str) -> Vec<u8> {
    match FsPath::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("pdf") => b"%PDF-1.4
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 300 160] /Contents 4 0 R >> endobj
4 0 obj << /Length 47 >> stream
BT /F1 12 Tf 32 90 Td (Demo attachment) Tj ET
endstream endobj
trailer << /Root 1 0 R >>
%%EOF
"
        .to_vec(),
        Some("png") => vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D,
            0xB0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ],
        Some("json") => br#"{"demo":true,"description":"Seeded KoWoBau attachment"}"#.to_vec(),
        Some("csv") => b"item,status\nAbnahme,offen\nDokumentation,bereit\n".to_vec(),
        _ => format!("Demo attachment: {file_name}\n").into_bytes(),
    }
}
