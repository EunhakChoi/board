//! Server settings from flags and environment variables.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

/// Default address: loopback only, which means local mode.
const DEFAULT_BIND: &str = "127.0.0.1:5280";
/// Default preview origin: same machine, other port. The tool is opened as `localhost`, previews as
/// `127.0.0.1`, so the two never share cookies.
const DEFAULT_PREVIEW_BIND: &str = "127.0.0.1:5281";
const DEFAULT_WORKSPACE_URL: &str = "http://127.0.0.1:5282";
const DEFAULT_RENDER_URL: &str = "http://127.0.0.1:5283";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Where board keeps its data (`BOARD_HOME`, default `~/.board`).
    pub home: PathBuf,
    /// Listen address (`BOARD_BIND`).
    pub bind: SocketAddr,
    /// `--public`: reachable from other machines, sign-in required.
    pub public: bool,
    /// Preview listener (`BOARD_PREVIEW_BIND`).
    pub preview_bind: SocketAddr,
    /// Preview origin as browsers reach it (`BOARD_PREVIEW_URL`, default `http://<preview bind>`).
    pub preview_url: String,
    /// `board-workspace` (`BOARD_WORKSPACE_URL`) and the token it expects (`BOARD_WORKSPACE_TOKEN`).
    pub workspace_url: String,
    pub workspace_token: Option<String>,
    /// `board-render` (`BOARD_RENDER_URL`) and its token (`BOARD_RENDER_TOKEN`).
    pub render_url: String,
    pub render_token: Option<String>,
    /// The preview origin as `board-render` reaches it (`BOARD_RENDER_PREVIEW_URL`, default
    /// `preview_url`; differs inside docker).
    pub render_preview_url: String,
    /// The built web tool (`BOARD_WEB_DIR`); found next to the binary when unset.
    pub web_dir: Option<PathBuf>,
    /// The tool's address as browsers reach it (`BOARD_PUBLIC_URL`), e.g. behind a tunnel.
    pub public_url: Option<String>,
    /// Daily check for a new release (`BOARD_UPDATE_CHECK=false` turns it off).
    pub update_check: bool,
}

impl Config {
    /// Reads `--public` from `args` and `BOARD_HOME` / `BOARD_BIND` / `HOME` through `env`.
    pub fn from_env(
        args: impl IntoIterator<Item = String>,
        env: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, String> {
        let mut public = false;
        for arg in args {
            match arg.as_str() {
                "--public" => public = true,
                other => {
                    return Err(format!(
                        "unknown option {other} (usage: board-server [--public])"
                    ));
                }
            }
        }
        let home = match env("BOARD_HOME") {
            Some(home) => PathBuf::from(home),
            None => env("HOME")
                .map(|home| Path::new(&home).join(".board"))
                .ok_or("set BOARD_HOME or HOME")?,
        };
        let bind_text = env("BOARD_BIND").unwrap_or_else(|| DEFAULT_BIND.to_owned());
        let bind: SocketAddr = bind_text.parse().map_err(|_| {
            format!("BOARD_BIND {bind_text:?} is not an address like 127.0.0.1:5280")
        })?;
        let preview_text =
            env("BOARD_PREVIEW_BIND").unwrap_or_else(|| DEFAULT_PREVIEW_BIND.to_owned());
        let preview_bind: SocketAddr = preview_text.parse().map_err(|_| {
            format!("BOARD_PREVIEW_BIND {preview_text:?} is not an address like 127.0.0.1:5281")
        })?;
        for addr in [bind, preview_bind] {
            if !public && !addr.ip().is_loopback() {
                return Err(format!(
                    "{addr} is reachable from other machines; pass --public to require sign-in"
                ));
            }
        }
        if preview_bind == bind {
            return Err("BOARD_PREVIEW_BIND must differ from BOARD_BIND".to_owned());
        }
        let preview_url = env("BOARD_PREVIEW_URL")
            .unwrap_or_else(|| format!("http://{preview_bind}"))
            .trim_end_matches('/')
            .to_owned();
        if !(preview_url.starts_with("http://") || preview_url.starts_with("https://")) {
            return Err(format!(
                "BOARD_PREVIEW_URL {preview_url:?} must be an http(s) URL"
            ));
        }
        let workspace_url =
            env("BOARD_WORKSPACE_URL").unwrap_or_else(|| DEFAULT_WORKSPACE_URL.to_owned());
        let workspace_token = env("BOARD_WORKSPACE_TOKEN").filter(|t| !t.is_empty());
        let render_url = env("BOARD_RENDER_URL").unwrap_or_else(|| DEFAULT_RENDER_URL.to_owned());
        let render_token = env("BOARD_RENDER_TOKEN").filter(|t| !t.is_empty());
        let render_preview_url = env("BOARD_RENDER_PREVIEW_URL").map_or_else(
            || preview_url.clone(),
            |u| u.trim_end_matches('/').to_owned(),
        );
        let web_dir = env("BOARD_WEB_DIR")
            .map(PathBuf::from)
            .or_else(find_web_dir);
        let public_url = env("BOARD_PUBLIC_URL").map(|u| u.trim_end_matches('/').to_owned());
        if let Some(url) = &public_url
            && !(url.starts_with("http://") || url.starts_with("https://"))
        {
            return Err(format!("BOARD_PUBLIC_URL {url:?} must be an http(s) URL"));
        }
        let update_check = !matches!(
            env("BOARD_UPDATE_CHECK").as_deref().map(str::trim),
            Some("false" | "0" | "off" | "no")
        );
        if public {
            // Sign-in (OIDC) lands in P1. Until then a public server would be open to anyone.
            return Err("--public needs sign-in, which is not available yet".to_owned());
        }
        Ok(Self {
            home,
            bind,
            public,
            preview_bind,
            preview_url,
            workspace_url,
            workspace_token,
            render_url,
            render_token,
            render_preview_url,
            web_dir,
            public_url,
            update_check,
        })
    }

    /// Origins the tool is opened at, which may frame previews.
    pub fn tool_origins(&self) -> Vec<String> {
        if let Some(url) = &self.public_url {
            return vec![url.clone()];
        }
        let port = self.bind.port();
        if self.bind.ip().is_loopback() {
            vec![
                format!("http://localhost:{port}"),
                format!("http://127.0.0.1:{port}"),
            ]
        } else {
            vec![format!("http://{}", self.bind)]
        }
    }

    /// Where people open the tool.
    pub fn tool_url(&self) -> String {
        self.public_url
            .clone()
            .unwrap_or_else(|| format!("http://localhost:{}", self.bind.port()))
    }

    /// Local mode: loopback only and not public. Every request acts as the built-in admin.
    pub fn local_mode(&self) -> bool {
        !self.public && self.bind.ip().is_loopback()
    }

    pub fn data_dir(&self) -> PathBuf {
        self.home.join("data")
    }
}

/// `share/board/web` next to an installed binary, or `apps/board-web/dist` when run from a checkout
/// (`target/<profile>/board-server`).
fn find_web_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let bin = exe.parent()?;
    [
        bin.join("../share/board/web"),
        bin.join("../../apps/board-web/dist"),
    ]
    .into_iter()
    .find(|dir| dir.join("index.html").is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn config(args: &[&str], env: &[(&str, &str)]) -> Result<Config, String> {
        let env: HashMap<String, String> = env
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        Config::from_env(args.iter().map(|a| (*a).to_owned()), |key| {
            env.get(key).cloned()
        })
    }

    #[test]
    fn defaults_to_local_mode_under_home() {
        let config = config(&[], &[("HOME", "/home/me")]).unwrap();
        assert_eq!(config.home, PathBuf::from("/home/me/.board"));
        assert_eq!(config.bind, DEFAULT_BIND.parse().unwrap());
        assert!(config.local_mode());
        assert_eq!(config.data_dir(), PathBuf::from("/home/me/.board/data"));
        assert_eq!(config.preview_bind, DEFAULT_PREVIEW_BIND.parse().unwrap());
        assert_eq!(config.preview_url, "http://127.0.0.1:5281");
        assert_eq!(config.workspace_url, DEFAULT_WORKSPACE_URL);
        assert_eq!(config.workspace_token, None);
        assert_eq!(config.render_url, DEFAULT_RENDER_URL);
        assert_eq!(config.render_preview_url, "http://127.0.0.1:5281");
        assert_eq!(
            config.tool_origins(),
            ["http://localhost:5280", "http://127.0.0.1:5280"]
        );
    }

    #[test]
    fn preview_settings() {
        let config = config(
            &[],
            &[
                ("HOME", "/h"),
                ("BOARD_PREVIEW_BIND", "127.0.0.1:6001"),
                ("BOARD_PREVIEW_URL", "https://preview.example.com/"),
                ("BOARD_WORKSPACE_TOKEN", "s"),
                ("BOARD_PUBLIC_URL", "https://board.example.com/"),
            ],
        )
        .unwrap();
        assert_eq!(config.tool_url(), "https://board.example.com");
        assert_eq!(config.tool_origins(), ["https://board.example.com"]);
        assert_eq!(config.preview_bind.port(), 6001);
        assert_eq!(config.preview_url, "https://preview.example.com");
        assert_eq!(config.workspace_token.as_deref(), Some("s"));
        let same = [("HOME", "/h"), ("BOARD_PREVIEW_BIND", "127.0.0.1:5280")];
        assert!(config_err(&same).contains("differ"));
        let open = [("HOME", "/h"), ("BOARD_PREVIEW_BIND", "0.0.0.0:5281")];
        assert!(config_err(&open).contains("--public"));
        let url = [("HOME", "/h"), ("BOARD_PREVIEW_URL", "preview")];
        assert!(config_err(&url).contains("http"));
    }

    fn config_err(env: &[(&str, &str)]) -> String {
        config(&[], env).unwrap_err()
    }

    #[test]
    fn board_home_wins() {
        let config = config(&[], &[("HOME", "/home/me"), ("BOARD_HOME", "/srv/board")]).unwrap();
        assert_eq!(config.home, PathBuf::from("/srv/board"));
    }

    #[test]
    fn refuses_an_open_bind_without_public() {
        let error = config(&[], &[("HOME", "/h"), ("BOARD_BIND", "0.0.0.0:5280")]).unwrap_err();
        assert!(error.contains("--public"));
    }

    #[test]
    fn public_waits_for_sign_in() {
        let error = config(&["--public"], &[("HOME", "/h")]).unwrap_err();
        assert!(error.contains("sign-in"));
    }

    #[test]
    fn rejects_bad_input() {
        assert!(config(&["--nope"], &[("HOME", "/h")]).is_err());
        assert!(config(&[], &[("HOME", "/h"), ("BOARD_BIND", "nowhere")]).is_err());
        assert!(config(&[], &[]).is_err());
    }
}
