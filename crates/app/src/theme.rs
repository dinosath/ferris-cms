//! Design-token driven inline styles (mirrors `ui::design::tokens`).

use ui::design::tokens::{color, typography, RADIUS_MD};

/// The global font family shared by every widget.
pub const FONT: &str = typography::FONT_FAMILY;

/// Render the design-token CSS custom properties as a `<style>` block.
pub fn token_styles() -> String {
    ui::design::tokens::css_variables()
}

/// Global component stylesheet: a single `<style>` block giving every shared
/// widget consistent, Strapi-5-aligned look with real :hover/:focus/:active
/// and :disabled states (inline styles cannot express those). Component code
/// references these class names instead of duplicating per-screen CSS.
pub fn component_styles() -> String {
    format!(
        r#"
/* ---- Focus ring shared by all interactive elements ---- */
* {{ box-sizing: border-box; }}
html, body {{ margin:0; min-width:0; }}
*:focus-visible {{
    outline: 2px solid {primary600};
    outline-offset: 1px;
    border-radius: 4px;
}}

/* ---- Responsive: let stacked form rows reflow on narrow screens ---- */
@media (max-width: 1024px) {{
    .field-row {{ flex-direction: column; }}
}}

/* ============================ Buttons ============================ */
.btn {{
    display:inline-flex; align-items:center; justify-content:center; gap:8px;
    border:1px solid transparent; border-radius:4px; cursor:pointer;
    font-family:inherit; font-size:14px; font-weight:600; line-height:1;
    white-space:nowrap; user-select:none; text-decoration:none;
    transition: background-color .12s ease, color .12s ease, border-color .12s ease, box-shadow .12s ease;
}}
.btn:disabled {{ opacity:.5; cursor:not-allowed; }}
.btn-sm {{ height:32px; padding:0 12px; font-size:13px; }}
.btn-md {{ height:40px; padding:0 16px; }}
.btn-lg {{ height:48px; padding:0 24px; font-size:15px; }}
.btn-icon {{ height:40px; width:40px; padding:0; }}

.btn-primary {{ background:{primary600}; color:#fff; }}
.btn-primary:hover:not(:disabled) {{ background:{primary700}; }}
.btn-primary:active:not(:disabled) {{ background:{primary800}; }}

.btn-secondary {{ background:#fff; color:{neutral800}; border-color:{neutral200}; }}
.btn-secondary:hover:not(:disabled) {{ background:{neutral100}; }}
.btn-secondary:active:not(:disabled) {{ background:{neutral150}; }}

.btn-tertiary {{ background:transparent; color:{primary600}; border-color:transparent; }}
.btn-tertiary:hover:not(:disabled) {{ background:{primary100}; }}
.btn-tertiary:active:not(:disabled) {{ background:{primary200}; }}

.btn-ghost {{ background:transparent; color:{neutral700}; border-color:transparent; }}
.btn-ghost:hover:not(:disabled) {{ background:{neutral100}; }}

.btn-danger {{ background:{danger600}; color:#fff; }}
.btn-danger:hover:not(:disabled) {{ background:{danger700}; }}

.btn-danger-light {{ background:{danger100}; color:{danger700}; border-color:transparent; }}
.btn-danger-light:hover:not(:disabled) {{ background:#FBDDDA; }}

.btn-success {{ background:{success600}; color:#fff; }}
.btn-success:hover:not(:disabled) {{ background:{success700}; }}

.btn-loading {{ cursor:progress; opacity:.8; }}
.btn-loading::before {{
    content:""; width:14px; height:14px; border:2px solid rgba(255,255,255,.5);
    border-top-color:#fff; border-radius:50%; animation:fc-spin .7s linear infinite;
}}

/* ============================ Inputs ============================ */
.input {{
    width:100%; height:40px; padding:0 14px; border:1px solid {neutral200};
    border-radius:4px; font-family:inherit; font-size:14px; color:{neutral800};
    background:#fff; transition:border-color .12s ease, box-shadow .12s ease;
}}
.input:hover {{ border-color:{neutral300}; }}
.input:focus {{ border-color:{primary600}; box-shadow:0 0 0 3px {primary100}; outline:none; }}
.input::placeholder {{ color:{neutral500}; }}
.input.input-error {{ border-color:{danger600}; }}
.textarea {{ height:auto; min-height:96px; padding:8px 14px; resize:vertical; }}

.select {{
    width:100%; height:40px; padding:0 32px 0 14px; border:1px solid {neutral200};
    border-radius:4px; font-family:inherit; font-size:14px; color:{neutral800};
    background:#fff; cursor:pointer; appearance:none;
    background-image:url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='16' height='16' viewBox='0 0 24 24' fill='none' stroke='%238E8EA9' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpolyline points='6 9 12 15 18 9'/%3E%3C/svg%3E");
    background-repeat:no-repeat; background-position:right 12px center;
}}
.select:hover {{ border-color:{neutral300}; }}
.select:focus {{ border-color:{primary600}; box-shadow:0 0 0 3px {primary100}; outline:none; }}

.field-label {{ display:block; margin-bottom:6px; font-size:12px; font-weight:600; color:{neutral700}; }}
.field-hint {{ margin-top:6px; font-size:11px; color:{neutral500}; }}
.field-error {{ margin-top:6px; font-size:11px; color:{danger700}; }}

/* ============================ Checkbox ============================ */
.checkbox {{ appearance:none; width:18px; height:18px; border:1.5px solid {neutral300};
    border-radius:3px; cursor:pointer; position:relative; background:#fff; flex-shrink:0;
    transition:background-color .12s ease, border-color .12s ease; }}
.checkbox:hover {{ border-color:{primary600}; }}
.checkbox:checked {{ background:{primary600}; border-color:{primary600}; }}
.checkbox:checked::after {{ content:""; position:absolute; left:5px; top:1px; width:5px; height:10px;
    border:solid #fff; border-width:0 2px 2px 0; transform:rotate(45deg); }}
.checkbox:focus-visible {{ outline:2px solid {primary600}; outline-offset:1px; }}

/* ============================ Switch ============================ */
.switch {{ position:relative; width:40px; height:22px; border-radius:999px; border:none;
    cursor:pointer; transition:background-color .15s ease; flex-shrink:0; background:{neutral300}; }}
.switch-checked {{ background:{primary600}; }}
.switch-knob {{ position:absolute; top:2px; width:18px; height:18px; border-radius:50%;
    background:#fff; box-shadow:0 1px 3px rgba(0,0,0,.2); transition:left .15s ease; left:2px; }}
.switch-checked .switch-knob {{ left:20px; }}

/* ============================ Table ============================ */
.table {{ width:100%; border-collapse:collapse; background:#fff; }}
.table-th {{ text-align:left; padding:12px 16px; font-size:12px; font-weight:600; color:{neutral600};
    background:{neutral100}; border-bottom:1px solid {neutral150}; }}
.table-td {{ padding:12px 16px; font-size:14px; color:{neutral800};
    border-bottom:1px solid {neutral150}; }}
.table-row {{ transition:background-color .1s ease; }}
.table-row:hover {{ background:{primary100}; cursor:pointer; }}
.table-row-selected {{ background:{primary100}; }}
.table-row .row-actions {{ opacity:0; transition:opacity .12s ease; }}
.table-row:hover .row-actions {{ opacity:1; }}

/* ============================ Content Manager ============================ */
.cm-screen {{ min-height:calc(100vh - 56px); background:{neutral100}; }}
.cm-page {{ max-width:1360px; margin:0 auto; padding:28px 32px 48px; }}
.cm-heading-row {{ display:flex; align-items:flex-start; justify-content:space-between; gap:20px; margin-bottom:24px; }}
.cm-kicker {{ font-size:12px; font-weight:600; color:{primary600}; margin-bottom:6px; }}
.cm-page-title {{ margin:0; font-size:24px; line-height:1.2; font-weight:600; color:{neutral900}; }}
.cm-page-subtitle {{ margin:6px 0 0; font-size:14px; color:{neutral600}; }}
.cm-toolbar {{ display:flex; align-items:center; gap:10px; flex-wrap:wrap; margin-bottom:16px; }}
.cm-search {{ flex:1 1 260px; max-width:380px; }}
.cm-segment {{ display:inline-flex; align-items:center; padding:3px; gap:2px; border:1px solid {neutral200}; border-radius:6px; background:#fff; }}
.cm-segment button {{ height:32px; padding:0 12px; border:0; border-radius:4px; background:transparent; color:{neutral600}; font:600 12px inherit; cursor:pointer; }}
.cm-segment button:hover {{ color:{primary600}; background:{neutral100}; }}
.cm-segment button.cm-segment-active {{ background:{primary100}; color:{primary700}; }}
.cm-surface {{ background:#fff; border:1px solid {neutral150}; border-radius:6px; box-shadow:0 1px 3px rgba(33,33,52,.05); overflow:hidden; }}
.cm-surface-header {{ display:flex; align-items:center; justify-content:space-between; gap:12px; padding:16px 20px; border-bottom:1px solid {neutral150}; }}
.cm-surface-title {{ font-size:14px; font-weight:600; color:{neutral800}; }}
.cm-table {{ width:100%; border-collapse:collapse; background:#fff; }}
.cm-table th {{ text-align:left; padding:11px 16px; background:{neutral100}; border-bottom:1px solid {neutral150}; color:{neutral600}; font-size:11px; font-weight:600; white-space:nowrap; }}
.cm-table td {{ padding:13px 16px; border-bottom:1px solid {neutral150}; color:{neutral800}; font-size:13px; }}
.cm-table tbody tr:last-child td {{ border-bottom:0; }}
.cm-table-row {{ transition:background-color .1s ease; }}
.cm-table-row:hover {{ background:{primary100}; }}
.cm-table-link {{ padding:0; border:0; background:transparent; color:{primary600}; font:600 13px inherit; text-align:left; cursor:pointer; }}
.cm-table-link:hover {{ color:{primary700}; text-decoration:underline; }}
.cm-row-actions {{ display:flex; gap:4px; opacity:.55; transition:opacity .1s ease; }}
.cm-table-row:hover .cm-row-actions, .cm-row-actions:focus-within {{ opacity:1; }}
.cm-filter-chip {{ display:inline-flex; align-items:center; gap:6px; min-height:28px; padding:4px 9px; border-radius:4px; background:{primary100}; color:{primary700}; font-size:11px; }}
.cm-filter-chip button {{ padding:0; border:0; background:transparent; color:inherit; cursor:pointer; font-size:14px; line-height:1; }}

.cm-editor {{ min-height:calc(100vh - 56px); background:{neutral100}; }}
.cm-editor-topbar {{ position:sticky; top:0; z-index:2; display:flex; align-items:center; gap:12px; min-height:64px; padding:0 32px; background:#fff; border-bottom:1px solid {neutral150}; }}
.cm-editor-back {{ display:inline-flex; align-items:center; gap:8px; padding:0; border:0; background:transparent; color:{neutral600}; font:600 13px inherit; cursor:pointer; }}
.cm-editor-back:hover {{ color:{primary600}; }}
.cm-editor-title {{ min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; font-size:20px; font-weight:600; color:{neutral900}; }}
.cm-editor-actions {{ display:flex; align-items:center; gap:8px; margin-left:auto; flex-wrap:wrap; justify-content:flex-end; }}
.cm-editor-body {{ max-width:1180px; margin:0 auto; padding:28px 32px 48px; display:grid; grid-template-columns:minmax(0,1fr) 296px; gap:24px; align-items:start; }}
.cm-editor-main {{ min-width:0; }}
.cm-form-card {{ padding:24px; background:#fff; border:1px solid {neutral150}; border-radius:6px; box-shadow:0 1px 3px rgba(33,33,52,.05); }}
.cm-form-title {{ margin:0 0 18px; font-size:15px; font-weight:600; color:{neutral800}; }}
.cm-field-grid {{ display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); column-gap:24px; row-gap:0; }}
.cm-field-grid .field {{ min-width:0; }}
.cm-form-section {{ margin-top:24px; padding-top:20px; border-top:1px solid {neutral150}; }}
.cm-info-card {{ padding:20px; background:#fff; border:1px solid {neutral150}; border-radius:6px; box-shadow:0 1px 3px rgba(33,33,52,.05); }}
.cm-info-title {{ margin:0 0 16px; font-size:14px; font-weight:600; color:{neutral800}; }}
.cm-meta-list {{ display:flex; flex-direction:column; gap:13px; }}
.cm-meta-row {{ display:flex; justify-content:space-between; align-items:flex-start; gap:14px; font-size:12px; }}
.cm-meta-label {{ color:{neutral500}; }}
.cm-meta-value {{ color:{neutral800}; font-weight:500; text-align:right; word-break:break-word; }}
.cm-relation-field {{ margin:0 0 18px; padding:16px; background:{neutral50}; border:1px solid {neutral150}; border-radius:4px; }}
.cm-relation-heading {{ display:flex; align-items:center; justify-content:space-between; gap:12px; margin-bottom:10px; }}
.cm-relation-label {{ font-size:12px; font-weight:600; color:{neutral700}; }}
.cm-relation-help {{ font-size:11px; color:{neutral500}; }}
.cm-relation-row {{ display:flex; align-items:center; justify-content:space-between; gap:12px; min-height:40px; padding:8px 10px; margin-bottom:8px; background:#fff; border:1px solid {neutral150}; border-radius:4px; }}
.cm-relation-name {{ min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; font-size:13px; color:{neutral800}; }}
.cm-relation-remove {{ flex:0 0 auto; padding:4px 6px; border:0; background:transparent; color:{danger600}; font-size:11px; cursor:pointer; }}
.cm-relation-remove:hover {{ background:{danger100}; border-radius:3px; }}
.cm-relation-empty {{ padding:4px 0 10px; color:{neutral500}; font-size:12px; }}
.cm-add-relation {{ padding:9px 12px; border:1px dashed {primary600}; border-radius:4px; background:#fff; color:{primary600}; font-size:12px; font-weight:600; cursor:pointer; }}
.cm-add-relation:hover {{ background:{primary100}; }}

@media (max-width: 850px) {{
    .cm-page {{ padding:22px 20px 36px; }}
    .cm-heading-row {{ flex-direction:column; }}
    .cm-editor-topbar {{ padding:0 20px; }}
    .cm-editor-body {{ grid-template-columns:1fr; padding:22px 20px 36px; }}
    .cm-editor-actions {{ margin-left:0; }}
}}
@media (max-width: 620px) {{
    .cm-field-grid {{ grid-template-columns:1fr; }}
    .cm-toolbar {{ align-items:stretch; }}
    .cm-search {{ max-width:none; flex-basis:100%; }}
    .cm-segment {{ width:100%; }}
    .cm-segment button {{ flex:1; }}
}}

/* ============================ Badge ============================ */
.badge {{ display:inline-flex; align-items:center; gap:6px; padding:4px 10px; border-radius:4px;
    font-size:11px; font-weight:600; line-height:1; }}
.badge-dot {{ width:6px; height:6px; border-radius:50%; }}
.badge-draft {{ background:{secondary100}; color:{secondary600}; }}
.badge-published {{ background:{success100}; color:{success600}; }}
.badge-modified {{ background:{warning100}; color:{warning600}; }}
.badge-new {{ background:{primary100}; color:{primary600}; }}
.badge-danger {{ background:{danger100}; color:{danger700}; }}
.badge-neutral {{ background:{neutral100}; color:{neutral600}; }}

/* ============================ Card ============================ */
.card {{ background:{neutral0}; border:1px solid {neutral150}; border-radius:{radius_md}px;
    box-shadow:{shadow_card}; }}
.card-header {{ padding:20px 24px; border-bottom:1px solid {neutral150};
    font-size:15px; font-weight:600; color:{neutral800}; }}
.card-body {{ padding:24px; }}

/* ============================ Field ============================ */
.field {{ display:flex; flex-direction:column; gap:6px; margin-bottom:16px; }}
.field-row {{ display:flex; gap:20px; }}

/* ============================ Tabs ============================ */
.tabs {{ display:flex; gap:4px; border-bottom:1px solid {neutral150}; }}
.tab {{ padding:10px 16px; border:none; background:transparent; color:{neutral600};
    font-size:14px; font-weight:600; cursor:pointer; border-bottom:2px solid transparent;
    margin-bottom:-1px; transition:color .12s ease, border-color .12s ease; }}
.tab:hover {{ color:{primary600}; }}
.tab-active {{ color:{primary600}; border-bottom-color:{primary600}; }}

/* ============================ Breadcrumb ============================ */
.breadcrumb {{ display:flex; align-items:center; gap:8px; font-size:14px; color:{neutral600}; }}
.breadcrumb-link {{ background:none; border:none; padding:0; color:{primary600}; font-weight:500;
    cursor:pointer; font-size:14px; }}
.breadcrumb-link:hover {{ text-decoration:underline; }}
.breadcrumb-sep {{ color:{neutral300}; }}

/* ============================ Pagination ============================ */
.pagination {{ display:flex; align-items:center; justify-content:space-between; padding:16px 32px; }}
.pagination-info {{ font-size:14px; color:{neutral600}; }}
.pagination-controls {{ display:flex; align-items:center; gap:8px; }}

/* ============================ Status indicator ============================ */
.status {{ display:inline-flex; align-items:center; gap:8px; font-size:13px; font-weight:600;
    padding:6px 12px; border-radius:{radius_md}px; }}
.status-draft {{ background:{secondary100}; color:{secondary700}; }}
.status-published {{ background:{success100}; color:{success700}; }}
.status-modified {{ background:{warning100}; color:{warning700}; }}
.status-unpublished {{ background:{neutral100}; color:{neutral700}; }}

/* ============================ Icon button ============================ */
.btn-icon {{ display:inline-flex; align-items:center; justify-content:center; width:32px; height:32px;
    padding:0; border:none; background:transparent; color:{neutral500}; border-radius:4px;
    cursor:pointer; transition:background-color .12s ease, color .12s ease; }}
.btn-icon:hover {{ background:{neutral100}; color:{neutral800}; }}
.btn-icon-danger:hover {{ background:{danger100}; color:{danger700}; }}
.btn-block {{ width:100%; }}

/* ============================ Modal ============================ */
.modal-overlay {{ position:fixed; inset:0; background:rgba(33,33,52,.45);
    display:flex; align-items:flex-start; justify-content:center; padding:64px 24px;
    z-index:100; animation:fc-fade .15s ease; }}
.modal-panel {{ background:#fff; border-radius:8px; max-width:100%; width:var(--modal-w,512px);
    max-height:80vh; overflow:auto; display:flex; flex-direction:column;
    box-shadow:0 8px 24px rgba(33,33,52,.25); animation:fc-pop .16s ease; }}

/* ============================ Toast ============================ */
.toast {{ display:flex; align-items:center; gap:12px; padding:12px 16px; border-radius:4px;
    color:#fff; box-shadow:0 4px 12px rgba(33,33,52,.25); min-width:260px; font-size:14px;
    animation:fc-slide .18s ease; }}

/* ============================ Nav item ============================ */
.nav-item {{ display:flex; align-items:center; gap:10px; width:100%; height:40px;
    padding:0 16px; border:none; background:transparent; color:{neutral700};
    font-size:14px; font-weight:500; border-radius:4px; cursor:pointer; text-align:left;
    transition:background-color .1s ease, color .1s ease; }}
.nav-item:hover {{ background:{neutral100}; color:{primary600}; }}
.nav-item-active {{ background:{primary100}; color:{primary700}; font-weight:600; }}
.nav-item svg {{ flex-shrink:0; }}

/* ============================ Misc ============================ */
.skeleton {{ display:block; border-radius:4px; background:{neutral150}; position:relative;
    overflow:hidden; }}
.skeleton::after {{ content:""; position:absolute; inset:0; transform:translateX(-100%);
    background:linear-gradient(90deg, transparent, rgba(255,255,255,.6), transparent);
    animation:fc-shimmer 1.4s infinite; }}
.empty-state {{ display:flex; flex-direction:column; align-items:center; justify-content:center;
    gap:12px; padding:48px; text-align:center; color:{neutral500}; }}
.link {{ color:{primary600}; cursor:pointer; background:none; border:none; font-size:14px;
    font-weight:600; padding:0; text-decoration:none; }}
.link:hover {{ text-decoration:underline; }}

/* ============================ Keyframes ============================ */
@keyframes fc-spin {{ to {{ transform:rotate(360deg); }} }}
@keyframes fc-fade {{ from {{ opacity:0; }} to {{ opacity:1; }} }}
@keyframes fc-pop {{ from {{ opacity:0; transform:translateY(8px) scale(.99); }} to {{ opacity:1; transform:none; }} }}
@keyframes fc-slide {{ from {{ opacity:0; transform:translateX(16px); }} to {{ opacity:1; transform:none; }} }}
@keyframes fc-shimmer {{ 100% {{ transform:translateX(100%); }} }}
"#,
        primary600 = color::PRIMARY_600,
        primary700 = color::PRIMARY_700,
        primary800 = color::PRIMARY_800,
        primary100 = color::PRIMARY_100,
        primary200 = color::PRIMARY_200,
        neutral0 = color::NEUTRAL_0,
        neutral100 = color::NEUTRAL_100,
        neutral150 = color::NEUTRAL_150,
        neutral200 = color::NEUTRAL_200,
        neutral300 = color::NEUTRAL_300,
        neutral500 = color::NEUTRAL_500,
        neutral600 = color::NEUTRAL_600,
        neutral700 = color::NEUTRAL_700,
        neutral800 = color::NEUTRAL_800,
        neutral900 = color::NEUTRAL_900,
        neutral50 = color::NEUTRAL_50,
        success100 = color::SUCCESS_100,
        warning100 = color::WARNING_100,
        warning600 = color::WARNING_600,
        warning700 = color::WARNING_700,
        secondary100 = color::SECONDARY_100,
        secondary600 = color::SECONDARY_600,
        secondary700 = color::SECONDARY_700,
        radius_md = RADIUS_MD,
        shadow_card = ui::design::tokens::SHADOW_CARD,
        danger600 = color::DANGER_600,
        danger700 = color::DANGER_700,
        danger100 = color::DANGER_100,
        success600 = color::SUCCESS_600,
        success700 = color::SUCCESS_700,
    )
}

// ---- color helpers ---------------------------------------------------------

pub fn primary_600() -> &'static str {
    color::PRIMARY_600
}
pub fn neutral_100() -> &'static str {
    color::NEUTRAL_100
}

// ---- common style snippets -------------------------------------------------

/// Reset for the full-app container.
pub fn app_shell() -> &'static str {
    "font-family: Inter, system-ui, -apple-system, sans-serif; color:#32324D;"
}

/// A flex container flowing vertically.
pub fn col(gap: u32) -> String {
    format!("display:flex; flex-direction:column; gap:{gap}px;")
}

/// A flex container flowing horizontally.
pub fn row(gap: u32, align: &str) -> String {
    format!("display:flex; flex-direction:row; gap:{gap}px; align-items:{align};")
}

/// A centered card used by login/register.
pub fn card(width: u32, padding: u32) -> String {
    format!(
        "background:#FFFFFF; border:1px solid {}; border-radius:{RADIUS_MD}px; padding:{padding}px; width:{width}px; box-shadow:0 1px 4px rgba(33,33,52,0.08);",
        color::NEUTRAL_150,
    )
}

/// Standard page padding.
pub fn page_padding() -> &'static str {
    "padding:32px;"
}
