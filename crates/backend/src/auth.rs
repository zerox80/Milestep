use crate::*;

pub(crate) async fn health() -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "service": "kowobau-planner" }))
}

pub(crate) async fn register(
    State(state): State<AppState>,
    Json(payload): Json<RegisterRequest>,
) -> Result<Response, AppError> {
    let email = payload.email.trim().to_lowercase();
    let name = required_capped(&payload.name, MAX_LABEL_LEN, "name")?;
    if name.chars().count() < 2 {
        return Err(AppError::BadRequest("name is too short".into()));
    }
    if !email.contains('@') || email.chars().count() > MAX_EMAIL_LEN {
        return Err(AppError::BadRequest("email is invalid".into()));
    }
    // Upper-bound the password too: Argon2 hashes the full input, so an
    // unbounded passphrase is needless work on every login attempt.
    if payload.password.len() < 8 || payload.password.len() > 256 {
        return Err(AppError::BadRequest(
            "password must contain between 8 and 256 characters".into(),
        ));
    }

    // Invite lookup happens before the expensive hash so bad tokens fail fast.
    // The row is still claimed atomically inside the transaction below.
    let invite_hash = payload
        .invite_token
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(invite_token_hash);
    if let Some(hash) = &invite_hash {
        let exists: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM workspace_invites \
             WHERE token_hash = $1 AND email = $2 AND expires_at > now()",
        )
        .bind(hash)
        .bind(&email)
        .fetch_optional(&state.db)
        .await?;
        if exists.is_none() {
            return Err(AppError::BadRequest(
                "invite code is invalid or expired".into(),
            ));
        }
    }

    if invite_hash.is_none() && !state.cfg.registration_enabled {
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
    // The fast-path check above can race two concurrent first-user registrations
    // through on an empty database. Re-check under a transaction-level advisory
    // lock so at most one such registration can succeed.
    if invite_hash.is_none() && !state.cfg.registration_enabled {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext('kowobau:registration'))")
            .execute(&mut *tx)
            .await?;
        let (user_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(&mut *tx)
            .await?;
        if user_count > 0 {
            return Err(AppError::Forbidden);
        }
    }
    let inserted =
        sqlx::query("INSERT INTO users (id, email, name, password_hash) VALUES ($1, $2, $3, $4)")
            .bind(user_id)
            .bind(&email)
            .bind(name)
            .bind(password_hash)
            .execute(&mut *tx)
            .await;
    if let Err(err) = inserted {
        if is_unique_violation(&err) {
            return Err(AppError::Conflict("email is already registered".into()));
        }
        return Err(err.into());
    }

    let workspace_id = match invite_hash {
        None => create_workspace_for_user(&mut tx, user_id, name).await?,
        Some(hash) => {
            let invite: Option<(Uuid, Uuid, String)> = sqlx::query_as(
                "DELETE FROM workspace_invites \
                 WHERE token_hash = $1 AND email = $2 AND expires_at > now() \
                 RETURNING id, workspace_id, role",
            )
            .bind(hash)
            .bind(&email)
            .fetch_optional(&mut *tx)
            .await?;
            let Some((_invite_id, invite_workspace, role)) = invite else {
                return Err(AppError::BadRequest(
                    "invite code is invalid or expired".into(),
                ));
            };
            role_from_db(&role)?;
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

pub(crate) async fn login(
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

pub(crate) async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    if let Ok(ctx) = require_auth(&state, &headers).await {
        sqlx::query("DELETE FROM sessions WHERE id = $1")
            .bind(ctx.session_id)
            .execute(&state.db)
            .await?;
    }

    let mut res = StatusCode::NO_CONTENT.into_response();
    res.headers_mut()
        .insert(SET_COOKIE, cookie_header_value(expired_cookie(&state.cfg))?);
    Ok(res)
}

/// Revokes every session of the current user ("log out everywhere").
pub(crate) async fn logout_all(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(ctx.user_id()?)
        .execute(&state.db)
        .await?;
    let mut res = StatusCode::NO_CONTENT.into_response();
    res.headers_mut()
        .insert(SET_COOKIE, cookie_header_value(expired_cookie(&state.cfg))?);
    Ok(res)
}

pub(crate) async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<AuthResponse>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    Ok(Json(AuthResponse { user: ctx.user }))
}

pub(crate) async fn bootstrap(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<WorkspaceQuery>,
) -> Result<Json<BootstrapDto>, AppError> {
    let ctx = require_auth(&state, &headers).await?;
    let data = fetch_bootstrap(&state.db, ctx.user_id()?, query.workspace_uuid()?).await?;
    Ok(Json(data))
}
