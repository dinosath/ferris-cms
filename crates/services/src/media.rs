//! Media service (design Part V §7).
//!
//! Stores uploaded files on disk under `AppConfig::media_storage_dir` and
//! records their metadata in the `upload_file` system table. Serves listing
//! for the Media Library UI and picker.

use crate::{AppContext, ServiceError};
use db::entities::upload_file;
use sea_orm::{ActiveModelTrait, EntityTrait, Set};
use std::path::PathBuf;

/// A media file record returned to the UI/API.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaFile {
    pub id: i64,
    pub name: String,
    pub alternative_text: Option<String>,
    pub caption: Option<String>,
    pub hash: String,
    pub ext: Option<String>,
    pub mime: String,
    pub size: i64,
    pub url: String,
    pub provider: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<upload_file::Model> for MediaFile {
    fn from(f: upload_file::Model) -> Self {
        Self {
            id: f.id,
            name: f.name,
            alternative_text: f.alternative_text,
            caption: f.caption,
            hash: f.hash,
            ext: f.ext,
            mime: f.mime,
            size: f.size as i64,
            url: f.url,
            provider: f.provider,
            created_at: f.created_at,
        }
    }
}

/// List all media files (most recent first).
pub async fn media_list(ctx: &AppContext) -> Result<Vec<MediaFile>, ServiceError> {
    use sea_orm::QueryOrder;

    let files = upload_file::Entity::find()
        .order_by_desc(upload_file::COLUMN.created_at)
        .all(&ctx.db)
        .await?;
    Ok(files.into_iter().map(MediaFile::from).collect())
}

/// Compute the storage path + public URL for a hash + ext.
fn storage_paths(ctx: &AppContext, hash: &str, ext: &str) -> (PathBuf, String) {
    let dir = PathBuf::from(&ctx.config.media_storage_dir);
    let rel = format!("{hash}{ext}");
    (dir.join(&rel), format!("/uploads/{rel}"))
}

/// Upload a file to disk and record its metadata in `upload_file`.
pub async fn media_upload(
    ctx: &AppContext,
    filename: &str,
    mime: &str,
    data: &[u8],
) -> Result<MediaFile, ServiceError> {
    if data.is_empty() {
        return Err(ServiceError::validation(
            "upload",
            vec![crate::ValidationErrorItem::new(
                vec!["file".into()],
                "cannot upload an empty file",
                "ValidationError",
            )],
        ));
    }

    // Generate a unique hash-based filename, keep the extension from the mime
    // or the original name.
    let hash: String = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(data);
        let digest = hasher.finalize();
        hex(&digest[..12])
    };

    let ext = infer_ext(mime, filename);
    let (path, url) = storage_paths(ctx, &hash, &ext);

    // Ensure the storage directory exists.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ServiceError::internal(format!("media dir: {e}")))?;
    }
    std::fs::write(&path, data).map_err(|e| ServiceError::internal(format!("media write: {e}")))?;

    let now = chrono::Utc::now();
    let document_id = uuid::Uuid::new_v4().to_string();
    let size = data.len() as i64;

    let model = upload_file::ActiveModel {
        document_id: Set(document_id),
        name: Set(filename.to_string()),
        alternative_text: Set(None),
        caption: Set(None),
        width: Set(None),
        height: Set(None),
        formats_json: Set(None),
        hash: Set(hash.clone()),
        ext: Set(if ext.is_empty() {
            None
        } else {
            Some(ext.clone())
        }),
        mime: Set(mime.to_string()),
        size: Set(size as f64),
        url: Set(url.clone()),
        preview_url: Set(None),
        provider: Set("local".to_string()),
        folder_id: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
        sync_version: Set(0),
        origin_node_id: Set(None),
        deleted_at: Set(None),
        ..Default::default()
    };
    let row = model.insert(&ctx.db).await?;

    let file = MediaFile::from(row);
    let _ = crate::workflow::triggers::dispatch_media_event(
        ctx,
        serde_json::json!({ "id": file.id, "name": file.name, "mime": file.mime, "url": file.url, "size": file.size }),
    )
    .await;
    Ok(file)
}

/// Compact lowercase hex of a byte slice (no external dep needed).
fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// Best-effort extension from the mime type, falling back to the filename.
fn infer_ext(mime: &str, filename: &str) -> String {
    let from_mime = match mime {
        "image/png" => Some(".png"),
        "image/jpeg" => Some(".jpg"),
        "image/gif" => Some(".gif"),
        "image/webp" => Some(".webp"),
        "image/svg+xml" => Some(".svg"),
        "application/pdf" => Some(".pdf"),
        "text/plain" => Some(".txt"),
        "application/json" => Some(".json"),
        "application/zip" => Some(".zip"),
        "video/mp4" => Some(".mp4"),
        "audio/mpeg" => Some(".mp3"),
        _ => None,
    };
    if let Some(m) = from_mime {
        return m.to_string();
    }
    // Fall back to the original filename's extension.
    filename
        .rsplit_once('.')
        .map(|(_, e)| format!(".{}", e.to_lowercase()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_extensions() {
        assert_eq!(infer_ext("image/png", "a.png"), ".png");
        assert_eq!(infer_ext("application/pdf", "doc"), ".pdf");
        assert_eq!(infer_ext("text/plain", "notes.TXT"), ".txt");
        // Unknown mime falls back to the filename extension (lowercased).
        assert_eq!(infer_ext("application/octet-stream", "data.BIN"), ".bin");
        assert_eq!(infer_ext("application/octet-stream", "noext"), "");
    }

    #[test]
    fn hex_lowercases() {
        assert_eq!(hex(&[0xAB, 0x0f]), "ab0f");
    }
}

#[cfg(test)]
mod send_tests {
    use sea_orm_migration::MigratorTrait;
    use tokio::io::AsyncWriteExt;

    fn require_send<T: Send>(_: &T) {}
    fn require_sync<T: Sync>(_: &T) {}

    #[tokio::test]
    async fn media_probe() {
        let db = db::connect_sqlite_memory().await.unwrap();
        db::migration::Migrator::up(&db, None).await.unwrap();
        let ctx = crate::AppContext::new(db, crate::AppConfig::default());
        tokio::spawn(async move {
            let _ = crate::media::media_upload(&ctx, "a", "text/plain", b"x").await;
        });
    }

    #[tokio::test]
    async fn app_context_is_send_sync() {
        let db = db::connect_sqlite_memory().await.unwrap();
        db::migration::Migrator::up(&db, None).await.unwrap();
        let ctx = crate::AppContext::new(db, crate::AppConfig::default());
        require_send(&ctx);
        require_sync(&ctx);
        require_send(&ctx.schema_cache);
        require_sync(&ctx.schema_cache);
    }

}
