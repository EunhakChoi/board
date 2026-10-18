//! Server settings from flags and environment variables.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

/// Default address: loopback only, which means local mode.
const DEFAULT_BIND: &str = "127.0.0.1:5280";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Where board keeps its data (`BOARD_HOME`, default `~/.board`).
    pub home: PathBuf,
    /// Listen address (`BOARD_BIND`).
    pub bind: SocketAddr,
    /// `--public`: reachable from other machines, sign-in required.
    pub public: bool,
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
        if !public && !bind.ip().is_loopback() {
            return Err(format!(
                "{bind} is reachable from other machines; pass --public to require sign-in"
            ));
        }
        if public {
            // Sign-in (OIDC) lands in P1. Until then a public server would be open to anyone.
            return Err("--public needs sign-in, which is not available yet".to_owned());
        }
        Ok(Self { home, bind, public })
    }

    /// Local mode: loopback only and not public. Every request acts as the built-in admin.
    pub fn local_mode(&self) -> bool {
        !self.public && self.bind.ip().is_loopback()
    }

    pub fn data_dir(&self) -> PathBuf {
        self.home.join("data")
    }
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
