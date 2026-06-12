pub(crate) use std::{
    collections::HashMap,
    env,
    net::{IpAddr, SocketAddr, TcpStream},
    path::{Path as FsPath, PathBuf},
    sync::{Arc, LazyLock, Mutex},
    time::{Duration as StdDuration, Instant},
};

pub(crate) use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
pub(crate) use axum::{
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
        HeaderMap, HeaderName, HeaderValue, Method, StatusCode,
    },
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
pub(crate) use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
pub(crate) use chrono::{DateTime, Duration, NaiveDate, Utc};
pub(crate) use hmac::{Hmac, Mac};
pub(crate) use kowobau_shared::*;
pub(crate) use rand_core::{OsRng, RngCore};
pub(crate) use serde::{Deserialize, Serialize};
pub(crate) use serde_json::json;
pub(crate) use sha2::{Digest, Sha256};
pub(crate) use sqlx::{postgres::PgPoolOptions, FromRow, PgConnection, PgPool};
pub(crate) use tokio::{
    fs,
    io::AsyncWriteExt,
    net::TcpListener,
    sync::{broadcast, Semaphore},
};
pub(crate) use tokio_util::io::ReaderStream;
pub(crate) use tower_http::{
    compression::CompressionLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
pub(crate) use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
pub(crate) use uuid::Uuid;

pub(crate) type HmacSha256 = Hmac<Sha256>;

mod access;
mod assemble;
mod attachments;
mod auth;
mod config;
mod convert;
mod error;
mod middleware;
mod queries;
mod recurrence;
mod router;
mod rows;
mod seed;
mod seed_tasks;
mod session;
mod subtasks;
mod tasks;
#[cfg(test)]
mod tests;
mod tickets;
mod workspace;
mod ws;

pub(crate) use access::*;
pub(crate) use assemble::*;
pub(crate) use attachments::*;
pub(crate) use auth::*;
pub(crate) use config::*;
pub(crate) use convert::*;
pub(crate) use error::*;
pub(crate) use middleware::*;
pub(crate) use queries::*;
pub(crate) use recurrence::*;
pub(crate) use router::*;
pub(crate) use rows::*;
pub(crate) use seed::*;
pub(crate) use seed_tasks::*;
pub(crate) use session::*;
pub(crate) use subtasks::*;
pub(crate) use tasks::*;
pub(crate) use tickets::*;
pub(crate) use workspace::*;
pub(crate) use ws::*;

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
        // The demo seed creates accounts with the well-known password
        // "password123"; leftover demo users in a non-demo deployment are an
        // open door and deserve a loud warning on every start.
        let demo_exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)")
                .bind(fixed_uuid("20000000-0000-4000-8000-000000000001")?)
                .fetch_one(&db)
                .await?;
        if demo_exists {
            tracing::warn!(
                "SECURITY: demo-seeded accounts with well-known passwords exist in this \
                 database while KOWOBAU_SEED_DEMO is off; delete the demo users or wipe \
                 the database before production use"
            );
        }
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
