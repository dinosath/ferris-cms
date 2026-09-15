# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.1](https://github.com/dinosath/ferris-cms/compare/api-rest-v0.3.0...api-rest-v0.3.1) - 2026-09-15

### Added

- *(build)* rustls-only deps + immutable release artifacts
- *(computed)* bulk create/update endpoints ignore computed values
- *(ctb)* computed/generated fields end-to-end
- *(auth)* wire OIDC SSO into the UI and add mock-IdP E2E test
- *(auth)* add OpenID Connect (SSO) admin login
- *(ai)* test provider connection and auto-discover models
- validate payload constraints (required/min/max/length/pattern/enum) before handling
- *(ai)* per-conversation privacy mode
- *(ai)* native AI subsystem (providers, assistant, tools, content/schema/media, usage)
- *(import-export)* persist mapping presets in the database + UI
- *(import-export)* add Import & Export system
- *(workflow)* n8n-style visual workflow automation engine
- embed the Dioxus WASM UI into the server binary as a single executable
- embed and serve Dioxus WASM admin UI from the server image

### Fixed

- *(ctb)* require explicit removal instead of absence-as-deletion
- *(ui)* serve index.html with Cache-Control: no-cache

### Other

- release v0.3.0
- release v0.2.0
- *(computed)* assert metadata exposure (req 4) and write-rejection details
- *(computed)* E2E CRUD/pagination/export + performance benchmarks
- Replace custom workflow engine with Open Workflow Specification (OWS)
- *(workflow)* CMS content-created trigger + retry coverage
- *(auth)* login credential validation and authorized-only UI access
- run cargo fmt across the workspace
- Fix relation/inverse-FK DDL with two-phase application
- Add UI flow tests and expand backend integration coverage
- Add Unpublish control and fix publish documentId bug
- Add Internationalization (locales) settings UI
- Add API Tokens service, endpoints, and Settings UI
- Add Configure-the-view modal to Content Manager list toolbar
- Add Discard changes control to entry edit view
- Implement Media Library: backend storage + upload route + UI
- Add Settings > Users management UI with invite modal
- Add Settings > Roles management UI with permission matrix
- Add durable end-to-end integration test for admin workflow
- Wire real JWT auth + fix runtime RBAC/API shape
- Initial commit: ferriscms — offline-first Strapi clone in Rust (Dioxus multiplatform UI + Axum backend)

## [0.3.0](https://github.com/dinosath/ferris-cms/compare/api-rest-v0.2.0...api-rest-v0.3.0) - 2026-09-15

### Added

- *(build)* rustls-only deps + immutable release artifacts
- *(computed)* bulk create/update endpoints ignore computed values
- *(ctb)* computed/generated fields end-to-end
- *(auth)* wire OIDC SSO into the UI and add mock-IdP E2E test
- *(auth)* add OpenID Connect (SSO) admin login
- *(ai)* test provider connection and auto-discover models
- validate payload constraints (required/min/max/length/pattern/enum) before handling
- *(ai)* per-conversation privacy mode
- *(ai)* native AI subsystem (providers, assistant, tools, content/schema/media, usage)
- *(import-export)* persist mapping presets in the database + UI
- *(import-export)* add Import & Export system
- *(workflow)* n8n-style visual workflow automation engine
- embed the Dioxus WASM UI into the server binary as a single executable
- embed and serve Dioxus WASM admin UI from the server image

### Fixed

- *(ctb)* require explicit removal instead of absence-as-deletion
- *(ui)* serve index.html with Cache-Control: no-cache

### Other

- release v0.2.0
- *(computed)* assert metadata exposure (req 4) and write-rejection details
- *(computed)* E2E CRUD/pagination/export + performance benchmarks
- Replace custom workflow engine with Open Workflow Specification (OWS)
- *(workflow)* CMS content-created trigger + retry coverage
- *(auth)* login credential validation and authorized-only UI access
- run cargo fmt across the workspace
- Fix relation/inverse-FK DDL with two-phase application
- Add UI flow tests and expand backend integration coverage
- Add Unpublish control and fix publish documentId bug
- Add Internationalization (locales) settings UI
- Add API Tokens service, endpoints, and Settings UI
- Add Configure-the-view modal to Content Manager list toolbar
- Add Discard changes control to entry edit view
- Implement Media Library: backend storage + upload route + UI
- Add Settings > Users management UI with invite modal
- Add Settings > Roles management UI with permission matrix
- Add durable end-to-end integration test for admin workflow
- Wire real JWT auth + fix runtime RBAC/API shape
- Initial commit: ferriscms — offline-first Strapi clone in Rust (Dioxus multiplatform UI + Axum backend)

## [0.2.0](https://github.com/dinosath/ferris-cms/releases/tag/api-rest-v0.2.0) - 2026-09-15

### Added

- *(build)* rustls-only deps + immutable release artifacts
- *(computed)* bulk create/update endpoints ignore computed values
- *(ctb)* computed/generated fields end-to-end
- *(auth)* wire OIDC SSO into the UI and add mock-IdP E2E test
- *(auth)* add OpenID Connect (SSO) admin login
- *(ai)* test provider connection and auto-discover models
- validate payload constraints (required/min/max/length/pattern/enum) before handling
- *(ai)* per-conversation privacy mode
- *(ai)* native AI subsystem (providers, assistant, tools, content/schema/media, usage)
- *(import-export)* persist mapping presets in the database + UI
- *(import-export)* add Import & Export system
- *(workflow)* n8n-style visual workflow automation engine
- embed the Dioxus WASM UI into the server binary as a single executable
- embed and serve Dioxus WASM admin UI from the server image

### Fixed

- *(ctb)* require explicit removal instead of absence-as-deletion
- *(ui)* serve index.html with Cache-Control: no-cache

### Other

- *(computed)* assert metadata exposure (req 4) and write-rejection details
- *(computed)* E2E CRUD/pagination/export + performance benchmarks
- Replace custom workflow engine with Open Workflow Specification (OWS)
- *(workflow)* CMS content-created trigger + retry coverage
- *(auth)* login credential validation and authorized-only UI access
- run cargo fmt across the workspace
- Fix relation/inverse-FK DDL with two-phase application
- Add UI flow tests and expand backend integration coverage
- Add Unpublish control and fix publish documentId bug
- Add Internationalization (locales) settings UI
- Add API Tokens service, endpoints, and Settings UI
- Add Configure-the-view modal to Content Manager list toolbar
- Add Discard changes control to entry edit view
- Implement Media Library: backend storage + upload route + UI
- Add Settings > Users management UI with invite modal
- Add Settings > Roles management UI with permission matrix
- Add durable end-to-end integration test for admin workflow
- Wire real JWT auth + fix runtime RBAC/API shape
- Initial commit: ferriscms — offline-first Strapi clone in Rust (Dioxus multiplatform UI + Axum backend)
