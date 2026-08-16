//! ferriscms UI crate — Dioxus design system + screens (design Part VI–VII).
//!
//! Module tree:
//! - `design/`   — tokens (colors, spacing, typography), icons
//! - `widgets/`  — base components (buttons, fields, cards, badges, tables, modals)
//! - `screens/`  — full-page screens (login, shell, CTB, CM)

pub mod design;
pub mod screens;
pub mod widgets;

// Re-export commonly used items.
pub use design::icons::Icon;
pub use design::tokens;
