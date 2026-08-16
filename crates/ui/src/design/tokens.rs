//! Design tokens (design Part VI §2–§5).
//!
//! Numeric, agent-buildable. Every colour, spacing, and typography token
//! is defined here; widget and screen code references these by name, never
//! by literal hex.

// ---------------------------------------------------------------------------
// Colour System — 100–900 scale (§2)
// ---------------------------------------------------------------------------

pub mod color {
    /// Primary – Violet (brand & interactive)
    pub const PRIMARY_100: &str = "#F0F0FF";
    pub const PRIMARY_200: &str = "#D9D8FF";
    pub const PRIMARY_300: &str = "#C9C7FF";
    pub const PRIMARY_400: &str = "#9B96FF";
    pub const PRIMARY_500: &str = "#7B79FF";
    pub const PRIMARY_600: &str = "#4945FF"; // button states, focus rings
    pub const PRIMARY_700: &str = "#271FE0";
    pub const PRIMARY_800: &str = "#1F15B0";
    pub const PRIMARY_900: &str = "#15088F";

    /// Neutrals (backgrounds, text, borders)
    pub const NEUTRAL_0: &str = "#FFFFFF"; // pure white, card backgrounds
    pub const NEUTRAL_50: &str = "#FAFAF9"; // subtle hover
    pub const NEUTRAL_100: &str = "#F6F6F9"; // app background
    pub const NEUTRAL_150: &str = "#EAEAEF"; // subtle dividers
    pub const NEUTRAL_200: &str = "#DCDCE4"; // input borders, disabled
    pub const NEUTRAL_300: &str = "#C0C0CF"; // secondary placeholder
    pub const NEUTRAL_400: &str = "#A5A5BA"; // placeholder text
    pub const NEUTRAL_500: &str = "#8E8EA9"; // secondary labels
    pub const NEUTRAL_600: &str = "#666687"; // secondary text
    pub const NEUTRAL_700: &str = "#4A4A6A"; // body text, copy
    pub const NEUTRAL_800: &str = "#32324D"; // headings, strong emphasis
    pub const NEUTRAL_900: &str = "#212134"; // highest contrast

    /// Semantic – Success (green)
    pub const SUCCESS_100: &str = "#EAFBE7";
    pub const SUCCESS_500: &str = "#31A856";
    pub const SUCCESS_600: &str = "#328048";
    pub const SUCCESS_700: &str = "#2F6846";

    /// Semantic – Warning (orange)
    pub const WARNING_100: &str = "#FDF4DC";
    pub const WARNING_500: &str = "#D19400";
    pub const WARNING_600: &str = "#BE5D01";
    pub const WARNING_700: &str = "#9B4C00";

    /// Semantic – Danger (red)
    pub const DANGER_100: &str = "#FCECEA";
    pub const DANGER_500: &str = "#EE5E52";
    pub const DANGER_600: &str = "#D02B20";
    pub const DANGER_700: &str = "#B72B1A";

    /// Semantic – Alternative (purple)
    pub const ALTERNATIVE_100: &str = "#F6ECFC";
    pub const ALTERNATIVE_600: &str = "#9736E8";

    /// Semantic – Secondary (blue)
    pub const SECONDARY_100: &str = "#EAF5FF";
    pub const SECONDARY_600: &str = "#0C75AF";
    pub const SECONDARY_700: &str = "#0A5F8A";

    /// Status badge colours
    pub fn badge_colors(kind: &str) -> (&'static str, &'static str) {
        match kind {
            "new" | "N" => (SECONDARY_600, SECONDARY_100),
            "modified" | "M" => (WARNING_600, WARNING_100),
            "deleted" | "D" => (DANGER_600, DANGER_100),
            "published" | "P" => (SUCCESS_600, SUCCESS_100),
            "draft" => (SECONDARY_600, SECONDARY_100),
            _ => (NEUTRAL_600, NEUTRAL_100),
        }
    }
}

// ---------------------------------------------------------------------------
// Typography — Inter font family, weights 400/500/600/700 (§3)
// ---------------------------------------------------------------------------

pub mod typography {
    pub const FONT_FAMILY: &str = "Inter, system-ui, -apple-system, sans-serif";

    /// ALPHA — 32px / weight 600
    pub const ALPHA_SIZE: &str = "32px";
    pub const ALPHA_WEIGHT: &str = "600";

    /// BETA — 24px / weight 600 (page title)
    pub const BETA_SIZE: &str = "24px";
    pub const BETA_WEIGHT: &str = "600";

    /// DELTA — 18px / weight 600
    pub const DELTA_SIZE: &str = "18px";
    pub const DELTA_WEIGHT: &str = "600";

    /// EPSILON — 16px / weight 600
    pub const EPSILON_SIZE: &str = "16px";
    pub const EPSILON_WEIGHT: &str = "600";

    /// BODY — 14px / weight 400
    pub const BODY_SIZE: &str = "14px";
    pub const BODY_WEIGHT: &str = "400";

    /// BODY_BOLD — 14px / weight 600
    pub const BODY_BOLD_SIZE: &str = "14px";
    pub const BODY_BOLD_WEIGHT: &str = "600";

    /// LABEL — 12px / weight 600
    pub const LABEL_SIZE: &str = "12px";
    pub const LABEL_WEIGHT: &str = "600";

    /// PI — 11px / weight 400
    pub const PI_SIZE: &str = "11px";
    pub const PI_WEIGHT: &str = "400";
}

// ---------------------------------------------------------------------------
// Spacing scale in px (§4)
// ---------------------------------------------------------------------------

pub mod spacing {
    pub const SP_1: u32 = 4;
    pub const SP_2: u32 = 8;
    pub const SP_3: u32 = 12;
    pub const SP_4: u32 = 16;
    pub const SP_5: u32 = 20;
    pub const SP_6: u32 = 24;
    pub const SP_7: u32 = 32;
    pub const SP_8: u32 = 40;
    pub const SP_9: u32 = 48;
    pub const SP_10: u32 = 56;

    pub const CARD_PADDING: u32 = 24;
    pub const INPUT_PADDING_H: u32 = 16;
    pub const INPUT_PADDING_V: u32 = 8;
    pub const FORM_ROW_GAP: u32 = 20;
    pub const PAGE_PADDING: u32 = 32;
}

// ---------------------------------------------------------------------------
// Radii, borders, shadows (§5)
// ---------------------------------------------------------------------------

pub const RADIUS_SM: u32 = 4;
pub const RADIUS_MD: u32 = 4;
pub const RADIUS_PILL: u32 = 999;

pub const BORDER_WIDTH: u32 = 1;

pub const SHADOW_CARD: &str = "0 1px 4px rgba(33, 33, 52, 0.08)";

// ---------------------------------------------------------------------------
// Layout constants (§8)
// ---------------------------------------------------------------------------

pub const SIDEBAR_WIDTH: u32 = 240;
pub const TOP_BAR_HEIGHT: u32 = 56;
pub const CTB_TOP_BAR_HEIGHT: u32 = 64;
pub const FORM_MAX_WIDTH: u32 = 900;
pub const MODAL_WIDTH_SM: u32 = 512;
pub const MODAL_WIDTH_MD: u32 = 640;
pub const MODAL_WIDTH_LG: u32 = 720;
pub const LOGIN_CARD_WIDTH: u32 = 552;

// ---------------------------------------------------------------------------
// Compiled CSS stylesheet helper
// ---------------------------------------------------------------------------

/// Build a `<style>` block with all design tokens as CSS custom properties.
pub fn css_variables() -> String {
    let mut s = String::from(":root {\n");
    // Primary
    s.push_str(&format!("  --color-primary-100: {};\n", color::PRIMARY_100));
    s.push_str(&format!("  --color-primary-200: {};\n", color::PRIMARY_200));
    s.push_str(&format!("  --color-primary-300: {};\n", color::PRIMARY_300));
    s.push_str(&format!("  --color-primary-400: {};\n", color::PRIMARY_400));
    s.push_str(&format!("  --color-primary-500: {};\n", color::PRIMARY_500));
    s.push_str(&format!("  --color-primary-600: {};\n", color::PRIMARY_600));
    s.push_str(&format!("  --color-primary-700: {};\n", color::PRIMARY_700));
    s.push_str(&format!("  --color-primary-800: {};\n", color::PRIMARY_800));
    s.push_str(&format!("  --color-primary-900: {};\n", color::PRIMARY_900));
    // Neutrals
    s.push_str(&format!("  --color-neutral-0: {};\n", color::NEUTRAL_0));
    s.push_str(&format!("  --color-neutral-50: {};\n", color::NEUTRAL_50));
    s.push_str(&format!("  --color-neutral-100: {};\n", color::NEUTRAL_100));
    s.push_str(&format!("  --color-neutral-150: {};\n", color::NEUTRAL_150));
    s.push_str(&format!("  --color-neutral-200: {};\n", color::NEUTRAL_200));
    s.push_str(&format!("  --color-neutral-300: {};\n", color::NEUTRAL_300));
    s.push_str(&format!("  --color-neutral-400: {};\n", color::NEUTRAL_400));
    s.push_str(&format!("  --color-neutral-500: {};\n", color::NEUTRAL_500));
    s.push_str(&format!("  --color-neutral-600: {};\n", color::NEUTRAL_600));
    s.push_str(&format!("  --color-neutral-700: {};\n", color::NEUTRAL_700));
    s.push_str(&format!("  --color-neutral-800: {};\n", color::NEUTRAL_800));
    s.push_str(&format!("  --color-neutral-900: {};\n", color::NEUTRAL_900));
    // Semantic
    s.push_str(&format!("  --color-success-100: {};\n", color::SUCCESS_100));
    s.push_str(&format!("  --color-success-500: {};\n", color::SUCCESS_500));
    s.push_str(&format!("  --color-success-600: {};\n", color::SUCCESS_600));
    s.push_str(&format!("  --color-success-700: {};\n", color::SUCCESS_700));
    s.push_str(&format!("  --color-warning-100: {};\n", color::WARNING_100));
    s.push_str(&format!("  --color-warning-500: {};\n", color::WARNING_500));
    s.push_str(&format!("  --color-warning-600: {};\n", color::WARNING_600));
    s.push_str(&format!("  --color-warning-700: {};\n", color::WARNING_700));
    s.push_str(&format!("  --color-danger-100: {};\n", color::DANGER_100));
    s.push_str(&format!("  --color-danger-500: {};\n", color::DANGER_500));
    s.push_str(&format!("  --color-danger-600: {};\n", color::DANGER_600));
    s.push_str(&format!("  --color-danger-700: {};\n", color::DANGER_700));
    // Typography
    s.push_str(&format!("  --font-family: {};\n", typography::FONT_FAMILY));
    s.push_str(&format!(
        "  --font-size-alpha: {};\n",
        typography::ALPHA_SIZE
    ));
    s.push_str(&format!("  --font-size-beta: {};\n", typography::BETA_SIZE));
    s.push_str(&format!(
        "  --font-size-delta: {};\n",
        typography::DELTA_SIZE
    ));
    s.push_str(&format!(
        "  --font-size-epsilon: {};\n",
        typography::EPSILON_SIZE
    ));
    s.push_str(&format!("  --font-size-body: {};\n", typography::BODY_SIZE));
    s.push_str(&format!(
        "  --font-size-label: {};\n",
        typography::LABEL_SIZE
    ));
    s.push_str(&format!("  --font-size-pi: {};\n", typography::PI_SIZE));
    // Spacing
    s.push_str(&format!("  --spacing-1: {}px;\n", spacing::SP_1));
    s.push_str(&format!("  --spacing-4: {}px;\n", spacing::SP_4));
    s.push_str(&format!("  --spacing-7: {}px;\n", spacing::SP_7));
    s.push_str(&format!("  --spacing-9: {}px;\n", spacing::SP_9));
    // Layout
    s.push_str(&format!("  --sidebar-width: {}px;\n", SIDEBAR_WIDTH));
    s.push_str(&format!("  --top-bar-height: {}px;\n", TOP_BAR_HEIGHT));
    s.push_str(&format!("  --radius-sm: {}px;\n", RADIUS_SM));
    s.push_str(&format!("  --radius-md: {}px;\n", RADIUS_MD));
    s.push_str("}\n");
    s
}
