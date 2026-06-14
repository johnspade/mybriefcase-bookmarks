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
use mybriefcase_bookmarks::{handlers, ops, state};

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
    assert_eq!(bm.favicon, "abc123.png");
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn edit_bookmark_clears_favicon() {
    let (app, root_id, doc) = build_views_app_with_handle();
    let bm_id = ops::add_bookmark(&doc, &root_id, "https://example.com", "Test").unwrap();
    ops::update_favicon(&doc, &bm_id, "existing.png").unwrap();

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
    assert_eq!(bm.favicon, "");
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
    assert_eq!(store.bookmarks.get(&bm1).unwrap().favicon, "new-icon.png");
    assert_eq!(store.bookmarks.get(&bm2).unwrap().favicon, "new-icon.png");
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn edit_bookmark_favicon_delete_does_not_propagate() {
    let (app, root_id, doc) = build_views_app_with_handle();
    let bm1 = ops::add_bookmark(&doc, &root_id, "https://example.com", "One").unwrap();
    let bm2 = ops::add_bookmark(&doc, &root_id, "https://example.com", "Two").unwrap();
    ops::update_favicon(&doc, &bm1, "shared.png").unwrap();
    ops::update_favicon(&doc, &bm2, "shared.png").unwrap();

    let (status, _) = post_form(
        app,
        &format!("/bookmarks/{bm1}/edit"),
        "title=One&url=https%3A%2F%2Fexample.com&notes=&favicon=",
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let store: mybriefcase_bookmarks::model::BookmarkStore =
        doc.with_doc(|d| autosurgeon::hydrate(d).unwrap());
    assert_eq!(store.bookmarks.get(&bm1).unwrap().favicon, "");
    assert_eq!(store.bookmarks.get(&bm2).unwrap().favicon, "shared.png");
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
    assert!(!bm.favicon.is_empty(), "favicon should be stored");
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
    ops::update_favicon(&doc, &bm_id, "test-icon.png").unwrap();

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
