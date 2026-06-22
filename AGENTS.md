# Repository Guidelines

## Project Structure & Module Organization

KoWoBau-Planner is a Rust workspace with three crates under `crates/`. `crates/backend` contains the Axum API, PostgreSQL access, WebSockets, migrations in `migrations/`, and upload storage placeholder in `uploads/`. `crates/frontend` contains the Leptos CSR app, Trunk entry files, UI modules in `src/`, CSS in `src/styles.css`, and fonts in `assets/fonts/`. `crates/shared` holds Serde DTOs shared by backend and frontend. Deployment examples live in `deploy/nginx/`.

## Build, Test, and Development Commands

- `cargo fmt --all`: format all crates.
- `cargo check --workspace`: type-check the workspace.
- `cargo clippy --workspace --all-targets -- -D warnings`: run native lint checks with warnings denied.
- `cargo clippy -p kowobau-frontend --target wasm32-unknown-unknown -- -D warnings`: lint the WASM frontend target.
- `cargo test --workspace`: run all Rust tests.
- `cd crates/frontend; trunk build --release --public-url /; py externalize-init.py dist`: build the frontend and move Trunk's inline bootstrap into `init.js` for CSP compliance.
- `cargo build --release -p kowobau-backend`: build the backend binary.
- `docker compose up --build`: run the full stack after creating `.env` from `.env.example`.

## Coding Style & Naming Conventions

Use Rust 2021, `rustfmt`, and the workspace lint policy in `Cargo.toml`. Unsafe code is denied. Keep modules focused: CI rejects Rust files over 500 lines and CSS files over 200 lines. Prefer existing domain names such as `TaskDto`, `TicketRow`, `workspace_id`, and `ticket_id`. Frontend components are organized by view or feature module, with shared helpers re-exported from `main.rs`.

## Testing Guidelines

Tests use Rust's built-in `#[test]` framework and are colocated in module test blocks or crate-level `src/tests.rs`. Add focused tests for parsing, date logic, access rules, config, and security-sensitive behavior. Name tests as behavior statements, for example `inline_preview_is_limited_to_safe_types`. Backend access-control integration tests live in `crates/backend/src/db_tests.rs`; they skip when `DATABASE_URL` is unset and run in CI against an ephemeral Postgres. Point `DATABASE_URL` at a disposable database when running them locally — they write rows and do not clean up.

## Commit & Pull Request Guidelines

Recent commits use short imperative summaries, often scoped by feature or refactor, for example `Add CSS line-count gate` or `Refactor backend startup config`; automated work may use `[codex]`. Keep PRs focused, describe behavior changes, list validation commands, and link issues. Include screenshots for visible frontend changes and call out `.env`, migration, or deployment impacts.

## Security & Configuration Tips

Never commit real secrets. Copy `.env.example` to `.env` and set `KOWOBAU_SESSION_SECRET` to at least 32 characters plus `POSTGRES_PASSWORD`. Do not enable `KOWOBAU_SEED_DEMO=true` in production. Only set `KOWOBAU_TRUST_PROXY=true` behind a trusted proxy, and set `KOWOBAU_PUBLIC_ORIGIN` plus secure cookies for production.
