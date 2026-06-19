use automerge::transaction::Transactable;
use automerge::{Automerge, ObjType};
use automerge_repo::tokio::FsStorage;
use automerge_repo::{DocHandle, DocumentId, Repo, RepoHandle};
use std::path::Path;

use crate::error::CoreError;
use crate::schema::BookmarkField::Deleted;
use crate::schema::BookmarkStoreField::{Bookmarks, Folders, Meta, RootFolderId};
use crate::schema::FolderField::{Children, CreatedAt, Title, UpdatedAt};
use crate::schema::StoreMetaField::{CollectionName, SchemaVersion};

/// # Errors
/// Returns `CoreError::Io` if filesystem operations fail, or `CoreError::DocumentCorrupted`
/// if the sync metadata file cannot be parsed.
///
/// # Panics
/// Panics if in-memory automerge transaction operations fail during initial schema creation.
pub async fn init_repo(
    local_data_dir: &Path,
    sync_root: &Path,
    client_id: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(RepoHandle, DocHandle), CoreError> {
    let local_store_path = local_data_dir.join("repo_store");
    std::fs::create_dir_all(&local_store_path)?;
    let store = FsStorage::open(&local_store_path)
        .map_err(|e| CoreError::Io(std::io::Error::other(format!("{e:?}"))))?;
    let repo = Repo::new(Some(client_id.to_string()), Box::new(store));
    let repo_handle = repo.run();

    let local_id_path = local_data_dir.join("local_doc_id");
    let sync_info_path = sync_root.join(".bookmarks-sync");

    if let Some(doc_id) = read_local_doc_id(&local_id_path) {
        // We have a persisted local doc ID. Try loading it from FsStorage.
        if let Some(handle) = repo_handle
            .load(doc_id)
            .await
            .map_err(|e| CoreError::Io(std::io::Error::other(format!("{e:?}"))))?
        {
            merge_own_export(&handle, sync_root, client_id);
            return Ok((repo_handle, handle));
        }

        // FsStorage lost it. Rebuild from sync exports.
        let handle = repo_handle.new_document();
        merge_own_export(&handle, sync_root, client_id);
        full_merge_pass(&handle, sync_root, client_id);
        write_local_doc_id(&local_id_path, &handle.document_id())?;
        Ok((repo_handle, handle))
    } else if sync_info_path.exists() {
        // New device joining an existing sync folder. Rebuild from peers.
        let handle = repo_handle.new_document();
        merge_own_export(&handle, sync_root, client_id);
        full_merge_pass(&handle, sync_root, client_id);
        write_local_doc_id(&local_id_path, &handle.document_id())?;
        Ok((repo_handle, handle))
    } else {
        // First client: create document with default folder structure.
        let handle = repo_handle.new_document();
        handle.with_doc_mut(|doc| {
            let mut tx = doc.transaction();
            let now = now.to_rfc3339();
            let root_id = uuid::Uuid::new_v4().to_string();

            tx.put(automerge::ROOT, RootFolderId.as_ref(), root_id.as_str())
                .unwrap();
            let folders = tx
                .put_object(automerge::ROOT, Folders.as_ref(), ObjType::Map)
                .unwrap();
            tx.put_object(automerge::ROOT, Bookmarks.as_ref(), ObjType::Map)
                .unwrap();
            let meta = tx
                .put_object(automerge::ROOT, Meta.as_ref(), ObjType::Map)
                .unwrap();
            tx.put(&meta, SchemaVersion.as_ref(), 1_u64).unwrap();
            tx.put(&meta, CollectionName.as_ref(), "bookmarks").unwrap();

            let root = tx
                .put_object(&folders, root_id.as_str(), ObjType::Map)
                .unwrap();
            tx.put(&root, Title.as_ref(), "Bookmarks").unwrap();
            let root_children = tx
                .put_object(&root, Children.as_ref(), ObjType::List)
                .unwrap();
            tx.put(&root, CreatedAt.as_ref(), now.as_str()).unwrap();
            tx.put(&root, UpdatedAt.as_ref(), now.as_str()).unwrap();
            tx.put(&root, Deleted.as_ref(), false).unwrap();

            for (i, folder_title) in ["Bookmarks Bar", "Other Bookmarks"].iter().enumerate() {
                let sub_id = uuid::Uuid::new_v4().to_string();
                let sub = tx
                    .put_object(&folders, sub_id.as_str(), ObjType::Map)
                    .unwrap();
                tx.put(&sub, Title.as_ref(), *folder_title).unwrap();
                tx.put_object(&sub, Children.as_ref(), ObjType::List)
                    .unwrap();
                tx.put(&sub, CreatedAt.as_ref(), now.as_str()).unwrap();
                tx.put(&sub, UpdatedAt.as_ref(), now.as_str()).unwrap();
                tx.put(&sub, Deleted.as_ref(), false).unwrap();
                tx.insert(&root_children, i, sub_id.as_str()).unwrap();
            }
            tx.commit();
        });

        write_local_doc_id(&local_id_path, &handle.document_id())?;
        let info = serde_json::json!({
            "version": 1,
            "engine": "automerge-repo",
            "app": "mybriefcase-bookmarks",
            "schema_version": 1
        });
        std::fs::create_dir_all(sync_root)?;
        std::fs::write(&sync_info_path, info.to_string())?;
        Ok((repo_handle, handle))
    }
}

fn read_local_doc_id(path: &Path) -> Option<DocumentId> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

fn write_local_doc_id(path: &Path, doc_id: &DocumentId) -> Result<(), CoreError> {
    std::fs::write(path, doc_id.to_string())?;
    Ok(())
}

pub fn full_merge_pass(doc_handle: &DocHandle, sync_root: &Path, own_client_id: &str) -> bool {
    let heads_before = doc_handle.with_doc(automerge::Automerge::get_heads);
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
                        let _ = doc.merge(&mut peer_doc);
                    } else {
                        let _ = doc.load_incremental(&data);
                    }
                });
            }
        }
    }
    let heads_after = doc_handle.with_doc(automerge::Automerge::get_heads);
    heads_before != heads_after
}

/// # Errors
/// Returns `CoreError::Io` if the export directory cannot be created or the file cannot be written.
pub fn export_doc_to_shared(
    doc_handle: &DocHandle,
    sync_root: &Path,
    client_id: &str,
    mtime: std::time::SystemTime,
) -> Result<(), CoreError> {
    let shared = sync_root.join(client_id).join("store");
    std::fs::create_dir_all(&shared)?;

    let data = doc_handle.with_doc(automerge::Automerge::save);
    let dest = shared.join("document.snapshot");
    let tmp = dest.with_extension("tmp");
    std::fs::write(&tmp, &data)?;
    std::fs::rename(&tmp, &dest)?;
    std::fs::File::open(&dest)?.set_modified(mtime)?;
    Ok(())
}

/// Exports the document to the sync directory using an atomic write.
#[must_use]
pub struct Exporter {
    sync_root: std::path::PathBuf,
    client_id: String,
}

impl Exporter {
    pub fn new(sync_root: &Path, client_id: &str) -> Self {
        Self {
            sync_root: sync_root.to_path_buf(),
            client_id: client_id.to_string(),
        }
    }

    /// # Errors
    /// Returns `CoreError::Io` if the filesystem write fails.
    pub fn export(
        &self,
        doc_handle: &DocHandle,
        mtime: std::time::SystemTime,
    ) -> Result<(), CoreError> {
        export_doc_to_shared(doc_handle, &self.sync_root, &self.client_id, mtime)
    }
}

/// Merge from own export file to recover changes that were written to the sync
/// directory but not flushed to `FsStorage` before process termination.
fn merge_own_export(doc_handle: &DocHandle, sync_root: &Path, client_id: &str) {
    let snapshot = sync_root
        .join(client_id)
        .join("store")
        .join("document.snapshot");
    if let Ok(data) = std::fs::read(&snapshot) {
        doc_handle.with_doc_mut(|doc| {
            if let Ok(mut peer_doc) = Automerge::load(&data) {
                let _ = doc.merge(&mut peer_doc);
            }
        });
    }
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
        } else if path.is_file() && !is_temp_file(&path) {
            files.push(path);
        }
    }
}

fn is_temp_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("tmp"))
        || name.starts_with(".syncthing.")
        || name.ends_with(".syncthing")
}

#[cfg(test)]
mod tests {
    use super::*;
    use automerge::ReadDoc;
    use automerge::transaction::Transactable;
    use std::time::Duration;

    fn make_peer_snapshot(sync_root: &Path, peer_id: &str, doc: &Automerge) {
        let store = sync_root.join(peer_id).join("store");
        std::fs::create_dir_all(&store).unwrap();
        std::fs::write(store.join("document.snapshot"), doc.save()).unwrap();
    }

    fn make_doc_handle(dir: &Path) -> (automerge_repo::RepoHandle, automerge_repo::DocHandle) {
        let store = automerge_repo::tokio::FsStorage::open(dir.join("repo_store")).unwrap();
        let repo = automerge_repo::Repo::new(None, Box::new(store));
        let repo_handle = repo.run();
        let doc_handle = repo_handle.new_document();
        (repo_handle, doc_handle)
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn full_merge_pass_returns_false_when_no_new_changes() {
        let dir = tempfile::tempdir().unwrap();
        let sync_root = dir.path();

        let (_rh, doc_handle) = make_doc_handle(dir.path());

        doc_handle.with_doc_mut(|doc| {
            let mut tx = doc.transaction();
            tx.put(automerge::ROOT, "key", "value").unwrap();
            tx.commit();
        });

        let local_save = doc_handle.with_doc(|doc| doc.save());
        let peer_store = sync_root.join("peer-a").join("store");
        std::fs::create_dir_all(&peer_store).unwrap();
        std::fs::write(peer_store.join("document.snapshot"), &local_save).unwrap();

        let changed = full_merge_pass(&doc_handle, sync_root, "my-client");
        assert!(!changed, "merge of already-known data should return false");
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn full_merge_pass_returns_true_when_peer_has_new_changes() {
        let dir = tempfile::tempdir().unwrap();
        let sync_root = dir.path();

        let (_rh, doc_handle) = make_doc_handle(dir.path());

        let mut peer_doc = Automerge::new();
        let mut tx = peer_doc.transaction();
        tx.put(automerge::ROOT, "peer_key", "peer_value").unwrap();
        tx.commit();

        make_peer_snapshot(sync_root, "peer-a", &peer_doc);

        let changed = full_merge_pass(&doc_handle, sync_root, "my-client");
        assert!(changed, "merge of new peer data should return true");
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn full_merge_pass_idempotent_on_second_call() {
        let dir = tempfile::tempdir().unwrap();
        let sync_root = dir.path();

        let (_rh, doc_handle) = make_doc_handle(dir.path());

        let mut peer_doc = Automerge::new();
        let mut tx = peer_doc.transaction();
        tx.put(automerge::ROOT, "key", "value").unwrap();
        tx.commit();

        make_peer_snapshot(sync_root, "peer-a", &peer_doc);

        let first = full_merge_pass(&doc_handle, sync_root, "my-client");
        assert!(first);

        let second = full_merge_pass(&doc_handle, sync_root, "my-client");
        assert!(!second, "second merge of same data should return false");
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn full_merge_pass_skips_own_client_dir() {
        let dir = tempfile::tempdir().unwrap();
        let sync_root = dir.path();

        let (_rh, doc_handle) = make_doc_handle(dir.path());

        let mut peer_doc = Automerge::new();
        let mut tx = peer_doc.transaction();
        tx.put(automerge::ROOT, "key", "value").unwrap();
        tx.commit();

        make_peer_snapshot(sync_root, "my-client", &peer_doc);

        let changed = full_merge_pass(&doc_handle, sync_root, "my-client");
        assert!(!changed, "should skip own client directory");
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn merge_own_export_recovers_unflushed_changes() {
        let dir = tempfile::tempdir().unwrap();
        let sync_root = dir.path();

        let (_rh, doc_handle) = make_doc_handle(dir.path());

        // Simulate a change that was exported but not flushed to FsStorage:
        // write directly to own export path.
        let mut exported_doc = Automerge::new();
        let mut tx = exported_doc.transaction();
        tx.put(automerge::ROOT, "recovered_key", "recovered_value")
            .unwrap();
        tx.commit();

        make_peer_snapshot(sync_root, "my-client", &exported_doc);

        // merge_own_export should bring the change back
        merge_own_export(&doc_handle, sync_root, "my-client");

        let val: Option<String> = doc_handle.with_doc(|doc| {
            doc.get(automerge::ROOT, "recovered_key")
                .ok()
                .flatten()
                .map(|(v, _)| v.into_string().unwrap())
        });
        assert_eq!(val.as_deref(), Some("recovered_value"));
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn walk_files_skips_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("store");
        std::fs::create_dir_all(&store).unwrap();

        std::fs::write(store.join("document.snapshot"), b"good").unwrap();
        std::fs::write(store.join("document.tmp"), b"temp").unwrap();
        std::fs::write(store.join(".syncthing.document.snapshot"), b"syncthing").unwrap();

        let files = walk_files(&store);
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("document.snapshot"));
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn export_doc_to_shared_sets_mtime() {
        let dir = tempfile::tempdir().unwrap();
        let sync_root = dir.path();
        let (_rh, doc_handle) = make_doc_handle(dir.path());

        doc_handle.with_doc_mut(|doc| {
            let mut tx = doc.transaction();
            tx.put(automerge::ROOT, "key", "value").unwrap();
            tx.commit();
        });

        let mtime = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        export_doc_to_shared(&doc_handle, sync_root, "client-a", mtime).unwrap();

        let snapshot = sync_root
            .join("client-a")
            .join("store")
            .join("document.snapshot");
        let actual_mtime = std::fs::metadata(&snapshot).unwrap().modified().unwrap();
        assert_eq!(actual_mtime, mtime);
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn init_repo_persists_local_doc_id() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("data");
        let sync_root = dir.path().join("sync");
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::create_dir_all(&sync_root).unwrap();

        let now = chrono::Utc::now();
        let (_rh, doc_handle) = init_repo(&data_dir, &sync_root, "client-a", now)
            .await
            .unwrap();

        let local_id_path = data_dir.join("local_doc_id");
        assert!(local_id_path.exists());
        let stored: DocumentId = std::fs::read_to_string(&local_id_path)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(stored, doc_handle.document_id());
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn init_repo_fsstorage_lost_doc_recovers_from_export() {
        // Simulates the Android swipe-kill scenario:
        // local_doc_id exists but FsStorage doesn't have the doc.
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("data");
        let sync_root = dir.path().join("sync");
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::create_dir_all(&sync_root).unwrap();

        let client_id = "my-phone";
        let now = chrono::Utc::now();

        // First init creates the doc.
        let (rh, doc_handle) = init_repo(&data_dir, &sync_root, client_id, now)
            .await
            .unwrap();
        let doc_id = doc_handle.document_id();

        // Add data and export to sync dir.
        doc_handle.with_doc_mut(|doc| {
            let mut tx = doc.transaction();
            tx.put(automerge::ROOT, "local_change", "important_value")
                .unwrap();
            tx.commit();
        });
        export_doc_to_shared(
            &doc_handle,
            &sync_root,
            client_id,
            std::time::SystemTime::now(),
        )
        .unwrap();
        drop(doc_handle);
        drop(rh);

        // Simulate FsStorage loss: use a fresh data_dir but carry over local_doc_id.
        let fresh_data_dir = dir.path().join("data2");
        std::fs::create_dir_all(&fresh_data_dir).unwrap();
        std::fs::write(fresh_data_dir.join("local_doc_id"), doc_id.to_string()).unwrap();

        // Re-init: should recover the local change from own export.
        let (_rh2, doc_handle2) = init_repo(&fresh_data_dir, &sync_root, client_id, now)
            .await
            .unwrap();

        let val: Option<String> = doc_handle2.with_doc(|doc| {
            doc.get(automerge::ROOT, "local_change")
                .ok()
                .flatten()
                .map(|(v, _)| v.into_string().unwrap())
        });
        assert_eq!(
            val.as_deref(),
            Some("important_value"),
            "own export should be recovered when FsStorage is empty"
        );

        // local_doc_id should be updated to the new actual ID.
        assert!(fresh_data_dir.join("local_doc_id").exists());
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn init_repo_new_device_joins_existing_sync() {
        // A new device (no local_doc_id) joining a sync folder with peer exports.
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("data");
        let sync_root = dir.path().join("sync");
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::create_dir_all(&sync_root).unwrap();

        // Create the sync metadata (as if another device created it).
        let sync_info = serde_json::json!({
            "version": 1,
            "engine": "automerge-repo",
            "app": "mybriefcase-bookmarks",
            "schema_version": 1
        });
        std::fs::write(sync_root.join(".bookmarks-sync"), sync_info.to_string()).unwrap();

        // Create a peer export with data.
        let mut peer_doc = Automerge::new();
        let mut tx = peer_doc.transaction();
        tx.put(automerge::ROOT, "peer_key", "peer_value").unwrap();
        tx.commit();
        make_peer_snapshot(&sync_root, "desktop", &peer_doc);

        // New device init (no local_doc_id file).
        let now = chrono::Utc::now();
        let (_rh, doc_handle) = init_repo(&data_dir, &sync_root, "my-phone", now)
            .await
            .unwrap();

        // Should have merged peer data.
        let val: Option<String> = doc_handle.with_doc(|doc| {
            doc.get(automerge::ROOT, "peer_key")
                .ok()
                .flatten()
                .map(|(v, _)| v.into_string().unwrap())
        });
        assert_eq!(val.as_deref(), Some("peer_value"));

        // local_doc_id should be written.
        assert!(data_dir.join("local_doc_id").exists());
    }
}
