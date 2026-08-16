//! App shell layout (design Part VII §3).
//!
//! `[Sidebar 240px][Main Fill]` on all authenticated screens.

/// Sidebar navigation item.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NavItem {
    pub icon: String,
    pub label: String,
    pub route: String,
    pub active: bool,
}

/// Global sidebar state.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct SidebarState {
    pub username: String,
    pub user_initials: String,
    pub active_route: String,
    pub search_query: String,
}

impl SidebarState {
    pub fn primary_nav(&self) -> Vec<NavItem> {
        vec![
            NavItem {
                icon: "stack".into(),
                label: "Content Manager".into(),
                route: "/content-manager".into(),
                active: self.active_route.starts_with("/content-manager"),
            },
            NavItem {
                icon: "grid".into(),
                label: "Content-Type Builder".into(),
                route: "/content-type-builder".into(),
                active: self.active_route.starts_with("/content-type-builder"),
            },
            NavItem {
                icon: "image".into(),
                label: "Media Library".into(),
                route: "/media".into(),
                active: self.active_route.starts_with("/media"),
            },
        ]
    }

    pub fn general_nav(&self) -> Vec<NavItem> {
        vec![NavItem {
            icon: "cog".into(),
            label: "Settings".into(),
            route: "/settings".into(),
            active: self.active_route.starts_with("/settings"),
        }]
    }
}

/// App shell layout — renders sidebar + main content area.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ShellLayout {
    pub sidebar: SidebarState,
}
