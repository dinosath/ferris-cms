# ferriscms — Project Overview

> A 1:1, offline-first clone of [Strapi](https://github.com/strapi/strapi) built entirely in Rust.
> Backend: **Axum** + **SeaORM 2.0**. Frontend/UI: **Dioxus** (native desktop + WASM for web).
> Public data access: **REST API** (Strapi-compatible shape).

This document is the entry point. Read it first, then follow the reading order at the bottom.

---

## 1. What we are building

Strapi is a **headless CMS**. Its core value is that non-developers can:

1. **Define content structures** ("content-types") through a visual **Content-Type Builder** — no code, no manual SQL.
2. **Create, edit, publish content** through a **Content Manager** that renders a form automatically from each content-type definition.
3. **Consume that content** through an automatically generated **REST API** (and GraphQL, which we defer to a later phase).
4. Manage **media**, **users/permissions (RBAC)**, **internationalization (i18n)**, and **draft/publish** workflows.

`ferriscms` reproduces this experience. The defining architectural challenge is that Strapi content-types are **dynamic**: the admin defines new tables/columns at runtime. Our system must store these definitions, apply schema changes to the database at runtime (via SeaORM 2.0's entity-first / `sync` workflow and dynamic DDL), and serve typed REST endpoints for them.

### 1.1 Non-negotiable product goals

- **Offline-first**: The desktop app (Dioxus native) runs fully offline against an embedded **SQLite** database. No server required.
- **Online**: The same core runs as an Axum server backed by **PostgreSQL** (or SQLite), serving the REST API and the web UI (Dioxus compiled to WASM).
- **One codebase, two runtimes**: The domain/core logic is shared; only the transport (embedded call vs HTTP) and database driver differ.
- **Sync**: An offline desktop instance can synchronize its content and schema with an online server (see [07-offline-sync.md](07-offline-sync.md)).

---

## 2. Technology stack (locked decisions)

| Layer | Technology | Notes |
|---|---|---|
| Language | Rust (edition 2021, MSRV 1.82+) | Entire stack. |
| Web server | [Axum](https://docs.rs/axum) 0.7+ | REST API + serves WASM UI + admin API. |
| ORM | [SeaORM 2.0](https://www.sea-ql.org/blog/2026-07-27-sea-orm-2.0/) | New dense entity format, entity-first `sync`, nested ActiveModel, strongly-typed columns. |
| Query builder | SeaQuery 1.0 | Used directly for **dynamic** (runtime-defined) tables. |
| Migrations | `sea-orm-migration` for fixed system tables; runtime DDL via SeaQuery for user content-types. |
| DB (online) | PostgreSQL 14+ | Default production driver. |
| DB (offline) | SQLite (bundled) | Embedded, zero-config. |
| UI framework | [Dioxus](https://dioxuslabs.com) | One codebase → native (desktop) + `wasm32` (web). RSX declarative UI. |
| Async runtime | Tokio | Server + background jobs. |
| Auth | JWT (admin) + API tokens (public API) | `jsonwebtoken`, `argon2` for password hashing. |
| Serialization | `serde` / `serde_json` | JSON everywhere; content-type schemas stored as JSON. |
| Validation | `validator` + custom rules engine | Field-level constraints from schema. |
| Testing | `cargo test`, `sqlx`-style fixtures, `insta` snapshots for API. |

> Rationale for Dioxus as the UI: Dioxus renders the same RSX component tree to a native desktop webview and to web (WASM + WebGL), giving us **one UI codebase** for both the offline desktop app and the online web admin, satisfying the "offline and online with a web frontend" requirement without React/JS.

---

## 3. Workspace layout (Cargo workspace)

```
ferriscms/
├─ Cargo.toml                  # workspace manifest
├─ docs/                       # <-- these planning docs
├─ crates/
│  ├─ core-domain/             # Pure domain types: ContentType, Field, Component, etc. No IO.
│  ├─ core-schema/             # Content-type schema model + JSON (de)serialization + validation.
│  ├─ db/                      # SeaORM 2.0 entities for SYSTEM tables + connection mgmt + migrations.
│  ├─ dynamic-store/           # Runtime DDL + CRUD for USER-defined content-types (SeaQuery).
│  ├─ services/                # Business logic: CTB service, content service, media, rbac, i18n, sync.
│  ├─ api-rest/                # Axum routers: /api (public) + /admin (management) + /content-type-builder.
│  ├─ api-types/               # Shared request/response DTOs (used by server AND Dioxus client).
│  ├─ client-core/             # Transport-abstract client the UI calls (HTTP impl + in-process impl).
│  ├─ ui/                      # Design system: tokens, icon catalog, widget/screen spec types (framework-agnostic).
│  ├─ app/                     # Dioxus UI app: base widgets + screens + routing, calls client-core.
│  ├─ server-bin/             # Binary: online Axum server.
│  └─ desktop-bin/            # Binary: offline Dioxus desktop app (embeds services + SQLite).
└─ web/                        # WASM build entry + static host page for the online web admin.
```

The crate boundaries are the backbone of the whole plan. Each downstream doc references these crates by name.

---

## 4. Reading order (build these docs into features in this order)

1. **[00-overview.md](00-overview.md)** — this file.
2. **[01-architecture.md](01-architecture.md)** — system architecture, crate responsibilities, request lifecycles, offline/online split.
3. **[02-data-model.md](02-data-model.md)** — SeaORM 2.0 system entities + how dynamic content-type tables are represented and generated.
4. **[03-content-type-builder-logic.md](03-content-type-builder-logic.md)** — field taxonomy, schema JSON format, DDL generation, validation rules. This is the heart of the clone.
5. **[04-rest-api.md](04-rest-api.md)** — every endpoint, query param, auth model, error shapes (Strapi-compatible).
6. **[05-ui-design-system.md](05-ui-design-system.md)** — exact Dioxus design tokens: colors, spacing, typography, iconography, base widgets. Pixel-level.
7. **[06-ui-screens.md](06-ui-screens.md)** — screen-by-screen layout specs so an agent that cannot see screenshots can build the UI verbatim.
8. **[07-offline-sync.md](07-offline-sync.md)** — offline embedded mode + online sync protocol.
9. **[08-roadmap.md](08-roadmap.md)** — phased milestones, acceptance criteria, and task breakdown.

---

## 5. Glossary (used consistently across all docs)

| Term | Meaning |
|---|---|
| **Content-Type (CT)** | A user-defined data structure. Two kinds: **Collection Type** (many entries) and **Single Type** (exactly one entry). |
| **Component** | A reusable named group of fields. Not independently addressable; embedded into CTs. Can be **repeatable** or **single**. |
| **Dynamic Zone (DZ)** | An ordered list where each element is any one of a whitelisted set of components. |
| **Field / Attribute** | A single typed slot inside a CT or component (Text, Number, Relation, etc.). |
| **Entry** | One record (row) of a Collection Type. |
| **Schema** | The JSON definition of a CT or component (its fields + settings). Stored in the `content_type_schemas` system table. |
| **UID** | Strapi's stable identifier for a CT, formatted `api::<singular>.<singular>` for collections, plus component UIDs `<category>.<name>`. |
| **Populate** | Request-time inclusion of related data (relations, components, media, DZ). |
| **Draft & Publish** | A per-CT workflow where entries have `draft` and `published` states. |
| **RBAC** | Role-Based Access Control for admin users. |
| **Document Service** | Strapi's internal data access layer. Our equivalent is the `services` crate. |

---

## 6. Scope for this planning phase

This planning phase (docs 00–08) delivers **design and architecture only**. No implementation code yet. Every downstream doc must be concrete enough that a low-context agent can implement it without additional design decisions. Where a decision is deferred, it is marked **`[LATER]`** with the target phase.
