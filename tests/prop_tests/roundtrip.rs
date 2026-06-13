use autosurgeon::hydrate;
use mybriefcase_bookmarks::export::export_netscape_html;
use mybriefcase_bookmarks::import::{ImportedItem, parse_netscape_html};
use mybriefcase_bookmarks::model::BookmarkStore;
use mybriefcase_bookmarks::ops;
use proptest::prelude::*;

use crate::common::new_initialized_doc;
use crate::strategies::arb_imported_tree;

fn hydrate_store(doc: &automerge_repo::DocHandle) -> BookmarkStore {
    doc.with_doc(|d| hydrate(d).unwrap())
}

fn count_items(items: &[ImportedItem]) -> (usize, usize) {
    let mut bookmarks = 0;
    let mut folders = 0;
    for item in items {
        match item {
            ImportedItem::Bookmark { .. } => bookmarks += 1,
            ImportedItem::Folder { children, .. } => {
                folders += 1;
                let (b, f) = count_items(children);
                bookmarks += b;
                folders += f;
            }
        }
    }
    (bookmarks, folders)
}

fn collect_titles_and_urls(items: &[ImportedItem]) -> (Vec<String>, Vec<String>) {
    let mut titles = Vec::new();
    let mut urls = Vec::new();
    collect_recursive(items, &mut titles, &mut urls);
    titles.sort();
    urls.sort();
    (titles, urls)
}

fn collect_recursive(items: &[ImportedItem], titles: &mut Vec<String>, urls: &mut Vec<String>) {
    for item in items {
        match item {
            ImportedItem::Bookmark { url, title, .. } => {
                titles.push(title.clone());
                urls.push(url.clone());
            }
            ImportedItem::Folder {
                title, children, ..
            } => {
                titles.push(title.clone());
                collect_recursive(children, titles, urls);
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, max_shrink_iters: 2048, .. ProptestConfig::default() })]

    #[test]
    #[cfg_attr(miri, ignore)]
    fn import_export_preserves_structure(tree in arb_imported_tree()) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();

        let doc = new_initialized_doc("prop-rt");
        let root_id = doc.root_folder_id.clone();

        let (expected_bm, expected_f) = count_items(&tree);
        let (bm_count, f_count) = ops::import_items(&doc.doc_handle, &root_id, &tree).unwrap();

        prop_assert_eq!(bm_count, expected_bm);
        prop_assert_eq!(f_count, expected_f);

        // Export and re-parse
        let store = hydrate_store(&doc.doc_handle);
        let mut buf = Vec::new();
        export_netscape_html(&store, &mut buf).unwrap();
        let html = String::from_utf8(buf).unwrap();
        let reimported = parse_netscape_html(&html);

        let (reimported_bm, reimported_f) = count_items(&reimported);
        // Exported store includes the root folder's children, so counts should match
        // (root folder itself is not in the export as a folder entry — it's the top-level DL)
        prop_assert_eq!(reimported_bm, expected_bm);
        prop_assert_eq!(reimported_f, expected_f);

        // Verify content integrity: titles and URLs must survive the round-trip
        let (orig_titles, orig_urls) = collect_titles_and_urls(&tree);
        let (rt_titles, rt_urls) = collect_titles_and_urls(&reimported);
        prop_assert_eq!(orig_urls, rt_urls, "URLs differ after round-trip");
        prop_assert_eq!(orig_titles, rt_titles, "titles differ after round-trip");
    }
}
