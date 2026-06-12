use std::{
    collections::HashMap,
    env,
    net::{IpAddr, SocketAddr, TcpStream},
    path::{Path as FsPath, PathBuf},
    sync::{Arc, LazyLock, Mutex},
    time::{Duration as StdDuration, Instant},
};

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    body::Body,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        ConnectInfo, DefaultBodyLimit, Multipart, Path, Query, Request, State,
    },
    http::{
        header::{
            CONTENT_DISPOSITION, CONTENT_SECURITY_POLICY, CONTENT_TYPE, COOKIE, HOST, ORIGIN,
            REFERRER_POLICY, SET_COOKIE, X_CONTENT_TYPE_OPTIONS, X_FRAME_OPTIONS,
        },
        HeaderMap, HeaderValue, Method, StatusCode,
    },
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use hmac::{Hmac, Mac};
use kowobau_shared::*;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{postgres::PgPoolOptions, FromRow, PgConnection, PgPool};
use tokio::{
    fs,
    io::AsyncWriteExt,
    net::TcpListener,
    sync::{broadcast, Semaphore},
};
use tokio_util::io::ReaderStream;
use tower_http::{
    compression::CompressionLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

const COOKIE_NAME: &str = "kowobau_session";
// __Host- locks the cookie to the exact host over HTTPS (requires Secure,
// Path=/ and no Domain attribute, all of which build_cookie guarantees).
const SECURE_COOKIE_NAME: &str = "__Host-kowobau_session";
const MAX_UPLOAD_BYTES: usize = 50 * 1024 * 1024;
const MAX_JSON_BODY_BYTES: usize = 64 * 1024;
const AUTH_RATE_LIMIT_WINDOW: StdDuration = StdDuration::from_secs(60);
const AUTH_RATE_LIMIT_MAX_ATTEMPTS: u32 = 10;
const INVITE_TTL_DAYS: i64 = 14;
// Bounds concurrent Argon2 work so unauthenticated login/register floods
// cannot pin every core with password hashing.
const MAX_CONCURRENT_PASSWORD_HASHES: usize = 4;
const MAX_WORKSPACE_STORAGE_BYTES: i64 = 2 * 1024 * 1024 * 1024;
const ALLOWED_UPLOAD_EXTENSIONS: &[&str] = &[
    "pdf", "png", "jpg", "jpeg", "webp", "svg", "csv", "xlsx", "docx", "txt", "json", "zip", "dwg",
    "ifc",
];
// Extensions that may be served with Content-Disposition: inline so the app
// can preview them in <img>/<iframe>. SVG is deliberately excluded: rendered
// as a document it could execute script; it stays download-only.
const INLINE_PREVIEW_EXTENSIONS: &[&str] = &["pdf", "png", "jpg", "jpeg", "webp"];
// Bounded fanout queue for realtime events; slow sockets get a resync hint
// instead of unbounded buffering.
const EVENT_CHANNEL_CAPACITY: usize = 256;

// Equalizes login timing for unknown emails so account existence cannot be inferred.
static DUMMY_PASSWORD_HASH: LazyLock<String> = LazyLock::new(|| {
    hash_password("timing-equalization-placeholder").expect("hashing a constant cannot fail")
});

#[derive(Debug, Clone)]
struct AppConfig {
    bind: String,
    static_dir: PathBuf,
    upload_dir: PathBuf,
    session_secret: String,
    cookie_secure: bool,
    seed_demo: bool,
    registration_enabled: bool,
    max_workspace_storage_bytes: i64,
    // When true (behind our nginx), the client IP for rate limiting is taken
    // from X-Real-IP instead of the peer address.
    trust_proxy: bool,
    // When set (e.g. "https://kowobau.example.com"), state-changing requests
    // must carry exactly this Origin, closing the scheme-blind host check.
    public_origin: Option<String>,
}

#[derive(Debug, Clone)]
struct AppState {
    db: PgPool,
    cfg: AppConfig,
    auth_limiter: Arc<Mutex<HashMap<IpAddr, (Instant, u32)>>>,
    hash_permits: Arc<Semaphore>,
    // Workspace-scoped realtime events, fanned out to every connected socket.
    events: broadcast::Sender<WorkspaceEventDto>,
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
    #[error("too many requests, try again later")]
    TooManyRequests,
    #[error("{0}")]
    Internal(String),
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
            AppError::TooManyRequests => StatusCode::TOO_MANY_REQUESTS,
            AppError::Internal(_)
            | AppError::Sqlx(_)
            | AppError::Io(_)
            | AppError::Chrono(_)
            | AppError::Anyhow(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };

        let message = match status {
            StatusCode::INTERNAL_SERVER_ERROR => {
                tracing::error!(error = %self, "internal server error");
                "internal server error".to_string()
            }
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

    let cfg = AppConfig::from_env()?;
    fs::create_dir_all(&cfg.upload_dir).await?;

    let database_url = env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://kowobau:kowobau@localhost:5432/kowobau".to_string());
    let db = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(StdDuration::from_secs(10))
        .connect(&database_url)
        .await?;

    sqlx::migrate!("./migrations").run(&db).await?;
    if cfg.seed_demo {
        tracing::info!("KOWOBAU_SEED_DEMO is enabled; seeding demo data on empty database");
        seed_demo(&db).await?;
    } else {
        tracing::info!("demo seed disabled (set KOWOBAU_SEED_DEMO=true to enable)");
    }

    let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
    let state = AppState {
        db,
        cfg,
        auth_limiter: Arc::new(Mutex::new(HashMap::new())),
        hash_permits: Arc::new(Semaphore::new(MAX_CONCURRENT_PASSWORD_HASHES)),
        events,
    };
    let app = build_router(state.clone());

    let listener = TcpListener::bind(&state.cfg.bind).await?;
    tracing::info!("KoWoBau-Planner listening on http://{}", state.cfg.bind);
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    Ok(())
}

impl AppConfig {
    fn from_env() -> anyhow::Result<Self> {
        let session_secret = env_var("KOWOBAU_SESSION_SECRET", "CADENCE_SESSION_SECRET")
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "KOWOBAU_SESSION_SECRET must be set (generate one with e.g. `openssl rand -base64 48`)"
                )
            })?;
        if session_secret.len() < 32 {
            anyhow::bail!("KOWOBAU_SESSION_SECRET must be at least 32 characters long");
        }

        Ok(Self {
            bind: env_var("KOWOBAU_BIND", "CADENCE_BIND")
                .unwrap_or_else(|| "127.0.0.1:8080".to_string()),
            static_dir: env_var("KOWOBAU_STATIC_DIR", "CADENCE_STATIC_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("crates/frontend/dist")),
            upload_dir: env_var("KOWOBAU_UPLOAD_DIR", "CADENCE_UPLOAD_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("crates/backend/uploads")),
            session_secret,
            cookie_secure: env_var("KOWOBAU_COOKIE_SECURE", "CADENCE_COOKIE_SECURE")
                .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE"))
                .unwrap_or(false),
            seed_demo: env_var("KOWOBAU_SEED_DEMO", "CADENCE_SEED_DEMO")
                .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE"))
                .unwrap_or(false),
            registration_enabled: env_var(
                "KOWOBAU_REGISTRATION_ENABLED",
                "CADENCE_REGISTRATION_ENABLED",
            )
            .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE"))
            .unwrap_or(true),
            max_workspace_storage_bytes: env_var(
                "KOWOBAU_MAX_WORKSPACE_STORAGE_BYTES",
                "CADENCE_MAX_WORKSPACE_STORAGE_BYTES",
            )
            // A typo must not silently fall back to the default quota.
            .map(|v| {
                v.parse().map_err(|_| {
                    anyhow::anyhow!("KOWOBAU_MAX_WORKSPACE_STORAGE_BYTES must be an integer byte count, got {v:?}")
                })
            })
            .transpose()?
            .unwrap_or(MAX_WORKSPACE_STORAGE_BYTES),
            trust_proxy: env_var("KOWOBAU_TRUST_PROXY", "CADENCE_TRUST_PROXY")
                .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE"))
                .unwrap_or(false),
            public_origin: env_var("KOWOBAU_PUBLIC_ORIGIN", "CADENCE_PUBLIC_ORIGIN")
                .map(|v| v.trim_end_matches('/').to_string())
                .filter(|v| !v.is_empty()),
        })
    }
}

fn env_var(primary: &str, fallback: &str) -> Option<String> {
    env::var(primary).ok().or_else(|| env::var(fallback).ok())
}

fn healthcheck_cli() -> anyhow::Result<()> {
    use std::io::{Read, Write};

    let bind =
        env_var("KOWOBAU_BIND", "CADENCE_BIND").unwrap_or_else(|| "127.0.0.1:8080".to_string());
    let port = bind
        .rsplit(':')
        .next()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(8080);
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(StdDuration::from_secs(5)))?;
    stream.set_write_timeout(Some(StdDuration::from_secs(5)))?;
    stream
        .write_all(b"GET /api/health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")?;
    let mut buf = Vec::with_capacity(64);
    let mut chunk = [0u8; 64];
    while buf.len() < 12 {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let head = String::from_utf8_lossy(&buf);
    anyhow::ensure!(
        head.starts_with("HTTP/1.1 200"),
        "unexpected health response: {head}"
    );
    Ok(())
}

fn build_router(state: AppState) -> Router {
    let auth_rate_limit = middleware::from_fn_with_state(state.clone(), rate_limit_auth);
    let api = Router::new()
        .route("/health", get(health))
        .route(
            "/auth/register",
            post(register).route_layer(auth_rate_limit.clone()),
        )
        .route("/auth/login", post(login).route_layer(auth_rate_limit))
        .route("/auth/logout", post(logout))
        .route("/auth/logout-all", post(logout_all))
        .route("/auth/me", get(me))
        .route("/bootstrap", get(bootstrap))
        .route("/ws", get(ws_handler))
        .route("/tasks", get(list_tasks).post(create_task))
        .route("/tickets", get(list_tickets).post(create_ticket))
        .route(
            "/tickets/{id}",
            get(get_ticket).patch(update_ticket).delete(delete_ticket),
        )
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
        .route(
            "/tasks/{id}/attachments",
            post(upload_attachment).route_layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES)),
        )
        .route("/attachments/{id}", get(download_attachment))
        .route("/notifications/{id}/read", post(read_notification))
        .route("/notifications/read-all", post(read_all_notifications))
        .route("/workspaces/{id}", patch(update_workspace))
        .route("/workspaces/{id}/invites", post(invite_member))
        .route(
            "/memberships/{id}",
            patch(update_membership).delete(remove_membership),
        )
        .route("/users/{id}", axum::routing::delete(delete_user))
        .layer(DefaultBodyLimit::max(MAX_JSON_BODY_BYTES))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            enforce_same_origin,
        ))
        .with_state(state.clone());

    let index = state.cfg.static_dir.join("index.html");
    let spa = ServeDir::new(&state.cfg.static_dir).not_found_service(ServeFile::new(index));

    Router::new()
        .nest("/api", api)
        .fallback_service(spa)
        .layer(middleware::from_fn(security_headers))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
}

/// CSRF defense-in-depth on top of SameSite=Lax: browser-sent state-changing
/// requests must come from our own origin. Requests without an Origin header
/// (curl, server-to-server) are allowed through. With KOWOBAU_PUBLIC_ORIGIN
/// set, the full origin (including scheme) must match exactly; otherwise we
/// fall back to comparing the Origin host against the Host header.
async fn enforce_same_origin(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, AppError> {
    if !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS)
        && !same_origin(&state.cfg, req.headers())
    {
        return Err(AppError::Forbidden);
    }
    Ok(next.run(req).await)
}

/// True when the request's Origin header (if present) matches our own origin.
/// Requests without an Origin header (curl, server-to-server) pass. With
/// KOWOBAU_PUBLIC_ORIGIN set, the full origin (including scheme) must match
/// exactly; otherwise the Origin host is compared against the Host header.
fn same_origin(cfg: &AppConfig, headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(ORIGIN).and_then(|v| v.to_str().ok()) else {
        return true;
    };
    if let Some(expected) = &cfg.public_origin {
        return origin.eq_ignore_ascii_case(expected);
    }
    let origin_host = host_only(origin.split_once("://").map_or(origin, |(_, a)| a));
    let request_host = headers
        .get(HOST)
        .and_then(|v| v.to_str().ok())
        .map(host_only)
        .unwrap_or("");
    origin != "null" && !request_host.is_empty() && origin_host.eq_ignore_ascii_case(request_host)
}

/// Fixed-window per-IP limiter for the unauthenticated auth endpoints. These
/// trigger expensive Argon2 hashing and are the brute-force surface, so they
/// get a much tighter budget than the rest of the API.
async fn rate_limit_auth(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let ip = client_ip(&state, &req).ok_or(AppError::TooManyRequests)?;
    {
        let mut limiter = state.auth_limiter.lock().expect("limiter lock poisoned");
        let now = Instant::now();
        // Opportunistic pruning keeps the map from growing with one entry per
        // IP ever seen.
        limiter.retain(|_, (start, _)| now.duration_since(*start) < AUTH_RATE_LIMIT_WINDOW);
        let entry = limiter.entry(ip).or_insert((now, 0));
        if now.duration_since(entry.0) >= AUTH_RATE_LIMIT_WINDOW {
            *entry = (now, 0);
        }
        entry.1 += 1;
        if entry.1 > AUTH_RATE_LIMIT_MAX_ATTEMPTS {
            return Err(AppError::TooManyRequests);
        }
    }
    Ok(next.run(req).await)
}

fn client_ip(state: &AppState, req: &Request) -> Option<IpAddr> {
    if state.cfg.trust_proxy {
        if let Some(ip) = req
            .headers()
            .get("x-real-ip")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse().ok())
        {
            return Some(ip);
        }
    }
    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip())
}

const CSP: &str = "default-src 'self'; script-src 'self' 'unsafe-inline' 'wasm-unsafe-eval'; \
     style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; \
     connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'";

/// Defense-in-depth security headers so a directly exposed backend (without
/// the nginx in front) still serves a hardened SPA.
async fn security_headers(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let headers = res.headers_mut();
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    // Handlers may set stricter, response-specific framing/CSP headers (inline
    // attachment previews need SAMEORIGIN framing); only fill in the defaults.
    if !headers.contains_key(X_FRAME_OPTIONS) {
        headers.insert(X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    }
    if !headers.contains_key(CONTENT_SECURITY_POLICY) {
        headers.insert(CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP));
    }
    headers.insert(
        REFERRER_POLICY,
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    res
}

/// Strips a `:port` suffix (and IPv6 brackets) from an authority string.
fn host_only(authority: &str) -> &str {
    if let Some(rest) = authority.strip_prefix('[') {
        return rest.split(']').next().unwrap_or(rest);
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => host,
        _ => authority,
    }
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

    // Invite lookup happens before the expensive hash so bad tokens fail fast.
    let invite: Option<(Uuid, Uuid, String)> =
        match payload
            .invite_token
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            Some(token) => {
                let row: Option<(Uuid, Uuid, String)> = sqlx::query_as(
                    "SELECT id, workspace_id, role FROM workspace_invites \
                 WHERE token_hash = $1 AND expires_at > now()",
                )
                .bind(invite_token_hash(token))
                .fetch_optional(&state.db)
                .await?;
                Some(row.ok_or_else(|| {
                    AppError::BadRequest("invite code is invalid or expired".into())
                })?)
            }
            None => None,
        };

    if invite.is_none() && !state.cfg.registration_enabled {
        let (user_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(&state.db)
            .await?;
        if user_count > 0 {
            return Err(AppError::Forbidden);
        }
    }

    let user_id = Uuid::new_v4();
    let password_hash = hash_password_async(&state, payload.password.clone()).await?;

    let mut tx = state.db.begin().await?;
    let inserted =
        sqlx::query("INSERT INTO users (id, email, name, password_hash) VALUES ($1, $2, $3, $4)")
            .bind(user_id)
            .bind(&email)
            .bind(payload.name.trim())
            .bind(password_hash)
            .execute(&mut *tx)
            .await;
    if let Err(err) = inserted {
        if is_unique_violation(&err) {
            return Err(AppError::Conflict("email is already registered".into()));
        }
        return Err(err.into());
    }

    let workspace_id = match invite {
        None => create_workspace_for_user(&mut tx, user_id, payload.name.trim()).await?,
        Some((invite_id, invite_workspace, role)) => {
            role_from_db(&role)?;
            // Single-use: claim the row first; a concurrent registration with
            // the same token loses and falls through to the error below.
            let deleted = sqlx::query("DELETE FROM workspace_invites WHERE id = $1")
                .bind(invite_id)
                .execute(&mut *tx)
                .await?;
            if deleted.rows_affected() == 0 {
                return Err(AppError::BadRequest(
                    "invite code is invalid or expired".into(),
                ));
            }
            sqlx::query(
                "INSERT INTO memberships (id, workspace_id, user_id, role, status, last_active_at) \
                 VALUES ($1, $2, $3, $4, 'active', now()) \
                 ON CONFLICT (workspace_id, user_id) DO NOTHING",
            )
            .bind(Uuid::new_v4())
            .bind(invite_workspace)
            .bind(user_id)
            .bind(&role)
            .execute(&mut *tx)
            .await?;
            invite_workspace
        }
    };

    record_audit(
        &mut *tx,
        workspace_id,
        user_id,
        "registered",
        "user",
        Some(user_id),
    )
    .await?;
    let session_id = create_session(&mut tx, user_id).await?;
    tx.commit().await?;

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
        // Burn the same amount of time as a real verification (see DUMMY_PASSWORD_HASH).
        let _ = verify_password_async(&state, payload.password, DUMMY_PASSWORD_HASH.clone()).await;
        return Err(AppError::Unauthorized);
    };

    verify_password_async(&state, payload.password.clone(), row.password_hash.clone()).await?;
    let mut conn = state.db.acquire().await?;
    let session_id = create_session(&mut conn, row.id).await?;

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

/// Revokes every session of the current user ("log out everywhere").
async fn logout_all(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(uuid_from_str(&ctx.user.id)?)
        .execute(&state.db)
        .await?;
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

async fn list_tickets(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<TicketDto>>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let bootstrap = fetch_bootstrap(&state.db, uuid_from_str(&ctx.user.id)?).await?;
    Ok(Json(bootstrap.tickets))
}

async fn get_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<TaskDto>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let task_id = uuid_from_str(&id)?;
    assert_task_read(&state.db, uuid_from_str(&ctx.user.id)?, task_id).await?;
    Ok(Json(fetch_task(&state.db, task_id).await?))
}

async fn get_ticket(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<TicketDto>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let ticket_id = uuid_from_str(&id)?;
    assert_ticket_read(&state.db, uuid_from_str(&ctx.user.id)?, ticket_id).await?;
    Ok(Json(fetch_ticket(&state.db, ticket_id).await?))
}

async fn create_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateTaskRequest>,
) -> Result<Json<TaskDto>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    let project_id = uuid_from_str(&payload.project_id)?;
    let workspace_id = assert_project_edit(&state.db, user_id, project_id).await?;

    if payload.title.trim().is_empty() {
        return Err(AppError::BadRequest("task title is required".into()));
    }

    let status_id = uuid_from_str(&payload.status_id)?;
    assert_status_in_project(&state.db, project_id, status_id).await?;

    let mut tx = state.db.begin().await?;
    // Serializes key generation per project so concurrent creates cannot collide.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(project_id.to_string())
        .execute(&mut *tx)
        .await?;
    let (project_key,): (String,) = sqlx::query_as("SELECT key FROM projects WHERE id = $1")
        .bind(project_id)
        .fetch_one(&mut *tx)
        .await?;
    let next: (i32,) = sqlx::query_as(
        "SELECT COALESCE(MAX(split_part(key, '-', 2)::INT), 100) + 1 \
         FROM tasks WHERE project_id = $1 AND key LIKE $2 || '-%' \
         AND split_part(key, '-', 2) ~ '^[0-9]+$'",
    )
    .bind(project_id)
    .bind(&project_key)
    .fetch_one(&mut *tx)
    .await?;
    let key = format!("{}-{}", project_key, next.0);

    let task_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO tasks \
         (id, project_id, key, title, description, tag, tag_color, priority, status_id, start_date, due_date, phase, recurrence, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
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
    .bind(payload.recurrence.as_ref().map(recurrence_to_db))
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    replace_assignees(&mut tx, task_id, &payload.assignee_ids).await?;
    for (idx, title) in payload.subtasks.iter().enumerate() {
        if !title.trim().is_empty() {
            sqlx::query(
                "INSERT INTO subtasks (id, task_id, title, position) VALUES ($1, $2, $3, $4)",
            )
            .bind(Uuid::new_v4())
            .bind(task_id)
            .bind(title.trim())
            .bind(idx as i32)
            .execute(&mut *tx)
            .await?;
        }
    }

    record_audit(
        &mut *tx,
        workspace_id,
        user_id,
        "created task",
        "task",
        Some(task_id),
    )
    .await?;
    tx.commit().await?;
    notify_workspace(&state, &headers, workspace_id, "task");
    Ok(Json(fetch_task(&state.db, task_id).await?))
}

async fn create_ticket(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateTicketRequest>,
) -> Result<Json<TicketDto>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    let project_id = uuid_from_str(&payload.project_id)?;
    let workspace_id = assert_project_edit(&state.db, user_id, project_id).await?;

    if payload.title.trim().is_empty() {
        return Err(AppError::BadRequest("ticket title is required".into()));
    }

    let assignee_id = payload
        .assignee_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
        .map(uuid_from_str)
        .transpose()?;

    let mut tx = state.db.begin().await?;
    if let Some(assignee_id) = assignee_id {
        assert_user_in_project(&mut *tx, project_id, assignee_id).await?;
    }

    // Serializes key generation per project so concurrent creates cannot collide.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(format!("tickets:{project_id}"))
        .execute(&mut *tx)
        .await?;
    let (project_key,): (String,) = sqlx::query_as("SELECT key FROM projects WHERE id = $1")
        .bind(project_id)
        .fetch_one(&mut *tx)
        .await?;
    let next: (i32,) = sqlx::query_as(
        "SELECT COALESCE(MAX(split_part(key, '-', 3)::INT), 0) + 1 \
         FROM tickets WHERE project_id = $1 AND key LIKE $2 || '-T-%' \
         AND split_part(key, '-', 3) ~ '^[0-9]+$'",
    )
    .bind(project_id)
    .bind(&project_key)
    .fetch_one(&mut *tx)
    .await?;
    let key = format!("{}-T-{}", project_key, next.0);

    let ticket_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO tickets \
         (id, project_id, key, title, description, status, priority, requester_name, assignee_id, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(ticket_id)
    .bind(project_id)
    .bind(&key)
    .bind(payload.title.trim())
    .bind(payload.description.trim())
    .bind(ticket_status_to_db(&payload.status))
    .bind(priority_to_db(&payload.priority))
    .bind(payload.requester_name.trim())
    .bind(assignee_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    record_audit(
        &mut *tx,
        workspace_id,
        user_id,
        "created ticket",
        "ticket",
        Some(ticket_id),
    )
    .await?;
    tx.commit().await?;
    notify_workspace(&state, &headers, workspace_id, "ticket");
    Ok(Json(fetch_ticket(&state.db, ticket_id).await?))
}

async fn update_ticket(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<UpdateTicketRequest>,
) -> Result<Json<TicketDto>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    let ticket_id = uuid_from_str(&id)?;
    let workspace_id = assert_ticket_edit(&state.db, user_id, ticket_id).await?;

    if let Some(title) = &payload.title {
        if title.trim().is_empty() {
            return Err(AppError::BadRequest("ticket title is required".into()));
        }
    }

    let mut tx = state.db.begin().await?;
    if let Some(title) = payload.title {
        sqlx::query("UPDATE tickets SET title = $1, updated_at = now() WHERE id = $2")
            .bind(title.trim())
            .bind(ticket_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(description) = payload.description {
        sqlx::query("UPDATE tickets SET description = $1, updated_at = now() WHERE id = $2")
            .bind(description.trim())
            .bind(ticket_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(status) = payload.status {
        sqlx::query("UPDATE tickets SET status = $1, updated_at = now() WHERE id = $2")
            .bind(ticket_status_to_db(&status))
            .bind(ticket_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(priority) = payload.priority {
        sqlx::query("UPDATE tickets SET priority = $1, updated_at = now() WHERE id = $2")
            .bind(priority_to_db(&priority))
            .bind(ticket_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(requester_name) = payload.requester_name {
        sqlx::query("UPDATE tickets SET requester_name = $1, updated_at = now() WHERE id = $2")
            .bind(requester_name.trim())
            .bind(ticket_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(assignee_id) = payload.assignee_id {
        let assignee_id = assignee_id
            .as_deref()
            .filter(|id| !id.trim().is_empty())
            .map(uuid_from_str)
            .transpose()?;
        if let Some(assignee_id) = assignee_id {
            let (project_id,): (Uuid,) =
                sqlx::query_as("SELECT project_id FROM tickets WHERE id = $1")
                    .bind(ticket_id)
                    .fetch_one(&mut *tx)
                    .await?;
            assert_user_in_project(&mut *tx, project_id, assignee_id).await?;
        }
        sqlx::query("UPDATE tickets SET assignee_id = $1, updated_at = now() WHERE id = $2")
            .bind(assignee_id)
            .bind(ticket_id)
            .execute(&mut *tx)
            .await?;
    }

    record_audit(
        &mut *tx,
        workspace_id,
        user_id,
        "updated ticket",
        "ticket",
        Some(ticket_id),
    )
    .await?;
    tx.commit().await?;
    notify_workspace(&state, &headers, workspace_id, "ticket");
    Ok(Json(fetch_ticket(&state.db, ticket_id).await?))
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
    let workspace_id = assert_task_edit(&state.db, user_id, task_id).await?;

    let mut tx = state.db.begin().await?;
    if let Some(title) = payload.title {
        if title.trim().is_empty() {
            return Err(AppError::BadRequest("task title is required".into()));
        }
        sqlx::query("UPDATE tasks SET title = $1, updated_at = now() WHERE id = $2")
            .bind(title.trim())
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(description) = payload.description {
        sqlx::query("UPDATE tasks SET description = $1, updated_at = now() WHERE id = $2")
            .bind(description.trim())
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(tag) = payload.tag {
        sqlx::query("UPDATE tasks SET tag = $1, updated_at = now() WHERE id = $2")
            .bind(tag.trim())
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(tag_color) = payload.tag_color {
        sqlx::query("UPDATE tasks SET tag_color = $1, updated_at = now() WHERE id = $2")
            .bind(tag_color.trim())
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(priority) = payload.priority {
        sqlx::query("UPDATE tasks SET priority = $1, updated_at = now() WHERE id = $2")
            .bind(priority_to_db(&priority))
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
    }
    let mut was_done_before_status_change: Option<bool> = None;
    if let Some(status_id) = payload.status_id {
        let status_id = uuid_from_str(&status_id)?;
        let project_id: (Uuid,) = sqlx::query_as("SELECT project_id FROM tasks WHERE id = $1")
            .bind(task_id)
            .fetch_one(&mut *tx)
            .await?;
        assert_status_in_project(&mut *tx, project_id.0, status_id).await?;
        was_done_before_status_change = Some(task_status_is_done(&mut *tx, task_id).await?);
        sqlx::query("UPDATE tasks SET status_id = $1, updated_at = now() WHERE id = $2")
            .bind(status_id)
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(start_date) = payload.start_date {
        sqlx::query("UPDATE tasks SET start_date = $1, updated_at = now() WHERE id = $2")
            .bind(parse_optional_date(start_date.as_deref())?)
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(due_date) = payload.due_date {
        sqlx::query("UPDATE tasks SET due_date = $1, updated_at = now() WHERE id = $2")
            .bind(parse_optional_date(due_date.as_deref())?)
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(phase) = payload.phase {
        sqlx::query("UPDATE tasks SET phase = $1, updated_at = now() WHERE id = $2")
            .bind(phase.trim())
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(recurrence) = payload.recurrence {
        sqlx::query("UPDATE tasks SET recurrence = $1, updated_at = now() WHERE id = $2")
            .bind(recurrence.as_ref().map(recurrence_to_db))
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(assignee_ids) = payload.assignee_ids {
        replace_assignees(&mut tx, task_id, &assignee_ids).await?;
    }

    // After all field updates so a recurrence change in the same PATCH counts.
    let mut spawned_follow_up = false;
    if let Some(was_done) = was_done_before_status_change {
        spawned_follow_up = spawn_recurrence_if_completed(&mut tx, task_id, was_done)
            .await?
            .is_some();
    }

    record_audit(
        &mut *tx,
        workspace_id,
        user_id,
        "updated task",
        "task",
        Some(task_id),
    )
    .await?;
    tx.commit().await?;
    notify_workspace(&state, &headers, workspace_id, "task");
    if spawned_follow_up {
        // Without a client id even the originating tab refetches and sees the
        // spawned follow-up task.
        notify_workspace_all(&state, workspace_id, "task");
    }
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
    let workspace_id = assert_task_edit(&state.db, user_id, task_id).await?;
    let status_id = uuid_from_str(&payload.status_id)?;
    let mut tx = state.db.begin().await?;
    let project_id: (Uuid,) = sqlx::query_as("SELECT project_id FROM tasks WHERE id = $1")
        .bind(task_id)
        .fetch_one(&mut *tx)
        .await?;
    assert_status_in_project(&mut *tx, project_id.0, status_id).await?;
    let was_done = task_status_is_done(&mut *tx, task_id).await?;

    sqlx::query("UPDATE tasks SET status_id = $1, updated_at = now() WHERE id = $2")
        .bind(status_id)
        .bind(task_id)
        .execute(&mut *tx)
        .await?;
    let spawned_follow_up = spawn_recurrence_if_completed(&mut tx, task_id, was_done)
        .await?
        .is_some();
    record_audit(
        &mut *tx,
        workspace_id,
        user_id,
        "moved task",
        "task",
        Some(task_id),
    )
    .await?;
    tx.commit().await?;
    notify_workspace(&state, &headers, workspace_id, "task");
    if spawned_follow_up {
        // Without a client id even the originating tab refetches and sees the
        // spawned follow-up task (drag&drop only patches the moved task locally).
        notify_workspace_all(&state, workspace_id, "task");
    }
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
    let workspace_id = assert_task_edit(&state.db, user_id, task_id).await?;
    let mut tx = state.db.begin().await?;
    // Capture file paths before the cascade deletes the attachment rows, so
    // the files can be removed from disk and don't leak storage forever.
    let storage_paths: Vec<(String,)> =
        sqlx::query_as("SELECT storage_path FROM attachments WHERE task_id = $1")
            .bind(task_id)
            .fetch_all(&mut *tx)
            .await?;
    sqlx::query("DELETE FROM tasks WHERE id = $1")
        .bind(task_id)
        .execute(&mut *tx)
        .await?;
    record_audit(
        &mut *tx,
        workspace_id,
        user_id,
        "deleted task",
        "task",
        Some(task_id),
    )
    .await?;
    tx.commit().await?;
    notify_workspace(&state, &headers, workspace_id, "task");
    for (path,) in storage_paths {
        if let Err(err) = fs::remove_file(&path).await {
            tracing::warn!(%path, %err, "could not remove attachment file of deleted task");
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_ticket(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    let ticket_id = uuid_from_str(&id)?;
    let workspace_id = assert_ticket_edit(&state.db, user_id, ticket_id).await?;
    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM tickets WHERE id = $1")
        .bind(ticket_id)
        .execute(&mut *tx)
        .await?;
    record_audit(
        &mut *tx,
        workspace_id,
        user_id,
        "deleted ticket",
        "ticket",
        Some(ticket_id),
    )
    .await?;
    tx.commit().await?;
    notify_workspace(&state, &headers, workspace_id, "ticket");
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
    let workspace_id = assert_task_edit(&state.db, user_id, task_id).await?;
    if payload.title.trim().is_empty() {
        return Err(AppError::BadRequest("subtask title is required".into()));
    }
    let mut tx = state.db.begin().await?;
    // Single statement so the next position is computed atomically.
    sqlx::query(
        "INSERT INTO subtasks (id, task_id, title, position) \
         SELECT $1, $2, $3, COALESCE(MAX(position), -1) + 1 FROM subtasks WHERE task_id = $2",
    )
    .bind(Uuid::new_v4())
    .bind(task_id)
    .bind(payload.title.trim())
    .execute(&mut *tx)
    .await?;
    touch_task(&mut *tx, task_id).await?;
    record_audit(
        &mut *tx,
        workspace_id,
        user_id,
        "created subtask",
        "subtask",
        Some(task_id),
    )
    .await?;
    tx.commit().await?;
    notify_workspace(&state, &headers, workspace_id, "task");
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
    let workspace_id = assert_task_edit(&state.db, user_id, task_id).await?;

    if let Some(title) = &payload.title {
        if title.trim().is_empty() {
            return Err(AppError::BadRequest("subtask title is required".into()));
        }
    }
    let mut tx = state.db.begin().await?;
    if let Some(title) = payload.title {
        sqlx::query("UPDATE subtasks SET title = $1 WHERE id = $2 AND task_id = $3")
            .bind(title.trim())
            .bind(subtask_id)
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(done) = payload.done {
        sqlx::query("UPDATE subtasks SET done = $1 WHERE id = $2 AND task_id = $3")
            .bind(done)
            .bind(subtask_id)
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
    }
    touch_task(&mut *tx, task_id).await?;
    record_audit(
        &mut *tx,
        workspace_id,
        user_id,
        "updated subtask",
        "subtask",
        Some(subtask_id),
    )
    .await?;
    tx.commit().await?;
    notify_workspace(&state, &headers, workspace_id, "task");
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
    let workspace_id = assert_task_edit(&state.db, user_id, task_id).await?;
    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM subtasks WHERE id = $1 AND task_id = $2")
        .bind(subtask_id)
        .bind(task_id)
        .execute(&mut *tx)
        .await?;
    touch_task(&mut *tx, task_id).await?;
    record_audit(
        &mut *tx,
        workspace_id,
        user_id,
        "deleted subtask",
        "subtask",
        Some(subtask_id),
    )
    .await?;
    tx.commit().await?;
    notify_workspace(&state, &headers, workspace_id, "task");
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
    // Commenting is intentionally open to viewers; only read access is required.
    let workspace_id = assert_task_read(&state.db, user_id, task_id).await?;
    if payload.body.trim().is_empty() {
        return Err(AppError::BadRequest("comment body is required".into()));
    }
    let body = payload.body.trim().to_string();
    let mut tx = state.db.begin().await?;
    sqlx::query("INSERT INTO comments (id, task_id, user_id, body) VALUES ($1, $2, $3, $4)")
        .bind(Uuid::new_v4())
        .bind(task_id)
        .bind(user_id)
        .bind(&body)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE tasks SET comments_count = comments_count + 1, updated_at = now() WHERE id = $1",
    )
    .bind(task_id)
    .execute(&mut *tx)
    .await?;

    let (task_key,): (String,) = sqlx::query_as("SELECT key FROM tasks WHERE id = $1")
        .bind(task_id)
        .fetch_one(&mut *tx)
        .await?;
    let members: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT m.user_id, u.name FROM memberships m JOIN users u ON u.id = m.user_id \
         WHERE m.workspace_id = $1 AND m.status = 'active'",
    )
    .bind(workspace_id)
    .fetch_all(&mut *tx)
    .await?;
    let mentioned = mentioned_user_ids(&body, &members);
    for &target in mentioned.iter().filter(|&&id| id != user_id) {
        insert_notification(
            &mut *tx,
            workspace_id,
            target,
            &NotificationKind::Mention,
            user_id,
            Some(task_id),
            &format!("hat dich in {task_key} erwähnt"),
            &format!("mentioned you in {task_key}"),
        )
        .await?;
    }
    // Everyone else involved with the task (assignees and earlier commenters)
    // gets a plain comment notification instead.
    let participants: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT user_id FROM task_assignees WHERE task_id = $1 \
         UNION SELECT user_id FROM comments WHERE task_id = $1",
    )
    .bind(task_id)
    .fetch_all(&mut *tx)
    .await?;
    for (target,) in participants {
        if target == user_id || mentioned.contains(&target) {
            continue;
        }
        // Old assignees/commenters may have left the workspace since;
        // `members` holds the active memberships.
        if !members.iter().any(|(id, _)| *id == target) {
            continue;
        }
        insert_notification(
            &mut *tx,
            workspace_id,
            target,
            &NotificationKind::Comment,
            user_id,
            Some(task_id),
            &format!("hat {task_key} kommentiert"),
            &format!("commented on {task_key}"),
        )
        .await?;
    }

    record_audit(
        &mut *tx,
        workspace_id,
        user_id,
        "commented",
        "task",
        Some(task_id),
    )
    .await?;
    tx.commit().await?;
    notify_workspace(&state, &headers, workspace_id, "comment");
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
    notify_workspace(&state, &headers, workspace_id, "attachment");
    Ok(Json(fetch_task(&state.db, task_id).await?))
}

async fn store_attachments(
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
        "SELECT COALESCE(SUM(a.size_bytes), 0) \
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
        let storage_name = format!("{}-{}", attachment_id, file_name);
        let storage_path = state.cfg.upload_dir.join(&storage_name);
        let mut file = fs::File::create(&storage_path).await?;
        written_paths.push(storage_path.clone());
        let mut size_bytes: u64 = 0;
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
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        drop(file);
        if size_bytes == 0 {
            let _ = fs::remove_file(&storage_path).await;
            written_paths.pop();
            continue;
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
struct InlineQuery {
    #[serde(default)]
    inline: Option<String>,
}

async fn download_attachment(
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

/// Fire-and-forget realtime fanout to the workspace. The originating tab
/// identifies itself via the X-Client-Id header so it can skip refetching its
/// own (already locally applied) mutation. send() only errs when no socket is
/// connected, which is fine to ignore.
fn notify_workspace(state: &AppState, headers: &HeaderMap, workspace_id: Uuid, topic: &str) {
    let client_id = headers
        .get("x-client-id")
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.is_empty() && v.len() <= 64)
        .map(str::to_string);
    let _ = state.events.send(WorkspaceEventDto {
        workspace_id: workspace_id.to_string(),
        topic: topic.to_string(),
        client_id,
    });
}

/// Like notify_workspace, but without echo suppression: every tab refetches,
/// including the one that caused the change (used when the server created
/// additional data the originator does not know about, e.g. a recurring
/// follow-up task).
fn notify_workspace_all(state: &AppState, workspace_id: Uuid, topic: &str) {
    let _ = state.events.send(WorkspaceEventDto {
        workspace_id: workspace_id.to_string(),
        topic: topic.to_string(),
        client_id: None,
    });
}

#[derive(Debug, Deserialize)]
struct WsQuery {
    #[serde(default)]
    client_id: Option<String>,
}

async fn ws_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, AppError> {
    // The handshake is a GET, so enforce_same_origin skips it — but browsers
    // do not apply CORS to WebSockets, so a foreign page could otherwise open
    // a cookie-authenticated socket (cross-site WebSocket hijacking).
    if !same_origin(&state.cfg, &headers) {
        return Err(AppError::Forbidden);
    }
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    // Same scoping rule as fetch_bootstrap: the first active membership
    // decides which workspace this connection belongs to.
    let membership: Option<(Uuid,)> = sqlx::query_as(
        "SELECT workspace_id FROM memberships \
         WHERE user_id = $1 AND status = 'active' ORDER BY created_at ASC LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;
    let (workspace_id,) = membership.ok_or(AppError::Forbidden)?;
    let client_id = query
        .client_id
        .filter(|v| !v.is_empty() && v.len() <= 64);
    Ok(ws.on_upgrade(move |socket| ws_loop(socket, state, workspace_id, client_id)))
}

async fn ws_loop(
    socket: WebSocket,
    state: AppState,
    workspace_id: Uuid,
    client_id: Option<String>,
) {
    use futures_util::{SinkExt, StreamExt};

    let (mut sink, mut stream) = socket.split();
    let mut events = state.events.subscribe();
    let workspace = workspace_id.to_string();
    // Keeps the connection alive through nginx's proxy_read_timeout.
    let mut ping = tokio::time::interval(StdDuration::from_secs(30));
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ping.tick().await; // the first tick fires immediately

    loop {
        tokio::select! {
            _ = ping.tick() => {
                if sink.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
            incoming = stream.next() => {
                match incoming {
                    None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                    Some(Ok(_)) => {}
                }
            }
            event = events.recv() => {
                let event = match event {
                    Ok(event) => {
                        if event.workspace_id != workspace {
                            continue;
                        }
                        // Skip the echo of this tab's own mutation.
                        if event.client_id.is_some() && event.client_id == client_id {
                            continue;
                        }
                        event
                    }
                    // This receiver fell behind and missed events; tell the
                    // client to refetch instead of dropping the connection.
                    Err(broadcast::error::RecvError::Lagged(_)) => WorkspaceEventDto {
                        workspace_id: workspace.clone(),
                        topic: "resync".to_string(),
                        client_id: None,
                    },
                    Err(broadcast::error::RecvError::Closed) => break,
                };
                let Ok(json) = serde_json::to_string(&event) else {
                    continue;
                };
                if sink.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        }
    }
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
    record_audit(
        &state.db,
        workspace_id,
        user_id,
        "updated workspace",
        "workspace",
        Some(workspace_id),
    )
    .await?;
    notify_workspace(&state, &headers, workspace_id, "workspace");
    Ok(Json(fetch_workspace(&state.db, workspace_id).await?))
}

async fn invite_member(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<InviteMemberRequest>,
) -> Result<Json<InviteMemberResponse>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    let workspace_id = uuid_from_str(&id)?;
    assert_workspace_admin(&state.db, user_id, workspace_id).await?;
    let email = payload.email.trim().to_lowercase();
    if !email.contains('@') {
        return Err(AppError::BadRequest("invite email is invalid".into()));
    }
    if matches!(payload.role, Role::Owner) {
        return Err(AppError::BadRequest(
            "cannot invite a member as owner".into(),
        ));
    }
    let already_invited: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM workspace_invites WHERE workspace_id = $1 AND email = $2")
            .bind(workspace_id)
            .bind(&email)
            .fetch_optional(&state.db)
            .await?;
    if already_invited.is_some() {
        return Err(AppError::Conflict("invite already exists".into()));
    }
    let already_member: Option<(Uuid,)> = sqlx::query_as(
        "SELECT m.id FROM memberships m JOIN users u ON u.id = m.user_id \
         WHERE m.workspace_id = $1 AND u.email = $2",
    )
    .bind(workspace_id)
    .bind(&email)
    .fetch_optional(&state.db)
    .await?;
    if already_member.is_some() {
        return Err(AppError::Conflict("user is already a member".into()));
    }
    // Existing users can never redeem an invite row (redemption happens at
    // registration), so add them as members directly instead.
    let existing_user: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM users WHERE email = $1")
        .bind(&email)
        .fetch_optional(&state.db)
        .await?;
    if let Some((invitee_id,)) = existing_user {
        let mut tx = state.db.begin().await?;
        sqlx::query(
            "INSERT INTO memberships (id, workspace_id, user_id, role, status) \
             VALUES ($1, $2, $3, $4, 'active') \
             ON CONFLICT (workspace_id, user_id) DO NOTHING",
        )
        .bind(Uuid::new_v4())
        .bind(workspace_id)
        .bind(invitee_id)
        .bind(role_to_db(&payload.role))
        .execute(&mut *tx)
        .await?;
        record_audit(
            &mut *tx,
            workspace_id,
            user_id,
            "added member",
            "membership",
            Some(invitee_id),
        )
        .await?;
        tx.commit().await?;
        notify_workspace(&state, &headers, workspace_id, "workspace");
        return Ok(Json(InviteMemberResponse {
            invite_token: None,
            invite_path: None,
        }));
    }
    // Single-use random token; only its hash is stored, so a database leak
    // does not expose redeemable invites.
    let token = generate_invite_token();
    sqlx::query(
        "INSERT INTO workspace_invites (id, workspace_id, email, role, invited_by, token_hash, expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(Uuid::new_v4())
    .bind(workspace_id)
    .bind(email)
    .bind(role_to_db(&payload.role))
    .bind(user_id)
    .bind(invite_token_hash(&token))
    .bind(Utc::now() + Duration::days(INVITE_TTL_DAYS))
    .execute(&state.db)
    .await?;
    record_audit(
        &state.db,
        workspace_id,
        user_id,
        "invited member",
        "workspace",
        Some(workspace_id),
    )
    .await?;
    Ok(Json(InviteMemberResponse {
        invite_path: Some(format!("/?invite={token}")),
        invite_token: Some(token),
    }))
}

fn generate_invite_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn invite_token_hash(token: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(token.as_bytes()))
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
    let mut tx = state.db.begin().await?;
    // FOR UPDATE serializes concurrent role changes on the same membership and
    // keeps the last-owner check below race-free.
    let row: MembershipWorkspaceRow = sqlx::query_as(
        "SELECT workspace_id, user_id, role FROM memberships WHERE id = $1 FOR UPDATE",
    )
    .bind(membership_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;
    let actor_role = workspace_role(&state.db, user_id, row.workspace_id)
        .await?
        .ok_or(AppError::Forbidden)?;
    if !actor_role.can_admin() {
        return Err(AppError::Forbidden);
    }
    let target_role = role_from_db(&row.role)?;
    // Only owners may touch owner memberships or hand out the owner role.
    if (target_role == Role::Owner || payload.role == Role::Owner) && actor_role != Role::Owner {
        return Err(AppError::Forbidden);
    }
    if target_role == Role::Owner && payload.role != Role::Owner {
        let owners: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM memberships \
             WHERE workspace_id = $1 AND role = 'owner' AND status = 'active' FOR UPDATE",
        )
        .bind(row.workspace_id)
        .fetch_all(&mut *tx)
        .await?;
        if owners.len() <= 1 {
            return Err(AppError::BadRequest(
                "cannot demote the last owner of the workspace".into(),
            ));
        }
    }
    if row.user_id == user_id && matches!(payload.role, Role::Viewer) {
        return Err(AppError::BadRequest(
            "cannot demote yourself to viewer".into(),
        ));
    }
    sqlx::query("UPDATE memberships SET role = $1 WHERE id = $2")
        .bind(role_to_db(&payload.role))
        .bind(membership_id)
        .execute(&mut *tx)
        .await?;
    record_audit(
        &mut *tx,
        row.workspace_id,
        user_id,
        "updated role",
        "membership",
        Some(membership_id),
    )
    .await?;
    tx.commit().await?;
    notify_workspace(&state, &headers, row.workspace_id, "workspace");
    Ok(Json(fetch_member(&state.db, membership_id).await?))
}

async fn remove_membership(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let user_id = uuid_from_str(&ctx.user.id)?;
    let membership_id = uuid_from_str(&id)?;
    let mut tx = state.db.begin().await?;
    let row: MembershipWorkspaceRow = sqlx::query_as(
        "SELECT workspace_id, user_id, role FROM memberships WHERE id = $1 FOR UPDATE",
    )
    .bind(membership_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;
    let actor_role = workspace_role(&state.db, user_id, row.workspace_id)
        .await?
        .ok_or(AppError::Forbidden)?;
    if !actor_role.can_admin() {
        return Err(AppError::Forbidden);
    }
    let target_role = role_from_db(&row.role)?;
    if target_role == Role::Owner && actor_role != Role::Owner {
        return Err(AppError::Forbidden);
    }
    if row.user_id == user_id {
        return Err(AppError::BadRequest(
            "cannot remove your own membership".into(),
        ));
    }
    if target_role == Role::Owner {
        let owners: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM memberships \
             WHERE workspace_id = $1 AND role = 'owner' AND status = 'active' FOR UPDATE",
        )
        .bind(row.workspace_id)
        .fetch_all(&mut *tx)
        .await?;
        if owners.len() <= 1 {
            return Err(AppError::BadRequest(
                "cannot remove the last owner of the workspace".into(),
            ));
        }
    }
    sqlx::query(
        "DELETE FROM task_assignees ta USING tasks t, projects p \
         WHERE ta.task_id = t.id AND t.project_id = p.id \
         AND p.workspace_id = $1 AND ta.user_id = $2",
    )
    .bind(row.workspace_id)
    .bind(row.user_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE tickets t SET assignee_id = NULL FROM projects p \
         WHERE t.project_id = p.id AND p.workspace_id = $1 AND t.assignee_id = $2",
    )
    .bind(row.workspace_id)
    .bind(row.user_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM memberships WHERE id = $1")
        .bind(membership_id)
        .execute(&mut *tx)
        .await?;
    record_audit(
        &mut *tx,
        row.workspace_id,
        user_id,
        "removed member",
        "membership",
        Some(membership_id),
    )
    .await?;
    tx.commit().await?;
    notify_workspace(&state, &headers, row.workspace_id, "workspace");
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let actor_id = uuid_from_str(&ctx.user.id)?;
    let target_id = uuid_from_str(&id)?;
    if actor_id == target_id {
        return Err(AppError::BadRequest(
            "cannot delete your own account".into(),
        ));
    }

    // The actor must own a workspace the target is a member of; a Forbidden
    // for unknown target ids keeps this from acting as a user-id oracle.
    let shared_workspace: (Uuid,) = sqlx::query_as(
        "SELECT a.workspace_id \
         FROM memberships a \
         JOIN memberships t ON t.workspace_id = a.workspace_id \
         WHERE a.user_id = $1 AND a.role = 'owner' AND a.status = 'active' \
         AND t.user_id = $2 AND t.status = 'active' \
         ORDER BY a.created_at ASC LIMIT 1",
    )
    .bind(actor_id)
    .bind(target_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::Forbidden)?;

    let mut tx = state.db.begin().await?;
    let target_memberships: Vec<MembershipWorkspaceRow> = sqlx::query_as(
        "SELECT workspace_id, user_id, role \
         FROM memberships WHERE user_id = $1 AND status = 'active' FOR UPDATE",
    )
    .bind(target_id)
    .fetch_all(&mut *tx)
    .await?;

    let mut workspaces_to_delete = Vec::new();
    for membership in &target_memberships {
        if role_from_db(&membership.role)? != Role::Owner {
            continue;
        }
        let owners: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM memberships \
             WHERE workspace_id = $1 AND role = 'owner' AND status = 'active' FOR UPDATE",
        )
        .bind(membership.workspace_id)
        .fetch_all(&mut *tx)
        .await?;
        if owners.len() > 1 {
            continue;
        }
        let (members,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM memberships WHERE workspace_id = $1 AND status = 'active'",
        )
        .bind(membership.workspace_id)
        .fetch_one(&mut *tx)
        .await?;
        if members <= 1 {
            workspaces_to_delete.push(membership.workspace_id);
        } else {
            return Err(AppError::BadRequest(
                "cannot delete user because they are the last owner of another workspace".into(),
            ));
        }
    }

    if !workspaces_to_delete.is_empty() {
        sqlx::query("DELETE FROM workspaces WHERE id = ANY($1)")
            .bind(&workspaces_to_delete)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(target_id)
        .execute(&mut *tx)
        .await?;
    record_audit(
        &mut *tx,
        shared_workspace.0,
        actor_id,
        "deleted user",
        "user",
        Some(target_id),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
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
        .find_map(|(name, value)| (name == cookie_name(cfg)).then_some(value))
        .ok_or(AppError::Unauthorized)?;

    let (session_id, signature) = raw.rsplit_once('.').ok_or(AppError::Unauthorized)?;
    let sig_bytes = URL_SAFE_NO_PAD
        .decode(signature)
        .map_err(|_| AppError::Unauthorized)?;
    let mut mac = HmacSha256::new_from_slice(cfg.session_secret.as_bytes())
        .map_err(|_| AppError::Unauthorized)?;
    mac.update(session_id.as_bytes());
    // Constant-time comparison; a plain `==` would leak timing information.
    mac.verify_slice(&sig_bytes)
        .map_err(|_| AppError::Unauthorized)?;

    uuid_from_str(session_id)
}

fn sign(cfg: &AppConfig, value: &str) -> Result<String, AppError> {
    let mut mac = HmacSha256::new_from_slice(cfg.session_secret.as_bytes())
        .map_err(|_| AppError::Internal("invalid session secret".into()))?;
    mac.update(value.as_bytes());
    Ok(URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
}

fn cookie_name(cfg: &AppConfig) -> &'static str {
    if cfg.cookie_secure {
        SECURE_COOKIE_NAME
    } else {
        COOKIE_NAME
    }
}

fn build_cookie(cfg: &AppConfig, session_id: Uuid) -> Result<String, AppError> {
    let id = session_id.to_string();
    let signed = format!("{}.{}", id, sign(cfg, &id)?);
    let secure = if cfg.cookie_secure { "; Secure" } else { "" };
    Ok(format!(
        "{}={signed}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{}",
        cookie_name(cfg),
        14 * 24 * 60 * 60,
        secure
    ))
}

fn expired_cookie(cfg: &AppConfig) -> String {
    let secure = if cfg.cookie_secure { "; Secure" } else { "" };
    format!(
        "{}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure}",
        cookie_name(cfg)
    )
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

/// Runs Argon2 hashing on a blocking thread, bounded by the global permit
/// pool, so request floods cannot stall the async runtime or pin all cores.
async fn hash_password_async(state: &AppState, password: String) -> Result<String, AppError> {
    let _permit = state
        .hash_permits
        .acquire()
        .await
        .map_err(|_| AppError::Internal("hash semaphore closed".into()))?;
    tokio::task::spawn_blocking(move || hash_password(&password))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
}

async fn verify_password_async(
    state: &AppState,
    password: String,
    hash: String,
) -> Result<(), AppError> {
    let _permit = state
        .hash_permits
        .acquire()
        .await
        .map_err(|_| AppError::Internal("hash semaphore closed".into()))?;
    tokio::task::spawn_blocking(move || verify_password(&password, &hash))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
}

fn hash_password(password: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| AppError::Internal(e.to_string()))?
        .to_string())
}

fn verify_password(password: &str, hash: &str) -> Result<(), AppError> {
    let parsed = PasswordHash::new(hash).map_err(|_| AppError::Unauthorized)?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .map_err(|_| AppError::Unauthorized)
}

async fn create_session(conn: &mut PgConnection, user_id: Uuid) -> Result<Uuid, AppError> {
    // Opportunistic cleanup keeps the sessions table from growing without bound.
    sqlx::query("DELETE FROM sessions WHERE expires_at < now()")
        .execute(&mut *conn)
        .await?;
    let session_id = Uuid::new_v4();
    let expires_at = Utc::now() + Duration::days(14);
    sqlx::query("INSERT INTO sessions (id, user_id, expires_at) VALUES ($1, $2, $3)")
        .bind(session_id)
        .bind(user_id)
        .bind(expires_at)
        .execute(&mut *conn)
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

#[derive(Debug, FromRow)]
struct RegisteredUserRow {
    id: Uuid,
    email: String,
    name: String,
    created_at: DateTime<Utc>,
    membership_id: Option<Uuid>,
    role: Option<String>,
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
    is_done: bool,
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
            is_done: row.is_done,
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
    status_is_done: bool,
    start_date: Option<NaiveDate>,
    due_date: Option<NaiveDate>,
    phase: String,
    recurrence: Option<String>,
    comments_count: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct TicketRow {
    id: Uuid,
    project_id: Uuid,
    key: String,
    title: String,
    description: String,
    status: String,
    priority: String,
    requester_name: String,
    assignee_id: Option<Uuid>,
    assignee_name: Option<String>,
    created_by_name: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct SubtaskRow {
    id: Uuid,
    task_id: Uuid,
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
    let user = fetch_user(db, user_id).await?;
    let membership: MembershipWorkspaceRow = sqlx::query_as(
        "SELECT workspace_id, user_id, role \
         FROM memberships WHERE user_id = $1 AND status = 'active' ORDER BY created_at ASC LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(db)
    .await?
    .ok_or(AppError::NotFound)?;

    sqlx::query(
        "UPDATE memberships SET last_active_at = now() WHERE user_id = $1 AND workspace_id = $2",
    )
    .bind(user_id)
    .bind(membership.workspace_id)
    .execute(db)
    .await?;

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
    let tickets = fetch_tickets(db, project_id).await?;
    let milestones = fetch_milestones(db, project_id).await?;
    let notifications = fetch_notifications(db, user_id).await?;
    let audit_events = fetch_audit_events(db, membership.workspace_id).await?;
    let current_role = role_from_db(&membership.role)?;
    let registered_users = if current_role.can_admin() {
        fetch_registered_users(db, membership.workspace_id).await?
    } else {
        Vec::new()
    };

    Ok(BootstrapDto {
        current_user: user,
        workspace,
        project,
        current_role,
        members,
        registered_users,
        statuses,
        tasks,
        tickets,
        milestones,
        notifications,
        audit_events,
    })
}

async fn fetch_statuses(db: &PgPool, project_id: Uuid) -> Result<Vec<StatusDto>, AppError> {
    let rows: Vec<StatusRow> = sqlx::query_as(
        "SELECT id, project_id, name_de, name_en, position, is_done, color \
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

    // One aggregate query for the whole workspace instead of two counts per member.
    let count_rows: Vec<(Uuid, i64, i64)> = sqlx::query_as(
        "SELECT ta.user_id, \
                COUNT(*) FILTER (WHERE NOT s.is_done), \
                COUNT(*) FILTER (WHERE s.is_done) \
         FROM task_assignees ta \
         JOIN tasks t ON t.id = ta.task_id \
         JOIN projects p ON p.id = t.project_id \
         JOIN project_statuses s ON s.id = t.status_id \
         WHERE p.workspace_id = $1 GROUP BY ta.user_id",
    )
    .bind(workspace_id)
    .fetch_all(db)
    .await?;
    let counts: HashMap<Uuid, (i64, i64)> = count_rows
        .into_iter()
        .map(|(user_id, open, done)| (user_id, (open, done)))
        .collect();

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let (open_tasks, done_tasks) = counts.get(&row.user_id).copied().unwrap_or((0, 0));

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
            open_tasks,
            done_tasks,
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

async fn fetch_registered_users(
    db: &PgPool,
    workspace_id: Uuid,
) -> Result<Vec<RegisteredUserDto>, AppError> {
    let rows: Vec<RegisteredUserRow> = sqlx::query_as(
        "SELECT u.id, u.email, u.name, u.created_at, \
                m.id AS membership_id, m.role \
         FROM users u \
         JOIN memberships m ON m.user_id = u.id \
         WHERE m.workspace_id = $1 AND m.status = 'active' \
         ORDER BY u.created_at DESC, u.email",
    )
    .bind(workspace_id)
    .fetch_all(db)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(RegisteredUserDto {
                id: row.id.to_string(),
                email: row.email,
                initials: initials(&row.name),
                name: row.name,
                membership_id: row.membership_id.map(|id| id.to_string()),
                role: row.role.as_deref().map(role_from_db).transpose()?,
                created_label_de: relative_label(row.created_at, "de"),
                created_label_en: relative_label(row.created_at, "en"),
            })
        })
        .collect()
}

const TASK_SELECT: &str =
    "SELECT t.id, t.project_id, t.key, t.title, t.title_en, t.description, t.description_en, \
            t.tag, t.tag_color, t.priority, t.status_id, s.position AS status_position, \
            s.is_done AS status_is_done, \
            t.start_date, t.due_date, t.phase, t.recurrence, t.comments_count, \
            t.created_at, t.updated_at \
     FROM tasks t JOIN project_statuses s ON s.id = t.status_id";

async fn fetch_tasks(db: &PgPool, project_id: Uuid) -> Result<Vec<TaskDto>, AppError> {
    let rows: Vec<TaskRow> = sqlx::query_as(&format!(
        "{TASK_SELECT} WHERE t.project_id = $1 ORDER BY s.position, t.due_date NULLS LAST, t.key"
    ))
    .bind(project_id)
    .fetch_all(db)
    .await?;
    assemble_tasks(db, rows).await
}

async fn fetch_task(db: &PgPool, task_id: Uuid) -> Result<TaskDto, AppError> {
    let row: TaskRow = sqlx::query_as(&format!("{TASK_SELECT} WHERE t.id = $1"))
        .bind(task_id)
        .fetch_optional(db)
        .await?
        .ok_or(AppError::NotFound)?;
    assemble_tasks(db, vec![row])
        .await?
        .pop()
        .ok_or(AppError::NotFound)
}

const TICKET_SELECT: &str =
    "SELECT t.id, t.project_id, t.key, t.title, t.description, t.status, t.priority, \
            t.requester_name, t.assignee_id, au.name AS assignee_name, \
            cu.name AS created_by_name, t.created_at, t.updated_at \
     FROM tickets t \
     LEFT JOIN users au ON au.id = t.assignee_id \
     LEFT JOIN users cu ON cu.id = t.created_by";

async fn fetch_tickets(db: &PgPool, project_id: Uuid) -> Result<Vec<TicketDto>, AppError> {
    let rows: Vec<TicketRow> = sqlx::query_as(&format!(
        "{TICKET_SELECT} WHERE t.project_id = $1 ORDER BY t.updated_at DESC, t.key DESC"
    ))
    .bind(project_id)
    .fetch_all(db)
    .await?;
    rows.into_iter().map(ticket_from_row).collect()
}

async fn fetch_ticket(db: &PgPool, ticket_id: Uuid) -> Result<TicketDto, AppError> {
    let row: TicketRow = sqlx::query_as(&format!("{TICKET_SELECT} WHERE t.id = $1"))
        .bind(ticket_id)
        .fetch_optional(db)
        .await?
        .ok_or(AppError::NotFound)?;
    ticket_from_row(row)
}

fn ticket_from_row(row: TicketRow) -> Result<TicketDto, AppError> {
    Ok(TicketDto {
        id: row.id.to_string(),
        project_id: row.project_id.to_string(),
        key: row.key,
        title: row.title,
        description: row.description,
        status: ticket_status_from_db(&row.status)?,
        priority: priority_from_db(&row.priority)?,
        requester_name: row.requester_name,
        assignee_id: row.assignee_id.map(|id| id.to_string()),
        assignee_name: row.assignee_name,
        created_by_name: row.created_by_name,
        created_label_de: relative_label(row.created_at, "de"),
        created_label_en: relative_label(row.created_at, "en"),
        updated_label_de: relative_label(row.updated_at, "de"),
        updated_label_en: relative_label(row.updated_at, "en"),
    })
}

/// Loads all task children (assignees, dependencies, subtasks, comments,
/// attachments) with one batched query each instead of one set per task.
async fn assemble_tasks(db: &PgPool, rows: Vec<TaskRow>) -> Result<Vec<TaskDto>, AppError> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();

    let assignee_rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT task_id, user_id FROM task_assignees WHERE task_id = ANY($1) ORDER BY user_id",
    )
    .bind(&ids)
    .fetch_all(db)
    .await?;
    let mut assignees: HashMap<Uuid, Vec<String>> = HashMap::new();
    for (task_id, user_id) in assignee_rows {
        assignees
            .entry(task_id)
            .or_default()
            .push(user_id.to_string());
    }

    let dependency_rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT task_id, depends_on_task_id FROM task_dependencies \
         WHERE task_id = ANY($1) ORDER BY depends_on_task_id",
    )
    .bind(&ids)
    .fetch_all(db)
    .await?;
    let mut dependencies: HashMap<Uuid, Vec<String>> = HashMap::new();
    for (task_id, dep_id) in dependency_rows {
        dependencies
            .entry(task_id)
            .or_default()
            .push(dep_id.to_string());
    }

    let subtask_rows: Vec<SubtaskRow> = sqlx::query_as(
        "SELECT id, task_id, title, title_en, done, position FROM subtasks \
         WHERE task_id = ANY($1) ORDER BY position",
    )
    .bind(&ids)
    .fetch_all(db)
    .await?;
    let mut subtasks: HashMap<Uuid, Vec<SubtaskDto>> = HashMap::new();
    for s in subtask_rows {
        subtasks.entry(s.task_id).or_default().push(SubtaskDto {
            id: s.id.to_string(),
            title: s.title,
            title_en: s.title_en,
            done: s.done,
            position: s.position,
        });
    }

    let comment_rows: Vec<CommentRow> = sqlx::query_as(
        "SELECT c.id, c.task_id, c.user_id, u.name AS author_name, c.body, c.created_at \
         FROM comments c JOIN users u ON u.id = c.user_id \
         WHERE c.task_id = ANY($1) ORDER BY c.created_at DESC",
    )
    .bind(&ids)
    .fetch_all(db)
    .await?;
    let mut comments: HashMap<Uuid, Vec<CommentDto>> = HashMap::new();
    for c in comment_rows {
        comments.entry(c.task_id).or_default().push(CommentDto {
            id: c.id.to_string(),
            task_id: c.task_id.to_string(),
            user_id: c.user_id.to_string(),
            author_initials: initials(&c.author_name),
            author_name: c.author_name,
            body: c.body,
            created_label_de: relative_label(c.created_at, "de"),
            created_label_en: relative_label(c.created_at, "en"),
        });
    }

    let attachment_rows: Vec<AttachmentRow> = sqlx::query_as(
        "SELECT id, task_id, file_name, kind, size_bytes FROM attachments \
         WHERE task_id = ANY($1) ORDER BY created_at DESC",
    )
    .bind(&ids)
    .fetch_all(db)
    .await?;
    let mut attachments: HashMap<Uuid, Vec<AttachmentDto>> = HashMap::new();
    for a in attachment_rows {
        attachments
            .entry(a.task_id)
            .or_default()
            .push(AttachmentDto {
                id: a.id.to_string(),
                task_id: a.task_id.to_string(),
                file_name: a.file_name,
                kind: attachment_kind_from_db(&a.kind).unwrap_or(AttachmentKind::File),
                size_label: size_label(a.size_bytes),
            });
    }

    rows.into_iter()
        .map(|row| {
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
                status_is_done: row.status_is_done,
                start_date: row.start_date.map(|d| d.to_string()),
                due_date: row.due_date.map(|d| d.to_string()),
                phase: row.phase,
                recurrence: row
                    .recurrence
                    .as_deref()
                    .map(recurrence_from_db)
                    .transpose()?,
                assignee_ids: assignees.remove(&row.id).unwrap_or_default(),
                dependency_ids: dependencies.remove(&row.id).unwrap_or_default(),
                subtasks: subtasks.remove(&row.id).unwrap_or_default(),
                comments: comments.remove(&row.id).unwrap_or_default(),
                attachments: attachments.remove(&row.id).unwrap_or_default(),
                comments_count: row.comments_count,
                created_label_de: relative_label(row.created_at, "de"),
                created_label_en: relative_label(row.created_at, "en"),
                updated_label_de: relative_label(row.updated_at, "de"),
                updated_label_en: relative_label(row.updated_at, "en"),
            })
        })
        .collect()
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

async fn project_access(
    db: &PgPool,
    user_id: Uuid,
    project_id: Uuid,
) -> Result<(Uuid, Role), AppError> {
    let row: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT p.workspace_id, m.role \
         FROM projects p JOIN memberships m ON m.workspace_id = p.workspace_id \
         WHERE p.id = $1 AND m.user_id = $2 AND m.status = 'active'",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(db)
    .await?;
    let Some((workspace_id, role)) = row else {
        return Err(AppError::Forbidden);
    };
    Ok((workspace_id, role_from_db(&role)?))
}

async fn assert_project_edit(
    db: &PgPool,
    user_id: Uuid,
    project_id: Uuid,
) -> Result<Uuid, AppError> {
    let (workspace_id, role) = project_access(db, user_id, project_id).await?;
    if !role.can_edit() {
        return Err(AppError::Forbidden);
    }
    Ok(workspace_id)
}

async fn task_access(db: &PgPool, user_id: Uuid, task_id: Uuid) -> Result<(Uuid, Role), AppError> {
    let row: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT p.workspace_id, m.role \
         FROM tasks t JOIN projects p ON p.id = t.project_id \
         JOIN memberships m ON m.workspace_id = p.workspace_id \
         WHERE t.id = $1 AND m.user_id = $2 AND m.status = 'active'",
    )
    .bind(task_id)
    .bind(user_id)
    .fetch_optional(db)
    .await?;
    let Some((workspace_id, role)) = row else {
        return Err(AppError::Forbidden);
    };
    Ok((workspace_id, role_from_db(&role)?))
}

async fn assert_task_read(db: &PgPool, user_id: Uuid, task_id: Uuid) -> Result<Uuid, AppError> {
    let (workspace_id, _) = task_access(db, user_id, task_id).await?;
    Ok(workspace_id)
}

async fn assert_task_edit(db: &PgPool, user_id: Uuid, task_id: Uuid) -> Result<Uuid, AppError> {
    let (workspace_id, role) = task_access(db, user_id, task_id).await?;
    if !role.can_edit() {
        return Err(AppError::Forbidden);
    }
    Ok(workspace_id)
}

async fn ticket_access(
    db: &PgPool,
    user_id: Uuid,
    ticket_id: Uuid,
) -> Result<(Uuid, Role), AppError> {
    let row: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT p.workspace_id, m.role \
         FROM tickets t JOIN projects p ON p.id = t.project_id \
         JOIN memberships m ON m.workspace_id = p.workspace_id \
         WHERE t.id = $1 AND m.user_id = $2 AND m.status = 'active'",
    )
    .bind(ticket_id)
    .bind(user_id)
    .fetch_optional(db)
    .await?;
    let Some((workspace_id, role)) = row else {
        return Err(AppError::Forbidden);
    };
    Ok((workspace_id, role_from_db(&role)?))
}

async fn assert_ticket_read(db: &PgPool, user_id: Uuid, ticket_id: Uuid) -> Result<Uuid, AppError> {
    let (workspace_id, _) = ticket_access(db, user_id, ticket_id).await?;
    Ok(workspace_id)
}

async fn assert_ticket_edit(db: &PgPool, user_id: Uuid, ticket_id: Uuid) -> Result<Uuid, AppError> {
    let (workspace_id, role) = ticket_access(db, user_id, ticket_id).await?;
    if !role.can_edit() {
        return Err(AppError::Forbidden);
    }
    Ok(workspace_id)
}

async fn assert_status_in_project(
    exec: impl sqlx::PgExecutor<'_>,
    project_id: Uuid,
    status_id: Uuid,
) -> Result<(), AppError> {
    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM project_statuses WHERE id = $1 AND project_id = $2")
            .bind(status_id)
            .bind(project_id)
            .fetch_one(exec)
            .await?;
    if count.0 == 0 {
        return Err(AppError::BadRequest(
            "status does not belong to project".into(),
        ));
    }
    Ok(())
}

async fn assert_user_in_project(
    exec: impl sqlx::PgExecutor<'_>,
    project_id: Uuid,
    user_id: Uuid,
) -> Result<(), AppError> {
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM memberships m \
         JOIN projects p ON p.workspace_id = m.workspace_id \
         WHERE p.id = $1 AND m.user_id = $2 AND m.status = 'active'",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_one(exec)
    .await?;
    if count.0 == 0 {
        return Err(AppError::BadRequest(
            "assignee is not an active workspace member".into(),
        ));
    }
    Ok(())
}

async fn workspace_role(
    db: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<Option<Role>, AppError> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT role FROM memberships WHERE user_id = $1 AND workspace_id = $2 AND status = 'active'")
            .bind(user_id)
            .bind(workspace_id)
            .fetch_optional(db)
            .await?;
    row.map(|(role,)| role_from_db(&role)).transpose()
}

async fn assert_workspace_admin(
    db: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<(), AppError> {
    let role = workspace_role(db, user_id, workspace_id)
        .await?
        .ok_or(AppError::Forbidden)?;
    if !role.can_admin() {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

async fn replace_assignees(
    conn: &mut PgConnection,
    task_id: Uuid,
    assignee_ids: &[String],
) -> Result<(), AppError> {
    let mut ids = assignee_ids
        .iter()
        .map(|id| uuid_from_str(id))
        .collect::<Result<Vec<_>, _>>()?;
    ids.sort_unstable();
    ids.dedup();

    if !ids.is_empty() {
        let valid: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM memberships m \
             JOIN projects p ON p.workspace_id = m.workspace_id \
             JOIN tasks t ON t.project_id = p.id \
             WHERE t.id = $1 AND m.status = 'active' AND m.user_id = ANY($2)",
        )
        .bind(task_id)
        .bind(&ids)
        .fetch_one(&mut *conn)
        .await?;
        if valid.0 != ids.len() as i64 {
            return Err(AppError::BadRequest(
                "assignee is not an active workspace member".into(),
            ));
        }
    }

    sqlx::query("DELETE FROM task_assignees WHERE task_id = $1")
        .bind(task_id)
        .execute(&mut *conn)
        .await?;
    for user_id in &ids {
        sqlx::query("INSERT INTO task_assignees (task_id, user_id) VALUES ($1, $2)")
            .bind(task_id)
            .bind(user_id)
            .execute(&mut *conn)
            .await?;
    }
    touch_task(&mut *conn, task_id).await?;
    Ok(())
}

async fn touch_task(exec: impl sqlx::PgExecutor<'_>, task_id: Uuid) -> Result<(), AppError> {
    sqlx::query("UPDATE tasks SET updated_at = now() WHERE id = $1")
        .bind(task_id)
        .execute(exec)
        .await?;
    Ok(())
}

async fn record_audit(
    exec: impl sqlx::PgExecutor<'_>,
    workspace_id: Uuid,
    actor_id: Uuid,
    action: &str,
    entity: &str,
    entity_id: Option<Uuid>,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO audit_events (id, workspace_id, actor_id, action, entity, entity_id, metadata) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(Uuid::new_v4())
    .bind(workspace_id)
    .bind(actor_id)
    .bind(action)
    .bind(entity)
    .bind(entity_id)
    .bind(json!({}))
    .execute(exec)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_notification(
    exec: impl sqlx::PgExecutor<'_>,
    workspace_id: Uuid,
    user_id: Uuid,
    kind: &NotificationKind,
    actor_id: Uuid,
    task_id: Option<Uuid>,
    text_de: &str,
    text_en: &str,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO notifications (id, workspace_id, user_id, kind, actor_id, task_id, text, text_en, unread) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, true)",
    )
    .bind(Uuid::new_v4())
    .bind(workspace_id)
    .bind(user_id)
    .bind(notification_kind_to_db(kind))
    .bind(actor_id)
    .bind(task_id)
    .bind(text_de)
    .bind(text_en)
    .execute(exec)
    .await?;
    Ok(())
}

async fn task_status_is_done(
    exec: impl sqlx::PgExecutor<'_>,
    task_id: Uuid,
) -> Result<bool, AppError> {
    let (is_done,): (bool,) = sqlx::query_as(
        "SELECT s.is_done FROM tasks t JOIN project_statuses s ON s.id = t.status_id WHERE t.id = $1",
    )
    .bind(task_id)
    .fetch_one(exec)
    .await?;
    Ok(is_done)
}

#[derive(Debug, FromRow)]
struct RecurrenceSourceRow {
    recurrence: Option<String>,
    is_done: bool,
    project_id: Uuid,
    title: String,
    title_en: Option<String>,
    description: String,
    description_en: Option<String>,
    tag: String,
    tag_color: String,
    priority: String,
    start_date: Option<NaiveDate>,
    due_date: Option<NaiveDate>,
    phase: String,
    created_by: Option<Uuid>,
}

/// If `task_id` just transitioned from a not-done into a done status and
/// carries a recurrence, creates the next instance (dates shifted, subtasks
/// reset, assignees copied) and moves the recurrence marker onto it. Moving
/// the marker makes repeated spawning from the same task impossible: the
/// chain continues through the new instance. Returns the new task id.
async fn spawn_recurrence_if_completed(
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
    .bind(source.start_date.map(|d| shift_date(d, &recurrence)))
    .bind(source.due_date.map(|d| shift_date(d, &recurrence)))
    .bind(&source.phase)
    .bind(recurrence_to_db(&recurrence))
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

async fn create_workspace_for_user(
    conn: &mut PgConnection,
    user_id: Uuid,
    name: &str,
) -> Result<Uuid, AppError> {
    let workspace_id = Uuid::new_v4();
    let project_id = Uuid::new_v4();
    // url_slug is UNIQUE; the workspace-id suffix keeps users with identical
    // initials from colliding (which would 500 the whole registration).
    let slug = format!(
        "{}-{}",
        initials(name).to_lowercase(),
        &workspace_id.to_string()[..8]
    );

    sqlx::query(
        "INSERT INTO workspaces (id, name, url_slug, default_lang) VALUES ($1, $2, $3, 'de')",
    )
    .bind(workspace_id)
    .bind(format!("{} Workspace", name))
    .bind(slug)
    .execute(&mut *conn)
    .await?;
    sqlx::query("INSERT INTO memberships (id, workspace_id, user_id, role, status, last_active_at) VALUES ($1, $2, $3, 'owner', 'active', now())")
        .bind(Uuid::new_v4())
        .bind(workspace_id)
        .bind(user_id)
        .execute(&mut *conn)
        .await?;
    sqlx::query("INSERT INTO projects (id, workspace_id, name, key) VALUES ($1, $2, 'Neues Bauprojekt', 'KWB')")
        .bind(project_id)
        .bind(workspace_id)
        .execute(&mut *conn)
        .await?;
    insert_default_statuses(&mut *conn, project_id).await?;
    Ok(workspace_id)
}

fn is_unique_violation(err: &sqlx::Error) -> bool {
    matches!(err, sqlx::Error::Database(db) if db.code().as_deref() == Some("23505"))
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

    seed_attachment(db, task_ids["KWB-104"], "maengelprotokoll.pdf", 240_000).await?;
    seed_attachment(db, task_ids["KWB-104"], "fotoanhang-liste.json", 18_000).await?;
    seed_attachment(db, task_ids["KWB-107"], "terminplan.png", 512_000).await?;
    seed_attachment(db, task_ids["KWB-108"], "abnahme-checkliste.csv", 4_000).await?;

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

async fn insert_default_statuses(
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
    // Days relative to "today" so the demo always shows live-looking data.
    start_offset: i64,
    due_offset: i64,
    phase: &'static str,
    assignees: &'static [&'static str],
    subtasks: &'static [(&'static str, &'static str, bool)],
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
            start_offset: -2,
            due_offset: 0,
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
            start_offset: -1,
            due_offset: 2,
            phase: "planung",
            assignees: &["MR", "AK"],
            subtasks: &[
                ("Liefertermine einarbeiten", "Add delivery dates", true),
                ("Kritischen Pfad prüfen", "Review critical path", true),
                ("Puffer für Fassade setzen", "Set facade buffer", false),
                ("Plan an Team verteilen", "Share schedule with team", false),
            ],
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
            start_offset: -8,
            due_offset: -3,
            phase: "planung",
            assignees: &["JS"],
            subtasks: &[("Nachweise prüfen", "Review evidence", true), ("Rückfragen klären", "Resolve questions", true), ("Freigabe ablegen", "File approval", true)],
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
            start_offset: -10,
            due_offset: -6,
            phase: "planung",
            assignees: &["AK"],
            subtasks: &[("Bodenbelag festlegen", "Select flooring", true), ("Badserie freigeben", "Approve bathroom series", true), ("Türliste exportieren", "Export door list", true)],
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
            start_offset: -3,
            due_offset: 1,
            phase: "planung",
            assignees: &["AK", "MR"],
            subtasks: &[("Aushang entwerfen", "Draft notice", true), ("Terminfenster prüfen", "Check appointment windows", false), ("Ansprechpartner ergänzen", "Add contacts", false), ("Freigabe Verwaltung", "Administration approval", false), ("Verteilung planen", "Plan distribution", false)],
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
            start_offset: 1,
            due_offset: 3,
            phase: "vergabe",
            assignees: &["JS"],
            subtasks: &[("Elektrofreigabe ablegen", "File electrical approval", false), ("Sanitärfreigabe ablegen", "File plumbing approval", false), ("Trockenbau-Nachweis ergänzen", "Add drywall evidence", false)],
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
            start_offset: -3,
            due_offset: -1,
            phase: "abnahme",
            assignees: &["MR"],
            subtasks: &[("Fotos abgleichen", "Compare photos", true), ("Unterschriften prüfen", "Check signatures", true), ("Restarbeiten markieren", "Mark remaining work", false)],
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
            start_offset: 0,
            due_offset: 1,
            phase: "abnahme",
            assignees: &["SB"],
            subtasks: &[("Treppenhaus prüfen", "Check stairwell", true), ("Keller prüfen", "Check basement", true), ("Status im Protokoll setzen", "Update protocol status", false)],
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
            start_offset: -6,
            due_offset: -4,
            phase: "ausfuehrung",
            assignees: &["DK"],
            subtasks: &[("Fotoliste ergänzen", "Add photo list", true), ("Risiken aktualisieren", "Update risks", true)],
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
            start_offset: 4,
            due_offset: 7,
            phase: "ausfuehrung",
            assignees: &["AL", "JS"],
            subtasks: &[("Materialabruf prüfen", "Check material call-offs", false), ("Logistikfläche reservieren", "Reserve logistics area", false), ("Sicherheitsunterweisung planen", "Schedule safety briefing", false), ("Mieterinformation versenden", "Send tenant notice", false)],
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

fn ticket_status_to_db(status: &TicketStatus) -> &'static str {
    match status {
        TicketStatus::Open => "open",
        TicketStatus::InProgress => "in_progress",
        TicketStatus::Resolved => "resolved",
        TicketStatus::Closed => "closed",
    }
}

fn ticket_status_from_db(value: &str) -> Result<TicketStatus, AppError> {
    match value {
        "open" => Ok(TicketStatus::Open),
        "in_progress" => Ok(TicketStatus::InProgress),
        "resolved" => Ok(TicketStatus::Resolved),
        "closed" => Ok(TicketStatus::Closed),
        _ => Err(AppError::BadRequest(format!(
            "unknown ticket status {value}"
        ))),
    }
}

fn recurrence_to_db(recurrence: &Recurrence) -> &'static str {
    match recurrence {
        Recurrence::Daily => "daily",
        Recurrence::Weekly => "weekly",
        Recurrence::Biweekly => "biweekly",
        Recurrence::Monthly => "monthly",
    }
}

fn recurrence_from_db(value: &str) -> Result<Recurrence, AppError> {
    match value {
        "daily" => Ok(Recurrence::Daily),
        "weekly" => Ok(Recurrence::Weekly),
        "biweekly" => Ok(Recurrence::Biweekly),
        "monthly" => Ok(Recurrence::Monthly),
        _ => Err(AppError::BadRequest(format!("unknown recurrence {value}"))),
    }
}

fn shift_date(date: NaiveDate, recurrence: &Recurrence) -> NaiveDate {
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

/// User ids of members whose exact name appears as "@Name" in `body`,
/// followed by a non-alphanumeric boundary (or end of input). Matching is
/// case-sensitive against the canonical member names the autocomplete
/// inserts, checked longest-first so "@Anna Schmidt" wins over a member
/// literally named "Anna".
fn mentioned_user_ids(body: &str, members: &[(Uuid, String)]) -> Vec<Uuid> {
    let mut by_length: Vec<&(Uuid, String)> = members
        .iter()
        .filter(|(_, name)| !name.trim().is_empty())
        .collect();
    by_length.sort_by_key(|(_, name)| std::cmp::Reverse(name.len()));

    // Byte ranges already claimed by a (longer) name, so "@Anna Schmidt"
    // does not additionally mention a member named "Anna".
    let mut claimed: Vec<(usize, usize)> = Vec::new();
    let mut out: Vec<Uuid> = Vec::new();
    for (user_id, name) in by_length {
        let pattern = format!("@{name}");
        for (start, _) in body.match_indices(&pattern) {
            let end = start + pattern.len();
            let boundary_ok = body[end..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_alphanumeric());
            let overlaps = claimed.iter().any(|&(s, e)| start < e && end > s);
            if boundary_ok && !overlaps {
                claimed.push((start, end));
                if !out.contains(user_id) {
                    out.push(*user_id);
                }
            }
        }
    }
    out
}

fn inline_previewable(file_name: &str) -> bool {
    FsPath::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .is_some_and(|e| INLINE_PREVIEW_EXTENSIONS.contains(&e.as_str()))
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

fn allowed_upload_extension(file_name: &str) -> bool {
    FsPath::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .is_some_and(|e| ALLOWED_UPLOAD_EXTENSIONS.contains(&e.as_str()))
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

    #[test]
    fn upload_extensions_are_allowlisted() {
        assert!(allowed_upload_extension("plan.pdf"));
        assert!(allowed_upload_extension("PHOTO.JPG"));
        assert!(allowed_upload_extension("modell.ifc"));
        assert!(!allowed_upload_extension("malware.exe"));
        assert!(!allowed_upload_extension("seite.html"));
        assert!(!allowed_upload_extension("noextension"));
    }

    #[test]
    fn inline_preview_is_limited_to_safe_types() {
        assert!(inline_previewable("plan.pdf"));
        assert!(inline_previewable("PHOTO.JPG"));
        assert!(inline_previewable("foto.webp"));
        // SVG can execute script when rendered as a document.
        assert!(!inline_previewable("logo.svg"));
        assert!(!inline_previewable("daten.xlsx"));
    }

    #[test]
    fn shift_date_advances_by_recurrence() {
        let date = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        assert_eq!(
            shift_date(date, &Recurrence::Daily),
            NaiveDate::from_ymd_opt(2026, 6, 2).unwrap()
        );
        assert_eq!(
            shift_date(date, &Recurrence::Weekly),
            NaiveDate::from_ymd_opt(2026, 6, 8).unwrap()
        );
        assert_eq!(
            shift_date(date, &Recurrence::Biweekly),
            NaiveDate::from_ymd_opt(2026, 6, 15).unwrap()
        );
        assert_eq!(
            shift_date(date, &Recurrence::Monthly),
            NaiveDate::from_ymd_opt(2026, 7, 1).unwrap()
        );
    }

    #[test]
    fn shift_date_monthly_clamps_to_month_end() {
        let date = NaiveDate::from_ymd_opt(2026, 1, 31).unwrap();
        assert_eq!(
            shift_date(date, &Recurrence::Monthly),
            NaiveDate::from_ymd_opt(2026, 2, 28).unwrap()
        );
    }

    #[test]
    fn mentions_match_exact_names_with_boundaries() {
        let anna = Uuid::new_v4();
        let anna_schmidt = Uuid::new_v4();
        let joerg = Uuid::new_v4();
        let members = vec![
            (anna, "Anna".to_string()),
            (anna_schmidt, "Anna Schmidt".to_string()),
            (joerg, "Jörg Müller".to_string()),
        ];

        // Longest name wins; the shorter prefix member is not also mentioned.
        assert_eq!(
            mentioned_user_ids("ping @Anna Schmidt bitte prüfen", &members),
            vec![anna_schmidt]
        );
        assert_eq!(mentioned_user_ids("@Anna kannst du?", &members), vec![anna]);
        // Boundary check: a longer word must not match a shorter name.
        assert!(mentioned_user_ids("@Annabelle hi", &members).is_empty());
        // Umlaut names work without any lowercasing tricks.
        assert_eq!(
            mentioned_user_ids("cc @Jörg Müller!", &members),
            vec![joerg]
        );
        // No mention syntax, no hits.
        assert!(mentioned_user_ids("mail an anna@example.com", &members).is_empty());
        // Duplicates collapse.
        assert_eq!(
            mentioned_user_ids("@Anna und nochmal @Anna", &members),
            vec![anna]
        );
    }

    #[test]
    fn host_only_strips_ports_and_brackets() {
        assert_eq!(host_only("example.com"), "example.com");
        assert_eq!(host_only("example.com:8080"), "example.com");
        assert_eq!(host_only("[::1]:8080"), "::1");
        assert_eq!(host_only("127.0.0.1:80"), "127.0.0.1");
    }

    #[test]
    fn viewer_cannot_edit_but_members_and_up_can() {
        assert!(Role::Owner.can_edit());
        assert!(Role::Admin.can_edit());
        assert!(Role::Member.can_edit());
        assert!(!Role::Viewer.can_edit());

        assert!(Role::Owner.can_admin());
        assert!(Role::Admin.can_admin());
        assert!(!Role::Member.can_admin());
        assert!(!Role::Viewer.can_admin());
    }

    fn test_config() -> AppConfig {
        AppConfig {
            bind: "127.0.0.1:0".into(),
            static_dir: PathBuf::from("."),
            upload_dir: PathBuf::from("."),
            session_secret: "test-secret-with-at-least-32-characters!".into(),
            cookie_secure: false,
            seed_demo: false,
            registration_enabled: true,
            max_workspace_storage_bytes: MAX_WORKSPACE_STORAGE_BYTES,
            trust_proxy: false,
            public_origin: None,
        }
    }

    fn headers_with_cookie(cookie: &str) -> HeaderMap {
        let pair = cookie.split(';').next().expect("cookie pair");
        let mut headers = HeaderMap::new();
        headers.insert(COOKIE, HeaderValue::from_str(pair).expect("valid header"));
        headers
    }

    fn origin_headers(origin: Option<&str>, host: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(origin) = origin {
            headers.insert(ORIGIN, HeaderValue::from_str(origin).expect("valid header"));
        }
        if let Some(host) = host {
            headers.insert(HOST, HeaderValue::from_str(host).expect("valid header"));
        }
        headers
    }

    #[test]
    fn same_origin_compares_against_host_header() {
        let cfg = test_config();
        // No Origin header (curl, server-to-server): allowed.
        assert!(same_origin(&cfg, &origin_headers(None, Some("example.com"))));
        assert!(same_origin(
            &cfg,
            &origin_headers(Some("https://example.com"), Some("example.com"))
        ));
        assert!(same_origin(
            &cfg,
            &origin_headers(Some("https://example.com:8443"), Some("example.com:443"))
        ));
        assert!(!same_origin(
            &cfg,
            &origin_headers(Some("https://evil.test"), Some("example.com"))
        ));
        assert!(!same_origin(
            &cfg,
            &origin_headers(Some("null"), Some("example.com"))
        ));
        assert!(!same_origin(
            &cfg,
            &origin_headers(Some("https://example.com"), None)
        ));
    }

    #[test]
    fn same_origin_requires_exact_public_origin() {
        let mut cfg = test_config();
        cfg.public_origin = Some("https://kowobau.example".into());
        assert!(same_origin(
            &cfg,
            &origin_headers(Some("https://kowobau.example"), Some("other-host"))
        ));
        assert!(same_origin(
            &cfg,
            &origin_headers(Some("HTTPS://KOWOBAU.EXAMPLE"), None)
        ));
        assert!(!same_origin(
            &cfg,
            &origin_headers(Some("http://kowobau.example"), Some("kowobau.example"))
        ));
        assert!(!same_origin(
            &cfg,
            &origin_headers(Some("https://evil.test"), Some("kowobau.example"))
        ));
    }

    #[test]
    fn session_cookie_roundtrip() {
        let cfg = test_config();
        let session_id = Uuid::new_v4();
        let cookie = build_cookie(&cfg, session_id).expect("cookie builds");
        let headers = headers_with_cookie(&cookie);
        let parsed = parse_session_cookie(&headers, &cfg).expect("cookie parses");
        assert_eq!(parsed, session_id);
    }

    #[test]
    fn tampered_signature_is_rejected() {
        let cfg = test_config();
        let session_id = Uuid::new_v4();
        let cookie = build_cookie(&cfg, session_id).expect("cookie builds");
        let other_id = Uuid::new_v4().to_string();
        let signature = cookie
            .split(';')
            .next()
            .and_then(|pair| pair.rsplit_once('.'))
            .map(|(_, sig)| sig.to_string())
            .expect("signature present");
        let forged = format!("{COOKIE_NAME}={other_id}.{signature}");
        let headers = headers_with_cookie(&forged);
        assert!(matches!(
            parse_session_cookie(&headers, &cfg),
            Err(AppError::Unauthorized)
        ));
    }

    #[test]
    fn secure_cookie_uses_host_prefix_and_roundtrips() {
        let mut cfg = test_config();
        cfg.cookie_secure = true;
        let session_id = Uuid::new_v4();
        let cookie = build_cookie(&cfg, session_id).expect("cookie builds");
        assert!(cookie.starts_with("__Host-kowobau_session="));
        assert!(cookie.contains("; Secure"));
        assert!(cookie.contains("Path=/"));
        assert!(!cookie.contains("Domain="));
        let headers = headers_with_cookie(&cookie);
        assert_eq!(
            parse_session_cookie(&headers, &cfg).expect("cookie parses"),
            session_id
        );
        assert!(expired_cookie(&cfg).starts_with("__Host-kowobau_session=;"));
    }

    #[test]
    fn invite_tokens_hash_deterministically_and_differ() {
        let a = generate_invite_token();
        let b = generate_invite_token();
        assert_ne!(a, b);
        assert_eq!(invite_token_hash(&a), invite_token_hash(&a));
        assert_ne!(invite_token_hash(&a), invite_token_hash(&b));
    }

    #[test]
    fn cookie_with_wrong_secret_is_rejected() {
        let cfg = test_config();
        let session_id = Uuid::new_v4();
        let cookie = build_cookie(&cfg, session_id).expect("cookie builds");
        let mut other_cfg = test_config();
        other_cfg.session_secret = "another-secret-with-at-least-32-chars!!".into();
        let headers = headers_with_cookie(&cookie);
        assert!(matches!(
            parse_session_cookie(&headers, &other_cfg),
            Err(AppError::Unauthorized)
        ));
    }
}
