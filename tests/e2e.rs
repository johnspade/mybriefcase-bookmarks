#![cfg(feature = "slow-tests")]

use std::net::TcpListener;
use std::time::Duration;
use tokio::process::{Child, Command};
use tokio::time::timeout;

struct ServerProcess {
    child: Child,
    port: u16,
}

impl ServerProcess {
    async fn start(
        binary: &str,
        sync_root: &str,
        local_dir: &str,
        port: u16,
        client_id: &str,
    ) -> Self {
        let child = Command::new(binary)
            .env("MBB_SYNC_ROOT", sync_root)
            .env("MBB_LOCAL_DIR", local_dir)
            .env("MBB_PORT", port.to_string())
            .env("MBB_CLIENT_ID", client_id)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .expect("failed to spawn server");

        let server = Self { child, port };
        server.wait_for_ready().await;
        server
    }

    async fn wait_for_ready(&self) {
        let url = format!("http://127.0.0.1:{}/", self.port);
        let client = reqwest::Client::new();
        let deadline = Duration::from_secs(10);

        timeout(deadline, async {
            loop {
                if client.get(&url).send().await.is_ok() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("server failed to start within 10 seconds");
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

fn find_free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

struct TestClient {
    inner: reqwest::Client,
}

impl TestClient {
    fn new() -> Self {
        Self {
            inner: reqwest::Client::new(),
        }
    }

    async fn get_tree(&self, base: &str) -> serde_json::Value {
        self.inner
            .get(format!("{base}/api/tree"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    async fn create_bookmark(
        &self,
        base: &str,
        folder_id: &str,
        url: &str,
        title: &str,
    ) -> serde_json::Value {
        self.inner
            .post(format!("{base}/api/folders/{folder_id}/bookmarks"))
            .json(&serde_json::json!({ "url": url, "title": title }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    async fn create_folder(&self, base: &str, parent_id: &str, title: &str) -> serde_json::Value {
        self.inner
            .post(format!("{base}/api/folders"))
            .json(&serde_json::json!({ "parent_folder_id": parent_id, "title": title }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    async fn update_bookmark(&self, base: &str, id: &str, body: &serde_json::Value) {
        self.inner
            .put(format!("{base}/api/bookmarks/{id}"))
            .json(body)
            .send()
            .await
            .unwrap();
    }

    async fn move_item(&self, base: &str, item_id: &str, from: &str, to: &str) {
        self.inner
            .post(format!("{base}/api/move"))
            .json(&serde_json::json!({
                "item_id": item_id,
                "from_folder_id": from,
                "to_folder_id": to,
            }))
            .send()
            .await
            .unwrap();
    }

    async fn poll_until_bookmark(&self, description: &str, base: &str, title: &str) {
        let deadline = Duration::from_secs(10);
        let title = title.to_owned();

        timeout(deadline, async {
            loop {
                let tree = self.get_tree(base).await;
                if tree["bookmarks"]
                    .as_array()
                    .is_some_and(|arr| arr.iter().any(|b| b["title"] == title))
                {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("Timed out waiting for: {description}"));
    }

    async fn poll_until(
        &self,
        description: &str,
        base: &str,
        predicate: impl Fn(&serde_json::Value) -> bool + Send + Sync,
    ) {
        let deadline = Duration::from_secs(10);

        timeout(deadline, async {
            loop {
                let tree = self.get_tree(base).await;
                if predicate(&tree) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("Timed out waiting for: {description}"));
    }
}

fn tree_root_id(tree: &serde_json::Value) -> String {
    tree["root_folder_id"].as_str().unwrap().to_owned()
}

fn find_folder_id(tree: &serde_json::Value, title: &str) -> String {
    tree["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["title"] == title)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn resp_id(resp: &serde_json::Value) -> String {
    resp["id"].as_str().unwrap().to_owned()
}

fn start_server<'a>(
    binary: &'a str,
    sync_root: &'a tempfile::TempDir,
    local_dir: &'a tempfile::TempDir,
    port: u16,
    client_id: &'a str,
) -> impl std::future::Future<Output = ServerProcess> + 'a {
    ServerProcess::start(
        binary,
        sync_root.path().to_str().unwrap(),
        local_dir.path().to_str().unwrap(),
        port,
        client_id,
    )
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn live_bidirectional_sync() {
    let binary = env!("CARGO_BIN_EXE_mybriefcase-bookmarks");
    let sync_root = tempfile::TempDir::new().unwrap();
    let local_a = tempfile::TempDir::new().unwrap();
    let local_b = tempfile::TempDir::new().unwrap();
    let (port_a, port_b) = (find_free_port(), find_free_port());
    let tc = TestClient::new();

    let server_a = start_server(binary, &sync_root, &local_a, port_a, "test-node-a").await;
    let base_a = server_a.base_url();

    let tree = tc.get_tree(&base_a).await;
    let root_id = tree_root_id(&tree);
    let bar_id = find_folder_id(&tree, "Bookmarks Bar");

    let bm1_id = resp_id(
        &tc.create_bookmark(&base_a, &bar_id, "https://www.rust-lang.org", "Rust")
            .await,
    );
    let bm2_id = resp_id(
        &tc.create_bookmark(&base_a, &bar_id, "https://automerge.org", "Automerge")
            .await,
    );
    let work_id = resp_id(&tc.create_folder(&base_a, &root_id, "Work").await);

    let server_b = start_server(binary, &sync_root, &local_b, port_b, "test-node-b").await;
    let base_b = server_b.base_url();

    let tree_b = tc.get_tree(&base_b).await;
    let b_titles: Vec<String> = tree_b["bookmarks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["title"].as_str().unwrap().to_owned())
        .collect();
    assert!(b_titles.contains(&"Rust".to_owned()));
    assert!(b_titles.contains(&"Automerge".to_owned()));

    tc.create_bookmark(&base_b, &bar_id, "https://syncthing.net", "Syncthing")
        .await;
    tc.update_bookmark(
        &base_b,
        &bm1_id,
        &serde_json::json!({"title": "Rust Lang (official)", "notes": "Check the book!"}),
    )
    .await;
    tc.move_item(&base_b, &bm2_id, &bar_id, &work_id).await;

    let expected_bm2_id = bm2_id.clone();
    tc.poll_until(
        "Client A sees all changes from B (Syncthing + move)",
        &base_a,
        |tree| {
            let has_syncthing = tree["bookmarks"]
                .as_array()
                .is_some_and(|arr| arr.iter().any(|b| b["title"] == "Syncthing"));
            let move_applied = tree["folders"].as_array().is_some_and(|arr| {
                arr.iter().filter(|f| f["title"] == "Work").any(|f| {
                    f["children"]
                        .as_array()
                        .is_some_and(|c| c.iter().any(|id| id == &*expected_bm2_id))
                })
            });
            has_syncthing && move_applied
        },
    )
    .await;

    let tree_a = tc.get_tree(&base_a).await;
    let rust_bm = tree_a["bookmarks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["url"] == "https://www.rust-lang.org")
        .expect("Rust bookmark should exist");
    assert_eq!(rust_bm["title"], "Rust Lang (official)");

    tc.create_bookmark(&base_a, &bar_id, "https://crates.io", "crates.io")
        .await;
    tc.poll_until_bookmark("Client B sees crates.io", &base_b, "crates.io")
        .await;

    let export_resp = tc
        .inner
        .get(format!("{base_a}/export"))
        .send()
        .await
        .unwrap();
    assert_eq!(export_resp.status(), 200);
    let html = export_resp.text().await.unwrap();
    assert!(html.contains("<!DOCTYPE NETSCAPE-Bookmark-file-1>"));
    assert!(html.contains("Syncthing"));
    assert!(html.contains("crates.io"));

    drop(server_a);
    drop(server_b);
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn history_and_revert_via_api() {
    let binary = env!("CARGO_BIN_EXE_mybriefcase-bookmarks");
    let sync_root = tempfile::TempDir::new().unwrap();
    let local_a = tempfile::TempDir::new().unwrap();
    let port = find_free_port();
    let tc = TestClient::new();

    let server = start_server(binary, &sync_root, &local_a, port, "test-history").await;
    let base = server.base_url();

    let tree = tc.get_tree(&base).await;
    let root_id = tree_root_id(&tree);

    let bm_id = resp_id(
        &tc.create_bookmark(&base, &root_id, "https://original.com", "Original Title")
            .await,
    );

    tc.update_bookmark(
        &base,
        &bm_id,
        &serde_json::json!({"title": "Updated Title", "url": "https://updated.com"}),
    )
    .await;

    let history: Vec<serde_json::Value> = tc
        .inner
        .get(format!("{base}/api/bookmarks/{bm_id}/history"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(history.len(), 2, "Should have 2 history entries");
    assert!(
        history[0]["changed_fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["field"] == "title"),
        "Latest entry should show title change"
    );

    let v1_hash = history.last().unwrap()["hash"].as_str().unwrap();
    let snapshot: serde_json::Value = tc
        .inner
        .get(format!("{base}/api/bookmarks/{bm_id}/at/{v1_hash}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(snapshot["title"], "Original Title");

    let revert_status = tc
        .inner
        .post(format!("{base}/api/bookmarks/{bm_id}/revert"))
        .json(&serde_json::json!({ "target_hash": v1_hash }))
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(revert_status, 200);

    let tree_after = tc.get_tree(&base).await;
    let bm = tree_after["bookmarks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == bm_id.as_str())
        .unwrap();
    assert_eq!(bm["title"], "Original Title");
    assert_eq!(bm["url"], "https://original.com");

    let history_after: Vec<serde_json::Value> = tc
        .inner
        .get(format!("{base}/api/bookmarks/{bm_id}/history"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(history_after.len(), 3, "Should have 3 entries after revert");
    assert!(
        history_after[0]["message"]
            .as_str()
            .unwrap()
            .starts_with("revert_bookmark:"),
        "Latest entry should be a revert"
    );

    drop(server);
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn sse_delivers_refresh_events() {
    let binary = env!("CARGO_BIN_EXE_mybriefcase-bookmarks");
    let sync_root = tempfile::TempDir::new().unwrap();
    let local_a = tempfile::TempDir::new().unwrap();
    let local_b = tempfile::TempDir::new().unwrap();
    let (port_a, port_b) = (find_free_port(), find_free_port());
    let tc = TestClient::new();

    let server_a = start_server(binary, &sync_root, &local_a, port_a, "test-sse-a").await;
    let base_a = server_a.base_url();

    let tree = tc.get_tree(&base_a).await;
    let bar_id = find_folder_id(&tree, "Bookmarks Bar");

    let sse_base = base_a.clone();
    let sse_client = reqwest::Client::new();
    let (sse_tx, mut sse_rx) = tokio::sync::mpsc::channel::<String>(8);
    tokio::spawn(async move {
        use futures_util::StreamExt;
        let resp = sse_client
            .get(format!("{sse_base}/events"))
            .send()
            .await
            .unwrap();
        let mut stream = resp.bytes_stream();
        while let Some(Ok(chunk)) = stream.next().await {
            let text = String::from_utf8_lossy(&chunk).to_string();
            if text.contains("event: refresh") {
                let _ = sse_tx.send(text).await;
            }
        }
    });

    let server_b = start_server(binary, &sync_root, &local_b, port_b, "test-sse-b").await;
    let base_b = server_b.base_url();

    tc.create_bookmark(&base_b, &bar_id, "https://example.com", "SSE Test Bookmark")
        .await;

    let event = timeout(Duration::from_secs(10), sse_rx.recv())
        .await
        .expect("Timed out waiting for SSE refresh event on Client A")
        .expect("SSE channel closed unexpectedly");

    assert!(
        event.contains("event: refresh"),
        "SSE event should be a refresh event"
    );

    let sidebar_html = tc
        .inner
        .get(format!("{base_a}/sidebar?folder_id={bar_id}"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(sidebar_html.contains("Bookmarks Bar"));

    let content_html = tc
        .inner
        .get(format!("{base_a}/folders/{bar_id}/content"))
        .header("hx-request", "true")
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(content_html.contains("SSE Test Bookmark"));

    drop(server_a);
    drop(server_b);
}
