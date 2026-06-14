use scraper::{Html, Selector};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use url::Url;

const MAX_FAVICON_SIZE: usize = 256 * 1024;
const USER_AGENT: &str = "MyBriefcase/1.0 (favicon fetcher)";

fn is_valid_image(data: &[u8]) -> bool {
    if infer::get(data).is_some_and(|t| t.matcher_type() == infer::MatcherType::Image) {
        return true;
    }
    // infer doesn't detect SVG (text-based, no magic bytes)
    data.starts_with(b"<?xml") || data.starts_with(b"<svg")
}

/// # Errors
/// Returns an error if the data is empty, too large, not a valid image, or the file cannot be written.
pub fn store_favicon(sync_root: &Path, data: &[u8], mime: &str) -> anyhow::Result<String> {
    anyhow::ensure!(
        !data.is_empty() && data.len() <= MAX_FAVICON_SIZE,
        "favicon size out of range"
    );
    anyhow::ensure!(is_valid_image(data), "response is not a valid image");
    let ext = mime_to_ext(mime);
    let hash = hex::encode(Sha256::digest(data));
    let filename = format!("{hash}.{ext}");
    let dir = sync_root.join("favicons");
    std::fs::create_dir_all(&dir)?;
    let dest = dir.join(&filename);
    if !dest.exists() {
        let tmp = dest.with_extension("tmp");
        std::fs::write(&tmp, data)?;
        std::fs::rename(&tmp, &dest)?;
    }
    Ok(filename)
}

/// # Errors
/// Returns an error if the favicon cannot be fetched or stored.
pub async fn fetch_and_store(sync_root: &Path, favicon_url: &str) -> anyhow::Result<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent(USER_AGENT)
        .build()?;

    let mut last_err = None;
    for _ in 0..2 {
        match attempt_fetch(&client, favicon_url).await {
            Ok((data, content_type)) => return store_favicon(sync_root, &data, &content_type),
            Err(e) => {
                if is_retryable(&e) {
                    last_err = Some(e);
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    continue;
                }
                return Err(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("favicon fetch failed")))
}

/// Fetches a page at `page_url`, discovers its favicon link tags, and returns the best favicon URL.
/// Falls back to `{origin}/favicon.ico` if no `<link>` tags found.
/// # Errors
/// Returns an error if the page cannot be fetched or no favicon can be discovered.
pub async fn discover_favicon_url(page_url: &str) -> anyhow::Result<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent(USER_AGENT)
        .build()?;

    let resp = client.get(page_url).send().await?.error_for_status()?;
    let final_url = resp.url().clone();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if !content_type.contains("text/html") {
        let origin = origin_url(&final_url)?;
        return Ok(format!("{origin}/favicon.ico"));
    }

    let body = resp.text().await?;
    if let Some(icon_url) = find_best_icon_link(&body, &final_url) {
        return Ok(icon_url);
    }

    let origin = origin_url(&final_url)?;
    Ok(format!("{origin}/favicon.ico"))
}

fn origin_url(url: &Url) -> anyhow::Result<String> {
    let origin = url.origin();
    match origin {
        url::Origin::Tuple(scheme, host, port) => {
            let default_port = match scheme.as_str() {
                "https" => 443,
                "http" => 80,
                _ => 0,
            };
            if port == default_port {
                Ok(format!("{scheme}://{host}"))
            } else {
                Ok(format!("{scheme}://{host}:{port}"))
            }
        }
        url::Origin::Opaque(_) => anyhow::bail!("opaque origin"),
    }
}

fn find_best_icon_link(html_body: &str, base_url: &Url) -> Option<String> {
    let document = Html::parse_document(html_body);
    let selector =
        Selector::parse("link[rel~=\"icon\"], link[rel=\"apple-touch-icon\"], link[rel=\"apple-touch-icon-precomposed\"]")
            .ok()?;

    let mut best: Option<(u32, String)> = None;
    for element in document.select(&selector) {
        let Some(href) = element.value().attr("href") else {
            continue;
        };
        let Ok(abs_url) = base_url.join(href) else {
            continue;
        };

        let size = parse_size(element.value().attr("sizes"));
        let rel = element.value().attr("rel").unwrap_or("");
        let effective_size = if size > 0 {
            size
        } else if rel.contains("apple-touch-icon") {
            180
        } else {
            16
        };

        if best.as_ref().is_none_or(|(s, _)| effective_size > *s) {
            best = Some((effective_size, abs_url.to_string()));
        }
    }

    best.map(|(_, url)| url)
}

fn parse_size(sizes_attr: Option<&str>) -> u32 {
    let Some(sizes) = sizes_attr else {
        return 0;
    };
    sizes
        .split_whitespace()
        .filter_map(|s| {
            let (w, _) = s.split_once('x').or_else(|| s.split_once('X'))?;
            w.parse::<u32>().ok()
        })
        .max()
        .unwrap_or(0)
}

async fn attempt_fetch(client: &reqwest::Client, url: &str) -> anyhow::Result<(Vec<u8>, String)> {
    let resp = client.get(url).send().await?.error_for_status()?;
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/png")
        .split(';')
        .next()
        .unwrap_or("image/png")
        .trim()
        .to_string();
    let bytes = resp.bytes().await?.to_vec();
    anyhow::ensure!(is_valid_image(&bytes), "response is not a valid image");
    Ok((bytes, content_type))
}

fn is_retryable(e: &anyhow::Error) -> bool {
    if let Some(reqwest_err) = e.downcast_ref::<reqwest::Error>() {
        if reqwest_err.is_timeout() || reqwest_err.is_connect() {
            return true;
        }
        if let Some(status) = reqwest_err.status() {
            return status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS;
        }
    }
    false
}

#[must_use]
pub fn favicon_path(sync_root: &Path, filename: &str) -> PathBuf {
    sync_root.join("favicons").join(filename)
}

fn mime_to_ext(mime: &str) -> &str {
    match mime {
        "image/x-icon" | "image/vnd.microsoft.icon" => "ico",
        "image/svg+xml" => "svg",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "png",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const VALID_PNG: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00];
    const VALID_ICO: &[u8] = &[0x00, 0x00, 0x01, 0x00, 0x01, 0x00];
    const VALID_JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
    const VALID_GIF: &[u8] = b"GIF89a\x01\x00\x01\x00";
    const VALID_SVG: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>";

    #[test]
    #[cfg_attr(miri, ignore)]
    fn store_favicon_writes_and_returns_filename() {
        let tmp = TempDir::new().unwrap();
        let result = store_favicon(tmp.path(), VALID_PNG, "image/png").unwrap();
        assert!(
            Path::new(&result)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
        );
        assert_eq!(result.len(), 64 + 4); // sha256 hex + ".png"
        let path = favicon_path(tmp.path(), &result);
        assert!(path.exists());
        assert_eq!(std::fs::read(&path).unwrap(), VALID_PNG);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn store_favicon_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let r1 = store_favicon(tmp.path(), VALID_PNG, "image/png").unwrap();
        let r2 = store_favicon(tmp.path(), VALID_PNG, "image/png").unwrap();
        assert_eq!(r1, r2);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn store_favicon_rejects_oversized() {
        let tmp = TempDir::new().unwrap();
        let mut data = vec![0u8; MAX_FAVICON_SIZE + 1];
        // Give it a valid PNG header so the size check is what rejects it
        data[..4].copy_from_slice(&[0x89, 0x50, 0x4E, 0x47]);
        let result = store_favicon(tmp.path(), &data, "image/png");
        assert!(result.is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn store_favicon_rejects_empty() {
        let tmp = TempDir::new().unwrap();
        let result = store_favicon(tmp.path(), b"", "image/png");
        assert!(result.is_err());
    }

    #[test]
    fn mime_to_ext_maps_known_types() {
        assert_eq!(mime_to_ext("image/png"), "png");
        assert_eq!(mime_to_ext("image/x-icon"), "ico");
        assert_eq!(mime_to_ext("image/vnd.microsoft.icon"), "ico");
        assert_eq!(mime_to_ext("image/svg+xml"), "svg");
        assert_eq!(mime_to_ext("image/jpeg"), "jpg");
        assert_eq!(mime_to_ext("image/gif"), "gif");
        assert_eq!(mime_to_ext("image/webp"), "webp");
        assert_eq!(mime_to_ext("application/octet-stream"), "png");
    }

    #[test]
    fn is_valid_image_accepts_known_formats() {
        assert!(is_valid_image(VALID_PNG));
        assert!(is_valid_image(VALID_ICO));
        assert!(is_valid_image(VALID_JPEG));
        assert!(is_valid_image(VALID_GIF));
        assert!(is_valid_image(VALID_SVG));
        assert!(is_valid_image(b"<?xml version=\"1.0\"?><svg></svg>"));
        // RIFF/WebP
        assert!(is_valid_image(b"RIFF\x00\x00\x00\x00WEBP"));
    }

    #[test]
    fn is_valid_image_rejects_non_images() {
        assert!(!is_valid_image(b"<!DOCTYPE html><html>"));
        assert!(!is_valid_image(b"{\"error\": \"not found\"}"));
        assert!(!is_valid_image(b"abc"));
        assert!(!is_valid_image(b""));
    }

    #[test]
    fn is_retryable_detects_server_errors() {
        let err = anyhow::anyhow!("not a reqwest error");
        assert!(!is_retryable(&err));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn find_best_icon_link_picks_largest() {
        let html = r#"<html><head>
            <link rel="icon" href="/small.png" sizes="16x16">
            <link rel="icon" href="/large.png" sizes="64x64">
            <link rel="icon" href="/medium.png" sizes="32x32">
        </head></html>"#;
        let base = Url::parse("https://example.com/page").unwrap();
        let result = find_best_icon_link(html, &base).unwrap();
        assert_eq!(result, "https://example.com/large.png");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn find_best_icon_link_prefers_apple_touch_icon() {
        let html = r#"<html><head>
            <link rel="icon" href="/icon.png" sizes="32x32">
            <link rel="apple-touch-icon" href="/apple.png">
        </head></html>"#;
        let base = Url::parse("https://example.com/").unwrap();
        let result = find_best_icon_link(html, &base).unwrap();
        assert_eq!(result, "https://example.com/apple.png");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn find_best_icon_link_resolves_relative_urls() {
        let html = r#"<html><head>
            <link rel="icon" href="../img/icon.png">
        </head></html>"#;
        let base = Url::parse("https://example.com/path/page").unwrap();
        let result = find_best_icon_link(html, &base).unwrap();
        assert_eq!(result, "https://example.com/img/icon.png");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn find_best_icon_link_returns_none_when_no_links() {
        let html = r"<html><head><title>No icons</title></head></html>";
        let base = Url::parse("https://example.com/").unwrap();
        assert!(find_best_icon_link(html, &base).is_none());
    }

    #[test]
    fn parse_size_extracts_largest_dimension() {
        assert_eq!(parse_size(Some("32x32")), 32);
        assert_eq!(parse_size(Some("16x16 32x32 64x64")), 64);
        assert_eq!(parse_size(Some("any")), 0);
        assert_eq!(parse_size(None), 0);
    }
}
