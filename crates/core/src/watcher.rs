use notify::{Event, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tokio::sync::mpsc;

/// # Panics
/// Panics if the file watcher cannot be created or the sync root cannot be watched.
#[must_use]
pub fn start_file_watcher(
    sync_root: &Path,
    own_client_id: &str,
) -> mpsc::UnboundedReceiver<Vec<String>> {
    let (tx, rx) = mpsc::unbounded_channel();
    let sync_root = sync_root
        .canonicalize()
        .unwrap_or_else(|_| sync_root.to_path_buf());
    let own_id = own_client_id.to_string();

    std::thread::spawn(move || {
        let (ntx, nrx) = std::sync::mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |res: Result<Event, _>| {
            if let Ok(event) = res {
                let _ = ntx.send(event);
            }
        })
        .expect("create watcher");
        watcher
            .watch(&sync_root, RecursiveMode::Recursive)
            .expect("watch sync_root");

        loop {
            let mut changed_peers = HashSet::new();
            match nrx.recv() {
                Ok(event) => {
                    extract_peer_ids(&event, &sync_root, &own_id, &mut changed_peers);
                }
                Err(_) => break,
            }
            // Debounce: drain events for 500ms.
            let deadline = std::time::Instant::now() + Duration::from_millis(500);
            while let Ok(event) =
                nrx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            {
                extract_peer_ids(&event, &sync_root, &own_id, &mut changed_peers);
            }
            if !changed_peers.is_empty() {
                let peers: Vec<String> = changed_peers.into_iter().collect();
                if tx.send(peers).is_err() {
                    break;
                }
            }
        }
    });
    rx
}

pub fn extract_peer_ids<S: std::hash::BuildHasher>(
    event: &Event,
    sync_root: &Path,
    own_id: &str,
    peers: &mut HashSet<String, S>,
) {
    for path in &event.paths {
        if let Ok(rel) = path.strip_prefix(sync_root) {
            if let Some(first) = rel.components().next() {
                let peer = first.as_os_str().to_string_lossy().to_string();
                if peer != own_id && !peer.starts_with('.') && peer != "favicons" {
                    peers.insert(peer);
                }
            }
        }
    }
}

pub fn merge_specific_peers(
    doc_handle: &automerge_repo::DocHandle,
    sync_root: &Path,
    peers: &[String],
) -> bool {
    let heads_before = doc_handle.with_doc(automerge::Automerge::get_heads);
    for peer_id in peers {
        let store_dir = sync_root.join(peer_id).join("store");
        if !store_dir.is_dir() {
            continue;
        }
        let files = walk_files(&store_dir);
        for file_path in &files {
            if let Ok(data) = std::fs::read(file_path) {
                doc_handle.with_doc_mut(|doc| {
                    if let Ok(mut peer_doc) = automerge::Automerge::load(&data) {
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

/// Tracks file modification times to efficiently detect peer changes via polling.
/// Only files whose mtime has changed since the last poll trigger a merge.
pub struct PollState {
    mtimes: HashMap<PathBuf, SystemTime>,
}

impl PollState {
    /// Seed with current mtimes so the first poll only detects new changes.
    #[must_use]
    pub fn new(sync_root: &Path, own_client_id: &str) -> Self {
        let mut state = Self {
            mtimes: HashMap::new(),
        };
        let _ = state.scan(sync_root, own_client_id);
        state
    }

    /// Returns peer IDs whose files have changed since the last call.
    pub fn poll_changed_peers(&mut self, sync_root: &Path, own_client_id: &str) -> Vec<String> {
        self.scan(sync_root, own_client_id)
    }

    fn scan(&mut self, sync_root: &Path, own_client_id: &str) -> Vec<String> {
        let mut changed_peers = HashSet::new();
        let mut seen = HashSet::new();

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

            for file_path in walk_files(&store_dir) {
                seen.insert(file_path.clone());
                let mtime = std::fs::metadata(&file_path)
                    .and_then(|m| m.modified())
                    .ok();

                let is_changed = match (self.mtimes.get(&file_path), mtime) {
                    (None, Some(mt)) => {
                        self.mtimes.insert(file_path, mt);
                        true
                    }
                    (Some(old), Some(new)) if *old != new => {
                        self.mtimes.insert(file_path, new);
                        true
                    }
                    _ => false,
                };

                if is_changed {
                    changed_peers.insert(peer_id.clone());
                }
            }
        }

        self.mtimes.retain(|path, _| seen.contains(path));
        changed_peers.into_iter().collect()
    }
}

fn walk_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk_files_inner(dir, &mut files);
    files
}

fn walk_files_inner(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_files_inner(&path, files);
        } else if path.is_file() && path.extension().is_none_or(|e| e != "tmp") {
            files.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use automerge::transaction::Transactable;
    use std::path::Path;

    fn make_peer_store(sync_root: &Path, peer: &str) -> PathBuf {
        let store = sync_root.join(peer).join("store");
        std::fs::create_dir_all(&store).unwrap();
        store
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn poll_seed_no_false_positives() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let store = make_peer_store(root, "peer-a");
        std::fs::write(store.join("doc.snapshot"), b"data").unwrap();

        let mut poll = PollState::new(root, "me");
        let changed = poll.poll_changed_peers(root, "me");
        assert!(
            changed.is_empty(),
            "first poll after seed should find nothing"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn poll_detects_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let store = make_peer_store(root, "peer-a");

        let mut poll = PollState::new(root, "me");

        std::fs::write(store.join("doc.snapshot"), b"new-data").unwrap();
        let changed = poll.poll_changed_peers(root, "me");
        assert_eq!(changed, vec!["peer-a"]);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn poll_detects_modified_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let store = make_peer_store(root, "peer-a");
        let file = store.join("doc.snapshot");
        std::fs::write(&file, b"v1").unwrap();

        let mut poll = PollState::new(root, "me");

        // Bump mtime by at least 1s for filesystems with coarse granularity.
        std::thread::sleep(Duration::from_millis(1100));
        std::fs::write(&file, b"v2").unwrap();

        let changed = poll.poll_changed_peers(root, "me");
        assert_eq!(changed, vec!["peer-a"]);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn poll_no_change_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let store = make_peer_store(root, "peer-a");
        std::fs::write(store.join("doc.snapshot"), b"stable").unwrap();

        let mut poll = PollState::new(root, "me");
        let first = poll.poll_changed_peers(root, "me");
        let second = poll.poll_changed_peers(root, "me");
        assert!(first.is_empty());
        assert!(second.is_empty());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn poll_ignores_own_client() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let mut poll = PollState::new(root, "me");

        let store = make_peer_store(root, "me");
        std::fs::write(store.join("doc.snapshot"), b"own").unwrap();

        let changed = poll.poll_changed_peers(root, "me");
        assert!(changed.is_empty());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn poll_ignores_dotdirs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let mut poll = PollState::new(root, "me");

        let store = make_peer_store(root, ".bookmarks-sync");
        std::fs::write(store.join("meta.json"), b"{}").unwrap();

        let changed = poll.poll_changed_peers(root, "me");
        assert!(changed.is_empty());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn poll_ignores_tmp_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let mut poll = PollState::new(root, "me");

        let store = make_peer_store(root, "peer-a");
        std::fs::write(store.join("doc.tmp"), b"in-progress").unwrap();

        let changed = poll.poll_changed_peers(root, "me");
        assert!(changed.is_empty());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn poll_skips_peer_without_store_dir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("peer-a")).unwrap();
        std::fs::write(root.join("peer-a").join("info.json"), b"{}").unwrap();

        let mut poll = PollState::new(root, "me");
        let changed = poll.poll_changed_peers(root, "me");
        assert!(changed.is_empty());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn poll_detects_multiple_peers() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let mut poll = PollState::new(root, "me");

        let store_a = make_peer_store(root, "peer-a");
        let store_b = make_peer_store(root, "peer-b");
        std::fs::write(store_a.join("doc.snapshot"), b"a").unwrap();
        std::fs::write(store_b.join("doc.snapshot"), b"b").unwrap();

        let mut changed = poll.poll_changed_peers(root, "me");
        changed.sort();
        assert_eq!(changed, vec!["peer-a", "peer-b"]);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn poll_cleans_up_deleted_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let store = make_peer_store(root, "peer-a");
        let file = store.join("doc.snapshot");
        std::fs::write(&file, b"data").unwrap();

        let mut poll = PollState::new(root, "me");
        assert_eq!(poll.mtimes.len(), 1);

        std::fs::remove_file(&file).unwrap();
        let _ = poll.poll_changed_peers(root, "me");
        assert!(
            poll.mtimes.is_empty(),
            "deleted file should be removed from mtime map"
        );
    }

    fn make_event(paths: &[&str]) -> Event {
        Event {
            kind: notify::EventKind::Modify(notify::event::ModifyKind::Data(
                notify::event::DataChange::Content,
            )),
            paths: paths.iter().map(PathBuf::from).collect(),
            attrs: notify::event::EventAttributes::default(),
        }
    }

    #[test]
    fn test_extract_peer_ids_basic() {
        let sync_root = Path::new("/data/sync");
        let event = make_event(&["/data/sync/peer-a/store/document.snapshot"]);
        let mut peers = HashSet::new();
        extract_peer_ids(&event, sync_root, "my-client", &mut peers);
        assert_eq!(peers.len(), 1);
        assert!(peers.contains("peer-a"));
    }

    #[test]
    fn test_extract_peer_ids_ignores_own_client() {
        let sync_root = Path::new("/data/sync");
        let event = make_event(&["/data/sync/my-client/store/document.snapshot"]);
        let mut peers = HashSet::new();
        extract_peer_ids(&event, sync_root, "my-client", &mut peers);
        assert!(peers.is_empty());
    }

    #[test]
    fn test_extract_peer_ids_ignores_dotdirs() {
        let sync_root = Path::new("/data/sync");
        let event = make_event(&["/data/sync/.bookmarks-sync"]);
        let mut peers = HashSet::new();
        extract_peer_ids(&event, sync_root, "my-client", &mut peers);
        assert!(peers.is_empty());
    }

    #[test]
    fn test_extract_peer_ids_path_outside_sync_root() {
        let sync_root = Path::new("/data/sync");
        let event = make_event(&["/other/path/file.txt"]);
        let mut peers = HashSet::new();
        extract_peer_ids(&event, sync_root, "my-client", &mut peers);
        assert!(peers.is_empty());
    }

    #[test]
    fn test_extract_peer_ids_multiple_peers() {
        let sync_root = Path::new("/data/sync");
        let event = make_event(&[
            "/data/sync/peer-a/store/doc.snapshot",
            "/data/sync/peer-b/store/doc.snapshot",
            "/data/sync/peer-a/store/other.bin",
        ]);
        let mut peers = HashSet::new();
        extract_peer_ids(&event, sync_root, "my-client", &mut peers);
        assert_eq!(peers.len(), 2);
        assert!(peers.contains("peer-a"));
        assert!(peers.contains("peer-b"));
    }

    #[test]
    fn test_extract_peer_ids_ignores_favicons_dir() {
        let sync_root = Path::new("/data/sync");
        let event = make_event(&["/data/sync/favicons/abc123.png"]);
        let mut peers = HashSet::new();
        extract_peer_ids(&event, sync_root, "my-client", &mut peers);
        assert!(peers.is_empty());
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
    async fn merge_specific_peers_returns_false_when_no_new_changes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let (_rh, doc_handle) = make_doc_handle(dir.path());

        doc_handle.with_doc_mut(|doc| {
            let mut tx = doc.transaction();
            tx.put(automerge::ROOT, "key", "value").unwrap();
            tx.commit();
        });

        let store = make_peer_store(root, "peer-a");
        let data = doc_handle.with_doc(|doc| doc.save());
        std::fs::write(store.join("document.snapshot"), &data).unwrap();

        let changed = merge_specific_peers(&doc_handle, root, &["peer-a".to_string()]);
        assert!(!changed, "merge of already-known data should return false");
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn merge_specific_peers_returns_true_when_peer_has_new_changes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let (_rh, doc_handle) = make_doc_handle(dir.path());

        let mut peer_doc = automerge::Automerge::new();
        let mut tx = peer_doc.transaction();
        tx.put(automerge::ROOT, "peer_key", "peer_value").unwrap();
        tx.commit();

        let store = make_peer_store(root, "peer-a");
        std::fs::write(store.join("document.snapshot"), peer_doc.save()).unwrap();

        let changed = merge_specific_peers(&doc_handle, root, &["peer-a".to_string()]);
        assert!(changed, "merge of new peer data should return true");
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn merge_specific_peers_idempotent_on_second_call() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let (_rh, doc_handle) = make_doc_handle(dir.path());

        let mut peer_doc = automerge::Automerge::new();
        let mut tx = peer_doc.transaction();
        tx.put(automerge::ROOT, "key", "val").unwrap();
        tx.commit();

        let store = make_peer_store(root, "peer-a");
        std::fs::write(store.join("document.snapshot"), peer_doc.save()).unwrap();

        let first = merge_specific_peers(&doc_handle, root, &["peer-a".to_string()]);
        assert!(first);

        let second = merge_specific_peers(&doc_handle, root, &["peer-a".to_string()]);
        assert!(!second, "second merge of same data should return false");
    }
}
