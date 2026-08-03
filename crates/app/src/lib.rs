//! ferriscms admin UI — a multiplatform Dioxus application.
//!
//! Module tree:
//! - `app`          — root component, app-state context, manual routing
//! - `client`       — thin typed wrapper over `client-core`
//! - `theme`        — inline-style helpers built from design tokens
//! - `components`   — base Dioxus widgets (buttons, fields, cards, modals, ...)
//! - `screens`      — full-page screens (login, register, shell, home, CTB, CM)

pub mod app;
pub mod client;
pub mod theme;

pub mod components;
pub mod screens;
