use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const MAX_FAVICON_SIZE: usize = 256 * 1024;

/// # Errors
/// Returns an error if the data is empty, too large, or the file cannot be written.
pub fn store_favicon(sync_root: &Path, data: &[u8], mime: &str) -> anyhow::Result<String> {
    anyhow::ensure!(
        !data.is_empty() && data.len() <= MAX_FAVICON_SIZE,
        "favicon size out of range"
    );
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
        .build()?;
    let resp = client.get(favicon_url).send().await?.error_for_status()?;
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
    let bytes = resp.bytes().await?;
    store_favicon(sync_root, &bytes, &content_type)
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

    #[test]
    #[cfg_attr(miri, ignore)]
    fn store_favicon_writes_and_returns_filename() {
        let tmp = TempDir::new().unwrap();
        let data = b"fake png data";
        let result = store_favicon(tmp.path(), data, "image/png").unwrap();
        assert!(
            Path::new(&result)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
        );
        assert_eq!(result.len(), 64 + 4); // sha256 hex + ".png"
        let path = favicon_path(tmp.path(), &result);
        assert!(path.exists());
        assert_eq!(std::fs::read(&path).unwrap(), data);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn store_favicon_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let data = b"same data";
        let r1 = store_favicon(tmp.path(), data, "image/png").unwrap();
        let r2 = store_favicon(tmp.path(), data, "image/png").unwrap();
        assert_eq!(r1, r2);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn store_favicon_rejects_oversized() {
        let tmp = TempDir::new().unwrap();
        let data = vec![0u8; MAX_FAVICON_SIZE + 1];
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
}
