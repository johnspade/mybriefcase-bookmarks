use automerge::{ChangeHash, ReadDoc};
use automerge_repo::DocHandle;
use serde::Serialize;

use crate::schema::BookmarkField::{CreatedAt, Notes, Title, UpdatedAt, Url};
use crate::schema::BookmarkStoreField::Bookmarks;

const MAX_HISTORY_ENTRIES: usize = 50;

#[derive(Debug, Clone, Serialize)]
pub struct HistoryEntry {
    pub hash: String,
    pub timestamp: i64,
    pub actor: String,
    pub message: Option<String>,
    pub changed_fields: Vec<FieldChange>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FieldChange {
    pub field: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BookmarkSnapshot {
    pub url: String,
    pub title: String,
    pub notes: String,
    pub created_at: String,
    pub updated_at: String,
}

fn extract_bookmark_fields(
    doc: &automerge::Automerge,
    bookmark_id: &str,
    heads: &[ChangeHash],
) -> Option<BookmarkSnapshot> {
    let bookmarks_obj = doc.get(automerge::ROOT, Bookmarks.as_ref()).ok()??.1;
    let bm_obj = doc.get(&bookmarks_obj, bookmark_id).ok()??.1;

    let url = doc
        .get_at(&bm_obj, Url.as_ref(), heads)
        .ok()?
        .map(|(v, _)| v.into_string().unwrap_or_default())?;
    let title = doc
        .get_at(&bm_obj, Title.as_ref(), heads)
        .ok()?
        .map(|(v, _)| v.into_string().unwrap_or_default())?;
    let notes = doc
        .get_at(&bm_obj, Notes.as_ref(), heads)
        .ok()?
        .map(|(v, _)| v.into_string().unwrap_or_default())
        .unwrap_or_default();
    let created_at = doc
        .get_at(&bm_obj, CreatedAt.as_ref(), heads)
        .ok()?
        .map(|(v, _)| v.into_string().unwrap_or_default())
        .unwrap_or_default();
    let updated_at = doc
        .get_at(&bm_obj, UpdatedAt.as_ref(), heads)
        .ok()?
        .map(|(v, _)| v.into_string().unwrap_or_default())
        .unwrap_or_default();

    Some(BookmarkSnapshot {
        url,
        title,
        notes,
        created_at,
        updated_at,
    })
}

fn compute_field_changes(
    before: Option<&BookmarkSnapshot>,
    after: &BookmarkSnapshot,
) -> Vec<FieldChange> {
    let mut changes = Vec::new();
    for (field, old_val, new_val) in [
        (
            Title,
            before.map(|s| s.title.as_str()),
            after.title.as_str(),
        ),
        (Url, before.map(|s| s.url.as_str()), after.url.as_str()),
        (
            Notes,
            before.map(|s| s.notes.as_str()),
            after.notes.as_str(),
        ),
    ] {
        if old_val.is_none_or(|o| o != new_val) {
            changes.push(FieldChange {
                field: <&str>::from(field).to_owned(),
                old_value: old_val.map(String::from),
                new_value: Some(new_val.to_owned()),
            });
        }
    }
    changes
}

/// Returns the change history for a specific bookmark, newest first.
/// Uses commit messages for fast filtering, then hydrates snapshots at each relevant change.
pub fn bookmark_history(doc_handle: &DocHandle, bookmark_id: &str) -> Vec<HistoryEntry> {
    doc_handle.with_doc(|doc| {
        let changes = doc.get_changes(&[]);
        let tag_prefix = format!("_bookmark:{bookmark_id}");
        let add_tag = format!("add_bookmark:{bookmark_id}");
        let update_tag = format!("update_bookmark:{bookmark_id}");
        let delete_tag = format!("delete_bookmark:{bookmark_id}");
        let revert_prefix = format!("revert_bookmark:{bookmark_id}:");

        let mut relevant: Vec<_> = changes
            .iter()
            .filter(|c| {
                c.message().is_some_and(|m| {
                    m.ends_with(&tag_prefix)
                        || m == &add_tag
                        || m == &update_tag
                        || m == &delete_tag
                        || m.starts_with(&revert_prefix)
                })
            })
            .collect();

        relevant.truncate(relevant.len().min(MAX_HISTORY_ENTRIES * 2));

        let mut accumulated_heads: Vec<ChangeHash> = Vec::new();
        let mut entries = Vec::new();
        let all_changes: Vec<_> = changes.iter().collect();
        let mut prev_snapshot: Option<BookmarkSnapshot> = None;

        for change in &all_changes {
            accumulated_heads.retain(|h| !change.deps().contains(h));
            accumulated_heads.push(change.hash());

            if !relevant.iter().any(|r| r.hash() == change.hash()) {
                continue;
            }

            let snapshot = extract_bookmark_fields(doc, bookmark_id, &accumulated_heads);
            let Some(current) = &snapshot else { continue };

            let changed_fields = compute_field_changes(prev_snapshot.as_ref(), current);

            entries.push(HistoryEntry {
                hash: change.hash().to_string(),
                timestamp: change.timestamp(),
                actor: change.actor_id().to_hex_string(),
                message: change.message().cloned(),
                changed_fields,
            });

            prev_snapshot = snapshot;
        }

        entries.reverse();
        entries.truncate(MAX_HISTORY_ENTRIES);
        entries
    })
}

/// Returns a bookmark snapshot at a specific change hash.
pub fn bookmark_at_hash(
    doc_handle: &DocHandle,
    bookmark_id: &str,
    hash: &ChangeHash,
) -> Option<BookmarkSnapshot> {
    doc_handle.with_doc(|doc| {
        doc.get_change_by_hash(hash)?;
        extract_bookmark_fields(doc, bookmark_id, &[*hash])
    })
}

/// Returns a bookmark snapshot at the current document heads.
pub fn bookmark_current(doc_handle: &DocHandle, bookmark_id: &str) -> Option<BookmarkSnapshot> {
    doc_handle.with_doc(|doc| {
        let heads = doc.get_heads();
        extract_bookmark_fields(doc, bookmark_id, &heads)
    })
}

/// Parses a hex-encoded change hash string into a `ChangeHash`.
#[must_use]
pub fn parse_change_hash(hex: &str) -> Option<ChangeHash> {
    if hex.len() != 64 {
        return None;
    }
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Some(ChangeHash(arr))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops;
    use crate::schema::BookmarkField::Deleted;
    use crate::schema::BookmarkStoreField::{Folders, Meta, RootFolderId};
    use crate::schema::FolderField::Children;
    use crate::schema::StoreMetaField::{CollectionName, SchemaVersion};
    use automerge::ObjType;
    use automerge::transaction::{CommitOptions, Transactable};
    use automerge_repo::Repo;
    use automerge_repo::tokio::FsStorage;
    use tempfile::TempDir;

    fn setup_repo() -> (DocHandle, TempDir, String) {
        let temp_dir = TempDir::new().unwrap();
        let store = FsStorage::open(temp_dir.path()).unwrap();
        let repo = Repo::new(None, Box::new(store));
        let handle = repo.run();
        let doc_handle = handle.new_document();
        let root_id = uuid::Uuid::new_v4().to_string();
        doc_handle.with_doc_mut(|doc| {
            let mut tx = doc.transaction();
            let now = chrono::Utc::now().to_rfc3339();
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
            tx.put_object(&root, Children.as_ref(), ObjType::List)
                .unwrap();
            tx.put(&root, CreatedAt.as_ref(), now.as_str()).unwrap();
            tx.put(&root, UpdatedAt.as_ref(), now.as_str()).unwrap();
            tx.put(&root, Deleted.as_ref(), false).unwrap();
            tx.commit_with(CommitOptions::default().with_message("init_schema"));
        });
        (doc_handle, temp_dir, root_id)
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn test_bookmark_history_tracks_changes() {
        let (doc, _tmp, root_id) = setup_repo();
        let bm_id = ops::add_bookmark(&doc, &root_id, "https://example.com", "Example").unwrap();
        ops::update_bookmark(&doc, &bm_id, None, Some("Updated Title"), None).unwrap();
        ops::update_bookmark(&doc, &bm_id, Some("https://new.com"), None, None).unwrap();

        let history = bookmark_history(&doc, &bm_id);
        assert_eq!(history.len(), 3);
        assert_eq!(
            history[0].message.as_deref(),
            Some(&*format!("update_bookmark:{bm_id}"))
        );
        assert!(history[0].changed_fields.iter().any(|f| f.field == "url"));
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn test_bookmark_at_hash_returns_snapshot() {
        let (doc, _tmp, root_id) = setup_repo();
        let bm_id = ops::add_bookmark(&doc, &root_id, "https://original.com", "Original").unwrap();

        let history_before = bookmark_history(&doc, &bm_id);
        let create_hash_hex = &history_before.last().unwrap().hash;
        let create_hash = parse_change_hash(create_hash_hex).unwrap();

        ops::update_bookmark(&doc, &bm_id, None, Some("Changed"), None).unwrap();

        let snapshot = bookmark_at_hash(&doc, &bm_id, &create_hash).unwrap();
        assert_eq!(snapshot.title, "Original");
        assert_eq!(snapshot.url, "https://original.com");
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn test_revert_bookmark_restores_old_state() {
        let (doc, _tmp, root_id) = setup_repo();
        let bm_id = ops::add_bookmark(&doc, &root_id, "https://original.com", "Original").unwrap();

        let history_before = bookmark_history(&doc, &bm_id);
        let create_hash = parse_change_hash(&history_before.last().unwrap().hash).unwrap();

        ops::update_bookmark(
            &doc,
            &bm_id,
            Some("https://changed.com"),
            Some("Changed"),
            None,
        )
        .unwrap();

        ops::revert_bookmark(&doc, &bm_id, &create_hash).unwrap();

        let current = bookmark_current(&doc, &bm_id).unwrap();
        assert_eq!(current.title, "Original");
        assert_eq!(current.url, "https://original.com");
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn test_revert_appears_in_history() {
        let (doc, _tmp, root_id) = setup_repo();
        let bm_id = ops::add_bookmark(&doc, &root_id, "https://example.com", "V1").unwrap();

        let history = bookmark_history(&doc, &bm_id);
        let create_hash = parse_change_hash(&history.last().unwrap().hash).unwrap();

        ops::update_bookmark(&doc, &bm_id, None, Some("V2"), None).unwrap();
        ops::revert_bookmark(&doc, &bm_id, &create_hash).unwrap();

        let history = bookmark_history(&doc, &bm_id);
        assert!(
            history[0]
                .message
                .as_ref()
                .unwrap()
                .starts_with("revert_bookmark:")
        );
    }

    #[test]
    fn test_parse_change_hash_valid() {
        let hex = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
        let result = parse_change_hash(hex);
        assert!(result.is_some());
    }

    #[test]
    fn test_parse_change_hash_invalid() {
        assert!(parse_change_hash("tooshort").is_none());
        assert!(parse_change_hash("zzzzzzzz").is_none());
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn test_history_only_shows_relevant_bookmark() {
        let (doc, _tmp, root_id) = setup_repo();
        let bm1 = ops::add_bookmark(&doc, &root_id, "https://one.com", "One").unwrap();
        let _bm2 = ops::add_bookmark(&doc, &root_id, "https://two.com", "Two").unwrap();
        ops::update_bookmark(&doc, &bm1, None, Some("One Updated"), None).unwrap();

        let history = bookmark_history(&doc, &bm1);
        assert_eq!(history.len(), 2);
        for entry in &history {
            assert!(entry.message.as_ref().unwrap().contains(&bm1));
        }
    }

    #[test]
    fn test_compute_field_changes_no_changes() {
        let snapshot = BookmarkSnapshot {
            url: "https://x.com".to_owned(),
            title: "X".to_owned(),
            notes: "n".to_owned(),
            created_at: "2026-01-01".to_owned(),
            updated_at: "2026-01-01".to_owned(),
        };
        let changes = compute_field_changes(Some(&snapshot), &snapshot);
        assert!(changes.is_empty());
    }

    #[test]
    fn test_compute_field_changes_all_fields() {
        let before = BookmarkSnapshot {
            url: "https://old.com".to_owned(),
            title: "Old".to_owned(),
            notes: "old notes".to_owned(),
            created_at: "2026-01-01".to_owned(),
            updated_at: "2026-01-01".to_owned(),
        };
        let after = BookmarkSnapshot {
            url: "https://new.com".to_owned(),
            title: "New".to_owned(),
            notes: "new notes".to_owned(),
            created_at: "2026-01-01".to_owned(),
            updated_at: "2026-01-02".to_owned(),
        };
        let changes = compute_field_changes(Some(&before), &after);
        assert_eq!(changes.len(), 3);
        assert!(changes.iter().any(|c| c.field == "title"));
        assert!(changes.iter().any(|c| c.field == "url"));
        assert!(changes.iter().any(|c| c.field == "notes"));
    }

    #[test]
    fn test_compute_field_changes_none_before() {
        let after = BookmarkSnapshot {
            url: "https://x.com".to_owned(),
            title: "X".to_owned(),
            notes: String::new(),
            created_at: "2026-01-01".to_owned(),
            updated_at: "2026-01-01".to_owned(),
        };
        let changes = compute_field_changes(None, &after);
        assert_eq!(changes.len(), 3);
    }

    #[test]
    fn test_parse_change_hash_empty_string() {
        assert!(parse_change_hash("").is_none());
    }

    #[test]
    fn test_parse_change_hash_odd_length() {
        assert!(parse_change_hash("abc").is_none());
    }

    #[test]
    fn test_parse_change_hash_non_hex() {
        let non_hex = "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz";
        assert!(parse_change_hash(non_hex).is_none());
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn test_history_truncates_at_max() {
        let (doc, _tmp, root_id) = setup_repo();
        let bm_id = ops::add_bookmark(&doc, &root_id, "https://x.com", "X").unwrap();
        for i in 0..55 {
            ops::update_bookmark(&doc, &bm_id, None, Some(&format!("Title {i}")), None).unwrap();
        }
        let history = bookmark_history(&doc, &bm_id);
        assert!(
            history.len() <= MAX_HISTORY_ENTRIES,
            "history should be capped at {MAX_HISTORY_ENTRIES}, got {}",
            history.len()
        );
    }
}
