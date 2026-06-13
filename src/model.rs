use autosurgeon::{Hydrate, Reconcile};
use schemars::JsonSchema;
use std::collections::HashMap;

/// Top-level document stored as an Automerge CRDT binary.
/// Open with any Automerge library; the root object is a Map with this shape.
#[derive(Debug, Clone, Reconcile, Hydrate, JsonSchema)]
pub struct BookmarkStore {
    /// UUID identifying the root folder (key into `folders` map)
    #[schemars(schema_with = "uuid_schema")]
    pub root_folder_id: String,
    /// Map from folder UUID to folder object (Automerge Map — per-key conflict resolution)
    pub folders: HashMap<String, Folder>,
    /// Map from bookmark UUID to bookmark object (Automerge Map — per-key conflict resolution)
    pub bookmarks: HashMap<String, Bookmark>,
    pub meta: StoreMeta,
}

#[derive(Debug, Clone, Reconcile, Hydrate, JsonSchema)]
pub struct Folder {
    pub title: String,
    /// Ordered list of child IDs referencing keys in `folders` or `bookmarks`
    /// (Automerge List — concurrent inserts interleave rather than conflict)
    pub children: Vec<String>,
    /// RFC 3339 timestamp
    #[schemars(schema_with = "datetime_schema")]
    pub created_at: String,
    /// RFC 3339 timestamp
    #[schemars(schema_with = "datetime_schema")]
    pub updated_at: String,
    /// Soft-delete flag
    pub deleted: bool,
}

#[derive(Debug, Clone, Reconcile, Hydrate, JsonSchema)]
pub struct Bookmark {
    #[schemars(url)]
    pub url: String,
    pub title: String,
    pub notes: String,
    pub favicon: String,
    /// RFC 3339 timestamp
    #[schemars(schema_with = "datetime_schema")]
    pub created_at: String,
    /// RFC 3339 timestamp
    #[schemars(schema_with = "datetime_schema")]
    pub updated_at: String,
    /// Soft-delete flag
    pub deleted: bool,
}

#[derive(Debug, Clone, Reconcile, Hydrate, JsonSchema)]
pub struct StoreMeta {
    pub schema_version: u64,
    pub collection_name: String,
}

fn datetime_schema(_: &mut schemars::r#gen::SchemaGenerator) -> schemars::schema::Schema {
    schemars::schema::SchemaObject {
        instance_type: Some(schemars::schema::InstanceType::String.into()),
        format: Some("date-time".to_owned()),
        ..Default::default()
    }
    .into()
}

fn uuid_schema(_: &mut schemars::r#gen::SchemaGenerator) -> schemars::schema::Schema {
    schemars::schema::SchemaObject {
        instance_type: Some(schemars::schema::InstanceType::String.into()),
        format: Some("uuid".to_owned()),
        ..Default::default()
    }
    .into()
}
