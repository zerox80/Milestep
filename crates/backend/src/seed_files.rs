use crate::*;

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
