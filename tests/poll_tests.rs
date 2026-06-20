#[expect(
    dead_code,
    reason = "shared test helpers; not all used by every test binary"
)]
mod common;

use autosurgeon::hydrate;
use mybriefcase_bookmarks::model::BookmarkStore;
use mybriefcase_bookmarks::ops;
use mybriefcase_bookmarks::repo;
use mybriefcase_bookmarks::watcher;
use std::time::Duration;
use tempfile::TempDir;

use common::{fork_doc, new_initialized_doc};

fn hydrate_store(doc: &automerge_repo::DocHandle) -> BookmarkStore {
    doc.with_doc(|d| hydrate(d).unwrap())
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn poll_detects_and_merges_new_peer() {
    let base = new_initialized_doc("base");
    let root_id = base.root_folder_id.clone();
    let sync_root = TempDir::new().unwrap();

    let local = fork_doc(&base, "local");
    repo::export_doc_to_shared(
        &local.doc_handle,
        sync_root.path(),
        "local",
        std::time::SystemTime::now(),
    )
    .unwrap();

    let mut poll = watcher::PollState::new(sync_root.path(), "local");

    let peer = fork_doc(&base, "peer-a");
    let _ = ops::add_bookmark(
        &peer.doc_handle,
        &root_id,
        "https://from-peer.com",
        "From Peer",
    );
    repo::export_doc_to_shared(
        &peer.doc_handle,
        sync_root.path(),
        "peer-a",
        std::time::SystemTime::now(),
    )
    .unwrap();

    let changed = poll.poll_changed_peers(sync_root.path(), "local");
    assert_eq!(changed, vec!["peer-a"]);

    let did_merge = watcher::merge_specific_peers(&local.doc_handle, sync_root.path(), &changed);
    assert!(did_merge);

    let store = hydrate_store(&local.doc_handle);
    assert!(
        store.bookmarks.values().any(|b| b.title == "From Peer"),
        "local should see peer-a's bookmark after poll+merge"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn poll_detects_updated_peer_snapshot() {
    let base = new_initialized_doc("base");
    let root_id = base.root_folder_id.clone();
    let sync_root = TempDir::new().unwrap();

    let local = fork_doc(&base, "local");
    let mut poll = watcher::PollState::new(sync_root.path(), "local");

    // Peer creates initial snapshot after poll is seeded.
    let peer = fork_doc(&base, "peer-a");
    let _ = ops::add_bookmark(&peer.doc_handle, &root_id, "https://v1.com", "Version 1");
    repo::export_doc_to_shared(
        &peer.doc_handle,
        sync_root.path(),
        "peer-a",
        std::time::SystemTime::now(),
    )
    .unwrap();

    // First poll: detect and merge the new snapshot.
    let changed = poll.poll_changed_peers(sync_root.path(), "local");
    assert!(!changed.is_empty());
    watcher::merge_specific_peers(&local.doc_handle, sync_root.path(), &changed);

    // Peer updates its doc and re-exports.
    std::thread::sleep(Duration::from_millis(1100));
    let _ = ops::add_bookmark(&peer.doc_handle, &root_id, "https://v2.com", "Version 2");
    repo::export_doc_to_shared(
        &peer.doc_handle,
        sync_root.path(),
        "peer-a",
        std::time::SystemTime::now(),
    )
    .unwrap();

    let changed = poll.poll_changed_peers(sync_root.path(), "local");
    assert_eq!(changed, vec!["peer-a"]);

    watcher::merge_specific_peers(&local.doc_handle, sync_root.path(), &changed);
    let store = hydrate_store(&local.doc_handle);
    assert!(store.bookmarks.values().any(|b| b.title == "Version 2"));
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn poll_quiet_when_nothing_changed() {
    let base = new_initialized_doc("base");
    let root_id = base.root_folder_id.clone();
    let sync_root = TempDir::new().unwrap();

    let local = fork_doc(&base, "local");
    let mut poll = watcher::PollState::new(sync_root.path(), "local");

    // Peer writes after poll is seeded.
    let peer = fork_doc(&base, "peer-a");
    let _ = ops::add_bookmark(&peer.doc_handle, &root_id, "https://stable.com", "Stable");
    repo::export_doc_to_shared(
        &peer.doc_handle,
        sync_root.path(),
        "peer-a",
        std::time::SystemTime::now(),
    )
    .unwrap();

    // Drain the initial detection.
    let first = poll.poll_changed_peers(sync_root.path(), "local");
    assert!(!first.is_empty());
    watcher::merge_specific_peers(&local.doc_handle, sync_root.path(), &first);

    // Subsequent polls with no changes should return empty.
    for _ in 0..3 {
        let changed = poll.poll_changed_peers(sync_root.path(), "local");
        assert!(
            changed.is_empty(),
            "poll should be quiet when nothing changed"
        );
    }
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn poll_multiple_peers_simultaneously() {
    let base = new_initialized_doc("base");
    let root_id = base.root_folder_id.clone();
    let sync_root = TempDir::new().unwrap();

    let local = fork_doc(&base, "local");
    repo::export_doc_to_shared(
        &local.doc_handle,
        sync_root.path(),
        "local",
        std::time::SystemTime::now(),
    )
    .unwrap();

    let mut poll = watcher::PollState::new(sync_root.path(), "local");

    for i in 0..3 {
        let peer = fork_doc(&base, &format!("peer-{i}"));
        let _ = ops::add_bookmark(
            &peer.doc_handle,
            &root_id,
            &format!("https://peer{i}.example.com"),
            &format!("Peer {i} Bookmark"),
        );
        repo::export_doc_to_shared(
            &peer.doc_handle,
            sync_root.path(),
            &format!("peer-{i}"),
            std::time::SystemTime::now(),
        )
        .unwrap();
    }

    let mut changed = poll.poll_changed_peers(sync_root.path(), "local");
    changed.sort();
    assert_eq!(changed, vec!["peer-0", "peer-1", "peer-2"]);

    let did_merge = watcher::merge_specific_peers(&local.doc_handle, sync_root.path(), &changed);
    assert!(did_merge);

    let store = hydrate_store(&local.doc_handle);
    assert_eq!(store.bookmarks.len(), 3);
    for i in 0..3 {
        assert!(
            store
                .bookmarks
                .values()
                .any(|b| b.title == format!("Peer {i} Bookmark")),
            "local should see peer-{i}'s bookmark"
        );
    }
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn poll_bidirectional_sync() {
    let base = new_initialized_doc("base");
    let root_id = base.root_folder_id.clone();
    let sync_root = TempDir::new().unwrap();

    let node_a = fork_doc(&base, "node-a");
    let node_b = fork_doc(&base, "node-b");
    repo::export_doc_to_shared(
        &node_a.doc_handle,
        sync_root.path(),
        "node-a",
        std::time::SystemTime::now(),
    )
    .unwrap();
    repo::export_doc_to_shared(
        &node_b.doc_handle,
        sync_root.path(),
        "node-b",
        std::time::SystemTime::now(),
    )
    .unwrap();

    let mut poll_a = watcher::PollState::new(sync_root.path(), "node-a");
    let mut poll_b = watcher::PollState::new(sync_root.path(), "node-b");

    // Node A creates a bookmark and re-exports.
    let _ = ops::add_bookmark(&node_a.doc_handle, &root_id, "https://from-a.com", "From A");
    repo::export_doc_to_shared(
        &node_a.doc_handle,
        sync_root.path(),
        "node-a",
        std::time::SystemTime::now(),
    )
    .unwrap();

    // Node B polls and merges A's change.
    let changed_b = poll_b.poll_changed_peers(sync_root.path(), "node-b");
    assert!(changed_b.contains(&"node-a".to_owned()));
    watcher::merge_specific_peers(&node_b.doc_handle, sync_root.path(), &changed_b);

    let store_b = hydrate_store(&node_b.doc_handle);
    assert!(store_b.bookmarks.values().any(|b| b.title == "From A"));

    // Node B creates its own bookmark and re-exports.
    std::thread::sleep(Duration::from_millis(1100));
    let _ = ops::add_bookmark(&node_b.doc_handle, &root_id, "https://from-b.com", "From B");
    repo::export_doc_to_shared(
        &node_b.doc_handle,
        sync_root.path(),
        "node-b",
        std::time::SystemTime::now(),
    )
    .unwrap();

    // Node A polls and merges B's change.
    let changed_a = poll_a.poll_changed_peers(sync_root.path(), "node-a");
    assert!(changed_a.contains(&"node-b".to_owned()));
    watcher::merge_specific_peers(&node_a.doc_handle, sync_root.path(), &changed_a);

    let store_a = hydrate_store(&node_a.doc_handle);
    assert!(store_a.bookmarks.values().any(|b| b.title == "From B"));

    // Re-export A so B can pick up A's merged state (A has both, B needs to re-merge).
    repo::export_doc_to_shared(
        &node_a.doc_handle,
        sync_root.path(),
        "node-a",
        std::time::SystemTime::now(),
    )
    .unwrap();

    std::thread::sleep(Duration::from_millis(1100));
    let changed_b2 = poll_b.poll_changed_peers(sync_root.path(), "node-b");
    if !changed_b2.is_empty() {
        watcher::merge_specific_peers(&node_b.doc_handle, sync_root.path(), &changed_b2);
    }
    let store_b_final = hydrate_store(&node_b.doc_handle);
    assert_eq!(
        store_a.bookmarks.len(),
        store_b_final.bookmarks.len(),
        "both nodes should converge to the same bookmark count"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn reexport_after_merge_propagates_to_third_peer() {
    let base = new_initialized_doc("base");
    let root_id = base.root_folder_id.clone();
    let sync_root = TempDir::new().unwrap();

    let node_a = fork_doc(&base, "node-a");
    let node_b = fork_doc(&base, "node-b");
    let node_c = fork_doc(&base, "node-c");
    repo::export_doc_to_shared(
        &node_a.doc_handle,
        sync_root.path(),
        "node-a",
        std::time::SystemTime::now(),
    )
    .unwrap();
    repo::export_doc_to_shared(
        &node_b.doc_handle,
        sync_root.path(),
        "node-b",
        std::time::SystemTime::now(),
    )
    .unwrap();
    repo::export_doc_to_shared(
        &node_c.doc_handle,
        sync_root.path(),
        "node-c",
        std::time::SystemTime::now(),
    )
    .unwrap();

    let mut poll_b = watcher::PollState::new(sync_root.path(), "node-b");
    let mut poll_c = watcher::PollState::new(sync_root.path(), "node-c");

    // Node A adds a bookmark and exports. Node C cannot see node-a's directory
    // (simulating partial Syncthing topology: A syncs with B, B syncs with C).
    let _ = ops::add_bookmark(&node_a.doc_handle, &root_id, "https://from-a.com", "From A");
    repo::export_doc_to_shared(
        &node_a.doc_handle,
        sync_root.path(),
        "node-a",
        std::time::SystemTime::now(),
    )
    .unwrap();

    // Node B polls, merges A's data, and re-exports (the behavior under test).
    std::thread::sleep(Duration::from_millis(1100));
    let changed_b = poll_b.poll_changed_peers(sync_root.path(), "node-b");
    let did_merge = watcher::merge_specific_peers(&node_b.doc_handle, sync_root.path(), &changed_b);
    assert!(did_merge);
    repo::export_doc_to_shared(
        &node_b.doc_handle,
        sync_root.path(),
        "node-b",
        std::time::SystemTime::now(),
    )
    .unwrap();

    // Node C polls and merges only from node-b (not node-a directly).
    std::thread::sleep(Duration::from_millis(1100));
    let _changed_c = poll_c.poll_changed_peers(sync_root.path(), "node-c");
    watcher::merge_specific_peers(
        &node_c.doc_handle,
        sync_root.path(),
        &[String::from("node-b")],
    );

    let store_c = hydrate_store(&node_c.doc_handle);
    assert!(
        store_c.bookmarks.values().any(|b| b.title == "From A"),
        "node-c should see node-a's bookmark transitively via node-b's re-export"
    );
}
