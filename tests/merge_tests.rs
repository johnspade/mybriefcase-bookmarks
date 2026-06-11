#[expect(
    dead_code,
    reason = "shared test helpers; not all used by every test binary"
)]
mod common;

use automerge_playground::model::BookmarkStore;
use automerge_playground::ops;
use autosurgeon::hydrate;

use common::{fork_doc, merge_docs, new_initialized_doc};

fn hydrate_store(doc: &automerge_repo::DocHandle) -> BookmarkStore {
    doc.with_doc(|d| hydrate(d).unwrap())
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn concurrent_add_both_visible() {
    let peer_a = new_initialized_doc("peer-a");
    let peer_b = fork_doc(&peer_a, "peer-b");
    let root_id = peer_a.root_folder_id.clone();

    let bm_a = ops::add_bookmark(
        &peer_a.doc_handle,
        &root_id,
        "https://a.example.com",
        "From A",
    )
    .unwrap();
    let bm_b = ops::add_bookmark(
        &peer_b.doc_handle,
        &root_id,
        "https://b.example.com",
        "From B",
    )
    .unwrap();

    merge_docs(&peer_a.doc_handle, &peer_b.doc_handle);
    merge_docs(&peer_b.doc_handle, &peer_a.doc_handle);

    let store_a = hydrate_store(&peer_a.doc_handle);
    let store_b = hydrate_store(&peer_b.doc_handle);

    assert!(store_a.bookmarks.contains_key(&bm_a));
    assert!(store_a.bookmarks.contains_key(&bm_b));
    assert!(store_b.bookmarks.contains_key(&bm_a));
    assert!(store_b.bookmarks.contains_key(&bm_b));
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn concurrent_edit_same_field() {
    let peer_a = new_initialized_doc("peer-a");
    let root_id = peer_a.root_folder_id.clone();

    let bm_id = ops::add_bookmark(
        &peer_a.doc_handle,
        &root_id,
        "https://example.com",
        "Original",
    )
    .unwrap();

    // Fork after bookmark exists so both have it
    let peer_b = fork_doc(&peer_a, "peer-b");

    ops::update_bookmark(&peer_a.doc_handle, &bm_id, None, Some("Title from A"), None).unwrap();
    ops::update_bookmark(&peer_b.doc_handle, &bm_id, None, Some("Title from B"), None).unwrap();

    merge_docs(&peer_a.doc_handle, &peer_b.doc_handle);
    merge_docs(&peer_b.doc_handle, &peer_a.doc_handle);

    let store_a = hydrate_store(&peer_a.doc_handle);
    let store_b = hydrate_store(&peer_b.doc_handle);

    let title_a = &store_a.bookmarks.get(&bm_id).unwrap().title;
    let title_b = &store_b.bookmarks.get(&bm_id).unwrap().title;
    assert_eq!(
        title_a, title_b,
        "Both peers must converge to the same title"
    );
    assert!(
        title_a == "Title from A" || title_a == "Title from B",
        "Winner must be one of the two concurrent values, got: {title_a}"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn delete_vs_edit_conflict() {
    let peer_a = new_initialized_doc("peer-a");
    let root_id = peer_a.root_folder_id.clone();

    let bm_id =
        ops::add_bookmark(&peer_a.doc_handle, &root_id, "https://example.com", "Test").unwrap();

    let peer_b = fork_doc(&peer_a, "peer-b");

    ops::delete_bookmark(&peer_a.doc_handle, &bm_id).unwrap();
    ops::update_bookmark(&peer_b.doc_handle, &bm_id, None, Some("Edited"), None).unwrap();

    merge_docs(&peer_a.doc_handle, &peer_b.doc_handle);
    merge_docs(&peer_b.doc_handle, &peer_a.doc_handle);

    let store_a = hydrate_store(&peer_a.doc_handle);
    let store_b = hydrate_store(&peer_b.doc_handle);

    let deleted_a = store_a.bookmarks.get(&bm_id).unwrap().deleted;
    let deleted_b = store_b.bookmarks.get(&bm_id).unwrap().deleted;
    assert_eq!(
        deleted_a, deleted_b,
        "Both peers must agree on deleted state"
    );
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn concurrent_move_same_item() {
    let peer_a = new_initialized_doc("peer-a");
    let root_id = peer_a.root_folder_id.clone();

    let folder_x = ops::create_folder(&peer_a.doc_handle, &root_id, "X").unwrap();
    let folder_y = ops::create_folder(&peer_a.doc_handle, &root_id, "Y").unwrap();
    let folder_z = ops::create_folder(&peer_a.doc_handle, &root_id, "Z").unwrap();
    let bm_id = ops::add_bookmark(
        &peer_a.doc_handle,
        &folder_x,
        "https://example.com",
        "Moveable",
    )
    .unwrap();

    let peer_b = fork_doc(&peer_a, "peer-b");

    ops::move_item(&peer_a.doc_handle, &bm_id, &folder_x, &folder_y).unwrap();
    ops::move_item(&peer_b.doc_handle, &bm_id, &folder_x, &folder_z).unwrap();

    merge_docs(&peer_a.doc_handle, &peer_b.doc_handle);
    merge_docs(&peer_b.doc_handle, &peer_a.doc_handle);

    let store_a = hydrate_store(&peer_a.doc_handle);
    let store_b = hydrate_store(&peer_b.doc_handle);

    let count_in_folders = |store: &BookmarkStore| -> usize {
        store
            .folders
            .values()
            .map(|f| f.children.iter().filter(|c| *c == &bm_id).count())
            .sum()
    };

    let count_a = count_in_folders(&store_a);
    let count_b = count_in_folders(&store_b);
    assert_eq!(count_a, count_b, "Both peers must converge");
    assert!(count_a >= 1, "Item must appear at least once");
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn concurrent_folder_creation() {
    let peer_a = new_initialized_doc("peer-a");
    let root_id = peer_a.root_folder_id.clone();
    let peer_b = fork_doc(&peer_a, "peer-b");

    let folder_a = ops::create_folder(&peer_a.doc_handle, &root_id, "SharedName").unwrap();
    let folder_b = ops::create_folder(&peer_b.doc_handle, &root_id, "SharedName").unwrap();

    merge_docs(&peer_a.doc_handle, &peer_b.doc_handle);
    merge_docs(&peer_b.doc_handle, &peer_a.doc_handle);

    let store_a = hydrate_store(&peer_a.doc_handle);
    let store_b = hydrate_store(&peer_b.doc_handle);

    assert!(store_a.folders.contains_key(&folder_a));
    assert!(store_a.folders.contains_key(&folder_b));
    assert!(store_b.folders.contains_key(&folder_a));
    assert!(store_b.folders.contains_key(&folder_b));
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn merge_symmetry() {
    let peer_a = new_initialized_doc("peer-a");
    let root_id = peer_a.root_folder_id.clone();
    let peer_b = fork_doc(&peer_a, "peer-b");

    ops::add_bookmark(&peer_a.doc_handle, &root_id, "https://a.example.com", "A").unwrap();
    ops::add_bookmark(&peer_b.doc_handle, &root_id, "https://b.example.com", "B").unwrap();
    ops::create_folder(&peer_a.doc_handle, &root_id, "Folder A").unwrap();
    ops::create_folder(&peer_b.doc_handle, &root_id, "Folder B").unwrap();

    // Merge in order A then B
    let fresh1 = fork_doc(&peer_a, "fresh-1");
    merge_docs(&peer_b.doc_handle, &fresh1.doc_handle);

    // Merge in order B then A
    let fresh2 = fork_doc(&peer_b, "fresh-2");
    merge_docs(&peer_a.doc_handle, &fresh2.doc_handle);

    let store1 = hydrate_store(&fresh1.doc_handle);
    let store2 = hydrate_store(&fresh2.doc_handle);

    assert_eq!(store1.bookmarks.len(), store2.bookmarks.len());
    assert_eq!(store1.folders.len(), store2.folders.len());
    for key in store1.bookmarks.keys() {
        assert!(store2.bookmarks.contains_key(key));
    }
    for key in store1.folders.keys() {
        assert!(store2.folders.contains_key(key));
    }
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn three_way_merge_convergence() {
    let peer_a = new_initialized_doc("peer-a");
    let root_id = peer_a.root_folder_id.clone();
    let peer_b = fork_doc(&peer_a, "peer-b");
    let peer_c = fork_doc(&peer_a, "peer-c");

    ops::add_bookmark(&peer_a.doc_handle, &root_id, "https://a.com", "A").unwrap();
    ops::add_bookmark(&peer_b.doc_handle, &root_id, "https://b.com", "B").unwrap();
    ops::add_bookmark(&peer_c.doc_handle, &root_id, "https://c.com", "C").unwrap();

    // Merge all into each
    merge_docs(&peer_b.doc_handle, &peer_a.doc_handle);
    merge_docs(&peer_c.doc_handle, &peer_a.doc_handle);

    merge_docs(&peer_a.doc_handle, &peer_b.doc_handle);
    merge_docs(&peer_c.doc_handle, &peer_b.doc_handle);

    merge_docs(&peer_a.doc_handle, &peer_c.doc_handle);
    merge_docs(&peer_b.doc_handle, &peer_c.doc_handle);

    let store_a = hydrate_store(&peer_a.doc_handle);
    let store_b = hydrate_store(&peer_b.doc_handle);
    let store_c = hydrate_store(&peer_c.doc_handle);

    assert_eq!(store_a.bookmarks.len(), 3);
    assert_eq!(store_b.bookmarks.len(), 3);
    assert_eq!(store_c.bookmarks.len(), 3);

    let keys_a: std::collections::BTreeSet<_> = store_a.bookmarks.keys().collect();
    let keys_b: std::collections::BTreeSet<_> = store_b.bookmarks.keys().collect();
    let keys_c: std::collections::BTreeSet<_> = store_c.bookmarks.keys().collect();
    assert_eq!(keys_a, keys_b);
    assert_eq!(keys_b, keys_c);
}
