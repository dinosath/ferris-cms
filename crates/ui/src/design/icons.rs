//! Icon catalog (design Part VI §7).
//!
//! Icons are referenced by name throughout the UI. Each icon maps to an
//! SVG path or a unicode glyph. The renderer resolves names to actual
//! rendering.

/// Icon identifier used in widget props.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    Plus,
    Pencil,
    Trash,
    DragHandle,
    ChevronDown,
    ChevronRight,
    ChevronLeft,
    Search,
    Close,
    Check,
    Cog,
    Grid,
    Stack,
    Image,
    Users,
    Shield,
    Globe,
    Key,
    Link,
    Puzzle,
    Layers,
    Text,
    Hash,
    Calendar,
    Toggle,
    Braces,
    Envelope,
    Lock,
    List,
    Tag,
    File,
    ExternalLink,
    Filter,
    Sort,
    MoreVertical,
    Eye,
    EyeOff,
    ArrowLeft,
    Refresh,
    WarningTriangle,
    InfoCircle,
    CheckCircle,
    XCircle,
    /// Custom: 6-dot grid for drag handles.
    DragDots,
    /// Kebab menu (three dots vertical).
    Kebab,
}

impl Icon {
    /// Human-readable label used as aria-label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Plus => "Add",
            Self::Pencil => "Edit",
            Self::Trash => "Delete",
            Self::DragHandle => "Drag to reorder",
            Self::ChevronDown => "Expand",
            Self::ChevronRight => "Collapse",
            Self::ChevronLeft => "Back",
            Self::Search => "Search",
            Self::Close => "Close",
            Self::Check => "Confirm",
            Self::Cog => "Settings",
            Self::Grid => "Grid view",
            Self::Stack => "List view",
            Self::Image => "Media",
            Self::Users => "Users",
            Self::Shield => "Security",
            Self::Globe => "Internationalization",
            Self::Key => "API Token",
            Self::Link => "Link",
            Self::Puzzle => "Plugin",
            Self::Layers => "Layers",
            Self::Text => "Text",
            Self::Hash => "Number",
            Self::Calendar => "Date",
            Self::Toggle => "Boolean",
            Self::Braces => "JSON",
            Self::Envelope => "Email",
            Self::Lock => "Password",
            Self::List => "Enumeration",
            Self::Tag => "Tag",
            Self::File => "File",
            Self::ExternalLink => "External link",
            Self::Filter => "Filter",
            Self::Sort => "Sort",
            Self::MoreVertical => "More",
            Self::Eye => "Show",
            Self::EyeOff => "Hide",
            Self::ArrowLeft => "Go back",
            Self::Refresh => "Refresh",
            Self::WarningTriangle => "Warning",
            Self::InfoCircle => "Information",
            Self::CheckCircle => "Success",
            Self::XCircle => "Error",
            Self::DragDots => "Reorder",
            Self::Kebab => "More actions",
        }
    }

    /// SVG path data (24×24 viewBox, stroke-based line icons).
    pub fn svg_path(&self) -> &'static str {
        match self {
            Self::Plus => "M12 5v14m-7-7h14",
            Self::Pencil => "M15.232 5.232l3.536 3.536m-2.036-5.036a2.5 2.5 0 113.536 3.536L6.5 21.036H3v-3.572L16.732 3.732z",
            Self::Trash => "M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16",
            Self::DragHandle => "M8 9h8M8 13h8",
            Self::ChevronDown => "M6 9l6 6 6-6",
            Self::ChevronRight => "M9 18l6-6-6-6",
            Self::ChevronLeft => "M15 18l-6-6 6-6",
            Self::Search => "M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z",
            Self::Close => "M6 6l12 12M6 18L18 6",
            Self::Check => "M5 13l4 4L19 7",
            Self::Cog => "M12 15a3 3 0 100-6 3 3 0 000 6zm-1-12.036a1 1 0 012 0v1.196a7.002 7.002 0 012.523 1.053l.85-.85a1 1 0 011.414 0l1.414 1.414a1 1 0 010 1.414l-.85.85A7.002 7.002 0 0118.404 11h1.196a1 1 0 010 2h-1.196a7.002 7.002 0 01-1.053 2.523l.85.85a1 1 0 010 1.414l-1.414 1.414a1 1 0 01-1.414 0l-.85-.85A7.002 7.002 0 0112 18.404v1.196a1 1 0 01-2 0v-1.196a7.002 7.002 0 01-2.523-1.053l-.85.85a1 1 0 01-1.414 0l-1.414-1.414a1 1 0 010-1.414l.85-.85A7.002 7.002 0 015.596 13H4.4a1 1 0 010-2h1.196a7.002 7.002 0 011.053-2.523l-.85-.85a1 1 0 010-1.414l1.414-1.414a1 1 0 011.414 0l.85.85A7.002 7.002 0 0111 4.16V2.964z",
            Self::Grid => "M4 6a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2H6a2 2 0 01-2-2V6zm10 0a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2h-2a2 2 0 01-2-2V6zM4 16a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2H6a2 2 0 01-2-2v-2zm10 0a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2h-2a2 2 0 01-2-2v-2z",
            Self::Stack => "M4 6h16M4 10h16M4 14h16M4 18h16",
            Self::Image => "M4 16l4.586-4.586a2 2 0 012.828 0L16 16m-2-2l1.586-1.586a2 2 0 012.828 0L20 14m-6-6h.01M6 20h12a2 2 0 002-2V6a2 2 0 00-2-2H6a2 2 0 00-2 2v12a2 2 0 002 2z",
            Self::Users => "M12 4.354a4 4 0 110 5.292M15 21H3v-1a6 6 0 0112 0v1zm0 0h6v-1a6 6 0 00-9-5.197M13 7a4 4 0 11-8 0 4 4 0 018 0z",
            Self::Shield => "M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z",
            Self::Globe => "M3.055 11H5a2 2 0 012 2v1a2 2 0 002 2 2 2 0 012 2v2.945M8 3.935V5.5A2.5 2.5 0 0010.5 8h.5a2 2 0 012 2 2 2 0 104 0 2 2 0 012-2h1.064M15 20.488V18a2 2 0 012-2h3.064M21 12a9 9 0 11-18 0 9 9 0 0118 0z",
            Self::Key => "M15 7a2 2 0 012 2m4 0a6 6 0 01-7.743 5.743L11 17H9v2H7v2H4a1 1 0 01-1-1v-2.586a1 1 0 01.293-.707l5.964-5.964A6 6 0 1121 9z",
            Self::Link => "M13.828 10.172a4 4 0 00-5.656 0l-4 4a4 4 0 105.656 5.656l1.102-1.101m-.758-4.899a4 4 0 005.656 0l4-4a4 4 0 00-5.656-5.656l-1.1 1.1",
            _ => "M4 6h16M4 12h16M4 18h16", // fallback: three horizontal lines
        }
    }

    /// The icon that represents each field type in the field picker.
    pub fn for_field_type(ft: core_domain::FieldType) -> Self {
        use core_domain::FieldType::*;
        match ft {
            String | Text => Self::Text,
            Richtext => Self::File,
            Blocks => Self::Layers,
            Integer | Biginteger | Decimal | Float => Self::Hash,
            Date | Datetime | Time => Self::Calendar,
            Boolean => Self::Toggle,
            Email => Self::Envelope,
            Password => Self::Lock,
            Enumeration => Self::List,
            Json => Self::Braces,
            Uid => Self::Tag,
            Media => Self::Image,
            Relation => Self::Link,
            Component => Self::Puzzle,
            Dynamiczone => Self::Grid,
        }
    }
}
