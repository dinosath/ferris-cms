# ferriscms

A **1:1, offline-first clone of [Strapi](https://github.com/strapi/strapi)** built entirely in **Rust**.

ferriscms reproduces Strapi's core headless-CMS workflow with a single Rust
codebase that runs in two modes:

- **Offline desktop** — an embedded SQLite database, no server, no config.
- **Online server** — an Axum server on PostgreSQL (or SQLite) serving a
  Strapi-compatible REST API plus the Dioxus admin UI.

```
┌─────────────────────────────────────────────────────────────┐
│                     shared library crates                    │
│   core-domain, core-schema, db, dynamic-store, services,     │
│   api-types, client-core                                     │
└──────────────▲──────────────────────────────▲───────────────┘
               │ in-process calls              │ HTTP
   ┌───────────┴──────────┐        ┌───────────┴─────────────┐
   │  desktop-bin (OFFLINE)│        │   server-bin (ONLINE)   │
   │  Dioxus UI ─ client-core     │   Axum ─ services        │
   │  services + SQLite (embedded)│   Postgres / SQLite       │
   └────────────────────────┘        └───────────┬─────────────┘
                                                │ HTTP
                                    ┌───────────┴─────────────┐
                                    │  Web admin (Dioxus WASM) │
                                    │  client-core (HTTP)      │
                                    └─────────────────────────┘
```

---

## Features

The project implements the "core" of Strapi (no code generation, plugin
system, or GraphQL yet):

- **Content-Type Builder** — visually define collection types, single types,
  components, and dynamic zones at runtime. Field types include text, rich
  text (Markdown/Blocks), number, date, boolean, email, password, enum,
  JSON, UID, media, relation, component, and dynamic zone. All six relation
  kinds, `New / Modified / Deleted` staging badges, undo/redo/discard.
- **Content Manager** — schema-driven entry forms, list/edit views, filters,
  sorting, pagination, **Draft & Publish**, and discard-draft controls.
- **Dynamic schema → real tables** — schemas are stored as JSON and applied to
  the database at runtime (SeaQuery-generated DDL). No manual SQL.
- **Strapi-compatible REST API** — `filters`, `populate`, `sort`,
  `pagination`, `fields`, `locale`, and `status` query params, with Strapi's
  `{ data, meta }` envelope shape.
- **Media Library** — upload files, list/delete, served from `/uploads`.
- **RBAC** — roles and a permission matrix (Super Admin, Editor, Author seeded).
- **i18n** — locale registry and localized content variants.
- **API Tokens** — read-only/full/custom tokens for the public API.
- **JWT admin auth** — first-run registration + login (argon2 password hashing).

---

## Tech stack

| Layer | Technology |
|---|---|
| Language | Rust (edition 2021) |
| Web server | [Axum](https://docs.rs/axum) 0.8 |
| ORM / query builder | [SeaORM](https://www.sea-ql.org) 2.0 + [SeaQuery](https://docs.rs/sea-query) 1.0 |
| Migrations | `sea-orm-migration` (system) + runtime DDL (user content-types) |
| Database | PostgreSQL 14+ (online) / SQLite (offline, embedded) |
| UI | [Dioxus](https://dioxuslabs.com) 0.7 (native desktop + WASM web) |
| Async runtime | Tokio |
| Auth | JWT (`jsonwebtoken`) + `argon2` password hashing |
| Serialization | `serde` / `serde_json` |

---

## Project layout

A Cargo workspace of library crates plus two thin binary crates:

```
ferriscms/
├─ Cargo.toml                  # workspace manifest + shared dependencies
├─ docs/                       # planning / design docs (see below)
├─ crates/
│  ├─ core-domain/             # pure domain types (no IO): kinds, fields, relations, UID
│  ├─ core-schema/             # schema model, JSON (de)serialization, validation, diffing
│  ├─ api-types/               # shared request/response DTOs + Strapi query parser
│  ├─ db/                      # SeaORM 2.0 system entities, migrations, seeding
│  ├─ dynamic-store/           # runtime DDL + CRUD for user-defined content-types
│  ├─ services/                # business logic (CTB, content, media, rbac, i18n, auth)
│  ├─ api-rest/                # Axum routers: /api, /admin, /content-type-builder
│  ├─ client-core/             # transport-abstract client the UI calls
│  ├─ ui/                      # design system: tokens, widgets, screens (framework-agnostic)
│  ├─ app/                     # Dioxus admin UI (web + desktop), calls client-core
│  ├─ server-bin/              # binary: online Axum server (ferriscms-server)
│  └─ desktop-bin/             # binary: offline desktop + embedded server (ferriscms-desktop)
```

---

## Installation

### Prerequisites

- **Rust** — the workspace targets Rust edition 2021; the docs assume
  **MSRV 1.82+**. Installed with [rustup](https://rustup.rs).
- **Dioxus CLI (`dx`)** — only needed for the web/desktop **UI** build:
  ```bash
  cargo install dioxus-cli
  ```
- **PostgreSQL** — optional; only if you run the server against Postgres
  instead of the default SQLite.

### Build

Build the entire workspace (backend + UI libraries):

```bash
cargo build --workspace
```

Build just the two runtime binaries:

```bash
cargo build -p server-bin -p desktop-bin
```

Build the Dioxus admin UI:

```bash
# native desktop app
dx build --desktop

# web (WASM) bundle -> target/dx/ferriscms/debug/web
dx build --web
```

### Test

```bash
cargo test --workspace
```

Tests include a full end-to-end admin workflow against the Axum router
in-memory (`crates/api-rest/tests/auth_workflow.rs`) and dynamic-store CRUD
integration tests.

---

## Running

### Online server (`ferriscms-server`)

Starts the Axum REST API + admin API on the configured port, running
migrations and seeding roles/locales on boot.

```bash
cargo run -p server-bin
```

| Env var | Default | Purpose |
|---|---|---|
| `DATABASE_URL` | `sqlite:ferriscms.db?mode=rwc` | DB connection (use a `postgres://…` URL for Postgres) |
| `BIND_ADDR` | `0.0.0.0:1337` | HTTP listen address |
| `JWT_SECRET` | `change-me-in-production` | HS256 signing secret (set in production!) |
| `MEDIA_STORAGE_DIR` | `media` | Directory for uploaded files |

On first boot, register the first admin via `POST /admin/register-admin`
(or use the UI). Then log in through `POST /admin/login` to get a JWT.

### Offline desktop (`ferriscms-desktop`)

Runs the full backend (SQLite + migrations + seed) and exposes the REST API
on a local HTTP port — no external database or server required.

```bash
cargo run -p desktop-bin
```

| Env var | Default | Purpose |
|---|---|---|
| `STRAPI_DB_PATH` | `ferriscms-desktop.db` | Path to the local SQLite file |
| `STRAPI_BIND_ADDR` | `127.0.0.1:1338` | Local HTTP listen address |
| `JWT_SECRET` | `desktop-local-dev` | Signing secret |
| `MEDIA_STORAGE_DIR` | `media` | Upload directory |

### Admin UI

The Dioxus admin UI talks to the backend through `client-core`. On **web**
it uses same-origin requests; on **desktop/native** it targets
`FERRISCMS_API_URL` (default `http://127.0.0.1:1337`).

```bash
# Dev server (hot reload) for the web UI — default http://localhost:8080
dx serve

# Native desktop app
dx run --desktop
# or: build then point FERRISCMS_API_URL at a running server
FERRISCMS_API_URL=http://127.0.0.1:1337 ./target/debug/ferriscms
```

> The web build is a separate static bundle. Run it alongside the server
> (`cargo run -p server-bin`) so the UI can reach the API, or host the
> `target/dx/ferriscms/debug/web` output on any static server that proxies
> `/api`, `/admin`, and `/content-type-builder` to the Axum process.

---

## Project design

Detailed design lives in **`docs/`**. Read them in order:

1. [`00-overview.md`](docs/00-overview.md) — what we're building, stack, glossary.
2. [`01-architecture.md`](docs/01-architecture.md) — the two-runtime architecture,
   crate responsibilities, request lifecycles, offline/online split.
3. [`02-data-model.md`](docs/02-data-model.md) — SeaORM 2.0 system entities and
   how dynamic content-type tables are generated.
4. [`03-content-type-builder-logic.md`](docs/03-content-type-builder-logic.md) —
   field taxonomy, canonical schema JSON, DDL generation, validation rules.
5. [`04-rest-api.md`](docs/04-rest-api.md) — every endpoint, query param, auth,
   error shape (Strapi-compatible).
6. [`05-ui-design-system.md`](docs/05-ui-design-system.md) — Dioxus design tokens
   and base widgets (pixel-level).
7. [`06-ui-screens.md`](docs/06-ui-screens.md) — screen-by-screen layout specs.
8. [`07-offline-sync.md`](docs/07-offline-sync.md) — offline embedded mode + sync.
9. [`08-roadmap.md`](docs/08-roadmap.md) — phased milestones and acceptance criteria.

[`docs/design.md`](docs/design.md) is the consolidated master design document.

### Architecture summary

The single most important decision: **the UI always talks to `client-core`**,
never directly to a database or to Axum. `client-core` defines an
`ApiTransport` trait with two implementations:

- `HttpTransport` — used by the WASM web admin; calls the Axum REST API.
- `InProcessTransport` — used by the desktop app; calls `services` directly.

So the UI cannot tell whether it is online or offline — same screens, same
DTOs, same behavior. Domain and core logic are shared; only the transport and
the database driver differ.

Because content-types are **dynamic** (defined at runtime by admins), their
tables do not exist at compile time. `core-schema` models them in memory,
`dynamic-store` turns a schema diff into real DDL (`CREATE/ALTER TABLE`,
relation join tables, component link tables) via SeaQuery, and `services`
orchestrates validate → diff → apply → registry update in a single
transaction. New content-types go live immediately without a restart.

---

## Implementation

The workspace is ~14k lines of Rust. Crate responsibilities are authoritative
in [`docs/01-architecture.md`](docs/01-architecture.md):

| Crate | Responsibility |
|---|---|
| `core-domain` | Pure types: `ContentTypeKind`, `FieldType`, `RelationKind`, `Uid`, `ApiId`. Depends on `serde` only. |
| `core-schema` | `Schema`/`Attribute` model, JSON (de)serialization, structural validation, `SchemaDiff` (added/modified/removed attributes). |
| `db` | SeaORM 2.0 dense entities for system tables (users, roles, permissions, schema registry, media, tokens, locales), migrations, seeding. |
| `dynamic-store` | SeaQuery DDL + DML for runtime-defined tables; returns `serde_json::Value` rows; join/link tables for relations, components, DZ, media. |
| `services` | All business logic — the only crate the transports call. Sub-modules: `content_type_builder`, `content`, `media`, `auth`, `api_tokens`, `rbac`, `i18n`, `schema_cache`. |
| `api-types` | Serde DTOs + Strapi query parser, shared by server and client so the wire contract is defined once. |
| `api-rest` | Axum routers (`/api/**` public, `/admin/**` management, `/content-type-builder/**` schema, `/uploads/**` media) + auth/RBAC/error middleware. |
| `client-core` | `ApiTransport` trait + `HttpTransport`/`InProcessTransport` impls. |
| `ui` | Design system: tokens, widgets, screens. |
| `app` | Dioxus admin UI (web + desktop). |
| `server-bin` / `desktop-bin` | Thin wiring binaries. |

The dependency graph is strictly acyclic — lower crates never depend on higher
crates, and `ui`/`app` never import `services`, `db`, or `dynamic-store`
directly.

### Implemented milestones

Progress follows the phased roadmap in `docs/08-roadmap.md`. Currently
implemented (via `cargo test --workspace`):

- Content-Type Builder engine (create/edit/delete collection types + apply to DB).
- Content Manager list/edit/publish/unpublish/discard, view configuration.
- REST API with Strapi query params + response envelope.
- Admin auth (register/login), RBAC roles/permissions, admin users.
- API tokens, i18n locales.
- Media upload/list.
- Dioxus admin UI screens: login, register, home, Content-Type Builder,
  Content Manager, Media, Settings (i18n, API tokens, roles, users).

---

## License

[MIT](LICENSE)
