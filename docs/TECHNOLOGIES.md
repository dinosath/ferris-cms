# Technologies

This page explains each technology in the stack, what it is used for, and why
it was chosen. It complements the summary in [`STACK.md`](STACK.md).

## Language & tooling

### Rust (edition 2021)
The entire system — backend, frontend, tooling, tests — is written in one
language. This gives a single shared domain model across all layers, memory
safety, and a self-contained binary. The workspace targets MSRV **1.82+** and
is managed with Cargo as a virtual workspace of crates.

### Cargo workspace
The workspace splits the code into focused crates (`core-domain`,
`core-schema`, `db`, `dynamic-store`, `services`, `api-rest`, etc.). Shared
dependencies are declared once in the root `Cargo.toml` and reused via
`workspace = true`, keeping versions consistent.

## Backend

### Axum 0.8 (web framework)
Axum provides the HTTP layer for the online server. It is built on `tower`
and `hyper` and integrates cleanly with Tokio. Routers are grouped into the
public API (`/api/**`), the admin API (`/admin/**`), the Content-Type Builder
(`/content-type-builder/**`), workflow webhooks, media serving, and the
embedded UI. Handlers receive shared `AppState` (an `Arc<AppContext>`) and
return `Result<Json<T>, AppError>`.

### SeaORM 2.0 + SeaQuery 1.0 (database access)
- **SeaORM** is used for the fixed, system tables (admin users, roles,
  permissions, content-type schema rows, media, workflows, i18n locales,
  import/export presets, sync metadata). Its `with-rbac` feature backs the
  RBAC engine.
- **SeaQuery** generates all SQL for **user-defined content types**, whose
  tables exist only at runtime and therefore cannot use a compile-time ORM.
  `dynamic-store` builds `SELECT`/`INSERT`/`UPDATE`/`DELETE` statements and
  maps rows to JSON.

### sea-orm-migration
System schema is versioned with SeaORM migrations run on boot. User content
types are applied at runtime with generated DDL instead (see
`dynamic-store`).

### Tokio (async runtime)
Tokio powers the async server, the executor for workflow runs, and async DB
access. The workflow and AI execution paths run as background tasks.

### tracing / tracing-subscriber
Structured, leveled logging across the server with `EnvFilter` so log
verbosity can be tuned via `RUST_LOG`.

### thiserror
Error types (`ServiceError`, `StoreError`) are derived with `thiserror`,
giving clean `Display` implementations and `From` conversions.

## Data & storage

### PostgreSQL 14+ (online)
The webserver persists to PostgreSQL. SeaORM connects with a tuned pool
(idle timeout, connection recycling, `test_before_acquire`) to avoid stale
connection errors.

### SQLite (offline / desktop)
The desktop binary uses an embedded SQLite database, so it runs with no
server and no configuration. The same schema and CRUD code path works for
both backends via SeaORM/SeaQuery's backend abstraction.

### serde / serde_json (with `preserve_order`)
All wire and storage JSON round-trips through serde. `preserve_order` keeps
schema attribute ordering stable (important for Strapi compatibility), and
`indexmap` backs ordered maps.

## Frontend

### Dioxus 0.7
The admin UI is written in Dioxus and compiles to **two targets**:
- **WASM** — served by the Axum server (embedded with `rust-embed` in release)
  or from `dx serve` in development.
- **Native desktop** — via the `desktop` feature.

The UI is split into a framework-agnostic design system (`ui` crate) and the
Dioxus app (`app` crate), and talks to the backend through `client-core`
(HTTP on web, same-origin; direct to `FERRISCMS_API_URL` on desktop).

### Dioxus CLI (`dx`)
Used to build/serve the web and desktop UI. The app's `Dioxus.toml` lives in
`crates/app/`.

## Auth & security

### jsonwebtoken (JWT)
Admin sessions and API tokens are HS256 JWTs carrying `sub`, `iat`, `exp`,
and `jti`. Signing uses `JWT_SECRET`.

### argon2
Passwords are hashed with argon2id using a fixed salt at registration/login.

### SeaORM RBAC
The `rbac` feature powers role-based access control: roles, permissions, and
user overrides. Every service method enforces actions against the current
user before acting.

## Automation

### Workflow engine (`workflow` crate + `services::workflow`)
A pure domain crate (`workflow`) defines the workflow model, node registry,
n8n-style expression engine, validation, and graph topology. `services`
implements execution (DB access, HTTP, CMS integration), triggers
(content-created/updated/webhook), credentials, and executions.

### AI subsystem (`ai` crate + `services::ai`)
Provider abstraction (OpenAI-compatible, Ollama, Anthropic, Gemini), a
persisted provider/model registry with encrypted keys, a chat/tool-calling
loop, an RBAC-aware tool registry, content/schema/media generation, usage
accounting, and a prompt-injection guard. See [`FEATURES.md`](FEATURES.md).

## Testing

- **Rust unit + integration tests** across the crates; a full admin workflow
  integration suite (`crates/api-rest/tests/auth_workflow.rs`), API surface,
  coverage, dynamic-store CRUD, and workflow engine tests.
- **`playwright-rs`** drives a real browser (via the Obscura browser wrapper,
  no containers) for UI e2e flows against a Turso database
  (`crates/e2e/tests/`).

## Packaging & delivery

- **Docker** — a `Dockerfile` produces a single self-contained server binary
  embedding the WASM UI.
- **Docker Compose** — local profile (build from Dockerfile) and registry
  profile (pull prebuilt image from GHCR), both with a PostgreSQL service.
- **Helm** — a chart in `deploy/helm/ferriscms/` for Kubernetes.
- **GitHub Actions + release-plz** — CI builds/pushes images and OCI charts
  to GHCR; release-plz handles versioning (nothing is published to crates.io).
