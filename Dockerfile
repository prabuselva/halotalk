# syntax=docker/dockerfile:1

# ---- stage 1: web assets (node 22 → web/dist) -------------------------------
FROM node:22 AS web
WORKDIR /src
COPY web/package.json web/package-lock.json ./web/
RUN npm ci --prefix web
COPY web/ ./web/
RUN npm run build --prefix web

# ---- stage 2: rust release build (web/dist embedded via rust-embed) ---------
FROM rust:1-slim AS app
RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY --from=web /src/web/dist ./web/dist
RUN cargo build --release -p halo-server

# ---- stage 3: minimal runtime (same Debian release as rust:1-slim = glibc match)
FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --home-dir /data --shell /usr/sbin/nologin halotalk
COPY --from=app /build/target/release/halotalk /usr/local/bin/halotalk
ENV HALO_HOST=0.0.0.0 \
    HALO_PORT=8420 \
    HALO_DB_PATH=/data/halotalk.db \
    HALO_UPLOAD_DIR=/data/uploads
VOLUME /data
EXPOSE 8420
USER halotalk
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s \
    CMD curl -fsS http://127.0.0.1:8420/api/health || exit 1
CMD ["halotalk"]
