# syntax=docker/dockerfile:1
# ferriscms server image — builds a SINGLE self-contained `ferriscms-server`
# binary that embeds both the Axum webserver and the Dioxus WASM admin UI
# (via rust-embed). No separate UI bundle is shipped: the WASM/JS/CSS assets
# are baked into the executable at compile time.
#
# The webserver uses PostgreSQL. sqlx uses rustls and does not need libpq, so
# the runtime stays small.

########## Build stage ##########
FROM rust:1.97.1-slim-bookworm AS builder
WORKDIR /app

# Cache dependency compilation. BuildKit cache mounts are NOT part of the
# image filesystem, so after building we copy the binary to a real path
# (`/app/ferriscms-server`) for the runtime stage to COPY.
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

# The `dx` CLI links against OpenSSL (via a transitive dep), so it needs the
# C toolchain + headers present at build time, plus the WASM target.
RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/* \
    && rustup target add wasm32-unknown-unknown

# Install the Dioxus CLI. Version must match the dioxus crate (0.7.10).
# `--locked` pins to the CLI's own vetted dependency set.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo install dioxus-cli --version 0.7.10 --locked

# Build the web (WASM) bundle. We build the debug profile on purpose: `dx
# build` only invokes `wasm-opt`/binaryen (which dx auto-downloads and is
# unreliable in Docker) for release builds, so the debug profile avoids that
# network dependency and produces a fully functional bundle.
#
# dx writes the web bundle under the Cargo target dir (`/app/target`, via the
# cache mount) at `dx/<app>/debug/web/public`. Copy it into the api-rest
# crate's `ui/` folder, where the server binary embeds it at compile time.
WORKDIR /app/crates/app
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    dx build --web --release \
    && cp -r /app/target/dx/ferriscms/release/web/public /app/crates/api-rest/ui

# Build the server now that the UI is present, embedding the assets into the
# binary (rust-embed), and stage it at a stable path for the runtime stage to
# COPY. This must run after the `ui/` copy above, so the first compile of
# api-rest picks up the embedded files.
WORKDIR /app
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release -p server-bin \
    && cp /app/target/release/ferriscms-server /app/ferriscms-server

########## Runtime stage ##########
FROM debian:bookworm-slim AS runtime
# Pin apt package versions (DL3008) for reproducible, lean image.
# Bookworm ships ca-certificates 20230311+deb12u1 (verified in bookworm main).
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates=20230311+deb12u1 \
    && rm -rf /var/lib/apt/lists/*

# The single self-contained binary (webserver + embedded admin UI).
COPY --from=builder /app/ferriscms-server /usr/local/bin/ferriscms-server

# Persistable directory for uploaded media.
RUN mkdir -p /data/media

# PostgreSQL for the webserver. Override via env/Helm. Example URL:
# postgres://user:password@host:5432/ferriscms
ENV BIND_ADDR=0.0.0.0:1337 \
    DATABASE_URL=postgres://postgres:postgres@localhost:5432/ferriscms \
    MEDIA_STORAGE_DIR=/data/media

EXPOSE 1337
ENTRYPOINT ["ferriscms-server"]
