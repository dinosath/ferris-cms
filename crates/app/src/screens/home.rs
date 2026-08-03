//! Home / dashboard screen (design doc §4).

use dioxus::prelude::*;
use ui::design::tokens::{color, typography};

use crate::app::{Route, use_global};
use crate::components::{Card, Icon};

#[component]
pub fn Home() -> Element {
    let page_title = format!("font-size:{}; font-weight:600; color:{};", typography::DELTA_SIZE, color::NEUTRAL_900);
    let card_title = format!("font-size:{}; font-weight:600; color:{};", typography::EPSILON_SIZE, color::NEUTRAL_900);
    let card_sub = format!("font-size:{}; color:{};", typography::BODY_SIZE, color::NEUTRAL_600);

    rsx! {
        div { style: "padding:32px;",
            div { style: "display:flex; align-items:center; justify-content:space-between; border-bottom:1px solid {color::NEUTRAL_150}; padding-bottom:16px;",
                span { style: "{page_title}", "Home" }
            }
            div { style: "padding-top:32px;",
                Card { padding: 24,
                    div { style: "display:flex; align-items:center; gap:12px;",
                        Icon { name: "check_circle".to_string(), size: 28, color: color::PRIMARY_600.to_string() }
                        div { style: "display:flex; flex-direction:column; gap:4px;",
                            span { style: "{card_title}", "Welcome 👋" }
                            span { style: "{card_sub}",
                                "Use ferriscms to define content types and manage your content."
                            }
                        }
                    }
                }
                div { style: "display:grid; grid-template-columns:repeat(auto-fill, minmax(240px, 1fr)); gap:20px; padding-top:24px;",
                    QuickLink { icon: "grid", title: "Content-Type Builder", subtitle: "Design your data architecture", route: Route::ContentTypeBuilder }
                    QuickLink { icon: "stack", title: "Content Manager", subtitle: "Create, edit and publish content", route: Route::ContentManager }
                    QuickLink { icon: "image", title: "Media Library", subtitle: "Manage your assets", route: Route::Media }
                    QuickLink { icon: "cog", title: "Settings", subtitle: "Roles, users and tokens", route: Route::Settings }
                }
            }
        }
    }
}

#[component]
fn QuickLink(icon: String, title: String, subtitle: String, route: Route) -> Element {
    let mut global = use_global();
    let target = route.clone();
    let title_style = format!("font-size:{}; font-weight:600; color:{};", typography::EPSILON_SIZE, color::NEUTRAL_900);
    let sub_style = format!("font-size:{}; color:{};", typography::BODY_SIZE, color::NEUTRAL_600);
    rsx! {
        button {
            style: "background:#fff; border:1px solid {color::NEUTRAL_150}; border-radius:4px; padding:24px; text-align:left; cursor:pointer; display:flex; flex-direction:column; gap:10px;",
            onclick: move |_| global.route.set(target.clone()),
            Icon { name: "{icon}", size: 24, color: color::PRIMARY_600.to_string() }
            span { style: "{title_style}", "{title}" }
            span { style: "{sub_style}", "{subtitle}" }
        }
    }
}
