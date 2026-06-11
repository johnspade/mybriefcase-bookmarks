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
use mybriefcase_bookmarks::ops;
use mybriefcase_bookmarks::views;
use std::sync::Arc;
use tower::ServiceExt;

use common::new_initialized_doc;

fn build_html_app(
    doc_handle: automerge_repo::DocHandle,
    sync_root: std::path::PathBuf,
    client_id: String,
) -> Router {
    let (sse_tx, _) = tokio::sync::broadcast::channel::<()>(16);
    let state = Arc::new(mybriefcase_bookmarks::api::AppState {
        doc_handle,
        sync_root,
        client_id,
        sse_tx,
    });

    Router::new()
        .route("/", get(views::index_page))
        .route("/folders/{id}", get(views::dispatch_get_folder))
        .route("/folders/new", post(views::create_folder_html))
        .route("/bookmarks/new", post(views::create_bookmark_html))
        .route("/bookmarks/{id}/remove", post(views::delete_bookmark_html))
        .route("/folders/{id}/remove", post(views::delete_folder_html))
        .route("/folders/{id}/rename", post(views::rename_folder_html))
        .with_state(state)
}

fn make_app() -> (Router, String, automerge_repo::DocHandle) {
    let td = new_initialized_doc("test-client");
    let root_id = td.root_folder_id.clone();
    let sync_root = tempfile::TempDir::new().unwrap();
    let doc_handle = td.doc_handle.clone();
    let app = build_html_app(
        td.doc_handle,
        sync_root.path().to_path_buf(),
        "test-client".to_string(),
    );
    std::mem::forget(td.temp_dir);
    std::mem::forget(sync_root);
    (app, root_id, doc_handle)
}

async fn get_body(app: Router, uri: &str, htmx: bool) -> (StatusCode, String) {
    let mut builder = Request::get(uri);
    if htmx {
        builder = builder.header("hx-request", "true");
    }
    let resp = app
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn full_page_root_has_title() {
    let (app, _root_id, _) = make_app();
    let (status, html) = get_body(app, "/", false).await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        html.contains("<title>MyBriefcase Bookmarks</title>"),
        "Root page should have 'MyBriefcase Bookmarks' title"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn full_page_folder_has_dynamic_title() {
    let (app, root_id, doc_handle) = make_app();

    let folder_id = ops::create_folder(&doc_handle, &root_id, "Work Stuff").unwrap();

    let (status, html) = get_body(app, &format!("/folders/{folder_id}"), false).await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        html.contains("<title>Work Stuff — MyBriefcase Bookmarks</title>"),
        "Folder page should have folder name in title, got: {}",
        html.lines()
            .find(|l| l.contains("<title>"))
            .unwrap_or("no title found")
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn htmx_folder_response_has_title_tag() {
    let (app, root_id, doc_handle) = make_app();

    let folder_id = ops::create_folder(&doc_handle, &root_id, "My Folder").unwrap();

    let (status, html) = get_body(app, &format!("/folders/{folder_id}"), true).await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        html.contains("<title>My Folder — MyBriefcase Bookmarks</title>"),
        "HTMX response should contain <title> tag for folder name, got: {}",
        html.lines()
            .find(|l| l.contains("<title>"))
            .unwrap_or("no title found")
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn htmx_root_folder_response_has_title_tag() {
    let (app, root_id, _) = make_app();

    let (status, html) = get_body(app, &format!("/folders/{root_id}"), true).await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        html.contains("<title>Bookmarks — MyBriefcase Bookmarks</title>"),
        "HTMX response for root folder should have 'Bookmarks' title, got: {}",
        html.lines()
            .find(|l| l.contains("<title>"))
            .unwrap_or("no title found")
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn title_escapes_html_entities() {
    let (app, root_id, doc_handle) = make_app();

    let folder_id = ops::create_folder(&doc_handle, &root_id, "A<B&C").unwrap();

    let (status, html) = get_body(app, &format!("/folders/{folder_id}"), true).await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        html.contains("<title>A&lt;B&amp;C — MyBriefcase Bookmarks</title>"),
        "Title should escape HTML entities"
    );
}
