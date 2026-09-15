# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.1](https://github.com/dinosath/ferris-cms/compare/db-v0.3.0...db-v0.3.1) - 2026-09-15

### Other

- update Cargo.toml dependencies

## [0.3.0](https://github.com/dinosath/ferris-cms/compare/db-v0.2.0...db-v0.3.0) - 2026-09-15

### Other

- update Cargo.toml dependencies

## [0.2.0](https://github.com/dinosath/ferris-cms/releases/tag/db-v0.2.0) - 2026-09-15

### Added

- *(ai)* per-conversation privacy mode
- *(ai)* native AI subsystem (providers, assistant, tools, content/schema/media, usage)
- *(import-export)* persist mapping presets in the database + UI
- *(workflow)* n8n-style visual workflow automation engine

### Fixed

- *(db)* make connection pool resilient to stale connections
- resolve web API URLs, Postgres timestamp types; add playwright e2e suite

### Other

- run cargo fmt across the workspace
- Initial commit: ferriscms — offline-first Strapi clone in Rust (Dioxus multiplatform UI + Axum backend)
