# 11 — Strapi Parity Audit: Content Navigation, Content-Type Builder, RBAC, UI/UX

This document records the audit of `ferriscms` against official Strapi features in
four areas, what was aligned in this pass, and the remaining gaps. It is meant to
be the living checklist for the "1:1 Strapi clone" goal (see [00-overview.md](00-overview.md)).

Legend: ✅ aligned · 🟡 partial · ❌ missing

---

## 1. Content navigation (Content Manager)

Official Strapi behaviour: the Content Manager secondary nav groups content-types
under **COLLECTION TYPES** and **SINGLE TYPES**; collections open a list view with
a configurable table, and clicking an entry opens an edit view; single types open
directly in the edit view.

### Aligned in this pass
- ✅ **Dual-section secondary nav.** `crates/app/src/screens/content_manager.rs`
  now splits `ctb_list()` results into COLLECTION TYPES and SINGLE TYPES.
- ✅ **Collection list view.** Table with ID, main field, **State badge**
  (Draft/Published), Updated At, bulk-selection checkboxes, and a bulk action bar.
- ✅ **Entry edit view.** Clicking a row opens a schema-driven form with back
  navigation, **Save**, and **Publish** controls (when Draft & Publish is on).
- ✅ **Single type edit view.** Selecting a single type opens its one entry
  directly in the edit view (`document_id = "default"`), matching Strapi.
- ✅ **Toolbar + pagination.** Search box (client-side filter on the main field),
  row-per-page selector (10/25/50/100), page controls.
- ✅ **RBAC-aware reads/mutations.** Backend enforces read/create/update/delete/
  publish per content-type (see §3).

### Remaining gaps 🟡
- No per-row edit/delete action buttons (bulk delete exists; edit is click-on-row).
- No "Configure the view" modal wired into the toolbar (the API and DTO exist:
  `cm_get_configuration` / `cm_update_configuration`).
- No draft/publish **Unpublish** or **Discard changes** controls in the edit view
  (endpoints exist: `/actions/discard`).
- No i18n locale switcher or Information rail persistence of created/updated by-user.
- Draft & Publish not yet wired to the list query (`status=published`).

---

## 2. Content-Type Builder

Official Strapi behaviour: a 3-way secondary nav (collections / single types /
components), a create-type modal with Basic/Advanced tabs (Draft & Publish, i18n),
a field picker with the full official field set, per-type configuration, and a
batch Save (unsaved-change badges N/M/D, Undo/Redo/Discard).

### Aligned in this pass
- ✅ **Full official field picker.** `PICKABLE_FIELDS` now lists the 15 official
  picker entries in Strapi's order with the verbatim descriptions: Text, Rich text
  (Blocks), Number, Date, Boolean, Relation, Email, Password, Enumeration, Media,
  JSON, Component, Dynamic Zone, Rich text (Markdown), UID.
- ✅ **Single-type creation.** Create modal has a Collection/Single segmented
  control; the built `Schema` sets `kind` correctly.
- ✅ **Draft & Publish + i18n toggles.** Create modal toggles set
  `options.draftAndPublish` and `pluginOptions.i18n.localized`.
- ✅ **Type-aware field config.** Field-config modal adds Number format, Date type,
  and Enumeration values inputs plus Required / Unique / Private advanced toggles;
  the built `Attribute` is inserted into the working copy.
- ✅ **Grouped secondary nav** (COLLECTION TYPES / SINGLE TYPES / COMPONENTS) and a
  batch Save via `ctb_apply`.

### Remaining gaps 🟡 / ❌
- 🟡 No unsaved-change badges (N/M/D), Undo/Redo/Discard, or unsaved-changes
  navigation guard.
- ❌ Relation field builder (6 relation kinds) not implemented; relations are
  pickable but not configurable end-to-end.
- ❌ Component / Dynamic Zone configuration (choose/reuse component, allowed
  components) not implemented.
- 🟡 No media-type selector, UID target-field, regex/min-max validation inputs in
  the field config modal.
- ✅ Backend (validation, diffing, DDL) already supports the full taxonomy; the
  gaps are primarily UI wiring.

---

## 3. RBAC

Official Strapi behaviour: roles carry a **permission matrix** of actions
(`plugin::content-manager.explorer.create/read/update/delete/publish`) scoped per
content-type, with field-level conditions; Super Admin bypasses the matrix.

### Aligned in this pass
- ✅ **Granular per-content-type permissions.** Added Strapi-standard action keys
  (`services::rbac::action`), `can_perform`, `grant_content_permissions`, and
  `enforce_action` in `crates/services/src/rbac.rs`.
- ✅ **Seeding.** Newly created content-types get Editor = full CRUD + publish and
  Author = create/read/update grants automatically (`ctb_apply` →
  `grant_content_permissions`).
- ✅ **Enforcement.** Content service (`cm_list/get/create/update/delete/publish`)
  now calls `enforce_action` for authenticated admins; unauthenticated (public)
  access is not governed by the admin matrix. Super Admin bypasses.
- ✅ **JWT auth middleware actually wired.** `api-rest/src/auth.rs` now implements
  real bearer-token resolution and an `AdminCtx` extractor; every `/admin/**`
  handler requires a valid JWT and builds a per-request `AppContext` with the
  authenticated identity. Previously `ctx.current_user` was never populated, so
  RBAC enforcement never ran at runtime.
- ✅ **Crypto provider fix.** `jsonwebtoken` now enables the `rust_crypto` feature
  — without it, every JWT sign/decode panicked at runtime and auth (and thus the
  whole admin API + RBAC) was broken.
- ✅ **Test.** `services::rbac::tests::granular_permissions_evaluate` validates
  role/permission decisions end-to-end (migration + seed + grants).

### Runtime-validated workflow
A full end-to-end HTTP smoke test now passes against the live Axum server
(register → login → unauthenticated admin request rejected 401 → create
content-type via CTB → create entry → list → publish → public read):
- Auth enforcement rejects `/admin/**` without a token (401) and accepts with one.
- Content-Type Builder `apply` persists schemas into the shared cache (fixed a
  per-request `SchemaCache` clone bug where `replace()` wasn't visible to the
  server-wide context).
- Entry responses use Strapi's camelCase keys (`documentId`, `publicationState`,
  `createdAt`, `updatedAt`) — the DML layer previously returned snake_case
  column names, which broke content-manager reads, publish-by-documentId, and
  the UI's field lookups.

### Remaining gaps 🟡
- 🟡 The RBAC **UI** (Settings → Roles permission matrix) is still a placeholder;
  the backend DTOs (`AdminPermissionDto`, `UpdateRolePermissionsRequest`) and
  endpoints (`/admin/roles/*/permissions`) exist.
- 🟡 Field-level permissions/conditions are stored (`properties`, `conditions`)
  but not yet evaluated against field attributes.
- 🟡 The SeaORM table-level RBAC (`register_content_table`) runs in parallel; it
  is compatible but not the primary enforcement path.

---

## 4. UI / UX

Official Strapi behaviour: consistent design tokens, global sidebar, secondary
navbars per plugin, toasts, confirm dialogs, empty states, loaders, and
schema-driven forms.

### Aligned in this pass
- ✅ **Design tokens** (`crates/ui/src/design/tokens.rs`) cover Strapi's colour /
  typography / spacing scale, referenced by name everywhere.
- ✅ **Global sidebar + secondary navbars** for Content Manager and Content-Type
  Builder render the Strapi grouping.
- ✅ **Schema-driven forms** in the CM create + edit views (Text/Number/Boolean/
  Enumeration field widgets, required toggles).
- ✅ **State badges** (Draft/Published) and bulk-action bar.

### Remaining gaps 🟡
- ❌ **Media Library** (`/media`) and **Settings** (`/settings`) are placeholders —
  no asset grid/upload, roles/users, API tokens, or i18n locale screens.
- 🟡 No toast system, confirm dialogs, or skeleton loaders (status banners used
  instead).
- 🟡 No rich-text (Blocks/Markdown), media picker, relation input, component /
  dynamic-zone widgets in the entry edit form (scalar fields only).

---

## Summary

| Area | Status | Backend | UI |
|---|---|---|---|
| Content navigation | ✅ core aligned | ✅ | 🟡 (edit-view controls, configure-view, i18n) |
| Content-Type Builder | 🟡 | ✅ taxonomy/validation/DDL | 🟡 (relation/component/DZ config, undo/redo) |
| RBAC | 🟡 | ✅ granular model + enforcement | ❌ roles/permissions UI |
| UI/UX | 🟡 | — | ❌ media, settings, toasts, rich editors |

The backend is the strongest part: schema taxonomy, validation, diffing, DDL, the
REST/DTO surface, and now granular RBAC enforcement are Strapi-shaped. The largest
remaining effort is admin-panel UI wiring (Settings/Roles/Media) and the richer
form widgets (relation builder, components, dynamic zones, rich text).
