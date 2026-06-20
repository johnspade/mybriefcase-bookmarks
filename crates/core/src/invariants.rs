use crate::model::BookmarkStore;
use std::collections::HashSet;

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

/// # Panics
/// Panics if any non-deleted folder has a child reference to a nonexistent ID.
pub fn assert_structural_integrity(store: &BookmarkStore) {
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

/// # Panics
/// Panics if any descendant of the deleted folder is not marked deleted.
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
