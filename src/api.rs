use automerge_repo::DocHandle;
use autosurgeon::hydrate;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use mybriefcase_bookmarks_core::error::CoreError;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::export::export_netscape_html;
use crate::history;
use crate::model::BookmarkStore;
use crate::ops;
use crate::state::AppState;

// ─── API Error (RFC 9457 Problem Details) ──────────────

pub struct ApiError(pub CoreError);

#[derive(Serialize)]
struct ProblemDetails {
    r#type: String,
    title: String,
    status: u16,
    detail: String,
}

impl From<CoreError> for ApiError {
    fn from(e: CoreError) -> Self {
        Self(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, type_uri, title) = match &self.0 {
            CoreError::NotFound(_) => (
                StatusCode::NOT_FOUND,
                "urn:mybriefcase:error:not-found",
                "Not Found",
            ),
            CoreError::Validation(_) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "urn:mybriefcase:error:validation",
                "Validation Error",
            ),
            CoreError::DocumentCorrupted(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "urn:mybriefcase:error:document-corrupted",
                "Document Corrupted",
            ),
            CoreError::Automerge(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "urn:mybriefcase:error:automerge",
                "Storage Error",
            ),
            CoreError::Io(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "urn:mybriefcase:error:io",
                "I/O Error",
            ),
        };

        let body = ProblemDetails {
            r#type: type_uri.to_owned(),
            title: title.to_owned(),
            status: status.as_u16(),
            detail: self.0.to_string(),
        };

        let json_bytes = serde_json::to_vec(&body).unwrap_or_default();
        let mut response = (status, json_bytes).into_response();
        response.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            "application/problem+json".parse().unwrap(),
        );
        response
    }
}

// ─── Response / Request DTOs ───────────────────────────

#[derive(Serialize)]
pub struct FolderResponse {
    id: String,
    title: String,
    children: Vec<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize)]
pub struct BookmarkResponse {
    id: String,
    url: String,
    title: String,
    notes: String,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize)]
pub struct TreeResponse {
    root_folder_id: String,
    folders: Vec<FolderResponse>,
    bookmarks: Vec<BookmarkResponse>,
}

#[derive(Deserialize)]
pub struct CreateFolderRequest {
    parent_folder_id: String,
    title: String,
}

#[derive(Deserialize)]
pub struct CreateBookmarkRequest {
    url: String,
    title: String,
}

#[derive(Deserialize)]
pub struct UpdateBookmarkRequest {
    url: Option<String>,
    title: Option<String>,
    notes: Option<String>,
}

#[derive(Deserialize)]
pub struct MoveRequest {
    #[serde(rename = "item_id")]
    item: String,
    #[serde(rename = "from_folder_id")]
    source: String,
    #[serde(rename = "to_folder_id")]
    destination: String,
}

fn read_store(doc_handle: &DocHandle) -> Result<BookmarkStore, ApiError> {
    doc_handle.with_doc(|doc| {
        hydrate(doc).map_err(|e| ApiError(CoreError::DocumentCorrupted(e.to_string())))
    })
}

/// # Errors
/// Returns `500 Internal Server Error` if the document cannot be read.
pub async fn get_tree(State(state): State<Arc<AppState>>) -> Result<Json<TreeResponse>, ApiError> {
    let store = read_store(&state.doc_handle)?;
    let folders: Vec<FolderResponse> = store
        .folders
        .iter()
        .filter(|(_, f)| !f.deleted)
        .map(|(id, f)| FolderResponse {
            id: id.clone(),
            title: f.title.clone(),
            children: f.children.clone(),
            created_at: f.created_at.clone(),
            updated_at: f.updated_at.clone(),
        })
        .collect();
    let bookmarks: Vec<BookmarkResponse> = store
        .bookmarks
        .iter()
        .filter(|(_, b)| !b.deleted)
        .map(|(id, b)| BookmarkResponse {
            id: id.clone(),
            url: b.url.clone(),
            title: b.title.clone(),
            notes: b.notes.clone(),
            created_at: b.created_at.clone(),
            updated_at: b.updated_at.clone(),
        })
        .collect();
    Ok(Json(TreeResponse {
        root_folder_id: store.root_folder_id,
        folders,
        bookmarks,
    }))
}

/// # Errors
/// Returns `404 Not Found` if the folder does not exist or is deleted.
/// Returns `500 Internal Server Error` if the document cannot be read.
pub async fn get_folder(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<FolderResponse>, ApiError> {
    let store = read_store(&state.doc_handle)?;
    let folder = store
        .folders
        .get(&id)
        .ok_or_else(|| ApiError(CoreError::NotFound(format!("folder not found: {id}"))))?;
    if folder.deleted {
        return Err(ApiError(CoreError::NotFound(format!(
            "folder not found: {id}"
        ))));
    }
    Ok(Json(FolderResponse {
        id,
        title: folder.title.clone(),
        children: folder.children.clone(),
        created_at: folder.created_at.clone(),
        updated_at: folder.updated_at.clone(),
    }))
}

/// # Errors
/// Returns `500 Internal Server Error` if the folder cannot be created.
pub async fn create_folder(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateFolderRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let id = state
        .mutate(|doc| ops::create_folder(doc, &req.parent_folder_id, &req.title))
        .map_err(ApiError)?;
    Ok((StatusCode::CREATED, Json(serde_json::json!({ "id": id }))))
}

/// # Errors
/// Returns `500 Internal Server Error` if the bookmark cannot be created.
pub async fn create_bookmark(
    State(state): State<Arc<AppState>>,
    Path(folder_id): Path<String>,
    Json(req): Json<CreateBookmarkRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let id = state
        .mutate(|doc| ops::add_bookmark(doc, &folder_id, &req.url, &req.title))
        .map_err(ApiError)?;
    Ok((StatusCode::CREATED, Json(serde_json::json!({ "id": id }))))
}

/// # Errors
/// Returns `500 Internal Server Error` if the bookmark cannot be updated.
pub async fn update_bookmark(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateBookmarkRequest>,
) -> Result<StatusCode, ApiError> {
    state
        .mutate(|doc| {
            ops::update_bookmark(
                doc,
                &id,
                req.url.as_deref(),
                req.title.as_deref(),
                req.notes.as_deref(),
            )
        })
        .map_err(ApiError)?;
    Ok(StatusCode::OK)
}

/// # Errors
/// Returns `500 Internal Server Error` if the bookmark cannot be deleted.
pub async fn delete_bookmark(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .mutate(|doc| ops::delete_bookmark(doc, &id))
        .map_err(ApiError)?;
    Ok(StatusCode::NO_CONTENT)
}

/// # Errors
/// Returns `500 Internal Server Error` if the item cannot be moved.
pub async fn move_item(
    State(state): State<Arc<AppState>>,
    Json(req): Json<MoveRequest>,
) -> Result<StatusCode, ApiError> {
    state
        .mutate(|doc| ops::move_item(doc, &req.item, &req.source, &req.destination))
        .map_err(ApiError)?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
pub struct RevertBookmarkRequest {
    target_hash: String,
}

/// # Errors
/// Returns `404 Not Found` if the bookmark has no history.
/// Returns `500 Internal Server Error` if the document cannot be read.
pub async fn get_bookmark_history(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<history::HistoryEntry>>, ApiError> {
    let store = read_store(&state.doc_handle)?;
    if !store.bookmarks.contains_key(&id) {
        return Err(ApiError(CoreError::NotFound(format!(
            "bookmark not found: {id}"
        ))));
    }
    let entries = history::bookmark_history(&state.doc_handle, &id);
    Ok(Json(entries))
}

/// # Errors
/// Returns `404 Not Found` if the bookmark or hash does not exist.
/// Returns `500 Internal Server Error` if the document cannot be read.
pub async fn get_bookmark_at_hash(
    State(state): State<Arc<AppState>>,
    Path((id, hash_hex)): Path<(String, String)>,
) -> Result<Json<history::BookmarkSnapshot>, ApiError> {
    let store = read_store(&state.doc_handle)?;
    if !store.bookmarks.contains_key(&id) {
        return Err(ApiError(CoreError::NotFound(format!(
            "bookmark not found: {id}"
        ))));
    }
    let hash = history::parse_change_hash(&hash_hex)
        .ok_or_else(|| ApiError(CoreError::Validation(format!("invalid hash: {hash_hex}"))))?;
    history::bookmark_at_hash(&state.doc_handle, &id, &hash)
        .map(Json)
        .ok_or_else(|| {
            ApiError(CoreError::NotFound(format!(
                "no snapshot at hash: {hash_hex}"
            )))
        })
}

/// # Errors
/// Returns `400 Bad Request` if the hash is invalid.
/// Returns `404 Not Found` if the bookmark does not exist.
/// Returns `500 Internal Server Error` if the revert fails.
pub async fn revert_bookmark(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<RevertBookmarkRequest>,
) -> Result<StatusCode, ApiError> {
    let store = read_store(&state.doc_handle)?;
    if !store.bookmarks.contains_key(&id) {
        return Err(ApiError(CoreError::NotFound(format!(
            "bookmark not found: {id}"
        ))));
    }
    let hash = history::parse_change_hash(&req.target_hash).ok_or_else(|| {
        ApiError(CoreError::Validation(format!(
            "invalid hash: {}",
            req.target_hash
        )))
    })?;
    state
        .mutate(|doc| ops::revert_bookmark(doc, &id, &hash))
        .map_err(ApiError)?;
    Ok(StatusCode::OK)
}

/// # Errors
/// Returns `500 Internal Server Error` if the document cannot be read or exported.
pub async fn export_bookmarks(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, ApiError> {
    let store = read_store(&state.doc_handle)?;
    let mut buf = Vec::new();
    export_netscape_html(&store, &mut buf)
        .map_err(|e| ApiError(CoreError::Io(std::io::Error::other(e.to_string()))))?;
    Ok((
        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
        buf,
    ))
}
