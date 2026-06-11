use crate::model::{BookmarkStore, Folder};
use std::io::Write;

/// # Errors
/// Returns an error if writing to the writer fails.
pub fn export_netscape_html<W: Write>(
    store: &BookmarkStore,
    writer: &mut W,
) -> std::io::Result<()> {
    writeln!(writer, "<!DOCTYPE NETSCAPE-Bookmark-file-1>")?;
    writeln!(writer, "<!--This is an automatically generated file.")?;
    writeln!(
        writer,
        "     It will be read and overwritten. Do Not Edit! -->"
    )?;
    writeln!(
        writer,
        r#"<META HTTP-EQUIV="Content-Type" CONTENT="text/html; charset=UTF-8">"#
    )?;
    writeln!(writer, "<TITLE>Bookmarks</TITLE>")?;
    writeln!(writer, "<H1>Bookmarks</H1>")?;
    writeln!(writer, "<DL><p>")?;
    if let Some(root) = store.folders.get(&store.root_folder_id) {
        write_folder_children(store, root, writer, 1)?;
    }
    writeln!(writer, "</DL><p>")?;
    Ok(())
}

fn write_folder_children<W: Write>(
    store: &BookmarkStore,
    folder: &Folder,
    writer: &mut W,
    depth: usize,
) -> std::io::Result<()> {
    let indent = "    ".repeat(depth);
    for child_id in &folder.children {
        if let Some(sub) = store.folders.get(child_id) {
            if sub.deleted {
                continue;
            }
            writeln!(
                writer,
                r#"{indent}<DT><H3 ADD_DATE="{}" LAST_MODIFIED="{}">{}</H3>"#,
                to_unix(&sub.created_at),
                to_unix(&sub.updated_at),
                html_escape(&sub.title)
            )?;
            writeln!(writer, "{indent}<DL><p>")?;
            write_folder_children(store, sub, writer, depth + 1)?;
            writeln!(writer, "{indent}</DL><p>")?;
        } else if let Some(bm) = store.bookmarks.get(child_id) {
            if bm.deleted {
                continue;
            }
            writeln!(
                writer,
                r#"{indent}<DT><A HREF="{}" ADD_DATE="{}" LAST_MODIFIED="{}">{}</A>"#,
                html_escape(&bm.url),
                to_unix(&bm.created_at),
                to_unix(&bm.updated_at),
                html_escape(&bm.title)
            )?;
        }
    }
    Ok(())
}

fn to_unix(iso: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(iso).map_or(0, |dt| dt.timestamp())
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_escape_ampersand() {
        assert_eq!(html_escape("a & b"), "a &amp; b");
    }

    #[test]
    fn test_html_escape_angle_brackets() {
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
    }

    #[test]
    fn test_html_escape_quotes() {
        assert_eq!(html_escape(r#"say "hello""#), "say &quot;hello&quot;");
    }

    #[test]
    fn test_html_escape_combined() {
        assert_eq!(
            html_escape(r#"<a href="x&y">"#),
            "&lt;a href=&quot;x&amp;y&quot;&gt;"
        );
    }

    #[test]
    fn test_html_escape_no_special_chars() {
        assert_eq!(html_escape("plain text"), "plain text");
    }

    #[test]
    fn test_to_unix_valid_rfc3339() {
        assert_eq!(to_unix("2024-01-15T10:30:00+00:00"), 1_705_314_600);
    }

    #[test]
    fn test_to_unix_invalid_returns_zero() {
        assert_eq!(to_unix("not-a-date"), 0);
    }

    #[test]
    fn test_to_unix_empty_string() {
        assert_eq!(to_unix(""), 0);
    }
}
