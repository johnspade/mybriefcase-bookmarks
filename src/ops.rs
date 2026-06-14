use anyhow::Context;
use automerge::transaction::{CommitOptions, Transactable};
use automerge::{ObjType, ReadDoc};
use automerge_repo::DocHandle;

use crate::schema;

fn commit_opts(message: String) -> CommitOptions {
    let now = chrono::Utc::now().timestamp_millis();
    CommitOptions::default()
        .with_message(message)
        .with_time(now)
}

/// # Errors
/// Returns an error if the document schema is invalid or the automerge transaction fails.
pub fn add_bookmark(
    doc_handle: &DocHandle,
    folder_id: &str,
    url: &str,
    title: &str,
) -> anyhow::Result<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    doc_handle.with_doc_mut(|doc| -> anyhow::Result<()> {
        let mut tx = doc.transaction();
        let bookmarks = tx
            .get(automerge::ROOT, "bookmarks")?
            .context("missing bookmarks map")?
            .1;
        let bm = tx.put_object(&bookmarks, id.as_str(), ObjType::Map)?;
        schema::write_bookmark(
            &mut tx,
            &bm,
            &schema::BookmarkFields {
                url,
                title,
                notes: "",
                favicon: "",
                created_at: &now,
                updated_at: &now,
            },
        )?;
        let folders = tx
            .get(automerge::ROOT, "folders")?
            .context("missing folders map")?
            .1;
        let folder = tx
            .get(&folders, folder_id)?
            .with_context(|| format!("folder not found: {folder_id}"))?
            .1;
        let children = tx
            .get(&folder, schema::CHILDREN)?
            .context("folder missing children")?
            .1;
        let len = tx.length(&children);
        tx.insert(&children, len, id.as_str())?;
        tx.commit_with(commit_opts(format!("add_bookmark:{id}")));
        Ok(())
    })?;
    Ok(id)
}

/// # Errors
/// Returns an error if the document schema is invalid or the automerge transaction fails.
pub fn update_favicon(
    doc_handle: &DocHandle,
    bookmark_id: &str,
    favicon: &str,
) -> anyhow::Result<()> {
    doc_handle.with_doc_mut(|doc| -> anyhow::Result<()> {
        let mut tx = doc.transaction();
        let bookmarks = tx
            .get(automerge::ROOT, "bookmarks")?
            .context("missing bookmarks map")?
            .1;
        let bm = tx
            .get(&bookmarks, bookmark_id)?
            .with_context(|| format!("bookmark not found: {bookmark_id}"))?
            .1;
        schema::patch_bookmark(&mut tx, &bm, None, None, None, Some(favicon))?;
        tx.commit_with(commit_opts(format!("update_favicon:{bookmark_id}")));
        Ok(())
    })?;
    Ok(())
}

/// # Errors
/// Returns an error if the document schema is invalid or the automerge transaction fails.
pub fn create_folder(
    doc_handle: &DocHandle,
    parent_folder_id: &str,
    title: &str,
) -> anyhow::Result<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    doc_handle.with_doc_mut(|doc| -> anyhow::Result<()> {
        let mut tx = doc.transaction();
        let folders = tx
            .get(automerge::ROOT, "folders")?
            .context("missing folders map")?
            .1;
        let f = tx.put_object(&folders, id.as_str(), ObjType::Map)?;
        schema::write_folder(&mut tx, &f, title, &now, &now)?;
        let parent = tx
            .get(&folders, parent_folder_id)?
            .with_context(|| format!("parent folder not found: {parent_folder_id}"))?
            .1;
        let ch = tx
            .get(&parent, schema::CHILDREN)?
            .context("parent missing children")?
            .1;
        tx.insert(&ch, tx.length(&ch), id.as_str())?;
        tx.commit_with(commit_opts(format!("create_folder:{id}")));
        Ok(())
    })?;
    Ok(id)
}

/// # Errors
/// Returns an error if the document schema is invalid or the automerge transaction fails.
pub fn update_bookmark(
    doc_handle: &DocHandle,
    bookmark_id: &str,
    url: Option<&str>,
    title: Option<&str>,
    notes: Option<&str>,
) -> anyhow::Result<()> {
    doc_handle.with_doc_mut(|doc| -> anyhow::Result<()> {
        let mut tx = doc.transaction();
        let bookmarks = tx
            .get(automerge::ROOT, "bookmarks")?
            .context("missing bookmarks map")?
            .1;
        let bm = tx
            .get(&bookmarks, bookmark_id)?
            .with_context(|| format!("bookmark not found: {bookmark_id}"))?
            .1;
        schema::patch_bookmark(&mut tx, &bm, url, title, notes, None)?;
        tx.commit_with(commit_opts(format!("update_bookmark:{bookmark_id}")));
        Ok(())
    })
}

/// # Errors
/// Returns an error if the document schema is invalid or the automerge transaction fails.
pub fn delete_bookmark(doc_handle: &DocHandle, bookmark_id: &str) -> anyhow::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    doc_handle.with_doc_mut(|doc| -> anyhow::Result<()> {
        let mut tx = doc.transaction();
        let bookmarks = tx
            .get(automerge::ROOT, "bookmarks")?
            .context("missing bookmarks map")?
            .1;
        let bm = tx
            .get(&bookmarks, bookmark_id)?
            .with_context(|| format!("bookmark not found: {bookmark_id}"))?
            .1;
        tx.put(&bm, schema::DELETED, true)?;
        tx.put(&bm, schema::UPDATED_AT, now.as_str())?;

        let folders = tx
            .get(automerge::ROOT, "folders")?
            .context("missing folders map")?
            .1;
        let folder_keys = tx.keys(&folders).collect::<Vec<_>>();
        for folder_key in folder_keys {
            let folder_obj = tx
                .get(&folders, folder_key.as_str())?
                .with_context(|| format!("folder not found: {folder_key}"))?
                .1;
            let children = tx
                .get(&folder_obj, schema::CHILDREN)?
                .context("folder missing children")?
                .1;
            let len = tx.length(&children);
            for i in (0..len).rev() {
                if let Ok(Some((automerge::Value::Scalar(s), _))) = tx.get(&children, i) {
                    if s.to_str() == Some(bookmark_id) {
                        tx.delete(&children, i)?;
                    }
                }
            }
        }
        tx.commit_with(commit_opts(format!("delete_bookmark:{bookmark_id}")));
        Ok(())
    })
}

fn mark_descendants_deleted(
    tx: &mut automerge::transaction::Transaction<'_>,
    folders: &automerge::ObjId,
    bookmarks: &automerge::ObjId,
    root_folder_id: &str,
    now: &str,
) -> anyhow::Result<()> {
    let mut stack = vec![root_folder_id.to_string()];
    let mut visited = std::collections::HashSet::new();

    while let Some(fid) = stack.pop() {
        if !visited.insert(fid.clone()) {
            continue;
        }

        let Some((_, folder)) = tx.get(folders, fid.as_str())? else {
            continue;
        };
        tx.put(&folder, schema::DELETED, true)?;
        tx.put(&folder, schema::UPDATED_AT, now)?;

        let children = tx
            .get(&folder, schema::CHILDREN)?
            .context("folder missing children")?
            .1;
        let len = tx.length(&children);
        for i in 0..len {
            if let Ok(Some((automerge::Value::Scalar(s), _))) = tx.get(&children, i) {
                let child_id = match s.to_str() {
                    Some(id) => id.to_string(),
                    None => continue,
                };
                if tx.get(folders, child_id.as_str())?.is_some() {
                    stack.push(child_id);
                } else if let Some((_, bm_obj)) = tx.get(bookmarks, child_id.as_str())? {
                    tx.put(&bm_obj, schema::DELETED, true)?;
                    tx.put(&bm_obj, schema::UPDATED_AT, now)?;
                }
            }
        }
    }
    Ok(())
}

/// # Errors
/// Returns an error if the document schema is invalid or the automerge transaction fails.
pub fn delete_folder(doc_handle: &DocHandle, folder_id: &str) -> anyhow::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    doc_handle.with_doc_mut(|doc| -> anyhow::Result<()> {
        let mut tx = doc.transaction();
        let folders = tx
            .get(automerge::ROOT, "folders")?
            .context("missing folders map")?
            .1;
        let bookmarks = tx
            .get(automerge::ROOT, "bookmarks")?
            .context("missing bookmarks map")?
            .1;

        mark_descendants_deleted(&mut tx, &folders, &bookmarks, folder_id, &now)?;

        let folder_keys = tx.keys(&folders).collect::<Vec<_>>();
        for key in folder_keys {
            let f = tx
                .get(&folders, key.as_str())?
                .with_context(|| format!("folder not found: {key}"))?
                .1;
            let children = tx
                .get(&f, schema::CHILDREN)?
                .context("folder missing children")?
                .1;
            let len = tx.length(&children);
            for i in (0..len).rev() {
                if let Ok(Some((automerge::Value::Scalar(s), _))) = tx.get(&children, i) {
                    if s.to_str() == Some(folder_id) {
                        tx.delete(&children, i)?;
                    }
                }
            }
        }
        tx.commit_with(commit_opts(format!("delete_folder:{folder_id}")));
        Ok(())
    })
}

/// # Errors
/// Returns an error if the document schema is invalid or the automerge transaction fails.
pub fn rename_folder(
    doc_handle: &DocHandle,
    folder_id: &str,
    new_title: &str,
) -> anyhow::Result<()> {
    doc_handle.with_doc_mut(|doc| -> anyhow::Result<()> {
        let mut tx = doc.transaction();
        let folders = tx
            .get(automerge::ROOT, "folders")?
            .context("missing folders map")?
            .1;
        let folder = tx
            .get(&folders, folder_id)?
            .with_context(|| format!("folder not found: {folder_id}"))?
            .1;
        schema::patch_folder(&mut tx, &folder, new_title)?;
        tx.commit_with(commit_opts(format!("rename_folder:{folder_id}")));
        Ok(())
    })
}

/// # Errors
/// Returns an error if the move would create a cycle, or if the document schema is invalid,
/// or if the automerge transaction fails.
pub fn move_item(
    doc_handle: &DocHandle,
    item_id: &str,
    from_folder_id: &str,
    to_folder_id: &str,
) -> anyhow::Result<()> {
    if from_folder_id == to_folder_id {
        return Ok(());
    }
    anyhow::ensure!(item_id != to_folder_id, "cannot move a folder into itself");

    doc_handle.with_doc_mut(|doc| -> anyhow::Result<()> {
        let mut tx = doc.transaction();
        let folders = tx
            .get(automerge::ROOT, "folders")?
            .context("missing folders map")?
            .1;

        if tx.get(&folders, item_id)?.is_some() {
            anyhow::ensure!(
                !is_descendant_in_tx(&tx, &folders, item_id, to_folder_id),
                "cannot move a folder into its own subtree"
            );
        }

        let from_folder = tx
            .get(&folders, from_folder_id)?
            .with_context(|| format!("source folder not found: {from_folder_id}"))?
            .1;
        let from_children = tx
            .get(&from_folder, schema::CHILDREN)?
            .context("source folder missing children")?
            .1;
        let from_len = tx.length(&from_children);
        for i in (0..from_len).rev() {
            if let Ok(Some((automerge::Value::Scalar(s), _))) = tx.get(&from_children, i) {
                if s.to_str() == Some(item_id) {
                    tx.delete(&from_children, i)?;
                    break;
                }
            }
        }

        let to_folder = tx
            .get(&folders, to_folder_id)?
            .with_context(|| format!("destination folder not found: {to_folder_id}"))?
            .1;
        let to_children = tx
            .get(&to_folder, schema::CHILDREN)?
            .context("destination folder missing children")?
            .1;
        let to_len = tx.length(&to_children);
        tx.insert(&to_children, to_len, item_id)?;
        tx.commit_with(commit_opts(format!("move_item:{item_id}")));
        Ok(())
    })
}

fn is_descendant_in_tx(
    tx: &automerge::transaction::Transaction<'_>,
    folders: &automerge::ObjId,
    ancestor_id: &str,
    target_id: &str,
) -> bool {
    let mut stack = vec![ancestor_id.to_string()];
    let mut visited = std::collections::HashSet::new();

    while let Some(fid) = stack.pop() {
        if !visited.insert(fid.clone()) {
            continue;
        }
        let Some((_, folder)) = tx.get(folders, fid.as_str()).ok().flatten() else {
            continue;
        };
        let Some((_, children)) = tx.get(&folder, schema::CHILDREN).ok().flatten() else {
            continue;
        };
        let len = tx.length(&children);
        for i in 0..len {
            if let Ok(Some((automerge::Value::Scalar(s), _))) = tx.get(&children, i) {
                if let Some(child_id) = s.to_str() {
                    if child_id == target_id {
                        return true;
                    }
                    if tx.get(folders, child_id).ok().flatten().is_some() {
                        stack.push(child_id.to_string());
                    }
                }
            }
        }
    }
    false
}

fn insert_items_recursive(
    tx: &mut automerge::transaction::Transaction<'_>,
    folders: &automerge::ObjId,
    bookmarks_map: &automerge::ObjId,
    parent_id: &str,
    items: &[crate::import::ImportedItem],
    bc: &mut usize,
    fc: &mut usize,
) -> anyhow::Result<()> {
    let parent = tx
        .get(folders, parent_id)?
        .with_context(|| format!("folder not found: {parent_id}"))?
        .1;
    let children = tx
        .get(&parent, schema::CHILDREN)?
        .context("folder missing children")?
        .1;

    for item in items {
        match item {
            crate::import::ImportedItem::Bookmark {
                url,
                title,
                notes,
                created_at,
                updated_at,
            } => {
                let id = uuid::Uuid::new_v4().to_string();
                let now = chrono::Utc::now().to_rfc3339();
                let bm = tx.put_object(bookmarks_map, id.as_str(), ObjType::Map)?;
                schema::write_bookmark(
                    tx,
                    &bm,
                    &schema::BookmarkFields {
                        url,
                        title,
                        notes,
                        favicon: "",
                        created_at: created_at.as_deref().unwrap_or(&now),
                        updated_at: updated_at.as_deref().unwrap_or(&now),
                    },
                )?;
                let len = tx.length(&children);
                tx.insert(&children, len, id.as_str())?;
                *bc += 1;
            }
            crate::import::ImportedItem::Folder {
                title,
                created_at,
                updated_at,
                children: sub_items,
            } => {
                let id = uuid::Uuid::new_v4().to_string();
                let now = chrono::Utc::now().to_rfc3339();
                let f = tx.put_object(folders, id.as_str(), ObjType::Map)?;
                schema::write_folder(
                    tx,
                    &f,
                    title,
                    created_at.as_deref().unwrap_or(&now),
                    updated_at.as_deref().unwrap_or(&now),
                )?;
                let len = tx.length(&children);
                tx.insert(&children, len, id.as_str())?;
                *fc += 1;

                if !sub_items.is_empty() {
                    insert_items_recursive(tx, folders, bookmarks_map, &id, sub_items, bc, fc)?;
                }
            }
        }
    }
    Ok(())
}

/// Bulk-imports items from a parsed Netscape HTML file into a target folder in a single
/// Automerge transaction.
/// # Errors
/// Returns an error if the document schema is invalid or the automerge transaction fails.
pub fn import_items(
    doc_handle: &DocHandle,
    parent_folder_id: &str,
    items: &[crate::import::ImportedItem],
) -> anyhow::Result<(usize, usize)> {
    let mut bookmark_count = 0usize;
    let mut folder_count = 0usize;

    doc_handle.with_doc_mut(|doc| -> anyhow::Result<()> {
        let mut tx = doc.transaction();
        let folders = tx
            .get(automerge::ROOT, "folders")?
            .context("missing folders map")?
            .1;
        let bookmarks_map = tx
            .get(automerge::ROOT, "bookmarks")?
            .context("missing bookmarks map")?
            .1;

        insert_items_recursive(
            &mut tx,
            &folders,
            &bookmarks_map,
            parent_folder_id,
            items,
            &mut bookmark_count,
            &mut folder_count,
        )?;

        tx.commit_with(commit_opts(format!(
            "import:{bookmark_count}_bookmarks,{folder_count}_folders"
        )));
        Ok(())
    })?;

    Ok((bookmark_count, folder_count))
}

/// Reverts a bookmark to a previous state identified by `target_hash`.
/// Reads the bookmark fields at the target heads and writes them as new `put()` operations.
/// # Errors
/// Returns an error if the document schema is invalid, the target hash is invalid,
/// or the automerge transaction fails.
pub fn revert_bookmark(
    doc_handle: &DocHandle,
    bookmark_id: &str,
    target_hash: &automerge::ChangeHash,
) -> anyhow::Result<()> {
    doc_handle.with_doc_mut(|doc| -> anyhow::Result<()> {
        let target_heads = &[*target_hash];

        let bookmarks_obj = doc
            .get(automerge::ROOT, "bookmarks")?
            .context("missing bookmarks map")?
            .1;
        let bm_obj = doc
            .get(&bookmarks_obj, bookmark_id)?
            .with_context(|| format!("bookmark not found: {bookmark_id}"))?
            .1;

        let old_url = doc
            .get_at(&bm_obj, schema::URL, target_heads)?
            .map(|(v, _)| v.into_string().unwrap_or_default())
            .unwrap_or_default();
        let old_title = doc
            .get_at(&bm_obj, schema::TITLE, target_heads)?
            .map(|(v, _)| v.into_string().unwrap_or_default())
            .unwrap_or_default();
        let old_notes = doc
            .get_at(&bm_obj, schema::NOTES, target_heads)?
            .map(|(v, _)| v.into_string().unwrap_or_default())
            .unwrap_or_default();
        let old_favicon = doc
            .get_at(&bm_obj, schema::FAVICON, target_heads)?
            .map(|(v, _)| v.into_string().unwrap_or_default())
            .unwrap_or_default();

        let mut tx = doc.transaction();
        schema::patch_bookmark(
            &mut tx,
            &bm_obj,
            Some(&old_url),
            Some(&old_title),
            Some(&old_notes),
            Some(&old_favicon),
        )?;
        let short_hash = &format!("{target_hash}")[..8];
        tx.commit_with(commit_opts(format!(
            "revert_bookmark:{bookmark_id}:{short_hash}"
        )));
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use automerge::ObjType;
    use automerge::transaction::{CommitOptions, Transactable};
    use automerge_repo::Repo;
    use automerge_repo::tokio::FsStorage;
    use autosurgeon::hydrate;
    use tempfile::TempDir;

    use crate::model::BookmarkStore;

    fn setup_repo() -> (DocHandle, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let store = FsStorage::open(temp_dir.path()).unwrap();
        let repo = Repo::new(None, Box::new(store));
        let handle = repo.run();
        let doc_handle = handle.new_document();
        doc_handle.with_doc_mut(|doc| {
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
            tx.put_object(&root, "children", ObjType::List).unwrap();
            tx.put(&root, "created_at", now.as_str()).unwrap();
            tx.put(&root, "updated_at", now.as_str()).unwrap();
            tx.put(&root, "deleted", false).unwrap();
            tx.commit_with(CommitOptions::default().with_message("init_schema"));
        });
        (doc_handle, temp_dir)
    }

    fn read_store(doc_handle: &DocHandle) -> BookmarkStore {
        doc_handle.with_doc(|doc| hydrate(doc).unwrap())
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn test_add_bookmark() {
        let (doc, _tmp) = setup_repo();
        let store = read_store(&doc);
        let root_id = store.root_folder_id;
        let id = add_bookmark(&doc, &root_id, "https://example.com", "Example").unwrap();
        let store = read_store(&doc);
        let bm = store.bookmarks.get(&id).unwrap();
        assert_eq!(bm.url, "https://example.com");
        assert_eq!(bm.title, "Example");
        assert_eq!(bm.favicon, "");
        assert!(!bm.deleted);
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn test_create_folder() {
        let (doc, _tmp) = setup_repo();
        let store = read_store(&doc);
        let root_id = store.root_folder_id;
        let folder_id = create_folder(&doc, &root_id, "Work").unwrap();
        let store = read_store(&doc);
        let root = store.folders.get(&root_id).unwrap();
        assert!(root.children.contains(&folder_id));
        let folder = store.folders.get(&folder_id).unwrap();
        assert_eq!(folder.title, "Work");
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn test_update_bookmark() {
        let (doc, _tmp) = setup_repo();
        let store = read_store(&doc);
        let root_id = store.root_folder_id;
        let id = add_bookmark(&doc, &root_id, "https://old.com", "Old").unwrap();
        update_bookmark(&doc, &id, Some("https://new.com"), Some("New"), None).unwrap();
        let store = read_store(&doc);
        let bm = store.bookmarks.get(&id).unwrap();
        assert_eq!(bm.url, "https://new.com");
        assert_eq!(bm.title, "New");
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn test_delete_bookmark() {
        let (doc, _tmp) = setup_repo();
        let store = read_store(&doc);
        let root_id = store.root_folder_id;
        let id = add_bookmark(&doc, &root_id, "https://del.com", "Del").unwrap();
        delete_bookmark(&doc, &id).unwrap();
        let store = read_store(&doc);
        let bm = store.bookmarks.get(&id).unwrap();
        assert!(bm.deleted);
        let root = store.folders.get(&root_id).unwrap();
        assert!(!root.children.contains(&id));
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn test_rename_folder() {
        let (doc, _tmp) = setup_repo();
        let store = read_store(&doc);
        let root_id = store.root_folder_id;
        let folder_id = create_folder(&doc, &root_id, "Old Name").unwrap();
        rename_folder(&doc, &folder_id, "New Name").unwrap();
        let store = read_store(&doc);
        let folder = store.folders.get(&folder_id).unwrap();
        assert_eq!(folder.title, "New Name");
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn test_move_item() {
        let (doc, _tmp) = setup_repo();
        let store = read_store(&doc);
        let root_id = store.root_folder_id;
        let folder_a = create_folder(&doc, &root_id, "A").unwrap();
        let folder_b = create_folder(&doc, &root_id, "B").unwrap();
        let bm_id = add_bookmark(&doc, &folder_a, "https://test.com", "Test").unwrap();
        move_item(&doc, &bm_id, &folder_a, &folder_b).unwrap();
        let store = read_store(&doc);
        let a = store.folders.get(&folder_a).unwrap();
        let b = store.folders.get(&folder_b).unwrap();
        assert!(!a.children.contains(&bm_id));
        assert!(b.children.contains(&bm_id));
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn test_delete_folder_cascades_to_child_bookmarks() {
        let (doc, _tmp) = setup_repo();
        let store = read_store(&doc);
        let root_id = store.root_folder_id;
        let folder_id = create_folder(&doc, &root_id, "Cascade").unwrap();
        let bm1 = add_bookmark(&doc, &folder_id, "https://a.com", "A").unwrap();
        let bm2 = add_bookmark(&doc, &folder_id, "https://b.com", "B").unwrap();
        delete_folder(&doc, &folder_id).unwrap();
        let store = read_store(&doc);
        assert!(store.folders.get(&folder_id).unwrap().deleted);
        assert!(store.bookmarks.get(&bm1).unwrap().deleted);
        assert!(store.bookmarks.get(&bm2).unwrap().deleted);
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn test_delete_folder_cascades_to_nested_folders() {
        let (doc, _tmp) = setup_repo();
        let store = read_store(&doc);
        let root_id = store.root_folder_id;
        let parent = create_folder(&doc, &root_id, "Parent").unwrap();
        let child = create_folder(&doc, &parent, "Child").unwrap();
        let grandchild_bm = add_bookmark(&doc, &child, "https://deep.com", "Deep").unwrap();
        delete_folder(&doc, &parent).unwrap();
        let store = read_store(&doc);
        assert!(store.folders.get(&parent).unwrap().deleted);
        assert!(store.folders.get(&child).unwrap().deleted);
        assert!(store.bookmarks.get(&grandchild_bm).unwrap().deleted);
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn test_delete_folder_preserves_unrelated_bookmarks() {
        let (doc, _tmp) = setup_repo();
        let store = read_store(&doc);
        let root_id = store.root_folder_id;
        let folder_a = create_folder(&doc, &root_id, "A").unwrap();
        let folder_b = create_folder(&doc, &root_id, "B").unwrap();
        let bm_a = add_bookmark(&doc, &folder_a, "https://a.com", "A").unwrap();
        let bm_b = add_bookmark(&doc, &folder_b, "https://b.com", "B").unwrap();
        delete_folder(&doc, &folder_a).unwrap();
        let store = read_store(&doc);
        assert!(store.folders.get(&folder_a).unwrap().deleted);
        assert!(store.bookmarks.get(&bm_a).unwrap().deleted);
        assert!(!store.folders.get(&folder_b).unwrap().deleted);
        assert!(!store.bookmarks.get(&bm_b).unwrap().deleted);
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn test_update_favicon() {
        let (doc, _tmp) = setup_repo();
        let store = read_store(&doc);
        let root_id = store.root_folder_id;
        let id = add_bookmark(&doc, &root_id, "https://example.com", "Example").unwrap();
        let store = read_store(&doc);
        let original_updated = store.bookmarks.get(&id).unwrap().updated_at.clone();

        std::thread::sleep(std::time::Duration::from_millis(10));
        update_favicon(&doc, &id, "abc123.png").unwrap();

        let store = read_store(&doc);
        let bm = store.bookmarks.get(&id).unwrap();
        assert_eq!(bm.favicon, "abc123.png");
        assert_ne!(bm.updated_at, original_updated);
    }
}
