//! board-server: the board server binary.

mod config;
mod secret;

use std::process::ExitCode;
use std::sync::Arc;

use board_db::{CaptureStore, CommentStore, Db};
use board_engine::{Engine, Ports, PreviewSettings};
use board_render_client::RenderClient;
use board_store::GitStore;
use board_workspace_client::WorkspaceClient;

use crate::config::Config;

#[tokio::main]
async fn main() -> ExitCode {
    let config = match Config::from_env(std::env::args().skip(1), |key| std::env::var(key).ok()) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("board-server: {message}");
            return ExitCode::FAILURE;
        }
    };
    match run(config).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("board-server: {message}");
            ExitCode::FAILURE
        }
    }
}

async fn run(config: Config) -> Result<(), String> {
    std::fs::create_dir_all(config.data_dir())
        .map_err(|e| format!("cannot create {}: {e}", config.data_dir().display()))?;
    let secret = secret::load(&config.data_dir(), std::env::var("BOARD_SECRET_KEY").ok())?;
    let db = Arc::new(Db::open(&config.data_dir().join("board.db")).map_err(|e| e.to_string())?);
    let ports = Ports {
        settings: db.clone(),
        records: db.clone(),
        captures: Arc::new(CaptureStore::new(
            db.clone(),
            config.data_dir().join("captures"),
        )),
        comments: Arc::new(CommentStore::new(db, config.data_dir().join("comments"))),
        renderer: Arc::new(RenderClient::new(
            &config.render_url,
            config.render_token.clone(),
        )),
        store: Arc::new(GitStore::new(config.data_dir().join("projects"))),
        workspace: Arc::new(WorkspaceClient::new(
            &config.workspace_url,
            config.workspace_token.clone(),
        )),
    };
    let engine = Engine::new(ports, config.local_mode()).with_preview(PreviewSettings {
        origin: config.preview_url.clone(),
        render_origin: config.render_preview_url.clone(),
        secret,
        ttl_secs: PREVIEW_TOKEN_SECS,
    });
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .map_err(|e| format!("cannot listen on {}: {e}", config.bind))?;
    let preview_listener = tokio::net::TcpListener::bind(config.preview_bind)
        .await
        .map_err(|e| format!("cannot listen on {}: {e}", config.preview_bind))?;
    println!(
        "board-server {} listening on {} ({}), previews on {}",
        board_model::VERSION,
        config.tool_url(),
        if config.local_mode() {
            "local mode: no sign-in"
        } else {
            "public"
        },
        config.preview_url,
    );
    let origin = board_api::PreviewOrigin {
        frame_ancestors: config.tool_origins(),
    };
    let previews = axum::serve(
        preview_listener,
        board_api::preview_router(engine.clone(), &origin),
    )
    .with_graceful_shutdown(shutdown());
    let api = axum::serve(listener, board_api::router(engine)).with_graceful_shutdown(shutdown());
    let (api, previews) = tokio::join!(api.into_future(), previews.into_future());
    api.and(previews).map_err(|e| e.to_string())
}

/// Preview tokens last an hour; the tool asks for a fresh one when it reopens a preview.
const PREVIEW_TOKEN_SECS: u64 = 3600;

/// Resolves on Ctrl-C or SIGTERM (`docker stop`).
async fn shutdown() {
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        () = term => {}
    }
}
