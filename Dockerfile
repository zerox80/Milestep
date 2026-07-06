FROM rust:1.95-bookworm AS chef
WORKDIR /app
# python3 runs the trunk post_build hook (crates/frontend/externalize-init.py).
RUN apt-get update \
  && apt-get install -y --no-install-recommends python3 \
  && rm -rf /var/lib/apt/lists/*
RUN cargo install cargo-chef --locked \
  && cargo install trunk --version 0.21.14 --locked \
  && rustup target add wasm32-unknown-unknown

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Dependencies are cooked from the recipe only, so source-only changes reuse
# the cached dependency layers instead of rebuilding everything.
FROM chef AS backend-builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release -p milestep-backend --recipe-path recipe.json
COPY . .
RUN cargo build --release -p milestep-backend

FROM chef AS frontend-builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --target wasm32-unknown-unknown -p milestep-frontend --recipe-path recipe.json
COPY . .
WORKDIR /app/crates/frontend
# externalize-init.py moves trunk's inline bootstrap into /init.js so the CSP
# can stay free of script-src 'unsafe-inline'.
RUN trunk build --release --public-url / \
  && python3 externalize-init.py dist

FROM debian:bookworm-slim AS backend-runtime
WORKDIR /app
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/* \
  && useradd --system --uid 10001 --user-group --no-create-home app \
  && mkdir -p /app/uploads \
  && chown app:app /app/uploads
COPY --from=backend-builder /app/target/release/milestep-backend /usr/local/bin/milestep-backend
COPY crates/backend/migrations /app/migrations
ENV MILESTEP_BIND=0.0.0.0:8080
ENV MILESTEP_UPLOAD_DIR=/app/uploads
VOLUME ["/app/uploads"]
EXPOSE 8080
USER app
CMD ["milestep-backend"]

FROM nginx:1.31-alpine AS nginx-runtime
COPY deploy/nginx/docker.conf /etc/nginx/conf.d/default.conf
COPY --from=frontend-builder /app/crates/frontend/dist /usr/share/nginx/html
EXPOSE 80
