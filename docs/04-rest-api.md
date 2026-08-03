# 04 — REST API

Prerequisite: [03-content-type-builder-logic.md](03-content-type-builder-logic.md).

Implemented in `api-rest` (Axum) → `services`. DTOs in `api-types`. Response envelopes and query syntax are **Strapi v5-compatible** so existing Strapi client tooling works unchanged. The same DTOs are consumed by `client-core` for the offline in-process path.

---

## 1. Route groups

| Prefix | Purpose | Auth |
|---|---|---|
| `/api/**` | Public content API (auto-generated per CT). | API token or public role. |
| `/admin/**` | Admin management (login, users, roles, content-manager, media, i18n, settings). | Admin JWT + RBAC. |
| `/content-type-builder/**` | Schema CRUD. | Admin JWT, dev environment only. |
| `/uploads/**` | Static media files. | Public/local. |

---

## 2. Response envelope (all `/api` responses)

```json
{ "data": <entity|array|null>, "meta": { "pagination": { "page": 1, "pageSize": 25, "pageCount": 4, "total": 92 } } }
```

Single entity:
```json
{ "data": { "id": 12, "documentId": "abc123", "title": "Hello", "createdAt": "...", "updatedAt": "...", "publishedAt": "...", "locale": "en" }, "meta": {} }
```

Error envelope (all groups):
```json
{ "data": null, "error": { "status": 400, "name": "ValidationError", "message": "...", "details": { "errors": [ { "path": ["title"], "message": "...", "name": "ValidationError" } ] } } }
```

> Strapi v5 flattened attributes (no nested `attributes` wrapper) — we adopt the flat shape. `documentId` is the stable id; `id` is the row id.

---

## 3. Public content API (`/api`) — generated per collection type

For CT with `pluralName = articles`:

| Method | Path | Operation |
|---|---|---|
| GET | `/api/articles` | list (find) |
| GET | `/api/articles/:documentId` | find one |
| POST | `/api/articles` | create |
| PUT | `/api/articles/:documentId` | update |
| DELETE | `/api/articles/:documentId` | delete |

For a **Single Type** `homepage`:

| Method | Path | Operation |
|---|---|---|
| GET | `/api/homepage` | get the single entry |
| PUT | `/api/homepage` | create-or-update |
| DELETE | `/api/homepage` | delete |

### 3.1 Query parameters (Strapi syntax)

- **fields**: `?fields[0]=title&fields[1]=slug` — select scalar fields.
- **populate**: `?populate=author` | `?populate[author][fields][0]=name` | `?populate=*` (all first-level) | deep: `?populate[seo][populate][shareImage]=true`.
- **filters**: operators `$eq, $ne, $lt, $lte, $gt, $gte, $in, $notIn, $contains, $notContains, $containsi, $startsWith, $endsWith, $null, $notNull, $between, $and, $or, $not`.
  - `?filters[title][$contains]=rust`
  - `?filters[$or][0][title][$contains]=a&filters[$or][1][slug][$eq]=b`
- **sort**: `?sort=createdAt:desc` or `?sort[0]=title:asc&sort[1]=id:desc`.
- **pagination**: `?pagination[page]=1&pagination[pageSize]=25` or `?pagination[start]=0&pagination[limit]=25`; `?pagination[withCount]=true`.
- **locale**: `?locale=fr` (i18n).
- **status**: `?status=draft` | `?status=published` (Draft & Publish). Default published for public role.

Parsing lives in `api-types::query` (a Strapi-query parser: qs-style bracket params → typed `QueryParams`). This parser is a **discrete, testable unit** — give it its own test suite with the examples above as fixtures.

### 3.2 Create/Update body

```json
{ "data": { "title": "Hello", "author": 4, "tags": [1,2], "seo": { "metaTitle": "H" }, "blocks": [ { "__component": "shared.hero", "heading": "Hi" } ] } }
```

- Relations accept id, documentId, array, or connect/disconnect/set object: `{ "author": { "connect": [{"documentId":"x"}] } }`.
- Components: nested object (single) or array (repeatable).
- Dynamic zones: array of `{ "__component": "<uid>", ...fields }`.
- Media: id or array of ids.

Written via `services::content` using nested/dynamic writes ([02 §3](02-data-model.md)); validated against schema ([03 §9](03-content-type-builder-logic.md)).

---

## 4. Admin content-manager API (`/admin/content-manager`)

Powers the Content Manager UI. Richer than public API (returns drafts, edit metadata).

| Method | Path | Operation |
|---|---|---|
| GET | `/admin/content-manager/collection-types/:uid` | list (with admin filters, RBAC field filtering) |
| GET | `/admin/content-manager/collection-types/:uid/:documentId` | find one |
| POST | `/admin/content-manager/collection-types/:uid` | create draft |
| PUT | `/admin/content-manager/collection-types/:uid/:documentId` | update |
| POST | `/admin/content-manager/collection-types/:uid/:documentId/actions/publish` | publish |
| POST | `.../actions/unpublish` | unpublish |
| POST | `.../actions/discard` | discard draft changes |
| DELETE | `.../:documentId` | delete |
| POST | `.../:uid/actions/bulkDelete` | bulk delete |
| POST | `.../:uid/actions/bulkPublish` | bulk publish |
| GET | `/admin/content-manager/single-types/:uid` | get single |
| PUT/POST/DELETE single-type actions | | analogous |
| GET | `/admin/content-manager/content-types` | list CTs for the UI nav |
| GET | `/admin/content-manager/content-types/:uid/configuration` | list/edit view config ([02 §8](02-data-model.md)) |
| PUT | `.../configuration` | save view config |
| GET | `/admin/content-manager/relations/...` | relation option lookup (search targets) |

---

## 5. Content-Type Builder API (`/content-type-builder`)

| Method | Path | Operation |
|---|---|---|
| GET | `/content-type-builder/content-types` | list all CT schemas |
| GET | `/content-type-builder/content-types/:uid` | get schema |
| POST | `/content-type-builder/content-types` | create/apply (batch declarative — [03 §7](03-content-type-builder-logic.md)) |
| PUT | `/content-type-builder/content-types/:uid` | update one (or use batch) |
| DELETE | `/content-type-builder/content-types/:uid` | delete |
| GET | `/content-type-builder/components` | list components |
| POST/PUT/DELETE | `/content-type-builder/components[/:uid]` | component CRUD |
| GET | `/content-type-builder/reserved-names` | reserved field/model names for client validation |
| POST | `/content-type-builder/schema` | **preferred** batch apply of full desired schema set |

Batch apply returns the new schema set + triggers router rebuild ([01 §5](01-architecture.md)).

---

## 6. Auth & Users API

Admin auth (`/admin`):
- `POST /admin/login` → `{ data: { token, user } }` (JWT).
- `POST /admin/register-admin` (first-run super admin).
- `GET /admin/users/me`, `PUT /admin/users/me`.
- `GET/POST/PUT/DELETE /admin/users` (user management, RBAC-guarded).
- `GET/POST/PUT/DELETE /admin/roles`, `GET /admin/roles/:id/permissions`, `PUT /admin/roles/:id/permissions`.
- `GET /admin/permissions` (permission catalog for the role editor).

API tokens (`/admin/api-tokens`): CRUD; regenerate; the raw key shown once on create.

Users-permissions plugin (public auth) `[PHASE 4]`:
- `POST /api/auth/local` (login), `POST /api/auth/local/register`, `GET /api/users/me`, roles/permissions for the public API consumer.

---

## 7. Media API (`/admin/upload`)

- `POST /admin/upload` (multipart) → creates `upload_file`(s), generates thumbnail/small/medium/large via `image` crate.
- `GET /admin/upload/files` (list, filter by folder, mime, search).
- `GET/PUT/DELETE /admin/upload/files/:id`.
- `GET/POST/PUT/DELETE /admin/upload/folders` (nested folders — [02 §5](02-data-model.md)).
- Files served from `/uploads/:hash.:ext` (local provider).

---

## 8. i18n API

- `GET /admin/i18n/locales`, `POST/PUT/DELETE /admin/i18n/locales/:id`.
- Content endpoints accept `?locale=` and, on create, a `?relatedEntityId` / `documentId` to link locale variants (shared `document_id`).

---

## 9. Middleware stack (order matters)

1. CORS.
2. Request id + tracing.
3. Body limit + multipart handling.
4. Auth extractor (JWT for `/admin` + `/content-type-builder`; API token/public for `/api`).
5. RBAC guard (checks `admin_permission` for the resolved action+subject).
6. Route → handler → `services`.
7. Error mapper (`ServiceError` → envelope §2).

---

## 10. Auth model summary

- **Admin JWT**: HS256, `sub = admin_user.id`, short expiry + refresh `[LATER]`; secret from config. Guards `/admin` + `/content-type-builder`.
- **API tokens**: hashed at rest (`sha256`); `read-only` → only `find`/`findOne`; `full-access` → all; `custom` → per-action allowlist (`api_token_permission`).
- **Public role**: unauthenticated `/api` access limited to actions granted to the "Public" users-permissions role `[PHASE 4]`; until then, `/api` requires a token.

---

## 11. OpenAPI

Generate an OpenAPI 3 document from `api-types` + the dynamic schemas at runtime (endpoint `GET /documentation/json`) `[PHASE 5]`. Not required for phases 1–3 but keep DTOs annotation-friendly (`utoipa`).
