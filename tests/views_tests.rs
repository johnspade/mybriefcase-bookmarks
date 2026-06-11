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
use mybriefcase_bookmarks::{api, views};
use views::SortOrder;

fn build_views_app() -> (Router, String) {
    let td = new_initialized_doc("test-views");
    let root_id = td.root_folder_id.clone();
    let sync_root = tempfile::TempDir::new().unwrap();
    let (sse_tx, _) = tokio::sync::broadcast::channel::<()>(16);
    let state = Arc::new(api::AppState {
        doc_handle: td.doc_handle,
        sync_root: sync_root.path().to_path_buf(),
        client_id: "test-views".to_string(),
        sse_tx,
    });
    let app = Router::new()
        .route("/folders/{id}/content", get(views::folder_content))
        .route("/bookmarks/{id}/detail", get(views::bookmark_detail))
        .route("/bookmarks/{id}/edit-form", get(views::bookmark_edit_form))
        .route("/bookmarks/{id}/edit", post(views::update_bookmark_html))
        .route("/bookmarks/new", post(views::create_bookmark_html))
        .with_state(state);
    std::mem::forget(td.temp_dir);
    std::mem::forget(sync_root);
    (app, root_id)
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
