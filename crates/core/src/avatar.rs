const PALETTE: [&str; 14] = [
    "#DB4437", "#E91E63", "#9C27B0", "#673AB7", "#3F51B5", "#4285F4", "#039BE5", "#0097A7",
    "#009688", "#0F9D58", "#689F38", "#EF6C00", "#FF5722", "#757575",
];

fn extract_domain(url: &str) -> String {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let host = rest.split('/').next().unwrap_or("");
    let lower = host.to_ascii_lowercase();
    lower.strip_prefix("www.").unwrap_or(&lower).to_owned()
}

fn djb2(input: &str) -> u32 {
    let mut hash: u32 = 5381;
    for b in input.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(u32::from(b));
    }
    hash
}

#[must_use]
pub fn domain_letter(url: &str) -> String {
    let domain = extract_domain(url);
    match domain.chars().next() {
        Some(c) if c.is_alphabetic() => c.to_uppercase().to_string(),
        _ => "?".to_owned(),
    }
}

#[must_use]
pub fn domain_color(url: &str) -> String {
    let domain = extract_domain(url);
    let hash = djb2(&domain);
    PALETTE[hash as usize % PALETTE.len()].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_pairs() {
        let cases = [
            ("https://github.com/foo/bar", "G", "#673AB7"),
            ("https://www.reddit.com/r/rust", "R", "#009688"),
            ("http://example.com", "E", "#039BE5"),
            ("https://docs.rs/tokio", "D", "#0F9D58"),
        ];
        for (url, expected_letter, expected_color) in cases {
            assert_eq!(domain_letter(url), expected_letter, "letter for {url}");
            assert_eq!(domain_color(url), expected_color, "color for {url}");
        }
    }

    #[test]
    fn case_insensitive_color() {
        assert_eq!(
            domain_color("https://GitHub.com/foo"),
            domain_color("https://github.com/bar"),
        );
        assert_eq!(
            domain_color("https://WWW.Reddit.COM"),
            domain_color("https://www.reddit.com"),
        );
    }

    #[test]
    fn same_domain_different_paths_same_color() {
        let a = domain_color("https://github.com/foo");
        let b = domain_color("https://github.com/bar/baz?q=1");
        let c = domain_color("https://github.com");
        assert_eq!(a, b);
        assert_eq!(b, c);
    }

    #[test]
    fn color_always_in_palette() {
        let urls = [
            "",
            "https://a.com",
            "http://z.org/path?q=1",
            "ftp://x",
            "https://www.example.co.uk/foo/bar/baz",
        ];
        for url in urls {
            let color = domain_color(url);
            assert!(
                PALETTE.contains(&color.as_str()),
                "unexpected color {color} for {url}"
            );
        }
    }

    #[test]
    fn unparseable_url_returns_question_mark() {
        assert_eq!(domain_letter(""), "?");
        assert_eq!(domain_letter("://"), "?");
    }
}
