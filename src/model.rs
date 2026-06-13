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
    pub children: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, Reconcile, Hydrate)]
pub struct Bookmark {
    pub url: String,
    pub title: String,
    pub notes: String,
    pub favicon: String,
    pub created_at: String,
    pub updated_at: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, Reconcile, Hydrate)]
pub struct StoreMeta {
    pub schema_version: u64,
    pub collection_name: String,
}
