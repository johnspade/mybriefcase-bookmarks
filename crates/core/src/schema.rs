use automerge::ObjId;
use automerge::transaction::Transactable;
use strum_macros::{AsRefStr, EnumIter, IntoStaticStr};

/// Top-level document (`BookmarkStore`) fields.
#[derive(Debug, Clone, Copy, EnumIter, IntoStaticStr, AsRefStr)]
pub enum BookmarkStoreField {
    #[strum(serialize = "root_folder_id")]
    RootFolderId,
    #[strum(serialize = "folders")]
    Folders,
    #[strum(serialize = "bookmarks")]
    Bookmarks,
    #[strum(serialize = "meta")]
    Meta,
}

/// `Bookmark` fields.
#[derive(Debug, Clone, Copy, EnumIter, IntoStaticStr, AsRefStr)]
pub enum BookmarkField {
    #[strum(serialize = "url")]
    Url,
    #[strum(serialize = "title")]
    Title,
    #[strum(serialize = "notes")]
    Notes,
    #[strum(serialize = "favicon")]
    Favicon,
    #[strum(serialize = "created_at")]
    CreatedAt,
    #[strum(serialize = "updated_at")]
    UpdatedAt,
    #[strum(serialize = "deleted")]
    Deleted,
}

/// `Folder` fields.
#[derive(Debug, Clone, Copy, EnumIter, IntoStaticStr, AsRefStr)]
pub enum FolderField {
    #[strum(serialize = "title")]
    Title,
    #[strum(serialize = "children")]
    Children,
    #[strum(serialize = "created_at")]
    CreatedAt,
    #[strum(serialize = "updated_at")]
    UpdatedAt,
    #[strum(serialize = "deleted")]
    Deleted,
}

/// `StoreMeta` fields.
#[derive(Debug, Clone, Copy, EnumIter, IntoStaticStr, AsRefStr)]
pub enum StoreMetaField {
    #[strum(serialize = "schema_version")]
    SchemaVersion,
    #[strum(serialize = "collection_name")]
    CollectionName,
}

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
    use BookmarkField::{CreatedAt, Deleted, Favicon, Notes, Title, UpdatedAt, Url};
    tx.put(obj, Url.as_ref(), fields.url)?;
    tx.put(obj, Title.as_ref(), fields.title)?;
    tx.put(obj, Notes.as_ref(), fields.notes)?;
    tx.put(obj, Favicon.as_ref(), fields.favicon)?;
    tx.put(obj, CreatedAt.as_ref(), fields.created_at)?;
    tx.put(obj, UpdatedAt.as_ref(), fields.updated_at)?;
    tx.put(obj, Deleted.as_ref(), false)?;
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
    use BookmarkField::{Favicon, Notes, Title, UpdatedAt, Url};
    if let Some(v) = url {
        tx.put(obj, Url.as_ref(), v)?;
    }
    if let Some(v) = title {
        tx.put(obj, Title.as_ref(), v)?;
    }
    if let Some(v) = notes {
        tx.put(obj, Notes.as_ref(), v)?;
    }
    if let Some(v) = favicon {
        tx.put(obj, Favicon.as_ref(), v)?;
    }
    tx.put(
        obj,
        UpdatedAt.as_ref(),
        chrono::Utc::now().to_rfc3339().as_str(),
    )?;
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
    use FolderField::{Children, CreatedAt, Deleted, Title, UpdatedAt};
    tx.put(obj, Title.as_ref(), title)?;
    let children = tx.put_object(obj, Children.as_ref(), automerge::ObjType::List)?;
    tx.put(obj, CreatedAt.as_ref(), created_at)?;
    tx.put(obj, UpdatedAt.as_ref(), updated_at)?;
    tx.put(obj, Deleted.as_ref(), false)?;
    Ok(children)
}

/// # Errors
/// Returns an error if any automerge `put` operation fails.
pub fn patch_folder(
    tx: &mut impl Transactable,
    obj: &ObjId,
    title: &str,
) -> Result<(), automerge::AutomergeError> {
    use FolderField::{Title, UpdatedAt};
    tx.put(obj, Title.as_ref(), title)?;
    tx.put(
        obj,
        UpdatedAt.as_ref(),
        chrono::Utc::now().to_rfc3339().as_str(),
    )?;
    Ok(())
}
