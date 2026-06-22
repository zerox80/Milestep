use crate::*;

pub(crate) async fn update_workspace(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<UpdateWorkspaceRequest>,
) -> Result<Json<WorkspaceDto>, AppError> {
    let (ctx, user_id) = require_user(&state, &headers).await?;
    let workspace_id = uuid_from_str(&id)?;
    assert_workspace_admin(&state.db, user_id, workspace_id).await?;
    // Whether this PATCH touches anything, decided before the fields are moved.
    let changed = payload.name.is_some() || payload.default_lang.is_some();
    if let Some(name) = payload.name {
        let name = required_capped(&name, MAX_LABEL_LEN, "workspace name")?;
        sqlx::query("UPDATE workspaces SET name = $1 WHERE id = $2")
            .bind(name)
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
    // Skip the audit entry and realtime fan-out for a no-op PATCH so an empty
    // request body cannot spam the audit log or trigger needless refetches.
    if changed {
        record_audit(
            &state.db,
            workspace_id,
            user_id,
            "updated workspace",
            "workspace",
            Some(workspace_id),
        )
        .await?;
        notify_workspace(&state, &ctx, &headers, workspace_id, "workspace");
    }
    Ok(Json(fetch_workspace(&state.db, workspace_id).await?))
}

pub(crate) async fn invite_member(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<InviteMemberRequest>,
) -> Result<Json<InviteMemberResponse>, AppError> {
    let (ctx, user_id) = require_user(&state, &headers).await?;
    let workspace_id = uuid_from_str(&id)?;
    assert_workspace_admin(&state.db, user_id, workspace_id).await?;
    let email = payload.email.trim().to_lowercase();
    if !email.contains('@') || email.chars().count() > MAX_EMAIL_LEN {
        return Err(AppError::BadRequest("invite email is invalid".into()));
    }
    if matches!(payload.role, Role::Owner) {
        return Err(AppError::BadRequest(
            "cannot invite a member as owner".into(),
        ));
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
        notify_workspace(&state, &ctx, &headers, workspace_id, "workspace");
        return Ok(Json(InviteMemberResponse {
            invite_token: None,
            invite_path: None,
        }));
    }
    // Single-use random token; only its hash is stored, so a database leak
    // does not expose redeemable invites. Insert-or-refresh is one atomic
    // statement: the ON CONFLICT path only fires for an *expired* row, so two
    // concurrent invites for the same address can never both mint a live token.
    // The loser conflicts with the still-valid row, the WHERE skips the update,
    // and RETURNING yields nothing — which we surface as the same conflict a
    // sequential duplicate invite would hit.
    let token = generate_invite_token();
    let inserted: Option<(Uuid,)> = sqlx::query_as(
        "INSERT INTO workspace_invites (id, workspace_id, email, role, invited_by, token_hash, expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT (workspace_id, email) DO UPDATE SET \
             role = EXCLUDED.role, invited_by = EXCLUDED.invited_by, \
             token_hash = EXCLUDED.token_hash, expires_at = EXCLUDED.expires_at, \
             created_at = now() \
         WHERE workspace_invites.expires_at <= now() \
         RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(workspace_id)
    .bind(&email)
    .bind(role_to_db(&payload.role))
    .bind(user_id)
    .bind(invite_token_hash(&token))
    .bind(Utc::now() + Duration::days(INVITE_TTL_DAYS))
    .fetch_optional(&state.db)
    .await?;
    if inserted.is_none() {
        return Err(AppError::Conflict("invite already exists".into()));
    }
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

pub(crate) fn generate_invite_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub(crate) fn invite_token_hash(token: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(token.as_bytes()))
}

pub(crate) async fn update_membership(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<UpdateMembershipRequest>,
) -> Result<Json<MemberDto>, AppError> {
    let (ctx, user_id) = require_user(&state, &headers).await?;
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
    // Read the actor's role inside the same transaction (and after the target
    // row is locked FOR UPDATE) so authorization and the last-owner check see
    // one consistent snapshot, even under concurrent role changes.
    let actor_role = workspace_role(&mut *tx, user_id, row.workspace_id)
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
    notify_workspace(&state, &ctx, &headers, row.workspace_id, "workspace");
    Ok(Json(fetch_member(&state.db, membership_id).await?))
}

pub(crate) async fn remove_membership(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let (ctx, user_id) = require_user(&state, &headers).await?;
    let membership_id = uuid_from_str(&id)?;
    let mut tx = state.db.begin().await?;
    let row: MembershipWorkspaceRow = sqlx::query_as(
        "SELECT workspace_id, user_id, role FROM memberships WHERE id = $1 FOR UPDATE",
    )
    .bind(membership_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;
    // Same consistency reasoning as update_membership: read the actor role in
    // the transaction so it cannot drift against the locked target row.
    let actor_role = workspace_role(&mut *tx, user_id, row.workspace_id)
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
    notify_workspace(&state, &ctx, &headers, row.workspace_id, "workspace");
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn create_workspace_for_user(
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
    .bind(format!("{name} Workspace"))
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

pub(crate) fn is_unique_violation(err: &sqlx::Error) -> bool {
    matches!(err, sqlx::Error::Database(db) if db.code().as_deref() == Some("23505"))
}
