#[expect(
    dead_code,
    reason = "shared test helpers; not all used by every test binary"
)]
mod common;

use autosurgeon::hydrate;
use mybriefcase_bookmarks::model::BookmarkStore;
use mybriefcase_bookmarks::ops;
use mybriefcase_bookmarks::repo;
use tempfile::TempDir;

use common::{fork_doc, new_initialized_doc};

fn hydrate_store(doc: &automerge_repo::DocHandle) -> BookmarkStore {
    doc.with_doc(|d| hydrate(d).unwrap())
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn export_creates_snapshot() {
    let td = new_initialized_doc("client-a");
    let sync_root = TempDir::new().unwrap();

    repo::export_doc_to_shared(
        &td.doc_handle,
        sync_root.path(),
        "client-a",
        std::time::SystemTime::now(),
    )
    .unwrap();

    let snapshot = sync_root
        .path()
        .join("client-a")
        .join("store")
        .join("document.snapshot");
    assert!(snapshot.exists());
    assert!(std::fs::metadata(&snapshot).unwrap().len() > 0);
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn export_no_tmp_file_remains() {
    let td = new_initialized_doc("client-a");
    let sync_root = TempDir::new().unwrap();

    repo::export_doc_to_shared(
        &td.doc_handle,
        sync_root.path(),
        "client-a",
        std::time::SystemTime::now(),
    )
    .unwrap();

    let store_dir = sync_root.path().join("client-a").join("store");
    for entry in std::fs::read_dir(&store_dir).unwrap() {
        let entry = entry.unwrap();
        assert_ne!(
            entry.path().extension().and_then(|e| e.to_str()),
            Some("tmp")
        );
    }
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn full_merge_pass_loads_peers() {
    let base = new_initialized_doc("base");
    let root_id = base.root_folder_id.clone();

    let peer_a = fork_doc(&base, "peer-a");
    let _ = ops::add_bookmark(
        &peer_a.doc_handle,
        &root_id,
        "https://from-peer-a.com",
        "From A",
    );

    let sync_root = TempDir::new().unwrap();
    repo::export_doc_to_shared(
        &peer_a.doc_handle,
        sync_root.path(),
        "peer-a",
        std::time::SystemTime::now(),
    )
    .unwrap();

    let local = fork_doc(&base, "local");
    let changed = repo::full_merge_pass(&local.doc_handle, sync_root.path(), "local");
    assert!(changed);

    let store = hydrate_store(&local.doc_handle);
    assert!(
        store.bookmarks.values().any(|b| b.title == "From A"),
        "Local client should see peer-a's bookmark"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn full_merge_pass_skips_own_dir() {
    let base = new_initialized_doc("my-client");
    let _ = ops::add_bookmark(
        &base.doc_handle,
        &base.root_folder_id,
        "https://self.com",
        "Self",
    );

    let sync_root = TempDir::new().unwrap();
    repo::export_doc_to_shared(
        &base.doc_handle,
        sync_root.path(),
        "my-client",
        std::time::SystemTime::now(),
    )
    .unwrap();

    let fresh = fork_doc(&base, "my-client");
    let changed = repo::full_merge_pass(&fresh.doc_handle, sync_root.path(), "my-client");
    assert!(!changed, "Should not merge own directory");
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn full_merge_pass_skips_dotdirs() {
    let base = new_initialized_doc("peer-x");
    let _ = ops::add_bookmark(
        &base.doc_handle,
        &base.root_folder_id,
        "https://hidden.com",
        "Hidden",
    );

    let sync_root = TempDir::new().unwrap();
    let dot_dir = sync_root.path().join(".hidden").join("store");
    std::fs::create_dir_all(&dot_dir).unwrap();
    let data = base.doc_handle.with_doc(automerge::Automerge::save);
    std::fs::write(dot_dir.join("document.snapshot"), &data).unwrap();

    let fresh = fork_doc(&base, "local");
    let changed = repo::full_merge_pass(&fresh.doc_handle, sync_root.path(), "local");
    assert!(!changed, "Should not merge dot-prefixed directories");
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn idempotent_merge() {
    let base = new_initialized_doc("base");
    let root_id = base.root_folder_id.clone();

    let peer = fork_doc(&base, "peer-a");
    let _ = ops::add_bookmark(&peer.doc_handle, &root_id, "https://example.com", "Test");

    let sync_root = TempDir::new().unwrap();
    repo::export_doc_to_shared(
        &peer.doc_handle,
        sync_root.path(),
        "peer-a",
        std::time::SystemTime::now(),
    )
    .unwrap();

    let local = fork_doc(&base, "local");
    repo::full_merge_pass(&local.doc_handle, sync_root.path(), "local");
    let store_after_first = hydrate_store(&local.doc_handle);

    repo::full_merge_pass(&local.doc_handle, sync_root.path(), "local");
    let store_after_second = hydrate_store(&local.doc_handle);

    assert_eq!(
        store_after_first.bookmarks.len(),
        store_after_second.bookmarks.len()
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn multi_peer_merge() {
    let base = new_initialized_doc("base");
    let root_id = base.root_folder_id.clone();
    let sync_root = TempDir::new().unwrap();

    for i in 0..3 {
        let peer = fork_doc(&base, &format!("peer-{i}"));
        let _ = ops::add_bookmark(
            &peer.doc_handle,
            &root_id,
            &format!("https://peer{i}.com"),
            &format!("Peer {i}"),
        );
        repo::export_doc_to_shared(
            &peer.doc_handle,
            sync_root.path(),
            &format!("peer-{i}"),
            std::time::SystemTime::now(),
        )
        .unwrap();
    }

    let local = fork_doc(&base, "local");
    let changed = repo::full_merge_pass(&local.doc_handle, sync_root.path(), "local");
    assert!(changed);

    let store = hydrate_store(&local.doc_handle);
    assert_eq!(store.bookmarks.len(), 3);
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn init_repo_fresh() {
    let local_dir = TempDir::new().unwrap();
    let sync_root = TempDir::new().unwrap();

    let (_repo_handle, doc_handle) = repo::init_repo(
        local_dir.path(),
        sync_root.path(),
        "test-client",
        chrono::Utc::now(),
    )
    .await
    .unwrap();

    let store = hydrate_store(&doc_handle);
    assert!(!store.root_folder_id.is_empty());
    assert_eq!(store.meta.schema_version, 1);

    let root = store.folders.get(&store.root_folder_id).unwrap();
    assert_eq!(root.title, "Bookmarks");
    assert_eq!(root.children.len(), 2);

    let child_titles: Vec<&str> = root
        .children
        .iter()
        .filter_map(|id| store.folders.get(id))
        .map(|f| f.title.as_str())
        .collect();
    assert!(child_titles.contains(&"Bookmarks Bar"));
    assert!(child_titles.contains(&"Other Bookmarks"));
}
