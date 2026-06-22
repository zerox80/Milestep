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
    routing::{delete, get, patch, post},
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
#[cfg(test)]
mod db_tests;
mod error;
mod middleware;
mod milestones;
mod queries;
mod recurrence;
mod router;
mod rows;
mod seed;
mod seed_tasks;
mod session;
mod startup;
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
pub(crate) use milestones::*;
pub(crate) use queries::*;
pub(crate) use recurrence::*;
pub(crate) use router::*;
pub(crate) use rows::*;
pub(crate) use seed::*;
pub(crate) use seed_tasks::*;
pub(crate) use session::*;
pub(crate) use startup::*;
pub(crate) use subtasks::*;
pub(crate) use tasks::*;
pub(crate) use tickets::*;
pub(crate) use workspace::*;
pub(crate) use ws::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    run().await
}
