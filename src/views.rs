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
    pub favicon: String,
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

#[must_use]
pub fn domain_color(url: &str) -> String {
    let colors = [
        "#e44", "#e84", "#4a9", "#46a", "#88a", "#a48", "#49a", "#a44",
    ];
    let mut hash: u32 = 0;
    for b in url.bytes() {
        hash = u32::from(b).wrapping_add(hash.wrapping_shl(5).wrapping_sub(hash));
    }
    colors[(hash as usize) % colors.len()].to_string()
}

#[must_use]
pub fn domain_letter(url: &str) -> String {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let host = rest.split('/').next().unwrap_or("");
    let domain = host.strip_prefix("www.").unwrap_or(host);
    domain
        .chars()
        .next()
        .unwrap_or('?')
        .to_uppercase()
        .to_string()
}

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
    let mut current = folder_id.to_string();
    while let Some(folder) = store.folders.get(&current) {
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
                id.to_string(),
                crate::model::Folder {
                    title: title.to_string(),
                    children: children.into_iter().map(String::from).collect(),
                    created_at: "2026-01-01T00:00:00Z".to_string(),
                    updated_at: "2026-01-01T00:00:00Z".to_string(),
                    deleted: false,
                },
            );
        }
        let mut bookmark_map = HashMap::new();
        for (id, title, url, created) in bookmarks {
            bookmark_map.insert(
                id.to_string(),
                crate::model::Bookmark {
                    url: url.to_string(),
                    title: title.to_string(),
                    notes: String::new(),
                    favicon: String::new(),
                    created_at: created.to_string(),
                    updated_at: created.to_string(),
                    deleted: false,
                },
            );
        }
        BookmarkStore {
            root_folder_id: root_id.to_string(),
            folders: folder_map,
            bookmarks: bookmark_map,
            meta: crate::model::StoreMeta {
                schema_version: 1,
                collection_name: "bookmarks".to_string(),
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
}
