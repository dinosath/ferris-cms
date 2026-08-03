# 03 — Content-Type Builder Logic

Prerequisite: [02-data-model.md](02-data-model.md).

This is the **core of the clone**. It defines the complete field taxonomy, the canonical schema JSON format, how schema changes become SQL, and all validation rules. Implemented in `core-schema` (model + validation) + `services::content_type_builder` (orchestration) + `dynamic-store` (DDL).

---

## 1. Content-type kinds

| Kind | `kind` value | Cardinality | Physical storage |
|---|---|---|---|
| Collection Type | `collectionType` | many entries | one table `ct_<plural>` |
| Single Type | `singleType` | exactly one entry | one table `ct_<singular>`, service enforces ≤1 row per (locale, state) |
| Component | `component` | embedded only | table `cmp_<category>_<name>` + link tables in host |

---

## 2. Field taxonomy (complete — 1:1 with Strapi)

Each field type below lists: **basic settings**, **advanced settings**, **SQL storage**, and **validation**. Advanced settings shared by most fields: `required`, `unique`, `private` (hidden from API responses), `default`, and a `configurable` flag. i18n adds a per-field `localized` toggle when the CT has i18n on.

### 2.1 Text
- Basic: `name`, sub-type `Short text` (≤255) or `Long text`.
- Advanced: required, unique, max length, min length, regex, default, private, localized.
- SQL: short → `VARCHAR(255)`; long → `TEXT`.
- Validation: length + regex enforced in `services` before write.

### 2.2 Rich Text (Markdown)
- Basic: `name`.
- SQL: `TEXT` (stores markdown source).
- Advanced: required, min/max length, private, localized.

### 2.3 Rich Text (Blocks)
- Basic: `name`. Structured JSON block editor (paragraphs, headings, lists, images, code, quotes, links).
- SQL: `JSONB`/`TEXT` storing an array of block nodes.
- Block node schema (subset, extend later): `{ "type": "paragraph|heading|list|quote|code|image|link", "level"?, "format"?, "children": [ { "type":"text", "text":"", "bold"?, "italic"?, "underline"?, "strikethrough"?, "code"? } ], "url"?, "image"? }`.

### 2.4 Number
- Basic: `name`, `Number format` ∈ {integer, big integer, decimal, float}.
- SQL: integer→`INTEGER`; big integer→`BIGINT`; decimal→`DECIMAL`; float→`DOUBLE PRECISION`.
- Advanced: required, unique, min, max, default, private, localized.

### 2.5 Date
- Basic: `name`, type ∈ {date, datetime, time}.
- SQL: `DATE` / `TIMESTAMPTZ` / `TIME`.
- Advanced: required, unique, default, private, localized.

### 2.6 Boolean
- SQL: `BOOLEAN`. Advanced: required, default, private, localized.

### 2.7 Email
- SQL: `VARCHAR(255)`. Validation: RFC-ish email regex. Advanced: required, unique, default, private, localized.

### 2.8 Password
- SQL: `VARCHAR(255)` (stores a hash; never returned in API — always `private` implicitly).
- Written via argon2 hashing in `services`. Never populated in responses.

### 2.9 Enumeration
- Basic: `name`, `values` (one per line).
- Advanced: required, default, `Name field override` (enum name transformation), private, localized.
- SQL: `VARCHAR` + CHECK constraint (or app-validated on SQLite).
- **Rule**: each value must start with an alphabetical character (Strapi caution about GraphQL crashes). Enforce in validation.

### 2.10 JSON
- SQL: `JSONB`/`TEXT`. Stored/returned as arbitrary JSON. Advanced: required, private, localized.

### 2.11 UID
- Basic: `name`, `Attached field` (optional source field, e.g. `title`).
- Behavior: auto-generates a slug from the attached field on create if empty; enforce uniqueness (per-locale when i18n).
- Advanced: required (implicit true), min/max length, regex, default, `targetField`.
- SQL: `VARCHAR(255)` UNIQUE.

### 2.12 Media
- Basic: `name`, `Type` ∈ {single media, multiple media}, allowed types (images/videos/files/audios).
- Storage: `*_files_links` join table (see [02 §5](02-data-model.md)). Single → ≤1 link row; multiple → many, ordered.
- Advanced: required, private, allowedTypes, localized.

### 2.13 Relation
- Basic: target CT (must be a collection type) + relation kind + field name(s).
- 6 kinds — see §6.
- Advanced: private, `Field name` on each side.

### 2.14 Component
- Basic: `name`, choose existing or create new component, `Type` ∈ {repeatable, single}, `category`.
- Advanced: required, min/max (repeatable count bounds), private.
- Storage: component link table (see [02 §3](02-data-model.md)).

### 2.15 Dynamic Zone
- Basic: `name`, list of allowed components.
- Advanced: required, min/max element count.
- Storage: heterogeneous component link table.
- **Rule**: components allowed in the same DZ cannot declare the same field name with differing types (or differing enum values). Enforced in `core-schema` validation.

### 2.16 Custom fields `[LATER]`
Extend the `Attribute` enum with a `CustomField { custom_field_uid, base_type, options }` variant that reuses an underlying base type's storage. Not in phases 1–5.

---

## 3. Canonical schema JSON format

Stored in `content_type_schemas.schema_json`. This is the **wire + storage contract**. Modeled after Strapi's schema so tooling stays familiar.

### Collection Type example (`api::article.article`)
```json
{
  "uid": "api::article.article",
  "kind": "collectionType",
  "collectionName": "ct_articles",
  "info": { "singularName": "article", "pluralName": "articles", "displayName": "Article", "description": "" },
  "options": { "draftAndPublish": true },
  "pluginOptions": { "i18n": { "localized": true } },
  "attributes": {
    "title":   { "type": "string", "required": true, "maxLength": 255, "pluginOptions": { "i18n": { "localized": true } } },
    "slug":    { "type": "uid", "targetField": "title", "required": true },
    "body":    { "type": "blocks" },
    "cover":   { "type": "media", "multiple": false, "allowedTypes": ["images"] },
    "author":  { "type": "relation", "relation": "manyToOne", "target": "api::author.author", "inversedBy": "articles" },
    "tags":    { "type": "relation", "relation": "manyToMany", "target": "api::tag.tag", "inversedBy": "articles" },
    "seo":     { "type": "component", "component": "shared.seo", "repeatable": false },
    "blocks":  { "type": "dynamiczone", "components": ["shared.hero", "shared.cta"] }
  }
}
```

Attribute `type` discriminator values (map to §2):
`string` (short text), `text` (long text), `richtext` (markdown), `blocks`, `integer`/`biginteger`/`decimal`/`float` (number formats), `date`/`datetime`/`time`, `boolean`, `email`, `password`, `enumeration`, `json`, `uid`, `media`, `relation`, `component`, `dynamiczone`.

### Component example (`shared.seo`)
```json
{
  "uid": "shared.seo",
  "kind": "component",
  "category": "shared",
  "collectionName": "cmp_shared_seo",
  "info": { "displayName": "SEO", "icon": "search" },
  "attributes": {
    "metaTitle":       { "type": "string", "maxLength": 60, "required": true },
    "metaDescription": { "type": "text", "maxLength": 160 },
    "shareImage":      { "type": "media", "multiple": false, "allowedTypes": ["images"] }
  }
}
```

`core-schema` deserializes this into a strongly-typed `Schema` with an ordered `IndexMap<String, Attribute>` (order = display order in UI; preserved on save).

---

## 4. Attribute → SQL type mapping (authoritative table)

| `type` | Extra | SQL (Postgres) | SQL (SQLite) |
|---|---|---|---|
| string | — | VARCHAR(maxLength or 255) | TEXT |
| text | — | TEXT | TEXT |
| richtext | — | TEXT | TEXT |
| blocks | — | JSONB | TEXT (JSON) |
| integer | — | INTEGER | INTEGER |
| biginteger | — | BIGINT | INTEGER |
| decimal | — | DECIMAL | REAL |
| float | — | DOUBLE PRECISION | REAL |
| date | — | DATE | TEXT (ISO) |
| datetime | — | TIMESTAMPTZ | TEXT (ISO) |
| time | — | TIME | TEXT |
| boolean | — | BOOLEAN | INTEGER (0/1) |
| email | — | VARCHAR(255) | TEXT |
| password | — | VARCHAR(255) | TEXT |
| enumeration | — | VARCHAR + CHECK | TEXT (+ app validation) |
| json | — | JSONB | TEXT |
| uid | — | VARCHAR(255) UNIQUE | TEXT UNIQUE |
| media | — | (link table) | (link table) |
| relation | FK/join | (see §6) | (see §6) |
| component | link | (link table) | (link table) |
| dynamiczone | link | (link table) | (link table) |

Nullable unless `required`. Unique → unique index (per-locale when i18n localized).

---

## 5. Physical naming rules (deterministic)

- CT table: `ct_` + snake_case(pluralName). e.g. `Blog Post` → `ct_blog_posts`.
- Component table: `cmp_` + snake_case(category) + `_` + snake_case(name).
- Column: snake_case(attributeName). Reserved-word collisions get a trailing `_`.
- Relation FK column: `<snake attr>_id`.
- M2M join table: `ct_<a_plural>_<attr>_links` with columns `<a_singular>_id`, `<b_singular>_id`, plus `<a>_order`, `<b>_order`.
- Media join table: `<host_table>_<attr>_files_links` `(entry_id, file_id, order)`.
- Component link table: `<host_table>_components` `(id, entry_id, component_uid, component_id, field, order)` (one shared table per host handles all components + DZ).

All mappings persisted in `content_type_table_map` ([02 §2](02-data-model.md)) so renames don't lose data references.

---

## 6. Relations (the 6 kinds) — storage + schema

| UI name | `relation` value | Owner side | Storage |
|---|---|---|---|
| One way | `oneWay` | A | FK `<attr>_id` on A. B unaware. |
| One-to-one | `oneToOne` | A (has `inversedBy` on B) | FK `<attr>_id` on A, unique. |
| One-to-many | `oneToMany` | A lists many B | FK on B (`mappedBy`); no column on A. |
| Many-to-one | `manyToOne` | A points to one B | FK `<attr>_id` on A. |
| Many-to-many | `manyToMany` | both | join table + order columns. |
| Many way | `manyWay` | A lists many B, B unaware | join table, one-directional. |

Schema fields: `target` (UID), `relation` (kind), `inversedBy` / `mappedBy` (the paired field name on the other CT, when bidirectional). Validation ensures the paired field exists and kinds are consistent (e.g., a `manyToOne` on A must pair with a `oneToMany` on B).

Self-referential relations allowed (e.g., `Page` many-to-one `Page` for parent/children page trees — Strapi's nested hierarchy pattern).

---

## 7. The staging/edit model ("unsaved changes")

Strapi's CTB batches all edits and applies them on a single **Save**. We replicate this exactly:

- Client holds a **working copy** of all schemas + a change list.
- Each create/edit/delete/reorder marks the CT or field with a status: `New` (N), `Modified` (M), or `Deleted` (D). These badges render in the UI ([06 §5](06-ui-screens.md)).
- **Undo/Redo/Discard all** operate on the working-copy change stack.
- **Save** sends the entire desired state; server computes diffs vs current registry and applies in one transaction (see [01 §5](01-architecture.md)).
- Server rejects the whole batch on any validation failure; UI keeps unsaved state and shows errors.

This means the CTB API is **declarative**: the client sends the target schema set, not a stream of imperative operations. Simpler for a low-cost agent to implement correctly.

---

## 8. Schema diffing → DDL (`SchemaDiff`)

`core-schema::diff(current, desired)` returns per-CT:
- `AddedAttributes: Vec<Attribute>`
- `ModifiedAttributes: Vec<(old, new)>`
- `RemovedAttributes: Vec<Attribute>`
- `TableCreated` / `TableDropped` flags.

`dynamic-store::apply_schema(diff)` emits, in order:
1. `CREATE TABLE` for new CTs/components (with system columns from [02 §3](02-data-model.md)).
2. `ALTER TABLE ADD COLUMN` for added scalar attrs.
3. Create join/link tables for added relations/media/components/DZ.
4. `ALTER TABLE ... ` type changes for compatible modifications; for incompatible ones (e.g., text→integer) follow Strapi: **rename semantics** — treat as drop+add (data under the old column is retained in DB but detached). Log to `schema_change_log`.
5. Drop columns/tables for removals (soft: Strapi keeps data; we `[DECISION]` keep the column but unmap it, matching Strapi "data kept in DB" behavior). Default: **unmap, don't drop**, to prevent data loss; expose a later "hard delete" admin action `[LATER]`.

All within one transaction; on error, rollback.

---

## 9. Validation rules (enforced in `core-schema`)

1. **API IDs**: `singularName`/`pluralName` are lowercase kebab/snake, unique across CTs, not reserved words (`id`, `document_id`, `created_at`, ...), singular ≠ plural is allowed but both required.
2. **Attribute names**: valid identifier, unique within the CT/component, not a reserved column.
3. **Enumeration values**: non-empty, unique, each starts with a letter.
4. **Relations**: target exists and is a collection type; paired field consistency; no dangling `inversedBy`.
5. **Components**: referenced component UID exists; DZ allowed-components exist.
6. **DZ field-collision rule**: across all components allowed in one DZ, any shared field name must have identical type (and identical enum values). Reject otherwise.
7. **UID targetField**: must reference an existing sibling field.
8. **Single Type**: no uniqueness/relation constraints that assume multiple rows are violated.
9. **Draft & Publish / i18n**: toggling on adds required columns; toggling off is allowed but warns about data.

Validation returns `Vec<FieldError>` with `{ path, code, message }` → surfaced in the UI next to the offending field.

---

## 10. Content-Type Builder service API (internal)

`services::content_type_builder`:
- `list() -> Vec<SchemaSummary>`
- `get(uid) -> Schema`
- `apply(desired: Vec<Schema>) -> Result<Vec<Schema>, ServiceError>` (the batch Save; validates + diffs + DDL + registry upsert + router rebuild).
- `delete(uid) -> Result<()>` (included in a batch `apply` as a removal).
- `list_components(category?) -> Vec<ComponentSummary>`

Exposed over HTTP in [04 §5](04-rest-api.md) under `/content-type-builder`.
