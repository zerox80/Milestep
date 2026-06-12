use crate::*;

pub(crate) async fn upload_attachment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    mut multipart: Multipart,
) -> Result<Json<TaskDto>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    let task_id = uuid_from_str(&id)?;
    let workspace_id = assert_task_edit(&state.db, user_id, task_id).await?;

    // Files written to disk so far; removed again if anything in the request fails,
    // so a rolled-back transaction never leaves orphaned files behind.
    let mut written_paths: Vec<PathBuf> = Vec::new();
    let result = store_attachments(
        &state,
        &mut multipart,
        task_id,
        user_id,
        workspace_id,
        &mut written_paths,
    )
    .await;
    if let Err(err) = result {
        for path in &written_paths {
            let _ = fs::remove_file(path).await;
        }
        return Err(err);
    }
    notify_workspace(&state, &ctx, &headers, workspace_id, "attachment");
    Ok(Json(fetch_task(&state.db, task_id).await?))
}

pub(crate) async fn store_attachments(
    state: &AppState,
    multipart: &mut Multipart,
    task_id: Uuid,
    user_id: Uuid,
    workspace_id: Uuid,
    written_paths: &mut Vec<PathBuf>,
) -> Result<(), AppError> {
    let mut tx = state.db.begin().await?;
    // Serializes uploads per workspace so concurrent requests cannot jointly
    // exceed the storage quota checked below.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(workspace_id.to_string())
        .execute(&mut *tx)
        .await?;
    let (used_bytes,): (i64,) = sqlx::query_as(
        // SUM(bigint) yields NUMERIC in Postgres; cast back so it decodes as i64.
        "SELECT COALESCE(SUM(a.size_bytes), 0)::BIGINT \
         FROM attachments a \
         JOIN tasks t ON t.id = a.task_id \
         JOIN projects p ON p.id = t.project_id \
         WHERE p.workspace_id = $1",
    )
    .bind(workspace_id)
    .fetch_one(&mut *tx)
    .await?;
    let mut remaining = (state.cfg.max_workspace_storage_bytes - used_bytes).max(0) as u64;
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?
    {
        let Some(file_name) = field.file_name().map(sanitize_file_name) else {
            continue;
        };
        if !allowed_upload_extension(&file_name) {
            return Err(AppError::BadRequest(format!(
                "file type of \"{file_name}\" is not allowed"
            )));
        }

        let attachment_id = Uuid::new_v4();
        let storage_name = format!("{attachment_id}-{file_name}");
        let storage_path = state.cfg.upload_dir.join(&storage_name);
        let mut file = fs::File::create(&storage_path).await?;
        written_paths.push(storage_path.clone());
        let mut size_bytes: u64 = 0;
        // First bytes of the file, for the magic-number check below.
        let mut head: Vec<u8> = Vec::with_capacity(16);
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|e| AppError::BadRequest(e.to_string()))?
        {
            size_bytes += chunk.len() as u64;
            if size_bytes > remaining {
                return Err(AppError::BadRequest(
                    "workspace storage limit exceeded".into(),
                ));
            }
            if head.len() < 16 {
                head.extend_from_slice(&chunk[..chunk.len().min(16 - head.len())]);
            }
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        drop(file);
        if size_bytes == 0 {
            let _ = fs::remove_file(&storage_path).await;
            written_paths.pop();
            continue;
        }
        // Inline-previewable types are later served with an image/PDF MIME
        // and Content-Disposition: inline; require the content to actually be
        // that format so no foreign payload gets rendered inline.
        if !magic_matches(&file_name, &head) {
            return Err(AppError::BadRequest(format!(
                "file content of \"{file_name}\" does not match its extension"
            )));
        }
        remaining -= size_bytes;

        let kind = if mime_guess::from_path(&file_name)
            .first_or_octet_stream()
            .type_()
            == mime_guess::mime::IMAGE
        {
            AttachmentKind::Image
        } else {
            AttachmentKind::File
        };

        sqlx::query(
            "INSERT INTO attachments (id, task_id, file_name, kind, size_bytes, storage_path, created_by) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(attachment_id)
        .bind(task_id)
        .bind(&file_name)
        .bind(attachment_kind_to_db(&kind))
        .bind(size_bytes as i64)
        .bind(storage_path.to_string_lossy().to_string())
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    }

    touch_task(&mut *tx, task_id).await?;
    record_audit(
        &mut *tx,
        workspace_id,
        user_id,
        "uploaded attachment",
        "task",
        Some(task_id),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

#[derive(Debug, Deserialize)]
pub(crate) struct InlineQuery {
    #[serde(default)]
    pub(crate) inline: Option<String>,
}

pub(crate) async fn download_attachment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<InlineQuery>,
) -> Result<Response, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    let attachment_id = uuid_from_str(&id)?;

    let row: Option<(Uuid, String, String)> =
        sqlx::query_as("SELECT task_id, file_name, storage_path FROM attachments WHERE id = $1")
            .bind(attachment_id)
            .fetch_optional(&state.db)
            .await?;
    let Some((task_id, file_name, storage_path)) = row else {
        return Err(AppError::NotFound);
    };
    assert_task_read(&state.db, user_id, task_id).await?;

    // Containment check: even if an insert path ever regresses, a stored path
    // outside the upload directory must never be served.
    let canonical = fs::canonicalize(&storage_path)
        .await
        .map_err(|_| AppError::NotFound)?;
    let upload_root = fs::canonicalize(&state.cfg.upload_dir)
        .await
        .map_err(|_| AppError::NotFound)?;
    if !canonical.starts_with(&upload_root) {
        tracing::error!(%storage_path, "attachment path escapes the upload directory");
        return Err(AppError::NotFound);
    }

    let file = fs::File::open(&canonical)
        .await
        .map_err(|_| AppError::NotFound)?;
    let mime = mime_guess::from_path(&file_name).first_or_octet_stream();

    let mut res = Body::from_stream(ReaderStream::new(file)).into_response();
    res.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_str(mime.as_ref())
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    let inline = query.inline.as_deref() == Some("1") && inline_previewable(&file_name);
    let disposition = if inline { "inline" } else { "attachment" };
    // file_name is sanitized to ASCII [A-Za-z0-9._-] on upload, so this is header-safe.
    res.headers_mut().insert(
        CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("{disposition}; filename=\"{file_name}\""))
            .map_err(|_| AppError::NotFound)?,
    );
    if inline {
        // Allow same-origin framing (PDF preview iframe) but lock the served
        // document down; security_headers leaves these handler-set values
        // alone. No `sandbox` directive: Chromium disables its built-in PDF
        // viewer inside CSP-sandboxed documents, which would blank the
        // preview. The whitelist is raster images + PDF, so default-src
        // 'none' already forbids script/embeds.
        res.headers_mut()
            .insert(X_FRAME_OPTIONS, HeaderValue::from_static("SAMEORIGIN"));
        res.headers_mut().insert(
            CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("default-src 'none'; frame-ancestors 'self'"),
        );
    }
    res.headers_mut()
        .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    Ok(res)
}

pub(crate) async fn delete_attachment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<TaskDto>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    let attachment_id = uuid_from_str(&id)?;

    let row: Option<(Uuid, String)> =
        sqlx::query_as("SELECT task_id, storage_path FROM attachments WHERE id = $1")
            .bind(attachment_id)
            .fetch_optional(&state.db)
            .await?;
    let Some((task_id, storage_path)) = row else {
        return Err(AppError::NotFound);
    };
    let workspace_id = assert_task_edit(&state.db, user_id, task_id).await?;

    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM attachments WHERE id = $1")
        .bind(attachment_id)
        .execute(&mut *tx)
        .await?;
    touch_task(&mut *tx, task_id).await?;
    record_audit(
        &mut *tx,
        workspace_id,
        user_id,
        "deleted attachment",
        "task",
        Some(task_id),
    )
    .await?;
    tx.commit().await?;

    // The row is gone; a leftover file only wastes space, so log and move on.
    if let Err(err) = fs::remove_file(&storage_path).await {
        tracing::warn!(%storage_path, %err, "could not remove deleted attachment file");
    }

    notify_workspace(&state, &ctx, &headers, workspace_id, "attachment");
    Ok(Json(fetch_task(&state.db, task_id).await?))
}

pub(crate) fn inline_previewable(file_name: &str) -> bool {
    FsPath::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .is_some_and(|e| INLINE_PREVIEW_EXTENSIONS.contains(&e.as_str()))
}

pub(crate) fn size_label(bytes: i64) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else {
        format!("{} KB", (bytes as f64 / 1_000.0).round() as i64)
    }
}

pub(crate) fn allowed_upload_extension(file_name: &str) -> bool {
    FsPath::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .is_some_and(|e| ALLOWED_UPLOAD_EXTENSIONS.contains(&e.as_str()))
}

/// For extensions that may be served inline (see `INLINE_PREVIEW_EXTENSIONS`)
/// the file content must carry the matching magic number. Every other
/// extension passes: those files are always served as downloads.
pub(crate) fn magic_matches(file_name: &str, head: &[u8]) -> bool {
    let Some(ext) = FsPath::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
    else {
        return true;
    };
    match ext.as_str() {
        "pdf" => head.starts_with(b"%PDF"),
        "png" => head.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]),
        "jpg" | "jpeg" => head.starts_with(&[0xFF, 0xD8, 0xFF]),
        "webp" => head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WEBP",
        _ => true,
    }
}

pub(crate) fn sanitize_file_name(name: &str) -> String {
    FsPath::new(name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("upload.bin")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect()
}
