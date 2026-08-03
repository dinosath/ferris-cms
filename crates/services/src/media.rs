//! Media service (design Part V §7).
//!
//! Handles upload, thumbnails, folder CRUD, and the media picker.
//! Phase 3: basic file storage; advanced image pipeline is [LATER].

use crate::{AppContext, ServiceError};

/// Placeholder: media service will be implemented in Phase 3-4.
#[derive(Clone, Debug, serde::Serialize)]
pub struct MediaFile {
    pub id: i64,
    pub name: String,
    pub url: String,
    pub mime: String,
    pub size: i64,
}

/// List all media files.
pub async fn media_list(_ctx: &AppContext) -> Result<Vec<MediaFile>, ServiceError> {
    // TODO: implement with upload_file entity
    Ok(vec![])
}

/// Upload a file (multipart).
pub async fn media_upload(
    _ctx: &AppContext,
    _filename: &str,
    _mime: &str,
    _data: Vec<u8>,
) -> Result<MediaFile, ServiceError> {
    Err(ServiceError::internal("media upload not yet implemented"))
}
