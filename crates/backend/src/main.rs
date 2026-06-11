use std::{
    collections::HashMap,
    env,
    net::TcpStream,
    path::{Path as FsPath, PathBuf},
    time::Duration as StdDuration,
};

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{Multipart, Path, State},
    http::{
        header::{COOKIE, SET_COOKIE},
        HeaderMap, HeaderValue, StatusCode,
    },
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use hmac::{Hmac, Mac};
use kowobau_shared::*;
use rand_core::OsRng;
use serde::Serialize;
use serde_json::json;
use sha2::Sha256;
use sqlx::{postgres::PgPoolOptions, FromRow, PgPool};
use tokio::{fs, net::TcpListener};
use tower_http::{
    compression::CompressionLayer,
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

const COOKIE_NAME: &str = "kowobau_session";

#[derive(Debug, Clone)]
struct AppConfig {
    bind: String,
    static_dir: PathBuf,
    upload_dir: PathBuf,
    session_secret: String,
    cookie_secure: bool,
}

#[derive(Debug, Clone)]
struct AppState {
    db: PgPool,
    cfg: AppConfig,
}

#[derive(Debug, Clone)]
struct AuthContext {
    user: UserDto,
    session_id: Uuid,
}

#[derive(Debug, thiserror::Error)]
enum AppError {
    #[error("not authenticated")]
    Unauthorized,
    #[error("not allowed")]
    Forbidden,
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Conflict(String),
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Chrono(#[from] chrono::ParseError),
    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match self {
            AppError::Unauthorized => StatusCode::UNAUTHORIZED,
            AppError::Forbidden => StatusCode::FORBIDDEN,
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::Conflict(_) => StatusCode::CONFLICT,
            AppError::Sqlx(_) | AppError::Io(_) | AppError::Chrono(_) | AppError::Anyhow(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };

        let message = match status {
            StatusCode::INTERNAL_SERVER_ERROR => "internal server error".to_string(),
            _ => self.to_string(),
        };

        (status, Json(ApiErrorDto { error: message })).into_response()
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    if env::args().any(|arg| arg == "--healthcheck") {
        return healthcheck_cli();
    }

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "kowobau_backend=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cfg = AppConfig::from_env();
    fs::create_dir_all(&cfg.upload_dir).await?;

    let database_url = env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://kowobau:kowobau@localhost:5432/kowobau".to_string());
    let db = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(StdDuration::from_secs(10))
        .connect(&database_url)
        .await?;

    sqlx::migrate!("./migrations").run(&db).await?;
    seed_demo(&db).await?;

    let state = AppState { db, cfg };
    let app = build_router(state.clone());

    let listener = TcpListener::bind(&state.cfg.bind).await?;
    tracing::info!("KoWoBau-Planner listening on http://{}", state.cfg.bind);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

impl AppConfig {
    fn from_env() -> Self {
        Self {
            bind: env_var("KOWOBAU_BIND", "CADENCE_BIND")
                .unwrap_or_else(|| "127.0.0.1:8080".to_string()),
            static_dir: env_var("KOWOBAU_STATIC_DIR", "CADENCE_STATIC_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("crates/frontend/dist")),
            upload_dir: env_var("KOWOBAU_UPLOAD_DIR", "CADENCE_UPLOAD_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("crates/backend/uploads")),
            session_secret: env_var("KOWOBAU_SESSION_SECRET", "CADENCE_SESSION_SECRET")
                .unwrap_or_else(|| "dev-only-change-me".to_string()),
            cookie_secure: env_var("KOWOBAU_COOKIE_SECURE", "CADENCE_COOKIE_SECURE")
                .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE"))
                .unwrap_or(false),
        }
    }
}

fn env_var(primary: &str, fallback: &str) -> Option<String> {
    env::var(primary).ok().or_else(|| env::var(fallback).ok())
}

fn healthcheck_cli() -> anyhow::Result<()> {
    let bind =
        env_var("KOWOBAU_BIND", "CADENCE_BIND").unwrap_or_else(|| "127.0.0.1:8080".to_string());
    let port = bind
        .rsplit(':')
        .next()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(8080);
    TcpStream::connect(("127.0.0.1", port))?;
    Ok(())
}

fn build_router(state: AppState) -> Router {
    let api = Router::new()
        .route("/health", get(health))
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me))
        .route("/bootstrap", get(bootstrap))
        .route("/tasks", get(list_tasks).post(create_task))
        .route(
            "/tasks/{id}",
            get(get_task).patch(update_task).delete(delete_task),
        )
        .route("/tasks/{id}/move", post(move_task))
        .route("/tasks/{id}/subtasks", post(create_subtask))
        .route(
            "/tasks/{id}/subtasks/{subtask_id}",
            patch(update_subtask).delete(delete_subtask),
        )
        .route("/tasks/{id}/comments", post(create_comment))
        .route("/tasks/{id}/attachments", post(upload_attachment))
        .route("/notifications/{id}/read", post(read_notification))
        .route("/notifications/read-all", post(read_all_notifications))
        .route("/workspaces/{id}", patch(update_workspace))
        .route("/workspaces/{id}/invites", post(invite_member))
        .route("/memberships/{id}", patch(update_membership))
        .with_state(state.clone());

    let index = state.cfg.static_dir.join("index.html");
    let spa = ServeDir::new(&state.cfg.static_dir).not_found_service(ServeFile::new(index));

    Router::new()
        .nest("/api", api)
        .fallback_service(spa)
        .layer(CompressionLayer::new())
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "service": "kowobau-planner" }))
}

async fn register(
    State(state): State<AppState>,
    Json(payload): Json<RegisterRequest>,
) -> Result<Response, AppError> {
    let email = payload.email.trim().to_lowercase();
    if payload.name.trim().len() < 2 {
        return Err(AppError::BadRequest("name is too short".into()));
    }
    if !email.contains('@') {
        return Err(AppError::BadRequest("email is invalid".into()));
    }
    if payload.password.len() < 8 {
        return Err(AppError::BadRequest(
            "password must contain at least 8 characters".into(),
        ));
    }

    let existing: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM users WHERE email = $1")
        .bind(&email)
        .fetch_optional(&state.db)
        .await?;
    if existing.is_some() {
        return Err(AppError::Conflict("email is already registered".into()));
    }

    let user_id = Uuid::new_v4();
    let password_hash = hash_password(&payload.password)?;
    sqlx::query("INSERT INTO users (id, email, name, password_hash) VALUES ($1, $2, $3, $4)")
        .bind(user_id)
        .bind(&email)
        .bind(payload.name.trim())
        .bind(password_hash)
        .execute(&state.db)
        .await?;

    create_workspace_for_user(&state.db, user_id, payload.name.trim()).await?;
    audit_for_user(&state.db, user_id, "registered", "user", Some(user_id)).await?;

    let session_id = create_session(&state.db, user_id).await?;
    let user = fetch_user(&state.db, user_id).await?;
    json_with_cookie(&state, session_id, AuthResponse { user })
}

async fn login(
    State(state): State<AppState>,
    Json(payload): Json<AuthRequest>,
) -> Result<Response, AppError> {
    let email = payload.email.trim().to_lowercase();
    let row: Option<UserAuthRow> =
        sqlx::query_as("SELECT id, email, name, password_hash FROM users WHERE email = $1")
            .bind(&email)
            .fetch_optional(&state.db)
            .await?;
    let Some(row) = row else {
        return Err(AppError::Unauthorized);
    };

    verify_password(&payload.password, &row.password_hash)?;
    let session_id = create_session(&state.db, row.id).await?;

    json_with_cookie(
        &state,
        session_id,
        AuthResponse {
            user: UserDto {
                id: row.id.to_string(),
                email: row.email,
                name: row.name,
            },
        },
    )
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, AppError> {
    if let Ok(ctx) = require_auth(&state, &headers).await {
        sqlx::query("DELETE FROM sessions WHERE id = $1")
            .bind(ctx.session_id)
            .execute(&state.db)
            .await?;
    }

    let mut res = StatusCode::NO_CONTENT.into_response();
    res.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_str(&expired_cookie(&state.cfg)).expect("valid cookie"),
    );
    Ok(res)
}

async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<AuthResponse>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    Ok(Json(AuthResponse { user: ctx.user }))
}

async fn bootstrap(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<BootstrapDto>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let data = fetch_bootstrap(&state.db, uuid_from_str(&ctx.user.id)?).await?;
    Ok(Json(data))
}

async fn list_tasks(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<TaskDto>>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let bootstrap = fetch_bootstrap(&state.db, uuid_from_str(&ctx.user.id)?).await?;
    Ok(Json(bootstrap.tasks))
}

async fn get_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<TaskDto>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let task_id = uuid_from_str(&id)?;
    assert_task_access(&state.db, uuid_from_str(&ctx.user.id)?, task_id).await?;
    Ok(Json(fetch_task(&state.db, task_id).await?))
}

async fn create_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateTaskRequest>,
) -> Result<Json<TaskDto>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    let project_id = uuid_from_str(&payload.project_id)?;
    assert_project_access(&state.db, user_id, project_id).await?;

    if payload.title.trim().is_empty() {
        return Err(AppError::BadRequest("task title is required".into()));
    }

    let status_id = uuid_from_str(&payload.status_id)?;
    assert_status_in_project(&state.db, project_id, status_id).await?;
    let next: (i32,) = sqlx::query_as(
        "SELECT COALESCE(MAX(NULLIF(split_part(key, '-', 2), '')::INT), 100) + 1 \
         FROM tasks WHERE project_id = $1 AND key LIKE 'KWB-%'",
    )
    .bind(project_id)
    .fetch_one(&state.db)
    .await?;
    let key = format!("KWB-{}", next.0);

    let task_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO tasks \
         (id, project_id, key, title, description, tag, tag_color, priority, status_id, start_date, due_date, phase, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
    )
    .bind(task_id)
    .bind(project_id)
    .bind(&key)
    .bind(payload.title.trim())
    .bind(payload.description.trim())
    .bind(payload.tag.trim())
    .bind(payload.tag_color.trim())
    .bind(priority_to_db(&payload.priority))
    .bind(status_id)
    .bind(parse_optional_date(payload.start_date.as_deref())?)
    .bind(parse_optional_date(payload.due_date.as_deref())?)
    .bind(payload.phase.trim())
    .bind(user_id)
    .execute(&state.db)
    .await?;

    replace_assignees(&state.db, task_id, &payload.assignee_ids).await?;
    for (idx, title) in payload.subtasks.iter().enumerate() {
        if !title.trim().is_empty() {
            sqlx::query(
                "INSERT INTO subtasks (id, task_id, title, position) VALUES ($1, $2, $3, $4)",
            )
            .bind(Uuid::new_v4())
            .bind(task_id)
            .bind(title.trim())
            .bind(idx as i32)
            .execute(&state.db)
            .await?;
        }
    }

    audit_for_user(&state.db, user_id, "created task", "task", Some(task_id)).await?;
    Ok(Json(fetch_task(&state.db, task_id).await?))
}

async fn update_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<UpdateTaskRequest>,
) -> Result<Json<TaskDto>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    let task_id = uuid_from_str(&id)?;
    assert_task_access(&state.db, user_id, task_id).await?;

    if let Some(title) = payload.title {
        if title.trim().is_empty() {
            return Err(AppError::BadRequest("task title is required".into()));
        }
        sqlx::query("UPDATE tasks SET title = $1, updated_at = now() WHERE id = $2")
            .bind(title.trim())
            .bind(task_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(description) = payload.description {
        sqlx::query("UPDATE tasks SET description = $1, updated_at = now() WHERE id = $2")
            .bind(description.trim())
            .bind(task_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(tag) = payload.tag {
        sqlx::query("UPDATE tasks SET tag = $1, updated_at = now() WHERE id = $2")
            .bind(tag.trim())
            .bind(task_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(tag_color) = payload.tag_color {
        sqlx::query("UPDATE tasks SET tag_color = $1, updated_at = now() WHERE id = $2")
            .bind(tag_color.trim())
            .bind(task_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(priority) = payload.priority {
        sqlx::query("UPDATE tasks SET priority = $1, updated_at = now() WHERE id = $2")
            .bind(priority_to_db(&priority))
            .bind(task_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(status_id) = payload.status_id {
        let status_id = uuid_from_str(&status_id)?;
        let project_id: (Uuid,) = sqlx::query_as("SELECT project_id FROM tasks WHERE id = $1")
            .bind(task_id)
            .fetch_one(&state.db)
            .await?;
        assert_status_in_project(&state.db, project_id.0, status_id).await?;
        sqlx::query("UPDATE tasks SET status_id = $1, updated_at = now() WHERE id = $2")
            .bind(status_id)
            .bind(task_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(start_date) = payload.start_date {
        sqlx::query("UPDATE tasks SET start_date = $1, updated_at = now() WHERE id = $2")
            .bind(parse_optional_date(start_date.as_deref())?)
            .bind(task_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(due_date) = payload.due_date {
        sqlx::query("UPDATE tasks SET due_date = $1, updated_at = now() WHERE id = $2")
            .bind(parse_optional_date(due_date.as_deref())?)
            .bind(task_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(phase) = payload.phase {
        sqlx::query("UPDATE tasks SET phase = $1, updated_at = now() WHERE id = $2")
            .bind(phase.trim())
            .bind(task_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(assignee_ids) = payload.assignee_ids {
        replace_assignees(&state.db, task_id, &assignee_ids).await?;
    }

    audit_for_user(&state.db, user_id, "updated task", "task", Some(task_id)).await?;
    Ok(Json(fetch_task(&state.db, task_id).await?))
}

async fn move_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<MoveTaskRequest>,
) -> Result<Json<TaskDto>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    let task_id = uuid_from_str(&id)?;
    assert_task_access(&state.db, user_id, task_id).await?;
    let status_id = uuid_from_str(&payload.status_id)?;
    let project_id: (Uuid,) = sqlx::query_as("SELECT project_id FROM tasks WHERE id = $1")
        .bind(task_id)
        .fetch_one(&state.db)
        .await?;
    assert_status_in_project(&state.db, project_id.0, status_id).await?;

    sqlx::query("UPDATE tasks SET status_id = $1, updated_at = now() WHERE id = $2")
        .bind(status_id)
        .bind(task_id)
        .execute(&state.db)
        .await?;
    audit_for_user(&state.db, user_id, "moved task", "task", Some(task_id)).await?;
    Ok(Json(fetch_task(&state.db, task_id).await?))
}

async fn delete_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    let task_id = uuid_from_str(&id)?;
    assert_task_access(&state.db, user_id, task_id).await?;
    sqlx::query("DELETE FROM tasks WHERE id = $1")
        .bind(task_id)
        .execute(&state.db)
        .await?;
    audit_for_user(&state.db, user_id, "deleted task", "task", Some(task_id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn create_subtask(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<CreateSubtaskRequest>,
) -> Result<Json<TaskDto>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    let task_id = uuid_from_str(&id)?;
    assert_task_access(&state.db, user_id, task_id).await?;
    if payload.title.trim().is_empty() {
        return Err(AppError::BadRequest("subtask title is required".into()));
    }
    let pos: (i32,) =
        sqlx::query_as("SELECT COALESCE(MAX(position), -1) + 1 FROM subtasks WHERE task_id = $1")
            .bind(task_id)
            .fetch_one(&state.db)
            .await?;
    sqlx::query("INSERT INTO subtasks (id, task_id, title, position) VALUES ($1, $2, $3, $4)")
        .bind(Uuid::new_v4())
        .bind(task_id)
        .bind(payload.title.trim())
        .bind(pos.0)
        .execute(&state.db)
        .await?;
    touch_task(&state.db, task_id).await?;
    audit_for_user(
        &state.db,
        user_id,
        "created subtask",
        "subtask",
        Some(task_id),
    )
    .await?;
    Ok(Json(fetch_task(&state.db, task_id).await?))
}

async fn update_subtask(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, subtask_id)): Path<(String, String)>,
    Json(payload): Json<UpdateSubtaskRequest>,
) -> Result<Json<TaskDto>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    let task_id = uuid_from_str(&id)?;
    let subtask_id = uuid_from_str(&subtask_id)?;
    assert_task_access(&state.db, user_id, task_id).await?;

    if let Some(title) = payload.title {
        if title.trim().is_empty() {
            return Err(AppError::BadRequest("subtask title is required".into()));
        }
        sqlx::query("UPDATE subtasks SET title = $1 WHERE id = $2 AND task_id = $3")
            .bind(title.trim())
            .bind(subtask_id)
            .bind(task_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(done) = payload.done {
        sqlx::query("UPDATE subtasks SET done = $1 WHERE id = $2 AND task_id = $3")
            .bind(done)
            .bind(subtask_id)
            .bind(task_id)
            .execute(&state.db)
            .await?;
    }
    touch_task(&state.db, task_id).await?;
    audit_for_user(
        &state.db,
        user_id,
        "updated subtask",
        "subtask",
        Some(subtask_id),
    )
    .await?;
    Ok(Json(fetch_task(&state.db, task_id).await?))
}

async fn delete_subtask(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, subtask_id)): Path<(String, String)>,
) -> Result<Json<TaskDto>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    let task_id = uuid_from_str(&id)?;
    let subtask_id = uuid_from_str(&subtask_id)?;
    assert_task_access(&state.db, user_id, task_id).await?;
    sqlx::query("DELETE FROM subtasks WHERE id = $1 AND task_id = $2")
        .bind(subtask_id)
        .bind(task_id)
        .execute(&state.db)
        .await?;
    touch_task(&state.db, task_id).await?;
    Ok(Json(fetch_task(&state.db, task_id).await?))
}

async fn create_comment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<CreateCommentRequest>,
) -> Result<Json<TaskDto>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    let task_id = uuid_from_str(&id)?;
    assert_task_access(&state.db, user_id, task_id).await?;
    if payload.body.trim().is_empty() {
        return Err(AppError::BadRequest("comment body is required".into()));
    }
    sqlx::query("INSERT INTO comments (id, task_id, user_id, body) VALUES ($1, $2, $3, $4)")
        .bind(Uuid::new_v4())
        .bind(task_id)
        .bind(user_id)
        .bind(payload.body.trim())
        .execute(&state.db)
        .await?;
    sqlx::query(
        "UPDATE tasks SET comments_count = comments_count + 1, updated_at = now() WHERE id = $1",
    )
    .bind(task_id)
    .execute(&state.db)
    .await?;
    audit_for_user(&state.db, user_id, "commented", "task", Some(task_id)).await?;
    Ok(Json(fetch_task(&state.db, task_id).await?))
}

async fn upload_attachment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    mut multipart: Multipart,
) -> Result<Json<TaskDto>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    let task_id = uuid_from_str(&id)?;
    assert_task_access(&state.db, user_id, task_id).await?;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?
    {
        let Some(file_name) = field.file_name().map(sanitize_file_name) else {
            continue;
        };
        let bytes = field
            .bytes()
            .await
            .map_err(|e| AppError::BadRequest(e.to_string()))?;
        if bytes.is_empty() {
            continue;
        }
        let attachment_id = Uuid::new_v4();
        let storage_name = format!("{}-{}", attachment_id, file_name);
        let storage_path = state.cfg.upload_dir.join(&storage_name);
        fs::write(&storage_path, &bytes).await?;
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
        .bind(bytes.len() as i64)
        .bind(storage_path.to_string_lossy().to_string())
        .bind(user_id)
        .execute(&state.db)
        .await?;
    }

    touch_task(&state.db, task_id).await?;
    audit_for_user(
        &state.db,
        user_id,
        "uploaded attachment",
        "task",
        Some(task_id),
    )
    .await?;
    Ok(Json(fetch_task(&state.db, task_id).await?))
}

async fn read_notification(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    let notification_id = uuid_from_str(&id)?;
    sqlx::query("UPDATE notifications SET unread = false WHERE id = $1 AND user_id = $2")
        .bind(notification_id)
        .bind(user_id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn read_all_notifications(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    sqlx::query("UPDATE notifications SET unread = false WHERE user_id = $1")
        .bind(user_id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn update_workspace(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<UpdateWorkspaceRequest>,
) -> Result<Json<WorkspaceDto>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    let workspace_id = uuid_from_str(&id)?;
    assert_workspace_admin(&state.db, user_id, workspace_id).await?;
    if let Some(name) = payload.name {
        if name.trim().is_empty() {
            return Err(AppError::BadRequest("workspace name is required".into()));
        }
        sqlx::query("UPDATE workspaces SET name = $1 WHERE id = $2")
            .bind(name.trim())
            .bind(workspace_id)
            .execute(&state.db)
            .await?;
    }
    if let Some(lang) = payload.default_lang {
        if lang != "de" && lang != "en" {
            return Err(AppError::BadRequest(
                "default language must be de or en".into(),
            ));
        }
        sqlx::query("UPDATE workspaces SET default_lang = $1 WHERE id = $2")
            .bind(lang)
            .bind(workspace_id)
            .execute(&state.db)
            .await?;
    }
    audit_for_user(
        &state.db,
        user_id,
        "updated workspace",
        "workspace",
        Some(workspace_id),
    )
    .await?;
    Ok(Json(fetch_workspace(&state.db, workspace_id).await?))
}

async fn invite_member(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<InviteMemberRequest>,
) -> Result<StatusCode, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    let workspace_id = uuid_from_str(&id)?;
    assert_workspace_admin(&state.db, user_id, workspace_id).await?;
    let email = payload.email.trim().to_lowercase();
    if !email.contains('@') {
        return Err(AppError::BadRequest("invite email is invalid".into()));
    }
    sqlx::query(
        "INSERT INTO workspace_invites (id, workspace_id, email, role, invited_by) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::new_v4())
    .bind(workspace_id)
    .bind(email)
    .bind(role_to_db(&payload.role))
    .bind(user_id)
    .execute(&state.db)
    .await?;
    audit_for_user(
        &state.db,
        user_id,
        "invited member",
        "workspace",
        Some(workspace_id),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn update_membership(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<UpdateMembershipRequest>,
) -> Result<Json<MemberDto>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    let membership_id = uuid_from_str(&id)?;
    let row: MembershipWorkspaceRow =
        sqlx::query_as("SELECT workspace_id, user_id, role FROM memberships WHERE id = $1")
            .bind(membership_id)
            .fetch_optional(&state.db)
            .await?
            .ok_or(AppError::NotFound)?;
    assert_workspace_admin(&state.db, user_id, row.workspace_id).await?;
    if row.user_id == user_id && matches!(payload.role, Role::Viewer) {
        return Err(AppError::BadRequest(
            "cannot demote yourself to viewer".into(),
        ));
    }
    sqlx::query("UPDATE memberships SET role = $1 WHERE id = $2")
        .bind(role_to_db(&payload.role))
        .bind(membership_id)
        .execute(&state.db)
        .await?;
    audit_for_user(
        &state.db,
        user_id,
        "updated role",
        "membership",
        Some(membership_id),
    )
    .await?;
    Ok(Json(fetch_member(&state.db, membership_id).await?))
}

async fn require_auth(state: &AppState, headers: &HeaderMap) -> Result<AuthContext, AppError> {
    let session_id = parse_session_cookie(headers, &state.cfg)?;
    let row: Option<UserRow> = sqlx::query_as(
        "SELECT u.id, u.email, u.name \
         FROM sessions s JOIN users u ON u.id = s.user_id \
         WHERE s.id = $1 AND s.expires_at > now()",
    )
    .bind(session_id)
    .fetch_optional(&state.db)
    .await?;

    let Some(row) = row else {
        return Err(AppError::Unauthorized);
    };

    Ok(AuthContext {
        user: row.into(),
        session_id,
    })
}

fn parse_session_cookie(headers: &HeaderMap, cfg: &AppConfig) -> Result<Uuid, AppError> {
    let cookie = headers
        .get(COOKIE)
        .and_then(|value| value.to_str().ok())
        .ok_or(AppError::Unauthorized)?;

    let raw = cookie
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .find_map(|(name, value)| (name == COOKIE_NAME).then_some(value))
        .ok_or(AppError::Unauthorized)?;

    let (session_id, signature) = raw.rsplit_once('.').ok_or(AppError::Unauthorized)?;
    let expected = sign(cfg, session_id)?;
    if signature != expected {
        return Err(AppError::Unauthorized);
    }

    uuid_from_str(session_id)
}

fn sign(cfg: &AppConfig, value: &str) -> Result<String, AppError> {
    let mut mac = HmacSha256::new_from_slice(cfg.session_secret.as_bytes())
        .map_err(|_| AppError::BadRequest("invalid session secret".into()))?;
    mac.update(value.as_bytes());
    Ok(URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
}

fn build_cookie(cfg: &AppConfig, session_id: Uuid) -> Result<String, AppError> {
    let id = session_id.to_string();
    let signed = format!("{}.{}", id, sign(cfg, &id)?);
    let secure = if cfg.cookie_secure { "; Secure" } else { "" };
    Ok(format!(
        "{COOKIE_NAME}={signed}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{}",
        14 * 24 * 60 * 60,
        secure
    ))
}

fn expired_cookie(cfg: &AppConfig) -> String {
    let secure = if cfg.cookie_secure { "; Secure" } else { "" };
    format!("{COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure}")
}

fn json_with_cookie<T: Serialize>(
    state: &AppState,
    session_id: Uuid,
    payload: T,
) -> Result<Response, AppError> {
    let mut res = Json(payload).into_response();
    res.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_str(&build_cookie(&state.cfg, session_id)?).expect("valid cookie"),
    );
    Ok(res)
}

fn hash_password(password: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| AppError::BadRequest(e.to_string()))?
        .to_string())
}

fn verify_password(password: &str, hash: &str) -> Result<(), AppError> {
    let parsed = PasswordHash::new(hash).map_err(|_| AppError::Unauthorized)?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .map_err(|_| AppError::Unauthorized)
}

async fn create_session(db: &PgPool, user_id: Uuid) -> Result<Uuid, AppError> {
    let session_id = Uuid::new_v4();
    let expires_at = Utc::now() + Duration::days(14);
    sqlx::query("INSERT INTO sessions (id, user_id, expires_at) VALUES ($1, $2, $3)")
        .bind(session_id)
        .bind(user_id)
        .bind(expires_at)
        .execute(db)
        .await?;
    Ok(session_id)
}

#[derive(Debug, FromRow)]
struct UserAuthRow {
    id: Uuid,
    email: String,
    name: String,
    password_hash: String,
}

#[derive(Debug, FromRow)]
struct UserRow {
    id: Uuid,
    email: String,
    name: String,
}

impl From<UserRow> for UserDto {
    fn from(row: UserRow) -> Self {
        Self {
            id: row.id.to_string(),
            email: row.email,
            name: row.name,
        }
    }
}

#[derive(Debug, FromRow)]
struct WorkspaceRow {
    id: Uuid,
    name: String,
    url_slug: String,
    default_lang: String,
}

impl From<WorkspaceRow> for WorkspaceDto {
    fn from(row: WorkspaceRow) -> Self {
        Self {
            id: row.id.to_string(),
            name: row.name,
            url_slug: row.url_slug,
            default_lang: row.default_lang,
        }
    }
}

#[derive(Debug, FromRow)]
struct ProjectRow {
    id: Uuid,
    workspace_id: Uuid,
    name: String,
    key: String,
}

impl From<ProjectRow> for ProjectDto {
    fn from(row: ProjectRow) -> Self {
        Self {
            id: row.id.to_string(),
            workspace_id: row.workspace_id.to_string(),
            name: row.name,
            key: row.key,
        }
    }
}

#[derive(Debug, FromRow)]
struct StatusRow {
    id: Uuid,
    project_id: Uuid,
    name_de: String,
    name_en: String,
    position: i32,
    color: String,
}

impl From<StatusRow> for StatusDto {
    fn from(row: StatusRow) -> Self {
        Self {
            id: row.id.to_string(),
            project_id: row.project_id.to_string(),
            name_de: row.name_de,
            name_en: row.name_en,
            position: row.position,
            color: row.color,
        }
    }
}

#[derive(Debug, FromRow)]
struct TaskRow {
    id: Uuid,
    project_id: Uuid,
    key: String,
    title: String,
    title_en: Option<String>,
    description: String,
    description_en: Option<String>,
    tag: String,
    tag_color: String,
    priority: String,
    status_id: Uuid,
    status_position: i32,
    start_date: Option<NaiveDate>,
    due_date: Option<NaiveDate>,
    phase: String,
    comments_count: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct SubtaskRow {
    id: Uuid,
    title: String,
    title_en: Option<String>,
    done: bool,
    position: i32,
}

#[derive(Debug, FromRow)]
struct CommentRow {
    id: Uuid,
    task_id: Uuid,
    user_id: Uuid,
    author_name: String,
    body: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct AttachmentRow {
    id: Uuid,
    task_id: Uuid,
    file_name: String,
    kind: String,
    size_bytes: i64,
}

#[derive(Debug, FromRow)]
struct MembershipWorkspaceRow {
    workspace_id: Uuid,
    user_id: Uuid,
    role: String,
}

#[derive(Debug, FromRow)]
struct MemberRow {
    id: Uuid,
    user_id: Uuid,
    workspace_id: Uuid,
    name: String,
    email: String,
    role: String,
    status: String,
    last_active_at: Option<DateTime<Utc>>,
}

#[derive(Debug, FromRow)]
struct MilestoneRow {
    id: Uuid,
    project_id: Uuid,
    title: String,
    title_en: Option<String>,
    due_date: NaiveDate,
    done: bool,
    phase: String,
}

#[derive(Debug, FromRow)]
struct NotificationRow {
    id: Uuid,
    kind: String,
    actor_id: Option<Uuid>,
    actor_name: Option<String>,
    task_id: Option<Uuid>,
    milestone_id: Option<Uuid>,
    text: Option<String>,
    text_en: Option<String>,
    unread: bool,
    created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct AuditRow {
    id: Uuid,
    actor_name: Option<String>,
    action: String,
    entity: String,
    created_at: DateTime<Utc>,
}

async fn fetch_user(db: &PgPool, id: Uuid) -> Result<UserDto, AppError> {
    let row: UserRow = sqlx::query_as("SELECT id, email, name FROM users WHERE id = $1")
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(row.into())
}

async fn fetch_workspace(db: &PgPool, id: Uuid) -> Result<WorkspaceDto, AppError> {
    let row: WorkspaceRow =
        sqlx::query_as("SELECT id, name, url_slug, default_lang FROM workspaces WHERE id = $1")
            .bind(id)
            .fetch_optional(db)
            .await?
            .ok_or(AppError::NotFound)?;
    Ok(row.into())
}

async fn fetch_bootstrap(db: &PgPool, user_id: Uuid) -> Result<BootstrapDto, AppError> {
    sqlx::query("UPDATE memberships SET last_active_at = now() WHERE user_id = $1")
        .bind(user_id)
        .execute(db)
        .await?;

    let user = fetch_user(db, user_id).await?;
    let membership: MembershipWorkspaceRow = sqlx::query_as(
        "SELECT workspace_id, user_id, role \
         FROM memberships WHERE user_id = $1 AND status = 'active' ORDER BY created_at ASC LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)?;

    let workspace = fetch_workspace(db, membership.workspace_id).await?;
    let project_row: ProjectRow = sqlx::query_as(
        "SELECT id, workspace_id, name, key FROM projects WHERE workspace_id = $1 ORDER BY created_at ASC LIMIT 1",
    )
    .bind(membership.workspace_id)
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)?;
    let project: ProjectDto = project_row.into();
    let project_id = uuid_from_str(&project.id)?;

    let statuses = fetch_statuses(db, project_id).await?;
    let members = fetch_members(db, membership.workspace_id).await?;
    let tasks = fetch_tasks(db, project_id).await?;
    let milestones = fetch_milestones(db, project_id).await?;
    let notifications = fetch_notifications(db, user_id).await?;
    let audit_events = fetch_audit_events(db, membership.workspace_id).await?;

    Ok(BootstrapDto {
        current_user: user,
        workspace,
        project,
        current_role: role_from_db(&membership.role)?,
        members,
        statuses,
        tasks,
        milestones,
        notifications,
        audit_events,
    })
}

async fn fetch_statuses(db: &PgPool, project_id: Uuid) -> Result<Vec<StatusDto>, AppError> {
    let rows: Vec<StatusRow> = sqlx::query_as(
        "SELECT id, project_id, name_de, name_en, position, color \
         FROM project_statuses WHERE project_id = $1 ORDER BY position",
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

async fn fetch_members(db: &PgPool, workspace_id: Uuid) -> Result<Vec<MemberDto>, AppError> {
    let rows: Vec<MemberRow> = sqlx::query_as(
        "SELECT m.id, m.user_id, m.workspace_id, u.name, u.email, m.role, m.status, m.last_active_at \
         FROM memberships m JOIN users u ON u.id = m.user_id \
         WHERE m.workspace_id = $1 ORDER BY u.name",
    )
    .bind(workspace_id)
    .fetch_all(db)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let open_tasks: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM task_assignees ta \
             JOIN tasks t ON t.id = ta.task_id \
             JOIN projects p ON p.id = t.project_id \
             JOIN project_statuses s ON s.id = t.status_id \
             WHERE p.workspace_id = $1 AND ta.user_id = $2 AND s.position < 3",
        )
        .bind(workspace_id)
        .bind(row.user_id)
        .fetch_one(db)
        .await?;
        let done_tasks: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM task_assignees ta \
             JOIN tasks t ON t.id = ta.task_id \
             JOIN projects p ON p.id = t.project_id \
             JOIN project_statuses s ON s.id = t.status_id \
             WHERE p.workspace_id = $1 AND ta.user_id = $2 AND s.position = 3",
        )
        .bind(workspace_id)
        .bind(row.user_id)
        .fetch_one(db)
        .await?;

        out.push(MemberDto {
            id: row.id.to_string(),
            user_id: row.user_id.to_string(),
            workspace_id: row.workspace_id.to_string(),
            initials: initials(&row.name),
            name: row.name,
            email: row.email,
            role: role_from_db(&row.role)?,
            status: member_status_from_db(&row.status)?,
            last_active_label_de: row
                .last_active_at
                .map(|t| relative_label(t, "de"))
                .unwrap_or_else(|| "nie".to_string()),
            last_active_label_en: row
                .last_active_at
                .map(|t| relative_label(t, "en"))
                .unwrap_or_else(|| "never".to_string()),
            open_tasks: open_tasks.0,
            done_tasks: done_tasks.0,
        });
    }
    Ok(out)
}

async fn fetch_member(db: &PgPool, membership_id: Uuid) -> Result<MemberDto, AppError> {
    let row: MemberRow = sqlx::query_as(
        "SELECT m.id, m.user_id, m.workspace_id, u.name, u.email, m.role, m.status, m.last_active_at \
         FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.id = $1",
    )
    .bind(membership_id)
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)?;
    let members = fetch_members(db, row.workspace_id).await?;
    members
        .into_iter()
        .find(|m| m.id == membership_id.to_string())
        .ok_or(AppError::NotFound)
}

async fn fetch_tasks(db: &PgPool, project_id: Uuid) -> Result<Vec<TaskDto>, AppError> {
    let ids: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT t.id FROM tasks t JOIN project_statuses s ON s.id = t.status_id \
         WHERE t.project_id = $1 ORDER BY s.position, t.due_date NULLS LAST, t.key",
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;

    let mut out = Vec::with_capacity(ids.len());
    for (id,) in ids {
        out.push(fetch_task(db, id).await?);
    }
    Ok(out)
}

async fn fetch_task(db: &PgPool, task_id: Uuid) -> Result<TaskDto, AppError> {
    let row: TaskRow = sqlx::query_as(
        "SELECT t.id, t.project_id, t.key, t.title, t.title_en, t.description, t.description_en, \
                t.tag, t.tag_color, t.priority, t.status_id, s.position AS status_position, \
                t.start_date, t.due_date, t.phase, t.comments_count, t.created_at, t.updated_at \
         FROM tasks t JOIN project_statuses s ON s.id = t.status_id WHERE t.id = $1",
    )
    .bind(task_id)
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)?;

    let assignees: Vec<(Uuid,)> =
        sqlx::query_as("SELECT user_id FROM task_assignees WHERE task_id = $1 ORDER BY user_id")
            .bind(task_id)
            .fetch_all(db)
            .await?;
    let dependencies: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT depends_on_task_id FROM task_dependencies WHERE task_id = $1 ORDER BY depends_on_task_id",
    )
    .bind(task_id)
    .fetch_all(db)
    .await?;
    let subtasks: Vec<SubtaskRow> = sqlx::query_as(
        "SELECT id, title, title_en, done, position FROM subtasks WHERE task_id = $1 ORDER BY position",
    )
    .bind(task_id)
    .fetch_all(db)
    .await?;
    let comments: Vec<CommentRow> = sqlx::query_as(
        "SELECT c.id, c.task_id, c.user_id, u.name AS author_name, c.body, c.created_at \
         FROM comments c JOIN users u ON u.id = c.user_id WHERE c.task_id = $1 ORDER BY c.created_at DESC",
    )
    .bind(task_id)
    .fetch_all(db)
    .await?;
    let attachments: Vec<AttachmentRow> = sqlx::query_as(
        "SELECT id, task_id, file_name, kind, size_bytes FROM attachments WHERE task_id = $1 ORDER BY created_at DESC",
    )
    .bind(task_id)
    .fetch_all(db)
    .await?;

    Ok(TaskDto {
        id: row.id.to_string(),
        project_id: row.project_id.to_string(),
        key: row.key,
        title: row.title,
        title_en: row.title_en,
        description: row.description,
        description_en: row.description_en,
        tag: row.tag,
        tag_color: row.tag_color,
        priority: priority_from_db(&row.priority)?,
        status_id: row.status_id.to_string(),
        status_position: row.status_position,
        start_date: row.start_date.map(|d| d.to_string()),
        due_date: row.due_date.map(|d| d.to_string()),
        phase: row.phase,
        assignee_ids: assignees.into_iter().map(|(id,)| id.to_string()).collect(),
        dependency_ids: dependencies
            .into_iter()
            .map(|(id,)| id.to_string())
            .collect(),
        subtasks: subtasks
            .into_iter()
            .map(|s| SubtaskDto {
                id: s.id.to_string(),
                title: s.title,
                title_en: s.title_en,
                done: s.done,
                position: s.position,
            })
            .collect(),
        comments: comments
            .into_iter()
            .map(|c| CommentDto {
                id: c.id.to_string(),
                task_id: c.task_id.to_string(),
                user_id: c.user_id.to_string(),
                author_initials: initials(&c.author_name),
                author_name: c.author_name,
                body: c.body,
                created_label_de: relative_label(c.created_at, "de"),
                created_label_en: relative_label(c.created_at, "en"),
            })
            .collect(),
        attachments: attachments
            .into_iter()
            .map(|a| AttachmentDto {
                id: a.id.to_string(),
                task_id: a.task_id.to_string(),
                file_name: a.file_name,
                kind: attachment_kind_from_db(&a.kind).unwrap_or(AttachmentKind::File),
                size_label: size_label(a.size_bytes),
            })
            .collect(),
        comments_count: row.comments_count,
        created_label_de: relative_label(row.created_at, "de"),
        created_label_en: relative_label(row.created_at, "en"),
        updated_label_de: relative_label(row.updated_at, "de"),
        updated_label_en: relative_label(row.updated_at, "en"),
    })
}

async fn fetch_milestones(db: &PgPool, project_id: Uuid) -> Result<Vec<MilestoneDto>, AppError> {
    let rows: Vec<MilestoneRow> = sqlx::query_as(
        "SELECT id, project_id, title, title_en, due_date, done, phase FROM milestones WHERE project_id = $1 ORDER BY due_date",
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|m| MilestoneDto {
            id: m.id.to_string(),
            project_id: m.project_id.to_string(),
            title: m.title,
            title_en: m.title_en,
            due_date: m.due_date.to_string(),
            done: m.done,
            phase: m.phase,
        })
        .collect())
}

async fn fetch_notifications(db: &PgPool, user_id: Uuid) -> Result<Vec<NotificationDto>, AppError> {
    let rows: Vec<NotificationRow> = sqlx::query_as(
        "SELECT n.id, n.kind, n.actor_id, u.name AS actor_name, n.task_id, n.milestone_id, \
                n.text, n.text_en, n.unread, n.created_at \
         FROM notifications n LEFT JOIN users u ON u.id = n.actor_id \
         WHERE n.user_id = $1 ORDER BY n.created_at DESC LIMIT 30",
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;

    rows.into_iter()
        .map(|n| {
            Ok(NotificationDto {
                id: n.id.to_string(),
                kind: notification_kind_from_db(&n.kind)?,
                actor_id: n.actor_id.map(|id| id.to_string()),
                actor_initials: n.actor_name.as_deref().map(initials),
                actor_name: n.actor_name,
                task_id: n.task_id.map(|id| id.to_string()),
                milestone_id: n.milestone_id.map(|id| id.to_string()),
                text: n.text,
                text_en: n.text_en,
                unread: n.unread,
                created_label_de: relative_label(n.created_at, "de"),
                created_label_en: relative_label(n.created_at, "en"),
            })
        })
        .collect()
}

async fn fetch_audit_events(
    db: &PgPool,
    workspace_id: Uuid,
) -> Result<Vec<AuditEventDto>, AppError> {
    let rows: Vec<AuditRow> = sqlx::query_as(
        "SELECT a.id, u.name AS actor_name, a.action, a.entity, a.created_at \
         FROM audit_events a LEFT JOIN users u ON u.id = a.actor_id \
         WHERE a.workspace_id = $1 ORDER BY a.created_at DESC LIMIT 20",
    )
    .bind(workspace_id)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|a| AuditEventDto {
            id: a.id.to_string(),
            actor_name: a.actor_name,
            action: a.action,
            entity: a.entity,
            created_label_de: relative_label(a.created_at, "de"),
            created_label_en: relative_label(a.created_at, "en"),
        })
        .collect())
}

async fn assert_project_access(
    db: &PgPool,
    user_id: Uuid,
    project_id: Uuid,
) -> Result<(), AppError> {
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM projects p JOIN memberships m ON m.workspace_id = p.workspace_id \
         WHERE p.id = $1 AND m.user_id = $2 AND m.status = 'active'",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_one(db)
    .await?;
    if count.0 == 0 {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

async fn assert_task_access(db: &PgPool, user_id: Uuid, task_id: Uuid) -> Result<(), AppError> {
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM tasks t JOIN projects p ON p.id = t.project_id \
         JOIN memberships m ON m.workspace_id = p.workspace_id \
         WHERE t.id = $1 AND m.user_id = $2 AND m.status = 'active'",
    )
    .bind(task_id)
    .bind(user_id)
    .fetch_one(db)
    .await?;
    if count.0 == 0 {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

async fn assert_status_in_project(
    db: &PgPool,
    project_id: Uuid,
    status_id: Uuid,
) -> Result<(), AppError> {
    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM project_statuses WHERE id = $1 AND project_id = $2")
            .bind(status_id)
            .bind(project_id)
            .fetch_one(db)
            .await?;
    if count.0 == 0 {
        return Err(AppError::BadRequest(
            "status does not belong to project".into(),
        ));
    }
    Ok(())
}

async fn assert_workspace_admin(
    db: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<(), AppError> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT role FROM memberships WHERE user_id = $1 AND workspace_id = $2 AND status = 'active'")
            .bind(user_id)
            .bind(workspace_id)
            .fetch_optional(db)
            .await?;
    let Some((role,)) = row else {
        return Err(AppError::Forbidden);
    };
    if !role_from_db(&role)?.can_admin() {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

async fn replace_assignees(
    db: &PgPool,
    task_id: Uuid,
    assignee_ids: &[String],
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM task_assignees WHERE task_id = $1")
        .bind(task_id)
        .execute(db)
        .await?;
    for id in assignee_ids {
        let user_id = uuid_from_str(id)?;
        sqlx::query(
            "INSERT INTO task_assignees (task_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(task_id)
        .bind(user_id)
        .execute(db)
        .await?;
    }
    touch_task(db, task_id).await?;
    Ok(())
}

async fn touch_task(db: &PgPool, task_id: Uuid) -> Result<(), AppError> {
    sqlx::query("UPDATE tasks SET updated_at = now() WHERE id = $1")
        .bind(task_id)
        .execute(db)
        .await?;
    Ok(())
}

async fn audit_for_user(
    db: &PgPool,
    user_id: Uuid,
    action: &str,
    entity: &str,
    entity_id: Option<Uuid>,
) -> Result<(), AppError> {
    let workspace_id: Option<(Uuid,)> = sqlx::query_as(
        "SELECT workspace_id FROM memberships WHERE user_id = $1 ORDER BY created_at ASC LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(db)
    .await?;

    if let Some((workspace_id,)) = workspace_id {
        sqlx::query(
            "INSERT INTO audit_events (id, workspace_id, actor_id, action, entity, entity_id, metadata) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(Uuid::new_v4())
        .bind(workspace_id)
        .bind(user_id)
        .bind(action)
        .bind(entity)
        .bind(entity_id)
        .bind(json!({}))
        .execute(db)
        .await?;
    }
    Ok(())
}

async fn create_workspace_for_user(db: &PgPool, user_id: Uuid, name: &str) -> Result<(), AppError> {
    let workspace_id = Uuid::new_v4();
    let project_id = Uuid::new_v4();
    let slug = format!("{}-workspace", initials(name).to_lowercase());

    sqlx::query(
        "INSERT INTO workspaces (id, name, url_slug, default_lang) VALUES ($1, $2, $3, 'de')",
    )
    .bind(workspace_id)
    .bind(format!("{} Workspace", name))
    .bind(slug)
    .execute(db)
    .await?;
    sqlx::query("INSERT INTO memberships (id, workspace_id, user_id, role, status, last_active_at) VALUES ($1, $2, $3, 'owner', 'active', now())")
        .bind(Uuid::new_v4())
        .bind(workspace_id)
        .bind(user_id)
        .execute(db)
        .await?;
    sqlx::query("INSERT INTO projects (id, workspace_id, name, key) VALUES ($1, $2, 'Neues Bauprojekt', 'KWB')")
        .bind(project_id)
        .bind(workspace_id)
        .execute(db)
        .await?;
    insert_default_statuses(db, project_id).await?;
    Ok(())
}

async fn seed_demo(db: &PgPool) -> Result<(), AppError> {
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
        ("Geplant", "Planned", "#8c867b"),
        ("In Arbeit", "In progress", "#6b8aa6"),
        ("Review", "Review", "#c98a3a"),
        ("Fertig", "Done", "#5f8d6a"),
    ];
    for (idx, (de, en, color)) in statuses.into_iter().enumerate() {
        sqlx::query(
            "INSERT INTO project_statuses (id, project_id, name_de, name_en, position, color) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(status_ids[idx])
        .bind(project_id)
        .bind(de)
        .bind(en)
        .bind(idx as i32)
        .bind(color)
        .execute(db)
        .await?;
    }

    let tasks = seed_tasks(project_id, status_ids);
    let mut task_ids = HashMap::new();
    for task in &tasks {
        task_ids.insert(task.key, task.id);
        sqlx::query(
            "INSERT INTO tasks \
             (id, project_id, key, title, title_en, description, description_en, tag, tag_color, priority, status_id, start_date, due_date, phase, created_by, comments_count, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, now() - interval '3 days', now() - interval '25 minutes')",
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
        .bind(NaiveDate::parse_from_str(task.start, "%Y-%m-%d")?)
        .bind(NaiveDate::parse_from_str(task.due, "%Y-%m-%d")?)
        .bind(task.phase)
        .bind(people_by_initial(task.assignees[0])?)
        .bind(task.comments_count)
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

    seed_attachment(db, task_ids["KWB-104"], "maengelprotokoll.pdf", 240_000).await?;
    seed_attachment(db, task_ids["KWB-104"], "fotoanhang-liste.json", 18_000).await?;
    seed_attachment(db, task_ids["KWB-107"], "terminplan.png", 512_000).await?;
    seed_attachment(db, task_ids["KWB-108"], "abnahme-checkliste.csv", 4_000).await?;

    let milestones = [
        (
            "Planungsfreigabe",
            "Planning approval",
            "2026-06-05",
            true,
            "planung",
        ),
        (
            "Gewerke koordiniert",
            "Trades coordinated",
            "2026-06-12",
            false,
            "ausfuehrung",
        ),
        (
            "Abnahme Bauabschnitt A",
            "Construction phase A handover",
            "2026-06-18",
            false,
            "abnahme",
        ),
    ];
    for (title, title_en, due, done, phase) in milestones {
        sqlx::query(
            "INSERT INTO milestones (id, project_id, title, title_en, due_date, done, phase) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(Uuid::new_v4())
        .bind(project_id)
        .bind(title)
        .bind(title_en)
        .bind(NaiveDate::parse_from_str(due, "%Y-%m-%d")?)
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
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, now() - interval '{}')",
            age
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

async fn insert_default_statuses(db: &PgPool, project_id: Uuid) -> Result<(), AppError> {
    for (idx, (de, en, color)) in [
        ("Geplant", "Planned", "#8c867b"),
        ("In Arbeit", "In progress", "#6b8aa6"),
        ("Review", "Review", "#c98a3a"),
        ("Fertig", "Done", "#5f8d6a"),
    ]
    .into_iter()
    .enumerate()
    {
        sqlx::query(
            "INSERT INTO project_statuses (id, project_id, name_de, name_en, position, color) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(Uuid::new_v4())
        .bind(project_id)
        .bind(de)
        .bind(en)
        .bind(idx as i32)
        .bind(color)
        .execute(db)
        .await?;
    }
    Ok(())
}

async fn seed_attachment(
    db: &PgPool,
    task_id: Uuid,
    file_name: &str,
    size: i64,
) -> Result<(), AppError> {
    let kind = if file_name.ends_with(".png") || file_name.ends_with(".jpg") {
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
    .bind(size)
    .bind(format!("seed/{file_name}"))
    .execute(db)
    .await?;
    Ok(())
}

#[derive(Debug)]
struct SeedTask {
    id: Uuid,
    key: &'static str,
    title: &'static str,
    title_en: &'static str,
    description: &'static str,
    description_en: &'static str,
    tag: &'static str,
    tag_color: &'static str,
    priority: Priority,
    status_id: Uuid,
    start: &'static str,
    due: &'static str,
    phase: &'static str,
    assignees: &'static [&'static str],
    subtasks: &'static [(&'static str, &'static str, bool)],
    comments_count: i64,
}

fn seed_tasks(project_id: Uuid, status_ids: [Uuid; 4]) -> Vec<SeedTask> {
    let _ = project_id;
    vec![
        SeedTask {
            id: fixed_uuid("30000000-0000-4000-8000-000000000104").unwrap(),
            key: "KWB-104",
            title: "Mängelaufnahme koordinieren",
            title_en: "Coordinate defect walk-through",
            description: "Begehung für die Wohnungseingänge A-C koordinieren, Fotos sichern und Nachfristen mit den Gewerken abstimmen.",
            description_en: "Coordinate the walk-through for entrances A-C, capture photos and align remediation deadlines with the trades.",
            tag: "Ausführung",
            tag_color: "accent",
            priority: Priority::Urgent,
            status_id: status_ids[1],
            start: "2026-06-09",
            due: "2026-06-11",
            phase: "ausfuehrung",
            assignees: &["TL", "JS"],
            subtasks: &[
                ("Begehungstermin bestätigen", "Confirm walk-through appointment", true),
                ("Fotodokumentation anlegen", "Create photo documentation", true),
                ("Mängelliste mit Gewerken abstimmen", "Align defect list with trades", true),
                ("Nachfrist für offene Punkte setzen", "Set deadline for open items", true),
                ("Rückmeldung an Bauleitung senden", "Send update to site management", false),
                ("Abnahme-Nachtermin planen", "Plan follow-up handover", false),
            ],
            comments_count: 4,
        },
        SeedTask {
            id: fixed_uuid("30000000-0000-4000-8000-000000000107").unwrap(),
            key: "KWB-107",
            title: "Terminplan aktualisieren",
            title_en: "Update construction schedule",
            description: "Bauzeitenplan mit neuen Lieferterminen, Pufferzeiten und Abhängigkeiten für Ausbau und Fassade aktualisieren.",
            description_en: "Update the construction schedule with new delivery dates, buffers and dependencies for interiors and facade work.",
            tag: "Planung",
            tag_color: "good",
            priority: Priority::High,
            status_id: status_ids[1],
            start: "2026-06-10",
            due: "2026-06-13",
            phase: "planung",
            assignees: &["MR", "AK"],
            subtasks: &[
                ("Liefertermine einarbeiten", "Add delivery dates", true),
                ("Kritischen Pfad prüfen", "Review critical path", true),
                ("Puffer für Fassade setzen", "Set facade buffer", false),
                ("Plan an Team verteilen", "Share schedule with team", false),
            ],
            comments_count: 2,
        },
        SeedTask {
            id: fixed_uuid("30000000-0000-4000-8000-000000000101").unwrap(),
            key: "KWB-101",
            title: "Freigabe Brandschutzkonzept",
            title_en: "Approve fire-safety concept",
            description: "Brandschutznachweise prüfen, Rückfragen klären und die Freigabe im Projektordner ablegen.",
            description_en: "Review fire-safety evidence, resolve questions and file the approval in the project folder.",
            tag: "Planung",
            tag_color: "accent",
            priority: Priority::High,
            status_id: status_ids[3],
            start: "2026-06-03",
            due: "2026-06-08",
            phase: "planung",
            assignees: &["JS"],
            subtasks: &[("Nachweise prüfen", "Review evidence", true), ("Rückfragen klären", "Resolve questions", true), ("Freigabe ablegen", "File approval", true)],
            comments_count: 6,
        },
        SeedTask {
            id: fixed_uuid("30000000-0000-4000-8000-000000000102").unwrap(),
            key: "KWB-102",
            title: "Bemusterung Wohnungen",
            title_en: "Apartment sample selection",
            description: "Materialvarianten für Boden, Bad und Türen final abstimmen und im Musterkatalog dokumentieren.",
            description_en: "Finalize material variants for flooring, bathrooms and doors and document them in the sample catalog.",
            tag: "Bemusterung",
            tag_color: "cool",
            priority: Priority::Medium,
            status_id: status_ids[3],
            start: "2026-06-01",
            due: "2026-06-05",
            phase: "planung",
            assignees: &["AK"],
            subtasks: &[("Bodenbelag festlegen", "Select flooring", true), ("Badserie freigeben", "Approve bathroom series", true), ("Türliste exportieren", "Export door list", true)],
            comments_count: 1,
        },
        SeedTask {
            id: fixed_uuid("30000000-0000-4000-8000-000000000103").unwrap(),
            key: "KWB-103",
            title: "Mieterinformation vorbereiten",
            title_en: "Prepare tenant communication",
            description: "Aushang, Terminfenster und Ansprechpartner für die Modernisierungsarbeiten abstimmen.",
            description_en: "Align notices, appointment windows and contacts for the modernization work.",
            tag: "Kommunikation",
            tag_color: "cool",
            priority: Priority::Medium,
            status_id: status_ids[1],
            start: "2026-06-08",
            due: "2026-06-12",
            phase: "planung",
            assignees: &["AK", "MR"],
            subtasks: &[("Aushang entwerfen", "Draft notice", true), ("Terminfenster prüfen", "Check appointment windows", false), ("Ansprechpartner ergänzen", "Add contacts", false), ("Freigabe Verwaltung", "Administration approval", false), ("Verteilung planen", "Plan distribution", false)],
            comments_count: 3,
        },
        SeedTask {
            id: fixed_uuid("30000000-0000-4000-8000-000000000105").unwrap(),
            key: "KWB-105",
            title: "Gewerkefreigabe dokumentieren",
            title_en: "Document trade approvals",
            description: "Freigaben für Elektro, Sanitär und Trockenbau inklusive Prüfnachweise im Projektraum ablegen.",
            description_en: "File approvals for electrical, plumbing and drywall work, including verification documents, in the project room.",
            tag: "Vergabe",
            tag_color: "ink",
            priority: Priority::Low,
            status_id: status_ids[0],
            start: "2026-06-12",
            due: "2026-06-14",
            phase: "vergabe",
            assignees: &["JS"],
            subtasks: &[("Elektrofreigabe ablegen", "File electrical approval", false), ("Sanitärfreigabe ablegen", "File plumbing approval", false), ("Trockenbau-Nachweis ergänzen", "Add drywall evidence", false)],
            comments_count: 0,
        },
        SeedTask {
            id: fixed_uuid("30000000-0000-4000-8000-000000000106").unwrap(),
            key: "KWB-106",
            title: "Abnahmeprotokoll prüfen",
            title_en: "Review handover protocol",
            description: "Protokoll für Bauabschnitt A gegen Fotodokumentation, Restarbeiten und Unterschriften prüfen.",
            description_en: "Check the phase A handover protocol against photos, remaining work and signatures.",
            tag: "Abnahme",
            tag_color: "good",
            priority: Priority::High,
            status_id: status_ids[2],
            start: "2026-06-08",
            due: "2026-06-10",
            phase: "abnahme",
            assignees: &["MR"],
            subtasks: &[("Fotos abgleichen", "Compare photos", true), ("Unterschriften prüfen", "Check signatures", true), ("Restarbeiten markieren", "Mark remaining work", false)],
            comments_count: 2,
        },
        SeedTask {
            id: fixed_uuid("30000000-0000-4000-8000-000000000108").unwrap(),
            key: "KWB-108",
            title: "Restarbeiten kontrollieren",
            title_en: "Check remaining work",
            description: "Offene Punkte aus der Abnahme in Treppenhaus und Keller prüfen und Status aktualisieren.",
            description_en: "Check open handover items in the stairwell and basement and update their status.",
            tag: "QA",
            tag_color: "cool",
            priority: Priority::Medium,
            status_id: status_ids[2],
            start: "2026-06-11",
            due: "2026-06-12",
            phase: "abnahme",
            assignees: &["SB"],
            subtasks: &[("Treppenhaus prüfen", "Check stairwell", true), ("Keller prüfen", "Check basement", true), ("Status im Protokoll setzen", "Update protocol status", false)],
            comments_count: 1,
        },
        SeedTask {
            id: fixed_uuid("30000000-0000-4000-8000-000000000109").unwrap(),
            key: "KWB-109",
            title: "Baustellenbericht erstellen",
            title_en: "Create site report",
            description: "Wochenbericht mit Wetter, Fortschritt, Risiken und Fotos für Eigentümer und Verwaltung erstellen.",
            description_en: "Create the weekly report with weather, progress, risks and photos for owners and administration.",
            tag: "Bericht",
            tag_color: "ink",
            priority: Priority::Low,
            status_id: status_ids[3],
            start: "2026-06-05",
            due: "2026-06-07",
            phase: "ausfuehrung",
            assignees: &["DK"],
            subtasks: &[("Fotoliste ergänzen", "Add photo list", true), ("Risiken aktualisieren", "Update risks", true)],
            comments_count: 0,
        },
        SeedTask {
            id: fixed_uuid("30000000-0000-4000-8000-000000000110").unwrap(),
            key: "KWB-110",
            title: "Bauabschnitt B vorbereiten",
            title_en: "Prepare construction phase B",
            description: "Baulogistik, Materialabruf, Sicherheitsunterweisung und Kommunikationsplan für Bauabschnitt B vorbereiten.",
            description_en: "Prepare site logistics, material call-offs, safety briefing and communication plan for construction phase B.",
            tag: "Meilenstein",
            tag_color: "accent",
            priority: Priority::Urgent,
            status_id: status_ids[0],
            start: "2026-06-15",
            due: "2026-06-18",
            phase: "ausfuehrung",
            assignees: &["AL", "JS"],
            subtasks: &[("Materialabruf prüfen", "Check material call-offs", false), ("Logistikfläche reservieren", "Reserve logistics area", false), ("Sicherheitsunterweisung planen", "Schedule safety briefing", false), ("Mieterinformation versenden", "Send tenant notice", false)],
            comments_count: 1,
        },
    ]
}

fn people_by_initial(initials: &str) -> Result<Uuid, AppError> {
    let id = match initials {
        "AL" => "20000000-0000-4000-8000-000000000001",
        "AK" => "20000000-0000-4000-8000-000000000002",
        "MR" => "20000000-0000-4000-8000-000000000003",
        "JS" => "20000000-0000-4000-8000-000000000004",
        "TL" => "20000000-0000-4000-8000-000000000005",
        "SB" => "20000000-0000-4000-8000-000000000006",
        "DK" => "20000000-0000-4000-8000-000000000007",
        _ => {
            return Err(AppError::BadRequest(format!(
                "unknown seed user {initials}"
            )))
        }
    };
    fixed_uuid(id)
}

fn role_to_db(role: &Role) -> &'static str {
    match role {
        Role::Owner => "owner",
        Role::Admin => "admin",
        Role::Member => "member",
        Role::Viewer => "viewer",
    }
}

fn role_from_db(value: &str) -> Result<Role, AppError> {
    match value {
        "owner" => Ok(Role::Owner),
        "admin" => Ok(Role::Admin),
        "member" => Ok(Role::Member),
        "viewer" => Ok(Role::Viewer),
        _ => Err(AppError::BadRequest(format!("unknown role {value}"))),
    }
}

fn member_status_from_db(value: &str) -> Result<MemberStatus, AppError> {
    match value {
        "active" => Ok(MemberStatus::Active),
        "invited" => Ok(MemberStatus::Invited),
        _ => Err(AppError::BadRequest(format!(
            "unknown member status {value}"
        ))),
    }
}

fn priority_to_db(priority: &Priority) -> &'static str {
    match priority {
        Priority::Urgent => "urgent",
        Priority::High => "high",
        Priority::Medium => "medium",
        Priority::Low => "low",
    }
}

fn priority_from_db(value: &str) -> Result<Priority, AppError> {
    match value {
        "urgent" => Ok(Priority::Urgent),
        "high" => Ok(Priority::High),
        "medium" => Ok(Priority::Medium),
        "low" => Ok(Priority::Low),
        _ => Err(AppError::BadRequest(format!("unknown priority {value}"))),
    }
}

fn notification_kind_to_db(kind: &NotificationKind) -> &'static str {
    match kind {
        NotificationKind::Assigned => "assigned",
        NotificationKind::Mention => "mention",
        NotificationKind::Due => "due",
        NotificationKind::Comment => "comment",
        NotificationKind::Done => "done",
        NotificationKind::Milestone => "milestone",
    }
}

fn notification_kind_from_db(value: &str) -> Result<NotificationKind, AppError> {
    match value {
        "assigned" => Ok(NotificationKind::Assigned),
        "mention" => Ok(NotificationKind::Mention),
        "due" => Ok(NotificationKind::Due),
        "comment" => Ok(NotificationKind::Comment),
        "done" => Ok(NotificationKind::Done),
        "milestone" => Ok(NotificationKind::Milestone),
        _ => Err(AppError::BadRequest(format!(
            "unknown notification kind {value}"
        ))),
    }
}

fn attachment_kind_to_db(kind: &AttachmentKind) -> &'static str {
    match kind {
        AttachmentKind::File => "file",
        AttachmentKind::Image => "image",
    }
}

fn attachment_kind_from_db(value: &str) -> Result<AttachmentKind, AppError> {
    match value {
        "file" => Ok(AttachmentKind::File),
        "image" => Ok(AttachmentKind::Image),
        _ => Err(AppError::BadRequest(format!(
            "unknown attachment kind {value}"
        ))),
    }
}

fn uuid_from_str(value: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(value).map_err(|_| AppError::BadRequest("invalid id".into()))
}

fn fixed_uuid(value: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(value).map_err(|e| AppError::BadRequest(e.to_string()))
}

fn parse_optional_date(value: Option<&str>) -> Result<Option<NaiveDate>, AppError> {
    value
        .filter(|v| !v.trim().is_empty())
        .map(|v| {
            NaiveDate::parse_from_str(v, "%Y-%m-%d")
                .map_err(|_| AppError::BadRequest("date must be YYYY-MM-DD".into()))
        })
        .transpose()
}

fn initials(name: &str) -> String {
    let mut chars = name
        .split_whitespace()
        .filter_map(|part| part.chars().next())
        .take(2)
        .collect::<String>();
    if chars.is_empty() {
        chars = "?".to_string();
    }
    chars.to_uppercase()
}

fn relative_label(ts: DateTime<Utc>, lang: &str) -> String {
    let delta = Utc::now().signed_duration_since(ts);
    if delta.num_minutes() < 1 {
        return if lang == "de" {
            "gerade eben"
        } else {
            "just now"
        }
        .to_string();
    }
    if delta.num_minutes() < 60 {
        return if lang == "de" {
            format!("vor {} Min", delta.num_minutes())
        } else {
            format!("{} min ago", delta.num_minutes())
        };
    }
    if delta.num_hours() < 24 {
        return if lang == "de" {
            format!("vor {} Std", delta.num_hours())
        } else {
            format!("{} h ago", delta.num_hours())
        };
    }
    if delta.num_days() == 1 {
        return if lang == "de" { "Gestern" } else { "Yesterday" }.to_string();
    }
    if lang == "de" {
        format!("vor {} Tagen", delta.num_days())
    } else {
        format!("{} days ago", delta.num_days())
    }
}

fn size_label(bytes: i64) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else {
        format!("{} KB", (bytes as f64 / 1_000.0).round() as i64)
    }
}

fn sanitize_file_name(name: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_use_first_two_words() {
        assert_eq!(initials("Alex Lindner"), "AL");
        assert_eq!(initials("Mira"), "M");
    }

    #[test]
    fn size_labels_are_human_readable() {
        assert_eq!(size_label(18_000), "18 KB");
        assert_eq!(size_label(1_250_000), "1.2 MB");
    }

    #[test]
    fn file_names_are_sanitized() {
        assert_eq!(sanitize_file_name("../bad name.pdf"), "bad_name.pdf");
    }
}
