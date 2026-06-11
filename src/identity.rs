use automerge::ActorId;
use std::path::Path;

/// Bare hostname, for normal mode.
#[must_use]
pub fn hostname_client_id() -> String {
    hostname::get()
        .ok()
        .and_then(|h| {
            let s = h.to_string_lossy().to_string();
            if s.is_empty() { None } else { Some(s) }
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// Hostname + random 8-char suffix, for dev mode without explicit ID.
#[must_use]
pub fn dev_client_id() -> String {
    let host = hostname_client_id();
    let short = &uuid::Uuid::new_v4().to_string()[..8];
    format!("{host}-{short}")
}

/// # Panics
/// Panics if the actor ID file cannot be written.
#[must_use]
pub fn get_or_create_actor_id(local_data_dir: &Path) -> ActorId {
    let path = local_data_dir.join("actor_id");
    if let Ok(bytes) = std::fs::read(&path) {
        return ActorId::from(bytes.as_slice());
    }
    let actor = ActorId::random();
    std::fs::write(&path, actor.to_bytes()).expect("write actor_id");
    actor
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_hostname_client_id_returns_nonempty() {
        let id = hostname_client_id();
        assert!(!id.is_empty());
        let expected = hostname::get().map_or_else(
            |_| "unknown".to_string(),
            |h| h.to_string_lossy().to_string(),
        );
        assert_eq!(id, expected);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_dev_client_id_has_suffix() {
        let host = hostname_client_id();
        let dev = dev_client_id();
        assert!(dev.starts_with(&host), "dev id should start with hostname");
        let suffix = &dev[host.len()..];
        assert!(suffix.starts_with('-'), "separator should be '-'");
        assert_eq!(suffix.len(), 9, "dash + 8 hex chars");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_dev_client_ids_are_unique() {
        let a = dev_client_id();
        let b = dev_client_id();
        assert_ne!(a, b);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_actor_id_create_and_read_back() {
        let dir = TempDir::new().unwrap();
        let actor1 = get_or_create_actor_id(dir.path());
        let actor2 = get_or_create_actor_id(dir.path());
        assert_eq!(actor1.to_bytes(), actor2.to_bytes());
    }
}
