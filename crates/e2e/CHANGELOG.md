# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/dinosath/ferris-cms/releases/tag/e2e-v0.2.0) - 2026-08-16

### Fixed

- resolve web API URLs, Postgres timestamp types; add playwright e2e suite

### Other

- Add comprehensive playwright-rs UI screen tests
- Run UI flow tests; discover and document WASM auth-submit crash
- Add UI flow tests and expand backend integration coverage
- install local toolchain, add per-page screenshots, make tests pass
- full CRUD REST tests via reqwest; keep Playwright UI e2e
- run against local Turso DB + Obscura browser, drop containers
