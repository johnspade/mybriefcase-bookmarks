// Canonical implementation: crates/core/src/invariants.rs (used as ops.rs postconditions).
// This copy exists because #[cfg(test)] modules are not visible to integration tests.
use mybriefcase_bookmarks::model::BookmarkStore;
use std::collections::{BTreeSet, HashSet};

pub fn assert_stores_converged(a: &BookmarkStore, b: &BookmarkStore) {
    assert_eq!(
        a.bookmarks.len(),
        b.bookmarks.len(),
        "bookmark count mismatch: {} vs {}",
        a.bookmarks.len(),
        b.bookmarks.len()
    );
    assert_eq!(
        a.folders.len(),
        b.folders.len(),
        "folder count mismatch: {} vs {}",
        a.folders.len(),
        b.folders.len()
    );
    for (id, bm_a) in &a.bookmarks {
        let bm_b = b
            .bookmarks
            .get(id)
            .unwrap_or_else(|| panic!("bookmark {id} missing in peer B"));
        assert_eq!(bm_a.url, bm_b.url, "url mismatch for bookmark {id}");
        assert_eq!(bm_a.title, bm_b.title, "title mismatch for bookmark {id}");
        assert_eq!(
            bm_a.deleted, bm_b.deleted,
            "deleted mismatch for bookmark {id}"
        );
    }
    for (id, f_a) in &a.folders {
        let f_b = b
            .folders
            .get(id)
            .unwrap_or_else(|| panic!("folder {id} missing in peer B"));
        assert_eq!(f_a.title, f_b.title, "title mismatch for folder {id}");
        assert_eq!(f_a.deleted, f_b.deleted, "deleted mismatch for folder {id}");
        let set_a: BTreeSet<_> = f_a.children.iter().collect();
        let set_b: BTreeSet<_> = f_b.children.iter().collect();
        assert_eq!(set_a, set_b, "children set mismatch for folder {id}");
    }
}

pub fn assert_valid_tree(store: &BookmarkStore) {
    assert_no_cycles(store, &store.root_folder_id, &mut HashSet::new());
}

fn assert_no_cycles(store: &BookmarkStore, folder_id: &str, path: &mut HashSet<String>) {
    assert!(
        path.insert(folder_id.to_owned()),
        "cycle detected: folder {folder_id} is an ancestor of itself"
    );
    if let Some(folder) = store.folders.get(folder_id) {
        if !folder.deleted {
            for child in &folder.children {
                if let Some(sub) = store.folders.get(child) {
                    if !sub.deleted {
                        assert_no_cycles(store, child, path);
                    }
                }
            }
        }
    }
    path.remove(folder_id);
}

pub fn assert_structural_integrity(store: &BookmarkStore) {
    // All children refs point to existing bookmark or folder IDs.
    // This invariant always holds: ops never insert references to
    // IDs that don't exist in the document.
    for (folder_id, folder) in &store.folders {
        if folder.deleted {
            continue;
        }
        for child_id in &folder.children {
            assert!(
                store.bookmarks.contains_key(child_id) || store.folders.contains_key(child_id),
                "folder {folder_id} has orphaned child ref: {child_id}"
            );
        }
    }
}

pub fn assert_cascade_complete(store: &BookmarkStore, deleted_folder_id: &str) {
    let folder = store
        .folders
        .get(deleted_folder_id)
        .expect("folder should exist");
    assert!(folder.deleted, "target folder not marked deleted");

    let mut stack = vec![deleted_folder_id.to_owned()];
    let mut visited = HashSet::new();
    while let Some(fid) = stack.pop() {
        if !visited.insert(fid.clone()) {
            continue;
        }
        if let Some(f) = store.folders.get(&fid) {
            for child_id in &f.children {
                if let Some(sub) = store.folders.get(child_id) {
                    assert!(
                        sub.deleted,
                        "descendant folder {child_id} not marked deleted"
                    );
                    stack.push(child_id.clone());
                } else if let Some(bm) = store.bookmarks.get(child_id) {
                    assert!(
                        bm.deleted,
                        "descendant bookmark {child_id} not marked deleted"
                    );
                }
            }
        }
    }
}
