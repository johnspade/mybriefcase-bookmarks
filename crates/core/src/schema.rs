use automerge::ObjId;
use automerge::transaction::Transactable;

// Top-level document (BookmarkStore) fields
pub const ROOT_FOLDER_ID: &str = "root_folder_id";
pub const FOLDERS: &str = "folders";
pub const BOOKMARKS: &str = "bookmarks";
pub const META: &str = "meta";

// Bookmark fields
pub const URL: &str = "url";
pub const TITLE: &str = "title";
pub const NOTES: &str = "notes";
pub const FAVICON: &str = "favicon";
pub const CREATED_AT: &str = "created_at";
pub const UPDATED_AT: &str = "updated_at";
pub const DELETED: &str = "deleted";

// Folder fields (TITLE, CREATED_AT, UPDATED_AT, DELETED shared with Bookmark)
pub const CHILDREN: &str = "children";

// StoreMeta fields
pub const SCHEMA_VERSION: &str = "schema_version";
pub const COLLECTION_NAME: &str = "collection_name";

pub struct BookmarkFields<'a> {
    pub url: &'a str,
    pub title: &'a str,
    pub notes: &'a str,
    pub favicon: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
}

/// # Errors
/// Returns an error if any automerge `put` operation fails.
pub fn write_bookmark(
    tx: &mut impl Transactable,
    obj: &ObjId,
    fields: &BookmarkFields<'_>,
) -> Result<(), automerge::AutomergeError> {
    tx.put(obj, URL, fields.url)?;
    tx.put(obj, TITLE, fields.title)?;
    tx.put(obj, NOTES, fields.notes)?;
    tx.put(obj, FAVICON, fields.favicon)?;
    tx.put(obj, CREATED_AT, fields.created_at)?;
    tx.put(obj, UPDATED_AT, fields.updated_at)?;
    tx.put(obj, DELETED, false)?;
    Ok(())
}

/// # Errors
/// Returns an error if any automerge `put` operation fails.
pub fn patch_bookmark(
    tx: &mut impl Transactable,
    obj: &ObjId,
    url: Option<&str>,
    title: Option<&str>,
    notes: Option<&str>,
    favicon: Option<&str>,
) -> Result<(), automerge::AutomergeError> {
    if let Some(v) = url {
        tx.put(obj, URL, v)?;
    }
    if let Some(v) = title {
        tx.put(obj, TITLE, v)?;
    }
    if let Some(v) = notes {
        tx.put(obj, NOTES, v)?;
    }
    if let Some(v) = favicon {
        tx.put(obj, FAVICON, v)?;
    }
    tx.put(obj, UPDATED_AT, chrono::Utc::now().to_rfc3339().as_str())?;
    Ok(())
}

/// # Errors
/// Returns an error if any automerge `put` operation fails.
pub fn write_folder(
    tx: &mut impl Transactable,
    obj: &ObjId,
    title: &str,
    created_at: &str,
    updated_at: &str,
) -> Result<ObjId, automerge::AutomergeError> {
    tx.put(obj, TITLE, title)?;
    let children = tx.put_object(obj, CHILDREN, automerge::ObjType::List)?;
    tx.put(obj, CREATED_AT, created_at)?;
    tx.put(obj, UPDATED_AT, updated_at)?;
    tx.put(obj, DELETED, false)?;
    Ok(children)
}

/// # Errors
/// Returns an error if any automerge `put` operation fails.
pub fn patch_folder(
    tx: &mut impl Transactable,
    obj: &ObjId,
    title: &str,
) -> Result<(), automerge::AutomergeError> {
    tx.put(obj, TITLE, title)?;
    tx.put(obj, UPDATED_AT, chrono::Utc::now().to_rfc3339().as_str())?;
    Ok(())
}
