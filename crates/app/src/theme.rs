//! Design-token driven inline styles (mirrors `ui::design::tokens`).

use ui::design::tokens::{RADIUS_MD, color, typography};

/// The global font family shared by every widget.
pub const FONT: &str = typography::FONT_FAMILY;

/// Render the design-token CSS custom properties as a `<style>` block.
pub fn token_styles() -> String {
    ui::design::tokens::css_variables()
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
