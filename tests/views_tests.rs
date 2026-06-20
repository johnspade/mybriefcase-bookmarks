#[expect(
    dead_code,
    reason = "shared test helpers; not all used by every test binary"
)]
mod common;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::{get, post};
use http_body_util::BodyExt;
use std::sync::Arc;
use tower::ServiceExt;

use common::new_initialized_doc;
use mybriefcase_bookmarks::views::SortOrder;
use mybriefcase_bookmarks::{handlers, history, ops, repo, state};

fn build_views_app() -> (Router, String) {
    let td = new_initialized_doc("test-views");
    let root_id = td.root_folder_id.clone();
    let sync_root = tempfile::TempDir::new().unwrap();
    let (sse_tx, _) = tokio::sync::broadcast::channel::<()>(16);
    let state = Arc::new(state::AppState {
        doc_handle: td.doc_handle,
        sync_root: sync_root.path().to_path_buf(),
        client_id: "test-views".to_string(),
        sse_tx,
        static_version: "test".to_string(),
        exporter: repo::Exporter::new(sync_root.path(), "test-views"),
    });
    let app = Router::new()
        .route("/folders/{id}/content", get(handlers::folder_content))
        .route("/folders/{id}/rename", post(handlers::rename_folder_html))
        .route("/bookmarks/{id}/detail", get(handlers::bookmark_detail))
        .route(
            "/bookmarks/{id}/edit-form",
            get(handlers::bookmark_edit_form),
        )
        .route("/bookmarks/{id}/edit", post(handlers::update_bookmark_html))
        .route(
            "/bookmarks/{id}/fetch-favicon",
            post(handlers::fetch_favicon_html),
        )
        .route("/bookmarks/new", post(handlers::create_bookmark_html))
        .route("/settings", get(handlers::settings_page))
        .route("/folder-options", get(handlers::folder_options))
        .route("/import", post(handlers::import_bookmarks_html))
        .route("/items/move", post(handlers::move_item_html))
        .route("/move-picker/{id}", get(handlers::move_picker_html))
        .with_state(state);
    std::mem::forget(td.temp_dir);
    std::mem::forget(sync_root);
    (app, root_id)
}

fn build_views_app_with_handle() -> (Router, String, automerge_repo::DocHandle) {
    let td = new_initialized_doc("test-views");
    let root_id = td.root_folder_id.clone();
    let doc_handle = td.doc_handle.clone();
    let sync_root = tempfile::TempDir::new().unwrap();
    let (sse_tx, _) = tokio::sync::broadcast::channel::<()>(16);
    let state = Arc::new(state::AppState {
        doc_handle: td.doc_handle,
        sync_root: sync_root.path().to_path_buf(),
        client_id: "test-views".to_string(),
        sse_tx,
        static_version: "test".to_string(),
        exporter: repo::Exporter::new(sync_root.path(), "test-views"),
    });
    let app = Router::new()
        .route("/folders/{id}/content", get(handlers::folder_content))
        .route("/folders/{id}/rename", post(handlers::rename_folder_html))
        .route("/folders/{id}/remove", post(handlers::delete_folder_html))
        .route("/bookmarks/{id}/detail", get(handlers::bookmark_detail))
        .route(
            "/bookmarks/{id}/edit-form",
            get(handlers::bookmark_edit_form),
        )
        .route("/bookmarks/{id}/edit", post(handlers::update_bookmark_html))
        .route(
            "/bookmarks/{id}/fetch-favicon",
            post(handlers::fetch_favicon_html),
        )
        .route(
            "/bookmarks/{id}/history",
            get(handlers::bookmark_history_html),
        )
        .route("/bookmarks/new", post(handlers::create_bookmark_html))
        .route("/settings", get(handlers::settings_page))
        .route("/folder-options", get(handlers::folder_options))
        .route("/import", post(handlers::import_bookmarks_html))
        .route("/items/move", post(handlers::move_item_html))
        .route("/move-picker/{id}", get(handlers::move_picker_html))
        .route("/sidebar", get(handlers::sidebar_only))
        .route("/search", get(handlers::search))
        .route("/favicons/{filename}", get(handlers::serve_favicon))
        .with_state(state);
    std::mem::forget(td.temp_dir);
    std::mem::forget(sync_root);
    (app, root_id, doc_handle)
}

async fn get_html(app: Router, uri: &str) -> (StatusCode, String) {
    let resp = app
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).to_string())
}

async fn post_form(app: Router, uri: &str, form_body: &str) -> (StatusCode, String) {
    let resp = app
        .oneshot(
            Request::post(uri)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(form_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).to_string())
}

async fn create_bookmark(app: &Router, root_id: &str) -> String {
    let body = format!("folder_id={root_id}&url=https%3A%2F%2Fexample.com&title=Test");
    let resp = app
        .clone()
        .oneshot(
            Request::post("/bookmarks/new")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8_lossy(&bytes).to_string();
    // Extract bookmark ID from the response HTML (it contains hx-get="/bookmarks/{id}/detail")
    let marker = "hx-get=\"/bookmarks/";
    let start = html.find(marker).expect("bookmark id in response") + marker.len();
    let end = html[start..].find('/').unwrap() + start;
    html[start..end].to_string()
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn detail_view_shows_time_elements_with_timestamps() {
    let (app, root_id) = build_views_app();
    let bm_id = create_bookmark(&app, &root_id).await;

    let (status, html) = get_html(app, &format!("/bookmarks/{bm_id}/detail")).await;
    assert_eq!(status, StatusCode::OK);

    assert!(
        html.contains("<time datetime="),
        "detail view should contain <time> elements"
    );
    assert!(
        html.contains("data-format=\"long\""),
        "detail view should use long date format"
    );
    assert!(
        html.contains("Date added"),
        "detail view should show 'Date added' label"
    );
    assert!(
        html.contains("Last modified"),
        "detail view should show 'Last modified' label"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn edit_form_does_not_contain_date_field() {
    let (app, root_id) = build_views_app();
    let bm_id = create_bookmark(&app, &root_id).await;

    let (status, html) = get_html(app, &format!("/bookmarks/{bm_id}/edit-form")).await;
    assert_eq!(status, StatusCode::OK);

    assert!(
        !html.contains("Date added"),
        "edit form should not contain date field"
    );
    assert!(
        !html.contains("readonly"),
        "edit form should not have readonly inputs"
    );
    assert!(
        html.contains("name=\"title\""),
        "edit form should have title"
    );
    assert!(html.contains("name=\"url\""), "edit form should have url");
    assert!(
        html.contains("name=\"notes\""),
        "edit form should have notes"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn folder_content_shows_time_element_for_bookmark_date() {
    let (app, root_id) = build_views_app();
    create_bookmark(&app, &root_id).await;

    let (status, html) = get_html(app, &format!("/folders/{root_id}/content")).await;
    assert_eq!(status, StatusCode::OK);

    assert!(
        html.contains("<time datetime="),
        "folder content should contain <time> element for bookmark date"
    );
    assert!(
        html.contains("data-format=\"short\""),
        "folder content should use short date format"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn update_bookmark_changes_updated_at() {
    let (app, root_id) = build_views_app();
    let bm_id = create_bookmark(&app, &root_id).await;

    // Small sleep to ensure updated_at will differ
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let (status, html) = post_form(
        app,
        &format!("/bookmarks/{bm_id}/edit"),
        "title=Updated+Title&url=https%3A%2F%2Fexample.com&notes=",
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The response HTML should contain two different timestamps in <time> elements
    let time_count = html.matches("<time datetime=").count();
    assert!(
        time_count >= 2,
        "update response should contain at least 2 <time> elements (detail + list), got {time_count}"
    );
}

// ─── Sort order tests ──────────────────────────────

#[test]
fn sort_order_from_param_defaults_to_name_asc() {
    assert_eq!(SortOrder::from_param(None), SortOrder::NameAsc);
    assert_eq!(SortOrder::from_param(Some("")), SortOrder::NameAsc);
    assert_eq!(SortOrder::from_param(Some("invalid")), SortOrder::NameAsc);
    assert_eq!(SortOrder::from_param(Some("name_asc")), SortOrder::NameAsc);
}

#[test]
fn sort_order_from_param_parses_all_variants() {
    assert_eq!(
        SortOrder::from_param(Some("name_desc")),
        SortOrder::NameDesc
    );
    assert_eq!(
        SortOrder::from_param(Some("date_desc")),
        SortOrder::DateDesc
    );
    assert_eq!(SortOrder::from_param(Some("date_asc")), SortOrder::DateAsc);
}

async fn create_bookmark_with_title(app: &Router, root_id: &str, title: &str) -> String {
    let encoded_title: String = title
        .bytes()
        .flat_map(|b| {
            if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' || b == b'~' {
                vec![b as char]
            } else {
                format!("%{b:02X}").chars().collect()
            }
        })
        .collect();
    let body = format!(
        "folder_id={}&url=https%3A%2F%2F{}.example.com&title={}",
        root_id,
        title.to_lowercase().replace(' ', "-"),
        encoded_title
    );
    let resp = app
        .clone()
        .oneshot(
            Request::post("/bookmarks/new")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8_lossy(&bytes).to_string();
    let marker = "hx-get=\"/bookmarks/";
    let start = html.find(marker).expect("bookmark id in response") + marker.len();
    let end = html[start..].find('/').unwrap() + start;
    html[start..end].to_string()
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn folder_content_sorts_by_name_asc_by_default() {
    let (app, root_id) = build_views_app();
    create_bookmark_with_title(&app, &root_id, "Zebra").await;
    create_bookmark_with_title(&app, &root_id, "Apple").await;
    create_bookmark_with_title(&app, &root_id, "Mango").await;

    let (status, html) = get_html(app, &format!("/folders/{root_id}/content")).await;
    assert_eq!(status, StatusCode::OK);

    let apple_pos = html.find("Apple").unwrap();
    let mango_pos = html.find("Mango").unwrap();
    let zebra_pos = html.find("Zebra").unwrap();
    assert!(
        apple_pos < mango_pos && mango_pos < zebra_pos,
        "Default sort should be name A→Z"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn folder_content_sorts_by_name_desc() {
    let (app, root_id) = build_views_app();
    create_bookmark_with_title(&app, &root_id, "Zebra").await;
    create_bookmark_with_title(&app, &root_id, "Apple").await;
    create_bookmark_with_title(&app, &root_id, "Mango").await;

    let (status, html) = get_html(app, &format!("/folders/{root_id}/content?sort=name_desc")).await;
    assert_eq!(status, StatusCode::OK);

    let apple_pos = html.find("Apple").unwrap();
    let mango_pos = html.find("Mango").unwrap();
    let zebra_pos = html.find("Zebra").unwrap();
    assert!(
        zebra_pos < mango_pos && mango_pos < apple_pos,
        "sort=name_desc should be Z→A"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn folder_content_sorts_by_date_desc() {
    let (app, root_id) = build_views_app();
    // Created in order: Zebra, Apple, Mango — Mango is newest
    create_bookmark_with_title(&app, &root_id, "Zebra").await;
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    create_bookmark_with_title(&app, &root_id, "Apple").await;
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    create_bookmark_with_title(&app, &root_id, "Mango").await;

    let (status, html) = get_html(app, &format!("/folders/{root_id}/content?sort=date_desc")).await;
    assert_eq!(status, StatusCode::OK);

    let apple_pos = html.find("Apple").unwrap();
    let mango_pos = html.find("Mango").unwrap();
    let zebra_pos = html.find("Zebra").unwrap();
    assert!(
        mango_pos < apple_pos && apple_pos < zebra_pos,
        "sort=date_desc should show newest first (Mango < Apple < Zebra)"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn folder_content_sorts_by_date_asc() {
    let (app, root_id) = build_views_app();
    create_bookmark_with_title(&app, &root_id, "Zebra").await;
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    create_bookmark_with_title(&app, &root_id, "Apple").await;
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    create_bookmark_with_title(&app, &root_id, "Mango").await;

    let (status, html) = get_html(app, &format!("/folders/{root_id}/content?sort=date_asc")).await;
    assert_eq!(status, StatusCode::OK);

    let apple_pos = html.find("Apple").unwrap();
    let mango_pos = html.find("Mango").unwrap();
    let zebra_pos = html.find("Zebra").unwrap();
    assert!(
        zebra_pos < apple_pos && apple_pos < mango_pos,
        "sort=date_asc should show oldest first (Zebra < Apple < Mango)"
    );
}

// ─── Move feature tests ──────────────────────────────

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn move_item_same_folder_noop() {
    let (app, root_id, doc) = build_views_app_with_handle();
    let bm_id = ops::add_bookmark(&doc, &root_id, "https://example.com", "Test").unwrap();

    let body = format!("item_id={bm_id}&from_folder_id={root_id}&to_folder_id={root_id}");
    let (status, _) = post_form(app, "/items/move", &body).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn move_item_into_itself_fails() {
    let (app, root_id, doc) = build_views_app_with_handle();
    let folder_a = ops::create_folder(&doc, &root_id, "A").unwrap();

    let body = format!("item_id={folder_a}&from_folder_id={root_id}&to_folder_id={folder_a}");
    let (status, _) = post_form(app, "/items/move", &body).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn move_item_into_descendant_fails() {
    let (app, root_id, doc) = build_views_app_with_handle();
    let folder_a = ops::create_folder(&doc, &root_id, "A").unwrap();
    let folder_b = ops::create_folder(&doc, &folder_a, "B").unwrap();
    let folder_c = ops::create_folder(&doc, &folder_b, "C").unwrap();

    let body = format!("item_id={folder_a}&from_folder_id={root_id}&to_folder_id={folder_c}");
    let (status, _) = post_form(app, "/items/move", &body).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn move_folder_between_siblings() {
    let (app, root_id, doc) = build_views_app_with_handle();
    let folder_a = ops::create_folder(&doc, &root_id, "A").unwrap();
    let folder_b = ops::create_folder(&doc, &root_id, "B").unwrap();

    let body = format!("item_id={folder_b}&from_folder_id={root_id}&to_folder_id={folder_a}");
    let (status, _) = post_form(app, "/items/move", &body).await;
    assert_eq!(status, StatusCode::OK);

    let store: mybriefcase_bookmarks::model::BookmarkStore =
        doc.with_doc(|d| autosurgeon::hydrate(d).unwrap());
    let a = store.folders.get(&folder_a).unwrap();
    assert!(a.children.contains(&folder_b));
    let root = store.folders.get(&root_id).unwrap();
    assert!(!root.children.contains(&folder_b));
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn move_folder_to_root() {
    let (app, root_id, doc) = build_views_app_with_handle();
    let folder_a = ops::create_folder(&doc, &root_id, "A").unwrap();
    let folder_b = ops::create_folder(&doc, &folder_a, "B").unwrap();

    let body = format!("item_id={folder_b}&from_folder_id={folder_a}&to_folder_id={root_id}");
    let (status, _) = post_form(app, "/items/move", &body).await;
    assert_eq!(status, StatusCode::OK);

    let store: mybriefcase_bookmarks::model::BookmarkStore =
        doc.with_doc(|d| autosurgeon::hydrate(d).unwrap());
    let root = store.folders.get(&root_id).unwrap();
    assert!(root.children.contains(&folder_b));
    let a = store.folders.get(&folder_a).unwrap();
    assert!(!a.children.contains(&folder_b));
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn move_picker_returns_html() {
    let (app, root_id, doc) = build_views_app_with_handle();
    let bm_id = ops::add_bookmark(&doc, &root_id, "https://example.com", "Test").unwrap();
    ops::create_folder(&doc, &root_id, "Target Folder").unwrap();

    let (status, html) = get_html(app, &format!("/move-picker/{bm_id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Move to"));
    assert!(html.contains("Target Folder"));
    assert!(html.contains("Bookmarks"));
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn move_picker_excludes_self_for_folder() {
    let (app, root_id, doc) = build_views_app_with_handle();
    let folder_a = ops::create_folder(&doc, &root_id, "FolderA").unwrap();

    let (status, html) = get_html(app, &format!("/move-picker/{folder_a}")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !html.contains(&format!(r#"name="to_folder_id" value="{folder_a}""#)),
        "folder should not appear as a move destination"
    );
    assert!(!html.contains("FolderA"));
    assert!(html.contains("Bookmarks"));
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn move_picker_excludes_descendants() {
    let (app, root_id, doc) = build_views_app_with_handle();
    let folder_a = ops::create_folder(&doc, &root_id, "A-Parent").unwrap();
    let folder_b = ops::create_folder(&doc, &folder_a, "B-Child").unwrap();
    let _folder_c = ops::create_folder(&doc, &folder_b, "C-Grandchild").unwrap();

    let (status, html) = get_html(app, &format!("/move-picker/{folder_a}")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!html.contains("A-Parent"));
    assert!(!html.contains("B-Child"));
    assert!(!html.contains("C-Grandchild"));
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn move_picker_shows_current_parent() {
    let (app, root_id, doc) = build_views_app_with_handle();
    let bm_id = ops::add_bookmark(&doc, &root_id, "https://example.com", "Test").unwrap();

    let (status, html) = get_html(app, &format!("/move-picker/{bm_id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("(current)"));
}

// ─── Settings & bookmarklet page tests ────────────────

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn settings_page_renders() {
    let (app, _root_id) = build_views_app();

    let (status, html) = get_html(app, "/settings").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        html.contains("Bookmarklet"),
        "should have bookmarklet section"
    );
    assert!(html.contains("Import"), "should have import section");
    assert!(html.contains("Export"), "should have export section");
    assert!(
        html.contains("buildBookmarklet"),
        "should reference bookmarklet JS"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn create_bookmark_returns_folder_html() {
    let (app, root_id) = build_views_app();

    let body = format!("folder_id={root_id}&url=https%3A%2F%2Fexample.com&title=Test");
    let resp = app
        .oneshot(
            Request::post("/bookmarks/new")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8_lossy(&bytes).to_string();
    assert!(html.contains("Test"), "should return folder content HTML");
}

// ─── Folder options endpoint tests ────────────────────

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn folder_options_returns_option_elements() {
    let (app, root_id) = build_views_app();

    let (status, html) = get_html(app, "/folder-options").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<option"), "should return option elements");
    assert!(html.contains(&root_id), "should contain the root folder id");
    assert!(
        html.contains("Bookmarks"),
        "should contain the root folder name"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn folder_options_includes_subfolders() {
    let (app, root_id, doc_handle) = build_views_app_with_handle();

    ops::create_folder(&doc_handle, &root_id, "Sub Folder").unwrap();

    let (status, html) = get_html(app, "/folder-options").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Sub Folder"), "should list the subfolder");
}

// ─── Import endpoint tests ────────────────────────────

fn multipart_body(target: &str, file_content: &str) -> (String, Vec<u8>) {
    let boundary = "----TestBoundary123";
    let mut body = Vec::new();
    body.extend_from_slice(format!("------TestBoundary123\r\nContent-Disposition: form-data; name=\"target\"\r\n\r\n{target}\r\n").as_bytes());
    body.extend_from_slice(format!("------TestBoundary123\r\nContent-Disposition: form-data; name=\"file\"; filename=\"bookmarks.html\"\r\nContent-Type: text/html\r\n\r\n{file_content}\r\n").as_bytes());
    body.extend_from_slice(b"------TestBoundary123--\r\n");
    (format!("multipart/form-data; boundary={boundary}"), body)
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn import_to_root_creates_bookmarks() {
    let (app, _root_id) = build_views_app();

    let html_file = r#"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<DL><p>
<DT><A HREF="https://example.com">Example</A>
<DT><A HREF="https://rust-lang.org">Rust</A>
</DL>"#;

    let (content_type, body) = multipart_body("root", html_file);
    let resp = app
        .oneshot(
            Request::post("/import")
                .header("content-type", content_type)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8_lossy(&bytes).to_string();
    assert!(html.contains("Example"), "should contain imported bookmark");
    assert!(
        html.contains("Rust"),
        "should contain second imported bookmark"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn import_to_new_folder_creates_subfolder() {
    let (app, _root_id) = build_views_app();

    let html_file = r#"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<DL><p>
<DT><A HREF="https://example.com">Example</A>
</DL>"#;

    let (content_type, body) = multipart_body("new", html_file);
    let resp = app
        .oneshot(
            Request::post("/import")
                .header("content-type", content_type)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8_lossy(&bytes).to_string();
    assert!(html.contains("Example"), "should contain imported bookmark");
    assert!(
        html.contains("Imported"),
        "should show imported folder context"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn import_empty_file_returns_bad_request() {
    let (app, _root_id) = build_views_app();

    let (content_type, body) = multipart_body("root", "");
    let resp = app
        .oneshot(
            Request::post("/import")
                .header("content-type", content_type)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn import_invalid_html_returns_bad_request() {
    let (app, _root_id) = build_views_app();

    let (content_type, body) = multipart_body("root", "<p>Not a bookmarks file</p>");
    let resp = app
        .oneshot(
            Request::post("/import")
                .header("content-type", content_type)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn rename_folder_updates_title() {
    let (app, root_id, doc) = build_views_app_with_handle();
    let folder_id = ops::create_folder(&doc, &root_id, "Original Name").unwrap();

    let form_body = format!("title=Renamed+Folder&current_folder_id={root_id}");
    let (status, html) = post_form(app, &format!("/folders/{folder_id}/rename"), &form_body).await;

    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Renamed Folder"));
    assert!(!html.contains("Original Name"));
}

// ─── Favicon management tests ────────────────────────

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn edit_bookmark_saves_favicon_field() {
    let (app, root_id, doc) = build_views_app_with_handle();
    let bm_id = ops::add_bookmark(&doc, &root_id, "https://example.com", "Test").unwrap();

    let (status, _) = post_form(
        app,
        &format!("/bookmarks/{bm_id}/edit"),
        "title=Test&url=https%3A%2F%2Fexample.com&notes=&favicon=abc123.png",
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let store: mybriefcase_bookmarks::model::BookmarkStore =
        doc.with_doc(|d| autosurgeon::hydrate(d).unwrap());
    let bm = store.bookmarks.get(&bm_id).unwrap();
    assert_eq!(bm.favicon, Some("abc123.png".to_string()));
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn edit_bookmark_clears_favicon() {
    let (app, root_id, doc) = build_views_app_with_handle();
    let bm_id = ops::add_bookmark(&doc, &root_id, "https://example.com", "Test").unwrap();
    ops::update_favicon(&doc, &bm_id, Some("existing.png")).unwrap();

    let (status, _) = post_form(
        app,
        &format!("/bookmarks/{bm_id}/edit"),
        "title=Test&url=https%3A%2F%2Fexample.com&notes=&favicon=",
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let store: mybriefcase_bookmarks::model::BookmarkStore =
        doc.with_doc(|d| autosurgeon::hydrate(d).unwrap());
    let bm = store.bookmarks.get(&bm_id).unwrap();
    assert_eq!(bm.favicon, None);
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn edit_bookmark_favicon_propagates_to_same_url() {
    let (app, root_id, doc) = build_views_app_with_handle();
    let bm1 = ops::add_bookmark(&doc, &root_id, "https://example.com", "One").unwrap();
    let bm2 = ops::add_bookmark(&doc, &root_id, "https://example.com", "Two").unwrap();

    let (status, _) = post_form(
        app,
        &format!("/bookmarks/{bm1}/edit"),
        "title=One&url=https%3A%2F%2Fexample.com&notes=&favicon=new-icon.png",
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let store: mybriefcase_bookmarks::model::BookmarkStore =
        doc.with_doc(|d| autosurgeon::hydrate(d).unwrap());
    assert_eq!(
        store.bookmarks.get(&bm1).unwrap().favicon,
        Some("new-icon.png".to_string())
    );
    assert_eq!(
        store.bookmarks.get(&bm2).unwrap().favicon,
        Some("new-icon.png".to_string())
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn edit_bookmark_favicon_delete_does_not_propagate() {
    let (app, root_id, doc) = build_views_app_with_handle();
    let bm1 = ops::add_bookmark(&doc, &root_id, "https://example.com", "One").unwrap();
    let bm2 = ops::add_bookmark(&doc, &root_id, "https://example.com", "Two").unwrap();
    ops::update_favicon(&doc, &bm1, Some("shared.png")).unwrap();
    ops::update_favicon(&doc, &bm2, Some("shared.png")).unwrap();

    let (status, _) = post_form(
        app,
        &format!("/bookmarks/{bm1}/edit"),
        "title=One&url=https%3A%2F%2Fexample.com&notes=&favicon=",
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let store: mybriefcase_bookmarks::model::BookmarkStore =
        doc.with_doc(|d| autosurgeon::hydrate(d).unwrap());
    assert_eq!(store.bookmarks.get(&bm1).unwrap().favicon, None);
    assert_eq!(
        store.bookmarks.get(&bm2).unwrap().favicon,
        Some("shared.png".to_string())
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn fetch_favicon_endpoint_returns_partial_with_img() {
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let icon_bytes: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00];

    let icon_clone = icon_bytes.clone();
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        loop {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let req = String::from_utf8_lossy(&buf);
            let response = if req.contains("GET /favicon.ico") {
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\n\r\n",
                    icon_clone.len()
                )
            } else {
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 50\r\n\r\n<html><head><link rel=\"icon\" href=\"/favicon.ico\"></head></html>".to_string()
            };
            let _ = stream.write_all(response.as_bytes());
            if req.contains("GET /favicon.ico") {
                let _ = stream.write_all(&icon_clone);
            }
            let _ = stream.flush();
        }
    });

    let (app, root_id, doc) = build_views_app_with_handle();
    let url = format!("http://127.0.0.1:{port}/page");
    let encoded_url = url.replace(':', "%3A").replace('/', "%2F");
    let body = format!("folder_id={root_id}&url={encoded_url}&title=Test");
    let resp = app
        .clone()
        .oneshot(
            Request::post("/bookmarks/new")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let html_resp = String::from_utf8_lossy(&bytes).to_string();
    let marker = "hx-get=\"/bookmarks/";
    let start = html_resp.find(marker).unwrap() + marker.len();
    let end = html_resp[start..].find('/').unwrap() + start;
    let bm_id = &html_resp[start..end];

    let (status, html) = post_form(app, &format!("/bookmarks/{bm_id}/fetch-favicon"), "").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<img"), "response should contain an img tag");
    assert!(
        html.contains("name=\"favicon\""),
        "response should contain the hidden favicon input"
    );

    let store: mybriefcase_bookmarks::model::BookmarkStore =
        doc.with_doc(|d| autosurgeon::hydrate(d).unwrap());
    let bm = store.bookmarks.get(bm_id).unwrap();
    assert!(bm.favicon.is_some(), "favicon should be stored");
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn fetch_favicon_endpoint_error_returns_inline_message() {
    let (app, root_id, doc) = build_views_app_with_handle();
    let bm_id = ops::add_bookmark(&doc, &root_id, "http://127.0.0.1:1/unreachable", "Bad").unwrap();

    let (status, html) = post_form(app, &format!("/bookmarks/{bm_id}/fetch-favicon"), "").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        html.contains("Could not fetch favicon"),
        "should show inline error"
    );
    assert!(
        html.contains("name=\"favicon\""),
        "should still contain hidden input"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn edit_form_contains_favicon_section() {
    let (app, root_id, doc) = build_views_app_with_handle();
    let bm_id = ops::add_bookmark(&doc, &root_id, "https://example.com", "Test").unwrap();
    ops::update_favicon(&doc, &bm_id, Some("test-icon.png")).unwrap();

    let (status, html) = get_html(app, &format!("/bookmarks/{bm_id}/edit-form")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        html.contains("name=\"favicon\""),
        "edit form should have hidden favicon input"
    );
    assert!(
        html.contains("test-icon.png"),
        "edit form should show current favicon"
    );
    assert!(
        html.contains("fetch-favicon"),
        "edit form should have refetch button"
    );
    assert!(html.contains("Refetch"), "should have Refetch label");
    assert!(html.contains("Delete"), "should have Delete label");
}

// ─── HTML structural assertions ─────────────────────

/// Helper: parse HTML and select elements matching a CSS selector.
fn select_all(html: &str, selector: &str) -> Vec<scraper::ElementRef<'static>> {
    // We leak the Html to get a 'static lifetime for ElementRef — acceptable in tests.
    let document = Box::leak(Box::new(scraper::Html::parse_fragment(html)));
    let sel = scraper::Selector::parse(selector).unwrap();
    document.select(&sel).collect()
}

/// Helper: check that an element has a specific attribute value.
fn has_attr(el: &scraper::ElementRef, attr: &str, value: &str) -> bool {
    el.value().attr(attr) == Some(value)
}

// ─── HTMX attribute assertions ──────────────────────

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn folder_content_htmx_wiring_breadcrumbs() {
    let (app, root_id, doc) = build_views_app_with_handle();
    ops::create_folder(&doc, &root_id, "Child").unwrap();

    // Navigate to child to get breadcrumbs with links
    let (status, html) = get_html(app.clone(), &format!("/folders/{root_id}/content")).await;
    assert_eq!(status, StatusCode::OK);

    // Root folder with no subfolders in path won't have clickable breadcrumbs,
    // but the structure should still include the breadcrumb container.
    let breadcrumb = select_all(&html, "#breadcrumb");
    assert!(
        !breadcrumb.is_empty(),
        "breadcrumb container with id should exist"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn folder_content_htmx_wiring_bookmark_items() {
    let (app, root_id) = build_views_app();
    create_bookmark(&app, &root_id).await;

    let (_, html) = get_html(app, &format!("/folders/{root_id}/content")).await;

    // Bookmark list items should have hx-get pointing to detail
    let items = select_all(&html, ".list-item[hx-get]");
    assert!(!items.is_empty(), "bookmark items should have hx-get");
    for item in &items {
        let hx_get = item.value().attr("hx-get").unwrap();
        assert!(
            hx_get.contains("/bookmarks/") && hx_get.contains("/detail"),
            "hx-get should point to bookmark detail: {hx_get}"
        );
        assert!(
            has_attr(item, "hx-target", "#detail-body"),
            "hx-target should be #detail-body"
        );
        assert!(
            has_attr(item, "hx-swap", "innerHTML"),
            "hx-swap should be innerHTML"
        );
    }
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn folder_content_htmx_wiring_folder_items() {
    let (app, root_id, doc) = build_views_app_with_handle();
    ops::create_folder(&doc, &root_id, "SubFolder").unwrap();

    let (_, html) = get_html(app, &format!("/folders/{root_id}/content")).await;

    // Folder list items navigate via hx-get
    let folder_items = select_all(&html, ".list-item[hx-get][hx-push-url]");
    assert!(
        !folder_items.is_empty(),
        "folder items should have hx-get and hx-push-url"
    );
    for item in &folder_items {
        let hx_get = item.value().attr("hx-get").unwrap();
        assert!(
            hx_get.starts_with("/folders/"),
            "folder hx-get should target /folders/: {hx_get}"
        );
        assert!(
            has_attr(item, "hx-target", "#folder-content"),
            "folder hx-target should be #folder-content"
        );
        assert!(
            has_attr(item, "hx-swap", "innerHTML"),
            "folder hx-swap should be innerHTML"
        );
    }
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn folder_content_htmx_delete_button_wiring() {
    let (app, root_id) = build_views_app();
    create_bookmark(&app, &root_id).await;

    let (_, html) = get_html(app, &format!("/folders/{root_id}/content")).await;

    // Delete buttons should use hx-post, hx-target, hx-include, hx-confirm
    let delete_btns = select_all(&html, "button[hx-post][hx-confirm]");
    assert!(
        !delete_btns.is_empty(),
        "should have delete buttons with hx-post and hx-confirm"
    );
    for btn in &delete_btns {
        let hx_post = btn.value().attr("hx-post").unwrap();
        assert!(
            hx_post.contains("/remove"),
            "delete hx-post should point to /remove endpoint: {hx_post}"
        );
        assert!(
            has_attr(btn, "hx-target", "#folder-content"),
            "delete should target #folder-content"
        );
        assert!(
            has_attr(btn, "hx-swap", "innerHTML"),
            "delete should use innerHTML swap"
        );
        assert!(
            has_attr(btn, "hx-include", "#current-folder-id"),
            "delete should include #current-folder-id"
        );
    }
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn detail_view_htmx_edit_button() {
    let (app, root_id) = build_views_app();
    let bm_id = create_bookmark(&app, &root_id).await;

    let (_, html) = get_html(app, &format!("/bookmarks/{bm_id}/detail")).await;

    // Edit button should trigger loading edit form into modal
    let edit_btns = select_all(&html, "button[hx-get]");
    let edit_btn = edit_btns
        .iter()
        .find(|b| {
            b.value()
                .attr("hx-get")
                .is_some_and(|v| v.contains("/edit-form"))
        })
        .expect("should have edit button with hx-get pointing to edit-form");
    assert!(
        has_attr(edit_btn, "hx-target", "#edit-modal-body"),
        "edit button should target #edit-modal-body"
    );
    assert!(
        has_attr(edit_btn, "hx-swap", "innerHTML"),
        "edit button should use innerHTML swap"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn detail_view_htmx_delete_button() {
    let (app, root_id) = build_views_app();
    let bm_id = create_bookmark(&app, &root_id).await;

    let (_, html) = get_html(app, &format!("/bookmarks/{bm_id}/detail")).await;

    let delete_btns = select_all(&html, "button[hx-post][hx-confirm]");
    assert!(
        !delete_btns.is_empty(),
        "detail view should have a delete button"
    );
    let delete_btn = &delete_btns[0];
    let hx_post = delete_btn.value().attr("hx-post").unwrap();
    assert!(
        hx_post.contains("/remove"),
        "delete hx-post should point to /remove: {hx_post}"
    );
    assert!(
        has_attr(delete_btn, "hx-include", "#current-folder-id"),
        "delete should include current-folder-id"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn detail_view_htmx_history_button() {
    let (app, root_id) = build_views_app();
    let bm_id = create_bookmark(&app, &root_id).await;

    let (_, html) = get_html(app, &format!("/bookmarks/{bm_id}/detail")).await;

    let history_btns = select_all(&html, "button[hx-get]");
    let history_btn = history_btns
        .iter()
        .find(|b| {
            b.value()
                .attr("hx-get")
                .is_some_and(|v| v.contains("/history"))
        })
        .expect("should have history button with hx-get");
    assert!(
        has_attr(history_btn, "hx-target", "#detail-body"),
        "history button should target #detail-body"
    );
    assert!(
        has_attr(history_btn, "hx-swap", "innerHTML"),
        "history button should use innerHTML swap"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn edit_form_htmx_submit_wiring() {
    let (app, root_id) = build_views_app();
    let bm_id = create_bookmark(&app, &root_id).await;

    let (_, html) = get_html(app, &format!("/bookmarks/{bm_id}/edit-form")).await;

    // The form itself should have hx-post, hx-target, hx-swap
    let forms = select_all(&html, "form[hx-post]");
    assert!(!forms.is_empty(), "edit form should have hx-post");
    let form = &forms[0];
    let hx_post = form.value().attr("hx-post").unwrap();
    assert!(
        hx_post.contains("/edit"),
        "form hx-post should point to /edit: {hx_post}"
    );
    assert!(
        has_attr(form, "hx-target", "#detail-body"),
        "form should target #detail-body"
    );
    assert!(
        has_attr(form, "hx-swap", "innerHTML"),
        "form should use innerHTML swap"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn edit_form_htmx_fetch_favicon_wiring() {
    let (app, root_id) = build_views_app();
    let bm_id = create_bookmark(&app, &root_id).await;

    let (_, html) = get_html(app, &format!("/bookmarks/{bm_id}/edit-form")).await;

    // Refetch button should have hx-post, hx-target, hx-swap
    let refetch_btns = select_all(&html, "button[hx-post]");
    let refetch_btn = refetch_btns
        .iter()
        .find(|b| {
            b.value()
                .attr("hx-post")
                .is_some_and(|v| v.contains("/fetch-favicon"))
        })
        .expect("should have refetch button with hx-post");
    assert!(
        has_attr(refetch_btn, "hx-target", "#favicon-preview"),
        "refetch should target #favicon-preview"
    );
    assert!(
        has_attr(refetch_btn, "hx-swap", "outerHTML"),
        "refetch should use outerHTML swap"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn settings_page_htmx_import_form() {
    let (app, _root_id) = build_views_app();

    let (_, html) = get_html(app, "/settings").await;

    let forms = select_all(&html, "form[hx-post]");
    let import_form = forms
        .iter()
        .find(|f| {
            f.value()
                .attr("hx-post")
                .is_some_and(|v| v.contains("/import"))
        })
        .expect("settings page should have import form with hx-post");
    assert!(
        has_attr(import_form, "hx-swap", "none"),
        "import form should use hx-swap=none"
    );
    assert!(
        has_attr(import_form, "hx-encoding", "multipart/form-data"),
        "import form should set hx-encoding for multipart"
    );
}

// ─── ARIA attribute assertions ──────────────────────

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn folder_content_aria_attributes() {
    let (app, root_id) = build_views_app();
    create_bookmark(&app, &root_id).await;

    let (_, html) = get_html(app, &format!("/folders/{root_id}/content")).await;

    // View settings button should have aria-label and aria-haspopup
    let view_btns = select_all(&html, "button[aria-label=\"View settings\"]");
    assert!(
        !view_btns.is_empty(),
        "should have view settings button with aria-label"
    );
    assert!(
        has_attr(&view_btns[0], "aria-haspopup", "true"),
        "view settings button should have aria-haspopup=true"
    );

    // SVG icons should be aria-hidden
    let hidden_svgs = select_all(&html, "svg[aria-hidden=\"true\"]");
    assert!(
        !hidden_svgs.is_empty(),
        "decorative SVGs should have aria-hidden=true"
    );

    // "More actions" buttons should have aria-label
    let action_btns = select_all(&html, "button[aria-label=\"More actions\"]");
    assert!(
        !action_btns.is_empty(),
        "context menu buttons should have aria-label='More actions'"
    );

    // "Open bookmark" button should have aria-label
    let open_btns = select_all(&html, "button[aria-label=\"Open bookmark\"]");
    assert!(
        !open_btns.is_empty(),
        "open button should have aria-label='Open bookmark'"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn folder_content_grid_view_aria() {
    let (app, root_id) = build_views_app();
    create_bookmark(&app, &root_id).await;

    let (_, html) = get_html(app, &format!("/folders/{root_id}/content")).await;

    // Grid view buttons for list/grid should have aria-label
    let list_btn = select_all(&html, "button[aria-label=\"List view\"]");
    assert!(
        !list_btn.is_empty(),
        "should have list view button with aria-label"
    );
    let grid_btn = select_all(&html, "button[aria-label=\"Grid view\"]");
    assert!(
        !grid_btn.is_empty(),
        "should have grid view button with aria-label"
    );
}

// ─── Alpine.js binding assertions ───────────────────

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn folder_content_alpine_view_settings() {
    let (app, root_id) = build_views_app();
    create_bookmark(&app, &root_id).await;

    let (_, html) = get_html(app, &format!("/folders/{root_id}/content")).await;

    // View settings container should have x-data for open state
    let xdata_els = select_all(&html, ".view-settings[x-data]");
    assert!(
        !xdata_els.is_empty(),
        "view-settings should have x-data binding"
    );

    // Settings popover should have x-show for visibility toggle
    let popover = select_all(&html, ".settings-popover[x-show]");
    assert!(
        !popover.is_empty(),
        "settings popover should have x-show binding"
    );

    // Items list and grid should have x-show for view mode switching
    let items_list = select_all(&html, ".items-list[x-show]");
    assert!(
        !items_list.is_empty(),
        "items-list should have x-show for view mode"
    );
    let items_grid = select_all(&html, ".items-grid[x-show]");
    assert!(
        !items_grid.is_empty(),
        "items-grid should have x-show for view mode"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn folder_content_alpine_context_menus() {
    let (app, root_id) = build_views_app();
    create_bookmark(&app, &root_id).await;

    let (_, html) = get_html(app, &format!("/folders/{root_id}/content")).await;

    // Item action containers should have x-data for menu state
    let action_xdata = select_all(&html, ".item-actions[x-data]");
    assert!(
        !action_xdata.is_empty(),
        "item-actions should have x-data for menu state"
    );

    // Item menus should have x-show for visibility
    let menus = select_all(&html, ".item-menu[x-show]");
    assert!(!menus.is_empty(), "item-menu should have x-show binding");
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn edit_form_alpine_bindings() {
    let (app, root_id) = build_views_app();
    let bm_id = create_bookmark(&app, &root_id).await;

    let (_, html) = get_html(app, &format!("/bookmarks/{bm_id}/edit-form")).await;

    // Form should have x-init for htmx processing
    let forms = select_all(&html, "form[x-init]");
    assert!(
        !forms.is_empty(),
        "edit form should have x-init for htmx.process"
    );

    // Favicon preview container should have id for targeting
    let preview = select_all(&html, "#favicon-preview");
    assert!(
        !preview.is_empty(),
        "favicon-preview element should exist for hx-target"
    );
}

// ─── ID/hx-target cross-reference assertions ────────

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn folder_content_id_targets_exist() {
    let (app, root_id) = build_views_app();
    create_bookmark(&app, &root_id).await;

    let (_, html) = get_html(app, &format!("/folders/{root_id}/content")).await;

    // Elements that use hx-target="#folder-content" — the target exists in the
    // parent page (base.html), not this partial. But hx-include="#current-folder-id"
    // references an element that MUST exist in this partial.
    let current_folder_input = select_all(&html, "#current-folder-id");
    assert!(
        !current_folder_input.is_empty(),
        "current-folder-id input must exist (referenced by hx-include)"
    );

    // Verify the hidden input has the correct folder_id value
    let input = &current_folder_input[0];
    assert_eq!(
        input.value().attr("value").unwrap(),
        root_id,
        "current-folder-id should contain the folder id"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn edit_form_id_targets_exist() {
    let (app, root_id) = build_views_app();
    let bm_id = create_bookmark(&app, &root_id).await;

    let (_, html) = get_html(app, &format!("/bookmarks/{bm_id}/edit-form")).await;

    // hx-target="#favicon-preview" on the refetch button means #favicon-preview must exist
    let favicon_preview = select_all(&html, "#favicon-preview");
    assert!(
        !favicon_preview.is_empty(),
        "#favicon-preview must exist (referenced by refetch button hx-target)"
    );

    // The refetch button's hx-target value should match an existing element id
    let refetch_btns = select_all(&html, "button[hx-post][hx-target]");
    for btn in &refetch_btns {
        let target = btn.value().attr("hx-target").unwrap();
        if let Some(target_id) = target.strip_prefix('#') {
            let found = select_all(&html, &format!("#{target_id}"));
            assert!(
                !found.is_empty(),
                "hx-target '{target}' should reference an existing element in the partial"
            );
        }
    }
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn detail_view_htmx_targets_reference_known_ids() {
    let (app, root_id) = build_views_app();
    let bm_id = create_bookmark(&app, &root_id).await;

    let (_, html) = get_html(app, &format!("/bookmarks/{bm_id}/detail")).await;

    // Collect all hx-target values from this partial
    let all_with_target = select_all(&html, "[hx-target]");
    for el in &all_with_target {
        let target = el.value().attr("hx-target").unwrap();
        // These targets reference elements in the parent page (base.html):
        // #folder-content, #detail-body, #edit-modal-body
        // We verify they use the expected selector format
        assert!(
            target.starts_with('#'),
            "hx-target should be an id selector: {target}"
        );
        let valid_targets = [
            "#folder-content",
            "#detail-body",
            "#edit-modal-body",
            "#favicon-preview",
        ];
        assert!(
            valid_targets.contains(&target),
            "hx-target '{target}' should be one of the known page targets"
        );
    }
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn folder_content_htmx_include_references_existing_id() {
    let (app, root_id) = build_views_app();
    create_bookmark(&app, &root_id).await;

    let (_, html) = get_html(app, &format!("/folders/{root_id}/content")).await;

    // All hx-include references should point to elements in this partial
    let includes = select_all(&html, "[hx-include]");
    for el in &includes {
        let include_sel = el.value().attr("hx-include").unwrap();
        assert!(
            include_sel.starts_with('#'),
            "hx-include should be an id selector: {include_sel}"
        );
        let target_id = &include_sel[1..];
        let found = select_all(&html, &format!("#{target_id}"));
        assert!(
            !found.is_empty(),
            "hx-include '{include_sel}' must reference an existing element"
        );
    }
}

// ─── Mutation side-effect tests (kill escaped mutants) ─────

fn build_views_app_with_sync_root() -> (
    Router,
    String,
    automerge_repo::DocHandle,
    std::path::PathBuf,
) {
    let td = new_initialized_doc("test-views");
    let root_id = td.root_folder_id.clone();
    let doc_handle = td.doc_handle.clone();
    let sync_root = tempfile::TempDir::new().unwrap();
    let sync_root_path = sync_root.path().to_path_buf();
    let (sse_tx, _) = tokio::sync::broadcast::channel::<()>(16);
    let state = Arc::new(state::AppState {
        doc_handle: td.doc_handle,
        sync_root: sync_root_path.clone(),
        client_id: "test-views".to_string(),
        sse_tx,
        static_version: "test".to_string(),
        exporter: repo::Exporter::new(&sync_root_path, "test-views"),
    });
    let app = Router::new()
        .route("/folders/{id}/content", get(handlers::folder_content))
        .route("/folders/{id}/rename", post(handlers::rename_folder_html))
        .route("/bookmarks/{id}/detail", get(handlers::bookmark_detail))
        .route("/bookmarks/{id}/edit", post(handlers::update_bookmark_html))
        .route(
            "/bookmarks/{id}/revert",
            post(handlers::revert_bookmark_html),
        )
        .route("/bookmarks/new", post(handlers::create_bookmark_html))
        .with_state(state);
    std::mem::forget(td.temp_dir);
    std::mem::forget(sync_root);
    (app, root_id, doc_handle, sync_root_path)
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn revert_bookmark_html_restores_previous_state() {
    let (app, root_id, doc_handle, _) = build_views_app_with_sync_root();
    let bm_id =
        ops::add_bookmark(&doc_handle, &root_id, "https://example.com", "Original").unwrap();
    ops::update_bookmark(
        &doc_handle,
        &bm_id,
        Some("https://example.com"),
        Some("Changed"),
        None,
    )
    .unwrap();

    let entries = history::bookmark_history(&doc_handle, &bm_id);
    let v1_hash = &entries.last().unwrap().hash;

    let body = format!("target_hash={v1_hash}");
    let resp = app
        .oneshot(
            Request::post(format!("/bookmarks/{bm_id}/revert"))
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let store: mybriefcase_bookmarks::model::BookmarkStore =
        doc_handle.with_doc(|d| autosurgeon::hydrate(d).unwrap());
    let bm = store.bookmarks.get(&bm_id).unwrap();
    assert_eq!(bm.title, "Original", "bookmark should be reverted");
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn after_write_exports_to_sync_root() {
    let (app, root_id, _, sync_root_path) = build_views_app_with_sync_root();

    let body = format!("folder_id={root_id}&url=https%3A%2F%2Fexample.com&title=ExportTest");
    let resp = app
        .oneshot(
            Request::post("/bookmarks/new")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let snapshot = sync_root_path
        .join("test-views")
        .join("store")
        .join("document.snapshot");
    assert!(
        snapshot.exists(),
        "after_write should export the document to sync_root"
    );
}

// ─── Kill escaped mutants: stories 10-20, 23 ────────

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn bookmark_detail_returns_empty_when_deleted() {
    let (app, root_id, doc) = build_views_app_with_handle();
    let bm_id = ops::add_bookmark(&doc, &root_id, "https://example.com", "Test").unwrap();
    ops::delete_bookmark(&doc, &bm_id).unwrap();

    let (status, html) = get_html(app, &format!("/bookmarks/{bm_id}/detail")).await;
    assert_eq!(status, StatusCode::OK);
    // DetailEmptyTemplate renders empty detail — should NOT contain bookmark data
    assert!(!html.contains("https://example.com"));
    assert!(!html.contains("detail-title"));
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn bookmark_edit_form_returns_empty_when_deleted() {
    let (app, root_id, doc) = build_views_app_with_handle();
    let bm_id = ops::add_bookmark(&doc, &root_id, "https://example.com", "Test").unwrap();
    ops::delete_bookmark(&doc, &bm_id).unwrap();

    let (status, html) = get_html(app, &format!("/bookmarks/{bm_id}/edit-form")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!html.contains("name=\"title\""));
    assert!(!html.contains("name=\"url\""));
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn bookmark_history_returns_empty_when_deleted() {
    let (app, root_id, doc) = build_views_app_with_handle();
    let bm_id = ops::add_bookmark(&doc, &root_id, "https://example.com", "Test").unwrap();
    ops::delete_bookmark(&doc, &bm_id).unwrap();

    let (status, html) = get_html(app, &format!("/bookmarks/{bm_id}/history")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!html.contains("https://example.com"));
    assert!(!html.contains("History"));
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn create_bookmark_data_favicon_not_fetched() {
    let (app, root_id, doc) = build_views_app_with_handle();

    let body = format!(
        "folder_id={root_id}&url=https%3A%2F%2Fexample.com&title=DataFav&favicon_url=data%3Aimage%2Fpng%3Bbase64%2CiVBOR"
    );
    let (status, _) = post_form(app, "/bookmarks/new", &body).await;
    assert_eq!(status, StatusCode::OK);

    // Give any potential async task a moment to run (it should NOT run)
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let store: mybriefcase_bookmarks::model::BookmarkStore =
        doc.with_doc(|d| autosurgeon::hydrate(d).unwrap());
    let bm = store
        .bookmarks
        .values()
        .find(|b| b.title == "DataFav")
        .expect("bookmark should exist");
    assert_eq!(
        bm.favicon, None,
        "data: favicon URLs must not trigger fetch"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn create_bookmark_empty_favicon_not_fetched() {
    let (app, root_id, doc) = build_views_app_with_handle();

    let body =
        format!("folder_id={root_id}&url=https%3A%2F%2Fexample.com&title=EmptyFav&favicon_url=");
    let (status, _) = post_form(app, "/bookmarks/new", &body).await;
    assert_eq!(status, StatusCode::OK);

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let store: mybriefcase_bookmarks::model::BookmarkStore =
        doc.with_doc(|d| autosurgeon::hydrate(d).unwrap());
    let bm = store
        .bookmarks
        .values()
        .find(|b| b.title == "EmptyFav")
        .expect("bookmark should exist");
    assert_eq!(bm.favicon, None, "empty favicon_url must not trigger fetch");
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn update_bookmark_moves_to_different_folder() {
    let (app, root_id, doc) = build_views_app_with_handle();
    let folder_a = ops::create_folder(&doc, &root_id, "FolderA").unwrap();
    let bm_id = ops::add_bookmark(&doc, &root_id, "https://example.com", "Movable").unwrap();

    // Move bookmark from root to folder_a via update
    let body = format!("title=Movable&url=https%3A%2F%2Fexample.com&notes=&folder_id={folder_a}");
    let (status, _) = post_form(app, &format!("/bookmarks/{bm_id}/edit"), &body).await;
    assert_eq!(status, StatusCode::OK);

    let store: mybriefcase_bookmarks::model::BookmarkStore =
        doc.with_doc(|d| autosurgeon::hydrate(d).unwrap());
    let a = store.folders.get(&folder_a).unwrap();
    assert!(
        a.children.contains(&bm_id),
        "bookmark should be in the new folder"
    );
    let root = store.folders.get(&root_id).unwrap();
    assert!(
        !root.children.contains(&bm_id),
        "bookmark should be removed from the old folder"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn delete_folder_redirects_to_root_when_current() {
    let (app, root_id, doc) = build_views_app_with_handle();
    let folder_id = ops::create_folder(&doc, &root_id, "ToDelete").unwrap();

    // Delete folder while current_folder_id == the deleted folder
    let body = format!("current_folder_id={folder_id}");
    let (status, html) = post_form(app, &format!("/folders/{folder_id}/remove"), &body).await;
    assert_eq!(status, StatusCode::OK);
    // Should redirect to root folder content — the root folder's breadcrumb title "Bookmarks"
    // should appear in the response, and the deleted folder should NOT
    assert!(
        html.contains(&root_id),
        "response should show root folder content"
    );
    assert!(
        !html.contains("ToDelete"),
        "deleted folder should not appear"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn delete_folder_stays_on_current_when_different() {
    let (app, root_id, doc) = build_views_app_with_handle();
    let folder_to_delete = ops::create_folder(&doc, &root_id, "Victim").unwrap();
    let _other_folder = ops::create_folder(&doc, &root_id, "Survivor").unwrap();

    // Delete folder while current_folder_id is root (different from deleted)
    let body = format!("current_folder_id={root_id}");
    let (status, html) =
        post_form(app, &format!("/folders/{folder_to_delete}/remove"), &body).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !html.contains("Victim"),
        "deleted folder should not appear in content"
    );
    assert!(
        html.contains("Survivor"),
        "other folder should still appear"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn sidebar_only_returns_correct_total_count() {
    let (app, root_id, doc) = build_views_app_with_handle();
    ops::create_folder(&doc, &root_id, "SubA").unwrap();
    ops::create_folder(&doc, &root_id, "SubB").unwrap();
    ops::add_bookmark(&doc, &root_id, "https://example.com", "BM1").unwrap();

    let (status, html) = get_html(app, &format!("/sidebar?folder_id={root_id}")).await;
    assert_eq!(status, StatusCode::OK);
    // 2 folders + 1 bookmark = 3 items
    assert!(
        html.contains("3 items"),
        "sidebar should show total count (folders + bookmarks): got {html}"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn search_matches_url() {
    let (app, root_id, doc) = build_views_app_with_handle();
    ops::add_bookmark(
        &doc,
        &root_id,
        "https://unique-domain.test/path",
        "Generic Title",
    )
    .unwrap();

    let (status, html) = get_html(app, "/search?q=unique-domain").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        html.contains("Generic Title"),
        "search should match on URL field"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn search_matches_notes() {
    let (app, root_id, doc) = build_views_app_with_handle();
    let bm_id = ops::add_bookmark(&doc, &root_id, "https://example.com", "Plain Title").unwrap();
    ops::update_bookmark(&doc, &bm_id, None, None, Some("special-keyword in notes")).unwrap();

    let (status, html) = get_html(app, "/search?q=special-keyword").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        html.contains("Plain Title"),
        "search should match on notes field"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn search_does_not_match_deleted_bookmarks() {
    let (app, root_id, doc) = build_views_app_with_handle();
    let bm_id =
        ops::add_bookmark(&doc, &root_id, "https://deleted-unique.test", "DeletedBM").unwrap();
    ops::delete_bookmark(&doc, &bm_id).unwrap();

    // Search by URL to avoid matching the search breadcrumb title
    let (status, html) = get_html(app, "/search?q=deleted-unique").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !html.contains("DeletedBM"),
        "deleted bookmarks should not appear in search results"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn serve_favicon_rejects_invalid_characters() {
    let (app, _, _) = build_views_app_with_handle();

    // Hyphens are not in the allowed set (hex digits, '.', ascii alpha)
    let (status, _) = get_html(app.clone(), "/favicons/file-name.png").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Underscores not allowed
    let (status, _) = get_html(app.clone(), "/favicons/file_name.png").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Spaces not allowed
    let (status, _) = get_html(app.clone(), "/favicons/file%20name.png").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Special characters not allowed
    let (status, _) = get_html(app, "/favicons/file%3Bname.png").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn serve_favicon_content_type_png() {
    let td = new_initialized_doc("test-favicon-ct");
    let sync_root = tempfile::TempDir::new().unwrap();
    let favicons_dir = sync_root.path().join("favicons");
    std::fs::create_dir_all(&favicons_dir).unwrap();
    std::fs::write(favicons_dir.join("abc123.png"), b"fake-png-data").unwrap();

    let (sse_tx, _) = tokio::sync::broadcast::channel::<()>(16);
    let state = Arc::new(state::AppState {
        doc_handle: td.doc_handle,
        sync_root: sync_root.path().to_path_buf(),
        client_id: "test-favicon-ct".to_string(),
        sse_tx,
        static_version: "test".to_string(),
        exporter: repo::Exporter::new(sync_root.path(), "test-favicon-ct"),
    });
    let app = Router::new()
        .route("/favicons/{filename}", get(handlers::serve_favicon))
        .with_state(state);
    std::mem::forget(td.temp_dir);
    std::mem::forget(sync_root);

    let resp = app
        .oneshot(
            Request::get("/favicons/abc123.png")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers().get("content-type").unwrap(), "image/png");
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn serve_favicon_content_type_ico() {
    let td = new_initialized_doc("test-favicon-ico");
    let sync_root = tempfile::TempDir::new().unwrap();
    let favicons_dir = sync_root.path().join("favicons");
    std::fs::create_dir_all(&favicons_dir).unwrap();
    std::fs::write(favicons_dir.join("abc123.ico"), b"fake-ico-data").unwrap();

    let (sse_tx, _) = tokio::sync::broadcast::channel::<()>(16);
    let state = Arc::new(state::AppState {
        doc_handle: td.doc_handle,
        sync_root: sync_root.path().to_path_buf(),
        client_id: "test-favicon-ico".to_string(),
        sse_tx,
        static_version: "test".to_string(),
        exporter: repo::Exporter::new(sync_root.path(), "test-favicon-ico"),
    });
    let app = Router::new()
        .route("/favicons/{filename}", get(handlers::serve_favicon))
        .with_state(state);
    std::mem::forget(td.temp_dir);
    std::mem::forget(sync_root);

    let resp = app
        .oneshot(
            Request::get("/favicons/abc123.ico")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers().get("content-type").unwrap(), "image/x-icon");
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn serve_favicon_content_type_svg() {
    let td = new_initialized_doc("test-favicon-svg");
    let sync_root = tempfile::TempDir::new().unwrap();
    let favicons_dir = sync_root.path().join("favicons");
    std::fs::create_dir_all(&favicons_dir).unwrap();
    std::fs::write(favicons_dir.join("abc123.svg"), b"<svg></svg>").unwrap();

    let (sse_tx, _) = tokio::sync::broadcast::channel::<()>(16);
    let state = Arc::new(state::AppState {
        doc_handle: td.doc_handle,
        sync_root: sync_root.path().to_path_buf(),
        client_id: "test-favicon-svg".to_string(),
        sse_tx,
        static_version: "test".to_string(),
        exporter: repo::Exporter::new(sync_root.path(), "test-favicon-svg"),
    });
    let app = Router::new()
        .route("/favicons/{filename}", get(handlers::serve_favicon))
        .with_state(state);
    std::mem::forget(td.temp_dir);
    std::mem::forget(sync_root);

    let resp = app
        .oneshot(
            Request::get("/favicons/abc123.svg")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers().get("content-type").unwrap(), "image/svg+xml");
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn serve_favicon_content_type_jpg() {
    let td = new_initialized_doc("test-favicon-jpg");
    let sync_root = tempfile::TempDir::new().unwrap();
    let favicons_dir = sync_root.path().join("favicons");
    std::fs::create_dir_all(&favicons_dir).unwrap();
    std::fs::write(favicons_dir.join("abc123.jpg"), b"fake-jpg").unwrap();

    let (sse_tx, _) = tokio::sync::broadcast::channel::<()>(16);
    let state = Arc::new(state::AppState {
        doc_handle: td.doc_handle,
        sync_root: sync_root.path().to_path_buf(),
        client_id: "test-favicon-jpg".to_string(),
        sse_tx,
        static_version: "test".to_string(),
        exporter: repo::Exporter::new(sync_root.path(), "test-favicon-jpg"),
    });
    let app = Router::new()
        .route("/favicons/{filename}", get(handlers::serve_favicon))
        .with_state(state);
    std::mem::forget(td.temp_dir);
    std::mem::forget(sync_root);

    let resp = app
        .oneshot(
            Request::get("/favicons/abc123.jpg")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers().get("content-type").unwrap(), "image/jpeg");
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn serve_favicon_content_type_gif() {
    let td = new_initialized_doc("test-favicon-gif");
    let sync_root = tempfile::TempDir::new().unwrap();
    let favicons_dir = sync_root.path().join("favicons");
    std::fs::create_dir_all(&favicons_dir).unwrap();
    std::fs::write(favicons_dir.join("abc123.gif"), b"GIF89a").unwrap();

    let (sse_tx, _) = tokio::sync::broadcast::channel::<()>(16);
    let state = Arc::new(state::AppState {
        doc_handle: td.doc_handle,
        sync_root: sync_root.path().to_path_buf(),
        client_id: "test-favicon-gif".to_string(),
        sse_tx,
        static_version: "test".to_string(),
        exporter: repo::Exporter::new(sync_root.path(), "test-favicon-gif"),
    });
    let app = Router::new()
        .route("/favicons/{filename}", get(handlers::serve_favicon))
        .with_state(state);
    std::mem::forget(td.temp_dir);
    std::mem::forget(sync_root);

    let resp = app
        .oneshot(
            Request::get("/favicons/abc123.gif")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers().get("content-type").unwrap(), "image/gif");
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn serve_favicon_content_type_webp() {
    let td = new_initialized_doc("test-favicon-webp");
    let sync_root = tempfile::TempDir::new().unwrap();
    let favicons_dir = sync_root.path().join("favicons");
    std::fs::create_dir_all(&favicons_dir).unwrap();
    std::fs::write(favicons_dir.join("abc123.webp"), b"RIFF").unwrap();

    let (sse_tx, _) = tokio::sync::broadcast::channel::<()>(16);
    let state = Arc::new(state::AppState {
        doc_handle: td.doc_handle,
        sync_root: sync_root.path().to_path_buf(),
        client_id: "test-favicon-webp".to_string(),
        sse_tx,
        static_version: "test".to_string(),
        exporter: repo::Exporter::new(sync_root.path(), "test-favicon-webp"),
    });
    let app = Router::new()
        .route("/favicons/{filename}", get(handlers::serve_favicon))
        .with_state(state);
    std::mem::forget(td.temp_dir);
    std::mem::forget(sync_root);

    let resp = app
        .oneshot(
            Request::get("/favicons/abc123.webp")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers().get("content-type").unwrap(), "image/webp");
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn render_folder_response_total_item_count() {
    let (app, root_id, doc) = build_views_app_with_handle();
    ops::create_folder(&doc, &root_id, "F1").unwrap();
    ops::create_folder(&doc, &root_id, "F2").unwrap();
    ops::add_bookmark(&doc, &root_id, "https://a.com", "A").unwrap();
    ops::add_bookmark(&doc, &root_id, "https://b.com", "B").unwrap();
    ops::add_bookmark(&doc, &root_id, "https://c.com", "C").unwrap();

    let (status, html) = get_html(app, &format!("/folders/{root_id}/content")).await;
    assert_eq!(status, StatusCode::OK);
    // 2 folders + 3 bookmarks = 5 items
    assert!(
        html.contains("5 items"),
        "render_folder_response should show folders.len() + bookmarks.len(): got {html}"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn update_bookmark_response_total_item_count() {
    let (app, root_id, doc) = build_views_app_with_handle();
    ops::create_folder(&doc, &root_id, "F1").unwrap();
    ops::create_folder(&doc, &root_id, "F2").unwrap();
    let bm_id = ops::add_bookmark(&doc, &root_id, "https://a.com", "A").unwrap();
    ops::add_bookmark(&doc, &root_id, "https://b.com", "B").unwrap();
    ops::add_bookmark(&doc, &root_id, "https://c.com", "C").unwrap();

    let (status, html) = post_form(
        app,
        &format!("/bookmarks/{bm_id}/edit"),
        "title=Updated&url=https%3A%2F%2Fa.com&notes=",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // 2 folders + 3 bookmarks = 5 items
    assert!(
        html.contains("5 items"),
        "update_bookmark response should show correct total item count: got {html}"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn revert_bookmark_response_total_item_count() {
    let (app, root_id, doc, _) = build_views_app_with_sync_root();
    ops::create_folder(&doc, &root_id, "F1").unwrap();
    ops::create_folder(&doc, &root_id, "F2").unwrap();
    let bm_id = ops::add_bookmark(&doc, &root_id, "https://a.com", "A").unwrap();
    ops::add_bookmark(&doc, &root_id, "https://b.com", "B").unwrap();
    ops::add_bookmark(&doc, &root_id, "https://c.com", "C").unwrap();
    ops::update_bookmark(&doc, &bm_id, None, Some("Changed"), None).unwrap();

    let entries = history::bookmark_history(&doc, &bm_id);
    let v1_hash = &entries.last().unwrap().hash;

    let body = format!("target_hash={v1_hash}");
    let (status, html) = post_form(app, &format!("/bookmarks/{bm_id}/revert"), &body).await;
    assert_eq!(status, StatusCode::OK);
    // 2 folders + 3 bookmarks = 5 items
    assert!(
        html.contains("5 items"),
        "revert_bookmark response should show correct total item count: got {html}"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn bookmark_history_returns_content_for_existing_bookmark() {
    let (app, root_id, doc) = build_views_app_with_handle();
    let bm_id = ops::add_bookmark(&doc, &root_id, "https://example.com", "HistoryTest").unwrap();
    ops::update_bookmark(&doc, &bm_id, None, Some("Updated Title"), None).unwrap();

    let (status, html) = get_html(app, &format!("/bookmarks/{bm_id}/history")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !html.is_empty(),
        "bookmark_history_html should return non-empty content"
    );
    assert!(
        html.contains("HistoryTest") || html.contains("Updated Title"),
        "history should contain bookmark info"
    );
    assert!(
        html.contains("History"),
        "history page should contain History button"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn create_bookmark_favicon_propagates_to_existing_same_url() {
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let icon_bytes: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00];

    let icon_clone = icon_bytes.clone();
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        loop {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\n\r\n",
                icon_clone.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(&icon_clone);
            let _ = stream.flush();
        }
    });

    let (app, root_id, doc) = build_views_app_with_handle();
    let url = format!("http://127.0.0.1:{port}/icon.png");

    // Create first bookmark with same URL (no favicon)
    let existing_bm = ops::add_bookmark(
        &doc,
        &root_id,
        &format!("http://127.0.0.1:{port}/page"),
        "Existing",
    )
    .unwrap();

    // Create second bookmark with favicon_url pointing to our server
    let encoded_url = format!("http://127.0.0.1:{port}/page")
        .replace(':', "%3A")
        .replace('/', "%2F");
    let encoded_favicon = url.replace(':', "%3A").replace('/', "%2F");
    let body =
        format!("folder_id={root_id}&url={encoded_url}&title=New&favicon_url={encoded_favicon}");
    let (status, _) = post_form(app, "/bookmarks/new", &body).await;
    assert_eq!(status, StatusCode::OK);

    // Wait for async favicon task to complete
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let store: mybriefcase_bookmarks::model::BookmarkStore =
        doc.with_doc(|d| autosurgeon::hydrate(d).unwrap());
    let existing = store.bookmarks.get(&existing_bm).unwrap();
    assert!(
        existing.favicon.is_some(),
        "favicon should propagate to existing bookmark with same URL"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn create_bookmark_favicon_propagates_skips_deleted_and_different_url() {
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let icon_bytes: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00];

    let icon_clone = icon_bytes.clone();
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        loop {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\n\r\n",
                icon_clone.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(&icon_clone);
            let _ = stream.flush();
        }
    });

    let (app, root_id, doc) = build_views_app_with_handle();
    let favicon_url = format!("http://127.0.0.1:{port}/icon.png");
    let same_url = format!("http://127.0.0.1:{port}/page");

    // Bookmark with same URL but deleted
    let deleted_bm = ops::add_bookmark(&doc, &root_id, &same_url, "Deleted").unwrap();
    ops::delete_bookmark(&doc, &deleted_bm).unwrap();

    // Bookmark with a different URL (alive)
    let different_bm =
        ops::add_bookmark(&doc, &root_id, "https://other-site.example", "Different").unwrap();

    // Create new bookmark with favicon_url
    let encoded_url = same_url.replace(':', "%3A").replace('/', "%2F");
    let encoded_favicon = favicon_url.replace(':', "%3A").replace('/', "%2F");
    let body =
        format!("folder_id={root_id}&url={encoded_url}&title=New&favicon_url={encoded_favicon}");
    let (status, _) = post_form(app, "/bookmarks/new", &body).await;
    assert_eq!(status, StatusCode::OK);

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let store: mybriefcase_bookmarks::model::BookmarkStore =
        doc.with_doc(|d| autosurgeon::hydrate(d).unwrap());
    assert_eq!(
        store.bookmarks.get(&deleted_bm).unwrap().favicon,
        None,
        "deleted bookmarks must not receive propagated favicon"
    );
    assert_eq!(
        store.bookmarks.get(&different_bm).unwrap().favicon,
        None,
        "bookmarks with different URLs must not receive propagated favicon"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn update_bookmark_favicon_propagates_skips_deleted() {
    let (app, root_id, doc) = build_views_app_with_handle();
    let bm1 = ops::add_bookmark(&doc, &root_id, "https://same.com", "Live").unwrap();
    let bm2 = ops::add_bookmark(&doc, &root_id, "https://same.com", "Deleted").unwrap();
    ops::delete_bookmark(&doc, &bm2).unwrap();

    let (status, _) = post_form(
        app,
        &format!("/bookmarks/{bm1}/edit"),
        "title=Live&url=https%3A%2F%2Fsame.com&notes=&favicon=icon.png",
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let store: mybriefcase_bookmarks::model::BookmarkStore =
        doc.with_doc(|d| autosurgeon::hydrate(d).unwrap());
    assert_eq!(
        store.bookmarks.get(&bm1).unwrap().favicon,
        Some("icon.png".to_string())
    );
    // Deleted bookmark should NOT get the propagated favicon
    assert_eq!(
        store.bookmarks.get(&bm2).unwrap().favicon,
        None,
        "deleted bookmarks must not receive propagated favicon"
    );
}
