# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.2](https://github.com/dinosath/ferris-cms/compare/dynamic-store-v0.3.1...dynamic-store-v0.3.2) - 2026-09-17

### Added

- *(ctb)* computed/generated fields end-to-end
- *(metadata)* Kubernetes-style labels/namespace on content types and workflows
- validate payload constraints (required/min/max/length/pattern/enum) before handling

### Fixed

- *(computed)* inline chained computed columns; real Postgres acceptance
- *(dynamic-store)* update crud test to read relation FK under attribute name
- *(read)* decode Postgres INT4 integer columns on read
- *(import/read)* return schema attribute names so imported fields display
- *(store)* decode Postgres NUMERIC decimal columns on read
- resolve web API URLs, Postgres timestamp types; add playwright e2e suite

### Other

- release v0.3.1
- release v0.3.0
- release v0.2.0
- *(computed)* assert remaining public outputs (JSON contract, defaults, DDL)
- *(computed)* DDL rendering + migration lifecycle tests
- run cargo fmt across the workspace
- Fix relation/inverse-FK DDL with two-phase application
- Wire real JWT auth + fix runtime RBAC/API shape
- Initial commit: ferriscms — offline-first Strapi clone in Rust (Dioxus multiplatform UI + Axum backend)

## [0.3.1](https://github.com/dinosath/ferris-cms/compare/dynamic-store-v0.3.0...dynamic-store-v0.3.1) - 2026-09-15

### Added

- *(ctb)* computed/generated fields end-to-end
- *(metadata)* Kubernetes-style labels/namespace on content types and workflows
- validate payload constraints (required/min/max/length/pattern/enum) before handling

### Fixed

- *(computed)* inline chained computed columns; real Postgres acceptance
- *(dynamic-store)* update crud test to read relation FK under attribute name
- *(read)* decode Postgres INT4 integer columns on read
- *(import/read)* return schema attribute names so imported fields display
- *(store)* decode Postgres NUMERIC decimal columns on read
- resolve web API URLs, Postgres timestamp types; add playwright e2e suite

### Other

- release v0.3.0
- release v0.2.0
- *(computed)* assert remaining public outputs (JSON contract, defaults, DDL)
- *(computed)* DDL rendering + migration lifecycle tests
- run cargo fmt across the workspace
- Fix relation/inverse-FK DDL with two-phase application
- Wire real JWT auth + fix runtime RBAC/API shape
- Initial commit: ferriscms — offline-first Strapi clone in Rust (Dioxus multiplatform UI + Axum backend)

## [0.3.0](https://github.com/dinosath/ferris-cms/compare/dynamic-store-v0.2.0...dynamic-store-v0.3.0) - 2026-09-15

### Added

- *(ctb)* computed/generated fields end-to-end
- *(metadata)* Kubernetes-style labels/namespace on content types and workflows
- validate payload constraints (required/min/max/length/pattern/enum) before handling

### Fixed

- *(computed)* inline chained computed columns; real Postgres acceptance
- *(dynamic-store)* update crud test to read relation FK under attribute name
- *(read)* decode Postgres INT4 integer columns on read
- *(import/read)* return schema attribute names so imported fields display
- *(store)* decode Postgres NUMERIC decimal columns on read
- resolve web API URLs, Postgres timestamp types; add playwright e2e suite

### Other

- release v0.2.0
- *(computed)* assert remaining public outputs (JSON contract, defaults, DDL)
- *(computed)* DDL rendering + migration lifecycle tests
- run cargo fmt across the workspace
- Fix relation/inverse-FK DDL with two-phase application
- Wire real JWT auth + fix runtime RBAC/API shape
- Initial commit: ferriscms — offline-first Strapi clone in Rust (Dioxus multiplatform UI + Axum backend)

## [0.2.0](https://github.com/dinosath/ferris-cms/releases/tag/dynamic-store-v0.2.0) - 2026-09-15

### Added

- *(ctb)* computed/generated fields end-to-end
- *(metadata)* Kubernetes-style labels/namespace on content types and workflows
- validate payload constraints (required/min/max/length/pattern/enum) before handling

### Fixed

- *(computed)* inline chained computed columns; real Postgres acceptance
- *(dynamic-store)* update crud test to read relation FK under attribute name
- *(read)* decode Postgres INT4 integer columns on read
- *(import/read)* return schema attribute names so imported fields display
- *(store)* decode Postgres NUMERIC decimal columns on read
- resolve web API URLs, Postgres timestamp types; add playwright e2e suite

### Other

- *(computed)* assert remaining public outputs (JSON contract, defaults, DDL)
- *(computed)* DDL rendering + migration lifecycle tests
- run cargo fmt across the workspace
- Fix relation/inverse-FK DDL with two-phase application
- Wire real JWT auth + fix runtime RBAC/API shape
- Initial commit: ferriscms — offline-first Strapi clone in Rust (Dioxus multiplatform UI + Axum backend)
