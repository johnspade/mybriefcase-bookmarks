use scraper::{Html, Node};

#[derive(Debug, Clone)]
pub enum ImportedItem {
    Folder {
        title: String,
        created_at: Option<String>,
        updated_at: Option<String>,
        children: Vec<Self>,
    },
    Bookmark {
        url: String,
        title: String,
        notes: String,
        created_at: Option<String>,
        updated_at: Option<String>,
    },
}

fn unix_ts_to_iso(ts_str: &str) -> Option<String> {
    let ts: i64 = ts_str.parse().ok()?;
    chrono::DateTime::from_timestamp(ts, 0).map(|dt| dt.to_rfc3339())
}

/// Parses a Netscape HTML bookmarks file into a flat or nested list of [`ImportedItem`]s.
#[must_use]
pub fn parse_netscape_html(input: &str) -> Vec<ImportedItem> {
    let document = Html::parse_document(input);
    let root = document.tree.root();
    let mut dl_nodes = Vec::new();
    find_top_level_dls(&document, root.id(), &mut dl_nodes);

    if dl_nodes.is_empty() {
        return Vec::new();
    }

    let mut items = Vec::new();
    for &dl_id in &dl_nodes {
        parse_dl(&document, dl_id, &mut items);
    }
    items
}

fn find_top_level_dls(doc: &Html, node_id: ego_tree::NodeId, result: &mut Vec<ego_tree::NodeId>) {
    let node = doc.tree.get(node_id).unwrap();
    if let Node::Element(el) = node.value() {
        if el.name().eq_ignore_ascii_case("dl") {
            result.push(node_id);
            return;
        }
    }
    for child in node.children() {
        find_top_level_dls(doc, child.id(), result);
    }
}

fn parse_dl(doc: &Html, parent_id: ego_tree::NodeId, items: &mut Vec<ImportedItem>) {
    let dl_node = doc.tree.get(parent_id).unwrap();

    let mut child_iter = dl_node.children().peekable();
    while let Some(child) = child_iter.next() {
        if let Node::Element(el) = child.value() {
            if el.name().eq_ignore_ascii_case("dt") {
                if let Some(item) = parse_dt_entry(doc, child.id(), &mut child_iter) {
                    items.push(item);
                }
            }
        }
    }
}

fn parse_dt_entry<'a, I>(
    doc: &Html,
    entry_id: ego_tree::NodeId,
    siblings: &mut std::iter::Peekable<I>,
) -> Option<ImportedItem>
where
    I: Iterator<Item = ego_tree::NodeRef<'a, Node>>,
{
    let entry_node = doc.tree.get(entry_id)?;

    for child in entry_node.children() {
        if let Node::Element(el) = child.value() {
            if el.name().eq_ignore_ascii_case("h3") {
                let title = collect_text(doc, child.id());
                let created_at = el
                    .attr("ADD_DATE")
                    .or_else(|| el.attr("add_date"))
                    .and_then(unix_ts_to_iso);
                let updated_at = el
                    .attr("LAST_MODIFIED")
                    .or_else(|| el.attr("last_modified"))
                    .and_then(unix_ts_to_iso);

                let mut children = Vec::new();
                // HTML5 parsers may nest the <DL> inside the <DT> or place it as a sibling.
                // Check DT children first, then siblings.
                if !find_dl_in_children(doc, entry_id, &mut children) {
                    consume_next_dl(doc, siblings, &mut children);
                }

                return Some(ImportedItem::Folder {
                    title,
                    created_at,
                    updated_at,
                    children,
                });
            }

            if el.name().eq_ignore_ascii_case("a") {
                let url = el
                    .attr("HREF")
                    .or_else(|| el.attr("href"))
                    .unwrap_or("")
                    .to_string();
                let title = collect_text(doc, child.id());
                let created_at = el
                    .attr("ADD_DATE")
                    .or_else(|| el.attr("add_date"))
                    .and_then(unix_ts_to_iso);
                let updated_at = el
                    .attr("LAST_MODIFIED")
                    .or_else(|| el.attr("last_modified"))
                    .and_then(unix_ts_to_iso);

                let mut notes = String::new();
                consume_next_dd(doc, siblings, &mut notes);

                return Some(ImportedItem::Bookmark {
                    url,
                    title,
                    notes,
                    created_at,
                    updated_at,
                });
            }
        }
    }

    None
}

fn find_dl_in_children(
    doc: &Html,
    node_id: ego_tree::NodeId,
    children: &mut Vec<ImportedItem>,
) -> bool {
    let node = doc.tree.get(node_id).unwrap();
    for child in node.children() {
        if let Node::Element(el) = child.value() {
            if el.name().eq_ignore_ascii_case("dl") {
                parse_dl(doc, child.id(), children);
                return true;
            }
        }
    }
    false
}

fn consume_next_dl<'a, I>(
    doc: &Html,
    siblings: &mut std::iter::Peekable<I>,
    children: &mut Vec<ImportedItem>,
) where
    I: Iterator<Item = ego_tree::NodeRef<'a, Node>>,
{
    while let Some(next) = siblings.peek() {
        match next.value() {
            Node::Element(el) if el.name().eq_ignore_ascii_case("dl") => {
                let id = next.id();
                siblings.next();
                parse_dl(doc, id, children);
                return;
            }
            Node::Element(el) if el.name().eq_ignore_ascii_case("dt") => return,
            Node::Element(_) | Node::Text(_) | Node::Comment(_) => {
                siblings.next();
            }
            _ => return,
        }
    }
}

fn consume_next_dd<'a, I>(doc: &Html, siblings: &mut std::iter::Peekable<I>, notes: &mut String)
where
    I: Iterator<Item = ego_tree::NodeRef<'a, Node>>,
{
    while let Some(next) = siblings.peek() {
        match next.value() {
            Node::Element(el) if el.name().eq_ignore_ascii_case("dd") => {
                *notes = collect_text(doc, next.id());
                siblings.next();
                return;
            }
            Node::Text(_) | Node::Comment(_) => {
                siblings.next();
            }
            _ => return,
        }
    }
}

fn collect_text(doc: &Html, node_id: ego_tree::NodeId) -> String {
    let node = doc.tree.get(node_id).unwrap();
    let mut text = String::new();
    for descendant in node.descendants() {
        if let Node::Text(t) = descendant.value() {
            text.push_str(t);
        }
    }
    text.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn empty_file() {
        let items = parse_netscape_html("");
        assert!(items.is_empty());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn flat_bookmarks() {
        let html = r#"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<DL><p>
<DT><A HREF="https://example.com" ADD_DATE="1700000000">Example</A>
<DT><A HREF="https://rust-lang.org">Rust</A>
</DL>"#;
        let items = parse_netscape_html(html);
        assert_eq!(items.len(), 2);
        match &items[0] {
            ImportedItem::Bookmark {
                url,
                title,
                created_at,
                ..
            } => {
                assert_eq!(url, "https://example.com");
                assert_eq!(title, "Example");
                assert!(created_at.is_some());
            }
            ImportedItem::Folder { .. } => panic!("expected bookmark"),
        }
        match &items[1] {
            ImportedItem::Bookmark { url, title, .. } => {
                assert_eq!(url, "https://rust-lang.org");
                assert_eq!(title, "Rust");
            }
            ImportedItem::Folder { .. } => panic!("expected bookmark"),
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn nested_folders() {
        let html = r#"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<DL><p>
<DT><H3 ADD_DATE="1700000000">Work</H3>
<DL><p>
<DT><A HREF="https://github.com">GitHub</A>
<DT><H3>Subproject</H3>
<DL><p>
<DT><A HREF="https://jira.com">Jira</A>
</DL><p>
</DL><p>
<DT><A HREF="https://news.ycombinator.com">HN</A>
</DL>"#;
        let items = parse_netscape_html(html);
        assert_eq!(items.len(), 2, "top-level items: {items:?}");
        match &items[0] {
            ImportedItem::Folder {
                title,
                children,
                created_at,
                ..
            } => {
                assert_eq!(title, "Work");
                assert!(created_at.is_some());
                assert_eq!(children.len(), 2, "Work children: {children:?}");
                match &children[1] {
                    ImportedItem::Folder {
                        title, children, ..
                    } => {
                        assert_eq!(title, "Subproject");
                        assert_eq!(children.len(), 1);
                    }
                    ImportedItem::Bookmark { .. } => panic!("expected subfolder"),
                }
            }
            ImportedItem::Bookmark { .. } => panic!("expected folder"),
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn dd_notes() {
        let html = r#"<DL><p>
<DT><A HREF="https://example.com">Example</A>
<DD>Some useful notes about this site
</DL>"#;
        let items = parse_netscape_html(html);
        assert_eq!(items.len(), 1);
        match &items[0] {
            ImportedItem::Bookmark { notes, .. } => {
                assert_eq!(notes, "Some useful notes about this site");
            }
            ImportedItem::Folder { .. } => panic!("expected bookmark"),
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn html_entities() {
        let html = r#"<DL><p>
<DT><A HREF="https://example.com">Tom &amp; Jerry&#39;s &lt;Site&gt;</A>
</DL>"#;
        let items = parse_netscape_html(html);
        match &items[0] {
            ImportedItem::Bookmark { title, .. } => {
                assert_eq!(title, "Tom & Jerry's <Site>");
            }
            ImportedItem::Folder { .. } => panic!("expected bookmark"),
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn timestamps_converted() {
        let html = r#"<DL><p>
<DT><A HREF="https://example.com" ADD_DATE="1700000000" LAST_MODIFIED="1700100000">Test</A>
</DL>"#;
        let items = parse_netscape_html(html);
        match &items[0] {
            ImportedItem::Bookmark {
                created_at,
                updated_at,
                ..
            } => {
                assert!(created_at.as_ref().unwrap().contains("2023-11-14"));
                assert!(updated_at.as_ref().unwrap().contains("2023-11-16"));
            }
            ImportedItem::Folder { .. } => panic!("expected bookmark"),
        }
    }
}
