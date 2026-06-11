FROM rust:1.95-bookworm AS planner
WORKDIR /app
RUN cargo install trunk --version 0.21.14
COPY . .

FROM planner AS frontend-builder
WORKDIR /app/crates/frontend
RUN trunk build --release --public-url /

FROM planner AS backend-builder
WORKDIR /app
RUN cargo build --release -p kowobau-backend

FROM debian:bookworm-slim AS backend-runtime
WORKDIR /app
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/*
COPY --from=backend-builder /app/target/release/kowobau-backend /usr/local/bin/kowobau-backend
COPY crates/backend/migrations /app/migrations
ENV KOWOBAU_BIND=0.0.0.0:8080
ENV KOWOBAU_UPLOAD_DIR=/app/uploads
VOLUME ["/app/uploads"]
EXPOSE 8080
CMD ["kowobau-backend"]

FROM nginx:1.29-alpine AS nginx-runtime
COPY deploy/nginx/docker.conf /etc/nginx/conf.d/default.conf
COPY --from=frontend-builder /app/crates/frontend/dist /usr/share/nginx/html
EXPOSE 80
