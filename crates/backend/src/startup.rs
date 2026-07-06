use crate::*;

pub(crate) async fn run() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    if env::args().any(|arg| arg == "--healthcheck") {
        return healthcheck_cli();
    }

    init_tracing();

    let cfg = AppConfig::from_env()?;
    fs::create_dir_all(&cfg.upload_dir).await?;

    let db = connect_database().await?;
    prepare_database(&db, &cfg).await?;

    let state = build_state(db, cfg);
    serve(state).await
}

pub(crate) fn init_tracing() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "milestep_backend=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
}

pub(crate) async fn connect_database() -> anyhow::Result<PgPool> {
    let database_url = env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://milestep:milestep@localhost:5432/milestep".to_string());
    Ok(PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(StdDuration::from_secs(10))
        .connect(&database_url)
        .await?)
}

pub(crate) async fn prepare_database(db: &PgPool, cfg: &AppConfig) -> anyhow::Result<()> {
    sqlx::migrate!("./migrations").run(db).await?;
    if cfg.seed_demo {
        tracing::info!("MILESTEP_SEED_DEMO is enabled; seeding demo data on empty database");
        seed_demo(db, &cfg.upload_dir).await?;
    } else {
        tracing::info!("demo seed disabled (set MILESTEP_SEED_DEMO=true to enable)");
        // The demo seed creates accounts with the well-known password
        // "password123"; leftover demo users in a non-demo deployment are an
        // open door and deserve a loud warning on every start.
        let demo_exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)")
                .bind(fixed_uuid("20000000-0000-4000-8000-000000000001")?)
                .fetch_one(db)
                .await?;
        if demo_exists {
            tracing::warn!(
                "SECURITY: demo-seeded accounts with well-known passwords exist in this \
                 database while MILESTEP_SEED_DEMO is off; delete the demo users or wipe \
                 the database before production use"
            );
        }
    }
    Ok(())
}

pub(crate) fn build_state(db: PgPool, cfg: AppConfig) -> AppState {
    let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
    AppState {
        db,
        cfg,
        auth_limiter: Arc::new(Mutex::new(HashMap::new())),
        hash_permits: Arc::new(Semaphore::new(MAX_CONCURRENT_PASSWORD_HASHES)),
        ws_conns: Arc::new(Mutex::new(HashMap::new())),
        events,
    }
}

pub(crate) async fn serve(state: AppState) -> anyhow::Result<()> {
    let app = build_router(state.clone());
    let listener = TcpListener::bind(&state.cfg.bind).await?;
    tracing::info!("Milestep listening on http://{}", state.cfg.bind);
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

pub(crate) async fn shutdown_signal() {
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
        () = ctrl_c => {},
        () = terminate => {},
    }
}
