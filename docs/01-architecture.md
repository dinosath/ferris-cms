# 01 — System Architecture

Prerequisite reading: [00-overview.md](00-overview.md).

This document defines the runtime architecture, the responsibilities of each crate, and the exact request lifecycles. It is the contract every other doc builds on.

---

## 1. The two runtimes (and why the core is shared)

`ferriscms` ships as **two binaries** built from **one set of library crates**.

```
                         ┌──────────────────────────────────────┐
                         │            shared library crates       │
                         │  core-domain, core-schema, db,         │
                         │  dynamic-store, services, api-types    │
                         └──────────────────────────────────────┘
                             ▲                          ▲
             in-process calls │                          │ HTTP (reqwest/axum)
                             │                          │
   ┌─────────────────────────┴────────┐     ┌───────────┴───────────────────────┐
   │        desktop-bin (OFFLINE)       │     │          server-bin (ONLINE)       │
   │  Dioxus UI  ─►  client-core       │     │  Axum(api-rest) ─► services        │
   │        (InProcess transport)       │     │  DB: Postgres or SQLite            │
   │        services + SQLite (embedded)│     │  Serves WASM UI bundle             │
   └────────────────────────────────────┘     └────────────────────────────────────┘
                                                          ▲
                                                          │ HTTP
                                              ┌───────────┴───────────┐
                                              │  Web admin (Dioxus    │
                                              │  compiled to WASM)     │
                                              │  client-core (Http)    │
                                              └────────────────────────┘
```

Key idea: the **UI always talks to `client-core`**, never directly to a database or to Axum. `client-core` has two implementations of a single `ApiTransport` trait:

- `InProcessTransport` — used by `desktop-bin`; calls `services` functions directly (no serialization overhead, no server).
- `HttpTransport` — used by the WASM web admin; calls the Axum REST endpoints over HTTP.

This is the single most important architectural decision: **the UI cannot tell whether it is online or offline.** Same screens, same DTOs, same behavior.

---

## 2. Crate responsibilities (authoritative)

### `core-domain`
Pure Rust types with no IO and no framework dependencies. Holds the conceptual model:
- `ContentTypeKind { Collection, Single }`
- `FieldType` enum (all Strapi field types — see [03](03-content-type-builder-logic.md)).
- `RelationKind { OneWay, OneToOne, OneToMany, ManyToOne, ManyToMany, ManyWay }`
- `Uid`, `ApiId` newtypes with validation.
Depends on: `serde` only. Everything else depends on this.

### `core-schema`
The **schema model** and its JSON (de)serialization + structural validation:
- `Schema { uid, kind, info, options, attributes: IndexMap<String, Attribute> }`
- `Attribute` (tagged enum matching Strapi's `type` discriminator).
- `ComponentSchema`, `DynamicZoneAttribute`.
- Validators: naming rules, reserved-word checks, relation target existence, DZ component-field-collision rule (Strapi caution: components in a DZ cannot share a field name with differing types).
- Diffing: `SchemaDiff` computing added/modified/deleted attributes between two schema versions (drives DDL + the CTB "New/Modified/Deleted" status badges).
Depends on: `core-domain`.

### `db`
SeaORM 2.0 layer for **fixed system tables** (users, roles, permissions, schemas registry, media, tokens, etc. — see [02](02-data-model.md)). Uses the **dense entity format** (`#[sea_orm::model]`, `HasMany`/`BelongsTo` typed relations). Owns:
- `Database` connection pool wrapper (Postgres or SQLite via feature flags).
- `sea-orm-migration` migrations for system tables.
- On startup: `schema_registry.sync(db)` (SeaORM 2.0 entity-first) to converge system tables.
Depends on: `core-domain`, SeaORM.

### `dynamic-store`
CRUD + DDL for **user-defined content-types** whose tables do not exist at compile time. Cannot use static SeaORM entities; instead uses **SeaQuery 1.0** to build DDL (`CREATE/ALTER TABLE`) and DML at runtime, executed through the shared SeaORM connection.
- `apply_schema(schema, diff)` → generates and runs DDL.
- `insert_entry / update_entry / find_entries / delete_entry` → dynamic DML returning `serde_json::Value` rows.
- Handles join tables for relations, component-link tables, and DZ storage.
Depends on: `core-schema`, `db`.

### `services`
All business logic. **The only crate the transports call.** Functions are transport-agnostic (`async fn ... -> Result<Dto, ServiceError>`), take a `&AppContext` (db handle, current user, config).
Sub-modules:
- `content_type_builder` — create/update/delete CTs & components; orchestrates `core-schema` validation + `dynamic-store` DDL + `db` registry writes, all in one transaction.
- `content` — entry CRUD, filtering, sorting, pagination, populate, draft/publish, i18n locale handling.
- `media` — upload, folders, image metadata.
- `auth` — admin login, JWT issue/verify, API token CRUD.
- `rbac` — role/permission checks.
- `i18n` — locale registry, per-locale entry variants.
- `sync` — offline↔online reconciliation (see [07](07-offline-sync.md)).
Depends on: everything below it.

### `api-types`
Serde DTOs shared by server and client so the wire contract is defined **once**. Request bodies, response envelopes (Strapi's `{ data, meta }` shape), error shapes, pagination meta.
Depends on: `core-domain` (for enums).

### `api-rest`
Axum routers mapping HTTP → `services`. Three route groups:
- `/api/**` — public content REST API (Strapi-compatible).
- `/admin/**` — admin/management API (auth, users, roles, media, content-manager).
- `/content-type-builder/**` — schema CRUD.
Owns middleware: auth extraction, RBAC guard, error → HTTP mapping, CORS, rate limiting.
Depends on: `services`, `api-types`.

### `client-core`
The UI-facing SDK. Defines `trait ApiTransport` with one method per logical operation (or a generic `request(Endpoint) -> Response`). Two impls: `HttpTransport` (reqwest/`gloo-net` on WASM) and `InProcessTransport` (wraps `services`). Also holds a small client-side cache + optimistic update helpers for the CTB "unsaved changes" model.
Depends on: `api-types`; `InProcessTransport` also depends on `services`.

### `ui`
Dioxus application. Design system + widgets + screens. Calls `client-core` only. See [05](05-ui-design-system.md) and [06](06-ui-screens.md).
Depends on: `client-core`, `api-types`, Dioxus.

### `server-bin` / `desktop-bin`
Thin wiring binaries. `server-bin` boots Tokio + Axum + Postgres. `desktop-bin` boots Dioxus + embedded SQLite + `InProcessTransport`.

---

## 3. Dependency graph (must remain acyclic)

```
core-domain
   ├─► core-schema
   ├─► api-types
   └─► db ─► dynamic-store
                 └─► services ─► api-rest ─► server-bin
                        │            └─► (serves) ui(wasm)
                        └─► client-core ─► ui ─► desktop-bin
```

Rule: lower crates never depend on higher crates. `ui` never imports `services`, `db`, or `dynamic-store` directly — only `client-core` + `api-types`.

---

## 4. Request lifecycle: reading content (online)

Example: `GET /api/articles?populate=author&filters[title][$contains]=rust&pagination[page]=1`

1. **Axum** matches route in `api-rest`; auth middleware validates API token / permission.
2. Handler parses Strapi-style query params (`filters`, `populate`, `sort`, `pagination`, `fields`, `locale`, `status`) into a typed `QueryParams` (in `api-types`).
3. Calls `services::content::find_many(ctx, "api::article.article", query)`.
4. `services` loads the CT schema from the registry (cached), asks `dynamic-store` to build a SeaQuery `SELECT` with WHERE/ORDER/LIMIT + join population.
5. `dynamic-store` executes against the pool, returns `Vec<serde_json::Value>` shaped to the schema (relations/components nested).
6. `services` applies field-level read permissions + draft/publish + locale filtering, wraps into `{ data: [...], meta: { pagination } }`.
7. Handler serializes DTO → JSON response.

Offline path is identical except step 1 is an in-process function call via `InProcessTransport`; steps 3–6 are byte-for-byte the same code.

---

## 5. Request lifecycle: creating a content-type (the hard one)

Example: admin adds a `title` (Text) + `author` (relation → user) to a new `Article` collection type and clicks **Save**.

1. UI accumulates **unsaved edits** locally (CTB is a staging model — nothing hits the DB until Save). See [03 §7](03-content-type-builder-logic.md).
2. On Save, UI sends the full desired schema list to `POST /content-type-builder/content-types` (batched).
3. `services::content_type_builder::apply(ctx, desired_schemas)`:
   a. Validate each schema via `core-schema` (naming, relation targets, DZ rules).
   b. Load current schemas from registry; compute `SchemaDiff` per CT.
   c. **Open one DB transaction.**
   d. For each diff, call `dynamic-store::apply_schema` → emit `CREATE TABLE` / `ALTER TABLE ADD COLUMN` / join-table creation / drop for deletions.
   e. Upsert the schema JSON into `content_type_schemas` registry.
   f. Commit. On any error, roll back — nothing is half-applied.
4. Rebuild the in-memory schema cache + regenerate the route table so `/api/articles` starts serving immediately (no restart — Strapi restarts; we hot-reload the router).
5. Return updated schemas; UI clears "unsaved" badges.

> Because SeaORM 2.0 supports **entity-first `sync`**, we use it for the *system* tables. For *user* tables the shape is unknown at compile time, so `dynamic-store` generates DDL directly. Both go through the same connection/transaction.

---

## 6. Offline/online configuration

A single `Config` (from env/TOML/CLI) selects mode:

| Setting | Offline (`desktop-bin`) | Online (`server-bin`) |
|---|---|---|
| `database.driver` | `sqlite` (file in app data dir) | `postgres` (or `sqlite`) |
| `transport` | in-process | HTTP (Axum listens) |
| `ui` | Dioxus native window | Dioxus WASM served at `/admin` |
| `media.storage` | local filesystem | local FS or `[LATER]` S3-compatible |
| `sync.enabled` | optional; points at a remote server | acts as sync target |

The core services do not branch on mode beyond the `Database` driver and the media storage backend, both injected via traits.

---

## 7. Concurrency & state

- Server: standard Tokio + connection pool; schema cache behind `arc-swap` for lock-free reads, rebuilt on CTB Save.
- Desktop: Dioxus runs the UI on the main thread; `client-core` HTTP calls run as spawned async tasks on the Dioxus runtime. No blocking the render thread.

---

## 8. Error model

`ServiceError` (in `services`) is the single internal error enum: `Validation(Vec<FieldError>)`, `NotFound`, `Conflict`, `Forbidden`, `Unauthorized`, `Db(...)`, `Internal(...)`. `api-rest` maps it to Strapi-compatible HTTP bodies:

```json
{ "data": null, "error": { "status": 400, "name": "ValidationError", "message": "...", "details": { "errors": [ ... ] } } }
```

The UI reads the same shape via `client-core` regardless of transport.

---

## 9. What is explicitly deferred `[LATER]`

- GraphQL API (Strapi has one; we add after REST is complete).
- Plugins/marketplace, Strapi AI assistant, custom fields SDK.
- Webhooks, audit logs, review workflows (Enterprise features).
- S3 media storage, image transformation pipeline beyond thumbnails.
These are noted where relevant but are **out of scope for phases 1–5** in [08-roadmap.md](08-roadmap.md).
