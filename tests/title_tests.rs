#[expect(
    dead_code,
    reason = "shared test helpers; not all used by every test binary"
)]
mod common;

use autosurgeon::hydrate;
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::{get, post};
use http_body_util::BodyExt;
use mybriefcase_bookmarks::handlers;
use mybriefcase_bookmarks::model::BookmarkStore;
use mybriefcase_bookmarks::ops;
use mybriefcase_bookmarks::repo;
use std::sync::Arc;
use tower::ServiceExt;

use common::new_initialized_doc;

fn build_html_app(
    doc_handle: automerge_repo::DocHandle,
    sync_root: std::path::PathBuf,
    client_id: String,
) -> Router {
    let (sse_tx, _) = tokio::sync::broadcast::channel::<()>(16);
    let exporter = repo::Exporter::new(&sync_root, &client_id);
    let state = Arc::new(mybriefcase_bookmarks::state::AppState {
        doc_handle,
        sync_root,
        client_id,
        sse_tx,
        static_version: "test".to_string(),
        exporter,
    });

    Router::new()
        .route("/", get(handlers::index_page))
        .route("/folders/{id}", get(handlers::dispatch_get_folder))
        .route("/folders/new", post(handlers::create_folder_html))
        .route("/bookmarks/new", post(handlers::create_bookmark_html))
        .route(
            "/bookmarks/{id}/remove",
            post(handlers::delete_bookmark_html),
        )
        .route("/folders/{id}/remove", post(handlers::delete_folder_html))
        .route("/folders/{id}/rename", post(handlers::rename_folder_html))
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

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn create_folder_html_creates_folder_in_doc() {
    let (app, root_id, doc_handle) = make_app();

    let body = format!("parent_folder_id={root_id}&title=New+Folder");
    let resp = app
        .oneshot(
            Request::post("/folders/new")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let store: BookmarkStore = doc_handle.with_doc(|d| hydrate(d).unwrap());
    assert!(
        store.folders.values().any(|f| f.title == "New Folder"),
        "folder should be created in the document"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn delete_bookmark_html_removes_bookmark_from_doc() {
    let (app, root_id, doc_handle) = make_app();
    let bm_id = ops::add_bookmark(&doc_handle, &root_id, "https://example.com", "Bye").unwrap();

    let body = format!("current_folder_id={root_id}");
    let resp = app
        .oneshot(
            Request::post(format!("/bookmarks/{bm_id}/remove"))
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let store: BookmarkStore = doc_handle.with_doc(|d| hydrate(d).unwrap());
    let bm = store.bookmarks.get(&bm_id).unwrap();
    assert!(bm.deleted, "bookmark should be marked deleted");
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn delete_folder_html_removes_folder_from_doc() {
    let (app, root_id, doc_handle) = make_app();
    let folder_id = ops::create_folder(&doc_handle, &root_id, "Doomed").unwrap();

    let body = format!("current_folder_id={root_id}");
    let resp = app
        .oneshot(
            Request::post(format!("/folders/{folder_id}/remove"))
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let store: BookmarkStore = doc_handle.with_doc(|d| hydrate(d).unwrap());
    let folder = store.folders.get(&folder_id).unwrap();
    assert!(folder.deleted, "folder should be marked deleted");
}
