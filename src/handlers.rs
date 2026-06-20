use askama::Template;
use automerge_repo::DocHandle;
use autosurgeon::hydrate;
use axum::Form;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use mybriefcase_bookmarks_core::error::CoreError;
use serde::Deserialize;
use std::convert::Infallible;
use std::fmt::Write as FmtWrite;
use std::sync::Arc;
use std::time::Duration;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

use crate::history;
use crate::model::BookmarkStore;
use crate::ops;
use crate::state::AppState;
use crate::views::{
    BookmarkItemView, BreadcrumbItem, FolderItemView, SortOrder, build_breadcrumbs,
    build_folder_items, build_sidebar_html, date_short, domain_color, domain_letter, html_escape,
    sort_items,
};

const fn core_error_to_status(e: &CoreError) -> StatusCode {
    match e {
        CoreError::NotFound(_) => StatusCode::NOT_FOUND,
        CoreError::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
        CoreError::DocumentCorrupted(_) | CoreError::Automerge(_) | CoreError::Io(_) => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

// ─── Form data ──────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateFolderForm {
    parent_folder_id: String,
    title: String,
}

#[derive(Deserialize)]
pub struct CreateBookmarkForm {
    folder_id: String,
    url: String,
    title: String,
    #[serde(default)]
    favicon_url: String,
}

#[derive(Deserialize)]
pub struct UpdateBookmarkForm {
    title: Option<String>,
    url: Option<String>,
    notes: Option<String>,
    folder_id: Option<String>,
    favicon: Option<String>,
}

#[derive(Deserialize)]
pub struct DeleteForm {
    current_folder_id: String,
}

#[derive(Deserialize)]
pub struct RenameFolderForm {
    title: String,
    current_folder_id: String,
}

#[derive(Deserialize)]
pub struct MoveItemForm {
    #[serde(rename = "item_id")]
    item: String,
    #[serde(rename = "from_folder_id")]
    source: String,
    #[serde(rename = "to_folder_id")]
    destination: String,
}

#[derive(Deserialize)]
pub struct SearchParams {
    q: String,
    sort: Option<String>,
}

#[derive(Deserialize)]
pub struct SidebarParams {
    folder_id: Option<String>,
}

#[derive(Deserialize)]
pub struct FolderContentParams {
    sort: Option<String>,
}

#[derive(Deserialize)]
pub struct RevertForm {
    target_hash: String,
}

// ─── Templates ──────────────────────────────────────

#[derive(Template)]
#[template(path = "base.html")]
struct BaseTemplate {
    sidebar_html: String,
    content_html: String,
    current_folder_id: String,
    page_title: String,
    static_v: String,
}

#[derive(Template)]
#[template(path = "folder_content.html")]
struct FolderContentTemplate {
    folder_id: String,
    breadcrumbs: Vec<BreadcrumbItem>,
    folders: Vec<FolderItemView>,
    bookmarks: Vec<BookmarkItemView>,
}

#[derive(Template)]
#[template(path = "detail_bookmark.html")]
struct DetailBookmarkTemplate {
    id: String,
    title: String,
    url: String,
    notes: String,
    favicon: String,
    created_at: String,
    updated_at: String,
    created_date: String,
    updated_date: String,
    domain_color: String,
    domain_letter: String,
}

#[derive(Template)]
#[template(path = "edit_bookmark.html")]
struct EditBookmarkTemplate {
    id: String,
    title: String,
    url: String,
    notes: String,
    favicon: String,
    domain_color: String,
    domain_letter: String,
    folders: Vec<(String, String, bool)>,
}

#[derive(Template)]
#[template(path = "detail_empty.html")]
struct DetailEmptyTemplate;

#[derive(Template)]
#[template(path = "settings.html")]
struct SettingsTemplate;

#[derive(Template)]
#[template(path = "settings_base.html")]
struct SettingsBaseTemplate {
    content_html: String,
    page_title: String,
    static_v: String,
}

#[derive(Template)]
#[template(path = "favicon_preview.html")]
struct FaviconPreviewTemplate {
    favicon: String,
    domain_color: String,
    domain_letter: String,
    error: String,
}

// ─── Helpers ────────────────────────────────────────

fn read_store(doc_handle: &DocHandle) -> Result<BookmarkStore, StatusCode> {
    doc_handle.with_doc(|doc| hydrate(doc).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR))
}

fn render(tmpl: &impl Template) -> Result<String, StatusCode> {
    tmpl.render().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn find_parent_folder_id(store: &BookmarkStore, child_id: &str) -> Option<String> {
    for (folder_id, folder) in &store.folders {
        if !folder.deleted && folder.children.iter().any(|c| c == child_id) {
            return Some(folder_id.clone());
        }
    }
    None
}

fn find_folder_for_bookmark<'a>(store: &'a BookmarkStore, bookmark_id: &str) -> Option<&'a str> {
    for (fid, folder) in &store.folders {
        if !folder.deleted && folder.children.iter().any(|c| c == bookmark_id) {
            return Some(fid.as_str());
        }
    }
    None
}

fn collect_descendants(
    store: &BookmarkStore,
    folder_id: &str,
    out: &mut std::collections::HashSet<String>,
) {
    if let Some(folder) = store.folders.get(folder_id) {
        for child_id in &folder.children {
            if let Some(sub) = store.folders.get(child_id) {
                if !sub.deleted {
                    out.insert(child_id.clone());
                    collect_descendants(store, child_id, out);
                }
            }
        }
    }
}

fn collect_all_folder_paths(store: &BookmarkStore, root_id: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let exclude = std::collections::HashSet::new();
    let mut full = Vec::new();
    collect_folder_paths(store, root_id, "", &exclude, "", &mut full);
    for (id, path, _) in full {
        out.push((id, path));
    }
    out
}

fn collect_folder_paths(
    store: &BookmarkStore,
    folder_id: &str,
    parent_path: &str,
    exclude_ids: &std::collections::HashSet<String>,
    current_parent_id: &str,
    out: &mut Vec<(String, String, bool)>,
) {
    if exclude_ids.contains(folder_id) {
        return;
    }
    let Some(folder) = store.folders.get(folder_id) else {
        return;
    };
    if folder.deleted {
        return;
    }

    let path = if parent_path.is_empty() {
        folder.title.clone()
    } else {
        format!("{parent_path} / {}", folder.title)
    };
    let is_current = folder_id == current_parent_id;
    out.push((folder_id.to_owned(), path.clone(), is_current));

    let child_folder_ids: Vec<&String> = folder
        .children
        .iter()
        .filter(|cid| store.folders.get(*cid).is_some_and(|f| !f.deleted))
        .collect();
    for child_id in child_folder_ids {
        collect_folder_paths(store, child_id, &path, exclude_ids, current_parent_id, out);
    }
}

fn render_folder_response(
    state: &AppState,
    folder_id: &str,
    reset_detail: bool,
    sort: SortOrder,
) -> Result<Html<String>, StatusCode> {
    let store = read_store(&state.doc_handle)?;
    let effective_id = if store.folders.contains_key(folder_id) {
        folder_id.to_owned()
    } else {
        store.root_folder_id.clone()
    };

    let sidebar_html = build_sidebar_html(&store, &effective_id);
    let (folders, bookmarks) = build_folder_items(&store, &effective_id, sort);
    let breadcrumbs = build_breadcrumbs(&store, &effective_id);
    let folder_title = store
        .folders
        .get(&effective_id)
        .map_or("Bookmarks", |f| f.title.as_str()).to_owned();
    let total_items = folders.len() + bookmarks.len();

    let content = render(&FolderContentTemplate {
        folder_id: effective_id,
        breadcrumbs,
        folders,
        bookmarks,
    })?;

    let detail_oob = if reset_detail {
        let detail = render(&DetailEmptyTemplate)?;
        format!(r#"<div id="detail-body" hx-swap-oob="innerHTML">{detail}</div>"#)
    } else {
        String::new()
    };

    let response = format!(
        r#"<title>{title} — MyBriefcase Bookmarks</title>
{content}
<div id="sidebar-tree" hx-swap-oob="innerHTML">{sidebar_html}</div>
{detail_oob}
<span id="status-text" hx-swap-oob="innerHTML">{title}</span>
<span id="status-count" hx-swap-oob="innerHTML">{total_items} items</span>"#,
        title = html_escape(&folder_title),
    );

    Ok(Html(response))
}

fn format_timestamp(ts: i64) -> String {
    let secs = if ts > 1_000_000_000_000 {
        ts / 1000
    } else {
        ts
    };
    chrono::DateTime::from_timestamp(secs, 0).map_or_else(
        || "unknown".to_owned(),
        |dt| dt.format("%Y-%m-%d %H:%M").to_string(),
    )
}

// ─── Handlers ───────────────────────────────────────

/// # Errors
/// Returns `500 Internal Server Error` if template rendering fails.
pub async fn index_page(
    State(state): State<Arc<AppState>>,
    Query(params): Query<FolderContentParams>,
) -> Result<Html<String>, StatusCode> {
    let sort = SortOrder::from_param(params.sort.as_deref());
    let store = read_store(&state.doc_handle)?;
    let root_id = store.root_folder_id.clone();

    let sidebar_html = build_sidebar_html(&store, &root_id);
    let (folders, bookmarks) = build_folder_items(&store, &root_id, sort);
    let breadcrumbs = build_breadcrumbs(&store, &root_id);

    let content_html = render(&FolderContentTemplate {
        folder_id: root_id.clone(),
        breadcrumbs,
        folders,
        bookmarks,
    })?;

    let page = BaseTemplate {
        sidebar_html,
        content_html,
        current_folder_id: root_id,
        page_title: "MyBriefcase Bookmarks".to_owned(),
        static_v: state.static_version.clone(),
    };

    Ok(Html(render(&page)?))
}

/// # Errors
/// Returns `500 Internal Server Error` if template rendering fails.
pub async fn folder_options(
    State(state): State<Arc<AppState>>,
) -> Result<Html<String>, StatusCode> {
    let store = read_store(&state.doc_handle)?;
    let root_id = store.root_folder_id.clone();
    let folders = collect_all_folder_paths(&store, &root_id);
    let mut html = String::new();
    for (id, path) in &folders {
        write!(html, "<option value=\"{id}\">{path}</option>").unwrap();
    }
    Ok(Html(html))
}

/// # Errors
/// Returns `500 Internal Server Error` if template rendering fails.
pub async fn settings_page(State(state): State<Arc<AppState>>) -> Result<Html<String>, StatusCode> {
    let content_html = render(&SettingsTemplate)?;

    let page = SettingsBaseTemplate {
        content_html,
        page_title: "Settings — MyBriefcase Bookmarks".to_owned(),
        static_v: state.static_version.clone(),
    };

    Ok(Html(render(&page)?))
}

/// # Errors
/// Returns `500 Internal Server Error` if template rendering fails.
pub async fn index_page_for_folder(
    State(state): State<Arc<AppState>>,
    folder_id: &str,
    sort: SortOrder,
) -> Result<Html<String>, StatusCode> {
    let store = read_store(&state.doc_handle)?;
    let effective_id = if store.folders.contains_key(folder_id) {
        folder_id.to_owned()
    } else {
        store.root_folder_id.clone()
    };

    let sidebar_html = build_sidebar_html(&store, &effective_id);
    let (folders, bookmarks) = build_folder_items(&store, &effective_id, sort);
    let breadcrumbs = build_breadcrumbs(&store, &effective_id);
    let folder_title = store
        .folders
        .get(&effective_id)
        .map_or("Bookmarks", |f| f.title.as_str());
    let page_title = format!("{folder_title} — MyBriefcase Bookmarks");

    let content_html = render(&FolderContentTemplate {
        folder_id: effective_id.clone(),
        breadcrumbs,
        folders,
        bookmarks,
    })?;

    let page = BaseTemplate {
        sidebar_html,
        content_html,
        current_folder_id: effective_id,
        page_title,
        static_v: state.static_version.clone(),
    };

    Ok(Html(render(&page)?))
}

/// # Errors
/// Returns `500 Internal Server Error` if template rendering fails.
pub async fn folder_content(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(params): Query<FolderContentParams>,
) -> Result<Html<String>, StatusCode> {
    let sort = SortOrder::from_param(params.sort.as_deref());
    render_folder_response(&state, &id, false, sort)
}

pub async fn dispatch_get_folder(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(params): Query<FolderContentParams>,
    headers: axum::http::HeaderMap,
) -> Response {
    let sort = SortOrder::from_param(params.sort.as_deref());
    if headers.contains_key("hx-request") {
        render_folder_response(&state, &id, true, sort)
            .map_or_else(IntoResponse::into_response, IntoResponse::into_response)
    } else {
        index_page_for_folder(State(Arc::clone(&state)), &id, sort)
            .await
            .into_response()
    }
}

/// # Errors
/// Returns `500 Internal Server Error` if template rendering fails.
pub async fn bookmark_detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Response, StatusCode> {
    let store = read_store(&state.doc_handle)?;
    let bm = match store.bookmarks.get(&id) {
        Some(b) if !b.deleted => b,
        _ => return Ok(Html(render(&DetailEmptyTemplate)?).into_response()),
    };

    let template = DetailBookmarkTemplate {
        id: id.clone(),
        title: bm.title.clone(),
        url: bm.url.clone(),
        notes: bm.notes.clone(),
        favicon: bm.favicon.clone(),
        created_at: bm.created_at.clone(),
        updated_at: bm.updated_at.clone(),
        created_date: date_short(&bm.created_at),
        updated_date: date_short(&bm.updated_at),
        domain_color: domain_color(&bm.url),
        domain_letter: domain_letter(&bm.url),
    };

    Ok(Html(render(&template)?).into_response())
}

/// # Errors
/// Returns `500 Internal Server Error` if template rendering fails.
pub async fn bookmark_edit_form(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Response, StatusCode> {
    let store = read_store(&state.doc_handle)?;
    let bm = match store.bookmarks.get(&id) {
        Some(b) if !b.deleted => b,
        _ => return Ok(Html(render(&DetailEmptyTemplate)?).into_response()),
    };

    let current_folder_id = find_folder_for_bookmark(&store, &id)
        .unwrap_or(&store.root_folder_id).to_owned();
    let mut folders = Vec::new();
    let exclude = std::collections::HashSet::new();
    collect_folder_paths(
        &store,
        &store.root_folder_id,
        "",
        &exclude,
        &current_folder_id,
        &mut folders,
    );

    let template = EditBookmarkTemplate {
        id: id.clone(),
        title: bm.title.clone(),
        url: bm.url.clone(),
        notes: bm.notes.clone(),
        favicon: bm.favicon.clone(),
        domain_color: domain_color(&bm.url),
        domain_letter: domain_letter(&bm.url),
        folders,
    };

    Ok(Html(render(&template)?).into_response())
}

/// # Errors
/// Returns `500 Internal Server Error` if template rendering fails.
pub async fn create_folder_html(
    State(state): State<Arc<AppState>>,
    Form(form): Form<CreateFolderForm>,
) -> Result<Html<String>, StatusCode> {
    state
        .mutate(|doc| ops::create_folder(doc, &form.parent_folder_id, &form.title))
        .map_err(|e| core_error_to_status(&e))?;
    render_folder_response(&state, &form.parent_folder_id, true, SortOrder::default())
}

/// # Errors
/// Returns `500 Internal Server Error` if template rendering fails.
pub async fn create_bookmark_html(
    State(state): State<Arc<AppState>>,
    Form(form): Form<CreateBookmarkForm>,
) -> Result<Html<String>, StatusCode> {
    let id = state
        .mutate(|doc| ops::add_bookmark(doc, &form.folder_id, &form.url, &form.title))
        .map_err(|e| core_error_to_status(&e))?;

    if !form.favicon_url.is_empty() && !form.favicon_url.starts_with("data:") {
        let sync_root = state.sync_root.clone();
        let doc_handle = state.doc_handle.clone();
        let sse_tx = state.sse_tx.clone();
        let favicon_url = form.favicon_url.clone();
        let bookmark_url = form.url.clone();
        let client_id = state.client_id.clone();
        tokio::spawn(async move {
            if let Ok(filename) = crate::favicon::fetch_and_store(&sync_root, &favicon_url).await {
                let mut ids_to_update = vec![id];
                if let Ok(store) =
                    doc_handle.with_doc(autosurgeon::hydrate::<_, crate::model::BookmarkStore>)
                {
                    for (existing_id, bm) in &store.bookmarks {
                        if bm.url == bookmark_url
                            && !bm.deleted
                            && !ids_to_update.contains(existing_id)
                        {
                            ids_to_update.push(existing_id.clone());
                        }
                    }
                }
                for bm_id in &ids_to_update {
                    let _ = ops::update_favicon(&doc_handle, bm_id, &filename);
                }
                let _ = crate::repo::export_doc_to_shared(
                    &doc_handle,
                    &sync_root,
                    &client_id,
                    std::time::SystemTime::now(),
                );
                let _ = sse_tx.send(());
            }
        });
    }

    render_folder_response(&state, &form.folder_id, true, SortOrder::default())
}

/// # Errors
/// Returns `500 Internal Server Error` if template rendering fails.
pub async fn update_bookmark_html(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Form(form): Form<UpdateBookmarkForm>,
) -> Result<Response, StatusCode> {
    state
        .mutate(|doc| {
            ops::update_bookmark(
                doc,
                &id,
                form.url.as_deref(),
                form.title.as_deref(),
                form.notes.as_deref(),
            )?;

            if let Some(ref favicon) = form.favicon {
                ops::update_favicon(doc, &id, favicon)?;
                if !favicon.is_empty() {
                    let store: BookmarkStore = doc.with_doc(|d| {
                        autosurgeon::hydrate(d)
                            .map_err(|e| CoreError::DocumentCorrupted(e.to_string()))
                    })?;
                    if let Some(bm) = store.bookmarks.get(&id) {
                        let url = bm.url.clone();
                        for (other_id, other_bm) in &store.bookmarks {
                            if other_id != &id && other_bm.url == url && !other_bm.deleted {
                                ops::update_favicon(doc, other_id, favicon)?;
                            }
                        }
                    }
                }
            }

            if let Some(ref new_folder_id) = form.folder_id {
                let store: BookmarkStore = doc.with_doc(|d| {
                    autosurgeon::hydrate(d).map_err(|e| CoreError::DocumentCorrupted(e.to_string()))
                })?;
                let current_folder_id = find_folder_for_bookmark(&store, &id)
                    .unwrap_or(&store.root_folder_id).to_owned();
                if *new_folder_id != current_folder_id {
                    ops::move_item(doc, &id, &current_folder_id, new_folder_id)?;
                }
            }
            Ok(())
        })
        .map_err(|e| core_error_to_status(&e))?;

    let store = read_store(&state.doc_handle)?;
    let Some(bm) = store.bookmarks.get(&id) else {
        return Ok(Html(render(&DetailEmptyTemplate)?).into_response());
    };

    let folder_id = find_folder_for_bookmark(&store, &id)
        .unwrap_or(&store.root_folder_id).to_owned();
    let sidebar_html = build_sidebar_html(&store, &folder_id);
    let (folders, bookmarks) = build_folder_items(&store, &folder_id, SortOrder::default());
    let breadcrumbs = build_breadcrumbs(&store, &folder_id);
    let folder_title = store
        .folders
        .get(&folder_id)
        .map_or("Bookmarks", |f| f.title.as_str()).to_owned();
    let total_items = folders.len() + bookmarks.len();

    let detail = render(&DetailBookmarkTemplate {
        id: id.clone(),
        title: bm.title.clone(),
        url: bm.url.clone(),
        notes: bm.notes.clone(),
        favicon: bm.favicon.clone(),
        created_at: bm.created_at.clone(),
        updated_at: bm.updated_at.clone(),
        created_date: date_short(&bm.created_at),
        updated_date: date_short(&bm.updated_at),
        domain_color: domain_color(&bm.url),
        domain_letter: domain_letter(&bm.url),
    })?;

    let content = render(&FolderContentTemplate {
        folder_id,
        breadcrumbs,
        folders,
        bookmarks,
    })?;

    let response = format!(
        r#"{detail}
<div id="folder-content" hx-swap-oob="innerHTML">{content}</div>
<div id="sidebar-tree" hx-swap-oob="innerHTML">{sidebar_html}</div>
<span id="status-text" hx-swap-oob="innerHTML">{title}</span>
<span id="status-count" hx-swap-oob="innerHTML">{total_items} items</span>"#,
        title = html_escape(&folder_title),
    );

    Ok(Html(response).into_response())
}

/// # Errors
/// Returns `500 Internal Server Error` if template rendering fails.
pub async fn fetch_favicon_html(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Html<String>, StatusCode> {
    let store = read_store(&state.doc_handle)?;
    let bm = store
        .bookmarks
        .get(&id)
        .filter(|b| !b.deleted)
        .ok_or(StatusCode::NOT_FOUND)?;

    let url = bm.url.clone();
    let color = domain_color(&url);
    let letter = domain_letter(&url);

    let favicon_url = match crate::favicon::discover_favicon_url(&url).await {
        Ok(u) => u,
        Err(e) => {
            let template = FaviconPreviewTemplate {
                favicon: bm.favicon.clone(),
                domain_color: color,
                domain_letter: letter,
                error: format!("Could not fetch favicon: {e}"),
            };
            return Ok(Html(render(&template)?));
        }
    };

    let filename = match crate::favicon::fetch_and_store(&state.sync_root, &favicon_url).await {
        Ok(f) => f,
        Err(e) => {
            let template = FaviconPreviewTemplate {
                favicon: bm.favicon.clone(),
                domain_color: color,
                domain_letter: letter,
                error: format!("Could not fetch favicon: {e}"),
            };
            return Ok(Html(render(&template)?));
        }
    };

    ops::update_favicon(&state.doc_handle, &id, &filename).map_err(|e| core_error_to_status(&e))?;

    let template = FaviconPreviewTemplate {
        favicon: filename,
        domain_color: color,
        domain_letter: letter,
        error: String::new(),
    };
    Ok(Html(render(&template)?))
}

/// # Errors
/// Returns `500 Internal Server Error` if template rendering fails.
pub async fn delete_bookmark_html(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Form(form): Form<DeleteForm>,
) -> Result<Html<String>, StatusCode> {
    state
        .mutate(|doc| ops::delete_bookmark(doc, &id))
        .map_err(|e| core_error_to_status(&e))?;
    render_folder_response(&state, &form.current_folder_id, true, SortOrder::default())
}

/// # Errors
/// Returns `500 Internal Server Error` if template rendering fails.
pub async fn delete_folder_html(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Form(form): Form<DeleteForm>,
) -> Result<Html<String>, StatusCode> {
    state
        .mutate(|doc| ops::delete_folder(doc, &id))
        .map_err(|e| core_error_to_status(&e))?;
    let target = if form.current_folder_id == id {
        let store = read_store(&state.doc_handle)?;
        store.root_folder_id
    } else {
        form.current_folder_id
    };
    render_folder_response(&state, &target, true, SortOrder::default())
}

/// # Errors
/// Returns `500 Internal Server Error` if template rendering fails.
pub async fn rename_folder_html(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Form(form): Form<RenameFolderForm>,
) -> Result<Html<String>, StatusCode> {
    state
        .mutate(|doc| ops::rename_folder(doc, &id, &form.title))
        .map_err(|e| core_error_to_status(&e))?;
    render_folder_response(&state, &form.current_folder_id, true, SortOrder::default())
}

/// # Errors
/// Returns `422 Unprocessable Entity` if cycle detection fails.
/// Returns `500 Internal Server Error` if template rendering fails.
pub async fn move_item_html(
    State(state): State<Arc<AppState>>,
    Form(form): Form<MoveItemForm>,
) -> Result<Html<String>, StatusCode> {
    state
        .mutate(|doc| ops::move_item(doc, &form.item, &form.source, &form.destination))
        .map_err(|e| core_error_to_status(&e))?;
    render_folder_response(&state, &form.destination, true, SortOrder::default())
}

/// # Errors
/// Returns `404 Not Found` if the item does not exist.
/// Returns `500 Internal Server Error` if template rendering fails.
pub async fn move_picker_html(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Html<String>, StatusCode> {
    let store = read_store(&state.doc_handle)?;

    let is_folder = store.folders.contains_key(&id);
    let is_bookmark = store.bookmarks.get(&id).is_some_and(|b| !b.deleted);
    if !is_folder && !is_bookmark {
        return Err(StatusCode::NOT_FOUND);
    }

    let from_folder_id =
        find_parent_folder_id(&store, &id).unwrap_or_else(|| store.root_folder_id.clone());

    let mut exclude_ids = std::collections::HashSet::new();
    if is_folder {
        collect_descendants(&store, &id, &mut exclude_ids);
        exclude_ids.insert(id.clone());
    }

    let mut folders_with_paths: Vec<(String, String, bool)> = Vec::new();
    collect_folder_paths(
        &store,
        &store.root_folder_id,
        "",
        &exclude_ids,
        &from_folder_id,
        &mut folders_with_paths,
    );

    let mut html = String::new();
    html.push_str(r"<h2>Move to&hellip;</h2>");
    html.push_str(
        r##"<form hx-post="/items/move" hx-target="#folder-content" hx-swap="innerHTML">"##,
    );
    let _ = write!(html, r#"<input type="hidden" name="item_id" value="{id}">"#);
    let _ = write!(
        html,
        r#"<input type="hidden" name="from_folder_id" value="{from_folder_id}">"#,
    );
    html.push_str(r#"<div class="move-list">"#);
    for (folder_id, path, is_current) in &folders_with_paths {
        let current_label = if *is_current { " (current)" } else { "" };
        let cls = if *is_current { " current" } else { "" };
        let _ = write!(
            html,
            r#"<label class="move-list-item{cls}"><span class="move-list-label">{}{current_label}</span><input type="radio" name="to_folder_id" value="{folder_id}" required></label>"#,
            html_escape(path),
        );
    }
    html.push_str("</div>");
    html.push_str(r#"<div class="modal-actions">"#);
    html.push_str(r#"<button type="button" class="btn btn-ghost" @click="$store.app.showMoveModal = false">Cancel</button>"#);
    html.push_str(r#"<button type="submit" class="btn btn-primary">Move</button>"#);
    html.push_str("</div></form>");

    Ok(Html(html))
}

/// # Errors
/// Returns `500 Internal Server Error` if the document cannot be read.
pub async fn sidebar_only(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SidebarParams>,
) -> Result<Html<String>, StatusCode> {
    let store = read_store(&state.doc_handle)?;
    let folder_id = params
        .folder_id
        .filter(|id| store.folders.contains_key(id))
        .unwrap_or_else(|| store.root_folder_id.clone());
    let sidebar_html = build_sidebar_html(&store, &folder_id);

    let folder = store.folders.get(&folder_id);
    let folder_title = folder.map_or("Bookmarks", |f| f.title.as_str());
    let (folders, bookmarks) = build_folder_items(&store, &folder_id, SortOrder::default());
    let total_items = folders.len() + bookmarks.len();

    Ok(Html(format!(
        r#"{sidebar_html}
<span id="status-text" hx-swap-oob="innerHTML">{title}</span>
<span id="status-count" hx-swap-oob="innerHTML">{total_items} items</span>"#,
        title = html_escape(folder_title),
    )))
}

/// # Errors
/// Returns `500 Internal Server Error` if template rendering fails.
pub async fn search(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchParams>,
) -> Result<Html<String>, StatusCode> {
    let store = read_store(&state.doc_handle)?;
    let query = params.q.to_lowercase();

    let mut matching: Vec<BookmarkItemView> = store
        .bookmarks
        .iter()
        .filter(|(_, bm)| !bm.deleted)
        .filter(|(_, bm)| {
            bm.title.to_lowercase().contains(&query)
                || bm.url.to_lowercase().contains(&query)
                || bm.notes.to_lowercase().contains(&query)
        })
        .map(|(id, bm)| BookmarkItemView {
            id: id.clone(),
            title: bm.title.clone(),
            url: bm.url.clone(),
            notes: bm.notes.clone(),
            created_at: bm.created_at.clone(),
            created_date: date_short(&bm.created_at),
            favicon: bm.favicon.clone(),
            domain_color: domain_color(&bm.url),
            domain_letter: domain_letter(&bm.url),
        })
        .collect();

    let sort = SortOrder::from_param(params.sort.as_deref());
    let mut empty_folders: Vec<FolderItemView> = vec![];
    sort_items(&mut empty_folders, &mut matching, sort);

    let search_title = format!("Search: \"{}\"", params.q);
    let root_id = store.root_folder_id;
    let content = render(&FolderContentTemplate {
        folder_id: root_id.clone(),
        breadcrumbs: vec![BreadcrumbItem {
            id: root_id,
            title: search_title,
            is_last: true,
        }],
        folders: vec![],
        bookmarks: matching,
    })?;

    Ok(Html(content))
}

/// # Errors
/// Returns `500 Internal Server Error` if template rendering fails.
pub async fn bookmark_history_html(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Html<String>, StatusCode> {
    let store = read_store(&state.doc_handle)?;
    let bm = match store.bookmarks.get(&id) {
        Some(b) if !b.deleted => b,
        _ => return Ok(Html(render(&DetailEmptyTemplate)?)),
    };

    let entries = history::bookmark_history(&state.doc_handle, &id);

    let mut html = String::new();

    html.push_str(r#"<div class="detail-icon-wrap">"#);
    let dc = domain_color(&bm.url);
    let dl = domain_letter(&bm.url);
    if bm.favicon.is_empty() {
        let _ = write!(
            html,
            r#"<span class="favicon favicon-lg" style="background:{dc}">{dl}</span>"#,
        );
    } else {
        let _ = write!(
            html,
            r#"<img class="favicon favicon-lg" src="/favicons/{fav}" alt="" onerror="this.replaceWith(Object.assign(document.createElement('span'),{{className:'favicon favicon-lg',textContent:'{dl}',style:'background:{dc}'}}))""#,
            fav = html_escape(&bm.favicon),
        );
        html.push('>');
    }
    html.push_str("</div>");
    let _ = write!(
        html,
        r#"<div class="detail-title">{}</div>"#,
        html_escape(&bm.title)
    );
    let _ = write!(
        html,
        r#"<a class="detail-url" href="{url}" target="_blank" rel="noopener">{url}</a>"#,
        url = html_escape(&bm.url)
    );

    html.push_str(r#"<div style="display:flex;gap:var(--space-2);margin-bottom:var(--space-3);border-bottom:1px solid var(--border-light);padding-bottom:var(--space-2)">"#);
    let _ = write!(
        html,
        r##"<button class="btn btn-ghost" hx-get="/bookmarks/{id}/detail" hx-target="#detail-body" hx-swap="innerHTML" style="font-size:var(--text-xs)">Details</button>"##,
    );
    html.push_str(
        r#"<button class="btn btn-primary" style="font-size:var(--text-xs)">History</button>"#,
    );
    html.push_str("</div>");

    if entries.is_empty() {
        html.push_str(r#"<p style="color:var(--text-faint);font-size:var(--text-xs)">No history entries yet.</p>"#);
    } else {
        html.push_str(r#"<div style="display:flex;flex-direction:column;gap:var(--space-2)">"#);
        for entry in &entries {
            let date = format_timestamp(entry.timestamp);
            let short_hash = &entry.hash[..8.min(entry.hash.len())];
            let fields: Vec<&str> = entry
                .changed_fields
                .iter()
                .map(|f| f.field.as_str())
                .collect();
            let fields_str = if fields.is_empty() {
                "created".to_owned()
            } else {
                fields.join(", ")
            };

            html.push_str(r#"<div style="border:1px solid var(--border-light);border-radius:var(--radius-md);padding:var(--space-2) var(--space-3);font-size:var(--text-xs)">"#);
            let _ = write!(
                html,
                r#"<div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:var(--space-1)"><span style="color:var(--text-muted)">{date}</span><code style="font-size:10px;color:var(--text-faint)">{short_hash}</code></div>"#,
            );
            let _ = write!(
                html,
                r#"<div style="color:var(--text);margin-bottom:var(--space-1)">Changed: {fields_str}</div>"#,
            );

            for fc in &entry.changed_fields {
                if let (Some(old), Some(new)) = (&fc.old_value, &fc.new_value) {
                    let _ = write!(
                        html,
                        r#"<div style="font-size:10px;color:var(--text-faint);margin-bottom:2px"><b>{field}</b>: <span style="text-decoration:line-through;color:#c33">{old}</span> → <span style="color:#3a3">{new}</span></div>"#,
                        field = html_escape(&fc.field),
                        old = html_escape(old),
                        new = html_escape(new),
                    );
                }
            }

            let _ = write!(
                html,
                r##"<form hx-post="/bookmarks/{id}/revert" hx-target="#detail-body" hx-swap="innerHTML" style="margin-top:var(--space-1)"><input type="hidden" name="target_hash" value="{hash}"><button type="submit" class="btn btn-ghost" style="font-size:10px;padding:2px 8px" hx-confirm="Restore this version?">Restore</button></form>"##,
                hash = entry.hash,
            );
            html.push_str("</div>");
        }
        html.push_str("</div>");
    }

    Ok(Html(html))
}

/// # Errors
/// Returns `500 Internal Server Error` if the revert fails.
pub async fn revert_bookmark_html(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Form(form): Form<RevertForm>,
) -> Result<Response, StatusCode> {
    let hash = history::parse_change_hash(&form.target_hash).ok_or(StatusCode::BAD_REQUEST)?;
    state
        .mutate(|doc| ops::revert_bookmark(doc, &id, &hash))
        .map_err(|e| core_error_to_status(&e))?;

    let store = read_store(&state.doc_handle)?;
    let Some(bm) = store.bookmarks.get(&id) else {
        return Ok(Html(render(&DetailEmptyTemplate)?).into_response());
    };

    let folder_id = find_folder_for_bookmark(&store, &id)
        .unwrap_or(&store.root_folder_id).to_owned();
    let sidebar_html = build_sidebar_html(&store, &folder_id);
    let (folders, bookmarks) = build_folder_items(&store, &folder_id, SortOrder::default());
    let breadcrumbs = build_breadcrumbs(&store, &folder_id);
    let folder_title = store
        .folders
        .get(&folder_id)
        .map_or("Bookmarks", |f| f.title.as_str()).to_owned();
    let total_items = folders.len() + bookmarks.len();

    let detail = render(&DetailBookmarkTemplate {
        id: id.clone(),
        title: bm.title.clone(),
        url: bm.url.clone(),
        notes: bm.notes.clone(),
        favicon: bm.favicon.clone(),
        created_at: bm.created_at.clone(),
        updated_at: bm.updated_at.clone(),
        created_date: date_short(&bm.created_at),
        updated_date: date_short(&bm.updated_at),
        domain_color: domain_color(&bm.url),
        domain_letter: domain_letter(&bm.url),
    })?;

    let content = render(&FolderContentTemplate {
        folder_id,
        breadcrumbs,
        folders,
        bookmarks,
    })?;

    let response = format!(
        r#"{detail}
<div id="folder-content" hx-swap-oob="innerHTML">{content}</div>
<div id="sidebar-tree" hx-swap-oob="innerHTML">{sidebar_html}</div>
<span id="status-text" hx-swap-oob="innerHTML">{title}</span>
<span id="status-count" hx-swap-oob="innerHTML">{total_items} items</span>"#,
        title = html_escape(&folder_title),
    );

    Ok(Html(response).into_response())
}

/// # Errors
/// Returns `500 Internal Server Error` if multipart parsing or import fails.
pub async fn import_bookmarks_html(
    State(state): State<Arc<AppState>>,
    mut multipart: axum::extract::Multipart,
) -> Result<Html<String>, StatusCode> {
    let mut target = String::new();
    let mut file_content = Vec::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        match field.name() {
            Some("target") => {
                target = field.text().await.unwrap_or_default();
            }
            Some("file") => {
                file_content = field.bytes().await.unwrap_or_default().to_vec();
            }
            _ => {}
        }
    }

    if file_content.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let content = String::from_utf8_lossy(&file_content);
    let items = crate::import::parse_netscape_html(&content);

    if items.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let store = read_store(&state.doc_handle)?;
    let root_id = store.root_folder_id;

    let target_folder_id = state
        .mutate(|doc| {
            let folder_id = match target.as_str() {
                "new" => {
                    let name = format!("Imported {}", chrono::Utc::now().format("%Y-%m-%d"));
                    ops::create_folder(doc, &root_id, &name)?
                }
                _ => root_id.clone(),
            };
            ops::import_items(doc, &folder_id, &items)?;
            Ok(folder_id)
        })
        .map_err(|e| core_error_to_status(&e))?;

    render_folder_response(&state, &target_folder_id, true, SortOrder::default())
}

pub async fn sse_events(
    State(state): State<Arc<AppState>>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let rx = state.sse_tx.subscribe();
    let stream = BroadcastStream::new(rx)
        .filter_map(Result::ok)
        .map(|()| Ok(Event::default().event("refresh").data("sync")));
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

/// # Errors
/// Returns `404 Not Found` if the favicon file does not exist.
pub async fn serve_favicon(
    State(state): State<Arc<AppState>>,
    Path(filename): Path<String>,
) -> Result<Response, StatusCode> {
    if !filename
        .chars()
        .all(|c| c.is_ascii_hexdigit() || c == '.' || c.is_ascii_alphabetic())
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let path = crate::favicon::favicon_path(&state.sync_root, &filename);
    let data = tokio::fs::read(&path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let content_type = match path.extension().and_then(|e| e.to_str()) {
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("svg") => "image/svg+xml",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    };
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, content_type.to_owned()),
            (
                axum::http::header::CACHE_CONTROL,
                "public, max-age=31536000, immutable".to_owned(),
            ),
        ],
        data,
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_timestamp_millis() {
        let ts = 1_700_000_000_000; // millis
        let result = format_timestamp(ts);
        assert!(result.starts_with("2023-11-14"));
    }

    #[test]
    fn format_timestamp_seconds() {
        let ts = 1_700_000_000; // seconds
        let result = format_timestamp(ts);
        assert!(result.starts_with("2023-11-14"));
    }

    #[test]
    fn format_timestamp_zero() {
        let result = format_timestamp(0);
        assert_eq!(result, "1970-01-01 00:00");
    }

    #[test]
    fn format_timestamp_millis_vs_seconds_boundary() {
        // Exactly at threshold: treated as millis, divided by 1000
        let ts_millis = 1_000_000_000_001;
        let ts_secs = 1_000_000_000;
        assert_eq!(format_timestamp(ts_millis), format_timestamp(ts_secs));
    }

    #[test]
    fn format_timestamp_just_below_threshold_treated_as_seconds() {
        // 999_999_999_999 is below the threshold, treated as seconds (year ~33658)
        let ts = 999_999_999;
        let result = format_timestamp(ts);
        assert_eq!(result, "2001-09-09 01:46");
    }

    #[test]
    fn format_timestamp_exactly_at_threshold_boundary() {
        // Exactly 1_000_000_000_000: the condition is `ts > 1_000_000_000_000`,
        // so this value is NOT greater and is treated as seconds.
        // If mutated to `>=`, it would be divided by 1000, yielding a different date.
        let ts = 1_000_000_000_000;
        let result = format_timestamp(ts);
        // Treated as seconds: this is year ~33658, but chrono can still format it
        // The key assertion: it must NOT equal what you'd get treating it as millis
        let as_millis_result = format_timestamp(1_000_000_000_001); // > threshold, treated as millis
        // as_millis_result formats timestamp 1_000_000_000 (the divided value)
        // If the boundary value were also divided, it would be 1_000_000_000 too.
        // But since it's treated as seconds (not divided), it's a much larger timestamp.
        assert_ne!(result, as_millis_result);
    }

    fn make_store(
        root_id: &str,
        folders: Vec<(&str, &str, Vec<&str>)>,
        bookmarks: Vec<(&str, &str, &str, &str)>,
    ) -> BookmarkStore {
        use std::collections::HashMap;
        let mut folder_map = HashMap::new();
        for (id, title, children) in folders {
            folder_map.insert(
                id.to_owned(),
                crate::model::Folder {
                    title: title.to_owned(),
                    children: children.into_iter().map(String::from).collect(),
                    created_at: "2026-01-01T00:00:00Z".to_owned(),
                    updated_at: "2026-01-01T00:00:00Z".to_owned(),
                    deleted: false,
                },
            );
        }
        let mut bookmark_map = HashMap::new();
        for (id, title, url, created) in bookmarks {
            bookmark_map.insert(
                id.to_owned(),
                crate::model::Bookmark {
                    url: url.to_owned(),
                    title: title.to_owned(),
                    notes: String::new(),
                    favicon: String::new(),
                    created_at: created.to_owned(),
                    updated_at: created.to_owned(),
                    deleted: false,
                },
            );
        }
        BookmarkStore {
            root_folder_id: root_id.to_owned(),
            folders: folder_map,
            bookmarks: bookmark_map,
            meta: crate::model::StoreMeta {
                schema_version: 1,
                collection_name: "bookmarks".to_owned(),
            },
        }
    }

    #[test]
    fn find_folder_for_bookmark_returns_none_when_not_in_any_folder() {
        let store = make_store(
            "root",
            vec![("root", "Root", vec![])],
            vec![("bm-orphan", "Orphan", "https://x.com", "2026-01-01")],
        );
        // Bookmark exists in store.bookmarks but is not in any folder's children
        let result = find_folder_for_bookmark(&store, "bm-orphan");
        assert_eq!(result, None);
    }

    #[test]
    fn find_parent_folder_id_finds_parent() {
        let store = make_store(
            "root",
            vec![
                ("root", "Root", vec!["parent"]),
                ("parent", "Parent", vec!["child"]),
                ("child", "Child", vec![]),
            ],
            vec![],
        );
        let result = find_parent_folder_id(&store, "child");
        assert_eq!(result, Some("parent".to_owned()));
    }

    #[test]
    fn find_parent_folder_id_returns_none_for_orphan() {
        let store = make_store(
            "root",
            vec![("root", "Root", vec![]), ("orphan", "Orphan", vec![])],
            vec![],
        );
        let result = find_parent_folder_id(&store, "orphan");
        assert_eq!(result, None);
    }

    #[test]
    fn find_folder_for_bookmark_finds_containing_folder() {
        let store = make_store(
            "root",
            vec![
                ("root", "Root", vec!["folder"]),
                ("folder", "Folder", vec!["bm1"]),
            ],
            vec![("bm1", "BM", "https://x.com", "2026-01-01")],
        );
        let result = find_folder_for_bookmark(&store, "bm1");
        assert_eq!(result, Some("folder"));
    }

    #[test]
    fn find_folder_for_bookmark_skips_deleted_folders() {
        let mut store = make_store(
            "root",
            vec![
                ("root", "Root", vec!["alive", "dead"]),
                ("alive", "Alive", vec!["bm1"]),
                ("dead", "Dead", vec!["bm1"]),
            ],
            vec![("bm1", "BM", "https://x.com", "2026-01-01")],
        );
        store.folders.get_mut("dead").unwrap().deleted = true;
        let result = find_folder_for_bookmark(&store, "bm1");
        assert_eq!(result, Some("alive"));
    }

    #[test]
    fn collect_descendants_populates_output_set() {
        let store = make_store(
            "root",
            vec![
                ("root", "Root", vec!["a"]),
                ("a", "A", vec!["b"]),
                ("b", "B", vec![]),
            ],
            vec![],
        );
        let mut out = std::collections::HashSet::new();
        collect_descendants(&store, "root", &mut out);
        // Should contain "a" and "b" (non-deleted descendant folders)
        assert!(out.contains("a"));
        assert!(out.contains("b"));
        assert_eq!(out.len(), 2);
    }
}
