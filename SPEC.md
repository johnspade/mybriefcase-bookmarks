# SPEC: Local-First Bookmark Manager with Automerge over Syncthing

**Version:** 0.2.0-draft
**Status:** Implementation-ready specification
**Architecture:** Rust web-server (Axum) + HTMX frontend, automerge-repo-rs, Syncthing file transport

---

## 1. Overview

This system is a local-first bookmark manager where each device runs its own
Rust web-server. Data is stored as Automerge CRDT documents managed by
`automerge-repo-rs`, persisted to the local filesystem via its built-in
`fs_store`. Syncthing synchronises the underlying files between devices.

The design is inspired by [DecSync](https://github.com/39aldo39/DecSync),
preserving its key architectural insight — **each client writes exclusively to
its own files** — while replacing DecSync's JSON-lines / last-writer-wins
mechanism with Automerge's richer CRDT merge.

The data model mirrors a typical browser bookmark manager: bookmarks live
inside a tree of **folders**, and the entire tree can be exported to the
Netscape Bookmark HTML format consumed by all major browsers.

### 1.1 Design Principles (inherited from DecSync)

| Principle | DecSync | This system |
|---|---|---|
| No custom server | Syncthing/Dropbox syncs plain files | Same — Syncthing syncs Automerge files |
| Per-client write isolation | Each app writes only to `v2/<appId>/` | Each client writes only to `<clientId>/` |
| Offline-first | All reads/writes are local | Same |
| Idempotent updates | LWW on `(path, key)` is idempotent | Automerge merge is idempotent by construction |
| New client catches up from disk | `initStoredEntries()` replays history | `Repo` loads + merges all chunks from `fs_store` |

### 1.2 Key Differences from DecSync

DecSync stores flat `(path, key, value)` triples as JSON-lines and resolves
conflicts by comparing ISO 8601 wall-clock timestamps (last-writer-wins).
This has three limitations that Automerge eliminates:

1. **Clock dependency.** DecSync requires roughly synchronised wall clocks.
   Automerge uses Lamport-like `(counter, actorId)` operation IDs — no clock
   sync needed.
2. **Flat data only.** DecSync values are opaque JSON scalars; the entire value
   is replaced atomically. Automerge supports nested maps, lists, and text with
   fine-grained concurrent merge — essential for our folder tree.
3. **Silent conflict discard.** DecSync silently drops the "losing" value.
   Automerge preserves all concurrent values, accessible via `get_all()`, and
   picks a deterministic winner.

### 1.3 Why automerge-repo-rs

The `automerge_repo` crate (v0.3, by Alex Good) provides:

- **`Repo`** — a background event loop that manages documents, persistence, and
  sync. It automatically saves changes and handles compaction.
- **`RepoHandle`** — the client-facing API: `new_document()`,
  `request_document()`, `new_remote_repo()`.
- **`DocHandle`** — a handle to a single Automerge document; wraps
  `automerge::Automerge` and dispatches changes to the `Repo` for storage.
- **`fs_store`** — a built-in filesystem storage backend that uses compound
  keys `[<document_id>, <chunk_type>, <chunk_identifier>]` and
  content-addressed filenames. It supports concurrent, lockless access and
  safe compaction — precisely the properties needed when Syncthing is the
  transport layer.
- **`Storage` trait** — a pluggable interface we can wrap to add the
  per-client write isolation layer.

The storage model is designed so that compaction is safe without external locks:
each chunk file is named by the hash of its content, so two processes compacting
the same data produce identical filenames. A compacting process only deletes
the incremental chunks it previously loaded, never data written by another
process since compaction began.

> **Note:** The `fs_store` compaction bug (issue #52) was fixed in PR #58
> (merged Dec 2023) and is included in all crates.io releases since v0.1.0.
> We depend on `automerge_repo` v0.3.0 from crates.io (published Oct 2025),
> which uses `automerge` 0.7 and `autosurgeon` 0.9. No git dependency or
> manual compaction workaround is needed.

---

## 2. Data Model

### 2.1 Folder and Bookmark Schema

A single Automerge document holds all bookmark state. The document models a
tree of folders, each containing an ordered list of **children** that are
either bookmarks or sub-folders. This mirrors how browsers (Chrome, Firefox,
Safari) organise bookmarks internally, and maps directly to the Netscape
Bookmark HTML format for import/export.

```
ROOT (Map)
├── "root_folder_id" : String                  # UUID of the root folder
├── "folders" (Map)
│   ├── "<folder_id>" (Map)                    # UUID string key
│   │   ├── "title"        : String
│   │   ├── "children"     : List<String>      # ordered list of child IDs
│   │   │                                      # (may be bookmark or folder IDs)
│   │   ├── "created_at"   : String            # ISO 8601
│   │   ├── "updated_at"   : String            # ISO 8601
│   │   └── "deleted"      : Boolean           # soft-delete flag
│   └── ...
├── "bookmarks" (Map)
│   ├── "<bookmark_id>" (Map)                  # UUID string key
│   │   ├── "url"          : String
│   │   ├── "title"        : String
│   │   ├── "notes"        : String            # free-text notes
│   │   ├── "created_at"   : String            # ISO 8601, seconds since epoch
│   │   ├── "updated_at"   : String            # ISO 8601
│   │   └── "deleted"      : Boolean           # soft-delete flag
│   └── ...
└── "meta" (Map)
    ├── "schema_version"    : u64
    └── "collection_name"   : String
```

**Key design decisions:**

- **Flat maps + ordered children list.** Folders and bookmarks are stored in
  flat `HashMap`-style maps keyed by UUID, with parent-child relationships
  expressed by the `children` list inside each folder. This avoids deeply
  nested Automerge objects (which are harder to address and merge) while still
  representing an arbitrary tree.
- **Children list uses Automerge List.** The `children` field is an Automerge
  `List<String>` of IDs. Automerge lists support concurrent insert/delete with
  positional merge — if device A adds a bookmark at position 0 and device B
  adds one at position 2, both appear in the merged result at their intended
  positions.
- **IDs are UUIDs.** Every folder and bookmark gets a UUID v4 at creation time.
  The `root_folder_id` is set once when the document is first created.
- **`created_at` stores an ISO 8601 string.** For Netscape HTML export, this
  is parsed to Unix seconds for the `ADD_DATE` attribute.

### 2.2 Default Folder Structure

On first initialisation, the document is seeded with a root folder and two
conventional sub-folders matching browser conventions:

```
Bookmarks (root)
├── Bookmarks Bar
└── Other Bookmarks
```

### 2.3 Rust Types (with autosurgeon)

```rust
use autosurgeon::{Hydrate, Reconcile};
use std::collections::HashMap;

#[derive(Debug, Clone, Reconcile, Hydrate)]
pub struct BookmarkStore {
    pub root_folder_id: String,
    pub folders: HashMap<String, Folder>,
    pub bookmarks: HashMap<String, Bookmark>,
    pub meta: StoreMeta,
}

#[derive(Debug, Clone, Reconcile, Hydrate)]
pub struct Folder {
    pub title: String,
    pub children: Vec<String>,   // ordered IDs of child bookmarks/folders
    pub created_at: String,
    pub updated_at: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, Reconcile, Hydrate)]
pub struct Bookmark {
    pub url: String,
    pub title: String,
    pub notes: String,
    pub created_at: String,
    pub updated_at: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, Reconcile, Hydrate)]
pub struct StoreMeta {
    pub schema_version: u64,
    pub collection_name: String,
}
```

### 2.4 Determining Item Type from an ID

A child ID in a `children` list may refer to either a bookmark or a folder.
To resolve the type, check membership:

```rust
impl BookmarkStore {
    pub fn resolve_child(&self, id: &str) -> Option<ChildRef<'_>> {
        if let Some(bm) = self.bookmarks.get(id) {
            if !bm.deleted { return Some(ChildRef::Bookmark(bm)); }
        }
        if let Some(f) = self.folders.get(id) {
            if !f.deleted { return Some(ChildRef::Folder(f)); }
        }
        None
    }
}

pub enum ChildRef<'a> {
    Bookmark(&'a Bookmark),
    Folder(&'a Folder),
}
```

### 2.5 Operations and Their Automerge Equivalents

| User action | DecSync equivalent | Automerge operation |
|---|---|---|
| Create folder | `setEntry(["folders","names"], id, "Work")` | `doc.put_object(folders, id, Map)` + fields; `doc.insert(parent.children, idx, id)` |
| Add bookmark to folder | `setEntry(["bookmarks","urls"], id, url)` | `doc.put_object(bookmarks, id, Map)` + fields; `doc.insert(folder.children, idx, id)` |
| Move bookmark to another folder | Two `setEntry` calls (remove + add) | `doc.delete(old.children, old_idx)`; `doc.insert(new.children, new_idx, id)` |
| Reorder within folder | Not supported by DecSync | `doc.delete(children, old_idx)`; `doc.insert(children, new_idx, id)` |
| Edit title | `setEntry(["bookmarks","titles"], id, "new")` | `doc.put(&bookmark_obj, "title", "new title")` |
| Delete bookmark | `setEntry(["bookmarks","urls"], id, null)` | `doc.put(&bm, "deleted", true)`; `doc.delete(parent.children, idx)` |
| Delete folder | Multiple `setEntry` calls | Mark folder + descendants `deleted=true`; remove from parent children |

---

## 3. Directory Layout and Storage

### 3.1 Syncthing Shared Folder Structure

```
<sync_root>/                              # Syncthing shared folder
├── .bookmarks-sync                       # metadata file (see §3.2)
├── <client_A>/                           # written ONLY by client A
│   ├── info.json                         # client identity (see §4)
│   └── store/                            # fs_store root for this client
│       ├── <doc_id>.snapshot.<heads>     # compacted document
│       ├── <doc_id>.incremental.<hash>   # individual change chunk
│       ├── <doc_id>.incremental.<hash>
│       └── ...
├── <client_B>/                           # written ONLY by client B
│   ├── info.json
│   └── store/
│       └── ...
└── <client_C>/
    └── ...
```

The `store/` subdirectory under each client is an `fs_store`-compatible
directory. Filenames follow the `automerge-repo-rs` convention:
`<document_id>.<chunk_type>.<chunk_identifier>`, where chunk type is
`snapshot` or `incremental` and the identifier is a content hash.

### 3.2 The `.bookmarks-sync` File

```json
{
  "version": 1,
  "engine": "automerge-repo",
  "app": "bookmark-manager",
  "schema_version": 1,
  "document_id": "f9b1a2c6-ca05-4fc7-a02f-7e3d83f1bb49"
}
```

Created by whichever client initialises the sync folder. Records the shared
`DocumentId` so all clients operate on the same Automerge document.

### 3.3 Local State (Not Synced)

```
<local_data_dir>/                         # e.g. ~/.local/share/bookmarks/
├── actor_id                              # persistent Automerge ActorId (binary)
├── repo_store/                           # fs_store for the local Repo
│   └── ...                               # Repo writes its merged doc here
└── merge_state.json                      # tracks which peer chunks have been merged
```

The client ID is no longer persisted — it is derived from the hostname
(or env vars) on each launch. See §4.1.

### 3.4 Storage Architecture

The system uses two `FsStore` instances:

1. **Local store** (`repo_store/`): The `Repo`'s primary store. All reads
   and writes from the `Repo` go here. This is the working copy.
2. **Shared store** (`<sync_root>/<own_client_id>/store/`): A mirror of the
   local store, exported after each change so Syncthing can propagate it.

Peer data flows in the opposite direction: the file watcher detects new
files in other clients' `store/` directories, loads them, and merges them
into the local `Repo` document.

---

## 4. Client Identity

### 4.1 Client ID Resolution

The client ID is derived fresh on every launch — no persistence file.

| Mode | `BOOKMARK_DEV_MODE` | `BOOKMARK_CLIENT_ID` | Result |
|---|---|---|---|
| Normal | unset | unset | bare hostname |
| Normal | unset | set | `BOOKMARK_CLIENT_ID` value |
| Dev | set | unset | hostname + `-` + random 8-char suffix (ephemeral) |
| Dev | set | set | `BOOKMARK_CLIENT_ID` value |

```rust
/// Bare hostname, for normal mode.
pub fn hostname_client_id() -> String {
    hostname::get()
        .ok()
        .and_then(|h| {
            let s = h.to_string_lossy().to_string();
            if s.is_empty() { None } else { Some(s) }
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// Hostname + random 8-char suffix, for dev mode without explicit ID.
pub fn dev_client_id() -> String {
    let host = hostname_client_id();
    let short = &uuid::Uuid::new_v4().to_string()[..8];
    format!("{host}-{short}")
}
```

Resolution in `main`:

```rust
let client_id = if cfg.dev_mode {
    cfg.client_id.unwrap_or_else(|| identity::dev_client_id())
} else {
    cfg.client_id.unwrap_or_else(|| identity::hostname_client_id())
};
```

### 4.2 Actor ID Persistence

```rust
use automerge::ActorId;

pub fn get_or_create_actor_id(local_data_dir: &Path) -> ActorId {
    let path = local_data_dir.join("actor_id");
    if let Ok(bytes) = std::fs::read(&path) {
        return ActorId::from(bytes.as_slice());
    }
    let actor = ActorId::random();
    std::fs::write(&path, actor.to_bytes()).expect("write actor_id");
    actor
}
```

### 4.3 The `info.json` File

Written to `<sync_root>/<client_id>/info.json`:

```json
{
  "client_id": "mars-laptop",
  "created_at": "2025-03-01T10:00:00Z",
  "app_version": "0.2.0",
  "automerge_actor_id": "a1b2c3d4e5f6..."
}
```

---

## 5. Core Algorithm

### 5.1 Lifecycle Overview

```
  App starts
      │
      ▼
  Create Repo with local FsStore (§5.2)
      │
      ▼
  Load or create document via RepoHandle (§5.2)
      │
      ▼
  Full merge pass: ingest all peer chunks (§5.3)
      │
      ▼
  Start file watcher (§5.4) + Start web server
      │
      ├── User writes via DocHandle (§5.5)
      │       └── export_own_chunks (§5.6)
      │
      ├── File change detected → selective merge (§5.3)
      │       └── notify UI via SSE
      │
      └── Repo auto-compacts via fs_store (§5.7)
              └── export_own_chunks mirrors compaction
```

### 5.2 Startup: Create Repo and Load/Create Document

```rust
use automerge_repo::{Repo, RepoHandle, DocHandle, DocumentId};
use automerge_repo::fs_store::FsStore;
use automerge::{ObjType, ReadDoc, ActorId};
use automerge::transaction::Transactable;
use std::path::Path;

pub fn init_repo(
    local_data_dir: &Path,
    sync_root: &Path,
    client_id: &str,
    actor_id: ActorId,
) -> (RepoHandle, DocHandle, DocumentId) {
    let local_store_path = local_data_dir.join("repo_store");
    std::fs::create_dir_all(&local_store_path).unwrap();
    let store = FsStore::open(&local_store_path).unwrap();
    let repo = Repo::new(Some(client_id.to_string()), Box::new(store));
    let repo_handle = repo.run();

    let sync_info_path = sync_root.join(".bookmarks-sync");
    let (doc_handle, document_id) = if sync_info_path.exists() {
        // Existing collection: load the shared document ID.
        let info: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&sync_info_path).unwrap()
        ).unwrap();
        let doc_id: DocumentId = info["document_id"].as_str().unwrap()
            .parse().unwrap();
        let handle = repo_handle.request_document(doc_id.clone()).unwrap();
        (handle, doc_id)
    } else {
        // First client: create document with root folder structure.
        let handle = repo_handle.new_document();
        handle.with_doc_mut(|doc| {
            doc.set_actor(actor_id);
            let now = chrono::Utc::now().to_rfc3339();
            let root_id = uuid::Uuid::new_v4().to_string();

            doc.put(automerge::ROOT, "root_folder_id", &root_id).unwrap();
            let folders = doc.put_object(automerge::ROOT, "folders", ObjType::Map).unwrap();
            doc.put_object(automerge::ROOT, "bookmarks", ObjType::Map).unwrap();
            let meta = doc.put_object(automerge::ROOT, "meta", ObjType::Map).unwrap();
            doc.put(&meta, "schema_version", 1_u64).unwrap();
            doc.put(&meta, "collection_name", "bookmarks").unwrap();

            // Root folder.
            let root = doc.put_object(&folders, &root_id, ObjType::Map).unwrap();
            doc.put(&root, "title", "Bookmarks").unwrap();
            let root_children = doc.put_object(&root, "children", ObjType::List).unwrap();
            doc.put(&root, "created_at", &now).unwrap();
            doc.put(&root, "updated_at", &now).unwrap();
            doc.put(&root, "deleted", false).unwrap();

            // Default sub-folders.
            for (i, title) in ["Bookmarks Bar", "Other Bookmarks"].iter().enumerate() {
                let sub_id = uuid::Uuid::new_v4().to_string();
                let sub = doc.put_object(&folders, &sub_id, ObjType::Map).unwrap();
                doc.put(&sub, "title", *title).unwrap();
                doc.put_object(&sub, "children", ObjType::List).unwrap();
                doc.put(&sub, "created_at", &now).unwrap();
                doc.put(&sub, "updated_at", &now).unwrap();
                doc.put(&sub, "deleted", false).unwrap();
                doc.insert(&root_children, i, &sub_id).unwrap();
            }
        });

        let doc_id = handle.document_id();
        let info = serde_json::json!({
            "version": 1, "engine": "automerge-repo",
            "app": "bookmark-manager", "schema_version": 1,
            "document_id": doc_id.to_string()
        });
        std::fs::write(&sync_info_path, info.to_string()).unwrap();
        (handle, doc_id)
    };

    (repo_handle, doc_handle, document_id)
}
```

### 5.3 Full Merge Pass (New Client Bootstrap)

Scans every peer's `store/` directory and merges all chunk files into the
local document. Because Automerge deduplicates by change hash, re-merging
already-incorporated changes is a harmless no-op.

```rust
use automerge::AutoCommit;

pub fn full_merge_pass(
    doc_handle: &DocHandle,
    sync_root: &Path,
    own_client_id: &str,
) -> bool {
    let mut changed = false;
    for entry in std::fs::read_dir(sync_root).into_iter().flatten().flatten() {
        let peer_dir = entry.path();
        if !peer_dir.is_dir() { continue; }
        let peer_id = peer_dir.file_name().unwrap().to_string_lossy().to_string();
        if peer_id == own_client_id || peer_id.starts_with('.') { continue; }

        let store_dir = peer_dir.join("store");
        if !store_dir.is_dir() { continue; }

        for f in std::fs::read_dir(&store_dir).into_iter().flatten().flatten() {
            if !f.path().is_file() { continue; }
            if let Ok(data) = std::fs::read(f.path()) {
                doc_handle.with_doc_mut(|doc| {
                    if let Ok(mut peer_doc) = AutoCommit::load(&data) {
                        if doc.merge(&mut peer_doc).is_ok() { changed = true; }
                    } else if doc.load_incremental(&data).is_ok() {
                        changed = true;
                    }
                });
            }
        }
    }
    changed
}
```

### 5.4 File Watching (Ongoing Sync)

Uses the `notify` crate to watch `<sync_root>/` recursively. Events are
debounced at 500ms. Only changes in **peer** directories (not own) trigger
a selective merge pass for those peers.

```rust
use notify::{Watcher, RecursiveMode, Event};
use std::sync::mpsc;
use std::time::Duration;

pub fn start_file_watcher(
    sync_root: &Path,
    own_client_id: &str,
) -> mpsc::Receiver<Vec<String>> {
    let (tx, rx) = mpsc::channel();
    let sync_root = sync_root.to_path_buf();
    let own_id = own_client_id.to_string();

    std::thread::spawn(move || {
        let (ntx, nrx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |res: Result<Event, _>| {
            if let Ok(event) = res { let _ = ntx.send(event); }
        }).expect("create watcher");
        watcher.watch(&sync_root, RecursiveMode::Recursive).expect("watch");

        loop {
            let mut changed_peers = std::collections::HashSet::new();
            if let Ok(event) = nrx.recv() {
                extract_peer_ids(&event, &sync_root, &own_id, &mut changed_peers);
            }
            // Debounce: drain events for 500ms.
            let deadline = std::time::Instant::now() + Duration::from_millis(500);
            while let Ok(event) = nrx.recv_timeout(
                deadline.saturating_duration_since(std::time::Instant::now())
            ) {
                extract_peer_ids(&event, &sync_root, &own_id, &mut changed_peers);
            }
            if !changed_peers.is_empty() {
                let _ = tx.send(changed_peers.into_iter().collect());
            }
        }
    });
    rx
}

fn extract_peer_ids(
    event: &Event, sync_root: &Path, own_id: &str,
    peers: &mut std::collections::HashSet<String>,
) {
    for path in &event.paths {
        if let Ok(rel) = path.strip_prefix(sync_root) {
            if let Some(first) = rel.components().next() {
                let peer = first.as_os_str().to_string_lossy().to_string();
                if peer != own_id && !peer.starts_with('.') {
                    peers.insert(peer);
                }
            }
        }
    }
}
```

The main loop consumes watcher events and merges only the affected peers:

```rust
// In the main event loop (runs on a dedicated tokio::task or thread):
while let Ok(changed_peers) = file_watcher_rx.recv() {
    let did_change = merge_specific_peers(
        &doc_handle, &sync_root, &changed_peers,
    );
    if did_change {
        export_own_chunks(&local_store_path, &sync_root, &client_id);
        sse_broadcast_tx.send(()).ok(); // triggers HTMX refresh
    }
}
```

### 5.5 Writing Changes (User Actions)

All writes go through `DocHandle::with_doc_mut()`. The `Repo` automatically
persists changes to the local `FsStore`. After each write, call
`export_own_chunks()` (§5.6) to mirror changes to the shared directory.

```rust
pub fn add_bookmark(
    doc_handle: &DocHandle,
    folder_id: &str,
    url: &str,
    title: &str,
) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    doc_handle.with_doc_mut(|doc| {
        let bookmarks = doc.get(automerge::ROOT, "bookmarks").unwrap().unwrap().1;
        let bm = doc.put_object(&bookmarks, &id, ObjType::Map).unwrap();
        doc.put(&bm, "url", url).unwrap();
        doc.put(&bm, "title", title).unwrap();
        doc.put(&bm, "notes", "").unwrap();
        doc.put(&bm, "created_at", &now).unwrap();
        doc.put(&bm, "updated_at", &now).unwrap();
        doc.put(&bm, "deleted", false).unwrap();
        let folders = doc.get(automerge::ROOT, "folders").unwrap().unwrap().1;
        let folder = doc.get(&folders, folder_id).unwrap().unwrap().1;
        let children = doc.get(&folder, "children").unwrap().unwrap().1;
        let len = doc.length(&children);
        doc.insert(&children, len, &id).unwrap();
    });
    id
}

pub fn create_folder(
    doc_handle: &DocHandle,
    parent_folder_id: &str,
    title: &str,
) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    doc_handle.with_doc_mut(|doc| {
        let folders = doc.get(automerge::ROOT, "folders").unwrap().unwrap().1;
        let f = doc.put_object(&folders, &id, ObjType::Map).unwrap();
        doc.put(&f, "title", title).unwrap();
        doc.put_object(&f, "children", ObjType::List).unwrap();
        doc.put(&f, "created_at", &now).unwrap();
        doc.put(&f, "updated_at", &now).unwrap();
        doc.put(&f, "deleted", false).unwrap();
        let parent = doc.get(&folders, parent_folder_id).unwrap().unwrap().1;
        let ch = doc.get(&parent, "children").unwrap().unwrap().1;
        doc.insert(&ch, doc.length(&ch), &id).unwrap();
    });
    id
}
```

### 5.6 Exporting Own Chunks to Shared Folder

Mirrors the local `repo_store/` to `<sync_root>/<own_client_id>/store/`.
Uses atomic writes (temp-file-then-rename). Cleans up stale files
post-compaction.

```rust
pub fn export_own_chunks(local_store: &Path, sync_root: &Path, client_id: &str) {
    let shared = sync_root.join(client_id).join("store");
    std::fs::create_dir_all(&shared).ok();
    // Copy new local files to shared.
    for f in std::fs::read_dir(local_store).into_iter().flatten().flatten() {
        if !f.path().is_file() { continue; }
        let dest = shared.join(f.file_name());
        if !dest.exists() {
            let tmp = dest.with_extension("tmp");
            if let Ok(data) = std::fs::read(f.path()) {
                std::fs::write(&tmp, &data).ok();
                std::fs::rename(&tmp, &dest).ok();
            }
        }
    }
    // Remove shared files no longer present locally (compaction cleanup).
    for f in std::fs::read_dir(&shared).into_iter().flatten().flatten() {
        if f.file_name().to_string_lossy().ends_with(".tmp") { continue; }
        if !local_store.join(f.file_name()).exists() {
            std::fs::remove_file(f.path()).ok();
        }
    }
}
```

### 5.7 Compaction

Handled by the `Repo`'s internal `fs_store` compaction. The algorithm:
1. Loads all incremental chunks for the document.
2. Writes a snapshot named `<doc_id>.snapshot.<heads_hash>`.
3. Deletes only the incremental files it loaded (safe for concurrency).

`export_own_chunks()` then mirrors the compacted state to the shared dir.

---

## 6. Netscape Bookmark HTML Export

### 6.1 Format Overview

The Netscape Bookmark HTML format uses nested `<DL>`/`<DT>` elements.
Folders are `<DT><H3>` elements wrapping a child `<DL>`. Bookmarks are
`<DT><A>` elements. Timestamps are Unix seconds in `ADD_DATE` and
`LAST_MODIFIED` attributes.

### 6.2 Export Implementation

```rust
use std::io::Write;

pub fn export_netscape_html<W: Write>(
    store: &BookmarkStore,
    writer: &mut W,
) -> std::io::Result<()> {
    writeln!(writer, "<!DOCTYPE NETSCAPE-Bookmark-file-1>")?;
    writeln!(writer, "<!--This is an automatically generated file.")?;
    writeln!(writer, "     It will be read and overwritten. Do Not Edit! -->")?;
    writeln!(writer, r#"<META HTTP-EQUIV="Content-Type" CONTENT="text/html; charset=UTF-8">"#)?;
    writeln!(writer, "<TITLE>Bookmarks</TITLE>")?;
    writeln!(writer, "<H1>Bookmarks</H1>")?;
    writeln!(writer, "<DL><p>")?;
    if let Some(root) = store.folders.get(&store.root_folder_id) {
        write_folder_children(store, root, writer, 1)?;
    }
    writeln!(writer, "</DL><p>")?;
    Ok(())
}

fn write_folder_children<W: Write>(
    store: &BookmarkStore,
    folder: &Folder,
    writer: &mut W,
    depth: usize,
) -> std::io::Result<()> {
    let indent = "    ".repeat(depth);
    for child_id in &folder.children {
        if let Some(sub) = store.folders.get(child_id) {
            if sub.deleted { continue; }
            writeln!(writer,
                r#"{}<DT><H3 ADD_DATE="{}" LAST_MODIFIED="{}">{}</H3>"#,
                indent, to_unix(&sub.created_at), to_unix(&sub.updated_at),
                html_escape(&sub.title))?;
            writeln!(writer, "{}<DL><p>", indent)?;
            write_folder_children(store, sub, writer, depth + 1)?;
            writeln!(writer, "{}</DL><p>", indent)?;
        } else if let Some(bm) = store.bookmarks.get(child_id) {
            if bm.deleted { continue; }
            writeln!(writer,
                r#"{}<DT><A HREF="{}" ADD_DATE="{}" LAST_MODIFIED="{}">{}</A>"#,
                indent, html_escape(&bm.url),
                to_unix(&bm.created_at), to_unix(&bm.updated_at),
                html_escape(&bm.title))?;
        }
    }
    Ok(())
}

fn to_unix(iso: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(iso)
        .map(|dt| dt.timestamp()).unwrap_or(0)
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;")
     .replace('>', "&gt;").replace('"', "&quot;")
}
```

### 6.3 Import from Netscape HTML

Import parses the HTML, creates folders and bookmarks in the Automerge
document, inserting them into the appropriate parent's `children` list.
Exposed as `POST /import` accepting a file upload. The parser handles
`<DT><H3>` (folder) and `<DT><A>` (bookmark) patterns and reconstructs
the tree by tracking `<DL>` nesting depth.

```rust
use std::io::BufRead;

pub fn import_netscape_html(
    doc_handle: &DocHandle,
    target_folder_id: &str,
    html: &str,
) {
    // Simple state-machine parser for Netscape bookmark format.
    // Track a stack of folder IDs; start with the target folder.
    let mut folder_stack: Vec<String> = vec![target_folder_id.to_string()];
    let mut pending_folder_title: Option<String> = None;

    for line in html.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("<DT><H3") {
            // Extract folder title from <DT><H3 ...>Title</H3>
            if let Some(title) = extract_tag_content(trimmed, "H3") {
                let add_date = extract_attr(trimmed, "ADD_DATE");
                let parent_id = folder_stack.last().unwrap().clone();
                let folder_id = create_folder(doc_handle, &parent_id, &title);
                // Optionally set created_at from ADD_DATE
                if let Some(ts) = add_date {
                    set_created_at_from_unix(doc_handle, &folder_id, ts);
                }
                folder_stack.push(folder_id);
            }
        } else if trimmed.starts_with("<DT><A") {
            // Extract bookmark from <DT><A HREF="..." ...>Title</A>
            if let (Some(url), Some(title)) = (
                extract_attr(trimmed, "HREF"),
                extract_tag_content(trimmed, "A"),
            ) {
                let parent = folder_stack.last().unwrap();
                add_bookmark(doc_handle, parent, &url, &title);
            }
        } else if trimmed.starts_with("</DL>") {
            // Pop folder stack (but never below the target folder).
            if folder_stack.len() > 1 {
                folder_stack.pop();
            }
        }
    }
}

fn extract_attr(html: &str, attr: &str) -> Option<String> {
    let needle = format!(r#"{}=""#, attr);
    let start = html.find(&needle)? + needle.len();
    let end = html[start..].find('"')? + start;
    Some(html[start..end].to_string())
}

fn extract_tag_content(html: &str, tag: &str) -> Option<String> {
    let open_end = html.find('>')? + 1;
    let close = html.rfind(&format!("</{}", tag))?;
    Some(html[open_end..close].to_string())
}
```

---

## 7. Conflict Handling

### 7.1 Automerge Built-in Resolution

Deterministic winner via `(counter, actorId)`. Losing values preserved
in `get_all()`.

### 7.2 Application-Level Policy

| Scenario | Resolution |
|---|---|
| Same bookmark edited on two devices | Per-field merge; concurrent non-overlapping edits preserved |
| Bookmark deleted on one, edited on another | `deleted` flag checked first by app |
| Two devices add bookmarks to same folder | Both appear — Automerge list merge preserves both inserts |
| Two devices reorder same folder | Automerge list interleaves both orderings |
| Two devices move same item to different folders | Item in both; app can detect and resolve |

---

## 8. Syncthing Configuration

| Setting | Value | Reason |
|---|---|---|
| Folder type | Send & Receive | Equal peers |
| File versioning | None | Automerge handles history |
| Ignore patterns | `*.tmp` | Skip atomic-write temp files |
| Watch for changes | Enabled | Fast propagation |
| Rescan interval | 60s | Fallback |

`.stignore`:
```
*.tmp
.DS_Store
Thumbs.db
```

---

## 9. Web Server Endpoints

```
GET    /                      → Full page: folder tree + bookmark list
GET    /folders/:id           → Bookmark list for folder (HTMX partial)
POST   /folders               → Create folder
POST   /folders/:id/bookmarks → Add bookmark to folder
PUT    /bookmarks/:id         → Edit bookmark
DELETE /bookmarks/:id         → Soft-delete bookmark
POST   /move                  → Move item between folders
GET    /export                → Download Netscape HTML file
POST   /import                → Upload Netscape HTML file
GET    /events                → SSE stream for live updates
```

---

## 10. Summary: Operation-by-Operation Reference

| Operation | DecSync | This system |
|---|---|---|
| **Create collection** | `<type>/` dir + `.decsync-info` | `<sync_root>/` + `.bookmarks-sync` with `DocumentId` |
| **Register client** | `v2/<appId>/info` | `<client_id>/info.json` |
| **Write entry** | Append JSON line, bump sequence | `DocHandle::with_doc_mut()` → Repo saves to `fs_store` → export to shared dir |
| **Read peer updates** | Compare sequences, LWW | Watch fs, load peer chunks, `doc.merge()` |
| **Resolve conflict** | Later timestamp wins | Automerge `(counter, actorId)` deterministic; all values preserved |
| **New client bootstrap** | `initStoredEntries()` | Load + merge all peer snapshots and incrementals |
| **Compaction** | Rewrite `stored-entries/` | `fs_store` auto-compacts: snapshot replaces incrementals |
| **Export bookmarks** | N/A | `GET /export` → Netscape HTML |
| **Import bookmarks** | N/A | `POST /import` ← Netscape HTML |

---

## 11. Crate Dependencies

```toml
[dependencies]
automerge = "0.7"
automerge_repo = { version = "0.3", features = ["tokio"] }
autosurgeon = { version = "0.9", features = ["uuid"] }
axum = { version = "0.8", features = ["macros"] }
chrono = { version = "0.4", features = ["serde"] }
hostname = "0.4"
notify = "6"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
uuid = { version = "1", features = ["v4"] }
```

> `automerge_repo` v0.3.0 (published Oct 2025) depends on `automerge ^0.7`
> and `autosurgeon ^0.9`. The `fs_store` compaction fix (issue #52) is
> included — no git dependency needed.
