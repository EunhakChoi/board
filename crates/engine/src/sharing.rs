//! Sharing: opening a local board to other machines from the web tool, without a restart.
//!
//! A local board has two doors. The local door (loopback) acts as the owner, this machine's
//! admin. The remote door treats everyone the same, by token; tunnels and reverse proxies point
//! only at it, so a request that came through one can never act as the owner. Sharing only
//! decides which tunnel runs and which address links use.

use std::sync::Arc;

use board_model::Actor;
use serde::{Deserialize, Serialize};

use crate::members::admin;
use crate::{Engine, EngineError};

/// How other machines reach the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShareMode {
    #[default]
    /// An address the person set up themselves (a reverse proxy or tunnel to the remote door).
    Address,
    /// `tailscale serve`: people on the same tailnet (Tailscale or Headscale).
    Tailscale,
    /// `tailscale funnel`: anyone on the internet, at the machine's `ts.net` name.
    Funnel,
    /// An ngrok tunnel run inside the server, with the person's authtoken.
    Ngrok,
    /// The remote door also listens on this machine's LAN address (for trying things out: the
    /// address is an IP, over plain HTTP).
    Lan,
}

impl ShareMode {
    /// Run by the server through [`crate::Tunnels`], rather than set up by hand.
    fn tunnel(self) -> bool {
        self != Self::Address
    }
}

/// Sharing that is on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sharing {
    pub mode: ShareMode,
    /// The address people open, without a trailing slash.
    pub url: String,
    /// The domain asked for (ngrok), kept to reopen the same address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
}

/// What the sharing page shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharingStatus {
    /// Set by the server's environment (`--public`): the page only shows it.
    pub fixed: bool,
    pub sharing: Option<Sharing>,
    /// The ways this machine can share.
    pub modes: Vec<crate::TunnelMode>,
    /// Something that went wrong with the tunnel (at start, or while turning sharing off).
    pub warning: Option<String>,
    /// The last characters of the saved ngrok authtoken, when one is saved.
    pub ngrok_token: Option<String>,
}

/// A request to turn sharing on.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct ShareRequest {
    pub mode: ShareMode,
    pub url: String,
    /// ngrok: a new authtoken (else the saved one), and a domain of theirs (else ngrok's).
    pub authtoken: Option<String>,
    pub domain: Option<String>,
}

impl std::fmt::Debug for ShareRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShareRequest")
            .field("mode", &self.mode)
            .field("url", &self.url)
            .field("domain", &self.domain)
            .finish_non_exhaustive()
    }
}

/// The parts of the engine that change while it runs.
#[derive(Default)]
pub(crate) struct Live {
    /// The address from the server's settings (`http://localhost:5280` on a local board).
    pub home_url: Option<Arc<str>>,
    pub sharing: Option<Sharing>,
    /// Who the local door acts as.
    pub owner: Option<Actor>,
    pub warning: Option<String>,
}

const SHARING: &str = "sharing";
/// The ngrok authtoken. Never sent back; the page sees its last characters.
const NGROK_TOKEN: &str = "sharing.ngrok.authtoken";

impl Engine {
    /// Picks up the saved sharing at start.
    pub fn restore_sharing(&self) -> Result<(), EngineError> {
        if !self.local_mode {
            return Ok(());
        }
        let mut sharing = match self.ports.settings.get(SHARING)? {
            Some(text) => serde_json::from_str::<Sharing>(&text).ok(),
            None => None,
        };
        // A tunnel is opened again: it may have stopped, or its address changed.
        let mut warning = None;
        if let Some(on) = sharing.as_mut().filter(|s| s.mode.tunnel()) {
            let options = crate::TunnelOptions {
                authtoken: self.saved_token()?,
                domain: on.domain.clone(),
            };
            match self.open_tunnel(on.mode, &options) {
                Ok(url) => on.url = url,
                Err(error) => warning = Some(error.to_string()),
            }
        }
        self.write_live(|live| {
            live.sharing = sharing;
            live.warning = warning;
        });
        Ok(())
    }

    pub fn sharing(&self, actor: &Actor) -> Result<SharingStatus, EngineError> {
        admin(actor)?;
        let mut modes = vec![crate::TunnelMode {
            mode: ShareMode::Address,
            available: true,
            detail: String::new(),
        }];
        if self.local_mode
            && let Some(tunnels) = &self.tunnels
        {
            modes.extend(tunnels.modes());
        }
        let ngrok_token = self.saved_token()?.map(|t| {
            let tail: String = t.chars().rev().take(4).collect();
            tail.chars().rev().collect()
        });
        let live = self.read_live();
        Ok(SharingStatus {
            ngrok_token,
            modes,
            warning: live.warning.clone(),
            fixed: !self.local_mode,
            sharing: live.sharing.clone(),
        })
    }

    /// Turns sharing on (or changes its address). Admins only.
    pub fn start_sharing(
        &self,
        actor: &Actor,
        request: &ShareRequest,
    ) -> Result<SharingStatus, EngineError> {
        self.check_changeable(actor)?;
        let current = self.read_live().sharing.clone();
        let domain = match request.domain.as_deref().map(str::trim) {
            Some(d) if !d.is_empty() && request.mode == ShareMode::Ngrok => Some(full_domain(d)?),
            _ => None,
        };
        let new_token = request
            .authtoken
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned);
        let url = if request.mode.tunnel() {
            // Another tunnel that was on gives way first.
            if let Some(old) = current.filter(|c| c.mode.tunnel() && c.mode != request.mode) {
                let _ = self.close_tunnel(old.mode);
            }
            let options = crate::TunnelOptions {
                authtoken: new_token.clone().or(self.saved_token()?),
                domain: domain.clone(),
            };
            let url = self.open_tunnel(request.mode, &options)?;
            if let Some(token) = &new_token {
                self.ports.settings.set(NGROK_TOKEN, token)?;
            }
            url
        } else {
            let url = request.url.trim().trim_end_matches('/').to_owned();
            if !(url.starts_with("https://") || url.starts_with("http://")) || url.contains(' ') {
                return Err(EngineError::Invalid(
                    "the address is an http(s) URL, e.g. https://board.example.com".into(),
                ));
            }
            if let Some(old) = current.filter(|c| c.mode.tunnel()) {
                let _ = self.close_tunnel(old.mode);
            }
            url
        };
        let sharing = Sharing {
            mode: request.mode,
            url,
            domain: domain.filter(|_| request.mode == ShareMode::Ngrok),
        };
        let text =
            serde_json::to_string(&sharing).map_err(|e| EngineError::Invalid(e.to_string()))?;
        self.ports.settings.set(SHARING, &text)?;
        self.write_live(|live| {
            live.sharing = Some(sharing);
            live.warning = None;
        });
        self.sharing(actor)
    }

    /// Turns sharing off: the tunnel closes and links use this machine's address again.
    pub fn stop_sharing(&self, actor: &Actor) -> Result<SharingStatus, EngineError> {
        self.check_changeable(actor)?;
        let current = self.read_live().sharing.clone();
        self.ports.settings.set(SHARING, "")?;
        // The remote door closes either way; a tunnel that would not stop is only reported.
        let warning = current
            .filter(|c| c.mode.tunnel())
            .and_then(|c| self.close_tunnel(c.mode).err())
            .map(|e| e.to_string());
        self.write_live(|live| {
            live.sharing = None;
            live.warning = warning;
        });
        self.sharing(actor)
    }

    fn saved_token(&self) -> Result<Option<String>, EngineError> {
        Ok(self
            .ports
            .settings
            .get(NGROK_TOKEN)?
            .filter(|t| !t.is_empty()))
    }

    fn open_tunnel(
        &self,
        mode: ShareMode,
        options: &crate::TunnelOptions,
    ) -> Result<String, EngineError> {
        let tunnels = self
            .tunnels
            .as_ref()
            .ok_or_else(|| EngineError::Unavailable("this server cannot run tunnels".into()))?;
        let url = tunnels
            .open(mode, options)
            .map_err(EngineError::Unavailable)?;
        Ok(url.trim_end_matches('/').to_owned())
    }

    fn close_tunnel(&self, mode: ShareMode) -> Result<(), EngineError> {
        match &self.tunnels {
            Some(tunnels) => tunnels.close(mode).map_err(EngineError::Unavailable),
            None => Ok(()),
        }
    }

    fn check_changeable(&self, actor: &Actor) -> Result<(), EngineError> {
        admin(actor)?;
        if !self.local_mode {
            return Err(EngineError::Invalid(
                "sharing is set by the server's environment (--public)".into(),
            ));
        }
        Ok(())
    }
}

/// A domain as ngrok wants it: the full name (`team.ngrok.app`), also when pasted as a URL.
fn full_domain(text: &str) -> Result<String, EngineError> {
    let name = text
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_lowercase();
    let valid = name.contains('.')
        && !name.starts_with('.')
        && !name.ends_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.');
    if valid {
        Ok(name)
    } else {
        Err(EngineError::Invalid(format!(
            "the domain is a full name, e.g. {}.ngrok.app (reserve it on the ngrok dashboard first)",
            if name.is_empty() || name.contains('.') {
                "my-team"
            } else {
                &name
            }
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::engine;

    fn request(url: &str) -> ShareRequest {
        ShareRequest {
            mode: ShareMode::Address,
            url: url.into(),
            ..ShareRequest::default()
        }
    }

    #[test]
    fn sharing_sets_the_address_admins_only() {
        let engine = engine().with_tool_url("http://localhost:5280");
        assert!(matches!(
            engine.start_sharing(&Actor::Local, &request("ftp://x")),
            Err(EngineError::Invalid(_))
        ));
        let status = engine
            .start_sharing(&Actor::Local, &request("https://b.example.com/"))
            .unwrap();
        assert_eq!(status.sharing.unwrap().url, "https://b.example.com");
        assert_eq!(engine.tool_url().as_deref(), Some("https://b.example.com"));

        let eve = engine
            .add_member(&Actor::Local, "Eve", board_model::Role::Editor)
            .unwrap()
            .user
            .actor();
        assert_eq!(
            engine
                .start_sharing(&eve, &request("https://evil.example"))
                .map(|_| ()),
            Err(EngineError::Forbidden)
        );

        engine.stop_sharing(&Actor::Local).unwrap();
        assert_eq!(engine.tool_url().as_deref(), Some("http://localhost:5280"));
    }

    /// Tailscale that hands out one name and remembers what is open.
    #[derive(Default)]
    struct FakeTunnels(std::sync::Mutex<Vec<ShareMode>>);

    impl crate::Tunnels for FakeTunnels {
        fn modes(&self) -> Vec<crate::TunnelMode> {
            vec![crate::TunnelMode {
                mode: ShareMode::Tailscale,
                available: true,
                detail: "https://kim.tail1.ts.net".into(),
            }]
        }
        fn open(&self, mode: ShareMode, options: &crate::TunnelOptions) -> Result<String, String> {
            if mode == ShareMode::Funnel {
                return Err("Funnel is not enabled: https://login.tailscale.com/f/funnel".into());
            }
            if mode == ShareMode::Ngrok {
                let token = options
                    .authtoken
                    .as_deref()
                    .ok_or("an ngrok authtoken, please")?;
                assert_eq!(token, "tok_secret_1234");
                self.0.lock().unwrap().push(mode);
                return Ok(format!(
                    "https://{}/",
                    options.domain.as_deref().unwrap_or("random.ngrok-free.app")
                ));
            }
            self.0.lock().unwrap().push(mode);
            Ok("https://kim.tail1.ts.net/".into())
        }
        fn close(&self, mode: ShareMode) -> Result<(), String> {
            self.0.lock().unwrap().retain(|m| *m != mode);
            Ok(())
        }
    }

    #[test]
    fn tunnels_give_the_address_and_close_when_sharing_stops() {
        let tunnels = Arc::new(FakeTunnels::default());
        let engine = engine().with_tunnels(tunnels.clone());
        let mut on = request("");
        on.mode = ShareMode::Funnel;
        assert!(matches!(
            engine.start_sharing(&Actor::Local, &on),
            Err(EngineError::Unavailable(m)) if m.contains("login.tailscale.com")
        ));
        on.mode = ShareMode::Tailscale;
        let status = engine.start_sharing(&Actor::Local, &on).unwrap();
        assert_eq!(status.sharing.unwrap().url, "https://kim.tail1.ts.net");
        assert_eq!(status.modes.len(), 2);
        assert_eq!(*tunnels.0.lock().unwrap(), vec![ShareMode::Tailscale]);
        engine.stop_sharing(&Actor::Local).unwrap();
        assert!(tunnels.0.lock().unwrap().is_empty());
    }

    #[test]
    fn ngrok_keeps_the_authtoken_out_of_sight_and_reopens_its_domain() {
        let tunnels = Arc::new(FakeTunnels::default());
        let engine = engine().with_tunnels(tunnels.clone());
        let mut on = request("");
        on.mode = ShareMode::Ngrok;
        assert!(
            engine.start_sharing(&Actor::Local, &on).is_err(),
            "no token yet"
        );
        on.authtoken = Some(" tok_secret_1234 ".into());
        on.domain = Some("kim.ngrok.app".into());
        let status = engine.start_sharing(&Actor::Local, &on).unwrap();
        assert_eq!(status.sharing.unwrap().url, "https://kim.ngrok.app");
        assert_eq!(status.ngrok_token.as_deref(), Some("1234"));
        assert!(!format!("{on:?}").contains("secret"));

        // After a restart the saved token and domain open the same address.
        engine.write_live(|live| live.sharing = None);
        engine.restore_sharing().unwrap();
        let status = engine.sharing(&Actor::Local).unwrap();
        assert_eq!(status.sharing.unwrap().url, "https://kim.ngrok.app");
        assert_eq!(status.warning, None);
    }

    #[test]
    fn a_public_board_takes_sharing_from_its_environment() {
        let engine = crate::testing::public_engine();
        assert!(engine.sharing(&Actor::Local).unwrap().fixed);
        assert!(matches!(
            engine.start_sharing(&Actor::Local, &request("https://b.example.com")),
            Err(EngineError::Invalid(_))
        ));
    }

    #[test]
    fn an_ngrok_domain_is_a_full_name() {
        assert_eq!(
            full_domain("https://Kim.ngrok.app/").unwrap(),
            "kim.ngrok.app"
        );
        let Err(EngineError::Invalid(message)) = full_domain("aigen-test") else {
            panic!("a bare name");
        };
        assert!(message.contains("aigen-test.ngrok.app"), "{message}");
        assert!(full_domain("a b.ngrok.app").is_err());
    }

    #[test]
    fn sharing_survives_a_restart() {
        let engine = engine();
        engine
            .start_sharing(&Actor::Local, &request("https://b.example.com"))
            .unwrap();
        engine.write_live(|live| live.sharing = None);
        engine.restore_sharing().unwrap();
        assert_eq!(engine.tool_url().as_deref(), Some("https://b.example.com"));
    }
}
