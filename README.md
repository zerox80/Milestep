# KoWoBau-Planner Fullstack MVP

KoWoBau-Planner is a Rust fullstack MVP for construction and modernization project planning, built from the supplied mockup.

## Stack

- Backend: Rust, Axum, SQLx, PostgreSQL
- Frontend: Rust, Leptos CSR, WebAssembly, Trunk
- Shared contract: Serde DTO crate
- Deployment: Docker Compose with an internal Nginx asset/proxy container

## Local development

Docker is not installed in the current environment, but the project includes Compose files.

```powershell
cargo fmt --all
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
Push-Location crates/frontend
trunk build --release
Pop-Location
cargo build --release -p kowobau-backend
```

With Docker available:

```powershell
docker compose up --build
```

Docker Compose starts:

- `app`: Axum API on the private Compose network.
- `nginx`: static WASM/assets plus `/api` reverse proxy, published on `http://127.0.0.1:80` by default.
- `postgres`: persistent PostgreSQL database, published on `127.0.0.1:5432` by default for local admin tools.

The app serves the API and frontend from one origin through Docker Nginx on `http://127.0.0.1`.

All published Compose ports bind to localhost only:

```yaml
127.0.0.1:${KOWOBAU_HTTP_PORT:-80}:80
127.0.0.1:${KOWOBAU_POSTGRES_PORT:-5432}:5432
```

## Production reverse proxy

Use `deploy/nginx/external-http3-ubuntu.conf` as the host-level Nginx example for Ubuntu 24.04 / 26.04.

It terminates TLS, enables HTTP/3 over QUIC on UDP 443, keeps HTTP/2 as TCP fallback, and proxies to Docker Nginx at `127.0.0.1:8080`.

When running the host-level Nginx on the same machine, start Compose with a non-conflicting internal host port:

```powershell
$env:KOWOBAU_HTTP_PORT="8080"
docker compose up --build
```

Before enabling it:

- Replace `kowobau.example.com`.
- Replace the Let's Encrypt certificate paths.
- Verify HTTP/3 support with `nginx -V 2>&1 | grep -o -- '--with-http_v3_module'`.
- Open TCP `80`, TCP `443`, and UDP `443`.
- In production set `KOWOBAU_COOKIE_SECURE=true` for the `app` service.

## Demo account

The backend seeds the demo workspace on an empty database.

- Email: `alex@firma.com`
- Password: `password123`
