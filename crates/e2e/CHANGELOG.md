# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/dinosath/ferris-cms/releases/tag/e2e-v0.2.0) - 2026-09-06

### Added

- *(ai)* auto-create a default model per provider and add provider via modal
- *(ai)* per-conversation privacy mode
- *(ai)* native AI subsystem (providers, assistant, tools, content/schema/media, usage)
- *(import-export)* export filtering
- *(import-export)* configurable CSV delimiter + header row
- *(import-export)* export field selection + reliable field projection
- *(import-export)* persist mapping presets in the database + UI
- *(import-export)* Content Manager contextual actions + prefer-uid prefill
- *(import-export)* add Import & Export system
- *(e2e)* make Obscura headless browser runnable in dev and test
- *(ui)* redirect unauthenticated users to the login screen

### Fixed

- *(ai)* resolve conversation provider and make tool names OpenAI-compatible
- *(read)* decode Postgres INT4 integer columns on read
- *(import/read)* return schema attribute names so imported fields display
- *(db)* make connection pool resilient to stale connections
- resolve web API URLs, Postgres timestamp types; add playwright e2e suite

### Other

- *(e2e)* CTB create-modal Continue dispatches (reactivity regression)
- *(e2e)* AI Assistant + AI Settings screens render
- *(e2e)* validate Import/Export wizard UI renders and navigates
- *(import-export)* broaden edge-case and failure-mode coverage
- *(e2e)* add table-first navigation smoke test + update for new UI
- *(e2e)* add Workflows screen + editor UI e2e coverage
- *(auth)* login credential validation and authorized-only UI access
- run cargo fmt across the workspace
- Add comprehensive playwright-rs UI screen tests
- Run UI flow tests; discover and document WASM auth-submit crash
- Add UI flow tests and expand backend integration coverage
- install local toolchain, add per-page screenshots, make tests pass
- full CRUD REST tests via reqwest; keep Playwright UI e2e
- run against local Turso DB + Obscura browser, drop containers
