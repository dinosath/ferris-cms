# Features

FerrisCMS implements the **core** of Strapi in Rust. This catalog describes
what is implemented today, grouped by functional area.

## Admin & authentication

- **First-run registration** — `POST /admin/register-admin` creates the first
  admin (password hashed with argon2).
- **Login** — `POST /admin/login` returns a signed JWT.
- **JWT sessions** — HS256 tokens with `sub`, `iat`, `exp`, `jti`.
- **`/admin/init`** — reports whether the system is initialized.

## Content-Type Builder (CTB)

Visually define collection types, single types, components, and dynamic zones
at runtime. The builder produces a JSON schema (Strapi-compatible `schema.json`
shape) that is **stored as JSON** and **applied to the database** by generated
DDL.

Supported **field types**:

| Type | Notes |
|---|---|
| `string`, `text`, `richtext`, `blocks` | text / rich text (Markdown & Blocks JSON) |
| `integer`, `biginteger`, `decimal`, `float` | numeric |
| `date`, `datetime`, `time` | temporal |
| `boolean` | boolean |
| `email`, `password` | string-like |
| `enumeration` | fixed option set |
| `json` | arbitrary JSON |
| `uid` | URL-safe identifier with a `targetField` |
| `media` | single/multiple, with allowed types |
| `relation` | all six Strapi relation kinds |
| `component` | reusable, repeatable or not |
| `dynamiczone` | a set of allowed components |

**Advanced field settings:** `required`, `unique`, `private`, `default`,
`min`/`max`, `minLength`/`maxLength`, `regex` (pattern), `enum`, and
conditional visibility (`visibleWhen`).

- **Structural validation** — schemas are validated as a batch before apply:
  duplicate UIDs / API ids / tables, reserved names, enum rules, relation
  target + inverse consistency, component existence, DZ field-collision rules,
  and valid regex.
- **Diffing** — `core-schema::diff` computes added/removed/updated attributes
  and maps changes to compatible/incompatible SQL `ALTER`s.
- **Runtime DDL** — `dynamic-store::ddl` creates/alters host tables and
  auxiliary tables (relation join tables, media/component link tables,
  one-to-many inverse FK columns) in two phases.
- **Removal** — soft-deletes a schema (sets `deleted_at`, negates `version`) and
  drops its tables.
- **Reserved names** — an endpoint exposes reserved API ids / attribute names.

## Content Manager

- **List / get / create / update / delete** for collection types.
- **Filters** (`$eq`, `$ne`, `$gt`, `$gte`, `$lt`, `$lte`, `$in`, `$notIn`,
  `$contains`, `$notContains`, `$containsi`, `$notContainsi`, `$startsWith`,
  `$endsWith`, `$null`, `$notNull`, `$between`, plus `$and`/`$or`/`$not`),
  **sorting**, **pagination**, and **populate**.
- **Draft & Publish** — publish, unpublish, and discard-draft actions.
- **Single types** — get/update a single-type entry.
- **View configuration** — per-content-type configuration endpoint.
- **Payload validation** — before create/update/import, payloads are validated
  against the schema's `required`, `min`/`max`, `minLength`/`maxLength`,
  `regex` (pattern), `enum`, and type constraints; the same check runs again at
  the database layer for defense in depth.

## Public REST API

- Strapi-compatible `{ data, meta }` envelope.
- Query params: `filters`, `populate`, `sort`, `pagination`, `fields`,
  `locale`, and `status`.
- Public CRUD for collection types at `/api/{uid}`.

## Media Library

- Upload files, list, delete.
- Served from the storage directory at `/uploads`.
- Multipart upload with validation.

## RBAC

- Roles and a permission matrix (Super Admin, Editor, Author seeded).
- Permission actions for content (create/read/update/delete/publish) and
  plugins (workflow, credentials).
- User management and per-role permission updates.
- SeaORM `rbac` engine integration with per-user connection restrictions.

## i18n

- Locale registry (list/create/delete).
- Localized content variants (a schema can opt in via `localized`).

## API Tokens

- Read-only, full-access, and custom tokens for the public API.
- Token list/create/delete and permission scoping.

## Workflow automation

- **Workflow editor** — graph-based workflows with a node library.
- **Nodes** — a node registry (n8n-style): content create/update/read,
  HTTP request, AI, condition, delay, etc.
- **Expression engine** — safely-evaluated template expressions.
- **Triggers** — `content.created`, `content.updated`, `content.deleted`,
  `content.published`, and public **webhooks** (`/workflow-hooks/{path}`).
- **Execution engine** — run workflows, track executions, cancel/retry,
  per-node run records.
- **Credentials** — persisted credentials (encrypted) with typed providers.
- **Permissions** — workflow/credential/execution actions integrated into RBAC.
- **Import/export/validate/duplicate/activate** of workflows.

## AI assistant

- **Providers** — OpenAI-compatible, Ollama, Anthropic, Gemini, all run
  through [Rig](https://rig.rs); provider/model CRUD with encrypted keys.
  Adding a provider automatically creates a sensible default model for it.
- **Chat** — assistant conversations + messages + tool-calling loop.
- **Tools** — an RBAC-aware tool registry (the model is never the security
  boundary; it only requests tools that the CMS authorizes and executes).
- **Content** — AI content generation / editing / translation.
- **Schema** — AI content-type / schema generation.
- **Media** — AI media metadata.
- **Usage** — usage + audit accounting.
- **Security** — prompt-injection guard + mutation confirmation.

## Import / Export

- **Import** — parse datasets (CSV/JSON), map source fields to target fields,
  apply transformations (number, boolean, trim, case, replace, split, join,
  default, parse JSON, slug, empty-to-null), validate records against the
  schema (`required`, type, min/max, length, pattern, enum), then create /
  update / upsert through the dynamic store, with detailed per-row results.
- **Export** — export content to a portable dataset.
- **Analyze** — analyze an uploaded dataset before importing.
- **Mappings** — save/load named mapping presets.

## Offline sync

- `sync_state` and `sync_oplog` tables track sync state and operations for the
  offline-first desktop mode.

## Admin UI screens

The Dioxus UI (`crates/app/src/screens/`) includes:

| Screen | Purpose |
|---|---|
| `login` / `register` | authentication |
| `home` | dashboard |
| `content_manager` | list/edit entries |
| `content_type_builder` | define collection/single types, components, DZ |
| `media` | upload library |
| `import_export` | import/export wizard |
| `workflows` / `workflow_editor` | automation |
| `executions` | workflow run history |
| `credentials` | workflow credential management |
| `ai` | AI assistant / chat |
| `settings` | roles, users, API tokens, i18n |
| `shell` | application shell/navigation |
