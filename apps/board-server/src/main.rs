//! board-server: the board server binary.

mod config;

use std::process::ExitCode;
use std::sync::Arc;

use board_db::Db;
use board_engine::{Engine, Ports};
use board_store::GitStore;

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
    let db = Arc::new(Db::open(&config.data_dir().join("board.db")).map_err(|e| e.to_string())?);
    let ports = Ports {
        settings: db.clone(),
        records: db,
        store: Arc::new(GitStore::new(config.data_dir().join("projects"))),
    };
    let engine = Engine::new(ports, config.local_mode());
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .map_err(|e| format!("cannot listen on {}: {e}", config.bind))?;
    println!(
        "board-server {} listening on http://{} ({})",
        board_model::VERSION,
        config.bind,
        if config.local_mode() {
            "local mode: no sign-in"
        } else {
            "public"
        }
    );
    axum::serve(listener, board_api::router(engine))
        .with_graceful_shutdown(shutdown())
        .await
        .map_err(|e| e.to_string())
}

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
