//! Server settings from flags and environment variables.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

/// Default address: loopback only, which means local mode.
const DEFAULT_BIND: &str = "127.0.0.1:5280";
/// Default preview origin: same machine, other port. The tool is opened as `localhost`, previews as
/// `127.0.0.1`, so the two never share cookies.
const DEFAULT_PREVIEW_BIND: &str = "127.0.0.1:5281";
/// Default remote door of a local board: what tunnels and reverse proxies point at once sharing
/// is on. Nobody is the owner there: members bring their token.
const DEFAULT_REMOTE_BIND: &str = "127.0.0.1:5284";
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
    /// `BOARD_CONTAINER=true`: listening on every container address is still local mode. Only for
    /// containers whose ports are published on the host's loopback (`127.0.0.1:5280:5280`).
    pub container: bool,
    /// The remote door of a local board (`BOARD_REMOTE_BIND`); none on a public server.
    pub remote_bind: Option<SocketAddr>,
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
                        "unknown option {other} (usage: board-server [admin-token] [--public])"
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
        let container = flag(env("BOARD_CONTAINER").as_deref());
        for addr in [bind, preview_bind] {
            if !public && !container && !addr.ip().is_loopback() {
                return Err(format!(
                    "{addr} is reachable from other machines; pass --public to require a token"
                ));
            }
        }
        if preview_bind == bind {
            return Err("BOARD_PREVIEW_BIND must differ from BOARD_BIND".to_owned());
        }
        let remote_bind = remote_bind(&env, public, bind, preview_bind)?;
        let public_url = env("BOARD_PUBLIC_URL").map(|u| u.trim_end_matches('/').to_owned());
        if let Some(url) = &public_url
            && !(url.starts_with("http://") || url.starts_with("https://"))
        {
            return Err(format!("BOARD_PUBLIC_URL {url:?} must be an http(s) URL"));
        }
        let preview_url = preview_url(&env, public, public_url.as_deref(), preview_bind)?;
        let workspace_url =
            env("BOARD_WORKSPACE_URL").unwrap_or_else(|| DEFAULT_WORKSPACE_URL.to_owned());
        let workspace_token = secret(&env, "BOARD_WORKSPACE_TOKEN")?;
        let render_url = env("BOARD_RENDER_URL").unwrap_or_else(|| DEFAULT_RENDER_URL.to_owned());
        let render_token = secret(&env, "BOARD_RENDER_TOKEN")?;
        // board-render reaches the preview listener directly, also when people use the tool's
        // address for previews.
        let render_preview_url = env("BOARD_RENDER_PREVIEW_URL").map_or_else(
            || {
                if public_url.as_deref() == Some(preview_url.as_str()) {
                    format!("http://127.0.0.1:{}", preview_bind.port())
                } else {
                    preview_url.clone()
                }
            },
            |u| u.trim_end_matches('/').to_owned(),
        );
        let web_dir = env("BOARD_WEB_DIR")
            .map(PathBuf::from)
            .or_else(find_web_dir);
        let update_check = !matches!(
            env("BOARD_UPDATE_CHECK").as_deref().map(str::trim),
            Some("false" | "0" | "off" | "no")
        );
        if public {
            check_public(public_url.as_deref())?;
        }
        Ok(Self {
            home,
            bind,
            public,
            container,
            remote_bind,
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

    /// Local mode: loopback only and not public. The local door acts as the owner.
    pub fn local_mode(&self) -> bool {
        !self.public && (self.container || self.bind.ip().is_loopback())
    }

    pub fn data_dir(&self) -> PathBuf {
        self.home.join("data")
    }
}

/// Where browsers reach previews: `BOARD_PREVIEW_URL`; on a public server without one, the tool's
/// own address (it serves previews itself, sandboxed); else the preview listener.
fn preview_url(
    env: &impl Fn(&str) -> Option<String>,
    public: bool,
    public_url: Option<&str>,
    preview_bind: SocketAddr,
) -> Result<String, String> {
    let url = env("BOARD_PREVIEW_URL")
        .filter(|v| !v.trim().is_empty())
        .or_else(|| public.then(|| public_url.map(str::to_owned)).flatten())
        .unwrap_or_else(|| format!("http://{preview_bind}"))
        .trim_end_matches('/')
        .to_owned();
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(url)
    } else {
        Err(format!("BOARD_PREVIEW_URL {url:?} must be an http(s) URL"))
    }
}

/// The remote door of a local board (a public server has none: every request signs in).
fn remote_bind(
    env: &impl Fn(&str) -> Option<String>,
    public: bool,
    bind: SocketAddr,
    preview_bind: SocketAddr,
) -> Result<Option<SocketAddr>, String> {
    if public {
        return Ok(None);
    }
    let text = env("BOARD_REMOTE_BIND").unwrap_or_else(|| DEFAULT_REMOTE_BIND.to_owned());
    let addr: SocketAddr = text
        .parse()
        .map_err(|_| format!("BOARD_REMOTE_BIND {text:?} is not an address like 127.0.0.1:5284"))?;
    if addr == bind || addr == preview_bind {
        return Err("BOARD_REMOTE_BIND must differ from the other addresses".to_owned());
    }
    Ok(Some(addr))
}

/// A public server needs the address people open: links point there.
fn check_public(public_url: Option<&str>) -> Result<(), String> {
    if public_url.is_none() {
        return Err("--public needs BOARD_PUBLIC_URL, the fixed address people open".to_owned());
    }
    Ok(())
}

fn flag(value: Option<&str>) -> bool {
    matches!(value.map(str::trim), Some("true" | "1" | "yes" | "on"))
}

/// A secret from `NAME`, or from the file `NAME_FILE` names (docker secrets, shared volumes).
fn secret(env: &impl Fn(&str) -> Option<String>, name: &str) -> Result<Option<String>, String> {
    if let Some(value) = env(name).filter(|v| !v.is_empty()) {
        return Ok(Some(value));
    }
    let Some(file) = env(&format!("{name}_FILE")) else {
        return Ok(None);
    };
    let value = std::fs::read_to_string(&file)
        .map_err(|e| format!("{name}_FILE {file:?}: {e}"))?
        .trim()
        .to_owned();
    Ok((!value.is_empty()).then_some(value))
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
    fn containers_listen_everywhere_in_local_mode() {
        let dir = std::env::temp_dir().join(format!("board-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let token = dir.join("token");
        std::fs::write(&token, "abc\n").unwrap();
        let token = token.to_str().unwrap();
        let env = [
            ("HOME", "/h"),
            ("BOARD_BIND", "0.0.0.0:5280"),
            ("BOARD_PREVIEW_BIND", "0.0.0.0:5281"),
            ("BOARD_WORKSPACE_TOKEN_FILE", token),
            ("BOARD_RENDER_TOKEN_FILE", token),
        ];
        assert!(config(&[], &env).unwrap_err().contains("--public"));
        let config = config(
            &[],
            &[env.as_slice(), &[("BOARD_CONTAINER", "true")]].concat(),
        )
        .unwrap();
        assert!(config.local_mode());
        assert_eq!(config.workspace_token.as_deref(), Some("abc"));
        assert_eq!(config.render_token.as_deref(), Some("abc"));
        std::fs::remove_dir_all(&dir).unwrap();
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
    fn public_needs_a_fixed_address() {
        let error = config(&["--public"], &[("HOME", "/h")]).unwrap_err();
        assert!(error.contains("BOARD_PUBLIC_URL"));
        let public = config(
            &["--public"],
            &[
                ("HOME", "/h"),
                ("BOARD_PUBLIC_URL", "https://board.example.com"),
                ("BOARD_BIND", "0.0.0.0:5280"),
                ("BOARD_PREVIEW_BIND", "0.0.0.0:5281"),
            ],
        )
        .unwrap();
        assert!(!public.local_mode());
        // One address: previews come from the tool itself; board-render goes to the listener.
        assert_eq!(public.preview_url, "https://board.example.com");
        assert_eq!(public.render_preview_url, "http://127.0.0.1:5281");
    }

    #[test]
    fn rejects_bad_input() {
        assert!(config(&["--nope"], &[("HOME", "/h")]).is_err());
        assert!(config(&[], &[("HOME", "/h"), ("BOARD_BIND", "nowhere")]).is_err());
        assert!(config(&[], &[]).is_err());
    }
}
