use crate::*;

pub(crate) const COOKIE_NAME: &str = "milestep_session";
// __Host- locks the cookie to the exact host over HTTPS (requires Secure,
// Path=/ and no Domain attribute, all of which build_cookie guarantees).
pub(crate) const SECURE_COOKIE_NAME: &str = "__Host-milestep_session";
pub(crate) const MAX_UPLOAD_BYTES: usize = 50 * 1024 * 1024;
pub(crate) const MAX_JSON_BODY_BYTES: usize = 64 * 1024;
// Per-field character caps for user free text. The 64 KB JSON body limit only
// bounds a whole request; without these a single title/description could still
// be tens of thousands of characters, breaking layout and bloating every
// bootstrap payload that re-ships it. Counted in characters, not bytes.
pub(crate) const MAX_TITLE_LEN: usize = 200;
pub(crate) const MAX_LABEL_LEN: usize = 80;
pub(crate) const MAX_TEXT_LEN: usize = 10_000;
pub(crate) const MAX_COMMENT_LEN: usize = 5_000;
pub(crate) const MAX_EMAIL_LEN: usize = 254;
pub(crate) const AUTH_RATE_LIMIT_WINDOW: StdDuration = StdDuration::from_mins(1);
pub(crate) const AUTH_RATE_LIMIT_MAX_ATTEMPTS: u32 = 10;
pub(crate) const INVITE_TTL_DAYS: i64 = 14;
// Sessions slide: each authenticated request near expiry extends the session
// by the TTL again, but never beyond the hard cap counted from creation.
pub(crate) const SESSION_TTL_DAYS: i64 = 14;
pub(crate) const SESSION_HARD_CAP_DAYS: i64 = 30;
pub(crate) const SESSION_REFRESH_THRESHOLD_DAYS: i64 = 7;
// Bounds concurrent Argon2 work so unauthenticated login/register floods
// cannot pin every core with password hashing.
pub(crate) const MAX_CONCURRENT_PASSWORD_HASHES: usize = 4;
pub(crate) const MAX_WORKSPACE_STORAGE_BYTES: i64 = 2 * 1024 * 1024 * 1024;
// Uploaded file names are length-capped so the on-disk name (uuid prefix plus
// ".tmp" suffix) always stays well below common 255-byte filesystem limits.
// Counted in UTF-8 bytes, truncated on a char boundary.
pub(crate) const MAX_UPLOAD_FILE_NAME_BYTES: usize = 100;
pub(crate) const ALLOWED_UPLOAD_EXTENSIONS: &[&str] = &[
    "pdf", "png", "jpg", "jpeg", "webp", "svg", "csv", "xlsx", "docx", "txt", "json", "zip", "dwg",
    "ifc",
];
// Extensions that may be served with Content-Disposition: inline so the app
// can preview them in <img>/<iframe>. SVG is deliberately excluded: rendered
// as a document it could execute script; it stays download-only.
pub(crate) const INLINE_PREVIEW_EXTENSIONS: &[&str] = &["pdf", "png", "jpg", "jpeg", "webp"];
// Bounded fanout queue for realtime events; slow sockets get a resync hint
// instead of unbounded buffering.
pub(crate) const EVENT_CHANNEL_CAPACITY: usize = 256;
// Caps simultaneous realtime sockets per user so a single account cannot
// exhaust connection/broadcast resources by opening sockets without bound.
pub(crate) const MAX_WS_CONNECTIONS_PER_USER: usize = 8;

// Equalizes login timing for unknown emails so account existence cannot be inferred.
pub(crate) static DUMMY_PASSWORD_HASH: LazyLock<String> = LazyLock::new(|| {
    hash_password("timing-equalization-placeholder").expect("hashing a constant cannot fail")
});

#[derive(Debug, Clone)]
pub(crate) struct AppConfig {
    pub(crate) bind: String,
    pub(crate) static_dir: PathBuf,
    pub(crate) upload_dir: PathBuf,
    pub(crate) session_secret: String,
    pub(crate) cookie_secure: bool,
    pub(crate) seed_demo: bool,
    pub(crate) registration_enabled: bool,
    pub(crate) max_workspace_storage_bytes: i64,
    // When true (behind our nginx), the client IP for rate limiting is taken
    // from X-Real-IP instead of the peer address — but only when the direct
    // peer is inside `trusted_proxies`, so a directly reachable backend
    // cannot have its rate limiter spoofed via the header.
    pub(crate) trust_proxy: bool,
    pub(crate) trusted_proxies: Vec<IpCidr>,
    // When set (e.g. "https://milestep.example.com"), state-changing requests
    // must carry exactly this Origin, closing the scheme-blind host check.
    pub(crate) public_origin: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct AppState {
    pub(crate) db: PgPool,
    pub(crate) cfg: AppConfig,
    pub(crate) auth_limiter: Arc<Mutex<HashMap<IpAddr, (Instant, u32)>>>,
    pub(crate) hash_permits: Arc<Semaphore>,
    // Live realtime-socket count per user id, for the per-user connection cap.
    pub(crate) ws_conns: Arc<Mutex<HashMap<Uuid, usize>>>,
    // Workspace-scoped realtime events, fanned out to every connected socket.
    pub(crate) events: broadcast::Sender<WorkspaceEventDto>,
}

#[derive(Debug, Clone)]
pub(crate) struct AuthContext {
    pub(crate) user: UserDto,
    pub(crate) session_id: Uuid,
}

impl AppConfig {
    pub(crate) fn from_env() -> anyhow::Result<Self> {
        let session_secret = session_secret_from_env()?;

        Ok(Self {
            bind: env_or_default("MILESTEP_BIND", "KOWOBAU_BIND", "127.0.0.1:8080"),
            static_dir: env_path_or_default(
                "MILESTEP_STATIC_DIR",
                "KOWOBAU_STATIC_DIR",
                "crates/frontend/dist",
            ),
            upload_dir: env_path_or_default(
                "MILESTEP_UPLOAD_DIR",
                "KOWOBAU_UPLOAD_DIR",
                "crates/backend/uploads",
            ),
            session_secret,
            cookie_secure: env_flag("MILESTEP_COOKIE_SECURE", "KOWOBAU_COOKIE_SECURE"),
            seed_demo: env_flag("MILESTEP_SEED_DEMO", "KOWOBAU_SEED_DEMO"),
            registration_enabled: env_var(
                "MILESTEP_REGISTRATION_ENABLED",
                "KOWOBAU_REGISTRATION_ENABLED",
            )
            .is_none_or(|v| flag_is_enabled(&v)),
            max_workspace_storage_bytes: env_i64(
                "MILESTEP_MAX_WORKSPACE_STORAGE_BYTES",
                "KOWOBAU_MAX_WORKSPACE_STORAGE_BYTES",
            )?
            .unwrap_or(MAX_WORKSPACE_STORAGE_BYTES),
            trust_proxy: env_flag("MILESTEP_TRUST_PROXY", "KOWOBAU_TRUST_PROXY"),
            trusted_proxies: trusted_proxies_from_env()?,
            public_origin: env_var("MILESTEP_PUBLIC_ORIGIN", "KOWOBAU_PUBLIC_ORIGIN")
                .map(normalize_origin)
                .filter(|v| !v.is_empty()),
        })
    }
}

pub(crate) fn env_var(primary: &str, fallback: &str) -> Option<String> {
    env::var(primary).ok().or_else(|| env::var(fallback).ok())
}

fn session_secret_from_env() -> anyhow::Result<String> {
    let session_secret = env_var("MILESTEP_SESSION_SECRET", "KOWOBAU_SESSION_SECRET").ok_or_else(
        || {
            anyhow::anyhow!(
                "MILESTEP_SESSION_SECRET must be set (generate one with e.g. `openssl rand -base64 48`)"
            )
        },
    )?;
    if session_secret.len() < 32 {
        anyhow::bail!("MILESTEP_SESSION_SECRET must be at least 32 characters long");
    }
    Ok(session_secret)
}

fn env_or_default(primary: &str, fallback: &str, default: &str) -> String {
    env_var(primary, fallback).unwrap_or_else(|| default.to_string())
}

fn env_path_or_default(primary: &str, fallback: &str, default: &str) -> PathBuf {
    env_var(primary, fallback).map_or_else(|| PathBuf::from(default), PathBuf::from)
}

fn env_flag(primary: &str, fallback: &str) -> bool {
    env_var(primary, fallback).is_some_and(|v| flag_is_enabled(&v))
}

fn flag_is_enabled(value: &str) -> bool {
    matches!(
        value.trim().to_lowercase().as_str(),
        "1" | "true" | "yes" | "on" | "enable" | "enabled"
    )
}

fn env_i64(primary: &str, fallback: &str) -> anyhow::Result<Option<i64>> {
    env_var(primary, fallback)
        // A typo must not silently fall back to the default quota.
        .map(|value| {
            value.parse().map_err(|_| {
                anyhow::anyhow!("{primary} must be an integer byte count, got {value:?}")
            })
        })
        .transpose()
}

fn trusted_proxies_from_env() -> anyhow::Result<Vec<IpCidr>> {
    match env_var("MILESTEP_TRUSTED_PROXIES", "KOWOBAU_TRUSTED_PROXIES") {
        Some(list) => parse_trusted_proxies(&list),
        None => Ok(default_trusted_proxies()),
    }
}

fn parse_trusted_proxies(list: &str) -> anyhow::Result<Vec<IpCidr>> {
    // A typo in the list must not silently widen or shrink trust.
    list.split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|s| {
            IpCidr::parse(s).ok_or_else(|| {
                anyhow::anyhow!("MILESTEP_TRUSTED_PROXIES contains an invalid CIDR: {s:?}")
            })
        })
        .collect()
}

fn normalize_origin(value: String) -> String {
    value.trim_end_matches('/').to_string()
}

pub(crate) fn healthcheck_cli() -> anyhow::Result<()> {
    use std::io::{Read, Write};

    let bind =
        env_var("MILESTEP_BIND", "KOWOBAU_BIND").unwrap_or_else(|| "127.0.0.1:8080".to_string());
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
