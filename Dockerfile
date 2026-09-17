########## Build stage ##########
FROM rust:1.97.1-slim-bookworm AS builder
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential pkg-config libssl-dev binaryen curl musl-tools \
    && rm -rf /var/lib/apt/lists/* \
    && rustup target add wasm32-unknown-unknown x86_64-unknown-linux-musl

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo install dioxus-cli --version 0.7.10 --locked

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo install wasm-bindgen-cli --version 0.2.126 --locked \
    && curl -sL "https://registry.npmjs.org/@esbuild/linux-x64/-/linux-x64-0.27.3.tgz" -o /tmp/esbuild.tgz \
    && tar xzf /tmp/esbuild.tgz -C /tmp \
    && install -m 0755 /tmp/package/bin/esbuild /usr/local/bin/esbuild \
    && rm -rf /tmp/package /tmp/esbuild.tgz

WORKDIR /app/crates/app
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    NO_DOWNLOADS=1 dx build --web --release \
    && cp -r /app/target/dx/ferriscms/release/web/public /app/crates/api-rest/ui

WORKDIR /app
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release --target x86_64-unknown-linux-musl -p server-bin \
    && cp /app/target/x86_64-unknown-linux-musl/release/ferriscms-server /app/ferriscms

FROM gcr.io/distroless/static-debian13 AS runtime

COPY --from=builder /app/ferriscms /usr/local/bin/ferriscms
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt

WORKDIR /data

ENV BIND_ADDR=0.0.0.0:8080 \
    DATABASE_URL=postgres://postgres:postgres@localhost:5432/ferriscms \
    MEDIA_STORAGE_DIR=/data/media

EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/ferriscms"]
