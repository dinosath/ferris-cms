# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/dinosath/ferris-cms/releases/tag/core-schema-v0.2.0) - 2026-09-06

### Added

- *(metadata)* Kubernetes-style labels/namespace on content types and workflows
- validate payload constraints (required/min/max/length/pattern/enum) before handling
- conditional field visibility (Strapi conditional fields)

### Other

- run cargo fmt across the workspace
- Initial commit: ferriscms — offline-first Strapi clone in Rust (Dioxus multiplatform UI + Axum backend)
