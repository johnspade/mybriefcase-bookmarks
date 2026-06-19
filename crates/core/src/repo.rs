use automerge::transaction::Transactable;
use automerge::{Automerge, ObjType};
use automerge_repo::tokio::FsStorage;
use automerge_repo::{DocHandle, DocumentId, Repo, RepoHandle};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::error::CoreError;
use crate::schema::BookmarkField::Deleted;
use crate::schema::BookmarkStoreField::{Bookmarks, Folders, Meta, RootFolderId};
use crate::schema::FolderField::{Children, CreatedAt, Title, UpdatedAt};
use crate::schema::StoreMetaField::{CollectionName, SchemaVersion};

const DEFAULT_DEBOUNCE: Duration = Duration::from_secs(4);

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
) -> Result<(RepoHandle, DocHandle, DocumentId), CoreError> {
    let local_store_path = local_data_dir.join("repo_store");
    std::fs::create_dir_all(&local_store_path)?;
    let store = FsStorage::open(&local_store_path)
        .map_err(|e| CoreError::Io(std::io::Error::other(format!("{e:?}"))))?;
    let repo = Repo::new(Some(client_id.to_string()), Box::new(store));
    let repo_handle = repo.run();

    let sync_info_path = sync_root.join(".bookmarks-sync");

    if sync_info_path.exists() {
        // Existing sync folder. Try loading from local storage first.
        let raw = std::fs::read_to_string(&sync_info_path)?;
        let info: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| CoreError::DocumentCorrupted(e.to_string()))?;
        let doc_id: DocumentId = info["document_id"]
            .as_str()
            .ok_or_else(|| {
                CoreError::DocumentCorrupted("missing document_id in sync metadata".into())
            })?
            .parse()
            .map_err(|_| CoreError::DocumentCorrupted("invalid document_id format".into()))?;

        if let Some(handle) = repo_handle
            .load(doc_id.clone())
            .await
            .map_err(|e| CoreError::Io(std::io::Error::other(format!("{e:?}"))))?
        {
            // Merge from own export to recover changes that were exported but not
            // flushed to FsStorage before the process was killed.
            merge_own_export(&handle, sync_root, client_id);
            return Ok((repo_handle, handle, doc_id));
        }

        // Not in local storage: create a new doc and merge from peers.
        let handle = repo_handle.new_document();
        full_merge_pass(&handle, sync_root, client_id);
        let actual_id = handle.document_id();
        Ok((repo_handle, handle, actual_id))
    } else {
        // First client: create document with default folder structure.
        let handle = repo_handle.new_document();
        handle.with_doc_mut(|doc| {
            let mut tx = doc.transaction();
            let now = chrono::Utc::now().to_rfc3339();
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

        let doc_id = handle.document_id();
        let info = serde_json::json!({
            "version": 1,
            "engine": "automerge-repo",
            "app": "mybriefcase-bookmarks",
            "schema_version": 1,
            "document_id": doc_id.to_string()
        });
        std::fs::create_dir_all(sync_root)?;
        std::fs::write(&sync_info_path, info.to_string())?;
        Ok((repo_handle, handle, doc_id))
    }
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
) -> Result<(), CoreError> {
    let shared = sync_root.join(client_id).join("store");
    std::fs::create_dir_all(&shared)?;

    let data = doc_handle.with_doc(automerge::Automerge::save);
    let dest = shared.join("document.snapshot");
    let tmp = dest.with_extension("tmp");
    std::fs::write(&tmp, &data)?;
    std::fs::rename(&tmp, &dest)?;
    std::fs::File::open(&dest)?.set_modified(std::time::SystemTime::now())?;
    Ok(())
}

/// Rate-limited exporter that prevents writing more than once per debounce interval.
///
/// Syncthing hashes files to detect changes; writing twice during a hash causes
/// "file changed during hashing" errors and expensive retries.
#[must_use]
pub struct DebouncedExporter {
    dest: PathBuf,
    debounce: Duration,
    last_export: Mutex<Option<Instant>>,
}

impl DebouncedExporter {
    pub fn new(sync_root: &Path, client_id: &str) -> Self {
        Self {
            dest: sync_root.join(client_id).join("store"),
            debounce: DEFAULT_DEBOUNCE,
            last_export: Mutex::new(None),
        }
    }

    /// Export only if the debounce interval has elapsed since the last write.
    ///
    /// Returns `Ok(true)` if exported, `Ok(false)` if skipped.
    ///
    /// # Errors
    /// Returns `CoreError::Io` if the filesystem write fails.
    ///
    /// # Panics
    /// Panics if the internal mutex is poisoned.
    pub fn export_debounced(&self, doc_handle: &DocHandle) -> Result<bool, CoreError> {
        let mut last = self.last_export.lock().unwrap();
        if let Some(t) = *last {
            if t.elapsed() < self.debounce {
                return Ok(false);
            }
        }
        self.write(doc_handle)?;
        *last = Some(Instant::now());
        drop(last);
        Ok(true)
    }

    /// Export unconditionally, ignoring the debounce interval.
    ///
    /// Use for shutdown or forced flush (e.g., after merge).
    ///
    /// # Errors
    /// Returns `CoreError::Io` if the filesystem write fails.
    ///
    /// # Panics
    /// Panics if the internal mutex is poisoned.
    pub fn export_now(&self, doc_handle: &DocHandle) -> Result<(), CoreError> {
        self.write(doc_handle)?;
        *self.last_export.lock().unwrap() = Some(Instant::now());
        Ok(())
    }

    fn write(&self, doc_handle: &DocHandle) -> Result<(), CoreError> {
        std::fs::create_dir_all(&self.dest)?;
        let data = doc_handle.with_doc(automerge::Automerge::save);
        let dest = self.dest.join("document.snapshot");
        let tmp = dest.with_extension("tmp");
        std::fs::write(&tmp, &data)?;
        std::fs::rename(&tmp, &dest)?;
        std::fs::File::open(&dest)?.set_modified(std::time::SystemTime::now())?;
        Ok(())
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
}
