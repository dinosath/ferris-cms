# syntax=docker/dockerfile:1
# ferriscms server image — builds the `ferriscms-server` binary and runs it
# on a slim Debian runtime. No OpenSSL/libpq needed (sea-orm uses rustls and
# bundled SQLite), so the runtime is kept small.

########## Build stage ##########
FROM rust:1.97.1-slim-bookworm AS builder
WORKDIR /app

# Cache dependency compilation.
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release -p server-bin

########## Runtime stage ##########
FROM debian:bookworm-slim AS runtime
# Pin apt package versions (DL3008) for reproducible, lean image.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates=20230311 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/ferriscms-server /usr/local/bin/ferriscms-server

# Persistable data directory (SQLite DB + uploaded media).
RUN mkdir -p /data/media

ENV BIND_ADDR=0.0.0.0:1337 \
    DATABASE_URL=sqlite:/data/ferriscms.db?mode=rwc \
    MEDIA_STORAGE_DIR=/data/media

EXPOSE 1337
ENTRYPOINT ["ferriscms-server"]
