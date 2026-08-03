# 02 — Data Model (SeaORM 2.0)

Prerequisite: [01-architecture.md](01-architecture.md).

This document defines **every system table** (fixed, known at compile time, modeled with SeaORM 2.0 entities in the `db` crate) and **how user-defined content-type tables** are represented at runtime (built by `dynamic-store`).

Conventions:
- All tables use `id BIGINT` surrogate primary keys unless noted.
- Timestamps: `created_at`, `updated_at` (UTC), plus `published_at NULLABLE` when Draft & Publish applies.
- Strapi uses a `document_id` (a stable string ID shared across locales/draft-published variants). We adopt the same: every content entry row carries a `document_id TEXT` (UUID-like), and `(document_id, locale, publication_state)` identifies a specific variant.

---

## 1. SeaORM 2.0 features we rely on

From the [2.0 release](https://www.sea-ql.org/blog/2026-07-27-sea-orm-2.0/):

1. **Dense entity format** — relations declared as typed fields on `Model`:
   ```rust
   #[sea_orm::model]
   #[sea_orm(table_name = "admin_user")]
   pub struct Model {
       #[sea_orm(primary_key)] pub id: i64,
       #[sea_orm(unique)] pub email: String,
       pub password_hash: String,
       #[sea_orm(has_many)] pub roles: HasMany<super::admin_user_role::Entity>,
   }
   ```
2. **`BelongsTo<Entity>` / `BelongsTo<Option<Entity>>`** — encodes FK nullability in the type.
3. **Entity-first workflow** — `db.get_schema_registry("strapi_rs::db::entity::*").sync(db).await?` creates/aligns system tables in FK order at startup. Used for **system tables only**.
4. **Nested ActiveModel** — build a parent + children graph and `.save(db)` in one transaction, in FK order. Used for seeding (e.g., create default role + permissions) and for writing entries that embed single-components.
5. **Strongly-typed columns** — `entity::COLUMN.email.contains("...")` in filters; compile-time checked.
6. **RBAC helper** (SeaORM 2.0 ships an RBAC module) — evaluated for our admin RBAC where convenient; otherwise our own tables (§4) are authoritative.

> Use the dense format everywhere in `db`. Generate initial entities with `sea-orm-cli generate entity --entity-format dense` only if introspecting; otherwise hand-write them (entity-first).

---

## 2. System tables — Content-Type registry

These store the **definitions** created by the Content-Type Builder.

### `content_type_schemas`
The source of truth for every user-defined CT and component.

| Column | Type | Notes |
|---|---|---|
| `id` | BIGINT PK | |
| `uid` | TEXT UNIQUE | e.g. `api::article.article` (CT) or `shared.seo` (component). |
| `kind` | TEXT | `collectionType` \| `singleType` \| `component`. |
| `category` | TEXT NULL | component category (null for CTs). |
| `display_name` | TEXT | |
| `singular_api_id` | TEXT NULL | CT only. |
| `plural_api_id` | TEXT NULL | CT only. |
| `schema_json` | JSONB/TEXT | Full schema (info + options + attributes). Canonical format in [03 §3](03-content-type-builder-logic.md). |
| `draft_and_publish` | BOOL | CT option. |
| `i18n_localized` | BOOL | CT option. |
| `is_system` | BOOL | true for built-in CTs (users-permissions user, etc.). |
| `version` | INTEGER | bumped each save; enables optimistic concurrency + sync. |
| `created_at` / `updated_at` | TIMESTAMP | |

> `schema_json` is the authority. The other columns are denormalized copies for fast listing/filtering and are rewritten from `schema_json` on every save.

### `content_type_table_map`
Maps a CT/component UID + attribute path to its physical table/column names. Lets `dynamic-store` translate between the logical schema and physical SQL, and supports safe renames.

| Column | Type | Notes |
|---|---|---|
| `id` | BIGINT PK | |
| `schema_uid` | TEXT FK→`content_type_schemas.uid` | |
| `physical_table` | TEXT | e.g. `ct_articles`. |
| `logical_attr` | TEXT NULL | null row = table-level mapping; else column mapping. |
| `physical_column` | TEXT NULL | |
| `join_table` | TEXT NULL | for relations/components. |

Physical naming rules are defined in [03 §5](03-content-type-builder-logic.md).

---

## 3. User-defined content tables (dynamic — built by `dynamic-store`)

For each **Collection Type** `Article`, `dynamic-store` creates a physical table (default name `ct_articles`) with:

- `id BIGINT PK`
- `document_id TEXT` — stable cross-variant id (indexed).
- `locale TEXT NULL` — when i18n enabled.
- `publication_state TEXT` — `draft` | `published` (when Draft & Publish enabled; otherwise always `published`).
- One column per **scalar** attribute (Text, Number, Boolean, Date, Email, Password, Enumeration, UID, JSON, RichText). Column SQL type derived per [03 §4](03-content-type-builder-logic.md).
- `created_at`, `updated_at`, `published_at NULL`, `created_by_id`, `updated_by_id`.
- Unique index on `(document_id, locale, publication_state)`.

**Single Types** get the same table but the service enforces at most one `document_id`.

### Relations
- **one-to-one / many-to-one / one-way**: FK column `<field>_id` on the owning table (SeaQuery `ALTER TABLE ADD COLUMN <field>_id BIGINT` + FK).
- **one-to-many**: FK lives on the *other* table (inverse of many-to-one). No column on this side; resolved via query.
- **many-to-many / many-way**: **join table** `ct_articles_authors_links (article_id, author_id, article_order, author_order)`; ordering columns preserve Strapi's manual relation ordering.

### Components
- **Single (non-repeatable) component** embedded in a CT: stored via a **component link table** `ct_articles_cmp_seo (id, entry_id, component_id, component_type, field, order)` pointing to the component's own physical table `cmp_shared_seo`. This mirrors Strapi's `*_components` link tables and supports both single and repeatable uniformly.
- **Repeatable component**: same link table, multiple rows ordered by `order`.
- Each component type gets its own physical table `cmp_<category>_<name>` with its scalar columns.

### Dynamic Zones
- Stored through the **same** component link table pattern, but `component_type` distinguishes which component each element is, and `order` gives the zone order. A DZ is essentially a heterogeneous repeatable component list.

> This link-table strategy means adding a component or DZ never alters the parent table (only inserts link rows), which keeps DDL minimal and matches Strapi's storage model closely.

---

## 4. System tables — Admin, Auth, RBAC

### `admin_user`
| Column | Type | Notes |
|---|---|---|
| `id` BIGINT PK | | |
| `email` TEXT UNIQUE | | |
| `first_name` / `last_name` TEXT NULL | | |
| `username` TEXT NULL | | |
| `password_hash` TEXT | argon2id | |
| `is_active` BOOL | | |
| `blocked` BOOL | | |
| `prefered_locale` TEXT NULL | admin UI locale | |
| `created_at`/`updated_at` | | |

### `admin_role`
`id, name (UNIQUE), code (UNIQUE, e.g. strapi-super-admin), description`. Seeded roles: **Super Admin**, **Editor**, **Author** (Strapi defaults).

### `admin_permission`
`id, role_id (FK), action (e.g. plugin::content-manager.explorer.create), subject (CT uid or null), properties_json (fields/locales), conditions_json`. This models Strapi's granular RBAC.

### `admin_user_role` (join)
`user_id, role_id`.

### `api_token`
Public REST API tokens. `id, name, description, type (read-only|full-access|custom), access_key_hash, last_used_at, expires_at, lifespan`.

### `api_token_permission`
For `custom` tokens: `token_id, action` (e.g. `api::article.article.find`).

### `strapi_transfer_token` `[LATER]`
For data transfer between environments.

---

## 5. System tables — Media Library

### `upload_file`
`id, name, alternative_text, caption, width, height, formats_json (thumbnail/small/medium/large), hash, ext, mime, size (KB), url, preview_url, provider (local|...), folder_id (FK NULL), created_at, updated_at`.

### `upload_folder`
`id, name, path_id (INT), path (TEXT materialized path e.g. /1/4), parent_id (FK NULL)`. Supports nested folders like Strapi.

### `upload_file_morph` `[LATER for polymorphic]`
We instead link media to entries via the same component/relation link tables using a dedicated `media` relation kind (see [03 §4 Media](03-content-type-builder-logic.md)): a `*_files_links` join table `(entry_id, file_id, field, order)`.

---

## 6. System tables — i18n

### `i18n_locale`
`id, code (e.g. en, fr-FR) UNIQUE, name, is_default BOOL`. Seeded with `en` default.

Localized entries live in the same CT table, differentiated by the `locale` column; the shared `document_id` ties locale variants together.

---

## 7. System tables — Config / misc

### `core_store`
Key-value store for runtime config Strapi keeps in DB: `id, key UNIQUE, value_json, type, environment, tag`. Used for content-manager view configuration (column order, list layout) and plugin settings.

### `webhook` `[LATER]`
### `audit_log` `[LATER]`

### `strapi_migrations` / `seaql_migrations`
Managed by `sea-orm-migration` for system tables. User-table DDL is **not** tracked here; its history is derivable from `content_type_schemas.version` + a `schema_change_log` table:

### `schema_change_log`
`id, schema_uid, from_version, to_version, diff_json, applied_at, applied_by`. Enables sync + audit of structural changes (critical for offline↔online schema merges — see [07](07-offline-sync.md)).

---

## 8. Content-Manager layout config

### `content_manager_configuration` (or stored in `core_store`)
Per-CT UI configuration: which fields show in the **list view**, their order, sortable flags, default sort, page size, and the **edit view** field layout (rows/columns). Mirrors Strapi's `configureView`. Shape:

```json
{
  "uid": "api::article.article",
  "settings": { "defaultSortBy": "createdAt", "defaultSortOrder": "DESC", "pageSize": 10, "mainField": "title" },
  "metadatas": { "title": { "list": { "label": "Title", "sortable": true, "searchable": true }, "edit": { "label": "Title", "description": "", "placeholder": "", "visible": true, "editable": true } } },
  "layouts": { "list": ["title", "author", "publishedAt"], "edit": [[{"name":"title","size":6},{"name":"slug","size":6}], [{"name":"body","size":12}]] }
}
```

The `edit.layouts` grid (12-column) is consumed directly by the UI to render the entry form — see [06 §6](06-ui-screens.md).

---

## 9. Seed data (created on first boot)

1. Roles: Super Admin, Editor, Author + their permissions.
2. Default locale `en` (is_default = true).
3. Built-in `plugin::users-permissions.user` and `.role` CTs (marked `is_system`).
4. Content-manager default configs for system CTs.
5. First-run: create the initial Super Admin via the **registration screen** ([06 §2](06-ui-screens.md)); no seeded admin password.

---

## 10. Indexing & constraints checklist

- Unique: every `*.uid`, `admin_user.email`, `i18n_locale.code`, `(document_id, locale, publication_state)` per CT table.
- FK indexes on all `*_id` relation columns and all join-table columns.
- UID fields marked unique in schema → unique index on the physical column (per-locale if i18n).
- Enumeration → CHECK constraint or app-level validation (SQLite lacks native enums; use CHECK or validate in `services`).
