use mybriefcase_bookmarks::model::BookmarkStore;
use mybriefcase_bookmarks::schema::{
    BookmarkField, BookmarkStoreField, FolderField, StoreMetaField,
};
use schemars::schema_for;
use std::collections::BTreeSet;
use strum::IntoEnumIterator;

#[test]
fn committed_schema_matches_code() {
    let generated = schema_for!(BookmarkStore);
    let generated_json = serde_json::to_string_pretty(&generated).unwrap() + "\n";
    let committed = include_str!("../schema/bookmarks.schema.json");
    assert_eq!(
        committed, generated_json,
        "schema/bookmarks.schema.json is out of date — run `just generate-schema`"
    );
}

/// Bidirectional contract test: every schema constant maps to a model field,
/// and every model field has a corresponding schema constant.
#[test]
fn schema_constants_match_model_fields_bidirectionally() {
    let root_schema = schema_for!(BookmarkStore);
    let root_json = serde_json::to_value(&root_schema).unwrap();

    // Extract property names from each type in the JSON Schema
    let root_fields = extract_properties(&root_json, "BookmarkStore");
    let bookmark_fields = extract_definition_properties(&root_json, "Bookmark");
    let folder_fields = extract_definition_properties(&root_json, "Folder");
    let store_meta_fields = extract_definition_properties(&root_json, "StoreMeta");

    // Derive field sets from exhaustive enums — adding a variant without
    // a matching model field (or vice versa) will fail this test.
    let root_constants: BTreeSet<String> = BookmarkStoreField::iter()
        .map(|v| <&str>::from(v).to_owned())
        .collect();

    let bookmark_constants: BTreeSet<String> = BookmarkField::iter()
        .map(|v| <&str>::from(v).to_owned())
        .collect();

    let folder_constants: BTreeSet<String> = FolderField::iter()
        .map(|v| <&str>::from(v).to_owned())
        .collect();

    let store_meta_constants: BTreeSet<String> = StoreMetaField::iter()
        .map(|v| <&str>::from(v).to_owned())
        .collect();

    // Bidirectional checks
    assert_sets_equal("BookmarkStore", &root_constants, &root_fields);
    assert_sets_equal("Bookmark", &bookmark_constants, &bookmark_fields);
    assert_sets_equal("Folder", &folder_constants, &folder_fields);
    assert_sets_equal("StoreMeta", &store_meta_constants, &store_meta_fields);
}

fn extract_properties(schema_value: &serde_json::Value, type_name: &str) -> BTreeSet<String> {
    let props = schema_value
        .get("properties")
        .unwrap_or_else(|| panic!("{type_name}: missing 'properties'"));
    props
        .as_object()
        .unwrap_or_else(|| panic!("{type_name}: 'properties' is not an object"))
        .keys()
        .cloned()
        .collect()
}

fn extract_definition_properties(
    root_schema: &serde_json::Value,
    definition_name: &str,
) -> BTreeSet<String> {
    let definitions = root_schema
        .get("definitions")
        .unwrap_or_else(|| panic!("missing 'definitions' in root schema"));
    let def = definitions
        .get(definition_name)
        .unwrap_or_else(|| panic!("missing definition for '{definition_name}'"));
    let props = def
        .get("properties")
        .unwrap_or_else(|| panic!("{definition_name}: missing 'properties'"));
    props
        .as_object()
        .unwrap_or_else(|| panic!("{definition_name}: 'properties' is not an object"))
        .keys()
        .cloned()
        .collect()
}

fn assert_sets_equal(
    type_name: &str,
    constants: &BTreeSet<String>,
    model_fields: &BTreeSet<String>,
) {
    let missing_from_model: BTreeSet<_> = constants.difference(model_fields).collect();
    let missing_from_constants: BTreeSet<_> = model_fields.difference(constants).collect();

    assert!(
        missing_from_model.is_empty() && missing_from_constants.is_empty(),
        "{type_name} mismatch:\n  \
         Constants without model field: {missing_from_model:?}\n  \
         Model fields without constant: {missing_from_constants:?}"
    );
}
