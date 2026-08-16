//! SVG icon component backed by the `ui` crate's icon catalog.

use dioxus::prelude::*;
use ui::design::icons::Icon as UiIcon;

/// Render an icon by name (a `ui::design::icons::Icon` variant name or the
/// kebab-case string form). Falls back to a neutral placeholder.
#[component]
pub fn Icon(
    #[props(default)] name: String,
    #[props(default)] size: u32,
    #[props(default)] color: String,
) -> Element {
    let path = resolve_path(&name).to_string();
    let stroke = if color.is_empty() {
        "currentColor".to_string()
    } else {
        color
    };
    rsx! {
        svg {
            width: "{size}",
            height: "{size}",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "{stroke}",
            stroke_width: "2",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            path { d: "{path}" }
        }
    }
}

fn resolve_path(name: &str) -> &'static str {
    let icon = match name {
        "plus" => UiIcon::Plus,
        "pencil" => UiIcon::Pencil,
        "trash" => UiIcon::Trash,
        "chevron_down" => UiIcon::ChevronDown,
        "chevron_right" => UiIcon::ChevronRight,
        "chevron_left" => UiIcon::ChevronLeft,
        "search" => UiIcon::Search,
        "close" => UiIcon::Close,
        "check" => UiIcon::Check,
        "cog" => UiIcon::Cog,
        "grid" => UiIcon::Grid,
        "stack" => UiIcon::Stack,
        "image" => UiIcon::Image,
        "users" => UiIcon::Users,
        "shield" => UiIcon::Shield,
        "globe" => UiIcon::Globe,
        "key" => UiIcon::Key,
        "link" => UiIcon::Link,
        "puzzle" => UiIcon::Puzzle,
        "layers" => UiIcon::Layers,
        "text" => UiIcon::Text,
        "hash" => UiIcon::Hash,
        "calendar" => UiIcon::Calendar,
        "toggle" => UiIcon::Toggle,
        "braces" => UiIcon::Braces,
        "envelope" => UiIcon::Envelope,
        "lock" => UiIcon::Lock,
        "list" => UiIcon::List,
        "tag" => UiIcon::Tag,
        "file" => UiIcon::File,
        "external_link" => UiIcon::ExternalLink,
        "filter" => UiIcon::Filter,
        "sort" => UiIcon::Sort,
        "more_vertical" => UiIcon::MoreVertical,
        "eye" => UiIcon::Eye,
        "eye_off" => UiIcon::EyeOff,
        "arrow_left" => UiIcon::ArrowLeft,
        "refresh" => UiIcon::Refresh,
        "warning" => UiIcon::WarningTriangle,
        "info" => UiIcon::InfoCircle,
        "check_circle" => UiIcon::CheckCircle,
        "x_circle" => UiIcon::XCircle,
        _ => UiIcon::Plus,
    };
    icon.svg_path()
}
