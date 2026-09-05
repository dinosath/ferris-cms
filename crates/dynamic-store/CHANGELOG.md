# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/dinosath/ferris-cms/releases/tag/dynamic-store-v0.2.0) - 2026-09-05

### Added

- *(metadata)* Kubernetes-style labels/namespace on content types and workflows
- validate payload constraints (required/min/max/length/pattern/enum) before handling

### Fixed

- *(dynamic-store)* update crud test to read relation FK under attribute name
- *(read)* decode Postgres INT4 integer columns on read
- *(import/read)* return schema attribute names so imported fields display
- *(store)* decode Postgres NUMERIC decimal columns on read
- resolve web API URLs, Postgres timestamp types; add playwright e2e suite

### Other

- run cargo fmt across the workspace
- Fix relation/inverse-FK DDL with two-phase application
- Wire real JWT auth + fix runtime RBAC/API shape
- Initial commit: ferriscms — offline-first Strapi clone in Rust (Dioxus multiplatform UI + Axum backend)
