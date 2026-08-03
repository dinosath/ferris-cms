# 08 — Roadmap & Task Breakdown

Prerequisite: all prior docs.

Phased milestones with acceptance criteria. Each phase is shippable. Tasks are sized for a low-context agent and reference the doc that specifies them.

---

## Phase 0 — Workspace bootstrap

**Goal**: compiling Cargo workspace with all crate skeletons.

Tasks:
- [ ] Create Cargo workspace + the 11 crates in [01 §2](01-architecture.md) with empty `lib.rs`/`main.rs`.
- [ ] Add dependencies: axum, tokio, sea-orm 2.0, sea-query, serde, jsonwebtoken, argon2, validator, tracing, image, dioxus.
- [ ] Feature flags for db driver (`sqlite` default in `desktop-bin`, `postgres` in `server-bin`).
- [ ] CI: `cargo build --workspace`, `cargo clippy`, `cargo test`.

**Acceptance**: `cargo build --workspace` succeeds; both binaries run and print a banner.

---

## Phase 1 — Core domain, schema, system DB

**Goal**: model content-types in memory + persist system tables.

Tasks:
- [ ] `core-domain`: enums + newtypes ([00 §5], [03 §1,§6]).
- [ ] `core-schema`: `Schema`/`Attribute` (de)serialize the canonical JSON ([03 §3]); validation rules ([03 §9]); `diff()` ([03 §8]); unit tests with the example schemas.
- [ ] `db`: SeaORM 2.0 dense entities for all system tables ([02 §2,§4,§5,§6,§7]); migrations; `sync()` on boot; seed data ([02 §9]).
- [ ] `api-types`: DTOs + the Strapi query parser ([04 §3.1]) with a full fixture test suite.

**Acceptance**: system tables created on fresh SQLite + Postgres; schema JSON round-trips; query parser passes all fixtures.

---

## Phase 2 — Content-Type Builder engine + dynamic store

**Goal**: create/edit content-types and have real tables appear.

Tasks:
- [ ] `dynamic-store`: DDL generation from `SchemaDiff` ([03 §8], [02 §3]); physical naming ([03 §5]); table map persistence; relation/component/DZ/media link tables.
- [ ] `services::content_type_builder`: `apply()` batch (validate → diff → DDL → registry upsert → router rebuild) in one transaction ([01 §5]).
- [ ] `api-rest`: `/content-type-builder/*` routes ([04 §5]) + admin JWT auth ([04 §6,§9]).
- [ ] Router hot-rebuild so new CTs serve immediately.

**Acceptance**: via HTTP, create an `Article` CT with text/number/relation/component/DZ fields; tables + link tables exist; re-editing adds columns; deleting unmaps safely.

---

## Phase 3 — Content API + Content Manager backend + UI shell + CTB UI

**Goal**: first end-to-end usable admin.

Backend:
- [ ] `services::content`: entry CRUD, filters/sort/pagination/populate, draft/publish, i18n ([04 §3,§4]).
- [ ] `dynamic-store`: dynamic SELECT/INSERT/UPDATE with populate + nested writes ([02 §3]).
- [ ] `api-rest`: `/api/*` + `/admin/content-manager/*` ([04 §3,§4]).
- [ ] `media` service + `/admin/upload/*` ([04 §7]) with thumbnail generation.

Frontend (Dioxus):
- [ ] Design tokens + base widgets ([05 §11]).
- [ ] App shell + sidebar + routing ([06 §3]).
- [ ] Login/Register ([06 §1,§2]) + `client-core` auth.
- [ ] **Content-Type Builder UI** ([06 §5]) with the working-copy/unsaved model.
- [ ] **Content Manager** list + edit views ([06 §6]) rendering forms from schema + edit-layout config.
- [ ] `client-core`: `HttpTransport` + `InProcessTransport` ([01 §2]) so both binaries share the UI.

**Acceptance**: from a clean install, a user registers, builds an `Article` CT in the UI, creates/publishes entries in the Content Manager, and reads them from `GET /api/articles`. Works in both `server-bin` (web/WASM) and `desktop-bin` (native/SQLite).

---

## Phase 4 — Media UI, Settings/RBAC, users-permissions, content sync

Tasks:
- [ ] Media Library UI ([06 §7]) + media picker in entry edit view.
- [ ] Settings UI: Roles + permission matrix, Users, API Tokens, Locales ([06 §8]); enforce RBAC in `api-rest` middleware ([04 §9]).
- [ ] Users-permissions public auth (`/api/auth/local`) + public role permissions ([04 §6]).
- [ ] Sync engine v1: oplog, push/pull for content + media, LWW conflict handling ([07 §3–§8]).
- [ ] Sync status UI ([07 §9]).

**Acceptance**: full RBAC works; two desktop nodes + one server converge content via sync; conflicts resolve by LWW.

---

## Phase 5 — Schema sync, i18n depth, rich editors, OpenAPI, polish

Tasks:
- [ ] Schema sync + conflict review UI ([07 §5,§6]).
- [ ] Rich Text (Blocks) editor widget + Markdown editor with preview ([06 §6.3]).
- [ ] i18n end-to-end (locale variants, per-field localization) ([04 §8]).
- [ ] OpenAPI generation ([04 §11]).
- [ ] Configure-the-view settings, bulk actions, drag-reorder polish, empty/loaders/toasts ([06 §9]).

**Acceptance**: feature-parity checklist (below) largely satisfied.

---

## `[LATER]` — explicitly out of scope for phases 0–5

GraphQL API, plugin system/marketplace, Strapi AI, custom fields SDK, webhooks, audit logs, review workflows, data transfer tokens, S3 media provider, advanced image pipeline, mobile layout.

---

## Feature-parity checklist (Strapi core)

- [ ] Collection types & single types.
- [ ] All field types ([03 §2]).
- [ ] Components (single/repeatable) + categories.
- [ ] Dynamic zones.
- [ ] All 6 relation kinds.
- [ ] Draft & Publish.
- [ ] Internationalization.
- [ ] Content Manager list + edit with configurable views.
- [ ] Media Library with nested folders.
- [ ] REST API with filters/populate/sort/pagination (Strapi-compatible).
- [ ] Admin RBAC (roles/permissions/users).
- [ ] API tokens.
- [ ] Offline (SQLite) + Online (Postgres) parity.
- [ ] Offline↔online sync.

---

## Testing strategy per phase

- **Unit**: `core-schema` validation/diff, query parser, DDL generator (snapshot the generated SQL with `insta`).
- **Integration**: spin up `services` against a temp SQLite; drive CTB apply + content CRUD; assert row shapes.
- **API**: `insta` snapshots of REST responses vs the envelopes in [04 §2].
- **UI**: Dioxus widget smoke tests + manual screen checklists from [06]; where possible, golden-image tests of key screens.
- **Sync**: multi-node simulation test harness applying interleaved oplogs and asserting convergence.

---

## Definition of Done (per task)

1. Code compiles, `clippy` clean.
2. Tests added and passing.
3. Behavior matches the referenced doc section (cite it in the PR).
4. No `[LATER]` scope creep.
5. Works on both SQLite and Postgres where the task touches the DB.
