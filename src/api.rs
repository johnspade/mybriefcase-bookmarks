use automerge_repo::DocHandle;
use autosurgeon::hydrate;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::export::export_netscape_html;
use crate::history;
use crate::model::BookmarkStore;
use crate::ops;
use crate::state::AppState;

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

fn read_store(doc_handle: &DocHandle) -> Result<BookmarkStore, StatusCode> {
    doc_handle.with_doc(|doc| hydrate(doc).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR))
}

/// # Errors
/// Returns `500 Internal Server Error` if the document cannot be read.
pub async fn get_tree(
    State(state): State<Arc<AppState>>,
) -> Result<Json<TreeResponse>, StatusCode> {
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
) -> Result<Json<FolderResponse>, StatusCode> {
    let store = read_store(&state.doc_handle)?;
    let folder = store.folders.get(&id).ok_or(StatusCode::NOT_FOUND)?;
    if folder.deleted {
        return Err(StatusCode::NOT_FOUND);
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
) -> Result<(StatusCode, Json<serde_json::Value>), StatusCode> {
    let id = state
        .mutate(|doc| ops::create_folder(doc, &req.parent_folder_id, &req.title))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::CREATED, Json(serde_json::json!({ "id": id }))))
}

/// # Errors
/// Returns `500 Internal Server Error` if the bookmark cannot be created.
pub async fn create_bookmark(
    State(state): State<Arc<AppState>>,
    Path(folder_id): Path<String>,
    Json(req): Json<CreateBookmarkRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), StatusCode> {
    let id = state
        .mutate(|doc| ops::add_bookmark(doc, &folder_id, &req.url, &req.title))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::CREATED, Json(serde_json::json!({ "id": id }))))
}

/// # Errors
/// Returns `500 Internal Server Error` if the bookmark cannot be updated.
pub async fn update_bookmark(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateBookmarkRequest>,
) -> Result<StatusCode, StatusCode> {
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
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

/// # Errors
/// Returns `500 Internal Server Error` if the bookmark cannot be deleted.
pub async fn delete_bookmark(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .mutate(|doc| ops::delete_bookmark(doc, &id))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

/// # Errors
/// Returns `500 Internal Server Error` if the item cannot be moved.
pub async fn move_item(
    State(state): State<Arc<AppState>>,
    Json(req): Json<MoveRequest>,
) -> Result<StatusCode, StatusCode> {
    state
        .mutate(|doc| ops::move_item(doc, &req.item, &req.source, &req.destination))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
) -> Result<Json<Vec<history::HistoryEntry>>, StatusCode> {
    let store = read_store(&state.doc_handle)?;
    if !store.bookmarks.contains_key(&id) {
        return Err(StatusCode::NOT_FOUND);
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
) -> Result<Json<history::BookmarkSnapshot>, StatusCode> {
    let store = read_store(&state.doc_handle)?;
    if !store.bookmarks.contains_key(&id) {
        return Err(StatusCode::NOT_FOUND);
    }
    let hash = history::parse_change_hash(&hash_hex).ok_or(StatusCode::BAD_REQUEST)?;
    history::bookmark_at_hash(&state.doc_handle, &id, &hash)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

/// # Errors
/// Returns `400 Bad Request` if the hash is invalid.
/// Returns `404 Not Found` if the bookmark does not exist.
/// Returns `500 Internal Server Error` if the revert fails.
pub async fn revert_bookmark(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<RevertBookmarkRequest>,
) -> Result<StatusCode, StatusCode> {
    let store = read_store(&state.doc_handle)?;
    if !store.bookmarks.contains_key(&id) {
        return Err(StatusCode::NOT_FOUND);
    }
    let hash = history::parse_change_hash(&req.target_hash).ok_or(StatusCode::BAD_REQUEST)?;
    state
        .mutate(|doc| ops::revert_bookmark(doc, &id, &hash))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

/// # Errors
/// Returns `500 Internal Server Error` if the document cannot be read or exported.
pub async fn export_bookmarks(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, StatusCode> {
    let store = read_store(&state.doc_handle)?;
    let mut buf = Vec::new();
    export_netscape_html(&store, &mut buf).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((
        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
        buf,
    ))
}
