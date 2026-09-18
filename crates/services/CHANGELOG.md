# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0](https://github.com/dinosath/ferris-cms/compare/v0.3.1...v0.4.0) - 2026-09-18

### Added

- add bulk import for content types and workflows
- add migration for admin credentials in dev mode

## [0.3.1](https://github.com/dinosath/ferris-cms/compare/services-v0.3.0...services-v0.3.1) - 2026-09-15

### Added

- *(build)* rustls-only deps + immutable release artifacts
- *(computed)* bulk create/update endpoints ignore computed values
- *(auth)* wire OIDC SSO into the UI and add mock-IdP E2E test
- *(auth)* add OpenID Connect (SSO) admin login
- auto-provision admin from env and add Helm configmap credentials
- *(metadata)* Kubernetes-style labels/namespace on content types and workflows
- *(ai)* expose content-type/workflow actions to the assistant, chat table + overlay
- *(ai)* test provider connection and auto-discover models
- *(ai)* auto-create a default model per provider and add provider via modal
- validate payload constraints (required/min/max/length/pattern/enum) before handling
- *(ai)* per-conversation privacy mode
- *(ai)* native AI subsystem (providers, assistant, tools, content/schema/media, usage)
- *(import-export)* export filtering
- *(import-export)* configurable CSV delimiter + header row
- *(import-export)* export field selection + reliable field projection
- *(import-export)* persist mapping presets in the database + UI
- *(import-export)* Content Manager contextual actions + prefer-uid prefill
- *(import-export)* add Import & Export system
- *(workflow)* n8n-style visual workflow automation engine

### Fixed

- *(computed)* inline chained computed columns; real Postgres acceptance
- *(ai)* keep tool-call history valid and surface reasoning
- *(ai)* skip orphaned tool messages when rebuilding chat history
- *(ai)* don't silently return empty assistant replies
- *(ai)* resolve conversation provider and make tool names OpenAI-compatible
- *(ctb)* require explicit removal instead of absence-as-deletion

### Other

- release v0.3.0
- release v0.2.0
- *(computed)* E2E CRUD/pagination/export + performance benchmarks
- *(computed)* DDL rendering + migration lifecycle tests
- *(auth)* split provision_admin for testability and cover env bootstrap
- Replace custom workflow engine with Open Workflow Specification (OWS)
- *(import-export)* broaden edge-case and failure-mode coverage
- *(workflow)* CMS content-created trigger + retry coverage
- run cargo fmt across the workspace
- Add unit tests for internal helper branches; backend coverage >90%
- Fix relation/inverse-FK DDL with two-phase application
- Add Unpublish control and fix publish documentId bug
- Add API Tokens service, endpoints, and Settings UI
- Implement Media Library: backend storage + upload route + UI
- Wire real JWT auth + fix runtime RBAC/API shape
- Align content navigation, CTB, RBAC, and UI with Strapi
- Initial commit: ferriscms — offline-first Strapi clone in Rust (Dioxus multiplatform UI + Axum backend)

## [0.3.0](https://github.com/dinosath/ferris-cms/compare/services-v0.2.0...services-v0.3.0) - 2026-09-15

### Added

- *(build)* rustls-only deps + immutable release artifacts
- *(computed)* bulk create/update endpoints ignore computed values
- *(auth)* wire OIDC SSO into the UI and add mock-IdP E2E test
- *(auth)* add OpenID Connect (SSO) admin login
- auto-provision admin from env and add Helm configmap credentials
- *(metadata)* Kubernetes-style labels/namespace on content types and workflows
- *(ai)* expose content-type/workflow actions to the assistant, chat table + overlay
- *(ai)* test provider connection and auto-discover models
- *(ai)* auto-create a default model per provider and add provider via modal
- validate payload constraints (required/min/max/length/pattern/enum) before handling
- *(ai)* per-conversation privacy mode
- *(ai)* native AI subsystem (providers, assistant, tools, content/schema/media, usage)
- *(import-export)* export filtering
- *(import-export)* configurable CSV delimiter + header row
- *(import-export)* export field selection + reliable field projection
- *(import-export)* persist mapping presets in the database + UI
- *(import-export)* Content Manager contextual actions + prefer-uid prefill
- *(import-export)* add Import & Export system
- *(workflow)* n8n-style visual workflow automation engine

### Fixed

- *(computed)* inline chained computed columns; real Postgres acceptance
- *(ai)* keep tool-call history valid and surface reasoning
- *(ai)* skip orphaned tool messages when rebuilding chat history
- *(ai)* don't silently return empty assistant replies
- *(ai)* resolve conversation provider and make tool names OpenAI-compatible
- *(ctb)* require explicit removal instead of absence-as-deletion

### Other

- release v0.2.0
- *(computed)* E2E CRUD/pagination/export + performance benchmarks
- *(computed)* DDL rendering + migration lifecycle tests
- *(auth)* split provision_admin for testability and cover env bootstrap
- Replace custom workflow engine with Open Workflow Specification (OWS)
- *(import-export)* broaden edge-case and failure-mode coverage
- *(workflow)* CMS content-created trigger + retry coverage
- run cargo fmt across the workspace
- Add unit tests for internal helper branches; backend coverage >90%
- Fix relation/inverse-FK DDL with two-phase application
- Add Unpublish control and fix publish documentId bug
- Add API Tokens service, endpoints, and Settings UI
- Implement Media Library: backend storage + upload route + UI
- Wire real JWT auth + fix runtime RBAC/API shape
- Align content navigation, CTB, RBAC, and UI with Strapi
- Initial commit: ferriscms — offline-first Strapi clone in Rust (Dioxus multiplatform UI + Axum backend)

## [0.2.0](https://github.com/dinosath/ferris-cms/releases/tag/services-v0.2.0) - 2026-09-15

### Added

- *(build)* rustls-only deps + immutable release artifacts
- *(computed)* bulk create/update endpoints ignore computed values
- *(auth)* wire OIDC SSO into the UI and add mock-IdP E2E test
- *(auth)* add OpenID Connect (SSO) admin login
- auto-provision admin from env and add Helm configmap credentials
- *(metadata)* Kubernetes-style labels/namespace on content types and workflows
- *(ai)* expose content-type/workflow actions to the assistant, chat table + overlay
- *(ai)* test provider connection and auto-discover models
- *(ai)* auto-create a default model per provider and add provider via modal
- validate payload constraints (required/min/max/length/pattern/enum) before handling
- *(ai)* per-conversation privacy mode
- *(ai)* native AI subsystem (providers, assistant, tools, content/schema/media, usage)
- *(import-export)* export filtering
- *(import-export)* configurable CSV delimiter + header row
- *(import-export)* export field selection + reliable field projection
- *(import-export)* persist mapping presets in the database + UI
- *(import-export)* Content Manager contextual actions + prefer-uid prefill
- *(import-export)* add Import & Export system
- *(workflow)* n8n-style visual workflow automation engine

### Fixed

- *(computed)* inline chained computed columns; real Postgres acceptance
- *(ai)* keep tool-call history valid and surface reasoning
- *(ai)* skip orphaned tool messages when rebuilding chat history
- *(ai)* don't silently return empty assistant replies
- *(ai)* resolve conversation provider and make tool names OpenAI-compatible
- *(ctb)* require explicit removal instead of absence-as-deletion

### Other

- *(computed)* E2E CRUD/pagination/export + performance benchmarks
- *(computed)* DDL rendering + migration lifecycle tests
- *(auth)* split provision_admin for testability and cover env bootstrap
- Replace custom workflow engine with Open Workflow Specification (OWS)
- *(import-export)* broaden edge-case and failure-mode coverage
- *(workflow)* CMS content-created trigger + retry coverage
- run cargo fmt across the workspace
- Add unit tests for internal helper branches; backend coverage >90%
- Fix relation/inverse-FK DDL with two-phase application
- Add Unpublish control and fix publish documentId bug
- Add API Tokens service, endpoints, and Settings UI
- Implement Media Library: backend storage + upload route + UI
- Wire real JWT auth + fix runtime RBAC/API shape
- Align content navigation, CTB, RBAC, and UI with Strapi
- Initial commit: ferriscms — offline-first Strapi clone in Rust (Dioxus multiplatform UI + Axum backend)
