//! board-server: the board server binary.

mod config;
mod secret;
mod update;
mod webhook;

use std::process::ExitCode;
use std::sync::Arc;

use board_collab::Hub;
use board_db::{CaptureStore, CommentStore, Db};
use board_engine::{Engine, Ports, PreviewSettings, SignInSettings};
use board_oidc::Oidc;
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
        comments: Arc::new(CommentStore::new(
            db.clone(),
            config.data_dir().join("comments"),
        )),
        accounts: db.clone(),
        notices: db.clone(),
        invites: db,
        webhooks: Arc::new(webhook::Sender::new()),
        realtime: Arc::new(Hub::new()),
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
    let mut engine = Engine::new(ports, config.local_mode())
        .with_preview(PreviewSettings {
            origin: config.preview_url.clone(),
            render_origin: config.render_preview_url.clone(),
            secret,
            ttl_secs: PREVIEW_TOKEN_SECS,
        })
        .with_tool_url(&config.tool_url())
        .with_secret(secret);
    let mut setup_link = None;
    // A public server always signs people in: invite links, plus the provider when set up.
    if config.oidc.is_some() || !config.local_mode() {
        let identity = config.oidc.as_ref().map(|oidc| {
            Arc::new(Oidc::new(
                &oidc.issuer,
                &oidc.client_id,
                &oidc.client_secret,
                &config.oidc_redirect(),
            )) as Arc<dyn board_engine::Identity>
        });
        engine = engine.with_sign_in(SignInSettings {
            identity,
            admin_emails: config.admin_emails.clone(),
        });
        if !config.local_mode() {
            setup_link = engine
                .issue_setup_token()
                .map_err(|e| e.to_string())?
                .map(|token| format!("{}/setup?token={token}", config.tool_url()));
        }
    }
    if let Some(addr) = config.remote_bind {
        engine = engine.with_tunnels(Arc::new(tailscale(addr)));
    }
    engine.restore_sharing().map_err(|e| e.to_string())?;
    if let Ok(status) = engine.sharing(&board_model::Actor::Local)
        && let Some(warning) = status.warning
    {
        eprintln!("board-server: sharing: {warning}");
    }
    if config.update_check {
        update::spawn(engine.clone());
    }
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .map_err(|e| format!("cannot listen on {}: {e}", config.bind))?;
    let preview_listener = tokio::net::TcpListener::bind(config.preview_bind)
        .await
        .map_err(|e| format!("cannot listen on {}: {e}", config.preview_bind))?;
    announce(&config, setup_link);
    let origin = board_api::PreviewOrigin {
        frame_ancestors: config.tool_origins(),
        sandbox: false,
    };
    let remote = serve_remote(
        config.remote_bind,
        board_api::remote_app(engine.clone(), config.web_dir.clone()),
    )
    .await?;
    let previews = axum::serve(
        preview_listener,
        board_api::preview_router(engine.clone(), &origin),
    )
    .with_graceful_shutdown(shutdown());
    let api = axum::serve(listener, board_api::app(engine, config.web_dir.clone()))
        .with_graceful_shutdown(shutdown());
    let (api, previews, remote) = tokio::join!(api.into_future(), previews.into_future(), remote);
    api.and(previews).and(remote).map_err(|e| e.to_string())
}

/// The tailscale CLI, sending people to the remote door at `addr`.
fn tailscale(addr: std::net::SocketAddr) -> board_tunnel::Tailscale {
    board_tunnel::Tailscale {
        cli: std::env::var("BOARD_TAILSCALE").unwrap_or_else(|_| "tailscale".to_owned()),
        socket: std::env::var("BOARD_TAILSCALE_SOCKET").ok(),
        // tailscaled runs on the host: in docker the remote door is published on its loopback.
        target: std::env::var("BOARD_TAILSCALE_TARGET")
            .unwrap_or_else(|_| format!("http://127.0.0.1:{}", addr.port())),
    }
}

/// Says where the server listens, and prints the setup link while there is no admin.
fn announce(config: &Config, setup_link: Option<String>) {
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
    if let Some(addr) = config.remote_bind {
        println!(
            "Remote door on {addr}: point a tunnel or reverse proxy here, then turn sharing on in Settings."
        );
    }
    if let Some(link) = setup_link {
        // Printed only here: whoever opens it first becomes the admin.
        println!("No admin yet. Open this one-time setup link to become the admin:\n  {link}");
    }
}

/// Listens on the remote door, when there is one; the future runs until shutdown.
async fn serve_remote(
    addr: Option<std::net::SocketAddr>,
    app: axum::Router,
) -> Result<impl Future<Output = std::io::Result<()>>, String> {
    let listener = match addr {
        Some(addr) => Some(
            tokio::net::TcpListener::bind(addr)
                .await
                .map_err(|e| format!("cannot listen on {addr}: {e}"))?,
        ),
        None => None,
    };
    Ok(async move {
        match listener {
            Some(listener) => {
                axum::serve(listener, app)
                    .with_graceful_shutdown(shutdown())
                    .await
            }
            None => Ok(()),
        }
    })
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
