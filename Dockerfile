# syntax=docker/dockerfile:1
# ferriscms server image — builds the `ferriscms-server` binary and runs it
# on a slim Debian runtime. The webserver uses PostgreSQL. sqlx uses rustls
# and does not need libpq, so the runtime stays small.

########## Build stage ##########
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

########## Runtime stage ##########
FROM debian:bookworm-slim AS runtime
# Pin apt package versions (DL3008) for reproducible, lean image.
# Bookworm ships ca-certificates 20230311+deb12u1 (verified in bookworm main).
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates=20230311+deb12u1 \
    && rm -rf /var/lib/apt/lists/*

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
