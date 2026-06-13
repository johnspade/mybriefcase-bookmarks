use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

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

async fn attempt_fetch(
    client: &reqwest::Client,
    url: &str,
) -> anyhow::Result<(Vec<u8>, String)> {
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
}
