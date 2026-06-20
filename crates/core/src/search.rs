use crate::model::{Bookmark, BookmarkStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    NameAsc,
    NameDesc,
    DateDesc,
    DateAsc,
    #[default]
    Relevance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub id: String,
    pub title: String,
    pub url: String,
    pub notes: String,
    pub favicon: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[must_use]
pub fn search_bookmarks(
    bookmark_store: &BookmarkStore,
    query: &str,
    sort: SortOrder,
) -> Vec<SearchHit> {
    let q = query.to_lowercase();
    let mut hits: Vec<(SearchHit, u32)> = bookmark_store
        .bookmarks
        .iter()
        .filter(|(_, bm)| !bm.deleted)
        .filter_map(|(id, bm)| {
            let score = relevance_score(bm, &q);
            if score == 0 {
                return None;
            }
            Some((
                SearchHit {
                    id: id.clone(),
                    title: bm.title.clone(),
                    url: bm.url.clone(),
                    notes: bm.notes.clone(),
                    favicon: bm.favicon.clone(),
                    created_at: bm.created_at.clone(),
                    updated_at: bm.updated_at.clone(),
                },
                score,
            ))
        })
        .collect();

    match sort {
        SortOrder::Relevance => {
            hits.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.id.cmp(&b.0.id)));
        }
        SortOrder::NameAsc => hits.sort_by_key(|h| h.0.title.to_lowercase()),
        SortOrder::NameDesc => {
            hits.sort_by_key(|h| std::cmp::Reverse(h.0.title.to_lowercase()));
        }
        SortOrder::DateDesc => {
            hits.sort_by_key(|h| std::cmp::Reverse(h.0.created_at.clone()));
        }
        SortOrder::DateAsc => hits.sort_by_key(|h| h.0.created_at.clone()),
    }

    hits.into_iter().map(|(hit, _)| hit).collect()
}

const TITLE_WEIGHT: u32 = 30;
const URL_WEIGHT: u32 = 20;
const NOTES_WEIGHT: u32 = 10;
const PREFIX_BONUS: u32 = 5;

fn relevance_score(bm: &Bookmark, query: &str) -> u32 {
    let title = bm.title.to_lowercase();
    let url = bm.url.to_lowercase();
    let notes = bm.notes.to_lowercase();

    let mut score = 0u32;
    if title.contains(query) {
        score += TITLE_WEIGHT;
        if title.starts_with(query) {
            score += PREFIX_BONUS;
        }
    }
    if url.contains(query) {
        score += URL_WEIGHT;
        if url.starts_with(query) {
            score += PREFIX_BONUS;
        }
    }
    if notes.contains(query) {
        score += NOTES_WEIGHT;
        if notes.starts_with(query) {
            score += PREFIX_BONUS;
        }
    }
    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Bookmark, BookmarkStore, Folder, StoreMeta};
    use std::collections::HashMap;

    fn make_store(bookmarks: Vec<(&str, Bookmark)>) -> BookmarkStore {
        let mut bm_map = HashMap::new();
        for (id, bm) in bookmarks {
            bm_map.insert(id.to_string(), bm);
        }
        let mut folders = HashMap::new();
        folders.insert(
            "root".to_string(),
            Folder {
                title: "Root".to_string(),
                children: bm_map.keys().cloned().collect(),
                created_at: "2024-01-01T00:00:00Z".to_string(),
                updated_at: "2024-01-01T00:00:00Z".to_string(),
                deleted: false,
            },
        );
        BookmarkStore {
            root_folder_id: "root".to_string(),
            folders,
            bookmarks: bm_map,
            meta: StoreMeta {
                schema_version: 1,
                collection_name: "bookmarks".to_string(),
            },
        }
    }

    fn bookmark(title: &str, url: &str, notes: &str) -> Bookmark {
        Bookmark {
            title: title.to_string(),
            url: url.to_string(),
            notes: notes.to_string(),
            favicon: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
            deleted: false,
        }
    }

    #[test]
    fn prefix_match_scores_higher_than_substring_match() {
        let store = make_store(vec![
            (
                "a_substring",
                bookmark("Learn Rust Today", "https://other.com", ""),
            ),
            (
                "z_prefix",
                bookmark("Rust Programming", "https://other.com", ""),
            ),
        ]);

        let results = search_bookmarks(&store, "rust", SortOrder::Relevance);
        let ids: Vec<&str> = results.iter().map(|r| r.id.as_str()).collect();

        // z_prefix should rank higher despite alphabetically-later ID
        assert_eq!(ids, vec!["z_prefix", "a_substring"]);
    }

    #[test]
    fn deleted_bookmarks_are_excluded() {
        let mut bm = bookmark("Rust Lang", "https://rust-lang.org", "");
        bm.deleted = true;
        let store = make_store(vec![("deleted", bm)]);

        let results = search_bookmarks(&store, "rust", SortOrder::Relevance);
        assert!(results.is_empty());
    }

    #[test]
    fn same_query_produces_stable_order() {
        let store = make_store(vec![
            ("bbb", bookmark("Rust A", "https://a.com", "")),
            ("aaa", bookmark("Rust B", "https://b.com", "")),
            ("ccc", bookmark("Rust C", "https://c.com", "")),
        ]);

        let first = search_bookmarks(&store, "rust", SortOrder::Relevance);
        let second = search_bookmarks(&store, "rust", SortOrder::Relevance);
        let third = search_bookmarks(&store, "rust", SortOrder::Relevance);

        let ids_first: Vec<&str> = first.iter().map(|r| r.id.as_str()).collect();
        let ids_second: Vec<&str> = second.iter().map(|r| r.id.as_str()).collect();
        let ids_third: Vec<&str> = third.iter().map(|r| r.id.as_str()).collect();

        assert_eq!(ids_first, ids_second);
        assert_eq!(ids_second, ids_third);
    }

    #[test]
    fn explicit_sort_overrides_relevance() {
        let store = make_store(vec![
            (
                "high_relevance",
                bookmark("Rust Lang", "https://rust-lang.org", "all about rust"),
            ),
            (
                "low_relevance",
                bookmark("About Programming", "https://other.com", "rust mentioned"),
            ),
        ]);

        // By relevance, high_relevance would win. With NameAsc, alphabetical order wins.
        let results = search_bookmarks(&store, "rust", SortOrder::NameAsc);
        let ids: Vec<&str> = results.iter().map(|r| r.id.as_str()).collect();

        // "About Programming" < "Rust Lang" alphabetically
        assert_eq!(ids, vec!["low_relevance", "high_relevance"]);
    }

    #[test]
    fn title_match_ranks_above_url_only_ranks_above_notes_only() {
        let store = make_store(vec![
            (
                "notes_only",
                bookmark("Unrelated", "https://other.com", "rust is great"),
            ),
            (
                "url_only",
                bookmark("Unrelated", "https://rust-lang.org", ""),
            ),
            (
                "title_match",
                bookmark("Rust Programming", "https://other.com", ""),
            ),
        ]);

        let results = search_bookmarks(&store, "rust", SortOrder::Relevance);
        let ids: Vec<&str> = results.iter().map(|r| r.id.as_str()).collect();

        assert_eq!(ids, vec!["title_match", "url_only", "notes_only"]);
    }
}
