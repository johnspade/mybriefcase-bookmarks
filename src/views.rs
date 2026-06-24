use std::fmt::Write as FmtWrite;

use crate::model::{BookmarkStore, Folder};

// ─── View data ──────────────────────────────────────

pub struct BreadcrumbItem {
    pub id: String,
    pub title: String,
    pub is_last: bool,
}

pub struct FolderItemView {
    pub id: String,
    pub title: String,
    pub updated_at: String,
    pub item_count: usize,
    pub bookmark_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    #[default]
    NameAsc,
    NameDesc,
    DateDesc,
    DateAsc,
}

impl SortOrder {
    #[must_use]
    pub fn from_param(s: Option<&str>) -> Self {
        match s {
            Some("name_desc") => Self::NameDesc,
            Some("date_desc") => Self::DateDesc,
            Some("date_asc") => Self::DateAsc,
            _ => Self::NameAsc,
        }
    }
}

pub struct BookmarkItemView {
    pub id: String,
    pub title: String,
    pub url: String,
    pub notes: String,
    pub created_at: String,
    pub created_date: String,
    pub favicon: Option<String>,
    pub domain_color: String,
    pub domain_letter: String,
}

// ─── Helpers ────────────────────────────────────────

#[must_use]
pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[must_use]
pub fn date_short(iso: &str) -> String {
    iso.chars().take(10).collect()
}

pub use mybriefcase_bookmarks_core::avatar::{domain_color, domain_letter};

#[must_use]
pub fn count_bookmarks_recursive(store: &BookmarkStore, folder: &Folder) -> usize {
    let mut count = 0;
    for child_id in &folder.children {
        if let Some(bm) = store.bookmarks.get(child_id) {
            if !bm.deleted {
                count += 1;
            }
        } else if let Some(sub) = store.folders.get(child_id) {
            if !sub.deleted {
                count += count_bookmarks_recursive(store, sub);
            }
        }
    }
    count
}

#[must_use]
pub fn build_breadcrumbs(store: &BookmarkStore, folder_id: &str) -> Vec<BreadcrumbItem> {
    let mut path = Vec::new();
    let mut current = folder_id.to_owned();
    let max_depth = store.folders.len();
    for _ in 0..max_depth {
        let Some(folder) = store.folders.get(&current) else {
            break;
        };
        path.push((current.clone(), folder.title.clone()));
        if current == store.root_folder_id {
            break;
        }
        match find_parent_folder_id(store, &current) {
            Some(pid) => current = pid,
            None => break,
        }
    }
    path.reverse();
    let len = path.len();
    path.into_iter()
        .enumerate()
        .map(|(i, (id, title))| BreadcrumbItem {
            id,
            title,
            is_last: i == len - 1,
        })
        .collect()
}

fn find_parent_folder_id(store: &BookmarkStore, child_id: &str) -> Option<String> {
    for (folder_id, folder) in &store.folders {
        if !folder.deleted && folder.children.iter().any(|c| c == child_id) {
            return Some(folder_id.clone());
        }
    }
    None
}

fn is_folder_ancestor(store: &BookmarkStore, ancestor_id: &str, target_id: &str) -> bool {
    if ancestor_id == target_id {
        return false;
    }
    if let Some(folder) = store.folders.get(ancestor_id) {
        for child_id in &folder.children {
            if child_id == target_id {
                return true;
            }
            if store.folders.contains_key(child_id)
                && is_folder_ancestor(store, child_id, target_id)
            {
                return true;
            }
        }
    }
    false
}

#[must_use]
pub fn build_sidebar_html(store: &BookmarkStore, current_folder_id: &str) -> String {
    let Some(root) = store.folders.get(&store.root_folder_id) else {
        return String::new();
    };
    let mut html = String::new();
    for child_id in &root.children {
        if let Some(folder) = store.folders.get(child_id) {
            if !folder.deleted {
                build_sidebar_folder(store, child_id, folder, current_folder_id, 0, &mut html);
            }
        }
    }
    html
}

fn build_sidebar_folder(
    store: &BookmarkStore,
    folder_id: &str,
    folder: &Folder,
    current_folder_id: &str,
    depth: usize,
    html: &mut String,
) {
    let is_selected = folder_id == current_folder_id;
    let is_ancestor = is_folder_ancestor(store, folder_id, current_folder_id);
    let is_open = is_selected || is_ancestor;

    let child_folder_ids: Vec<&String> = folder
        .children
        .iter()
        .filter(|id| store.folders.get(*id).is_some_and(|f| !f.deleted))
        .collect();
    let has_sub = !child_folder_ids.is_empty();
    let bm_count = count_bookmarks_recursive(store, folder);
    let selected_cls = if is_selected { " selected" } else { "" };
    let padding = 12 + depth * 18;

    let _ = write!(
        html,
        r##"<div class="tree-item{selected_cls}" style="padding-left:{padding}px" hx-get="/folders/{folder_id}" hx-target="#folder-content" hx-swap="innerHTML" hx-push-url="/folders/{folder_id}" data-folder-id="{folder_id}">"##,
    );

    if has_sub {
        let open_cls = if is_open { " open" } else { "" };
        let _ = write!(
            html,
            r#"<span class="chevron{open_cls}" onclick="event.stopPropagation();toggleChevron(this)"><svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><polyline points="9 18 15 12 9 6"/></svg></span>"#,
        );
    } else {
        html.push_str(r#"<span style="width:14px;flex-shrink:0"></span>"#);
    }

    html.push_str(r#"<span class="item-icon"><svg width="14" height="14" viewBox="0 0 24 24" fill="var(--folder-color)" stroke="none"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg></span>"#);
    let _ = write!(
        html,
        r#"<span class="item-label">{}</span>"#,
        html_escape(&folder.title)
    );
    if bm_count > 0 {
        let _ = write!(html, r#"<span class="item-count">{bm_count}</span>"#);
    }
    html.push_str("</div>");

    if has_sub {
        let open_cls = if is_open { " open" } else { "" };
        let _ = write!(html, r#"<div class="tree-children{open_cls}">"#);
        for child_id in &child_folder_ids {
            if let Some(child) = store.folders.get(*child_id) {
                build_sidebar_folder(store, child_id, child, current_folder_id, depth + 1, html);
            }
        }
        html.push_str("</div>");
    }
}

#[must_use]
pub fn build_folder_items(
    store: &BookmarkStore,
    folder_id: &str,
    sort: SortOrder,
) -> (Vec<FolderItemView>, Vec<BookmarkItemView>) {
    let Some(folder) = store.folders.get(folder_id) else {
        return (vec![], vec![]);
    };

    let mut folders = Vec::new();
    let mut bookmarks = Vec::new();

    for child_id in &folder.children {
        if let Some(sub) = store.folders.get(child_id) {
            if !sub.deleted {
                let item_count = sub
                    .children
                    .iter()
                    .filter(|id| {
                        store.folders.get(*id).is_some_and(|f| !f.deleted)
                            || store.bookmarks.get(*id).is_some_and(|b| !b.deleted)
                    })
                    .count();
                folders.push(FolderItemView {
                    id: child_id.clone(),
                    title: sub.title.clone(),
                    updated_at: sub.updated_at.clone(),
                    item_count,
                    bookmark_count: count_bookmarks_recursive(store, sub),
                });
            }
        } else if let Some(bm) = store.bookmarks.get(child_id) {
            if !bm.deleted {
                bookmarks.push(BookmarkItemView {
                    id: child_id.clone(),
                    title: bm.title.clone(),
                    url: bm.url.clone(),
                    notes: bm.notes.clone(),
                    created_at: bm.created_at.clone(),
                    created_date: date_short(&bm.created_at),
                    favicon: bm.favicon.clone(),
                    domain_color: domain_color(&bm.url),
                    domain_letter: domain_letter(&bm.url),
                });
            }
        }
    }

    sort_items(&mut folders, &mut bookmarks, sort);

    (folders, bookmarks)
}

pub fn sort_items(
    folders: &mut [FolderItemView],
    bookmarks: &mut [BookmarkItemView],
    sort: SortOrder,
) {
    use std::cmp::Reverse;
    match sort {
        SortOrder::NameAsc => {
            folders.sort_by_key(|f| f.title.to_lowercase());
            bookmarks.sort_by_key(|b| b.title.to_lowercase());
        }
        SortOrder::NameDesc => {
            folders.sort_by_key(|f| Reverse(f.title.to_lowercase()));
            bookmarks.sort_by_key(|b| Reverse(b.title.to_lowercase()));
        }
        SortOrder::DateDesc => {
            folders.sort_by_key(|f| Reverse(f.updated_at.clone()));
            bookmarks.sort_by_key(|b| Reverse(b.created_at.clone()));
        }
        SortOrder::DateAsc => {
            folders.sort_by_key(|f| f.updated_at.clone());
            bookmarks.sort_by_key(|b| b.created_at.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_store(
        root_id: &str,
        folders: Vec<(&str, &str, Vec<&str>)>,
        bookmarks: Vec<(&str, &str, &str, &str)>,
    ) -> BookmarkStore {
        let mut folder_map = HashMap::new();
        for (id, title, children) in folders {
            folder_map.insert(
                id.to_owned(),
                crate::model::Folder {
                    title: title.to_owned(),
                    children: children.into_iter().map(String::from).collect(),
                    created_at: "2026-01-01T00:00:00Z".to_owned(),
                    updated_at: "2026-01-01T00:00:00Z".to_owned(),
                    deleted: false,
                },
            );
        }
        let mut bookmark_map = HashMap::new();
        for (id, title, url, created) in bookmarks {
            bookmark_map.insert(
                id.to_owned(),
                crate::model::Bookmark {
                    url: url.to_owned(),
                    title: title.to_owned(),
                    notes: String::new(),
                    favicon: None,
                    created_at: created.to_owned(),
                    updated_at: created.to_owned(),
                    deleted: false,
                },
            );
        }
        BookmarkStore {
            root_folder_id: root_id.to_owned(),
            folders: folder_map,
            bookmarks: bookmark_map,
            meta: crate::model::StoreMeta {
                schema_version: 1,
                collection_name: "bookmarks".to_owned(),
            },
        }
    }

    #[test]
    fn date_short_extracts_date_portion() {
        assert_eq!(date_short("2026-06-11T02:57:00+00:00"), "2026-06-11");
    }

    #[test]
    fn date_short_handles_short_input() {
        assert_eq!(date_short("2026-06"), "2026-06");
    }

    #[test]
    fn domain_color_is_deterministic() {
        let c1 = domain_color("https://example.com");
        let c2 = domain_color("https://example.com");
        assert_eq!(c1, c2);
    }

    #[test]
    fn domain_color_differs_for_different_urls() {
        let c1 = domain_color("https://example.com");
        let c2 = domain_color("https://other.org");
        assert_ne!(c1, c2);
    }

    #[test]
    fn domain_letter_extracts_first_char() {
        assert_eq!(domain_letter("https://example.com/page"), "E");
        assert_eq!(domain_letter("https://www.github.com"), "G");
        assert_eq!(domain_letter("http://rust-lang.org"), "R");
    }

    #[test]
    fn domain_letter_handles_empty_url() {
        assert_eq!(domain_letter(""), "?");
    }

    #[test]
    fn build_breadcrumbs_single_root() {
        let store = make_store("root", vec![("root", "Bookmarks", vec![])], vec![]);
        let crumbs = build_breadcrumbs(&store, "root");
        assert_eq!(crumbs.len(), 1);
        assert_eq!(crumbs[0].title, "Bookmarks");
        assert!(crumbs[0].is_last);
    }

    #[test]
    fn build_breadcrumbs_nested_path() {
        let store = make_store(
            "root",
            vec![
                ("root", "Bookmarks", vec!["work"]),
                ("work", "Work", vec!["rust"]),
                ("rust", "Rust", vec![]),
            ],
            vec![],
        );
        let crumbs = build_breadcrumbs(&store, "rust");
        assert_eq!(crumbs.len(), 3);
        assert_eq!(crumbs[0].title, "Bookmarks");
        assert_eq!(crumbs[1].title, "Work");
        assert_eq!(crumbs[2].title, "Rust");
        assert!(!crumbs[0].is_last);
        assert!(crumbs[2].is_last);
    }

    #[test]
    fn build_folder_items_sorts_by_name_asc() {
        let store = make_store(
            "root",
            vec![("root", "Bookmarks", vec!["bm-z", "bm-a", "bm-m"])],
            vec![
                ("bm-z", "Zebra", "https://z.com", "2026-01-03"),
                ("bm-a", "Apple", "https://a.com", "2026-01-01"),
                ("bm-m", "Mango", "https://m.com", "2026-01-02"),
            ],
        );
        let (_, bookmarks) = build_folder_items(&store, "root", SortOrder::NameAsc);
        let titles: Vec<&str> = bookmarks.iter().map(|b| b.title.as_str()).collect();
        assert_eq!(titles, vec!["Apple", "Mango", "Zebra"]);
    }

    #[test]
    fn build_folder_items_sorts_by_name_desc() {
        let store = make_store(
            "root",
            vec![("root", "Bookmarks", vec!["bm-z", "bm-a", "bm-m"])],
            vec![
                ("bm-z", "Zebra", "https://z.com", "2026-01-03"),
                ("bm-a", "Apple", "https://a.com", "2026-01-01"),
                ("bm-m", "Mango", "https://m.com", "2026-01-02"),
            ],
        );
        let (_, bookmarks) = build_folder_items(&store, "root", SortOrder::NameDesc);
        let titles: Vec<&str> = bookmarks.iter().map(|b| b.title.as_str()).collect();
        assert_eq!(titles, vec!["Zebra", "Mango", "Apple"]);
    }

    #[test]
    fn build_folder_items_sorts_by_date_desc() {
        let store = make_store(
            "root",
            vec![("root", "Bookmarks", vec!["bm-z", "bm-a", "bm-m"])],
            vec![
                ("bm-z", "Zebra", "https://z.com", "2026-01-03"),
                ("bm-a", "Apple", "https://a.com", "2026-01-01"),
                ("bm-m", "Mango", "https://m.com", "2026-01-02"),
            ],
        );
        let (_, bookmarks) = build_folder_items(&store, "root", SortOrder::DateDesc);
        let titles: Vec<&str> = bookmarks.iter().map(|b| b.title.as_str()).collect();
        assert_eq!(titles, vec!["Zebra", "Mango", "Apple"]);
    }

    #[test]
    fn build_folder_items_sorts_by_date_asc() {
        let store = make_store(
            "root",
            vec![("root", "Bookmarks", vec!["bm-z", "bm-a", "bm-m"])],
            vec![
                ("bm-z", "Zebra", "https://z.com", "2026-01-03"),
                ("bm-a", "Apple", "https://a.com", "2026-01-01"),
                ("bm-m", "Mango", "https://m.com", "2026-01-02"),
            ],
        );
        let (_, bookmarks) = build_folder_items(&store, "root", SortOrder::DateAsc);
        let titles: Vec<&str> = bookmarks.iter().map(|b| b.title.as_str()).collect();
        assert_eq!(titles, vec!["Apple", "Mango", "Zebra"]);
    }

    #[test]
    fn build_folder_items_filters_deleted() {
        let mut store = make_store(
            "root",
            vec![("root", "Bookmarks", vec!["bm-a", "bm-b"])],
            vec![
                ("bm-a", "Visible", "https://a.com", "2026-01-01"),
                ("bm-b", "Deleted", "https://b.com", "2026-01-02"),
            ],
        );
        store.bookmarks.get_mut("bm-b").unwrap().deleted = true;
        let (_, bookmarks) = build_folder_items(&store, "root", SortOrder::NameAsc);
        assert_eq!(bookmarks.len(), 1);
        assert_eq!(bookmarks[0].title, "Visible");
    }

    #[test]
    fn build_folder_items_nonexistent_folder_returns_empty() {
        let store = make_store("root", vec![("root", "Bookmarks", vec![])], vec![]);
        let (folders, bookmarks) = build_folder_items(&store, "nonexistent", SortOrder::NameAsc);
        assert!(folders.is_empty());
        assert!(bookmarks.is_empty());
    }

    #[test]
    fn count_bookmarks_recursive_counts_nested() {
        let store = make_store(
            "root",
            vec![
                ("root", "Bookmarks", vec!["sub", "bm-1"]),
                ("sub", "Sub", vec!["bm-2", "bm-3"]),
            ],
            vec![
                ("bm-1", "One", "https://1.com", "2026-01-01"),
                ("bm-2", "Two", "https://2.com", "2026-01-01"),
                ("bm-3", "Three", "https://3.com", "2026-01-01"),
            ],
        );
        let root = store.folders.get("root").unwrap();
        assert_eq!(count_bookmarks_recursive(&store, root), 3);
    }

    #[test]
    fn html_escape_handles_all_special_chars() {
        assert_eq!(
            html_escape(r#"<a href="x">&</a>"#),
            "&lt;a href=&quot;x&quot;&gt;&amp;&lt;/a&gt;"
        );
    }

    #[test]
    fn is_folder_ancestor_same_id_is_false() {
        let store = make_store("root", vec![("root", "Root", vec![])], vec![]);
        assert!(!is_folder_ancestor(&store, "root", "root"));
    }

    #[test]
    fn is_folder_ancestor_direct_child() {
        let store = make_store(
            "root",
            vec![("root", "Root", vec!["child"]), ("child", "Child", vec![])],
            vec![],
        );
        assert!(is_folder_ancestor(&store, "root", "child"));
    }

    #[test]
    fn is_folder_ancestor_grandchild() {
        let store = make_store(
            "root",
            vec![
                ("root", "Root", vec!["mid"]),
                ("mid", "Mid", vec!["leaf"]),
                ("leaf", "Leaf", vec![]),
            ],
            vec![],
        );
        assert!(is_folder_ancestor(&store, "root", "leaf"));
    }

    #[test]
    fn is_folder_ancestor_not_ancestor() {
        let store = make_store(
            "root",
            vec![
                ("root", "Root", vec!["a", "b"]),
                ("a", "A", vec![]),
                ("b", "B", vec![]),
            ],
            vec![],
        );
        assert!(!is_folder_ancestor(&store, "a", "b"));
    }

    #[test]
    fn is_folder_ancestor_nonexistent_ancestor() {
        let store = make_store("root", vec![("root", "Root", vec![])], vec![]);
        assert!(!is_folder_ancestor(&store, "missing", "root"));
    }

    #[test]
    fn find_parent_folder_id_skips_deleted_folders() {
        let mut store = make_store(
            "root",
            vec![
                ("root", "Root", vec!["parent", "child"]),
                ("parent", "Parent", vec!["child"]),
                ("child", "Child", vec![]),
            ],
            vec![],
        );
        // Mark the parent as deleted — it should be skipped
        store.folders.get_mut("parent").unwrap().deleted = true;
        // Root also has "child" in its children, so root should be found
        let result = find_parent_folder_id(&store, "child");
        assert_eq!(result, Some("root".to_owned()));
    }

    #[test]
    fn build_sidebar_html_nonempty_for_store_with_child_folders() {
        let store = make_store(
            "root",
            vec![("root", "Root", vec!["child"]), ("child", "Work", vec![])],
            vec![],
        );
        let html = build_sidebar_html(&store, "root");
        assert!(!html.is_empty());
        assert!(html.contains("Work"));
    }

    #[test]
    fn build_sidebar_folder_count_badge_absent_when_zero_bookmarks() {
        let store = make_store(
            "root",
            vec![
                ("root", "Root", vec!["empty"]),
                ("empty", "Empty Folder", vec![]),
            ],
            vec![],
        );
        let html = build_sidebar_html(&store, "root");
        // The "item-count" badge should not appear for a folder with 0 bookmarks
        assert!(!html.contains("item-count"));
    }

    #[test]
    fn build_sidebar_folder_padding_math() {
        let store = make_store(
            "root",
            vec![
                ("root", "Root", vec!["level0"]),
                ("level0", "L0", vec!["level1"]),
                ("level1", "L1", vec![]),
            ],
            vec![],
        );
        // Open ancestor so nested folders render
        let html = build_sidebar_html(&store, "level1");
        // depth 0: padding-left:12px
        assert!(html.contains("padding-left:12px"));
        // depth 1: padding-left:30px (12 + 1*18)
        assert!(html.contains("padding-left:30px"));
    }

    #[test]
    fn build_folder_items_item_count_excludes_deleted_children() {
        let mut store = make_store(
            "root",
            vec![
                ("root", "Root", vec!["parent"]),
                (
                    "parent",
                    "Parent",
                    vec!["alive_folder", "dead_folder", "bm-alive", "bm-dead"],
                ),
                ("alive_folder", "Alive", vec![]),
                ("dead_folder", "Dead", vec![]),
            ],
            vec![
                ("bm-alive", "Alive BM", "https://a.com", "2026-01-01"),
                ("bm-dead", "Dead BM", "https://b.com", "2026-01-01"),
            ],
        );
        store.folders.get_mut("dead_folder").unwrap().deleted = true;
        store.bookmarks.get_mut("bm-dead").unwrap().deleted = true;

        let (folders, _) = build_folder_items(&store, "root", SortOrder::NameAsc);
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].title, "Parent");
        // item_count should only include alive_folder + bm-alive = 2
        assert_eq!(folders[0].item_count, 2);
    }

    #[test]
    fn is_folder_ancestor_and_conjunction_exists_and_recurses() {
        // The && requires BOTH that child_id exists in store.folders AND that
        // recursion returns true. If && is mutated to ||, then a child existing
        // in the store (but not containing the target) would incorrectly return true.
        //
        // Setup: root -> [mid], mid -> [] (no children), target "other" exists
        // separately in the store but is NOT reachable from root.
        // With &&: contains_key("mid") is true, but is_folder_ancestor("mid", "other")
        //          is false => overall false (correct).
        // With ||: contains_key("mid") is true => would return true (incorrect).
        let store = make_store(
            "root",
            vec![
                ("root", "Root", vec!["mid"]),
                ("mid", "Mid", vec![]),
                ("other", "Other", vec![]),
            ],
            vec![],
        );
        assert!(!is_folder_ancestor(&store, "root", "other"));
    }

    #[test]
    fn build_sidebar_folder_selected_class_only_on_current() {
        let store = make_store(
            "root",
            vec![
                ("root", "Root", vec!["a", "b"]),
                ("a", "FolderA", vec![]),
                ("b", "FolderB", vec![]),
            ],
            vec![],
        );
        let html = build_sidebar_html(&store, "a");
        let selected_div = html
            .split("<div")
            .find(|s| s.contains(r#"data-folder-id="a""#))
            .expect("folder a should be in sidebar");
        assert!(
            selected_div.contains("selected"),
            "folder 'a' should be selected"
        );
        let unselected_div = html
            .split("<div")
            .find(|s| s.contains(r#"data-folder-id="b""#))
            .expect("folder b should be in sidebar");
        assert!(
            !unselected_div.contains("selected"),
            "folder 'b' should not be selected"
        );
    }

    #[test]
    fn build_sidebar_folder_ancestor_opens_tree() {
        let store = make_store(
            "root",
            vec![
                ("root", "Root", vec!["parent"]),
                ("parent", "Parent", vec!["child"]),
                ("child", "Child", vec![]),
            ],
            vec![],
        );
        let html = build_sidebar_html(&store, "child");
        let open_count = html.matches("tree-children open").count();
        assert!(
            open_count >= 1,
            "ancestor folder should have open tree-children"
        );
    }

    #[test]
    fn build_sidebar_folder_shows_count_badge_when_bookmarks_exist() {
        let store = make_store(
            "root",
            vec![
                ("root", "Root", vec!["folder"]),
                ("folder", "HasBookmarks", vec!["bm1", "bm2", "bm3"]),
            ],
            vec![
                ("bm1", "A", "https://a.com", "2026-01-01"),
                ("bm2", "B", "https://b.com", "2026-01-01"),
                ("bm3", "C", "https://c.com", "2026-01-01"),
            ],
        );
        let html = build_sidebar_html(&store, "root");
        assert!(
            html.contains(r#"class="item-count">3<"#),
            "folder with 3 bookmarks should show count badge '3': {html}"
        );
    }

    #[test]
    fn build_folder_items_item_count_distinguishes_alive_from_dead() {
        let mut store = make_store(
            "root",
            vec![
                ("root", "Root", vec!["parent"]),
                ("parent", "Parent", vec!["alive1", "alive2", "dead1"]),
                ("alive1", "Alive1", vec![]),
                ("alive2", "Alive2", vec![]),
                ("dead1", "Dead1", vec![]),
            ],
            vec![],
        );
        store.folders.get_mut("dead1").unwrap().deleted = true;

        let (folders, _) = build_folder_items(&store, "root", SortOrder::NameAsc);
        assert_eq!(folders.len(), 1);
        // 2 alive children, not 1 dead child
        assert_eq!(
            folders[0].item_count, 2,
            "item_count must count only non-deleted children"
        );
    }

    #[test]
    fn build_folder_items_item_count_distinguishes_alive_bookmarks_from_dead() {
        let mut store = make_store(
            "root",
            vec![
                ("root", "Root", vec!["parent"]),
                ("parent", "Parent", vec!["bm-a", "bm-b", "bm-dead"]),
            ],
            vec![
                ("bm-a", "A", "https://a.com", "2026-01-01"),
                ("bm-b", "B", "https://b.com", "2026-01-01"),
                ("bm-dead", "Dead", "https://d.com", "2026-01-01"),
            ],
        );
        store.bookmarks.get_mut("bm-dead").unwrap().deleted = true;

        let (folders, _) = build_folder_items(&store, "root", SortOrder::NameAsc);
        assert_eq!(folders.len(), 1);
        assert_eq!(
            folders[0].item_count, 2,
            "item_count must count only non-deleted bookmark children"
        );
    }
}
