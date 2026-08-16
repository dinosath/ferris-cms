# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/dinosath/ferris-cms/releases/tag/services-v0.2.0) - 2026-08-16

### Other

- Add unit tests for internal helper branches; backend coverage >90%
- Fix relation/inverse-FK DDL with two-phase application
- Add Unpublish control and fix publish documentId bug
- Add API Tokens service, endpoints, and Settings UI
- Implement Media Library: backend storage + upload route + UI
- Wire real JWT auth + fix runtime RBAC/API shape
- Align content navigation, CTB, RBAC, and UI with Strapi
- Initial commit: ferriscms — offline-first Strapi clone in Rust (Dioxus multiplatform UI + Axum backend)
