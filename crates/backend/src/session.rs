use crate::*;

pub(crate) async fn require_auth(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<AuthContext, AppError> {
    let session_id = parse_session_cookie(headers, &state.cfg)?;
    let row: Option<(Uuid, String, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT u.id, u.email, u.name, s.expires_at \
         FROM sessions s JOIN users u ON u.id = s.user_id \
         WHERE s.id = $1 AND s.expires_at > now()",
    )
    .bind(session_id)
    .fetch_optional(&state.db)
    .await?;

    let Some((id, email, name, expires_at)) = row else {
        return Err(AppError::Unauthorized);
    };

    // Sliding expiry: refresh only when the session is in its last stretch so
    // active users stay logged in without one UPDATE per request. The hard
    // cap (counted from created_at) bounds the total lifetime of any session.
    if expires_at - Utc::now() < Duration::days(SESSION_REFRESH_THRESHOLD_DAYS) {
        sqlx::query(
            "UPDATE sessions SET expires_at = LEAST( \
                 now() + make_interval(days => $2), \
                 created_at + make_interval(days => $3)) \
             WHERE id = $1",
        )
        .bind(session_id)
        .bind(SESSION_TTL_DAYS as i32)
        .bind(SESSION_HARD_CAP_DAYS as i32)
        .execute(&state.db)
        .await?;
    }

    Ok(AuthContext {
        user: UserDto {
            id: id.to_string(),
            email,
            name,
        },
        session_id,
    })
}

impl AuthContext {
    pub(crate) fn user_id(&self) -> Result<Uuid, AppError> {
        uuid_from_str(&self.user.id)
    }
}

pub(crate) async fn require_user(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(AuthContext, Uuid), AppError> {
    let ctx = require_auth(state, headers).await?;
    let user_id = ctx.user_id()?;
    Ok((ctx, user_id))
}

pub(crate) fn parse_session_cookie(headers: &HeaderMap, cfg: &AppConfig) -> Result<Uuid, AppError> {
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

pub(crate) fn sign(cfg: &AppConfig, value: &str) -> Result<String, AppError> {
    let mut mac = HmacSha256::new_from_slice(cfg.session_secret.as_bytes())
        .map_err(|_| AppError::Internal("invalid session secret".into()))?;
    mac.update(value.as_bytes());
    Ok(URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
}

pub(crate) const fn cookie_name(cfg: &AppConfig) -> &'static str {
    if cfg.cookie_secure {
        SECURE_COOKIE_NAME
    } else {
        COOKIE_NAME
    }
}

pub(crate) fn build_cookie(cfg: &AppConfig, session_id: Uuid) -> Result<String, AppError> {
    let id = session_id.to_string();
    let signed = format!("{}.{}", id, sign(cfg, &id)?);
    let secure = if cfg.cookie_secure { "; Secure" } else { "" };
    // Max-Age matches the server-side hard cap so the cookie never dies
    // before a (slid) session would; actual expiry is enforced server-side.
    Ok(format!(
        "{}={signed}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{}",
        cookie_name(cfg),
        SESSION_HARD_CAP_DAYS * 24 * 60 * 60,
        secure
    ))
}

pub(crate) fn expired_cookie(cfg: &AppConfig) -> String {
    let secure = if cfg.cookie_secure { "; Secure" } else { "" };
    format!(
        "{}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure}",
        cookie_name(cfg)
    )
}

pub(crate) fn cookie_header_value(cookie: String) -> Result<HeaderValue, AppError> {
    HeaderValue::from_str(&cookie)
        .map_err(|_| AppError::Internal("generated cookie was not header-safe".into()))
}

pub(crate) fn json_with_cookie<T: Serialize>(
    state: &AppState,
    session_id: Uuid,
    payload: T,
) -> Result<Response, AppError> {
    let mut res = Json(payload).into_response();
    res.headers_mut().insert(
        SET_COOKIE,
        cookie_header_value(build_cookie(&state.cfg, session_id)?)?,
    );
    Ok(res)
}

/// Runs Argon2 hashing on a blocking thread, bounded by the global permit
/// pool, so request floods cannot stall the async runtime or pin all cores.
pub(crate) async fn hash_password_async(
    state: &AppState,
    password: String,
) -> Result<String, AppError> {
    let _permit = state
        .hash_permits
        .acquire()
        .await
        .map_err(|_| AppError::Internal("hash semaphore closed".into()))?;
    tokio::task::spawn_blocking(move || hash_password(&password))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
}

pub(crate) async fn verify_password_async(
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

pub(crate) fn hash_password(password: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| AppError::Internal(e.to_string()))?
        .to_string())
}

pub(crate) fn verify_password(password: &str, hash: &str) -> Result<(), AppError> {
    let parsed = PasswordHash::new(hash).map_err(|_| AppError::Unauthorized)?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .map_err(|_| AppError::Unauthorized)
}

pub(crate) async fn create_session(
    conn: &mut PgConnection,
    user_id: Uuid,
) -> Result<Uuid, AppError> {
    // Opportunistic cleanup keeps the sessions table from growing without bound.
    sqlx::query("DELETE FROM sessions WHERE expires_at < now()")
        .execute(&mut *conn)
        .await?;
    let session_id = Uuid::new_v4();
    let expires_at = Utc::now() + Duration::days(SESSION_TTL_DAYS);
    sqlx::query("INSERT INTO sessions (id, user_id, expires_at) VALUES ($1, $2, $3)")
        .bind(session_id)
        .bind(user_id)
        .bind(expires_at)
        .execute(&mut *conn)
        .await?;
    Ok(session_id)
}
