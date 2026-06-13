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
# Moves trunk's inline bootstrap into /init.js; required because the CSP
# does not allow inline scripts (use python3 instead of py on Linux/macOS).
py externalize-init.py dist
Pop-Location
cargo build --release -p kowobau-backend
```

## Session secret (required)

The backend refuses to start unless `KOWOBAU_SESSION_SECRET` is set to at least 32 characters. Generate one and put it in `.env`:

```powershell
# PowerShell
[Convert]::ToBase64String((1..48 | ForEach-Object { Get-Random -Maximum 256 }))
```

```bash
# Linux/macOS
openssl rand -base64 48
```

With Docker available:

```powershell
Copy-Item .env.example .env
# Edit .env and set KOWOBAU_SESSION_SECRET and POSTGRES_PASSWORD (see above)
docker compose up --build
```

`POSTGRES_PASSWORD` is required as well; Compose refuses to start without it.

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
- In production set `KOWOBAU_COOKIE_SECURE=true` for the `app` service (the
  session cookie then uses the `__Host-` prefix).
- In production also set `KOWOBAU_PUBLIC_ORIGIN=https://kowobau.example.com` so
  state-changing requests must carry exactly this Origin (CSRF hardening).

## Internal vSphere reverse proxy

Use `deploy/nginx/internal-vsphere-http3.conf` when the app should run only on an
internal LAN address such as `192.168.1.x`.

It binds Nginx to the VM LAN IP, allows only `192.168.1.0/24`, enables HTTPS with
HTTP/3 over QUIC on UDP 443, keeps HTTP/2 as TCP fallback, and proxies to the
Docker Nginx published on `127.0.0.1:8080`.

Verify that the host Nginx supports HTTP/3:

```bash
nginx -V 2>&1 | grep -o -- '--with-http_v3_module'
```

Open TCP `80`, TCP `443`, and UDP `443` from the internal LAN.

For this setup, start Compose with:

```powershell
$env:KOWOBAU_HTTP_PORT="8080"
docker compose up -d --build
```

Set these production-style values in `.env`:

```env
KOWOBAU_COOKIE_SECURE=true
KOWOBAU_PUBLIC_ORIGIN=https://planner.example.test
```

## Security notes

- Login and register are rate limited per IP (10/min) both in nginx and in the
  backend; concurrent Argon2 hashing is bounded so auth floods cannot pin the CPU.
- `KOWOBAU_TRUST_PROXY=true` (set automatically in Compose) makes the backend
  use `X-Real-IP` for rate limiting; never enable it without a trusted proxy
  in front. Compose isolates nginx and the app on a dedicated `edge` network and
  trusts only that subnet (`KOWOBAU_TRUSTED_PROXIES=172.30.0.0/24`), so postgres
  or any future sibling container cannot spoof the header to dodge the limit.
- Workspace invites are single-use tokens with a 14-day expiry. `POST
  /api/workspaces/{id}/invites` returns `invite_token` and `invite_path`
  (`/?invite=<token>`); share that link out-of-band. Registering with the token
  joins the inviting workspace. If the invited email already has an account, it
  is added as a member directly and no token is returned.
- `POST /api/auth/logout-all` revokes every session of the current user.

## Demo account

The demo seed is disabled by default. To try the demo, set `KOWOBAU_SEED_DEMO=true`
(in `.env` or the environment) before the first start on an **empty** database:

- Email: `alex@firma.com`
- Password: `password123`

Do not enable the seed in production: it creates seven well-known accounts with
this public password.
