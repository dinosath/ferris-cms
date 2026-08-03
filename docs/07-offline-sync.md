# 07 — Offline & Online Sync

Prerequisite: [01-architecture.md](01-architecture.md).

Goal: the desktop app works **fully offline** against embedded SQLite, and can **optionally sync** with an online Axum server (Postgres). Sync covers both **schema** (content-type definitions) and **content** (entries + media).

---

## 1. Modes recap

| Mode | Binary | DB | Transport | Sync |
|---|---|---|---|---|
| Offline standalone | `desktop-bin` | SQLite (local) | in-process | off |
| Offline + sync | `desktop-bin` | SQLite (local) | in-process + periodic HTTP to server | on |
| Online | `server-bin` | Postgres | HTTP | is the sync target |

Offline works with zero configuration. Sync is opt-in via config (`sync.remote_url`, `sync.token`).

---

## 2. What must sync

1. **Schema** (`content_type_schemas`, `schema_change_log`, `content_type_table_map`) — structural changes.
2. **Content** (all `ct_*` tables, component tables, link tables).
3. **Media** (`upload_file`, `upload_folder`, and the binary blobs).
4. **System-ish**: locales, content-manager view configs. (Users/roles/tokens are **not** synced by default — they are environment-local.)

---

## 3. Sync data model additions

Every syncable row gets:
- `document_id` (already present for content).
- `updated_at` (UTC, monotonic per node).
- `sync_version` (BIGINT, Lamport-ish counter incremented on each local write).
- `origin_node_id` (UUID of the node that created the row).
- `deleted_at` (soft-delete tombstone; nulls = live).

Add a per-node table:

### `sync_state`
`id, node_id (UUID), remote_url, last_pulled_version, last_pushed_version, last_synced_at`.

### `sync_oplog`
Append-only local change log used to compute pushes:
`id, entity (table), document_id, op (insert|update|delete), sync_version, payload_json, created_at, pushed BOOL`.

`services::sync` writes an oplog entry inside the same transaction as every content/schema mutation (offline mode). This makes push a simple "send un-pushed oplog since last_pushed_version".

---

## 4. Sync protocol (HTTP, server = source of coordination)

Two endpoints on `server-bin` (`/admin/sync`):

- `POST /admin/sync/pull` — body `{ since_version, node_id, cursors }` → returns changes on the server newer than the client has, in dependency order (schema before content, parents before children), paginated by a `next_cursor`.
- `POST /admin/sync/push` — body `{ node_id, changes: [oplog entries] }` → server applies them with conflict resolution, returns accepted versions + any conflicts.

Sync loop (client):
1. **Pull** schema changes first; apply DDL locally via `dynamic-store` (schema convergence must precede content).
2. **Pull** content + media metadata; download new media blobs by hash.
3. **Push** local un-pushed oplog.
4. Update `sync_state`.

Runs on an interval and on-demand (a "Sync now" action in the UI footer, with a status indicator: Synced / Syncing / Offline / Conflicts).

---

## 5. Conflict resolution

Default strategy: **Last-Writer-Wins per field** using `(updated_at, node_id)` as the tiebreaker, applied at the **field level** where possible (merge non-overlapping field edits), falling back to row-level LWW.

- **Content conflicts**: field-level merge; if the same field changed on both sides, newer `updated_at` wins; the losing value is recorded in a `sync_conflict` table for optional review.
- **Delete vs update**: delete wins if its `updated_at` is newer; otherwise the update resurrects the row.
- **Schema conflicts**: structural changes are **serialized through the server**. If two nodes changed the same CT schema, the server rejects the older `version` on push and the client must pull + rebase (re-apply its local schema edits on top). Because schema edits are rare and admin-driven, surface these as an explicit "Schema conflict — review" dialog rather than auto-merging.

### `sync_conflict`
`id, entity, document_id, field, local_value_json, remote_value_json, resolution (auto_lww|manual|pending), resolved_at`.

---

## 6. Schema sync specifics (the tricky part)

Because content tables are created by DDL, a pulled schema change must be **applied as DDL locally before** its content rows arrive:

1. Pull returns `schema_change_log` diffs.
2. Client validates each diff against its local registry version chain (`from_version` must match local `version`; else it's a divergence → schema-conflict flow §5).
3. Client applies the diff via `dynamic-store::apply_schema` in a transaction, bumps local `version`, records the change log.
4. Only then does the client accept content rows targeting the new/changed table.

This ordering is enforced by the server returning changes grouped: `[schema...][components...][content...][links...][media...]`.

---

## 7. Media blob sync

- Metadata syncs like any row. Blobs sync by content hash:
  - Pull: for each new `upload_file.hash` not present locally, `GET /uploads/:hash.:ext` and store in local media dir.
  - Push: `POST /admin/sync/blob` (multipart) for local-only hashes.
- Dedup by hash means identical files transfer once.

---

## 8. Consistency guarantees

- **Within a node**: full ACID (single DB transaction per operation + oplog entry).
- **Across nodes**: eventual consistency; convergence guaranteed by LWW + monotonic versions + tombstones.
- **Schema**: strongly serialized via server to avoid divergent table shapes.
- Idempotency: applying the same oplog entry twice is a no-op (keyed by `(entity, document_id, sync_version)`).

---

## 9. UI surface for sync

- Sidebar footer status chip: `Offline` (grey), `Synced` (green check), `Syncing…` (spinner), `N conflicts` (red) → opens a **Sync/Conflicts** panel listing `sync_conflict` rows with "Keep local / Keep remote" per conflict.
- Settings → "Sync" page: remote URL, token, interval, "Sync now", last-synced time, node id.

---

## 10. Phasing

- **Phase 1–3**: no sync. Offline (SQLite) and Online (Postgres) run independently; the shared-core architecture already guarantees identical behavior.
- **Phase 4**: add oplog + push/pull for content + media (LWW).
- **Phase 5**: schema sync + conflict review UI.

Design the schema (`document_id`, `sync_version`, oplog columns) **from day one** even though the sync engine lands later — retrofitting these columns is expensive.
