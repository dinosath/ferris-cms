# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/dinosath/ferris-cms/releases/tag/api-types-v0.2.0) - 2026-09-06

### Added

- *(auth)* wire OIDC SSO into the UI and add mock-IdP E2E test
- *(ai)* per-conversation privacy mode
- *(ai)* native AI subsystem (providers, assistant, tools, content/schema/media, usage)
- *(import-export)* configurable CSV delimiter + header row
- *(import-export)* Content Manager contextual actions + prefer-uid prefill
- *(import-export)* add Import & Export system

### Fixed

- *(ctb)* require explicit removal instead of absence-as-deletion

### Other

- run cargo fmt across the workspace
- Initial commit: ferriscms — offline-first Strapi clone in Rust (Dioxus multiplatform UI + Axum backend)
