use automerge::transaction::Transactable;
use automerge::{Automerge, ObjType, ReadDoc, Value};
use automerge_repo::tokio::FsStorage;
use automerge_repo::{DocHandle, DocumentId, Repo, RepoHandle};
use std::path::Path;

/// # Panics
/// Panics if the local storage cannot be initialized or the document cannot be loaded.
pub async fn init_repo(
    local_data_dir: &Path,
    sync_root: &Path,
    client_id: &str,
) -> (RepoHandle, DocHandle, DocumentId) {
    let local_store_path = local_data_dir.join("repo_store");
    std::fs::create_dir_all(&local_store_path).unwrap();
    let store = FsStorage::open(&local_store_path).unwrap();
    let repo = Repo::new(Some(client_id.to_string()), Box::new(store));
    let repo_handle = repo.run();

    let sync_info_path = sync_root.join(".bookmarks-sync");

    if sync_info_path.exists() {
        // Existing sync folder. Try loading from local storage first.
        let info: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&sync_info_path).unwrap()).unwrap();
        let doc_id: DocumentId = info["document_id"].as_str().unwrap().parse().unwrap();

        if let Some(handle) = repo_handle.load(doc_id.clone()).await.unwrap() {
            return (repo_handle, handle, doc_id);
        }

        // Not in local storage: create a new doc and merge from peers.
        let handle = repo_handle.new_document();
        full_merge_pass(&handle, sync_root, client_id);
        let actual_id = handle.document_id();
        (repo_handle, handle, actual_id)
    } else {
        // First client: create document with default folder structure.
        let handle = repo_handle.new_document();
        handle.with_doc_mut(|doc| {
            let mut tx = doc.transaction();
            let now = chrono::Utc::now().to_rfc3339();
            let root_id = uuid::Uuid::new_v4().to_string();

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
            let root_children = tx.put_object(&root, "children", ObjType::List).unwrap();
            tx.put(&root, "created_at", now.as_str()).unwrap();
            tx.put(&root, "updated_at", now.as_str()).unwrap();
            tx.put(&root, "deleted", false).unwrap();

            for (i, title) in ["Bookmarks Bar", "Other Bookmarks"].iter().enumerate() {
                let sub_id = uuid::Uuid::new_v4().to_string();
                let sub = tx
                    .put_object(&folders, sub_id.as_str(), ObjType::Map)
                    .unwrap();
                tx.put(&sub, "title", *title).unwrap();
                tx.put_object(&sub, "children", ObjType::List).unwrap();
                tx.put(&sub, "created_at", now.as_str()).unwrap();
                tx.put(&sub, "updated_at", now.as_str()).unwrap();
                tx.put(&sub, "deleted", false).unwrap();
                tx.insert(&root_children, i, sub_id.as_str()).unwrap();
            }
            tx.commit();
        });

        let doc_id = handle.document_id();
        let info = serde_json::json!({
            "version": 1,
            "engine": "automerge-repo",
            "app": "mybriefcase-bookmarks",
            "schema_version": 1,
            "document_id": doc_id.to_string()
        });
        std::fs::create_dir_all(sync_root).unwrap();
        std::fs::write(&sync_info_path, info.to_string()).unwrap();
        (repo_handle, handle, doc_id)
    }
}

pub fn full_merge_pass(doc_handle: &DocHandle, sync_root: &Path, own_client_id: &str) -> bool {
    let mut changed = false;
    for entry in std::fs::read_dir(sync_root).into_iter().flatten().flatten() {
        let peer_dir = entry.path();
        if !peer_dir.is_dir() {
            continue;
        }
        let Some(name) = peer_dir.file_name() else {
            continue;
        };
        let peer_id = name.to_string_lossy().to_string();
        if peer_id == own_client_id || peer_id.starts_with('.') || peer_id == "favicons" {
            continue;
        }

        let store_dir = peer_dir.join("store");
        if !store_dir.is_dir() {
            continue;
        }

        let peer_files = walk_files(&store_dir);
        for file_path in &peer_files {
            if let Ok(data) = std::fs::read(file_path) {
                doc_handle.with_doc_mut(|doc| {
                    if let Ok(mut peer_doc) = Automerge::load(&data) {
                        if doc.merge(&mut peer_doc).is_ok() {
                            changed = true;
                        }
                    } else if doc.load_incremental(&data).is_ok() {
                        changed = true;
                    }
                });
            }
        }
    }
    changed
}

pub fn export_doc_to_shared(doc_handle: &DocHandle, sync_root: &Path, client_id: &str) {
    let shared = sync_root.join(client_id).join("store");
    std::fs::create_dir_all(&shared).ok();

    let data = doc_handle.with_doc(automerge::Automerge::save);
    let dest = shared.join("document.snapshot");
    let tmp = dest.with_extension("tmp");
    std::fs::write(&tmp, &data).ok();
    std::fs::rename(&tmp, &dest).ok();
}

pub fn migrate_add_favicon_field(doc_handle: &DocHandle) {
    doc_handle.with_doc_mut(|doc| {
        let Ok(Some((_, bookmarks))) = doc.get(automerge::ROOT, "bookmarks") else {
            return;
        };
        let keys: Vec<String> = doc.keys(&bookmarks).collect();
        let mut needs_migration = Vec::new();
        for key in &keys {
            let Ok(Some((_, bm_obj))) = doc.get(&bookmarks, key.as_str()) else {
                continue;
            };
            if doc.get(&bm_obj, "favicon").ok().flatten().is_none() {
                needs_migration.push(key.clone());
            }
        }
        if needs_migration.is_empty() {
            return;
        }
        let mut tx = doc.transaction();
        for key in &needs_migration {
            if let Ok(Some((_, bm_obj))) = tx.get(&bookmarks, key.as_str()) {
                let _ = tx.put(&bm_obj, "favicon", "");
            }
        }
        if let Ok(Some((_, meta))) = tx.get(automerge::ROOT, "meta") {
            let version = tx
                .get(&meta, "schema_version")
                .ok()
                .flatten()
                .and_then(|(v, _)| {
                    if let Value::Scalar(s) = &v {
                        s.to_u64()
                    } else {
                        None
                    }
                })
                .unwrap_or(1);
            if version < 2 {
                let _ = tx.put(&meta, "schema_version", 2_u64);
            }
        }
        tx.commit_with(
            automerge::transaction::CommitOptions::default()
                .with_message("migrate:add_favicon_field".to_string()),
        );
    });
}

fn walk_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    walk_files_inner(dir, &mut files);
    files
}

fn walk_files_inner(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_files_inner(&path, files);
        } else if path.is_file() {
            files.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use automerge::transaction::CommitOptions;
    use automerge_repo::Repo;
    use autosurgeon::hydrate;

    use crate::model::BookmarkStore;

    #[tokio::test]
    async fn test_migration_adds_favicon_field() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let store = FsStorage::open(temp_dir.path()).unwrap();
        let repo = Repo::new(None, Box::new(store));
        let handle = repo.run();
        let doc_handle = handle.new_document();

        // Create a document without the favicon field (old schema)
        doc_handle.with_doc_mut(|doc| {
            let mut tx = doc.transaction();
            let now = chrono::Utc::now().to_rfc3339();
            let root_id = uuid::Uuid::new_v4().to_string();
            tx.put(automerge::ROOT, "root_folder_id", root_id.as_str())
                .unwrap();
            let folders = tx
                .put_object(automerge::ROOT, "folders", ObjType::Map)
                .unwrap();
            let bookmarks = tx
                .put_object(automerge::ROOT, "bookmarks", ObjType::Map)
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

            // Add a bookmark WITHOUT the favicon field
            let bm = tx.put_object(&bookmarks, "bm-1", ObjType::Map).unwrap();
            tx.put(&bm, "url", "https://example.com").unwrap();
            tx.put(&bm, "title", "Example").unwrap();
            tx.put(&bm, "notes", "").unwrap();
            tx.put(&bm, "created_at", now.as_str()).unwrap();
            tx.put(&bm, "updated_at", now.as_str()).unwrap();
            tx.put(&bm, "deleted", false).unwrap();
            // Note: no "favicon" field

            tx.commit_with(CommitOptions::default().with_message("test_init"));
        });

        // Hydration should fail before migration
        let result = doc_handle.with_doc(hydrate::<_, BookmarkStore>);
        assert!(result.is_err());

        // Run migration
        migrate_add_favicon_field(&doc_handle);

        // Hydration should succeed after migration
        let store = doc_handle.with_doc(|doc| hydrate::<_, BookmarkStore>(doc).unwrap());
        let bm = store.bookmarks.get("bm-1").unwrap();
        assert_eq!(bm.favicon, "");
        assert_eq!(store.meta.schema_version, 2);
    }
}
