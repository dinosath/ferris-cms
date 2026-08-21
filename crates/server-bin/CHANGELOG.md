# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/dinosath/ferris-cms/releases/tag/server-bin-v0.2.0) - 2026-08-21

### Added

- *(workflow)* n8n-style visual workflow automation engine
- *(server)* serve HTTPS directly via rustls using Let's Encrypt cert
- embed and serve Dioxus WASM admin UI from the server image

### Other

- run cargo fmt across the workspace
- Fix Dockerfile cache-mount build failure; webserver uses Postgres, desktop uses SQLite
- Implement Media Library: backend storage + upload route + UI
- Initial commit: ferriscms — offline-first Strapi clone in Rust (Dioxus multiplatform UI + Axum backend)
