#[expect(
    dead_code,
    reason = "shared test helpers; not all used by every test binary"
)]
mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use common::{build_app, new_initialized_doc};

async fn get_json(app: axum::Router, uri: &str) -> (StatusCode, serde_json::Value) {
    let resp = app
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    (status, json)
}

async fn post_json(
    app: axum::Router,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let resp = app
        .oneshot(
            Request::post(uri)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    };
    (status, json)
}

async fn put_json(app: axum::Router, uri: &str, body: serde_json::Value) -> StatusCode {
    let resp = app
        .oneshot(
            Request::put(uri)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    resp.status()
}

async fn delete_request(app: axum::Router, uri: &str) -> StatusCode {
    let resp = app
        .oneshot(Request::delete(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    resp.status()
}

fn make_app() -> (axum::Router, String) {
    let td = new_initialized_doc("test-client");
    let root_id = td.root_folder_id.clone();
    let sync_root = tempfile::TempDir::new().unwrap();
    let app = build_app(
        td.doc_handle,
        sync_root.path().to_path_buf(),
        "test-client".to_owned(),
    );
    // Leak the TempDirs so they live for the test duration
    std::mem::forget(td.temp_dir);
    std::mem::forget(sync_root);
    (app, root_id)
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn get_tree_initial() {
    let (app, _root_id) = make_app();
    let (status, json) = get_json(app, "/").await;

    assert_eq!(status, StatusCode::OK);
    assert!(!json["root_folder_id"].as_str().unwrap().is_empty());
    assert_eq!(json["folders"].as_array().unwrap().len(), 1);
    assert_eq!(json["bookmarks"].as_array().unwrap().len(), 0);
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn create_and_get_bookmark() {
    let (app, root_id) = make_app();

    let (status, resp) = post_json(
        app.clone(),
        &format!("/folders/{root_id}/bookmarks"),
        serde_json::json!({
            "url": "https://example.com",
            "title": "Example"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let bm_id = resp["id"].as_str().unwrap();
    assert!(!bm_id.is_empty());

    let (status, tree) = get_json(app, "/").await;
    assert_eq!(status, StatusCode::OK);
    let bookmarks = tree["bookmarks"].as_array().unwrap();
    assert_eq!(bookmarks.len(), 1);
    assert_eq!(bookmarks[0]["url"], "https://example.com");
    assert_eq!(bookmarks[0]["title"], "Example");
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn create_folder() {
    let (app, root_id) = make_app();

    let (status, resp) = post_json(
        app.clone(),
        "/folders",
        serde_json::json!({
            "parent_folder_id": root_id,
            "title": "Work"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let folder_id = resp["id"].as_str().unwrap();
    assert!(!folder_id.is_empty());

    let (_, tree) = get_json(app, "/").await;
    let folders = tree["folders"].as_array().unwrap();
    assert!(folders.iter().any(|f| f["title"] == "Work"));
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn update_bookmark() {
    let (app, root_id) = make_app();

    let (_, resp) = post_json(
        app.clone(),
        &format!("/folders/{root_id}/bookmarks"),
        serde_json::json!({
            "url": "https://old.com",
            "title": "Old"
        }),
    )
    .await;
    let bm_id = resp["id"].as_str().unwrap().to_owned();

    let status = put_json(
        app.clone(),
        &format!("/bookmarks/{bm_id}"),
        serde_json::json!({
            "title": "New Title",
            "notes": "Some notes"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, tree) = get_json(app, "/").await;
    let bm = tree["bookmarks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == bm_id)
        .unwrap();
    assert_eq!(bm["title"], "New Title");
    assert_eq!(bm["notes"], "Some notes");
    assert_eq!(bm["url"], "https://old.com");
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn delete_bookmark_204() {
    let (app, root_id) = make_app();

    let (_, resp) = post_json(
        app.clone(),
        &format!("/folders/{root_id}/bookmarks"),
        serde_json::json!({
            "url": "https://delete-me.com",
            "title": "Delete Me"
        }),
    )
    .await;
    let bm_id = resp["id"].as_str().unwrap().to_owned();

    let status = delete_request(app.clone(), &format!("/bookmarks/{bm_id}")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, tree) = get_json(app, "/").await;
    let bookmarks = tree["bookmarks"].as_array().unwrap();
    assert!(
        !bookmarks.iter().any(|b| b["id"] == bm_id.as_str()),
        "Deleted bookmark should not appear in tree"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn get_nonexistent_folder_404() {
    let (app, _) = make_app();
    let resp = app
        .oneshot(
            Request::get("/folders/nonexistent-id")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn move_item_between_folders() {
    let (app, root_id) = make_app();

    let (_, folder_resp) = post_json(
        app.clone(),
        "/folders",
        serde_json::json!({
            "parent_folder_id": root_id,
            "title": "Target"
        }),
    )
    .await;
    let target_id = folder_resp["id"].as_str().unwrap().to_owned();

    let (_, bm_resp) = post_json(
        app.clone(),
        &format!("/folders/{root_id}/bookmarks"),
        serde_json::json!({
            "url": "https://moveable.com",
            "title": "Moveable"
        }),
    )
    .await;
    let bm_id = bm_resp["id"].as_str().unwrap().to_owned();

    let (status, _) = post_json(
        app.clone(),
        "/move",
        serde_json::json!({
            "item_id": bm_id,
            "from_folder_id": root_id,
            "to_folder_id": target_id
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, tree) = get_json(app, "/").await;
    let target_folder = tree["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == target_id.as_str())
        .unwrap();
    assert!(
        target_folder["children"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c == &bm_id)
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn export_html_content_type() {
    let (app, root_id) = make_app();

    post_json(
        app.clone(),
        &format!("/folders/{root_id}/bookmarks"),
        serde_json::json!({
            "url": "https://example.com",
            "title": "Test Export"
        }),
    )
    .await;

    let resp = app
        .oneshot(Request::get("/export").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "text/html; charset=utf-8"
    );

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("<!DOCTYPE NETSCAPE-Bookmark-file-1>"));
    assert!(html.contains("https://example.com"));
    assert!(html.contains("Test Export"));
}

// ─── History API tests ──────────────────────────────

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn history_returns_entries_for_bookmark() {
    let (app, root_id) = make_app();

    let (_, resp) = post_json(
        app.clone(),
        &format!("/folders/{root_id}/bookmarks"),
        serde_json::json!({
            "url": "https://example.com",
            "title": "Original"
        }),
    )
    .await;
    let bm_id = resp["id"].as_str().unwrap().to_owned();

    put_json(
        app.clone(),
        &format!("/bookmarks/{bm_id}"),
        serde_json::json!({ "title": "Updated" }),
    )
    .await;

    let (status, json) = get_json(app, &format!("/bookmarks/{bm_id}/history")).await;
    assert_eq!(status, StatusCode::OK);
    let entries = json.as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries[0]["hash"].as_str().unwrap().len() >= 8);
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn history_at_hash_returns_snapshot() {
    let (app, root_id) = make_app();

    let (_, resp) = post_json(
        app.clone(),
        &format!("/folders/{root_id}/bookmarks"),
        serde_json::json!({
            "url": "https://example.com",
            "title": "V1"
        }),
    )
    .await;
    let bm_id = resp["id"].as_str().unwrap().to_owned();

    put_json(
        app.clone(),
        &format!("/bookmarks/{bm_id}"),
        serde_json::json!({ "title": "V2" }),
    )
    .await;

    let (_, history) = get_json(app.clone(), &format!("/bookmarks/{bm_id}/history")).await;
    let entries = history.as_array().unwrap();
    let v1_hash = entries.last().unwrap()["hash"].as_str().unwrap();

    let (status, snapshot) = get_json(app, &format!("/bookmarks/{bm_id}/at/{v1_hash}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(snapshot["title"], "V1");
    assert_eq!(snapshot["url"], "https://example.com");
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn revert_restores_bookmark_state() {
    let (app, root_id) = make_app();

    let (_, resp) = post_json(
        app.clone(),
        &format!("/folders/{root_id}/bookmarks"),
        serde_json::json!({
            "url": "https://original.com",
            "title": "Original"
        }),
    )
    .await;
    let bm_id = resp["id"].as_str().unwrap().to_owned();

    put_json(
        app.clone(),
        &format!("/bookmarks/{bm_id}"),
        serde_json::json!({ "title": "Changed", "url": "https://changed.com" }),
    )
    .await;

    let (_, history) = get_json(app.clone(), &format!("/bookmarks/{bm_id}/history")).await;
    let v1_hash = history.as_array().unwrap().last().unwrap()["hash"]
        .as_str()
        .unwrap().to_owned();

    let status = post_json(
        app.clone(),
        &format!("/bookmarks/{bm_id}/revert"),
        serde_json::json!({ "target_hash": v1_hash }),
    )
    .await
    .0;
    assert_eq!(status, StatusCode::OK);

    let (_, tree) = get_json(app, "/").await;
    let bm = tree["bookmarks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == bm_id.as_str())
        .unwrap();
    assert_eq!(bm["title"], "Original");
    assert_eq!(bm["url"], "https://original.com");
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn history_nonexistent_bookmark_404() {
    let (app, _) = make_app();
    let resp = app
        .oneshot(
            Request::get("/bookmarks/nonexistent/history")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn revert_invalid_hash_400() {
    let (app, root_id) = make_app();

    let (_, resp) = post_json(
        app.clone(),
        &format!("/folders/{root_id}/bookmarks"),
        serde_json::json!({
            "url": "https://example.com",
            "title": "Test"
        }),
    )
    .await;
    let bm_id = resp["id"].as_str().unwrap().to_owned();

    let status = post_json(
        app,
        &format!("/bookmarks/{bm_id}/revert"),
        serde_json::json!({ "target_hash": "not-a-valid-hash" }),
    )
    .await
    .0;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

// ─── RFC 9457 Problem Details tests ────────────────────

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn api_not_found_returns_problem_json() {
    let (app, _) = make_app();

    let resp = app
        .oneshot(
            Request::get("/folders/nonexistent-id")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "application/problem+json"
    );

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["type"], "urn:mybriefcase:error:not-found");
    assert_eq!(json["status"], 404);
    assert_eq!(json["title"], "Not Found");
    assert!(!json["detail"].as_str().unwrap().is_empty());
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn api_validation_error_returns_problem_json() {
    let (app, root_id) = make_app();

    let (_, resp) = post_json(
        app.clone(),
        "/folders",
        serde_json::json!({
            "parent_folder_id": root_id,
            "title": "Folder"
        }),
    )
    .await;
    let folder_id = resp["id"].as_str().unwrap().to_owned();

    let resp = app
        .oneshot(
            Request::post("/move")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "item_id": folder_id,
                        "from_folder_id": root_id,
                        "to_folder_id": folder_id,
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "application/problem+json"
    );

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["type"], "urn:mybriefcase:error:validation");
    assert_eq!(json["status"], 422);
    assert_eq!(json["title"], "Validation Error");
    assert!(json["detail"].as_str().unwrap().contains("itself"));
}
