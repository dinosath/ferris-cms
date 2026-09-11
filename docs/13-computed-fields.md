# Computed / generated fields

Ferris CMS supports **computed fields**: scalar columns whose value is produced
by the database from an expression over other fields of the same content type.
They are defined in the Content-Type Builder and emitted as native generated
columns (`GENERATED ALWAYS AS (...) STORED|VIRTUAL`).

Examples:

| Domain | Expression |
|--------|------------|
| ERP | `quantity * unit_price` |
| ERP (chained) | `subtotal - discount_amount` |
| CRM | `first_name \|\| ' ' \|\| last_name` |
| Finance | `revenue - expenses` |
| Inventory | `received_qty - sold_qty` |

## Model

A computed field is a normal attribute with four extra properties:

```json
{
  "name": "total_amount",
  "type": "decimal",
  "computed": true,
  "expression": "subtotal + tax",
  "stored": true,
  "dependencies": ["subtotal", "tax"]
}
```

| Property | Meaning |
|----------|---------|
| `computed` | Marks the column as database-generated. |
| `expression` | Portable SQL expression (see below). Required when `computed`. |
| `stored` | `true` = `STORED`, `false` = `VIRTUAL`. Omitted = backend default (STORED, the only mode PostgreSQL allows). |
| `dependencies` | Field names the expression reads. Used for validation/dependency checks; the expression itself is also scanned. |

Supported types: `integer`, `bigint`, `decimal`, `float`, `string`, `text`,
`boolean`, `date`, `datetime`.

### Rules (enforced by schema validation)

- Computed fields **cannot** be `required`, have a `default`, or be `unique`.
- `dependencies` must reference existing fields (scalar or computed).
- The `expression` must parse and only reference existing scalar/computed
  fields of the same content type.
- **Circular dependencies are rejected** (Kahn's algorithm over the
  computed-field graph), including self-reference and chains like
  `a -> b -> a`.
- `expression`/`dependencies` are rejected on non-computed fields.

Validation failures are returned as Strapi-style `ValidationError` details and
surfaced in the Content-Type Builder UI.

## Content-Type Builder UI

The "Add field" modal exposes a **Computed field** section for the supported
scalar types:

- toggle to mark the field computed;
- expression editor;
- storage mode (Stored / Virtual);
- dependency viewer plus a text dependency graph (`field → dependency`);
- generated-DDL preview (`GENERATED ALWAYS AS (<expr>) STORED|VIRTUAL`);
- clickable field-name chips that insert a sibling field into the expression
  (autocomplete);
- live warnings for invalid expressions and unknown references;
- required/unique options are hidden for computed fields.

The admin Content Manager renders computed fields read-only (formula icon,
type, storage mode, expression, value) in the entry view and includes them in
list views; they are stripped from the save payload.

Not implemented (UI polish): full syntax highlighting, a real-time preview that
evaluates the expression against sample values, and a graphical (node-link)
dependency visualization. The UI is compile-validated on the host and wasm32
targets; it is not exercised by a browser-driven test in this repository.

## Expression language

Portable subset that renders on SQLite / PostgreSQL / MySQL:

- column references: `quantity`, `first_name`
- literals: numbers (`100`, `2.5`), single-quoted strings (`' '`), booleans, `NULL`
- arithmetic: `+ - * / %`
- concatenation: `||`
- comparisons: `= <> != < <= > >=`
- parentheses
- functions: `COALESCE`, `ROUND`, `ABS`, `LOWER`, `UPPER`, `LENGTH`, and other
  `NAME(arg, ...)` calls (rendered verbatim)

Parsing lives in `core-schema::expression` (a pure AST); `dynamic-store` renders
the AST to SeaQuery.

## Database generation

`dynamic-store` builds the column with SeaQuery 1.0's native generated-column
support (`ColumnDef::generated(expr, stored)`), which emits:

```sql
-- PostgreSQL (stored only)
total_amount DECIMAL GENERATED ALWAYS AS (subtotal + tax) STORED

-- SQLite / MySQL
full_name TEXT GENERATED ALWAYS AS (first_name || ' ' || last_name) STORED
```

### Chained computed fields are inlined

PostgreSQL **rejects a generated column that references another generated
column** (`ERROR: A generated column cannot reference another generated
column`). Chained definitions such as the ERP example
(`total_amount = subtotal - discount_amount`, where `subtotal` is itself
computed) are therefore **flattened at DDL time**: each reference to a computed
field is substituted with that field's (parenthesized) expression. This is done
for every backend so the emitted DDL is consistent and portable; the evaluated
values are identical. Cycles are rejected by schema validation before this
point.

### Backend capability handling

| Backend | STORED | VIRTUAL | Notes |
|---------|--------|---------|-------|
| PostgreSQL | ✅ | ❌ | A `VIRTUAL` request is upgraded to `STORED`. |
| SQLite | ✅ (CREATE) | ✅ | `ALTER TABLE ADD COLUMN` cannot add a STORED generated column, so an added computed field falls back to `VIRTUAL`. |
| MySQL | ✅ | ✅ | Native. |

### Migrations

| Change | Behaviour |
|--------|-----------|
| Create table with computed field | Column created with the generated clause. |
| Add computed field | `ALTER TABLE ADD COLUMN` (Virtual fallback on SQLite). |
| Alter expression / storage mode | Treated as **incompatible**: the old column is detached (renamed to a unique `*__detached_*`, data retained) and the new generated column is added. |
| Remove computed field | Unmapped (retained), not hard-dropped. |
| Rollback / regenerate | Re-applying a previous schema re-runs the incompatible path safely; detached names are unique so repeated changes do not collide. |

## Runtime behaviour

- **Insert / Update**: computed columns are never written by the application
  (`dynamic-store::build_write_values` skips them). Values are always
  database-derived.
- **Read**: returned like any other column, in list and detail responses.
- **Filter / sort / pagination**: work as for any scalar field
  (`filters[total_amount][$gte]=1000`, `sort[0]=total_amount:desc`).
- **Writes to computed fields are rejected**: a payload that explicitly sets a
  computed field returns `400 ValidationError`. (The spec allowed "silently
  ignore" or "validation error"; this implementation consistently returns a
  validation error across create/update/bulk.) The admin UI excludes computed
  fields from edit forms so normal round-trips are unaffected.

## Metadata

Computed metadata is persisted with the schema (`schema_json`) and exposed by
the Content-Type Builder API, e.g. `GET /content-type-builder/content-types`
returns each attribute with `computed`, `expression`, `stored` and
`dependencies`.

## SeaORM integration

The workspace pins `sea-orm`/`sea-orm-migration` to the `dinosath/sea-orm`
**`generated`** branch via `[patch.crates-io]` (see the root `Cargo.toml`).
That branch adds entity-first generated-column attributes
(`#[sea_orm(generated = "...")]`, `generated_expression`, `column_definition`)
for statically-derived entities. Ferris CMS content types are **runtime
dynamic** (tables are created from JSON at runtime, not from derive macros), so
the canonical DDL path for them is SeaQuery's native `ColumnDef::generated`,
which the branch is built on and which the runtime uses. The migration/entity
tooling on the branch is available for codegen flows that emit entities.

## GraphQL

The CMS itself does not expose a GraphQL API (GraphQL only appears as a workflow
function node), so no GraphQL schema work applies. If a GraphQL layer is added
later, computed fields should be read-only there — they are already read-only at
the REST/store layer.

## Examples

Ready-to-apply content types live in `examples/content-types/`:

- `erp-sales-order.json` — `subtotal = quantity * unit_price`,
  `discount_amount = subtotal * discount / 100`,
  `total_amount = subtotal - discount_amount`.
- `crm-customer.json` — `full_name = first_name || ' ' || last_name`,
  `success_rate = (deals_won * 100) / (deals_won + deals_lost)`.

Apply with the Content-Type Builder API:

```bash
curl -sS -X POST http://localhost:1337/content-type-builder/schema \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d "$(jq -n --slurpfile s examples/content-types/erp-sales-order.json '{schemas:$s}')"
```

## Tests

- `core-schema` unit tests: expression parser and computed-field validation
  (required/default/type, missing dependency, cycles, bad expression).
- `dynamic-store` DDL tests: STORED/VIRTUAL rendering per backend, Postgres
  STORED-only, SQLite ALTER fallback, portable concat.
- `services/tests/computed_migrations.rs`: create/add/alter/rollback/remove.
- `api-rest/tests/computed_fields.rs`: ERP + CRM end-to-end REST scenarios
  (database-derived values, recomputation on update, filtering, sorting, write
  rejection), CTB validation errors, CTB metadata exposure, bulk
  create/update (computed values ignored), and CRUD/pagination/delete/export.
- `services/tests/computed_postgres.rs`: **real PostgreSQL acceptance** (the
  production DB). Driven by the real service API (`ctb_apply` → `cm_*`) against
  Postgres 16, covering chained ERP computed columns (inlined), CRM concat +
  division, filter/sort by computed fields, write rejection, and a
  virtual-requested column upgraded to STORED. Ignored by default:

  ```bash
  TEST_POSTGRES_URL=postgres://postgres:postgres@127.0.0.1:55432/ferriscms \
    cargo test -p services --test computed_postgres -- --ignored --nocapture
  ```

  Note: `init_rbac` currently errors on PostgreSQL with a duplicate-key on
  `sea_orm_permission_action_key`; the server logs this as a warning and
  continues, and the acceptance test mirrors that tolerance (pre-existing,
  unrelated to computed fields).
- `scripts/acceptance_computed_postgres.py`: **live end-user acceptance** — runs
  the real `ferriscms-server` binary against PostgreSQL and drives the ERP + CRM
  scenarios over HTTP (schema apply, create/update, filter/sort, write
  rejection). Verified against PostgreSQL 16, where
  `information_schema.columns` shows the columns as `GENERATED ALWAYS` with the
  nested expressions inlined.

## Performance

Stored generated columns are computed on write and read like a normal column;
virtual generated columns are computed on read. A benchmark harness lives in
`crates/services/tests/computed_performance.rs` (ignored by default):

```bash
cargo test -p services --test computed_performance -- --ignored --nocapture
COMPUTED_PERF_LARGE=1 cargo test -p services --test computed_performance -- --ignored --nocapture
```

It seeds 100 / 10,000 (and, with `COMPUTED_PERF_LARGE=1`, 100,000) rows into
stored and virtual tables and measures insert / query / filtered / sorted
latency. Representative results (SQLite, in-memory):

| mode | rows | insert (ms) | query (ms) | filter (ms) | sort (ms) |
|------|------|-------------|------------|-------------|-----------|
| stored | 100 | 1.8 | 0.36 | 0.30 | 0.31 |
| stored | 10,000 | 166 | 0.85 | 0.77 | 1.51 |
| stored | 100,000 | 1726 | 4.8 | 6.1 | 11.9 |
| virtual | 100 | 1.8 | 0.34 | 0.30 | 0.36 |
| virtual | 10,000 | 172 | 1.00 | 1.38 | 1.62 |
| virtual | 100,000 | 1778 | 4.7 | 7.8 | 13.4 |

Stored columns are comparable to virtual on raw reads and modestly faster on
filtered/sorted reads at scale (the expression is not recomputed per row). On
PostgreSQL only `STORED` exists, so this is also the required mode there.

## Bulk APIs

Bulk writes are supported and **ignore** user-supplied computed values (the
values are stripped and the database derives them), so batches that echo full
rows succeed:

| Method | Route | Body |
|--------|-------|------|
| `POST` | `/admin/content-manager/collection-types/{uid}/bulk` | `{ "data": [ { ... }, { ... } ] }` |
| `PUT`  | `/admin/content-manager/collection-types/{uid}/bulk` | `{ "data": [ { "documentId": "...", ... }, ... ] }` |

Single-entry writes (`POST`/`PUT .../collection-types/{uid}[/{id}]`) instead
return a `400 ValidationError` when a computed field is written, surfacing the
mistake. Both paths never persist a user-supplied computed value: it is either
rejected (single) or stripped (bulk).

