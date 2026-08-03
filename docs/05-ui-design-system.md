# 05 — UI Design System (Dioxus)

Prerequisite: [04-rest-api.md](04-rest-api.md).

> **This document exists because the implementing agent cannot see screenshots.** Every color, size, spacing, radius, font weight, and interaction state is specified numerically. Build the design system tokens and base widgets here first; screens in [06-ui-screens.md](06-ui-screens.md) reference these by name. Do not invent values — use these.

The UI is built in **Dioxus** using RSX components. Dioxus renders the same component tree on native (desktop webview) and WASM (web), so the same widgets serve the offline desktop app and the online web admin.

---

## 1. Dioxus primer (what the agent must know)

- UI is declared with the `rsx!{ ... }` macro; every element is an HTML-ish node (`div`, `span`, `input`, `button`, `svg`, ...) or a custom component.
- Layout via CSS inline `style:` strings: `display:flex`, `flex-direction`, `gap`, `padding`, `align-items`, `width`, etc.
- Styling via inline styles referencing the design-token constants (colors, spacing, typography) so there is a single source of truth.
- State via `use_signal` hooks; async work in `spawn`; global app state via a context-provided `Global` (tokens + client + current route).
- Reusable components are `#[component]` functions (e.g. `Button`, `TextField`, `Card`, `Badge`) instantiated by name with named props.
- Naming convention for our components: `Button`, `TextField`, `Card`, `Badge`, `Modal`, `NavItem`, `Toggle`, `Dropdown`, `Table`, `EmptyState`.

Define all tokens in `crates/ui/src/design/tokens.rs` and reuse them from `crates/app` (via `theme`) everywhere.

---

## 2. Color palette (exact hex)

Strapi's admin uses a violet-accented, neutral-gray light theme. Reproduce these values.

### Brand / primary (violet)
| Token | Hex | Use |
|---|---|---|
| `PRIMARY_100` | `#F0F0FF` | primary tint backgrounds (selected nav row, hover). |
| `PRIMARY_200` | `#D9D8FF` | borders of active/focus elements. |
| `PRIMARY_500` | `#7B79FF` | secondary accents. |
| `PRIMARY_600` | `#4945FF` | **main brand** — primary buttons, active nav text/icon, links, focus ring. |
| `PRIMARY_700` | `#271FE0` | primary button hover/pressed. |

### Neutrals (grays)
| Token | Hex | Use |
|---|---|---|
| `NEUTRAL_0`   | `#FFFFFF` | card/surface background. |
| `NEUTRAL_100` | `#F6F6F9` | **app background** (main canvas). |
| `NEUTRAL_150` | `#EAEAEF` | dividers, table row separators. |
| `NEUTRAL_200` | `#DCDCE4` | input borders (default). |
| `NEUTRAL_300` | `#C0C0CF` | disabled borders. |
| `NEUTRAL_400` | `#A5A5BA` | placeholder text, muted icons. |
| `NEUTRAL_500` | `#8E8EA9` | secondary text. |
| `NEUTRAL_600` | `#666687` | labels, table header text. |
| `NEUTRAL_700` | `#4A4A6A` | body text. |
| `NEUTRAL_800` | `#32324D` | headings, strong text. |
| `NEUTRAL_900` | `#212134` | top-level headings, sidebar title. |

### Semantic
| Token | Hex | Use |
|---|---|---|
| `SUCCESS_100/500/600/700` | `#EAFBE7` / `#5CB176` / `#328048` / `#2F6846` | published state, success toasts, "N" not used. |
| `WARNING_100/600/700` | `#FDF4DC` / `#BE5D01` / `#9B4C00` | modified badge, warnings. |
| `DANGER_100/500/600/700` | `#FCECEA` / `#EE5E52` / `#D02B20` / `#B72B1A` | delete buttons, errors, "D" badge, required-field errors. |
| `ALTERNATIVE_100/500/600` | `#F6ECFC` / `#AC73E6` / `#9736E8` | component/relation accents. |
| `SECONDARY_100/500/700` | `#EAF5FF` / `#66B7F1` / `#0C75AF` | info, draft state badge. |

### Status-badge mapping (CTB "N/M/D" + Draft/Publish)
- **New / Draft**: SECONDARY (blue) background `SECONDARY_100`, text `SECONDARY_700`.
- **Modified**: WARNING (orange) `WARNING_100` / `WARNING_700`.
- **Deleted**: DANGER (red) `DANGER_100` / `DANGER_700`.
- **Published**: SUCCESS (green) `SUCCESS_100` / `SUCCESS_700`.

---

## 3. Typography

Font family: **Inter** (bundle the TTF in `crates/ui/resources/fonts/`). Fallback: system sans.

| Token | Size (px) | Weight | Line height | Use |
|---|---|---|---|---|
| `TEXT_ALPHA` | 32 | 600 | 40 | page hero titles (rare). |
| `TEXT_BETA` | 24 | 600 | 32 | main page title (e.g. "Content-Type Builder"). |
| `TEXT_DELTA` | 18 | 600 | 24 | section titles, CT name header. |
| `TEXT_EPSILON` | 16 | 600 | 24 | card titles, modal titles. |
| `TEXT_BODY` | 14 | 400 | 20 | default body text, inputs. |
| `TEXT_BODY_BOLD` | 14 | 600 | 20 | button labels, table headers emphasis. |
| `TEXT_LABEL` | 12 | 600 | 16 | field labels (uppercase optional), table headers. |
| `TEXT_PI` | 11 | 400 | 16 | helper/hint text, badges. |

Weights: Inter Regular(400), Medium(500), SemiBold(600), Bold(700). Bundle at least 400/500/600/700.

---

## 4. Spacing scale (px)

Use a strict scale. Token names `SP_1..SP_10`.

| Token | px |
|---|---|
| `SP_0` | 0 |
| `SP_1` | 4 |
| `SP_2` | 8 |
| `SP_3` | 12 |
| `SP_4` | 16 |
| `SP_5` | 20 |
| `SP_6` | 24 |
| `SP_7` | 32 |
| `SP_8` | 40 |
| `SP_9` | 48 |
| `SP_10` | 56 |

Default gaps: card padding `SP_6` (24), input vertical padding `SP_2` (8) + horizontal `SP_4` (16), row spacing in forms `SP_5` (20), page content padding `SP_7` (32) top/left/right.

---

## 5. Radii, borders, shadows

- `RADIUS_SM` = 4px (badges, inputs, buttons).
- `RADIUS_MD` = 4px (cards) — Strapi uses subtle 4px everywhere.
- `RADIUS_PILL` = 999px (status pills, avatars).
- Border width default 1px, color `NEUTRAL_200`. Focus border `PRIMARY_600` 1px + a 2px outer glow `PRIMARY_200`.
- Card shadow: `0 1px 4px rgba(33,33,52,0.10)` (use CSS `box-shadow`; fall back to a 1px `NEUTRAL_150` border if shadows are unsupported). Prefer a 1px border + very light shadow.

---

## 6. Base widget specs

Define each as a reusable `#[component]` function. States listed must be implemented with Dioxus props + CSS.

### 6.1 `SButtonPrimary`
- Height 36px; padding H `SP_4` (16); radius 4; bg `PRIMARY_600`; text `NEUTRAL_0`, `TEXT_BODY_BOLD`.
- Hover: bg `PRIMARY_700`. Pressed: bg `PRIMARY_700` + inset. Disabled: bg `NEUTRAL_150`, text `NEUTRAL_400`.
- Optional leading icon 16px, gap `SP_2` to label.

### 6.2 `SButtonSecondary` (tertiary/outline)
- Same size; bg `NEUTRAL_0`; 1px border `NEUTRAL_200`; text `NEUTRAL_800`.
- Hover: bg `NEUTRAL_100`. Pressed: border `NEUTRAL_300`.

### 6.3 `SButtonDanger`
- bg `DANGER_600`; hover `DANGER_700`; text white. Used for destructive confirms.

### 6.4 `SButtonGhost` (icon-only)
- 32×32; transparent bg; icon `NEUTRAL_500`; hover bg `NEUTRAL_100`, icon `NEUTRAL_700`.

### 6.5 `STextField`
- Height 40px; bg `NEUTRAL_0`; 1px border `NEUTRAL_200`; radius 4; padding H `SP_4`; text `TEXT_BODY` `NEUTRAL_800`; placeholder `NEUTRAL_400`.
- Focus: border `PRIMARY_600` + glow `PRIMARY_200`. Error: border `DANGER_600`, helper text `DANGER_600`.
- Structure: optional label above (`TEXT_LABEL` `NEUTRAL_600`, margin-bottom `SP_1`), the input, optional helper/hint below (`TEXT_PI` `NEUTRAL_500`, margin-top `SP_1`).

### 6.6 `STextArea`
- Like `STextField` but min-height 120px, multiline, vertical resize handle.

### 6.7 `SDropdown` (select)
- Trigger looks like `STextField` with a trailing chevron-down 16px `NEUTRAL_500`.
- Menu: `NEUTRAL_0` surface, 1px `NEUTRAL_150` border, shadow, radius 4; items 36px tall, hover bg `PRIMARY_100`, selected text `PRIMARY_600` + check.

### 6.8 `SCheckbox`
- 20×20; unchecked border `NEUTRAL_300` bg white; checked bg `PRIMARY_600` + white check glyph; radius 4. Label right, `SP_2` gap.

### 6.9 `SToggle` (switch, for booleans / options)
- Track 40×24 radius pill; off bg `NEUTRAL_200`, knob white 20px; on bg `PRIMARY_600`.

### 6.10 `SBadge` / status pill
- Height 24px; radius pill; padding H `SP_2`; `TEXT_PI` 600. Color per §2 status mapping. Single-letter variant (N/M/D) is a 20×20 rounded square with the letter centered.

### 6.11 `SCard`
- bg `NEUTRAL_0`; radius 4; 1px border `NEUTRAL_150`; padding `SP_6`.

### 6.12 `SModal`
- Overlay: full-screen `rgba(50,50,77,0.20)` scrim. Dialog: centered, `NEUTRAL_0`, radius 4, shadow, width 640 (or 512 small / 830 large). Header row (title `TEXT_EPSILON` + close `SButtonGhost` X), body padded `SP_6`, footer row right-aligned buttons with `SP_2` gap, top border `NEUTRAL_150`.

### 6.13 `STable`
- Header row: bg `NEUTRAL_100`, height 40, `TEXT_LABEL` `NEUTRAL_600`, sticky.
- Body rows: height 52, bg `NEUTRAL_0`, bottom border `NEUTRAL_150`; hover bg `NEUTRAL_100`.
- Checkbox column 48px, then data cells, then an actions cell (edit/delete ghost icons) right-aligned.
- Use `PortalList` for virtualization when >50 rows.

### 6.14 `SNavItem` (sidebar row)
- Height 40; padding H `SP_4`; radius 4; icon 16 + label `TEXT_BODY`.
- Default text `NEUTRAL_700`, icon `NEUTRAL_500`. Hover bg `NEUTRAL_100`. **Active**: bg `PRIMARY_100`, text+icon `PRIMARY_600`, `TEXT_BODY_BOLD`.

### 6.15 `SToast`
- Bottom-center; `NEUTRAL_0` card, 1px border, left color bar 4px (success/danger/warning), icon + message + close; auto-dismiss 4s.

### 6.16 `STooltip`
- `NEUTRAL_800` bg, white `TEXT_PI`, radius 4, padding `SP_1 SP_2`, appears on hover after 400ms.

### 6.17 `SIconButtonWithLabel`, `STab`, `SBreadcrumb`, `SSearchInput` (text field with leading magnifier icon 16px), `SEmptyState` (centered illustration placeholder + title + subtitle + primary action).

---

## 7. Icon set

Use a monochrome 16px/24px line-icon set (bundle inline SVGs rendered through the `Icon` component). Required icons (name them exactly):
`plus, pencil, trash, drag_handle (six dots), chevron_down, chevron_right, chevron_left, search, close (x), check, cog (settings), grid (content-type-builder), stack (content-manager), image (media), users, shield (roles), globe (i18n), key (api-tokens), link (relation), puzzle (component), layers (dynamic-zone), text (Aa), hash (number), calendar (date), toggle (boolean), braces (json), envelope (email), lock (password), list (enumeration), tag (uid), file (media), external_link, filter, sort, more_vertical (kebab), eye, eye_off, arrow_left, refresh, warning_triangle, info_circle, check_circle, x_circle`.

Field-type icons map to the field taxonomy in [03 §2](03-content-type-builder-logic.md) and are reused in the CTB "choose a field" grid.

---

## 8. Layout grid & app shell

- **App shell** is a 3-region layout, `flow: Right`:
  1. **Left sidebar** (fixed width **240px**), bg `NEUTRAL_0`, right border `NEUTRAL_150`.
  2. **Main column** (Fill), bg `NEUTRAL_100`, `flow: Down`:
     - Top bar (height 56px) — optional per screen.
     - Content area — scrollable, padding `SP_7`.
  3. Some screens add a **right panel** (e.g., CTB edit view uses a secondary left nav 240px + editor). See [06](06-ui-screens.md).
- **Forms**: 12-column grid. `size` in the edit layout ([02 §8](02-data-model.md)) maps columns → width fraction (size 6 = 50%, 12 = 100%). Column gap `SP_5`, row gap `SP_5`.
- Max content width for forms: 900px, centered within the content area when the area is wider.

---

## 9. Interaction & animation standards

- Hover/press transitions: 120ms ease-out on bg/border color (CSS `transition`).
- Modal enter: scrim fade 120ms + dialog scale 0.98→1.0 + fade 150ms.
- Toast slide-up 180ms.
- Drag-reorder (fields, DZ items, relation order): show a `drag_handle`; on drag, lift the row (shadow + 2px `PRIMARY_200` outline), show a 2px `PRIMARY_600` insertion line between rows; drop commits reorder.
- Keyboard: Tab order top→bottom; Enter submits primary action in modals; Esc closes modal; `/` focuses search on list screens.

---

## 10. Responsive / window sizing

- Min window 1024×640 (desktop). Below 1280 the sidebar stays 240px; content reflows.
- WASM web build: same, plus the browser scrollbar; ensure `PortalList` handles wheel + touch.
- No mobile layout in phases 1–5 `[LATER]`.

---

## 11. Deliverable checklist for this doc

Implement in `crates/ui/src/design/`:
- `tokens.rs` — colors, spacing, radii, typography constants reused by `crates/app`.
- `widgets/` — one file per base widget in §6, each with all states.
- `icons.rs` — icon registry loading §7 assets.
- `shell.rs` — the app-shell layout in §8.
Every screen in [06](06-ui-screens.md) must be built **only** from these primitives.
