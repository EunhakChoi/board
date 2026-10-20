//! board-server: the board server binary.

mod config;
mod lan;
mod secret;
mod update;
mod webhook;

use std::process::ExitCode;
use std::sync::Arc;

use board_collab::Hub;
use board_db::{CaptureStore, CommentStore, Db};
use board_engine::{Engine, Ports, PreviewSettings};
use board_render_client::RenderClient;
use board_store::GitStore;
use board_workspace_client::WorkspaceClient;

use crate::config::Config;

#[tokio::main]
async fn main() -> ExitCode {
    // `board-server admin-token`: print the owner's token (a public server's way in) and exit.
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let admin_token = args.first().is_some_and(|a| a == "admin-token");
    if admin_token {
        args.remove(0);
    }
    let config = match Config::from_env(args, |key| std::env::var(key).ok()) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("board-server: {message}");
            return ExitCode::FAILURE;
        }
    };
    match run(config, admin_token).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("board-server: {message}");
            ExitCode::FAILURE
        }
    }
}

async fn run(config: Config, admin_token: bool) -> Result<(), String> {
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
        notices: db,
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
    let engine = Engine::new(ports, config.local_mode())
        .with_preview(PreviewSettings {
            origin: config.preview_url.clone(),
            render_origin: config.render_preview_url.clone(),
            secret,
            ttl_secs: PREVIEW_TOKEN_SECS,
        })
        .with_tool_url(&config.tool_url())
        .with_secret(secret);
    // The owner: the local door acts as them; on a public server their token is the way in.
    let created = engine.ensure_owner().map_err(|e| e.to_string())?;
    if admin_token {
        let token = engine
            .owner_token()
            .map_err(|e| e.to_string())?
            .ok_or("the owner was removed")?;
        println!("{token}");
        return Ok(());
    }
    let first_token = created.filter(|_| !config.local_mode());
    let engine = with_sharing(engine, &config).await?;
    if config.update_check {
        update::spawn(engine.clone());
    }
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .map_err(|e| format!("cannot listen on {}: {e}", config.bind))?;
    let preview_listener = tokio::net::TcpListener::bind(config.preview_bind)
        .await
        .map_err(|e| format!("cannot listen on {}: {e}", config.preview_bind))?;
    announce(&config, first_token);
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

/// Lets a local board run tunnels to its remote door, and reopens sharing that was on.
async fn with_sharing(mut engine: Engine, config: &Config) -> Result<Engine, String> {
    let remote = config.remote_bind;
    if let Some(addr) = remote {
        let target = format!("http://127.0.0.1:{}", addr.port());
        let runtime = tokio::runtime::Handle::current();
        let machine = board_tunnel::Machine {
            tailscale: tailscale(&target),
            ngrok: board_tunnel::Ngrok::new(runtime.clone(), &target)?,
        };
        let lan = lan::Lan {
            runtime,
            port: addr.port(),
            app: board_api::remote_app(engine.clone(), config.web_dir.clone()),
            container: config.container,
            running: std::sync::Mutex::default(),
        };
        engine = engine.with_tunnels(Arc::new(lan::ServerTunnels { machine, lan }));
    }
    // Tunnels block on the runtime, so they reopen from a blocking thread.
    let restored = engine.clone();
    tokio::task::spawn_blocking(move || {
        restored.restore_sharing()?;
        restored.sharing(&board_model::Actor::Local)
    })
    .await
    .map_err(|e| e.to_string())?
    .map(|status| {
        if let Some(warning) = status.warning {
            eprintln!("board-server: sharing: {warning}");
        }
    })
    .map_err(|e| e.to_string())?;
    Ok(engine)
}

/// The tailscale CLI, sending people to the remote door at `target`.
fn tailscale(target: &str) -> board_tunnel::Tailscale {
    board_tunnel::Tailscale {
        cli: std::env::var("BOARD_TAILSCALE").unwrap_or_else(|_| "tailscale".to_owned()),
        socket: std::env::var("BOARD_TAILSCALE_SOCKET").ok(),
        // tailscaled runs on the host: in docker the remote door is published on its loopback.
        target: std::env::var("BOARD_TAILSCALE_TARGET").unwrap_or_else(|_| target.to_owned()),
    }
}

/// Says where the server listens, and prints a public server's admin token the first time.
fn announce(config: &Config, admin_token: Option<String>) {
    println!(
        "board-server {} listening on {} ({}), previews on {}",
        board_model::VERSION,
        config.tool_url(),
        if config.local_mode() {
            "local mode: this machine is the admin"
        } else {
            "public"
        },
        config.preview_url,
    );
    if let Some(addr) = config.remote_bind {
        println!(
            "Remote door on {addr}: tunnels and reverse proxies point here (members bring their token)."
        );
    }
    if let Some(token) = admin_token {
        // Printed once; `board-server admin-token` prints it again.
        println!(
            "Admin token (enter it in the web tool; `board-server admin-token` shows it again):\n  {token}"
        );
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
