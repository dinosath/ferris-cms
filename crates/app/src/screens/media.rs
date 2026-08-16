//! Media Library screen (design doc §7).
//!
//! Lists uploaded assets with their metadata and an upload control that
//! POSTs a file as multipart to `/admin/upload/files`. This is a functional
//! media library backed by the local-file storage service.

use dioxus::prelude::*;
use ui::design::tokens::{color, typography};

use crate::app::use_global;
use crate::components::{Button, Card, EmptyState};

#[component]
pub fn MediaLibrary() -> Element {
    let global = use_global();
    let mut files = use_signal(Vec::<serde_json::Value>::new);
    let mut loaded = use_signal(|| false);
    let mut status = use_signal(|| None::<String>);

    let g_load = global.clone();
    use_effect(move || {
        if !loaded() {
            loaded.set(true);
            let g = g_load.clone();
            spawn(async move {
                match g.client.media_list().await {
                    Ok(v) => {
                        files.set(
                            v.get("data")
                                .and_then(|d| d.as_array())
                                .cloned()
                                .unwrap_or_default(),
                        );
                    }
                    Err(e) => status.set(Some(format!("Failed to load media: {e}"))),
                }
            });
        }
    });

    let title_style = format!(
        "font-size:{}; font-weight:600; color:{};",
        typography::DELTA_SIZE,
        color::NEUTRAL_900
    );
    let count_style = format!(
        "font-size:{}; color:{};",
        typography::BODY_SIZE,
        color::NEUTRAL_500
    );
    let top_bar = format!("display:flex; align-items:center; justify-content:space-between; padding:0 32px; height:56px; border-bottom:1px solid {}; background:{};", color::NEUTRAL_150, color::NEUTRAL_0);
    let status_style = format!("padding:12px; margin-bottom:16px; border-radius:4px; background:{}; color:{}; font-size:{};", color::WARNING_100, color::WARNING_700, typography::BODY_SIZE);
    let count = files().len();

    rsx! {
        div { style: "flex:1; min-width:0;",
            div { style: "{top_bar}",
                div { style: "display:flex; align-items:baseline; gap:12px;",
                    span { style: "{title_style}", "Media Library" }
                    span { style: "{count_style}", "({count} assets)" }
                }
                UploadButton { on_uploaded: move |f| {
                    files.write().push(f);
                } }
            }
            div { style: "padding:32px;",
                if let Some(status) = status() {
                    div { style: "{status_style}", "{status}" }
                }
                if files().is_empty() {
                    Card { padding: 0,
                        EmptyState {
                            title: "No media yet".to_string(),
                            subtitle: "Upload your first asset to get started.".to_string(),
                            icon: "image".to_string(),
                        }
                    }
                } else {
                    div { style: "display:grid; grid-template-columns:repeat(auto-fill, minmax(180px, 1fr)); gap:16px;",
                        for f in files() {
                            AssetCard { asset: f }
                        }
                    }
                }
            }
        }
    }
}

/// A single asset card in the media grid.
#[component]
fn AssetCard(asset: serde_json::Value) -> Element {
    let name = asset
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();
    let mime = asset
        .get("mime")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();
    let url = asset
        .get("url")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();
    let size = asset.get("size").and_then(|n| n.as_f64()).unwrap_or(0.0);
    rsx! {
        div { style: "border:1px solid {color::NEUTRAL_150}; border-radius:4px; background:#fff; overflow:hidden;",
            div { style: "height:120px; background:{color::NEUTRAL_100}; display:flex; align-items:center; justify-content:center; color:{color::NEUTRAL_400}; font-size:36px;",
                if url.starts_with('/') { "{mime}" } else { "?" }
            }
            div { style: "padding:8px 12px;",
                div { style: "font-size:{typography::PI_SIZE}; font-weight:600; color:{color::NEUTRAL_800}; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;", "{name}" }
                div { style: "font-size:{typography::PI_SIZE}; color:{color::NEUTRAL_500};", "{mime}" }
                div { style: "font-size:{typography::PI_SIZE}; color:{color::NEUTRAL_500};", "{size:.1} KB" }
            }
        }
    }
}

/// A button that opens a file picker and uploads the selected file.
#[component]
fn UploadButton(on_uploaded: EventHandler<serde_json::Value>) -> Element {
    let global = use_global();
    rsx! {
        label { style: "cursor:pointer;",
            Button {
                label: "+ Add new assets".to_string(),
                variant: "primary".to_string(),
                on_click: move |_| {},
            }
            input {
                r#type: "file",
                style: "display:none;",
                onchange: move |e| {
                    let file = e.files().into_iter().next();
                    if let Some(file) = file {
                        let g = global.clone();
                        let name = file.name();
                        let mime = file.content_type().unwrap_or_else(|| "application/octet-stream".to_string());
                        spawn(async move {
                            match file.read_bytes().await {
                                Ok(bytes) => {
                                    let data: &[u8] = bytes.as_ref();
                                    match g.client.media_upload(&name, &mime, data).await {
                                        Ok(v) => {
                                            if let Some(f) = v.get("data").and_then(|d| d.as_array()).and_then(|a| a.first()) {
                                                on_uploaded.call(f.clone());
                                            }
                                        }
                                        Err(_) => {}
                                    }
                                }
                                Err(_) => {}
                            }
                        });
                    }
                },
            }
        }
    }
}
