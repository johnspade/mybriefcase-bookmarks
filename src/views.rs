use askama::Template;
use automerge_repo::DocHandle;
use autosurgeon::hydrate;
use axum::Form;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use serde::Deserialize;
use std::convert::Infallible;
use std::fmt::Write as FmtWrite;
use std::sync::Arc;
use std::time::Duration;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

use crate::api::AppState;
use crate::history;
use crate::model::{BookmarkStore, Folder};
use crate::ops;

// ─── View data ──────────────────────────────────────

pub struct BreadcrumbItem {
    pub id: String,
    pub title: String,
    pub is_last: bool,
}

pub struct FolderItemView {
    pub id: String,
    pub title: String,
    pub updated_at: String,
    pub item_count: usize,
    pub bookmark_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    #[default]
    NameAsc,
    NameDesc,
    DateDesc,
    DateAsc,
}

impl SortOrder {
    #[must_use]
    pub fn from_param(s: Option<&str>) -> Self {
        match s {
            Some("name_desc") => Self::NameDesc,
            Some("date_desc") => Self::DateDesc,
            Some("date_asc") => Self::DateAsc,
            _ => Self::NameAsc,
        }
    }
}

pub struct BookmarkItemView {
    pub id: String,
    pub title: String,
    pub url: String,
    pub notes: String,
    pub created_at: String,
    pub created_date: String,
    pub domain_color: String,
    pub domain_letter: String,
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
}

#[derive(Deserialize)]
pub struct UpdateBookmarkForm {
    title: Option<String>,
    url: Option<String>,
    notes: Option<String>,
    folder_id: Option<String>,
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

// ─── Templates ──────────────────────────────────────

#[derive(Template)]
#[template(path = "base.html")]
struct BaseTemplate {
    sidebar_html: String,
    content_html: String,
    current_folder_id: String,
    page_title: String,
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
    created_at: String,
    updated_at: String,
    created_date: String,
    updated_date: String,
    domain_color: String,
    domain_letter: String,
}

#[derive(Template)]
#[template(path = "detail_folder.html")]
struct DetailFolderTemplate {
    id: String,
    title: String,
    item_count: usize,
    bookmark_count: usize,
}

#[derive(Template)]
#[template(path = "edit_bookmark.html")]
struct EditBookmarkTemplate {
    id: String,
    title: String,
    url: String,
    notes: String,
    folders: Vec<(String, String, bool)>,
}

#[derive(Template)]
#[template(path = "detail_empty.html")]
struct DetailEmptyTemplate;

// ─── Helpers ────────────────────────────────────────

fn read_store(doc_handle: &DocHandle) -> Result<BookmarkStore, StatusCode> {
    doc_handle.with_doc(|doc| hydrate(doc).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR))
}

fn after_write(state: &AppState) {
    crate::repo::export_doc_to_shared(&state.doc_handle, &state.sync_root, &state.client_id);
    let _ = state.sse_tx.send(());
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn render(tmpl: &impl Template) -> Result<String, StatusCode> {
    tmpl.render().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn date_short(iso: &str) -> String {
    iso.chars().take(10).collect()
}

fn domain_color(url: &str) -> String {
    let colors = [
        "#e44", "#e84", "#4a9", "#46a", "#88a", "#a48", "#49a", "#a44",
    ];
    let mut hash: u32 = 0;
    for b in url.bytes() {
        hash = u32::from(b).wrapping_add(hash.wrapping_shl(5).wrapping_sub(hash));
    }
    colors[(hash as usize) % colors.len()].to_string()
}

fn domain_letter(url: &str) -> String {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let host = rest.split('/').next().unwrap_or("");
    let domain = host.strip_prefix("www.").unwrap_or(host);
    domain
        .chars()
        .next()
        .unwrap_or('?')
        .to_uppercase()
        .to_string()
}

fn count_bookmarks_recursive(store: &BookmarkStore, folder: &Folder) -> usize {
    let mut count = 0;
    for child_id in &folder.children {
        if let Some(bm) = store.bookmarks.get(child_id) {
            if !bm.deleted {
                count += 1;
            }
        } else if let Some(sub) = store.folders.get(child_id) {
            if !sub.deleted {
                count += count_bookmarks_recursive(store, sub);
            }
        }
    }
    count
}

fn find_parent_folder_id(store: &BookmarkStore, child_id: &str) -> Option<String> {
    for (folder_id, folder) in &store.folders {
        if !folder.deleted && folder.children.iter().any(|c| c == child_id) {
            return Some(folder_id.clone());
        }
    }
    None
}

fn build_breadcrumbs(store: &BookmarkStore, folder_id: &str) -> Vec<BreadcrumbItem> {
    let mut path = Vec::new();
    let mut current = folder_id.to_string();
    while let Some(folder) = store.folders.get(&current) {
        path.push((current.clone(), folder.title.clone()));
        if current == store.root_folder_id {
            break;
        }
        match find_parent_folder_id(store, &current) {
            Some(pid) => current = pid,
            None => break,
        }
    }
    path.reverse();
    let len = path.len();
    path.into_iter()
        .enumerate()
        .map(|(i, (id, title))| BreadcrumbItem {
            id,
            title,
            is_last: i == len - 1,
        })
        .collect()
}

fn is_folder_ancestor(store: &BookmarkStore, ancestor_id: &str, target_id: &str) -> bool {
    if ancestor_id == target_id {
        return false;
    }
    if let Some(folder) = store.folders.get(ancestor_id) {
        for child_id in &folder.children {
            if child_id == target_id {
                return true;
            }
            if store.folders.contains_key(child_id)
                && is_folder_ancestor(store, child_id, target_id)
            {
                return true;
            }
        }
    }
    false
}

fn build_sidebar_html(store: &BookmarkStore, current_folder_id: &str) -> String {
    let Some(root) = store.folders.get(&store.root_folder_id) else {
        return String::new();
    };
    let mut html = String::new();
    for child_id in &root.children {
        if let Some(folder) = store.folders.get(child_id) {
            if !folder.deleted {
                build_sidebar_folder(store, child_id, folder, current_folder_id, 0, &mut html);
            }
        }
    }
    html
}

fn build_sidebar_folder(
    store: &BookmarkStore,
    folder_id: &str,
    folder: &Folder,
    current_folder_id: &str,
    depth: usize,
    html: &mut String,
) {
    let is_selected = folder_id == current_folder_id;
    let is_ancestor = is_folder_ancestor(store, folder_id, current_folder_id);
    let is_open = is_selected || is_ancestor;

    let child_folder_ids: Vec<&String> = folder
        .children
        .iter()
        .filter(|id| store.folders.get(*id).is_some_and(|f| !f.deleted))
        .collect();
    let has_sub = !child_folder_ids.is_empty();
    let bm_count = count_bookmarks_recursive(store, folder);
    let selected_cls = if is_selected { " selected" } else { "" };
    let padding = 12 + depth * 18;

    let _ = write!(
        html,
        r##"<div class="tree-item{selected_cls}" style="padding-left:{padding}px" hx-get="/folders/{folder_id}" hx-target="#folder-content" hx-swap="innerHTML" hx-push-url="/folders/{folder_id}" data-folder-id="{folder_id}">"##,
    );

    if has_sub {
        let open_cls = if is_open { " open" } else { "" };
        let _ = write!(
            html,
            r#"<span class="chevron{open_cls}" onclick="event.stopPropagation();toggleChevron(this)"><svg width="10" height="10" viewBox="7 4 10 16" fill="none" stroke="currentColor" stroke-width="2.5"><polyline points="9 18 15 12 9 6"/></svg></span>"#,
        );
    } else {
        html.push_str(r#"<span style="width:14px;flex-shrink:0"></span>"#);
    }

    html.push_str(r#"<span class="item-icon"><svg width="14" height="14" viewBox="0 0 24 24" fill="var(--folder-color)" stroke="none"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg></span>"#);
    let _ = write!(
        html,
        r#"<span class="item-label">{}</span>"#,
        html_escape(&folder.title)
    );
    if bm_count > 0 {
        let _ = write!(html, r#"<span class="item-count">{bm_count}</span>"#);
    }
    html.push_str("</div>");

    if has_sub {
        let open_cls = if is_open { " open" } else { "" };
        let _ = write!(html, r#"<div class="tree-children{open_cls}">"#);
        for child_id in &child_folder_ids {
            if let Some(child) = store.folders.get(*child_id) {
                build_sidebar_folder(store, child_id, child, current_folder_id, depth + 1, html);
            }
        }
        html.push_str("</div>");
    }
}

fn build_folder_items(
    store: &BookmarkStore,
    folder_id: &str,
    sort: SortOrder,
) -> (Vec<FolderItemView>, Vec<BookmarkItemView>) {
    let Some(folder) = store.folders.get(folder_id) else {
        return (vec![], vec![]);
    };

    let mut folders = Vec::new();
    let mut bookmarks = Vec::new();

    for child_id in &folder.children {
        if let Some(sub) = store.folders.get(child_id) {
            if !sub.deleted {
                let item_count = sub
                    .children
                    .iter()
                    .filter(|id| {
                        store.folders.get(*id).is_some_and(|f| !f.deleted)
                            || store.bookmarks.get(*id).is_some_and(|b| !b.deleted)
                    })
                    .count();
                folders.push(FolderItemView {
                    id: child_id.clone(),
                    title: sub.title.clone(),
                    updated_at: sub.updated_at.clone(),
                    item_count,
                    bookmark_count: count_bookmarks_recursive(store, sub),
                });
            }
        } else if let Some(bm) = store.bookmarks.get(child_id) {
            if !bm.deleted {
                bookmarks.push(BookmarkItemView {
                    id: child_id.clone(),
                    title: bm.title.clone(),
                    url: bm.url.clone(),
                    notes: bm.notes.clone(),
                    created_at: bm.created_at.clone(),
                    created_date: date_short(&bm.created_at),
                    domain_color: domain_color(&bm.url),
                    domain_letter: domain_letter(&bm.url),
                });
            }
        }
    }

    sort_items(&mut folders, &mut bookmarks, sort);

    (folders, bookmarks)
}

fn sort_items(folders: &mut [FolderItemView], bookmarks: &mut [BookmarkItemView], sort: SortOrder) {
    use std::cmp::Reverse;
    match sort {
        SortOrder::NameAsc => {
            folders.sort_by_key(|f| f.title.to_lowercase());
            bookmarks.sort_by_key(|b| b.title.to_lowercase());
        }
        SortOrder::NameDesc => {
            folders.sort_by_key(|f| Reverse(f.title.to_lowercase()));
            bookmarks.sort_by_key(|b| Reverse(b.title.to_lowercase()));
        }
        SortOrder::DateDesc => {
            folders.sort_by_key(|f| Reverse(f.updated_at.clone()));
            bookmarks.sort_by_key(|b| Reverse(b.created_at.clone()));
        }
        SortOrder::DateAsc => {
            folders.sort_by_key(|f| f.updated_at.clone());
            bookmarks.sort_by_key(|b| b.created_at.clone());
        }
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
        folder_id.to_string()
    } else {
        store.root_folder_id.clone()
    };

    let sidebar_html = build_sidebar_html(&store, &effective_id);
    let (folders, bookmarks) = build_folder_items(&store, &effective_id, sort);
    let breadcrumbs = build_breadcrumbs(&store, &effective_id);
    let folder_title = store
        .folders
        .get(&effective_id)
        .map_or("Bookmarks", |f| f.title.as_str())
        .to_string();
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

fn find_folder_for_bookmark<'a>(store: &'a BookmarkStore, bookmark_id: &str) -> Option<&'a str> {
    for (fid, folder) in &store.folders {
        if !folder.deleted && folder.children.iter().any(|c| c == bookmark_id) {
            return Some(fid.as_str());
        }
    }
    None
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
        page_title: "MyBriefcase Bookmarks".to_string(),
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
        folder_id.to_string()
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
        .unwrap_or(&store.root_folder_id)
        .to_string();
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
        folders,
    };

    Ok(Html(render(&template)?).into_response())
}

/// # Errors
/// Returns `500 Internal Server Error` if template rendering fails.
pub async fn folder_detail_view(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Response, StatusCode> {
    let store = read_store(&state.doc_handle)?;
    let folder = match store.folders.get(&id) {
        Some(f) if !f.deleted => f,
        _ => return Ok(Html(render(&DetailEmptyTemplate)?).into_response()),
    };

    let item_count = folder
        .children
        .iter()
        .filter(|cid| {
            store.folders.get(*cid).is_some_and(|f| !f.deleted)
                || store.bookmarks.get(*cid).is_some_and(|b| !b.deleted)
        })
        .count();

    let template = DetailFolderTemplate {
        id: id.clone(),
        title: folder.title.clone(),
        item_count,
        bookmark_count: count_bookmarks_recursive(&store, folder),
    };

    Ok(Html(render(&template)?).into_response())
}

/// # Errors
/// Returns `500 Internal Server Error` if template rendering fails.
pub async fn create_folder_html(
    State(state): State<Arc<AppState>>,
    Form(form): Form<CreateFolderForm>,
) -> Result<Html<String>, StatusCode> {
    ops::create_folder(&state.doc_handle, &form.parent_folder_id, &form.title)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    after_write(&state);
    render_folder_response(&state, &form.parent_folder_id, true, SortOrder::default())
}

/// # Errors
/// Returns `500 Internal Server Error` if template rendering fails.
pub async fn create_bookmark_html(
    State(state): State<Arc<AppState>>,
    Form(form): Form<CreateBookmarkForm>,
) -> Result<Html<String>, StatusCode> {
    ops::add_bookmark(&state.doc_handle, &form.folder_id, &form.url, &form.title)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    after_write(&state);
    render_folder_response(&state, &form.folder_id, true, SortOrder::default())
}

/// # Errors
/// Returns `500 Internal Server Error` if template rendering fails.
pub async fn update_bookmark_html(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Form(form): Form<UpdateBookmarkForm>,
) -> Result<Response, StatusCode> {
    ops::update_bookmark(
        &state.doc_handle,
        &id,
        form.url.as_deref(),
        form.title.as_deref(),
        form.notes.as_deref(),
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if let Some(ref new_folder_id) = form.folder_id {
        let store = read_store(&state.doc_handle)?;
        let current_folder_id = find_folder_for_bookmark(&store, &id)
            .unwrap_or(&store.root_folder_id)
            .to_string();
        if *new_folder_id != current_folder_id {
            ops::move_item(&state.doc_handle, &id, &current_folder_id, new_folder_id)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }
    }

    after_write(&state);

    let store = read_store(&state.doc_handle)?;
    let Some(bm) = store.bookmarks.get(&id) else {
        return Ok(Html(render(&DetailEmptyTemplate)?).into_response());
    };

    let folder_id = find_folder_for_bookmark(&store, &id)
        .unwrap_or(&store.root_folder_id)
        .to_string();
    let sidebar_html = build_sidebar_html(&store, &folder_id);
    let (folders, bookmarks) = build_folder_items(&store, &folder_id, SortOrder::default());
    let breadcrumbs = build_breadcrumbs(&store, &folder_id);
    let folder_title = store
        .folders
        .get(&folder_id)
        .map_or("Bookmarks", |f| f.title.as_str())
        .to_string();
    let total_items = folders.len() + bookmarks.len();

    let detail = render(&DetailBookmarkTemplate {
        id: id.clone(),
        title: bm.title.clone(),
        url: bm.url.clone(),
        notes: bm.notes.clone(),
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
pub async fn delete_bookmark_html(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Form(form): Form<DeleteForm>,
) -> Result<Html<String>, StatusCode> {
    ops::delete_bookmark(&state.doc_handle, &id).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    after_write(&state);
    render_folder_response(&state, &form.current_folder_id, true, SortOrder::default())
}

/// # Errors
/// Returns `500 Internal Server Error` if template rendering fails.
pub async fn delete_folder_html(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Form(form): Form<DeleteForm>,
) -> Result<Html<String>, StatusCode> {
    ops::delete_folder(&state.doc_handle, &id).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    after_write(&state);
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
    ops::rename_folder(&state.doc_handle, &id, &form.title)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    after_write(&state);
    render_folder_response(&state, &form.current_folder_id, true, SortOrder::default())
}

/// # Errors
/// Returns `422 Unprocessable Entity` if cycle detection fails.
/// Returns `500 Internal Server Error` if template rendering fails.
pub async fn move_item_html(
    State(state): State<Arc<AppState>>,
    Form(form): Form<MoveItemForm>,
) -> Result<Html<String>, StatusCode> {
    ops::move_item(
        &state.doc_handle,
        &form.item,
        &form.source,
        &form.destination,
    )
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("cycle") || msg.contains("itself") || msg.contains("subtree") {
            StatusCode::UNPROCESSABLE_ENTITY
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    })?;
    after_write(&state);
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
    out.push((folder_id.to_string(), path.clone(), is_current));

    let child_folder_ids: Vec<&String> = folder
        .children
        .iter()
        .filter(|cid| store.folders.get(*cid).is_some_and(|f| !f.deleted))
        .collect();
    for child_id in child_folder_ids {
        collect_folder_paths(store, child_id, &path, exclude_ids, current_parent_id, out);
    }
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

#[derive(Deserialize)]
pub struct RevertForm {
    target_hash: String,
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
    let _ = write!(
        html,
        r#"<span class="favicon" style="background:{dc};width:28px;height:28px;font-size:16px">{dl}</span>"#,
    );
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

    // Tab bar
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
                "created".to_string()
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
    ops::revert_bookmark(&state.doc_handle, &id, &hash)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    after_write(&state);

    let store = read_store(&state.doc_handle)?;
    let Some(bm) = store.bookmarks.get(&id) else {
        return Ok(Html(render(&DetailEmptyTemplate)?).into_response());
    };

    let folder_id = find_folder_for_bookmark(&store, &id)
        .unwrap_or(&store.root_folder_id)
        .to_string();
    let sidebar_html = build_sidebar_html(&store, &folder_id);
    let (folders, bookmarks) = build_folder_items(&store, &folder_id, SortOrder::default());
    let breadcrumbs = build_breadcrumbs(&store, &folder_id);
    let folder_title = store
        .folders
        .get(&folder_id)
        .map_or("Bookmarks", |f| f.title.as_str())
        .to_string();
    let total_items = folders.len() + bookmarks.len();

    let detail = render(&DetailBookmarkTemplate {
        id: id.clone(),
        title: bm.title.clone(),
        url: bm.url.clone(),
        notes: bm.notes.clone(),
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

fn format_timestamp(ts: i64) -> String {
    let secs = if ts > 1_000_000_000_000 {
        ts / 1000
    } else {
        ts
    };
    chrono::DateTime::from_timestamp(secs, 0).map_or_else(
        || "unknown".to_string(),
        |dt| dt.format("%Y-%m-%d %H:%M").to_string(),
    )
}

/// # Errors
/// Returns `500 Internal Server Error` if multipart parsing or import fails.
pub async fn import_bookmarks_html(
    State(state): State<Arc<AppState>>,
    mut multipart: axum::extract::Multipart,
) -> Result<Html<String>, StatusCode> {
    let mut target = String::new();
    let mut folder_id = String::new();
    let mut file_content = Vec::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        match field.name() {
            Some("target") => {
                target = field.text().await.unwrap_or_default();
            }
            Some("folder_id") => {
                folder_id = field.text().await.unwrap_or_default();
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
    let target_folder_id = match target.as_str() {
        "current" if store.folders.contains_key(&folder_id) => folder_id,
        "new" => {
            let name = format!("Imported {}", chrono::Utc::now().format("%Y-%m-%d"));
            ops::create_folder(&state.doc_handle, &store.root_folder_id, &name)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        }
        _ => store.root_folder_id,
    };

    ops::import_items(&state.doc_handle, &target_folder_id, &items)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    after_write(&state);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_short_extracts_date_portion() {
        assert_eq!(date_short("2026-06-11T02:57:00+00:00"), "2026-06-11");
    }

    #[test]
    fn date_short_handles_short_input() {
        assert_eq!(date_short("2026-06"), "2026-06");
    }

    #[test]
    fn domain_color_is_deterministic() {
        let c1 = domain_color("https://example.com");
        let c2 = domain_color("https://example.com");
        assert_eq!(c1, c2);
    }

    #[test]
    fn domain_letter_extracts_first_char() {
        assert_eq!(domain_letter("https://example.com/page"), "E");
        assert_eq!(domain_letter("https://www.github.com"), "G");
        assert_eq!(domain_letter("http://rust-lang.org"), "R");
    }
}
