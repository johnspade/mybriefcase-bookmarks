use mybriefcase_bookmarks::{api, handlers, identity, repo, state, watcher};

use axum::Router;
use axum::routing::{get, post, put};
use axum_embed::ServeEmbed;
use rust_embed::Embed;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tower_http::catch_panic::CatchPanicLayer;

#[derive(Embed, Clone)]
#[folder = "static/"]
struct StaticAssets;

struct Config {
    sync_root: PathBuf,
    local_data_dir: PathBuf,
    port: u16,
    dev_mode: bool,
    client_id: Option<String>,
}

impl Config {
    fn from_env() -> Self {
        Self {
            sync_root: PathBuf::from(
                std::env::var("MBB_SYNC_ROOT").unwrap_or_else(|_| "./sync_data".to_owned()),
            ),
            local_data_dir: PathBuf::from(
                std::env::var("MBB_LOCAL_DIR").unwrap_or_else(|_| "./local_data".to_owned()),
            ),
            port: std::env::var("MBB_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(3000),
            dev_mode: std::env::var("MBB_DEV_MODE").is_ok(),
            client_id: std::env::var("MBB_CLIENT_ID").ok(),
        }
    }
}

fn compute_static_version() -> String {
    let mut hasher = DefaultHasher::new();
    for path in StaticAssets::iter() {
        if let Some(file) = StaticAssets::get(&path) {
            file.data.hash(&mut hasher);
        }
    }
    format!("{:016x}", hasher.finish())
}

fn write_peer_info(sync_root: &std::path::Path, client_id: &str) {
    let info_path = sync_root.join(client_id).join("info.json");
    std::fs::create_dir_all(info_path.parent().unwrap()).unwrap();
    let info = serde_json::json!({
        "client_id": client_id,
        "created_at": chrono::Utc::now().to_rfc3339(),
        "app_version": env!("CARGO_PKG_VERSION"),
    });
    std::fs::write(&info_path, info.to_string()).unwrap();
}

fn build_router(state: Arc<state::AppState>) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/", get(handlers::index_page))
        .route("/folders/{id}", get(handlers::dispatch_get_folder))
        .route(
            "/bookmarks/{id}",
            get(handlers::bookmark_detail)
                .put(api::update_bookmark)
                .delete(api::delete_bookmark),
        )
        .route("/bookmarks/{id}/detail", get(handlers::bookmark_detail))
        .route(
            "/bookmarks/{id}/edit-form",
            get(handlers::bookmark_edit_form),
        )
        .route("/folders/new", post(handlers::create_folder_html))
        .route("/bookmarks/new", post(handlers::create_bookmark_html))
        .route("/bookmarks/{id}/edit", post(handlers::update_bookmark_html))
        .route(
            "/bookmarks/{id}/history",
            get(handlers::bookmark_history_html),
        )
        .route(
            "/bookmarks/{id}/revert",
            post(handlers::revert_bookmark_html),
        )
        .route(
            "/bookmarks/{id}/remove",
            post(handlers::delete_bookmark_html),
        )
        .route("/folders/{id}/remove", post(handlers::delete_folder_html))
        .route("/folders/{id}/rename", post(handlers::rename_folder_html))
        .route("/items/move", post(handlers::move_item_html))
        .route("/move-picker/{id}", get(handlers::move_picker_html))
        .route("/events", get(handlers::sse_events))
        .route("/sidebar", get(handlers::sidebar_only))
        .route("/search", get(handlers::search))
        .route("/folders/{id}/content", get(handlers::folder_content))
        .route("/api/tree", get(api::get_tree))
        .route("/api/folders/{id}", get(api::get_folder))
        .route("/api/folders", post(api::create_folder))
        .route("/api/folders/{id}/bookmarks", post(api::create_bookmark))
        .route(
            "/api/bookmarks/{id}",
            put(api::update_bookmark).delete(api::delete_bookmark),
        )
        .route(
            "/api/bookmarks/{id}/history",
            get(api::get_bookmark_history),
        )
        .route(
            "/api/bookmarks/{id}/at/{hash}",
            get(api::get_bookmark_at_hash),
        )
        .route("/api/bookmarks/{id}/revert", post(api::revert_bookmark))
        .route("/api/move", post(api::move_item))
        .route("/settings", get(handlers::settings_page))
        .route("/folder-options", get(handlers::folder_options))
        .route("/import", post(handlers::import_bookmarks_html))
        .route("/export", get(api::export_bookmarks))
        .route("/favicons/{filename}", get(handlers::serve_favicon))
        .nest_service("/static", ServeEmbed::<StaticAssets>::new())
        .with_state(state)
        .layer(CatchPanicLayer::new())
}

fn spawn_watcher(state: &Arc<state::AppState>) {
    let mut watcher_rx = watcher::start_file_watcher(&state.sync_root, &state.client_id);
    let watcher_state = Arc::clone(state);
    tokio::spawn(async move {
        while let Some(changed_peers) = watcher_rx.recv().await {
            let did_change = watcher::merge_specific_peers(
                &watcher_state.doc_handle,
                &watcher_state.sync_root,
                &changed_peers,
            );
            if did_change {
                let _ = watcher_state.sse_tx.send(());
                eprintln!("Merged changes from peers: {}", changed_peers.join(", "));
            }
        }
    });
}

fn spawn_poller(state: &Arc<state::AppState>) {
    let st = Arc::clone(state);
    tokio::spawn(async move {
        let mut poll = watcher::PollState::new(&st.sync_root, &st.client_id);
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        interval.tick().await;
        loop {
            interval.tick().await;
            let changed = poll.poll_changed_peers(&st.sync_root, &st.client_id);
            if changed.is_empty() {
                continue;
            }
            let did_merge = watcher::merge_specific_peers(&st.doc_handle, &st.sync_root, &changed);
            if did_merge {
                let _ = st.sse_tx.send(());
                eprintln!("Poll: merged changes from peers: {}", changed.join(", "));
            }
        }
    });
}

#[tokio::main]
async fn main() {
    let cfg = Config::from_env();

    std::fs::create_dir_all(&cfg.sync_root).unwrap();
    std::fs::create_dir_all(&cfg.local_data_dir).unwrap();

    let client_id = if cfg.dev_mode {
        cfg.client_id.unwrap_or_else(identity::dev_client_id)
    } else {
        cfg.client_id.unwrap_or_else(identity::hostname_client_id)
    };
    let _actor_id = identity::get_or_create_actor_id(&cfg.local_data_dir);

    eprintln!("Client ID: {client_id}");
    eprintln!("Sync root: {}", cfg.sync_root.display());
    eprintln!("Local data: {}", cfg.local_data_dir.display());

    let (_repo_handle, doc_handle, _document_id) =
        repo::init_repo(&cfg.local_data_dir, &cfg.sync_root, &client_id).await;

    repo::full_merge_pass(&doc_handle, &cfg.sync_root, &client_id);
    repo::export_doc_to_shared(&doc_handle, &cfg.sync_root, &client_id);
    write_peer_info(&cfg.sync_root, &client_id);

    let (sse_tx, _) = tokio::sync::broadcast::channel::<()>(16);
    let static_version = compute_static_version();
    let state = Arc::new(state::AppState {
        doc_handle,
        sync_root: cfg.sync_root,
        client_id,
        sse_tx,
        static_version,
    });

    spawn_watcher(&state);
    spawn_poller(&state);
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", cfg.port))
        .await
        .unwrap();
    let actual_port = listener.local_addr().unwrap().port();
    eprintln!("Listening on http://localhost:{actual_port}");
    axum::serve(listener, app).await.unwrap();
}
