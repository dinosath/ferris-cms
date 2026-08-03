# 06 — UI Screens (Dioxus, pixel-level)

Prerequisite: [05-ui-design-system.md](05-ui-design-system.md).

> Every screen is described so precisely that an agent that cannot see Strapi can rebuild it verbatim. Coordinates/sizes use the tokens from [05](05-ui-design-system.md). Each screen lists: **route/state**, **layout tree**, **exact regions**, **data source (client-core call)**, and **interactions**. Build screens only from the base widgets in [05 §6].

Global app shell (from [05 §8]): `[ Sidebar 240px ] [ Main (Fill) ]`. The sidebar is present on all authenticated screens except the login/register screens.

---

## 0. Screen inventory

1. Login
2. Register (first-run super admin)
3. App shell + global sidebar
4. Home / dashboard
5. **Content-Type Builder** (list + create modal + edit view + field picker + field config modals) ← highest fidelity
6. **Content Manager** (list view + edit view)
7. Media Library
8. Settings (roles, users, API tokens, i18n locales)
9. Common: toasts, confirm dialogs, empty states, loaders

---

## 1. Login screen

- **Route**: unauthenticated root. Centered card on full `NEUTRAL_100` background.
- **Layout**: a single centered `SCard` width **552px**, padding `SP_9` (48). `flow: Down`, `align: {x:0.5}`.
  - Logo block: 40px square brand mark (violet `PRIMARY_600`) centered, margin-bottom `SP_6`.
  - Title `TEXT_BETA` `NEUTRAL_900` "Welcome!" centered.
  - Subtitle `TEXT_BODY` `NEUTRAL_600` "Log in to your account" centered, margin-bottom `SP_7`.
  - `STextField` label "Email" placeholder "kai@doe.com". margin-bottom `SP_5`.
  - `STextField` label "Password" type=password, trailing `eye`/`eye_off` toggle. margin-bottom `SP_4`.
  - `SCheckbox` "Remember me" left-aligned. margin-bottom `SP_6`.
  - `SButtonPrimary` "Login" **width Fill**.
  - Error banner (if login fails): full-width `DANGER_100` bg, `DANGER_700` text, radius 4, padding `SP_3`, above the email field.
- **Data**: `client-core.auth_login(email, password)` → `POST /admin/login`. On success store JWT (desktop: OS keychain/file; web: memory + httpOnly-ish localStorage) and route to Home.
- **Validation**: inline required + email format; disable button while pending (spinner in button).

---

## 2. Register screen (first run only)

Shown when `GET /admin/init` reports no admin exists.
- Same card layout; title "Welcome!", subtitle "Credentials are only used to authenticate in the admin panel. All saved data will be stored in your database."
- Fields (each `STextField`, stacked, `SP_5` gap): First name*, Last name, Email*, Password*, Confirm Password*.
- Password rules helper under password: "Must be at least 8 characters, 1 uppercase, 1 lowercase, 1 number." Validate live.
- `SCheckbox` "Keep me updated..." (optional, no-op).
- `SButtonPrimary` "Let's start" Fill.
- **Data**: `POST /admin/register-admin`. On success auto-login → Home.

---

## 3. Global sidebar (left, 240px)

`flow: Down`, bg `NEUTRAL_0`, right border `NEUTRAL_150`, padding top `SP_4`.

- **Header** (height 56, padding H `SP_4`): brand mark 32px + workspace name `TEXT_EPSILON` `NEUTRAL_900` "ferriscms". Bottom border `NEUTRAL_150`.
- **Search row** (padding `SP_4`): `SSearchInput` placeholder "Search". (filters nav) — optional phase 2.
- **Primary nav** (`SNavItem` list, `SP_1` gaps, padding H `SP_3`):
  - `stack` icon — **Content Manager** → route `/content-manager`.
  - `grid` icon — **Content-Type Builder** → `/content-type-builder`.
  - `image` icon — **Media Library** → `/media`.
- **Divider** `NEUTRAL_150` margin `SP_4`.
- **Section label** `TEXT_LABEL` `NEUTRAL_500` "GENERAL", padding H `SP_4`, margin-bottom `SP_2`.
  - `cog` — **Settings** → `/settings`.
- **Spacer Fill**.
- **User footer** (height 56, top border `NEUTRAL_150`, padding `SP_4`): avatar pill (initials on `PRIMARY_100`, text `PRIMARY_600`) + name `TEXT_BODY` + `more_vertical` ghost button opening a menu (Profile, Logout).
- **Active state**: exactly one `SNavItem` is active per current route (see [05 §6.14]).

---

## 4. Home / dashboard

- **Top bar** (56px, bg `NEUTRAL_0`, bottom border): title `TEXT_DELTA` "Home".
- **Content** (padding `SP_7`): a welcome `SCard` "Welcome 👋" + grid of quick-link cards to Content-Type Builder, Content Manager, Media, Docs. Each card: icon 24 + title `TEXT_EPSILON` + one-line subtitle `TEXT_BODY` `NEUTRAL_600`. Keep minimal; this is not a critical screen.

---

## 5. CONTENT-TYPE BUILDER (highest fidelity)

Route `/content-type-builder`. This screen has its **own secondary sidebar** inside the main column, so the layout is:

```
[ global sidebar 240 ] [ CTB secondary nav 240 ] [ CTB editor (Fill) ]
```

### 5.1 CTB secondary nav (240px, bg `NEUTRAL_0`, right border `NEUTRAL_150`)

`flow: Down`.
- Header (padding `SP_4`): title `TEXT_DELTA` `NEUTRAL_900` "Content-Type Builder" + a `search` ghost icon on the right (opens inline filter).
- **Section: COLLECTION TYPES**
  - Row: `TEXT_LABEL` `NEUTRAL_600` "COLLECTION TYPES" + count badge; padding H `SP_4`.
  - List of CT entries (`SNavItem`, indent): each shows the CT `displayName`. Active CT highlighted. A trailing status letter badge (N/M/D) appears if that CT has unsaved changes.
  - Below list: a text button `+ Create new collection type` (`PRIMARY_600` text, `plus` icon), padding `SP_3 SP_4`.
- **Section: SINGLE TYPES** — same pattern, `+ Create new single type`.
- **Section: COMPONENTS** — grouped by **category**; each category is a collapsible group (`chevron_right/down`) listing its components; `+ Create new component`.
- The **Save** button is NOT here; it's in the editor top bar (5.2). But Strapi places a persistent Save in the CTB sub-nav header area — put a green **Save** `SButtonPrimary`-styled (use `SUCCESS_600`) at the top-right of the editor top bar (5.2), plus a `more_vertical` kebab for Undo/Redo/Discard all.

### 5.2 CTB editor top bar (height 64, bg `NEUTRAL_100`, padding H `SP_7`)

`flow: Right`, `align: {y:0.5}`.
- Left: CT `displayName` `TEXT_BETA` `NEUTRAL_900`; below it, a subtitle `TEXT_PI` `NEUTRAL_600` "Build the data architecture of your content" OR the CT description. A small `pencil` ghost button next to the name opens **Edit content-type settings** modal (5.5).
- Right: `more_vertical` kebab (menu: Undo, Redo, Discard all changes — disabled when no unsaved changes) + **Save** button (`SUCCESS_600` bg, white text, "Save"), disabled/greyed when there are no unsaved changes; shows spinner while applying.

### 5.3 CTB editor body (Fill, padding `SP_7`)

When a CT is selected, show an `SCard` (or bordered panel) titled with the CT name and a right-aligned link button `+ Add another field` when the field list is non-empty.

**Field list table** (custom, not the generic table):
- Each field is a **row** (height 56, bottom border `NEUTRAL_150`, hover bg `NEUTRAL_100`, padding H `SP_4`), `flow: Right`, `align: {y:0.5}`:
  - `drag_handle` icon (6-dots) `NEUTRAL_400`, 16px — drag to reorder ([05 §9]).
  - Field-type icon (from [05 §7]) in a 32px rounded `NEUTRAL_100` square.
  - Field **name** `TEXT_BODY_BOLD` `NEUTRAL_800`.
  - Field **type descriptor** `TEXT_PI` `NEUTRAL_500` (e.g. "Text", "Relation with Author (many-to-one)", "Component (repeatable) - shared.seo").
  - Spacer Fill.
  - Status badge (N/M/D) if changed.
  - Trailing actions (appear on row hover): `pencil` ghost (edit field → 5.7) + `trash` ghost (delete field → marks D).
- **Nested display**: for a **component** field, the row is expandable (`chevron`) to reveal the component's own fields indented (read-only preview). For a **dynamic zone**, show the allowed components as chips below the row.
- **Empty state** (new CT with no fields): centered `SEmptyState` — icon, title "Add your first field to this Collection-Type", primary button "**+ Add new field**" (opens field picker 5.6).
- Below the list: full-width dashed-border button "**+ Add another field**" (`PRIMARY_600` text) — opens field picker.

**Data source**: `client-core.ctb_list()` on mount → working copy in a `CtbStore`. All edits mutate the working copy and set dirty flags; nothing calls the server until Save (5.2). Save → `client-core.ctb_apply(desired_schemas)` → `POST /content-type-builder/schema`.

### 5.4 Create Content-Type modal (`SModal`, width 640)

Triggered by "Create new collection/single type".
- **Header**: "Create a collection type" (or single). Close X.
- **Tabs**: `Basic settings` | `Advanced settings` (`STab`).
- **Basic tab body**:
  - `STextField` "Display name" placeholder "Article" (required). On input, derive API IDs live.
  - Two read-only-ish `STextField`s: "API ID (Singular)" = `article`, "API ID (Plural)" = `articles` (editable to fix pluralization). Helper text: "The UID is used to generate the API routes and databases tables/collections."
- **Advanced tab body**:
  - `SToggle` "Draft & publish" (default ON) + helper.
  - `SToggle` "Internationalization" (default OFF) + helper.
- **Footer**: `SButtonSecondary` "Cancel" + `SButtonPrimary` "Continue".
- **On Continue**: close this modal, immediately open the **field picker** (5.6) for the new (in-memory) CT. The CT appears in the secondary nav with an **N** badge. Nothing persisted yet.

Component create modal is the same but Basic tab has: Display name, **Select icon** (icon grid picker with search), **Category** (`SDropdown` create-or-select). No Advanced tab needed beyond optional.

### 5.5 Edit content-type settings modal

Same layout as 5.4 but pre-filled; Basic tab shows Display name + API IDs; Advanced shows Draft&Publish + i18n. Footer: "Cancel" + "Finish". Also contains a **Delete** danger button (bottom-left) that marks the CT for deletion (D badge) — confirmed only on Save.

### 5.6 Field picker modal ("Select a field for your collection type")

`SModal` width 720.
- Header: "Add new field to <CT name>". Close X.
- Optional top tabs: `Default` | `Custom` (Custom empty for now → "No custom fields installed").
- **Grid of field-type cards**, 2 columns × N rows, each card 48px tall, `flow: Right`, hover bg `PRIMARY_100`, radius 4, padding `SP_3`:
  - Left: field-type icon in a colored 32px square (Text/Number = neutral, Relation/Component/DZ = ALTERNATIVE/violet accent).
  - Right: `flow: Down`: name `TEXT_BODY_BOLD` + one-line description `TEXT_PI` `NEUTRAL_500`.
  - Cards, in this exact order (matching [03 §2]): **Text**, **Rich text (Blocks)**, **Number**, **Date**, **Boolean**, **Relation**, **Email**, **Password**, **Enumeration**, **Media**, **JSON**, **Component**, **Dynamic Zone**, **Rich text (Markdown)**, **UID**.
  - Descriptions (use verbatim):
    - Text — "Small or long text like title or description"
    - Rich text (Blocks) — "The new JSON-based rich text editor"
    - Number — "Numbers (integer, float, decimal)"
    - Date — "A date picker with hours, minutes and seconds"
    - Boolean — "Yes or no, 1 or 0, true or false"
    - Relation — "Refers to a Collection Type"
    - Email — "Email field with validations format"
    - Password — "Password field with encryption"
    - Enumeration — "List of values, then pick one"
    - Media — "Files like images, videos, etc"
    - JSON — "Data in JSON format"
    - Component — "A group of fields that you can repeat or reuse"
    - Dynamic Zone — "Dynamically pick components while editing content"
    - Rich text (Markdown) — "The classic rich text editor"
    - UID — "Unique identifier"
- Clicking a card → opens the **field config modal** (5.7) for that type.

### 5.7 Field configuration modal (per type)

`SModal` width 640. Header shows the type icon + "Add new <Type> field". Tabs: `Basic settings` | `Advanced settings` (| `Conditions` `[LATER]`).

**Basic tab** — fields depend on type (from [03 §2]):
- All types: `STextField` "Name" (required; validate identifier). Helper "No space is allowed for the name of the attribute."
- Text: radio/segmented "Type" = Short text | Long text.
- Number: `SDropdown` "Number format" = integer | big integer | decimal | float.
- Date: `SDropdown` "Type" = date | datetime | time.
- Enumeration: `STextArea` "Values (one line per value)".
- Media: segmented "Type" = Multiple media | Single media; `SDropdown` allowed types (images/videos/audios/files) multi-select.
- Relation: the **relation builder** (5.8).
- Component: choose "Create a new component" | "Use an existing component"; if new → inline the component create fields; segmented "Type" = Repeatable | Single.
- Dynamic Zone: after Name, a **component selector** to add allowed components (chips + "+ Add component").
- UID: `SDropdown` "Attached field" (existing sibling fields | None).

**Advanced tab** — checkboxes/toggles from the field's advanced settings ([03 §2]):
- `SCheckbox` "Required field", "Unique field", "Private field (not exposed in API)".
- `STextField` default value (type-appropriate input).
- Text/Number: min/max length or value inputs.
- Regex pattern (text/uid).
- If CT has i18n: `SToggle` "Enable localization for this field" (default ON).

**Footer**: `SButtonSecondary` "Cancel", a link "**+ Add another field**" (saves this field to working copy and reopens the picker), and `SButtonPrimary` "Finish" (saves this field to working copy and closes). New field appears in the list (5.3) with an **N** badge.

### 5.8 Relation builder (inside 5.7 for Relation type)

A horizontal composer:
- Left grey box (`NEUTRAL_100` card): the current CT name (fixed) + its field name input below.
- **Middle**: a row of **6 relation icons** ([03 §6]): one-way, one-to-one, one-to-many, many-to-one, many-to-many, many-way. The selected one is highlighted (`PRIMARY_600` border/bg tint). Hovering shows a tooltip with the sentence form ("Article has and belongs to one Author").
- Right grey box: `SDropdown` "Select a content type" listing all **collection types**; below it the inverse field name input (disabled for one-way/many-way).
- Live sentence under the composer: e.g. "Article has many Authors" updates as selections change.

### 5.9 CTB unsaved-changes behavior

- Any create/edit/delete/reorder updates a `CtbStore` change stack and sets dirty badges.
- The kebab menu (5.2) Undo/Redo pop/replay the stack; "Discard all changes" resets working copy to server state (confirm dialog).
- Navigating away with unsaved changes → confirm dialog "You have unsaved changes. Are you sure you want to leave?".
- **Save** disabled unless dirty; on click calls `ctb_apply`; on success clears badges + shows success toast "Saved"; on validation error, shows field errors inline in the relevant field config + a danger toast.

---

## 6. CONTENT MANAGER

Route `/content-manager`. Layout: `[ global sidebar ] [ CM secondary nav 240 ] [ CM view (Fill) ]`.

### 6.1 CM secondary nav (240px)

- Header "Content Manager" `TEXT_DELTA`.
- Section "COLLECTION TYPES": `SNavItem` per collection CT (sorted alpha) → route `/content-manager/collection-types/:uid`.
- Section "SINGLE TYPES": `SNavItem` per single CT → `/content-manager/single-types/:uid`.

### 6.2 Collection list view

- **Top bar** (56): CT `displayName` `TEXT_DELTA` + entry count `TEXT_BODY` `NEUTRAL_500` "(92 entries found)". Right: `SButtonPrimary` "**+ Create new entry**".
- **Toolbar row** (padding `SP_4 SP_7`, `flow: Right`, gap `SP_3`): `SSearchInput` (Fill up to 320px) + `filter` button "Filters" (opens filter popover) + right side: a `cog` "**Configure the view**" ghost button (opens list-view settings 6.5).
- **Table** (`STable` from [05 §6.13]) filling the content area:
  - Checkbox column (bulk select).
  - Columns come from the CT's list-view config ([02 §8]); header labels + sort arrows on sortable columns. Default columns: `id`, main field (title), a couple of scalars, `State` (Draft/Published badge), `createdAt`, `updatedAt`.
  - **State** cell: `SBadge` — Published green, Draft blue (if Draft&Publish on).
  - Actions cell: `pencil` (edit → 6.3), `trash` (delete → confirm).
- **Bulk action bar**: when rows checked, a bar appears above the table: "N entries selected" + `SButtonSecondary` "Publish" + `SButtonDanger` "Delete".
- **Footer**: pagination — page size `SDropdown` (10/25/50/100) left, page controls (chevrons + page numbers) right.
- **Empty state**: `SEmptyState` "No content found" + "Create new entry".
- **Data**: `client-core.cm_list(uid, query)` → `GET /admin/content-manager/collection-types/:uid`.

### 6.3 Entry edit view

Route `/content-manager/collection-types/:uid/:documentId` (or `/create`).
- **Top bar** (64): back `arrow_left` + entry title (main field value or "Create an entry") `TEXT_BETA`. Right cluster: state controls.
  - If Draft&Publish: `SButtonSecondary` "Save" (saves draft) + `SButtonPrimary` "Publish". If already published with draft changes, show "Publish" + a "Discard changes" option in a kebab. An "Unpublish" appears in the kebab when published.
  - Else: single `SButtonPrimary` "Save".
- **Body**: two columns `flow: Right`, gap `SP_7`, padding `SP_7`:
  - **Main form** (Fill, max 900px): renders fields per the edit layout grid ([02 §8]) — a vertical stack of rows; each row is a 12-col grid of field widgets. Each field widget = its `STextField`/`SDropdown`/`SToggle`/editor with label + helper + inline error. Field widgets by type:
    - Text short → `STextField`; long → `STextArea`.
    - Rich text (Blocks) → block editor widget (paragraph/heading/list/quote/code/image toolbar). Phase-2 rich editor; phase-1 fallback = `STextArea` storing JSON.
    - Rich text (Markdown) → `STextArea` + a live preview toggle.
    - Number → numeric `STextField`.
    - Boolean → `SToggle`.
    - Date → date/time picker widget.
    - Enumeration → `SDropdown`.
    - Email/Password/UID → `STextField` (UID has a "regenerate" `refresh` button + lock toggle).
    - JSON → monospaced `STextArea` with validate-on-blur.
    - Media → media picker card (thumbnail grid + "Add" opening the Media Library picker modal).
    - Relation → relation input: a searchable multi/single select showing linked entries as removable chips, drag to reorder (for ordered relations), "+ Add relation" opens a search dropdown querying `/admin/content-manager/relations`.
    - Component (single) → a bordered sub-card rendering the component's fields.
    - Component (repeatable) → list of collapsible sub-cards, each with drag handle + delete; "+ Add an entry" appends.
    - Dynamic Zone → ordered list of component blocks; each block header shows component name + delete + drag; a "+ Add a component to <zone>" button opens a component picker popover listing allowed components.
  - **Right rail** (fixed 320px) `SCard`s:
    - "**Information**" card: state badge, Created/Updated timestamps + by-user, documentId.
    - If i18n: "**Internationalization**" / **Locales** `SDropdown` to switch/create locale variants.
    - "**Editing draft version**" note when applicable.
- **Data**: load `cm_get(uid, documentId)`; save `cm_update`; publish `cm_publish`.

### 6.4 Single type view

Same as 6.3 but no list; the nav item opens directly into the edit view for the one entry (create if none). Header actions identical.

### 6.5 Configure the view (list settings) modal

`SModal` width 720. Header "Configure the view - <CT>".
- "Settings" section: `SDropdown` "Entries per page" (10/25/50/100), `SDropdown` "Default sort attribute", `SDropdown` "Default sort order" (ASC/DESC).
- "View" section: a two-list transfer or a checkbox list of fields to display as columns + drag to order. Toggle sortable/searchable per field.
- Footer "Cancel" / "Save". Persists via `PUT .../configuration`.

---

## 7. Media Library

Route `/media`.
- **Top bar** (56): "Media Library" + entry count. Right: `SButtonSecondary` "+ Add new folder" + `SButtonPrimary` "+ Add new assets".
- **Toolbar**: search + `filter` + sort `SDropdown` + a grid/list toggle.
- **Body** (padding `SP_7`): 
  - Breadcrumb of current folder path.
  - **Folder grid**: folder cards (folder icon + name + item count), then
  - **Asset grid**: cards ~180px wide: thumbnail (contain, `NEUTRAL_100` bg) + filename `TEXT_PI` + ext/size; hover shows checkbox + `pencil`(edit metadata) + `trash`.
- **Upload modal**: drag-and-drop zone ("Drag & drop here or browse files") + from-URL tab; on drop, show upload progress rows; on done, assets appear.
- **Asset detail modal**: large preview + editable Alternative text, Caption, Name; replace media; copy URL; delete.
- **Media picker modal** (used by entry edit view media fields): same grid + a "Select"/multi-select + "Add" footer.
- **Data**: `/admin/upload/*` ([04 §7]).

---

## 8. Settings

Route `/settings`. Layout: `[ global sidebar ] [ settings secondary nav 240 ] [ settings pane (Fill) ]`.

### 8.1 Settings secondary nav
Grouped sections:
- **GLOBAL SETTINGS**: Internationalization, Media Library `[LATER opts]`, API Tokens.
- **ADMINISTRATION PANEL**: Roles, Users.
- (Later) Webhooks, Transfer Tokens.

### 8.2 Roles list + role editor
- List: table of roles (Name, Description, Users count, actions). "+ Add new role".
- Role editor: Name + Description fields; then a **permissions matrix**: an accordion per plugin/CT; rows = actions (create/read/update/delete/publish), columns = checkboxes; a field-level permissions sub-panel when a row is expanded. Save → `PUT /admin/roles/:id/permissions`.

### 8.3 Users list + invite
- Table (Firstname, Lastname, Email, Roles, Active badge, actions). "+ Invite new user" modal (email, first/last, role select). Edit user modal.

### 8.4 API Tokens
- Table (Name, Description, Created, Last used). "+ Create new API Token".
- Create form: Name, Description, `SDropdown` Token type (Read-only/Full access/Custom), duration (7d/30d/90d/unlimited). If Custom → permission matrix like roles. On save, show the raw token **once** in a copy-able banner with a warning "Make sure to copy this token, you won't be able to see it again."

### 8.5 Internationalization
- Table of locales (Display name, Default badge, ISO code, actions). "+ Add new locale" modal: `SDropdown` locale (from ISO list) + display name + "Set as default" toggle.

---

## 9. Common components & states

- **Confirm dialog** (`SModal` width 512): warning icon, title "Are you sure?", message, "Cancel" + `SButtonDanger` "Confirm"/"Delete". Used for deletes, discards, unpublish.
- **Toasts** ([05 §6.15]): success (green) on save/publish; danger on errors; info otherwise. Message text from API or fixed strings.
- **Loaders**: page-level = centered spinner; button-level = inline spinner replacing label; table = skeleton rows (5 grey bars).
- **Empty states** ([05 §6]): icon + title + subtitle + primary action.
- **Error boundary**: if a `client-core` call fails hard, show a full-pane error card with "Retry".
- **Unsaved-changes guard**: any screen with a dirty form intercepts navigation with the confirm dialog.

---

## 10. State management (UI side)

- One `AppState` holding: `auth` (token, current user), `schemas` (cached CT/component list), `route`, and per-screen stores (`CtbStore`, `ContentManagerStore`, `MediaStore`).
- All server interaction via `client-core` (async); results delivered to Dioxus widgets through actions/messages ([01 §7]). Never block the render loop.
- Optimistic UI only in the CTB working-copy model; everything else waits for server confirmation then updates the store.

---

## 11. Build order for the UI (for the implementing agent)

1. Design tokens + base widgets ([05 §11]).
2. App shell + sidebar (§3) + routing.
3. Login/Register (§1–2) + auth wiring in `client-core`.
4. **Content-Type Builder** (§5) end-to-end against `/content-type-builder` — this unblocks everything (you need CTs to exist).
5. **Content Manager** list + edit (§6) driven by schemas.
6. Media Library (§7).
7. Settings (§8).
8. Polish: toasts, confirms, empty/loaders (§9), i18n switching, drag-reorder.
