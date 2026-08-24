# Implementation

This document explains **how FerrisCMS is built**: workspace layout, request
flows, the schema lifecycle, the data layer, auth/RBAC, workflows, AI, and the
testing strategy. It complements [`STACK.md`](STACK.md),
[`TECHNOLOGIES.md`](TECHNOLOGIES.md), and [`FEATURES.md`](FEATURES.md).

## 1. Workspace layout

A Cargo virtual workspace of library crates plus two thin binary crates:

```
ferriscms/
├─ Cargo.toml                # workspace manifest + shared deps
├─ docs/                     # documentation (this folder)
├─ deploy/helm/ferriscms/    # Helm chart
├─ crates/
│  ├─ core-domain/           # pure domain types (kinds, fields, relations, UID)
│  ├─ core-schema/           # schema model, JSON, validation, diffing, payload validation
│  ├─ api-types/             # DTOs + Strapi query parser + error envelope
│  ├─ db/                    # SeaORM system entities, migrations, seeding
│  ├─ dynamic-store/         # runtime DDL + CRUD for user-defined content types
│  ├─ workflow/              # pure workflow domain + expression engine
│  ├─ ai/                    # AI provider/model domain
│  ├─ services/              # business logic
│  ├─ api-rest/              # Axum routers + error mapping
│  ├─ client-core/           # transport-abstract client for the UI
│  ├─ ui/                    # design system (framework-agnostic)
│  ├─ app/                   # Dioxus admin UI (web + desktop)
│  ├─ server-bin/            # online Axum server binary
│  ├─ desktop-bin/           # offline desktop binary
│  └─ e2e/                   # end-to-end tests
```

The dependency direction flows **down**: `core-domain` ← `core-schema` ←
`dynamic-store`/`services` ← `api-rest` ← binaries. `core-domain` and the pure
`workflow`/`ai` crates perform no I/O, which keeps the domain easily testable.

## 2. Request flow

```
HTTP request
   │
   ▼
api-rest router (Axum)        → parses path, query, JSON body
   │  AppState { Arc<AppContext> }
   ▼
handler                     → extracts AdminCtx / ApiTokenCtx / PublicCtx
   │
   ▼
services::<function>        → loads schema from cache, enforces RBAC,
   │                           validates payload, calls dynamic-store
   ▼
dynamic-store (dml/ddl)     → SeaQuery SQL against Postgres/SQLite
   │
   ▼
ServiceError / JSON response → api-rest::error maps ServiceError → HTTP
```

Every handler returns `Result<Json<T>, AppError>`, and `AppError` maps
`ServiceError` variants to Strapi-compatible HTTP responses:

| `ServiceError` | HTTP status | `error.name` |
|---|---|---|
| `Validation` | 400 | `ValidationError` (with `details.errors`) |
| `NotFound` | 404 | `NotFound` |
| `Conflict` | 409 | `Conflict` |
| `Forbidden` / `Rbac` | 403 | `Forbidden` / `RbacError` |
| `Unauthorized` | 401 | `Unauthorized` |
| `Db` / `Store` | 500 | `DatabaseError` / `StoreError` |
| `Internal` | 500 | `InternalError` |

## 3. Shared context

`AppContext` (in `services`) bundles the `DatabaseConnection`, the current user,
`AppConfig`, and a lock-free `SchemaCache`. It is cheap to clone: a per-request
clone shares the same DB and schema-cache handles while carrying its own
`current_user`. `with_user()` produces the per-request identity.

## 4. Schema lifecycle

The Content-Type Builder is the heart of the system:

```
submit schema JSON
   │
   ▼
core-schema::validate_schemas    → structural/cross-schema validation (batch)
   │
   ▼
core-schema::diff                → diff current registry vs. desired
   │
   ▼
dynamic-store::ddl               → Phase 1 host tables, Phase 2 aux tables
   │                                (join/link tables, inverse FK columns)
   │
   ▼
persist schema rows              → upsert content_type_schemas JSON
   │                                (bump version; soft-delete removed uids)
   ▼
rebuild SchemaCache              → atomic replace (arc-swap)
```

### Schema model
`core-schema::Schema` is the canonical model, with a flat Strapi-compatible
`Attribute` (all type-specific payloads are optional members). Attribute
ordering is preserved via `IndexMap`. System columns (createdAt, updatedAt,
publicationState, …) are reserved and use Strapi camelCase keys; content fields
round-trip the user's chosen casing.

### DDL
`dynamic-store::ddl` turns a `SchemaDiff` into SeaQuery statements. Host tables
(`ct_<plural>`, `ct_<singular>`, `cmp_<category>_<name>`) are created/updated
first; auxiliary tables (many-to-many join tables, media link tables, component
link tables, one-to-many inverse FK columns) are applied second so every target
table exists before any `ALTER` references it.

## 5. Data layer

`dynamic-store` is split into:

- `value` — JSON ↔ SQL conversion (`attr_to_value`, `coerce_filter_value`,
  `column_to_json`), row→JSON mapping, and API-key mapping for system columns.
- `dml` — CRUD, filters, sorting, pagination, populate, and link-table helpers.
  Also validates payloads against the schema (`insert_one`/`update_one`) as a
  **defense-in-depth** layer.
- `ddl` — runtime DDL from schema diffs.
- `error` — `StoreError` (includes a `Validation` variant carrying
  `core_schema::PayloadError`s).

### Dynamic tables
Because user content types are created at runtime, `dml` builds SeaQuery
statements rather than using a compile-time ORM. `column_map`/`select_columns`
derive the physical columns and output keys from the schema. Relations, media,
and components are stored via link tables and joined on read.

## 6. Payload validation

A shared validator (`core-schema::payload::validate_payload`) is the single
source of truth for field constraints. It checks:

- **`required`** — missing, `null`, and blank-string values are rejected on
  create/import; not enforced on partial updates.
- **value type** — with leniency for numeric/boolean strings (imports).
- **`min`/`max`** — numeric bounds; also cardinality bounds for repeatable
  components and multi-media.
- **`minLength`/`maxLength`** — character length.
- **`regex`** — pattern matching.
- **`enum`** — membership in the allowed set.

It runs at **three layers**, before a payload is handled:
1. Content CRUD (`cm_create`/`cm_update`) → `ServiceError::Validation`.
2. Import pipeline (before a record is written).
3. Dynamic store (`insert_one`/`update_one`) for defense in depth.

## 7. Auth & RBAC

- **Passwords** — argon2id hashing.
- **JWTs** — HS256 with `sub`/`iat`/`exp`/`jti`, signed with `JWT_SECRET`.
- **`AdminCtx`/`ApiTokenCtx`** — extractors that resolve the caller before a
  handler runs.
- **RBAC** — `services::rbac` initializes standard roles/permissions (SeaORM
  `rbac`) and `enforce_action(db, user, action, subject)` guards every service
  method. The AI subsystem reuses the same RBAC so the LLM is never a security
  boundary.

## 8. Workflow engine

- **Domain** (`workflow` crate): `model`, `node` (registry + static
  definitions), `expression` (n8n-style safe templates), `validation`, and
  `graph` (topological ordering).
- **Execution** (`services::workflow`): `engine`, `executors`, `triggers`,
  `credentials`. Runs node logic against the CMS DB, issues HTTP requests, and
  persists executions + per-node run records.
- **Triggers** — CMS events (`content.created`, `content.updated`, …) and
  public webhooks (`/workflow-hooks/{path}`).
- **Permissions** — workflow/credential/execution actions integrated into RBAC.

## 9. AI subsystem

`services::ai` layers provider/model CRUD (encrypted keys), chat with a
tool-calling loop, an RBAC-aware tool registry, content/schema/media
generation, usage accounting, and a prompt-injection guard. Providers:
OpenAI-compatible, Ollama, Anthropic, Gemini. The model only returns *typed
tool requests*; FerrisCMS authorizes and executes them.

## 10. Import / Export

`services::import_export` orchestrates parse → map → transform → validate →
write. Per-row validation reuses the shared payload validator, and writes go
through the dynamic store (which validates again). Results include per-row
errors with suggested fixes. Mappings can be saved as named presets.

## 11. Testing

- **Unit tests** in each crate (domain, schema validation, payload validation,
  DML, workflow expression/engine).
- **Integration tests** — `api-rest/tests` exercise the full router in-memory:
  an end-to-end admin workflow, API surface, auth, coverage, workflow triggers.
- **dynamic-store integration tests** — DDL + CRUD + filters + payload
  validation against in-memory SQLite.
- **e2e** — `playwright-rs` drives a real browser against a Turso database
  (UI flows, API flows, AI, import/export).

## 12. Build, run, deploy

See [`STACK.md`](STACK.md) and the root `README.md`. In short:

```bash
cargo build --workspace
cargo build -p server-bin -p desktop-bin      # runtime binaries
cd crates/app && dx build --web               # web UI
cd crates/app && dx build --desktop --features desktop   # desktop UI
cargo test --workspace                        # tests
cargo run -p server-bin                        # online server (PostgreSQL)
cargo run -p desktop-bin                       # offline desktop
docker build -t ferriscms-server .             # single-binary image
helm install ferriscms deploy/helm/ferriscms   # Kubernetes
```

Environment variables for the server: `DATABASE_URL`, `BIND_ADDR`,
`JWT_SECRET`, `MEDIA_STORAGE_DIR`, `FERRISCMS_UI_DIR` (dev UI directory),
`TLS_CERT_FILE`/`TLS_KEY_FILE` (optional HTTPS).
