# ferriscms — Master Design & Architecture

> A 1:1, offline-first clone of [Strapi](https://github.com/strapi/strapi) built entirely in **Rust**.
> Backend: **Axum** + **SeaORM 2.0**. UI: **Dioxus** (native desktop + WASM web). Data access: Strapi-compatible **REST API**.
> Runs **offline** (embedded SQLite) and **online** (PostgreSQL), with optional sync, code generation, and progressive deployment.

This is the consolidated design. Each part is self-contained; the **Phased Delivery Plan** (Part XI) sequences everything. Individual chapter docs (`00`–`10`) remain as detailed source; this document is the single merged reference.

## Table of contents
- **Part I — Overview, Stack & Glossary**
- **Part II — System Architecture**
- **Part III — Data Model (SeaORM 2.0)**
- **Part IV — Content-Type Builder Logic**
- **Part V — REST API**
- **Part VI — UI Design System (Dioxus)**
- **Part VII — UI Screens (pixel-level)**
- **Part VIII — Offline & Online Sync**
- **Part IX — Codegen, Sandboxing & Eject (decision record)**
- **Part X — Deployment Modes, Progressive Promotion & GitOps**
- **Part XI — Phased Delivery Plan (unified roadmap)**
- **Part XII — Glossary**

---
---

# Part I — Overview, Stack & Glossary

## 1. What we are building
Strapi is a **headless CMS**: non-developers define content structures visually (Content-Type Builder), create/edit/publish content (Content Manager), and consume it via an auto-generated REST API, with media, RBAC, i18n, and draft/publish.

`ferriscms` reproduces this. The defining challenge: content-types are **dynamic** (defined at runtime), so the system stores definitions, applies schema changes to the DB at runtime, and serves typed REST endpoints for them.

### Non-negotiable product goals
- **Offline-first**: desktop app runs fully offline on embedded **SQLite**, zero config.
- **Online**: same core runs as an Axum server on **PostgreSQL**, serving REST + the web UI (Dioxus→WASM).
- **One codebase, two runtimes**: shared domain/core; only transport (in-process vs HTTP) and DB driver differ.
- **Sync**: an offline instance can synchronize content + schema with an online server.

## 2. Technology stack (locked)
| Layer | Tech | Notes |
|---|---|---|
| Language | Rust (2021, MSRV 1.82+) | entire stack |
| Web server | Axum 0.7+ | REST + serves WASM UI + admin API |
| ORM | SeaORM 2.0 | dense entities, entity-first `sync`, nested ActiveModel, typed columns |
| Query builder | SeaQuery 1.0 | dynamic (runtime) tables |
| Migrations | `sea-orm-migration` (system) + runtime DDL (user CTs) | |
| DB online | PostgreSQL 14+ | default prod |
| DB offline | SQLite (bundled) | embedded |
| UI | Dioxus | native desktop + wasm32; RSX component model |
| Async | Tokio | server + jobs |
| Auth | JWT (admin) + API tokens (public) | `jsonwebtoken`, `argon2` |
| Serde | serde/serde_json | schemas stored as JSON |
| Validation | `validator` + custom rules | field constraints from schema |
| Codegen | Baker / MiniJinja | eject/export feature (Part IX/X) |
| Scripting | Rhai → wasmtime | custom hooks/policies (Part IX) |

Rationale for Dioxus: one UI codebase renders to native (desktop/offline) and web (WASM/online) using the same RSX component tree.

## 3. Workspace layout (Cargo workspace)
```
ferriscms/
├─ Cargo.toml
├─ docs/
├─ crates/
│  ├─ core-domain/     # pure domain types, no IO
│  ├─ core-schema/     # schema model + JSON + validation + diff
│  ├─ db/              # SeaORM 2.0 system entities + migrations
│  ├─ dynamic-store/   # runtime DDL + CRUD for user CTs (SeaQuery)
│  ├─ services/        # business logic (CTB, content, media, rbac, i18n, sync)
│  ├─ api-types/       # shared DTOs (server + client)
│  ├─ api-rest/        # Axum routers (/api, /admin, /content-type-builder)
│  ├─ client-core/     # transport-abstract SDK (HTTP + in-process)
│  ├─ ui/              # Dioxus app (design system + screens)
│  ├─ server-bin/      # online Axum server
│  └─ desktop-bin/     # offline Dioxus desktop app (embeds services + SQLite)
└─ web/                # WASM host page for the online web admin
```
Glossary is in **Part XII**. Deferred items are marked **`[LATER]`**.

---
---

# Part II — System Architecture

## 1. Two runtimes, shared core
Two binaries from one set of library crates. The **UI always talks to `client-core`**, never a DB or Axum directly. `client-core` has two `ApiTransport` impls:
- `InProcessTransport` — desktop; calls `services` directly (no server).
- `HttpTransport` — web/WASM; calls Axum REST.

**The UI cannot tell whether it is online or offline.** Same screens, DTOs, behavior.

```
                shared crates: core-domain, core-schema, db,
                dynamic-store, services, api-types
                 ▲                               ▲
  in-process     │                               │ HTTP
 ┌───────────────┴────────┐          ┌───────────┴────────────┐
 │ desktop-bin (OFFLINE)  │          │ server-bin (ONLINE)     │
 │ Dioxus → client-core   │          │ Axum(api-rest)→services │
 │ services + SQLite       │          │ Postgres; serves WASM   │
 └────────────────────────┘          └─────────────┬───────────┘
                                                    │ HTTP
                                       ┌────────────┴──────────┐
                                       │ Web admin (Dioxus→WASM) │
                                       │ client-core (Http)      │
                                       └─────────────────────────┘
```

## 2. Crate responsibilities
- **core-domain** — pure types: `ContentTypeKind`, `FieldType`, `RelationKind`, `Uid`, `ApiId`. serde only.
- **core-schema** — `Schema`/`Attribute` model, JSON (de)serialize, structural validation, `SchemaDiff`.
- **db** — SeaORM 2.0 dense entities for **system** tables; migrations; `schema_registry.sync(db)` on boot.
- **dynamic-store** — SeaQuery DDL + DML for **user** CTs; returns `serde_json::Value` rows; join/link tables.
- **services** — all business logic; the only crate transports call; `AppContext` (db, current user, config).
- **api-types** — serde DTOs + Strapi query parser; one wire contract.
- **api-rest** — Axum routers + middleware (auth, RBAC, error mapping, CORS).
- **client-core** — `ApiTransport` trait + two impls + client-side cache for CTB unsaved model.
- **ui** — Dioxus design system + components + screens; calls `client-core` only.
- **server-bin / desktop-bin** — thin wiring binaries.

## 3. Dependency graph (acyclic)
```
core-domain → core-schema, api-types, db → dynamic-store → services → api-rest → server-bin
                                                     └→ client-core → ui → desktop-bin
```
`ui` never imports `services`/`db`/`dynamic-store` directly — only `client-core` + `api-types`.

## 4. Request lifecycle — reading content (online)
`GET /api/articles?populate=author&filters[title][$contains]=rust&pagination[page]=1`
1. Axum matches route; auth middleware validates token/permission.
2. Parse Strapi query params → typed `QueryParams`.
3. `services::content::find_many(ctx, uid, query)`.
4. Load CT schema (cached) → `dynamic-store` builds SeaQuery SELECT with WHERE/ORDER/LIMIT + populate joins.
5. Execute → `Vec<Value>` shaped to schema (relations/components nested).
6. Apply read permissions + draft/publish + locale → `{ data, meta }`.
7. Serialize JSON. Offline path is identical except step 1 is an in-process call.

## 5. Request lifecycle — creating a content-type (the hard one)
Admin adds fields to a new `Article` CT and clicks **Save**:
1. UI accumulates **unsaved edits** locally (staging model — nothing hits DB until Save).
2. Save → `POST /content-type-builder/schema` with the full desired schema set.
3. `services::content_type_builder::apply`:
   a. validate each schema (`core-schema`);
   b. load current registry; compute `SchemaDiff` per CT;
   c. **open one transaction**;
   d. `dynamic-store::apply_schema` → CREATE/ALTER/join-table DDL;
   e. upsert schema JSON into `content_type_schemas`;
   f. commit (rollback on any error).
4. Rebuild schema cache + regenerate router → `/api/articles` serves immediately (hot reload, no restart).
5. Return schemas; UI clears badges.

System tables use SeaORM 2.0 entity-first `sync`; user tables use runtime DDL — both via the same connection/transaction.

## 6. Offline/online configuration
A single `Config` selects mode: `database.driver` (sqlite|postgres), `transport` (in-process|HTTP), `ui` (native|WASM), `media.storage`, `sync.enabled`. Core services branch only on the DB driver + media backend, injected via traits.

## 7. Concurrency
- Server: Tokio + pool; schema cache behind `arc-swap` (lock-free reads), rebuilt on Save.
- Desktop: Dioxus desktop runtime + Tokio for `services`; results propagated to the UI via signals/coroutines. Never block the render thread.

## 8. Error model
`ServiceError { Validation(Vec<FieldError>), NotFound, Conflict, Forbidden, Unauthorized, Db, Internal }` → mapped by `api-rest` to Strapi-compatible bodies (see Part V §2).

## 9. Deferred `[LATER]`
GraphQL, plugins/marketplace, Strapi AI, custom fields SDK, webhooks, audit logs, review workflows, S3 media, advanced image pipeline.

---
---

# Part III — Data Model (SeaORM 2.0)

Conventions: `id BIGINT` PKs; `created_at`/`updated_at` UTC; `published_at NULL` when Draft&Publish; every content entry carries a stable `document_id TEXT`; `(document_id, locale, publication_state)` identifies a variant.

## 1. SeaORM 2.0 features used
- **Dense entity format** — relations as typed fields on `Model` (`HasMany`, `BelongsTo<Entity>` / `BelongsTo<Option<Entity>>` encodes FK nullability).
- **Entity-first `sync`** — `db.get_schema_registry("...::entity::*").sync(db)` aligns **system** tables in FK order at boot.
- **Nested ActiveModel** — build a parent+children graph and `.save(db)` in one transaction (seeding, single-component writes).
- **Strongly-typed columns** — `entity::COLUMN.email.contains(...)` compile-checked filters.
- **RBAC helper** — evaluated where convenient; our tables (§4) are authoritative.
Use dense format everywhere in `db`.

## 2. Content-Type registry (definitions)
**`content_type_schemas`** — source of truth: `id, uid UNIQUE, kind (collectionType|singleType|component), category NULL, display_name, singular_api_id NULL, plural_api_id NULL, schema_json (canonical, Part IV §3), draft_and_publish, i18n_localized, is_system, version, created_at, updated_at`. `schema_json` is authoritative; other columns are denormalized copies rewritten on each save.

**`content_type_table_map`** — logical↔physical mapping for safe renames: `id, schema_uid, physical_table, logical_attr NULL, physical_column NULL, join_table NULL`.

## 3. User-defined content tables (dynamic — `dynamic-store`)
Collection `Article` → table `ct_articles`: `id`, `document_id` (indexed), `locale NULL`, `publication_state (draft|published)`, one column per scalar attribute, `created_at/updated_at/published_at NULL/created_by_id/updated_by_id`, unique `(document_id, locale, publication_state)`. Single types share the shape; service enforces ≤1 `document_id`.

- **Relations**: 1-1/many-to-one/one-way → FK `<field>_id` on owner; one-to-many → FK on the other table; m2m/many-way → join table with order columns.
- **Components**: a shared **component link table** per host `<host>_components (id, entry_id, component_uid, component_id, field, order)` → each component type has its own `cmp_<category>_<name>` table. Single vs repeatable = one vs many link rows.
- **Dynamic zones**: same link table; `component_uid` distinguishes elements; `order` gives zone order.
This link-table strategy means adding components/DZ never alters the parent table.

## 4. Admin, Auth, RBAC
- **admin_user** `id, email UNIQUE, first_name/last_name NULL, username NULL, password_hash (argon2id), is_active, blocked, prefered_locale NULL, timestamps`.
- **admin_role** `id, name UNIQUE, code UNIQUE, description`. Seed: Super Admin, Editor, Author.
- **admin_permission** `id, role_id, action, subject (CT uid|null), properties_json, conditions_json`.
- **admin_user_role** join.
- **api_token** `id, name, description, type (read-only|full-access|custom), access_key_hash, last_used_at, expires_at, lifespan`.
- **api_token_permission** `token_id, action`.

## 5. Media Library
- **upload_file** `id, name, alternative_text, caption, width, height, formats_json (thumbnail/small/medium/large), hash, ext, mime, size, url, preview_url, provider, folder_id NULL, timestamps`.
- **upload_folder** `id, name, path_id, path (materialized), parent_id NULL` (nested folders).
- Media↔entry via `<host>_<attr>_files_links (entry_id, file_id, order)`.

## 6. i18n
- **i18n_locale** `id, code UNIQUE, name, is_default`. Seed `en`. Localized entries differ by `locale`, tied by `document_id`.

## 7. Config / misc
- **core_store** `id, key UNIQUE, value_json, type, environment, tag` (view config, plugin settings).
- **schema_change_log** `id, schema_uid, from_version, to_version, diff_json, applied_at, applied_by` (drives sync/audit).
- **webhook**/**audit_log** `[LATER]`. **seaql_migrations** for system tables.

## 8. Content-Manager layout config
Per-CT UI config (in `core_store` or a table): list columns/order/sortable, default sort, page size, `mainField`, and the **edit view** 12-column grid layout consumed directly by the UI (Part VII §6.3):
```json
{ "settings": { "defaultSortBy": "createdAt", "defaultSortOrder": "DESC", "pageSize": 10, "mainField": "title" },
  "layouts": { "list": ["title","author","publishedAt"],
               "edit": [[{"name":"title","size":6},{"name":"slug","size":6}],[{"name":"body","size":12}]] } }
```

## 9. Seed data (first boot)
Roles + permissions; default locale `en`; built-in users-permissions `user`/`role` CTs (`is_system`); default view configs; first-run Super Admin via registration (no seeded password).

## 10. Indexing & constraints
Unique on every `uid`, `admin_user.email`, `i18n_locale.code`, `(document_id, locale, publication_state)`; FK indexes on all relation/join columns; UID fields unique (per-locale when i18n); enumeration via CHECK or app validation (SQLite).

> **Sync columns** (`document_id`, `sync_version`, `origin_node_id`, `deleted_at`) are added to syncable rows **from day one** even though the sync engine ships later (Part VIII).

---
---

# Part IV — Content-Type Builder Logic (core of the clone)

Implemented in `core-schema` (model + validation) + `services::content_type_builder` (orchestration) + `dynamic-store` (DDL).

## 1. Kinds
Collection Type (`ct_<plural>`), Single Type (`ct_<singular>`, ≤1 row per locale/state), Component (`cmp_<category>_<name>` + link tables).

## 2. Field taxonomy (1:1 with Strapi)
Shared advanced settings: `required`, `unique`, `private`, `default`, `configurable`; `localized` toggle when CT has i18n.
- **Text** — Short(≤255)/Long; VARCHAR(255)/TEXT; length/regex.
- **Rich Text (Markdown)** — TEXT (markdown source).
- **Rich Text (Blocks)** — JSON block array (paragraph/heading/list/quote/code/image/link + inline marks).
- **Number** — integer/biginteger/decimal/float → INTEGER/BIGINT/DECIMAL/DOUBLE.
- **Date** — date/datetime/time → DATE/TIMESTAMPTZ/TIME.
- **Boolean** — BOOLEAN.
- **Email** — VARCHAR(255) + email regex.
- **Password** — VARCHAR(255) argon2 hash; implicitly private; never returned.
- **Enumeration** — VARCHAR + CHECK; each value must start with a letter.
- **JSON** — JSONB/TEXT.
- **UID** — VARCHAR(255) UNIQUE; auto-slug from `targetField`; per-locale unique.
- **Media** — single/multiple; `*_files_links`; allowedTypes.
- **Relation** — 6 kinds (§6).
- **Component** — single/repeatable; category; link table; min/max count.
- **Dynamic Zone** — allowed components; heterogeneous link table; DZ field-collision rule.
- **Custom fields** `[LATER]`.

## 3. Canonical schema JSON (wire + storage contract)
Collection example:
```json
{ "uid": "api::article.article", "kind": "collectionType", "collectionName": "ct_articles",
  "info": { "singularName": "article", "pluralName": "articles", "displayName": "Article" },
  "options": { "draftAndPublish": true }, "pluginOptions": { "i18n": { "localized": true } },
  "attributes": {
    "title":  { "type": "string", "required": true, "maxLength": 255 },
    "slug":   { "type": "uid", "targetField": "title", "required": true },
    "body":   { "type": "blocks" },
    "cover":  { "type": "media", "multiple": false, "allowedTypes": ["images"] },
    "author": { "type": "relation", "relation": "manyToOne", "target": "api::author.author", "inversedBy": "articles" },
    "tags":   { "type": "relation", "relation": "manyToMany", "target": "api::tag.tag", "inversedBy": "articles" },
    "seo":    { "type": "component", "component": "shared.seo", "repeatable": false },
    "blocks": { "type": "dynamiczone", "components": ["shared.hero","shared.cta"] } } }
```
`type` discriminators: `string, text, richtext, blocks, integer/biginteger/decimal/float, date/datetime/time, boolean, email, password, enumeration, json, uid, media, relation, component, dynamiczone`. Deserialized into `Schema` with an **ordered** `IndexMap<String, Attribute>` (order = UI display order).

## 4. Attribute → SQL mapping
As in the taxonomy; nullable unless `required`; unique → unique index (per-locale when i18n). SQLite fallbacks: JSON/dates as TEXT, booleans as INTEGER, enums app-validated.

## 5. Physical naming (deterministic)
`ct_<snake plural>`; `cmp_<snake category>_<snake name>`; column `snake(attr)` (reserved → trailing `_`); FK `<snake attr>_id`; m2m `ct_<a_plural>_<attr>_links` with `<a_singular>_id/<b_singular>_id` + order; media `<host>_<attr>_files_links`; components `<host>_components`. All persisted in `content_type_table_map`.

## 6. Relations (6 kinds)
| UI | value | storage |
|---|---|---|
| One way | oneWay | FK on A, B unaware |
| One-to-one | oneToOne | FK on A unique, inversedBy on B |
| One-to-many | oneToMany | FK on B (mappedBy) |
| Many-to-one | manyToOne | FK on A |
| Many-to-many | manyToMany | join table + order |
| Many way | manyWay | join table, one-directional |
Fields: `target`, `relation`, `inversedBy`/`mappedBy`. Self-referential allowed (page trees).

## 7. Staging/edit model (unsaved changes)
Client holds a working copy + change list; each edit marks CT/field `New`/`Modified`/`Deleted`; Undo/Redo/Discard operate on the stack; **Save** sends the entire desired state (declarative); server diffs + applies in one transaction; whole batch rejected on any validation error.

## 8. Schema diffing → DDL
`core-schema::diff(current, desired)` → per CT: added/modified/removed attributes + table created/dropped. `dynamic-store::apply_schema` emits, in order: CREATE TABLE (new) → ADD COLUMN → create join/link tables → compatible ALTERs (incompatible = drop+add semantics, data retained but detached, logged) → removals (**default: unmap, don't hard-drop** to prevent data loss; hard delete `[LATER]`). One transaction, rollback on error.

## 9. Validation rules
API IDs (lowercase, unique, not reserved, singular+plural required); attribute names (identifier, unique, not reserved); enum values (non-empty, unique, start with letter); relations (target exists + is collection, paired-field consistency); components/DZ exist; **DZ field-collision** (shared field names across DZ components must have identical type/enum values); UID targetField exists; single-type constraints; i18n/draft toggles add required columns. Returns `Vec<FieldError { path, code, message }>`.

## 10. CTB service API
`list()`, `get(uid)`, `apply(desired: Vec<Schema>)` (batch Save), `delete(uid)` (as a batch removal), `list_components(category?)`. Exposed at `/content-type-builder` (Part V §5).

---
---

# Part V — REST API

`api-rest` (Axum) → `services`; DTOs in `api-types`. Envelopes + query syntax are **Strapi v5-compatible**; same DTOs used by `client-core` offline.

## 1. Route groups
`/api/**` (public content, API token/public role), `/admin/**` (management, admin JWT + RBAC), `/content-type-builder/**` (schema CRUD, admin JWT, dev-only), `/uploads/**` (static media).

## 2. Envelopes
List: `{ "data": [...], "meta": { "pagination": { page, pageSize, pageCount, total } } }`. Single: flat attributes + `documentId`, `id`, timestamps, `publishedAt`, `locale`. Error: `{ "data": null, "error": { status, name, message, details: { errors: [{ path, message, name }] } } }`.

## 3. Public content API (per CT)
Collection `articles`: GET `/api/articles`, GET `/api/articles/:documentId`, POST, PUT `/:documentId`, DELETE `/:documentId`. Single `homepage`: GET/PUT/DELETE `/api/homepage`.

**Query params (Strapi):** `fields`, `populate` (`*`, nested, `populate[x][fields][0]`), `filters` (`$eq,$ne,$lt,$lte,$gt,$gte,$in,$notIn,$contains,$notContains,$containsi,$startsWith,$endsWith,$null,$notNull,$between,$and,$or,$not`), `sort` (`field:desc`), `pagination` (page/pageSize or start/limit, withCount), `locale`, `status` (draft|published). Parser lives in `api-types::query` with a full fixture suite.

**Create/update body:** `{ "data": { ... } }`; relations accept id/documentId/array/connect-disconnect-set; components nested; DZ = `[{ "__component": uid, ... }]`; media = id(s). Validated against schema (Part IV §9).

## 4. Admin content-manager API
`/admin/content-manager/collection-types/:uid` (list/create), `/:documentId` (get/update/delete), `.../actions/publish|unpublish|discard`, `.../actions/bulkDelete|bulkPublish`; single-types analogues; `/content-types` (nav), `/content-types/:uid/configuration` (view config get/put), `/relations/...` (relation lookup).

## 5. Content-Type Builder API
`/content-type-builder/content-types` (list/create-batch/get/update/delete), `/components[/:uid]`, `/reserved-names`, and preferred `POST /content-type-builder/schema` (batch declarative apply → router rebuild).

## 6. Auth & users
Admin: `POST /admin/login`, `/admin/register-admin`, `GET/PUT /admin/users/me`, `/admin/users`, `/admin/roles` + `/roles/:id/permissions`, `/admin/permissions`. API tokens: `/admin/api-tokens` (CRUD, regenerate, key shown once). Public users-permissions `[PHASE 4]`: `/api/auth/local`, `/api/auth/local/register`, `/api/users/me`.

## 7. Media API
`POST /admin/upload` (multipart → thumbnails via `image`), `/admin/upload/files` (list/get/put/delete), `/admin/upload/folders` (nested). Served at `/uploads/:hash.:ext`.

## 8. i18n API
`/admin/i18n/locales` CRUD; content endpoints accept `?locale=` + link variants by `document_id`.

## 9. Middleware (order)
CORS → request-id/tracing → body limit/multipart → auth extractor → RBAC guard → handler → error mapper.

## 10. Auth model
Admin JWT (HS256, `sub=admin_user.id`), API tokens (sha256 at rest; read-only/full/custom), public role `[PHASE 4]`. OpenAPI generation `[PHASE 5]` via `utoipa`.

---
---

# Part VI — UI Design System (Dioxus)

> Numeric so a screenshot-blind agent can build it. Build tokens + base components first; screens (Part VII) use these by name.

## 1. Dioxus primer
RSX component model; elements map to native widgets on desktop (via `dioxus-desktop`) and DOM on web (via `dioxus-web`/WASM); state via `use_signal`/`use_memo`; async via `use_coroutine`. Prefix reusable components `S` (`SButtonPrimary`, `SCard`). Tokens in `crates/ui/src/design/tokens.rs`.

## 1a. Strapi UI Philosophy
**Low-code admin surface for power users and developers.** The admin UI is **task-oriented, not data-driven**: the interface prioritizes workflow (Content-Type Builder → Content Manager → Publish) over exposing raw data models. Visual hierarchy is strict — primary actions are prominent, secondary actions are discoverable, destructive actions are gated by confirmation. The **Content-Type Builder** is the hero screen: it must be fast, unambiguous, and support undo/redo/discard for zero-fear iteration. The **Content Manager** is consumption-driven: list views are scannable, inline actions are minimal, bulk actions are powerful. **Offline-first sensibility**: every interaction should feel instant (no spinners for 100ms operations). Errors are contextual, never silent. Status is always visible (draft/published/syncing/conflicts).

## 1b. Design Principles
- **Clarity over elegance**: always show what's happening; avoid surprises.
- **Consistency**: one pattern for dropdowns, tables, modals across the entire app.
- **Progressive disclosure**: beginners see simple CTB interface; advanced users access validation rules, relational constraints, i18n toggles via tabs/accordions.
- **Offline parity**: desktop and web UIs are pixel-identical (same Dioxus code).
- **Data safety**: draft mode, undo, version history, sync conflict resolution — no accidental data loss.

## 2. Colour System (exact hex)
## 2. Colour System (exact hex)
All colours follow a 100–900 scale (light → dark), except semantic colours (status-specific). Use colour tokens, never hex literals in code.

**Primary (Violet — brand & interactive):** 
- 100 `#F0F0FF`, 200 `#D9D8FF`, 300 `#C9C7FF`, 400 `#9B96FF`, 500 `#7B79FF`, **600 `#4945FF` (button states, focus rings, active elements)**, 700 `#271FE0`, 800 `#1F15B0`, 900 `#15088F`.

**Neutrals (backgrounds, text, borders):**
- 0 `#FFFFFF` (pure white, card backgrounds).
- 50 `#FAFAF9` (very light grey, subtle hover).
- 100 `#F6F6F9` (app background).
- 150 `#EAEAEF` (subtle dividers).
- 200 `#DCDCE4` (input borders, disabled states).
- 300 `#C0C0CF` (secondary placeholder text).
- 400 `#A5A5BA` (placeholder text inside inputs).
- 500 `#8E8EA9` (secondary labels).
- 600 `#666687` (secondary text).
- 700 `#4A4A6A` (body text, copy).
- 800 `#32324D` (headings, strong emphasis).
- 900 `#212134` (highest contrast, dark mode base).

**Semantic — Success (green, positive actions/states):**
- 100 `#EAFBE7`, 500 `#31A856`, 600 `#328048`, 700 `#2F6846`.

**Semantic — Warning (orange, caution/draft/modified):**
- 100 `#FDF4DC`, 500 `#D19400`, 600 `#BE5D01`, 700 `#9B4C00`.

**Semantic — Danger (red, destructive/error):**
- 100 `#FCECEA`, 500 `#EE5E52`, 600 `#D02B20`, 700 `#B72B1A`.

**Semantic — Alternative (purple, advanced/experimental):**
- 100 `#F6ECFC`, 600 `#9736E8`.

**Semantic — Secondary (blue, information/draft-state highlight):**
- 100 `#EAF5FF`, 600 `#0C75AF`, 700 `#0A5F8A`.

**Status mapping (field badges in CTB list):**
- **New (N)** = Secondary 600 (blue) on Secondary 100 bg.
- **Modified (M)** = Warning 600 (orange) on Warning 100 bg.
- **Deleted (D)** = Danger 600 (red) on Danger 100 bg.
- **Published** = Success 600 (green) on Success 100 bg.

## 3. Typography (Inter)
ALPHA 32/600, BETA 24/600 (page title), DELTA 18/600, EPSILON 16/600, BODY 14/400, BODY_BOLD 14/600, LABEL 12/600, PI 11/400. Bundle weights 400/500/600/700.

## 4. Spacing (px)
`SP_1..SP_10` = 4,8,12,16,20,24,32,40,48,56. Card padding 24; input padding 8×16; form row gap 20; page padding 32.

## 5. Radii/borders/shadows
`RADIUS_SM/MD` = 4, `RADIUS_PILL` = 999. Border 1px `NEUTRAL_200`; focus `PRIMARY_600` + 2px `PRIMARY_200` glow. Card: 1px `NEUTRAL_150` + very light shadow.

## 6. Base widgets (with states)
`SButtonPrimary` (36h, bg 600, hover 700, disabled 150/400), `SButtonSecondary` (outline), `SButtonDanger`, `SButtonGhost` (32×32 icon), `STextField` (40h, label+input+helper, focus/error), `STextArea`, `SDropdown`, `SCheckbox` (20×20), `SToggle` (40×24), `SBadge`/pill + N/M/D square, `SCard`, `SModal` (scrim + 640 dialog + header/body/footer), `STable` (header 40 `NEUTRAL_100`, rows 52, virtualised list >50), `SNavItem` (40h, active bg `PRIMARY_100` text 600), `SToast`, `STooltip`, `SSearchInput`, `SEmptyState`, `STab`, `SBreadcrumb`.

## 7. Icons (16/24 line)
`plus, pencil, trash, drag_handle, chevron_down/right/left, search, close, check, cog, grid, stack, image, users, shield, globe, key, link, puzzle, layers, text, hash, calendar, toggle, braces, envelope, lock, list, tag, file, external_link, filter, sort, more_vertical, eye/eye_off, arrow_left, refresh, warning_triangle, info_circle, check_circle, x_circle`. Field-type icons map to Part IV §2 and reuse in the field picker.

## 8. Layout grid & shell
Shell: `[Sidebar 240px][Main Fill]`; Main = top bar (56) + scroll content (padding 32). Some screens add a secondary nav (240). Forms: 12-col grid (`size 6` = 50%), column/row gap 20, max form width 900 centered.

## 9. Interaction standards
Hover/press 120ms ease-out; modal enter fade+scale 0.98→1.0; toast slide-up 180ms; drag-reorder with lift + `PRIMARY_600` insertion line; Tab order top→bottom, Enter submits, Esc closes, `/` focuses search.

## 10. Responsive
Min 1024×640; sidebar stays 240; no mobile in phases 1–5 `[LATER]`.

## 11. Deliverables
`design/tokens.rs`, `design/widgets/*`, `design/icons.rs`, `design/shell.rs`. Every screen built only from these.

## 12. Component Schema (composition & hierarchy)
All components follow a strict hierarchy: **Primitives** → **Base** → **Composite** → **Layouts** → **Screens**.

**Primitives** (unstyled Dioxus elements): `div`, `button`, `input`, `label`, `textarea`, `select`, `svg`, `img`, etc. Never used directly in views; wrapped by Base components.

**Base components** (styled, single responsibility):
- **Typography**: `SLabel`, `SHeading`, `SBody`, `SCaption` (text only, no borders/bg).
- **Input**: `STextField`, `STextArea`, `SDropdown`, `SCheckbox`, `SToggle`, `SDatePicker`, `SSearchInput` (single control + optional label/helper).
- **Button**: `SButtonPrimary`, `SButtonSecondary`, `SButtonDanger`, `SButtonGhost`, `SButtonIcon` (action triggers).
- **Feedback**: `SBadge`, `SToast`, `STooltip`, `SSkeleton`, `SSpinner`, `SEmptyState`, `SErrorBoundary`.
- **Container**: `SCard`, `SPanel`, `SDivider`, `SSpace` (structural grouping, no content logic).

**Composite components** (combination of Base + logic):
- **SModal** = `SCard` + header/body/footer + backdrop + close handler.
- **STable** = header row + virtualised rows (via `SRow`) + pagination.
- **SNavItem** = `SLabel` + icon + active state + hover + selected styling.
- **SForm** = grid layout + `STextField` + validation + submit handler.
- **SConfirmDialog** = `SModal` + warning icon + message + Cancel/Confirm buttons.
- **SMediaPicker** = modal with grid of `SMediaCard` items + upload + selection.

**Layouts** (page structure):
- **SShell** = `SSidebar` (240px fixed) + `SMainContent` (Fill flex).
- **SMainContent** = `STopBar` (56px fixed) + scroll region (padding 32).
- **SFormLayout** = 12-column grid, row gap 20, centered max-width 900.
- **SListLayout** = toolbar + table + pagination.

**Screens** (full pages):
- **Login** = centered `SCard` (552px).
- **ContentTypeBuilder** = `SShell` + three-column layout (nav/editor/inspector).
- **ContentManager** = `SShell` + list or edit view.
- **MediaLibrary** = `SShell` + breadcrumb + folder/asset grids.

**Reusable patterns:**
- **Form row** = label + input + helper text; always 56px height (flex row).
- **List item** = icon (24×24) + label + secondary text + actions; always 52px height.
- **Modal** = 512/640/720px width, centered, scrim backdrop, fade-in 120ms.
- **Empty state** = centered icon (48×48) + headline (DELTA) + description (BODY) + CTA button.
- **Validation error** = field gets RED border (2px PRIMARY_600); helper text shows error message in red.

**State conventions:**
- `:hover` — opacity 90% or bg NEUTRAL_100.
- `:active/:focus` — ring 2px NEUTRAL_200 (unfocused), ring 2px PRIMARY_200 + bg PRIMARY_100 (focused).
- `:disabled` — opacity 50%, no pointer events.
- `:loading` — show `SSpinner` replacing content or inline.
- `:error` — field border RED, helper text RED, optional error icon.

---
---

# Part VII — UI Screens (pixel-level)

Shell `[Sidebar 240][Main Fill]` on all authenticated screens. Each screen: route, layout tree, regions, `client-core` data source, interactions.

## 1. Login
Centered `SCard` 552px, padding 48: 40px brand mark, "Welcome!" BETA, subtitle, Email field, Password field (+eye toggle), "Remember me" checkbox, `SButtonPrimary` "Login" Fill, danger banner on error. `auth_login` → `POST /admin/login`; store JWT; route Home. Inline required + email validation.

## 2. Register (first run)
Same card; First name*, Last name, Email*, Password* (live rules), Confirm*; "Let's start". `POST /admin/register-admin` → auto-login.

## 3. Global sidebar (240)
Header (brand + "ferriscms"), search row, primary nav (`stack` Content Manager, `grid` Content-Type Builder, `image` Media Library), divider, GENERAL section (`cog` Settings), spacer, user footer (avatar initials + name + kebab → Profile/Logout). Exactly one active `SNavItem`.

## 4. Home
Top bar "Home"; welcome card + quick-link cards to CTB/CM/Media/Docs.

## 5. Content-Type Builder (highest fidelity)
Layout `[global 240][CTB nav 240][editor Fill]`.
- **CTB nav**: title + search; sections COLLECTION TYPES / SINGLE TYPES / COMPONENTS(by category, collapsible); each CT `SNavItem` + N/M/D badge; "+ Create new …" text buttons.
- **Editor top bar (64)**: CT name BETA + subtitle + `pencil` (settings modal); right = kebab (Undo/Redo/Discard, disabled when clean) + green **Save** (`SUCCESS_600`, disabled when clean, spinner while applying).
- **Editor body**: `SCard` with field-list table; each row (56h): `drag_handle`, type icon in 32px square, field name BODY_BOLD, type descriptor PI, spacer, N/M/D badge, hover actions (`pencil`/`trash`). Component rows expand to preview; DZ shows component chips. Empty state → "Add your first field". Bottom dashed "+ Add another field".
- **Data**: `ctb_list()` → `CtbStore` working copy; Save → `ctb_apply` → `POST /content-type-builder/schema`.
- **Create CT modal (640)**: tabs Basic (Display name → live singular/plural API IDs) / Advanced (Draft&Publish toggle default ON, i18n default OFF); Cancel/Continue → opens field picker; CT appears with **N** badge. Component create modal adds icon picker + category.
- **Edit settings modal**: prefilled Basic/Advanced + Delete (marks D); Cancel/Finish.
- **Field picker modal (720)**: tabs Default/Custom; 2-col grid of type cards (icon square + name + description) in order Text, Rich text (Blocks), Number, Date, Boolean, Relation, Email, Password, Enumeration, Media, JSON, Component, Dynamic Zone, Rich text (Markdown), UID — with the verbatim descriptions. Click → field config modal.
- **Field config modal (640)**: tabs Basic (Name + type-specific inputs) / Advanced (required/unique/private/default/min-max/regex + i18n toggle); footer Cancel / "+ Add another field" / Finish. New field appears with **N**.
- **Relation builder** (inside config): left grey box (current CT + field name), middle 6 relation icons (selected highlighted, tooltips), right grey box (target collection dropdown + inverse field name), live sentence.
- **Unsaved model**: change stack + dirty badges; Undo/Redo/Discard; navigation guard; Save clears badges + success toast; validation errors inline + danger toast.

## 6. Content Manager
Layout `[global 240][CM nav 240][view Fill]`. Nav lists collection + single types.
- **List view**: top bar (name + count + "+ Create new entry"); toolbar (search + Filters + "Configure the view"); `STable` with checkbox col, config-driven columns + sort arrows, State badge (Published green/Draft blue), row actions; bulk-action bar on selection (Publish/Delete); footer pagination (page size dropdown + controls); empty state. `cm_list`.
- **Edit view**: top bar (back + title + state controls: Save draft / Publish / kebab Unpublish-Discard, or single Save); body two columns: main form (12-col grid from edit layout) with per-type widgets (short=STextField, long=STextArea, Blocks editor, Markdown+preview, Number, Boolean=SToggle, Date picker, Enum=SDropdown, Email/Password/UID with regenerate+lock, JSON mono, Media picker card, Relation chips+search, Component sub-cards single/repeatable+drag, DZ ordered blocks + add-component popover) + right rail cards (Information, Internationalization/locale switch, draft note). `cm_get`/`cm_update`/`cm_publish`.
- **Single type**: opens directly into edit view (create if none).
- **Configure the view modal (720)**: Settings (entries/page, default sort attr/order) + View (field columns checklist + drag order + sortable/searchable). `PUT .../configuration`.

## 7. Media Library
Top bar (title + count + "+ Add new folder"/"+ Add new assets"); toolbar (search/filter/sort/grid-list); body: breadcrumb + folder grid + asset grid (~180px cards, thumbnail + name + ext/size, hover checkbox/edit/delete). Upload modal (drag-drop + from-URL + progress). Asset detail modal (preview + alt/caption/name + replace/copy-url/delete). Media picker modal for entry fields. `/admin/upload/*`.

## 8. Settings
Layout `[global 240][settings nav 240][pane Fill]`. Nav: GLOBAL (i18n, Media, API Tokens) / ADMIN PANEL (Roles, Users).
- **Roles**: list + editor (Name/Description + permission matrix accordion per plugin/CT, field-level sub-panel). `PUT /admin/roles/:id/permissions`.
- **Users**: table + invite modal + edit modal.
- **API Tokens**: table + create (Name/Description/type/duration; Custom → matrix); raw token shown once with warning.
- **i18n**: locale table + add modal (locale + display name + default toggle).

## 9. Common
Confirm dialog (512, warning + Cancel/Confirm-Delete); toasts (success/danger/info); loaders (page spinner / button spinner / table skeleton); empty states; error boundary with Retry; unsaved-changes guard.

## 10. UI state
`AppState { auth, schemas, route } + per-screen stores (CtbStore, ContentManagerStore, MediaStore)`. All IO via `client-core` async → Dioxus signals/coroutines. Optimistic only in CTB working copy.

## 11. Build order
Tokens+widgets → shell+sidebar+routing → login/register+auth → **CTB** → **Content Manager** → Media → Settings → polish.

---
---

# Part VIII — Offline & Online Sync

Desktop works fully offline on SQLite; optional sync with an online Axum/Postgres server covers **schema** and **content** (+ media).

## 1. Modes
Offline standalone (SQLite, in-process, no sync); Offline+sync (periodic HTTP to server); Online (Postgres, HTTP, is the sync target). Offline is zero-config; sync is opt-in (`sync.remote_url`, `sync.token`).

## 2. What syncs
Schema (`content_type_schemas`, `schema_change_log`, `content_type_table_map`), content (all `ct_*`/component/link tables), media (`upload_file`/`upload_folder` + blobs), locales + view configs. Users/roles/tokens are environment-local (not synced by default).

## 3. Sync model additions
Every syncable row: `document_id`, `updated_at`, `sync_version` (Lamport-ish), `origin_node_id`, `deleted_at` (tombstone). **`sync_state`** `(node_id, remote_url, last_pulled_version, last_pushed_version, last_synced_at)`. **`sync_oplog`** append-only `(entity, document_id, op, sync_version, payload_json, created_at, pushed)` — written in the same transaction as every mutation.

## 4. Protocol (HTTP)
`POST /admin/sync/pull { since_version, node_id, cursors }` → changes newer than client, in dependency order (schema→components→content→links→media), paginated. `POST /admin/sync/push { node_id, changes }` → applies with conflict resolution, returns accepted versions + conflicts. Client loop: pull schema (apply DDL) → pull content/media (download blobs by hash) → push oplog → update `sync_state`. Runs on interval + on-demand.

## 5. Conflict resolution
Default **field-level LWW** by `(updated_at, node_id)`, falling back to row-level. Delete-vs-update: newer wins. **Schema conflicts serialized through the server**: reject older `version` on push → client pulls + rebases; surfaced as an explicit "Schema conflict" dialog. **`sync_conflict`** `(entity, document_id, field, local/remote value, resolution, resolved_at)`.

## 6. Schema sync specifics
Pulled schema diffs applied as DDL **before** their content rows; validate `from_version` == local `version` (else divergence → conflict); apply in a transaction, bump version, log. Server returns grouped changes to enforce ordering.

## 7. Media blobs
Metadata syncs like rows; blobs by content hash (pull GET `/uploads/:hash.:ext`, push `POST /admin/sync/blob`). Dedup by hash.

## 8. Guarantees
Within a node: full ACID (op + oplog in one transaction). Across nodes: eventual consistency via LWW + monotonic versions + tombstones. Schema: strongly serialized. Idempotent by `(entity, document_id, sync_version)`.

## 9. UI
Sidebar footer chip (Offline/Synced/Syncing/N conflicts → conflicts panel). Settings → Sync page (remote URL, token, interval, Sync now, last-synced, node id).

---
---

# Part IX — Codegen, Sandboxing & Eject (Decision Record — Accepted 2026-07-30)

## 1. Context & question
Strapi never compiles — generated `src/api` files are boilerplate delegating to factories; `schema.json` drives everything, interpreted by Node. So for a 1:1 clone, **codegen is not required** — the dynamic engine already reproduces Strapi's behavior. Codegen is a **capability beyond Strapi**. And because Rust compiles (JS doesn't), per-type codegen is expensive and hostile to offline.

## 2. Decision — three-layer model
Dynamic engine is the runtime spine; sandboxed scripting adds logic; codegen is an **opt-in eject/export**, never on the request path.
| Layer | Role | Location | Codegen | Compile |
|---|---|---|---|---|
| **L1 dynamic CRUD** | serves all CRUD | in-process (both modes) | no | no |
| **L2 scriptable hooks/policies** | custom logic | sandboxed in-process | no | no |
| **L3 eject/export** | standalone Rust project | offline disk / online Docker | **yes (Baker/MiniJinja)** | yes |

## 3. Untangling "sandbox" (three jobs)
Generate = in-process MiniJinja (no container). Build = Docker, server-side only (K8s only for multi-tenant SaaS). Run untrusted logic = WASM(wasmtime)/Rhai in-process, identical offline/online. **You cannot compile native Rust inside WASM** — WASM runs logic, it doesn't build Rust.

## 4. L1 — dynamic engine
Unchanged (Parts II/IV). Content Manager = gateway routing by `uid` into one generic engine, optionally intercepted by L2. Not per-type binaries.

## 5. L2 — scriptable hooks & policies
Hook points mirror Strapi's lifecycle: `before/after Create/Update/Delete`, `before/after FindMany`, request policies (allow/deny), field validators. Context `{ event, uid, action, data, where, user, locale, state }`; may modify data/where, throw, or short-circuit.
Engine: **Rhai** first (embeds easily, no toolchain, sandbox limits), **wasmtime components** later (language-agnostic, fuel/epoch limits, precompiled once). Both in-process, identical across modes.
**`extension_script`** `(schema_uid, kind (hook|policy|validator), hook_point, engine (rhai|wasm), source_or_bytes, enabled, version)` + sync columns.

## 6. L3 — eject/export (Baker + MiniJinja)
For owning code / standalone deploy / max performance. Baker = MiniJinja engine + loop templates + codegen filters (`snake_case, pascal_case, plural, singular, foreign_key, table_case`) + `baker update` with git-style conflict markers + language-agnostic hooks. Pipeline: `schema JSON → context → Baker templates → Rust project → git commit → (online) Docker build / (offline) user builds`. Output = real Axum+SeaORM 2.0 service (dense entities, typed columns, routers/handlers/migrations) generated from the same `Schema` model so it agrees with the engine. Re-eject via `baker update`. Build isolation: single Docker container online (K8s only for multi-tenant); offline emits project, user builds (no embedded toolchain).

## 7. Git strategy
Always track **schema JSON** (like Strapi's `src/api`) — cheap versioning/audit/PR/GitOps. Track generated Rust only in eject mode. Commits work offline; push rides the online/sync path. Ties into `schema_change_log`.

## 8. Consequences & rejected alternatives
Positive: offline instant; faithful clone; one runtime path; sandbox used where it helps. Risks: two logic engines (keep context contract identical), eject drift (generate from one model + snapshot tests), Docker latency (queue + `sccache`), script security (op/time/memory or fuel limits, no ambient file/net). Rejected: per-type runtime compile+load; WASM as a build env; K8s by default; codegen as the primary runtime.

---
---

# Part X — Deployment Modes, Progressive Promotion & GitOps (Proposed)

Extends the 3-layer model with a **progressive execution ladder**. The Content Manager is **always the gateway**: it serves immediately via the dynamic engine and transparently promotes to a faster/owned backend when ready, demoting on failure.

## 1. The execution ladder
| Level | Backend | Where | Trigger | Fallback |
|---|---|---|---|---|
| **L0 dynamic** | in-process `dynamic-store` + hot router | this process | always | — (floor) |
| **L1 container** | generated service in local Docker | same host | Docker + build ok | L0 |
| **L2 microservice** | generated service on Kubernetes | cluster | operator reconcile + healthy | L0/L1 |
| **L3 external/GitOps** | user-owned generated code | anywhere | schema exported + CI built/deployed | not managed |
L0 is floor and fallback in every mode — always functional offline/online.

## 2. Capability detection
`RuntimeCapabilities { docker, kubernetes, registry, git_remote, toolchain }` probed at startup + on config change → selects promotion policy:
- **Desktop, no Docker** → **L0 only** (content-manager + table creation dynamic). Eject still on-demand.
- **Desktop, Docker** → delegate generate→build→run in Docker (**L1**); proxy to container when healthy; fall back to L0.
- **Kubernetes** → app is also **gateway + operator**: serve L0 now; in parallel schedule build job → registry → deploy microservice (**L2**); cut over when ready. Same for the **UI**.

## 3. Desktop mode
No Docker: DDL + hot router in-process (L0). Docker: L0 serves now; background generate→`docker build`→`docker run` (shared config injected); health-check → proxy matching routes to container (L1); demote to L0 on failure. No dropped requests.

## 4. Kubernetes mode — gateway + operator
Immediate: temporary migrations + dynamic REST from schema (L0). Parallel build Job: generate microservice → build in-cluster (Kaniko/BuildKit, rootless) → push internal registry → deploy Deployment/Service/HPA/ConfigMap/Secret with shared DB/tracing/secrets → readiness → gateway cutover to microservice (L2, L0 stays fallback). Operator reconciles desired (schema registry + deploy spec) vs actual continuously; a `ContentService` CRD `{ schemas, topology, data_strategy, config_refs }` makes it declarative `[DECISION: CRD vs internal spec]`. UI follows the same ladder.

## 5. Gateway resolution & cutover
Route table `uid → BackendRef { Dynamic|Container|Service|External }`. Rules: resolve to highest ready; health-gated promotion; blue/green (keep old warm until N successes); automatic demotion (to next ready, ultimately L0); observable (events/spans, UI shows "Serving: dynamic/container/microservice"); **schema-parity guard** (backend schema hash == registry hash before traffic).

## 6. Shared config propagation
Generated backends inherit DB (shared or dedicated/provider), OpenTelemetry endpoint + labels, secrets (JWT/API/provider), feature flags — via a `ServiceConfigBundle` rendered to env/ConfigMap/Secret. 12-factor; nothing hard-coded or baked into images.

## 7. Codegen independence & topologies (key)
Generated code stands alone (no `ferriscms` runtime dependency); `ferriscms` becomes an optional no/low-code design surface; **data need not be tied to it**.
- **Topologies**: monolith / microservices (per schema or bounded group) / monorepo / multirepo / UI app(s).
- **Data strategies**: shared DB / dedicated DB / external provider (Postgres/MySQL/SQLite/BaaS adapters `[LATER]`) / headless (schema+OpenAPI only).
- **Best-practices requirement**: service (layered structure, typed errors, env config, migrations, health/readiness, OTel, tests, Dockerfile, CI, README, dense SeaORM entities); UI (component structure, a11y, typed API client from schema/OpenAPI, env config, tests, CI). A **conformance checklist** the templates must pass (lint/format/test) before "built".

## 8. GitOps codegen
Schema-as-source-of-truth in GitHub/GitLab/Gitea (mirrors Strapi's `src/api`), via a Git provider adapter.
**Draft/publish → branch/merge:** edit (draft) → commit to `schema/<uid>/draft` → CI validates (no deploy); publish → merge to `main` → CI builds + deploys (GitOps); discard → close branch. Generated CI: validate → codegen → build → test → push image → deploy (manifests/Helm/Argo/Flux). App can open PRs, set statuses, read results to update L3 readiness. Two paths to L2/L3: in-app operator (self-contained) vs GitOps (decoupled); `promotion.driver = operator | gitops | both` (GitOps recommended for fully independent code/data).

## 9. Failure handling
Never block writes on build (L0 accepts); atomic health-gated cutover + auto-demotion; schema-parity guard; rootless in-cluster / resource-limited desktop builds; rollback to previous image/deploy; secrets via Secret/env only; transactional provider migrations; unmap-not-drop by default.

## 10. Backend state machine + table
`Dynamic(L0) → Generating → Building → Pushing → Deploying → HealthChecking → Live(proxied)`; any failure → stay/return to L0; health loss → Demoting → L0. **`service_backend`** `(schema_uids, mode (docker|k8s|gitops), topology, data_strategy, image_ref, endpoint_url, state, schema_hash, health, config_bundle_ref)` + sync columns.

## 11. Open decisions
Promotion driver default per mode; CRD vs internal spec; microservice grouping policy; default data strategy on promotion (+ whether it may switch stores); UI codegen stack (Dioxus-only vs pluggable); provider adapter set for v1; draft/publish→Git granularity.

---
---

# Part XI — Phased Delivery Plan (unified roadmap)

Each phase is shippable. Cross-references point to the Part above. Definition of Done per task: compiles + clippy clean; tests added; matches the referenced section; no `[LATER]` creep; works on SQLite + Postgres where DB is touched.

## Phase 0 — Workspace bootstrap
Cargo workspace + 11 crate skeletons (Part I §3); dependencies; DB driver feature flags; CI (build/clippy/test). **Done:** `cargo build --workspace` + both binaries run.

## Phase 1 — Core domain, schema, system DB
`core-domain` enums/newtypes; `core-schema` model/validation/diff (+ tests); `db` SeaORM 2.0 system entities + migrations + `sync()` + seed (Part III); `api-types` DTOs + query parser (+ fixtures). **Done:** system tables on SQLite+Postgres; schema JSON round-trips; parser passes fixtures.

## Phase 2 — CTB engine + dynamic store
`dynamic-store` DDL from `SchemaDiff` + naming + table map + relation/component/DZ/media link tables (Part IV); `services::content_type_builder::apply` (validate→diff→DDL→registry→router rebuild, one transaction); `/content-type-builder/*` + admin JWT; router hot-rebuild. **Done:** create/edit/delete an `Article` CT over HTTP; tables + link tables appear; re-edit adds columns; delete unmaps safely.

## Phase 3 — Content API + CM backend + UI shell + CTB UI
Backend: `services::content` (CRUD + filters/sort/pagination/populate + draft/publish + i18n); dynamic SELECT/nested writes; `/api/*` + `/admin/content-manager/*`; media service + `/admin/upload/*` + thumbnails (Part V).
Frontend: design tokens + base widgets; app shell + sidebar + routing; Login/Register + `client-core` auth; **CTB UI** (working-copy model); **Content Manager** list + edit (schema-driven forms); `client-core` `HttpTransport` + `InProcessTransport`.
Gateway abstraction added (single backend = Dynamic/L0).
**Done:** register → build a CT in the UI → create/publish entries → read from `GET /api/articles`; works in both `server-bin` (WASM) and `desktop-bin` (SQLite).

## Phase 4 — Media UI, Settings/RBAC, users-permissions, content sync, Docker promotion
Media Library UI + picker; Settings (Roles/permission matrix, Users, API Tokens, Locales) + RBAC enforcement; public `/api/auth/local` + public role; **sync engine v1** (oplog, push/pull content+media, LWW) + sync status UI (Part VIII).
**Desktop Docker promotion (L1)** (Part X §2–3): capability detection, health-gated cutover/demotion, `service_backend` table.
**Done:** RBAC works; two desktop nodes + one server converge via sync; a CT can be promoted to a local Docker container with transparent fallback.

## Phase 5 — Schema sync, rich editors, i18n depth, K8s operator, OpenAPI, L2 scripting
Schema sync + conflict review UI (Part VIII §5–6); Rich Text (Blocks) + Markdown editors; i18n end-to-end; OpenAPI (`utoipa`); Configure-the-view, bulk actions, drag-reorder, empty/loaders/toasts polish.
**L2 scripting via Rhai** + `extension_script` + sync (Part IX §5).
**Kubernetes operator (L2)** (Part X §4): in-cluster build Job → registry → deploy → cutover; shared config propagation; UI promotion.
**Done:** feature-parity checklist largely satisfied; K8s promotion cuts over with fallback; hooks/policies run sandboxed.

## Phase 6 — GitOps codegen (L3)
Git provider adapters (GitHub/GitLab/Gitea); schema-as-source-of-truth; **draft→branch / publish→merge** mapping; CI-driven build/deploy; app opens PRs / reads workflow results to update L3 readiness (Part X §8). Eject pipeline (Baker templates + Docker build) from Part IX §6. **Done:** editing a schema commits to a draft branch (CI validates); publishing merges and CI builds+deploys a standalone service.

## Phase 7 — Topologies & data decoupling
Monolith / microservices / monorepo / multirepo outputs; data-ownership strategies (shared/dedicated/provider/headless); provider adapters; best-practices conformance suite; L2 upgrade to **wasmtime** WASM components (Part X §7, Part IX §5). **Done:** generate a fully independent project (own data store) that runs with no `ferriscms` dependency and passes the conformance checklist.

## `[LATER]` (out of scope for phases 0–7)
GraphQL; plugin system/marketplace; Strapi AI; custom fields SDK; webhooks; audit logs; review workflows; data transfer tokens; S3 media; advanced image pipeline; mobile UI; pluggable UI stacks; Argo/Flux integration; multi-cluster; Kubernetes build fan-out for multi-tenant SaaS; gateway proxy to ejected standalone services; local-toolchain offline builds.

## Feature-parity checklist (Strapi core)
Collection & single types; all field types; components (single/repeatable) + categories; dynamic zones; all 6 relation kinds; Draft & Publish; i18n; Content Manager list+edit with configurable views; Media Library with nested folders; REST API with filters/populate/sort/pagination; admin RBAC; API tokens; offline (SQLite) + online (Postgres) parity; offline↔online sync.

## Testing strategy
Unit (`core-schema` validation/diff, query parser, DDL generator via `insta` snapshots); integration (`services` on temp SQLite; CTB apply + content CRUD); API (`insta` response snapshots vs envelopes); UI (widget smoke tests + screen checklists + golden images); sync (multi-node convergence harness).

---
---

# Part XII — Glossary
| Term | Meaning |
|---|---|
| **Content-Type (CT)** | User-defined structure: Collection (many) or Single (one) |
| **Component** | Reusable field group, embedded; repeatable or single |
| **Dynamic Zone (DZ)** | Ordered list of heterogeneous components |
| **Field / Attribute** | Typed slot in a CT/component |
| **Entry** | One record of a collection type |
| **Schema** | JSON definition of a CT/component |
| **UID** | Stable identifier (`api::<singular>.<singular>`, `<category>.<name>`) |
| **document_id** | Stable id shared across locales/draft-published variants |
| **Populate** | Request-time inclusion of related data |
| **Draft & Publish** | Per-CT draft/published workflow |
| **RBAC** | Role-Based Access Control for admins |
| **L0–L3** | Execution ladder: dynamic → container → microservice → external/GitOps |
| **Eject** | Generate a standalone project the user owns |
