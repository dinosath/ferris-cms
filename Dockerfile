# syntax=docker/dockerfile:1
# ferriscms server image — builds the `ferriscms-server` binary, embeds the
# Dioxus WASM admin UI, and runs them on a slim Debian runtime. The webserver
# uses PostgreSQL. sqlx uses rustls and does not need libpq, so the runtime
# stays small.

########## Build stage: server binary ##########
FROM rust:1.97.1-slim-bookworm AS builder
WORKDIR /app

# Cache dependency compilation. BuildKit cache mounts are NOT part of the
# image filesystem, so after building we copy the binary to a real path
# (`/app/ferriscms-server`) for the runtime stage to COPY.
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release -p server-bin \
    && cp /app/target/release/ferriscms-server /app/ferriscms-server

########## Build stage: Dioxus WASM admin UI ##########
# Derives from the builder so it inherits Cargo.lock + sources, but this stage
# installs the `dx` CLI and cross-compiles the `ferriscms` app to WASM. The
# `dx` install is cached as a layer, so it is only compiled once.
FROM builder AS ui-builder

# Add the WASM target so the UI can be cross-compiled.
RUN rustup target add wasm32-unknown-unknown

# The `dx` CLI links against OpenSSL (via a transitive dep), so it needs the
# C toolchain + headers present at build time.
RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Install the Dioxus CLI. Version must match the dioxus crate (0.7.10).
# `--locked` pins to the CLI's own vetted dependency set.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo install dioxus-cli --version 0.7.10 --locked

# Build the web (WASM) bundle and stage it at a stable path for the runtime
# COPY. We build the debug profile on purpose: `dx build` only invokes
# `wasm-opt`/binaryen (which dx auto-downloads and is unreliable in Docker)
# for release builds, so the debug profile avoids that network dependency and
# produces a fully functional bundle.
#
# dx writes the web bundle under the Cargo target dir (`/app/target` here, via
# the cache mount) at `dx/<app>/debug/web/public` — `index.html` and hashed
# assets live in that `public` folder. We copy it out of the cache mount to a
# stable path the runtime stage can COPY.
WORKDIR /app/crates/app
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    dx build --web \
    && cp -r /app/target/dx/ferriscms/debug/web/public /app/web-ui

########## Runtime stage ##########
FROM debian:bookworm-slim AS runtime
# Pin apt package versions (DL3008) for reproducible, lean image.
# Bookworm ships ca-certificates 20230311+deb12u1 (verified in bookworm main).
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates=20230311+deb12u1 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/ferriscms-server /usr/local/bin/ferriscms-server
# The Dioxus WASM admin UI is served by the webserver at `/` (see api-rest).
COPY --from=ui-builder /app/web-ui /app/web

# Persistable directory for uploaded media.
RUN mkdir -p /data/media

# PostgreSQL for the webserver. Override via env/Helm. Example URL:
# postgres://user:password@host:5432/ferriscms
ENV BIND_ADDR=0.0.0.0:1337 \
    DATABASE_URL=postgres://postgres:postgres@localhost:5432/ferriscms \
    MEDIA_STORAGE_DIR=/data/media \
    FERRISCMS_UI_DIR=/app/web

EXPOSE 1337
ENTRYPOINT ["ferriscms-server"]
