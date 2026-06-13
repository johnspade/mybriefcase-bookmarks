use mybriefcase_bookmarks::model::BookmarkStore;
use schemars::schema_for;

#[test]
fn committed_schema_matches_code() {
    let generated = schema_for!(BookmarkStore);
    let generated_json = serde_json::to_string_pretty(&generated).unwrap() + "\n";
    let committed = std::fs::read_to_string("schema/bookmarks.schema.json")
        .expect("schema/bookmarks.schema.json not found — run `just generate-schema`");
    assert_eq!(
        committed, generated_json,
        "schema/bookmarks.schema.json is out of date — run `just generate-schema`"
    );
}
