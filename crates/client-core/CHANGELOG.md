# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0](https://github.com/dinosath/ferris-cms/compare/client-core-v0.2.0...client-core-v0.3.0) - 2026-09-15

### Other

- update Cargo.toml dependencies

## [0.2.0](https://github.com/dinosath/ferris-cms/releases/tag/client-core-v0.2.0) - 2026-09-15

### Added

- *(build)* rustls-only deps + immutable release artifacts
- *(auth)* wire OIDC SSO into the UI and add mock-IdP E2E test
- *(ai)* test provider connection and auto-discover models
- *(ai)* native AI subsystem (providers, assistant, tools, content/schema/media, usage)
- *(import-export)* add Import & Export system
- *(workflow)* n8n-style visual workflow automation engine

### Fixed

- *(ctb)* require explicit removal instead of absence-as-deletion
- *(client)* use native fetch for wasm requests so the auth header is sent
- *(client)* set Authorization header explicitly and expose Client::token
- *(ctb)* always show add-field button and surface real save errors
- resolve web API URLs, Postgres timestamp types; add playwright e2e suite

### Other

- run cargo fmt across the workspace
- Add Unpublish control and fix publish documentId bug
- Add Internationalization (locales) settings UI
- Add API Tokens service, endpoints, and Settings UI
- Add Configure-the-view modal to Content Manager list toolbar
- Add Discard changes control to entry edit view
- Implement Media Library: backend storage + upload route + UI
- Add Settings > Users management UI with invite modal
- Add Settings > Roles management UI with permission matrix
- Initial commit: ferriscms — offline-first Strapi clone in Rust (Dioxus multiplatform UI + Axum backend)
