//! Integration tests for the access-control layer against a real Postgres.
//!
//! These exercise the `assert_*`/`*_access` functions that guard every handler
//! — the IDOR-critical guarantees that pure unit tests cannot cover. They are
//! skipped gracefully when `DATABASE_URL` is unset (so `cargo test` stays green
//! without a database); CI provides a Postgres service and runs them for real.
//!
//! The tests insert rows and do not clean up, so point `DATABASE_URL` at a
//! disposable database (CI uses an ephemeral `kowobau_test`), never a real one.

use crate::*;

/// Connects and migrates. Returns `None` only when `DATABASE_URL` is unset, so
/// `cargo test` without a database skips these. When it *is* set (CI), a failed
/// connection or migration panics loudly instead of silently skipping.
async fn connect() -> Option<PgPool> {
    let Ok(url) = env::var("DATABASE_URL") else {
        return None;
    };
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("DATABASE_URL is set but the database connection failed");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrations failed to run on the test database");
    Some(pool)
}

/// Inserts a fresh user with a globally unique email and returns its id.
async fn make_user(pool: &PgPool) -> Uuid {
    let id = Uuid::new_v4();
    let email = format!("{id}@test.local");
    sqlx::query("INSERT INTO users (id, email, name, password_hash) VALUES ($1, $2, $3, $4)")
        .bind(id)
        .bind(&email)
        .bind("Test User")
        .bind("x")
        .execute(pool)
        .await
        .expect("insert user");
    id
}

/// Creates a workspace owned by `owner` (with its default project and statuses)
/// and returns `(workspace_id, project_id, first_status_id)`.
async fn make_workspace(pool: &PgPool, owner: Uuid) -> (Uuid, Uuid, Uuid) {
    let mut conn = pool.acquire().await.expect("acquire");
    let workspace_id = create_workspace_for_user(&mut conn, owner, "Test")
        .await
        .expect("create workspace");
    let (project_id,): (Uuid,) = sqlx::query_as(
        "SELECT id FROM projects WHERE workspace_id = $1 ORDER BY created_at LIMIT 1",
    )
    .bind(workspace_id)
    .fetch_one(pool)
    .await
    .expect("project");
    let (status_id,): (Uuid,) = sqlx::query_as(
        "SELECT id FROM project_statuses WHERE project_id = $1 ORDER BY position LIMIT 1",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await
    .expect("status");
    (workspace_id, project_id, status_id)
}

/// Adds `user` to `workspace` with the given role.
async fn add_member(pool: &PgPool, workspace_id: Uuid, user: Uuid, role: &str) {
    sqlx::query(
        "INSERT INTO memberships (id, workspace_id, user_id, role, status, last_active_at) \
         VALUES ($1, $2, $3, $4, 'active', now())",
    )
    .bind(Uuid::new_v4())
    .bind(workspace_id)
    .bind(user)
    .bind(role)
    .execute(pool)
    .await
    .expect("insert membership");
}

/// Inserts a minimal task into `project` and returns its id.
async fn make_task(pool: &PgPool, project_id: Uuid, status_id: Uuid) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO tasks (id, project_id, key, title, priority, status_id) \
         VALUES ($1, $2, $3, $4, 'medium', $5)",
    )
    .bind(id)
    .bind(project_id)
    .bind(format!("T-{id}"))
    .bind("Task")
    .bind(status_id)
    .execute(pool)
    .await
    .expect("insert task");
    id
}

#[tokio::test]
async fn owner_can_read_and_edit_their_task() {
    let Some(pool) = connect().await else { return };
    let owner = make_user(&pool).await;
    let (workspace_id, project_id, status_id) = make_workspace(&pool, owner).await;
    let task_id = make_task(&pool, project_id, status_id).await;

    assert_eq!(
        assert_task_read(&pool, owner, task_id).await.ok(),
        Some(workspace_id)
    );
    assert_eq!(
        assert_task_edit(&pool, owner, task_id).await.ok(),
        Some(workspace_id)
    );
}

#[tokio::test]
async fn viewer_can_read_but_not_edit() {
    let Some(pool) = connect().await else { return };
    let owner = make_user(&pool).await;
    let viewer = make_user(&pool).await;
    let (workspace_id, project_id, status_id) = make_workspace(&pool, owner).await;
    add_member(&pool, workspace_id, viewer, "viewer").await;
    let task_id = make_task(&pool, project_id, status_id).await;

    assert!(assert_task_read(&pool, viewer, task_id).await.is_ok());
    assert!(matches!(
        assert_task_edit(&pool, viewer, task_id).await,
        Err(AppError::Forbidden)
    ));
}

#[tokio::test]
async fn member_can_edit() {
    let Some(pool) = connect().await else { return };
    let owner = make_user(&pool).await;
    let member = make_user(&pool).await;
    let (workspace_id, project_id, status_id) = make_workspace(&pool, owner).await;
    add_member(&pool, workspace_id, member, "member").await;
    let task_id = make_task(&pool, project_id, status_id).await;

    assert!(assert_task_edit(&pool, member, task_id).await.is_ok());
}

#[tokio::test]
async fn non_member_cannot_access_task() {
    let Some(pool) = connect().await else { return };
    let owner = make_user(&pool).await;
    let outsider = make_user(&pool).await;
    let (_workspace_id, project_id, status_id) = make_workspace(&pool, owner).await;
    let task_id = make_task(&pool, project_id, status_id).await;

    assert!(matches!(
        assert_task_read(&pool, outsider, task_id).await,
        Err(AppError::Forbidden)
    ));
    assert!(matches!(
        task_access(&pool, outsider, task_id).await,
        Err(AppError::Forbidden)
    ));
}

#[tokio::test]
async fn task_access_is_isolated_across_workspaces() {
    let Some(pool) = connect().await else { return };
    let owner_a = make_user(&pool).await;
    let owner_b = make_user(&pool).await;
    let (_ws_a, project_a, status_a) = make_workspace(&pool, owner_a).await;
    make_workspace(&pool, owner_b).await; // owner_b owns a different workspace
    let task_a = make_task(&pool, project_a, status_a).await;

    // The owner of workspace B has no membership in workspace A.
    assert!(matches!(
        assert_task_read(&pool, owner_b, task_a).await,
        Err(AppError::Forbidden)
    ));
}

#[tokio::test]
async fn workspace_role_reflects_membership() {
    let Some(pool) = connect().await else { return };
    let owner = make_user(&pool).await;
    let outsider = make_user(&pool).await;
    let (workspace_id, _project_id, _status_id) = make_workspace(&pool, owner).await;

    assert_eq!(
        workspace_role(&pool, owner, workspace_id).await.ok(),
        Some(Some(Role::Owner))
    );
    assert_eq!(
        workspace_role(&pool, outsider, workspace_id).await.ok(),
        Some(None)
    );
}
