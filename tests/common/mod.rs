use automerge::ObjType;
use automerge::transaction::{CommitOptions, Transactable};
use automerge_repo::tokio::FsStorage;
use automerge_repo::{DocHandle, Repo};
use axum::Router;
use axum::routing::{delete, get, post, put};
use mybriefcase_bookmarks::{api, repo, state};
use std::sync::Arc;
use tempfile::TempDir;

pub struct TestDoc {
    pub temp_dir: TempDir,
    pub doc_handle: DocHandle,
    pub root_folder_id: String,
}

pub fn new_initialized_doc(client_id: &str) -> TestDoc {
    let temp_dir = TempDir::new().unwrap();
    let store = FsStorage::open(temp_dir.path()).unwrap();
    let repo = Repo::new(Some(client_id.to_string()), Box::new(store));
    let repo_handle = repo.run();
    let doc_handle = repo_handle.new_document();

    let root_id = uuid::Uuid::new_v4().to_string();
    doc_handle.with_doc_mut(|doc| {
        let mut tx = doc.transaction();
        let now = chrono::Utc::now().to_rfc3339();

        tx.put(automerge::ROOT, "root_folder_id", root_id.as_str())
            .unwrap();
        let folders = tx
            .put_object(automerge::ROOT, "folders", ObjType::Map)
            .unwrap();
        tx.put_object(automerge::ROOT, "bookmarks", ObjType::Map)
            .unwrap();
        let meta = tx
            .put_object(automerge::ROOT, "meta", ObjType::Map)
            .unwrap();
        tx.put(&meta, "schema_version", 1_u64).unwrap();
        tx.put(&meta, "collection_name", "bookmarks").unwrap();

        let root = tx
            .put_object(&folders, root_id.as_str(), ObjType::Map)
            .unwrap();
        tx.put(&root, "title", "Bookmarks").unwrap();
        tx.put_object(&root, "children", ObjType::List).unwrap();
        tx.put(&root, "created_at", now.as_str()).unwrap();
        tx.put(&root, "updated_at", now.as_str()).unwrap();
        tx.put(&root, "deleted", false).unwrap();
        tx.commit_with(CommitOptions::default().with_message("init_schema"));
    });

    TestDoc {
        temp_dir,
        doc_handle,
        root_folder_id: root_id,
    }
}

pub fn fork_doc(source: &TestDoc, new_client_id: &str) -> TestDoc {
    let temp_dir = TempDir::new().unwrap();
    let store = FsStorage::open(temp_dir.path()).unwrap();
    let repo = Repo::new(Some(new_client_id.to_string()), Box::new(store));
    let repo_handle = repo.run();
    let doc_handle = repo_handle.new_document();

    let data = source.doc_handle.with_doc(automerge::Automerge::save);
    doc_handle.with_doc_mut(|d| {
        let mut source_doc = automerge::Automerge::load(&data).unwrap();
        d.merge(&mut source_doc).unwrap();
    });

    TestDoc {
        temp_dir,
        doc_handle,
        root_folder_id: source.root_folder_id.clone(),
    }
}

pub fn merge_docs(source: &DocHandle, target: &DocHandle) {
    let data = source.with_doc(automerge::Automerge::save);
    target.with_doc_mut(|d| {
        let mut peer = automerge::Automerge::load(&data).unwrap();
        d.merge(&mut peer).unwrap();
    });
}

pub fn build_app(
    doc_handle: DocHandle,
    sync_root: std::path::PathBuf,
    client_id: String,
) -> Router {
    let (sse_tx, _) = tokio::sync::broadcast::channel::<()>(16);
    let exporter = repo::Exporter::new(&sync_root, &client_id);
    let state = Arc::new(state::AppState {
        doc_handle,
        sync_root,
        client_id,
        sse_tx,
        static_version: "test".to_string(),
        exporter,
    });

    Router::new()
        .route("/", get(api::get_tree))
        .route("/folders/{id}", get(api::get_folder))
        .route("/folders", post(api::create_folder))
        .route("/folders/{id}/bookmarks", post(api::create_bookmark))
        .route("/bookmarks/{id}", put(api::update_bookmark))
        .route("/bookmarks/{id}", delete(api::delete_bookmark))
        .route("/bookmarks/{id}/history", get(api::get_bookmark_history))
        .route("/bookmarks/{id}/at/{hash}", get(api::get_bookmark_at_hash))
        .route("/bookmarks/{id}/revert", post(api::revert_bookmark))
        .route("/move", post(api::move_item))
        .route("/export", get(api::export_bookmarks))
        .with_state(state)
}
