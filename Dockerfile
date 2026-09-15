# syntax=docker/dockerfile:1
# ferriscms server image — builds a SINGLE self-contained `ferriscms-server`
# binary that embeds both the Axum webserver and the Dioxus WASM admin UI
# (via rust-embed). No separate UI bundle is shipped: the WASM/JS/CSS assets
# are baked into the executable at compile time.
#
# The Dioxus WASM UI is always built in RELEASE mode (optimized, no debug
# symbols, no devtools/hot-reload overlay). The tooling dx needs (wasm-opt from
# binaryen, wasm-bindgen-cli, esbuild) is installed here so `NO_DOWNLOADS=1`
# makes dx use these system binaries instead of auto-downloading them.
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
# `binaryen` provides `wasm-opt`, which `dx build --release` needs (dx would
# otherwise try to auto-download it from GitHub). `curl` fetches esbuild.
# `musl-tools` provides musl-gcc for the fully static server build.
RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential pkg-config libssl-dev binaryen curl musl-tools \
    && rm -rf /var/lib/apt/lists/* \
    && rustup target add wasm32-unknown-unknown x86_64-unknown-linux-musl

# Install the Dioxus CLI. Version must match the dioxus crate (0.7.10).
# `--locked` pins to the CLI's own vetted dependency set.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo install dioxus-cli --version 0.7.10 --locked

# Install the matching `wasm-bindgen-cli` (crates.io) and `esbuild` (npm) so dx
# uses them instead of auto-downloading from GitHub/npm.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo install wasm-bindgen-cli --version 0.2.126 --locked \
    && curl -sL "https://registry.npmjs.org/@esbuild/linux-x64/-/linux-x64-0.27.3.tgz" -o /tmp/esbuild.tgz \
    && tar xzf /tmp/esbuild.tgz -C /tmp \
    && install -m 0755 /tmp/package/bin/esbuild /usr/local/bin/esbuild \
    && rm -rf /tmp/package /tmp/esbuild.tgz

# Build the web (WASM) bundle in release mode and copy it into the api-rest
# crate's `ui/` folder where the server binary embeds it at compile time.
# `NO_DOWNLOADS=1` makes dx use the system `wasm-opt`/`wasm-bindgen`/`esbuild`
# installed above instead of fetching its own copies.
#
# dx writes the bundle under the Cargo target dir (`/app/target`, via the cache
# mount) at `dx/<app>/release/web/public`.
WORKDIR /app/crates/app
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    NO_DOWNLOADS=1 dx build --web --release \
    && cp -r /app/target/dx/ferriscms/release/web/public /app/crates/api-rest/ui

# Build the server now that the UI is present, embedding the assets into the
# binary (rust-embed), and stage it at a stable path for the runtime stage to
# COPY. This must run after the `ui/` copy above, so the first compile of
# api-rest picks up the embedded files.
WORKDIR /app
# Static musl build: no libc/OpenSSL at runtime, so the image can use
# distroless/static (no shell, no package manager, tiny base).
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release --target x86_64-unknown-linux-musl -p server-bin \
    && cp /app/target/x86_64-unknown-linux-musl/release/ferriscms-server /app/ferriscms-server

########## Runtime stage ##########
# Distroless static (no libc, no shell, no package manager; ~1 MiB base). The
# server is a static musl binary that uses rustls with bundled webpki roots, so
# it needs nothing from the base image.
FROM gcr.io/distroless/static-debian13 AS runtime

# The single self-contained binary (webserver + embedded admin UI).
COPY --from=builder /app/ferriscms-server /usr/local/bin/ferriscms-server

# CA bundle for outbound TLS (rustls-platform-verifier used by reqwest 0.13 for
# the AI providers reads the system trust store; distroless/static ships none).
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt

# Persistable directory for uploaded media (mounted by the Helm chart/Compose).
WORKDIR /data

# PostgreSQL for the webserver. Override via env/Helm. Example URL:
# postgres://user:password@host:5432/ferriscms
ENV BIND_ADDR=0.0.0.0:1337 \
    DATABASE_URL=postgres://postgres:postgres@localhost:5432/ferriscms \
    MEDIA_STORAGE_DIR=/data/media

EXPOSE 1337
ENTRYPOINT ["/usr/local/bin/ferriscms-server"]
