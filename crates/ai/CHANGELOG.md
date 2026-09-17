# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.1](https://github.com/dinosath/ferris-cms/compare/ai-v0.3.0...ai-v0.3.1) - 2026-09-15

### Other

- update Cargo.toml dependencies

## [0.3.0](https://github.com/dinosath/ferris-cms/compare/ai-v0.2.0...ai-v0.3.0) - 2026-09-15

### Other

- update Cargo.toml dependencies

## [0.2.0](https://github.com/dinosath/ferris-cms/releases/tag/ai-v0.2.0) - 2026-09-15

### Added

- *(build)* rustls-only deps + immutable release artifacts
- *(ai)* test provider connection and auto-discover models
- *(ai)* run all LLM calls through Rig (rig.rs)
- *(ai)* native AI subsystem (providers, assistant, tools, content/schema/media, usage)

### Fixed

- *(ai)* keep tool-call history valid and surface reasoning

### Other

- drop aws-lc-rs, run rig's reqwest on ring
