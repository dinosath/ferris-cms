# Technology Stack

FerrisCMS is a single Rust codebase that reproduces Strapi's core headless-CMS
workflow. It runs in two modes — an **offline desktop** app and an **online
Axum server** — sharing the same library crates.

## Stack at a glance

| Layer | Technology |
|---|---|
| Language | Rust (edition 2021, MSRV 1.82+) |
| Web server | Axum 0.8 |
| ORM / query builder | SeaORM 2.0 + SeaQuery 1.0 |
| Migrations | `sea-orm-migration` (system) + runtime DDL (user content-types) |
| Database | PostgreSQL 14+ (online) / SQLite (offline, embedded) |
| UI | Dioxus 0.7 (native desktop + WASM web) |
| Async runtime | Tokio |
| Auth | JWT (`jsonwebtoken`) + argon2 password hashing |
| Serialization | serde / serde_json (preserve_order) |
| Error handling | thiserror |
| Logging | tracing / tracing-subscriber (env-filter) |
| Testing | built-in unit + integration tests, Playwright (`playwright-rs`) e2e |
| Packaging | Docker, Docker Compose, Helm, GitHub Actions, release-plz |

## Runtime modes

```
┌─────────────────────────────────────────────────────────────┐
│                     shared library crates                    │
│   core-domain, core-schema, db, dynamic-store, services,     │
│   api-types, client-core, workflow, ai                       │
└──────────────▲──────────────────────────────▲───────────────┘
               │ in-process calls              │ HTTP
   ┌───────────┴──────────┐        ┌───────────┴─────────────┐
   │  desktop-bin (OFFLINE)│        │   server-bin (ONLINE)   │
   │  Dioxus UI + client-core      │   Axum + services        │
   │  services + SQLite (embedded) │   Postgres / SQLite       │
   └────────────────────────┘        └───────────┬─────────────┘
                                                │ HTTP
                                    ┌───────────┴─────────────┐
                                    │  Web admin (Dioxus WASM) │
                                    │  client-core (HTTP)      │
                                    └─────────────────────────┘
```

## Crate map

| Crate | Role |
|---|---|
| `core-domain` | Pure domain types (no I/O): kinds, field types, relations, UID, reserved names. |
| `core-schema` | Schema model, JSON round-trip, structural + payload validation, diffing. |
| `api-types` | Shared request/response DTOs + Strapi-compatible query parser + error envelope. |
| `db` | SeaORM 2.0 system entities, migrations, seeding. |
| `dynamic-store` | Runtime DDL + CRUD for user-defined content types (SeaQuery generated SQL). |
| `workflow` | Pure workflow domain, node registry, expression engine, validation, graph. |
| `ai` | AI provider/model CRUD + chat + tool registry (domain/HTTP layer). |
| `services` | Business logic: CTB, content, media, RBAC, i18n, auth, workflows, AI, import/export. |
| `api-rest` | Axum routers for `/api`, `/admin`, `/content-type-builder`, error mapping. |
| `client-core` | Transport-abstract client the UI calls. |
| `ui` | Framework-agnostic design system: tokens, widgets, screens. |
| `app` | The Dioxus admin UI (web + desktop), calls `client-core`. |
| `server-bin` | Binary: online Axum server (`ferriscms-server`). |
| `desktop-bin` | Binary: offline desktop + embedded server (`ferriscms-desktop`). |
| `e2e` | End-to-end tests (Turso database + Obscura browser, no containers). |

## Dependencies (workspace)

```toml
serde, serde_json (preserve_order), indexmap, thiserror, async-trait, tokio,
tracing, tracing-subscriber, chrono, uuid, regex, once_cell, csv, serde_yaml,
sea-orm (sqlx-sqlite, sqlx-postgres, runtime-tokio-rustls, macros,
          with-chrono, with-json, with-uuid, rbac),
sea-orm-migration, sea-query, axum (multipart, macros),
tower, tower-http (cors, fs, trace, limit),
jsonwebtoken (rust_crypto), argon2, sha2, rand,
playwright-rs (e2e only)
```

See [`TECHNOLOGIES.md`](TECHNOLOGIES.md) for a deeper look at each technology.
