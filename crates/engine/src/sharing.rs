//! Sharing: opening a local board to other machines from the web tool, without a restart.
//!
//! A local board has two doors. The local door (loopback) acts as this machine's admin, as
//! before. The remote door always signs people in; tunnels and reverse proxies point only at it,
//! so a request that came through one can never act as the local admin. Turning sharing on makes
//! an admin account for the person at this machine (the owner); the local door then acts as them.

use std::sync::Arc;

use board_model::{Actor, Role, UserStatus};
use serde::{Deserialize, Serialize};

use crate::access::admin;
use crate::{Engine, EngineError, SignInSettings};

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
    /// The account the local door acts as, once sharing was turned on.
    pub owner: Option<Owner>,
    /// The ways this machine can share.
    pub modes: Vec<crate::TunnelMode>,
    /// Something that went wrong with the tunnel (at start, or while turning sharing off).
    pub warning: Option<String>,
    /// The last characters of the saved ngrok authtoken, when one is saved.
    pub ngrok_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner {
    pub id: String,
    pub name: String,
}

/// A request to turn sharing on.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct ShareRequest {
    pub mode: ShareMode,
    pub url: String,
    /// The owner's name, needed the first time.
    pub owner_name: Option<String>,
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
    /// Sign-in from the server's settings (a public server, or a provider set up locally).
    pub base_sign_in: Option<Arc<SignInSettings>>,
    pub sharing: Option<Sharing>,
    /// Who the local door acts as.
    pub owner: Option<Actor>,
    pub warning: Option<String>,
}

const SHARING: &str = "sharing";
const OWNER: &str = "sharing.owner";
/// The ngrok authtoken. Never sent back; the page sees its last characters.
const NGROK_TOKEN: &str = "sharing.ngrok.authtoken";

impl Engine {
    /// Sign-in as it stands: the server's own, or invite links while sharing is on.
    pub(crate) fn sign_in(&self) -> Option<Arc<SignInSettings>> {
        let live = self.read_live();
        live.base_sign_in.clone().or_else(|| {
            live.sharing.as_ref().map(|_| {
                Arc::new(SignInSettings {
                    identity: None,
                    admin_emails: Vec::new(),
                })
            })
        })
    }

    /// Who a request through the local door acts as: the owner once there is one.
    pub fn local_actor(&self) -> Actor {
        self.read_live().owner.clone().unwrap_or(Actor::Local)
    }

    /// Whether the remote door lets people in: sharing is on, or the whole server is public.
    pub fn remote_open(&self) -> bool {
        !self.local_mode || self.read_live().sharing.is_some()
    }

    /// Picks up the saved owner and sharing at start.
    pub fn restore_sharing(&self) -> Result<(), EngineError> {
        if !self.local_mode {
            return Ok(());
        }
        let owner = self.owner_actor()?;
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
            live.owner = owner;
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
            owner: live.owner.as_ref().and_then(|o| match o {
                Actor::User { id, name, .. } => Some(Owner {
                    id: id.clone(),
                    name: name.clone(),
                }),
                _ => None,
            }),
        })
    }

    /// Turns sharing on (or changes its address). Admins at this machine only.
    pub fn start_sharing(
        &self,
        actor: &Actor,
        request: &ShareRequest,
    ) -> Result<SharingStatus, EngineError> {
        self.check_changeable(actor)?;
        let owner_name = match self.read_live().owner {
            Some(_) => None,
            None => Some(crate::invites::check_name(
                request.owner_name.as_deref().unwrap_or_default(),
            )?),
        };
        let current = self.read_live().sharing.clone();
        let domain = request
            .domain
            .as_deref()
            .map(str::trim)
            .filter(|d| !d.is_empty())
            .map(str::to_owned);
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
        if let Some(name) = owner_name {
            let user = self.insert_member(&name, Role::Admin)?;
            self.ports.settings.set(OWNER, &user.id)?;
            let owner = self.owner_actor()?;
            self.write_live(|live| live.owner = owner);
        }
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
        self.sharing(&self.local_actor())
    }

    /// Turns sharing off: the remote door closes. Accounts and sessions stay for next time.
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
        self.sharing(&self.local_actor())
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

    /// Only the person at this machine changes sharing (whoever comes through the local door):
    /// someone who came in through it could otherwise move the board to an address of theirs.
    fn check_changeable(&self, actor: &Actor) -> Result<(), EngineError> {
        if !self.local_mode {
            return Err(EngineError::Invalid(
                "sharing is set by the server's environment (--public)".into(),
            ));
        }
        if *actor != self.local_actor() {
            return Err(EngineError::Forbidden);
        }
        Ok(())
    }

    fn owner_actor(&self) -> Result<Option<Actor>, EngineError> {
        let Some(id) = self.ports.settings.get(OWNER)?.filter(|v| !v.is_empty()) else {
            return Ok(None);
        };
        Ok(self
            .ports
            .accounts
            .user(&id)?
            .filter(|u| u.status == UserStatus::Active)
            .map(|u| Actor::User {
                id: u.id,
                name: u.name,
                email: u.email,
                role: Role::Admin,
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::engine;

    fn request(url: &str, name: Option<&str>) -> ShareRequest {
        ShareRequest {
            mode: ShareMode::Address,
            url: url.into(),
            owner_name: name.map(str::to_owned),
            ..ShareRequest::default()
        }
    }

    #[test]
    fn sharing_makes_an_owner_and_opens_the_remote_door() {
        let engine = engine().with_tool_url("http://localhost:5280");
        assert!(!engine.remote_open());
        assert!(engine.sign_in().is_none());
        assert!(matches!(
            engine.start_sharing(&Actor::Local, &request("https://b.example.com/", None)),
            Err(EngineError::Invalid(_))
        ));
        assert!(matches!(
            engine.start_sharing(&Actor::Local, &request("ftp://x", Some("Kim"))),
            Err(EngineError::Invalid(_))
        ));
        let status = engine
            .start_sharing(
                &Actor::Local,
                &request("https://b.example.com/", Some("Kim")),
            )
            .unwrap();
        assert_eq!(status.sharing.unwrap().url, "https://b.example.com");
        assert_eq!(status.owner.unwrap().name, "Kim");
        assert!(engine.remote_open());
        assert!(engine.sign_in().is_some(), "invite links work");
        assert_eq!(engine.tool_url().as_deref(), Some("https://b.example.com"));
        let owner = engine.local_actor();
        assert_eq!((owner.name(), owner.role()), ("Kim", Role::Admin));

        // Someone who came in remotely cannot move the board elsewhere.
        let link = engine.create_invite(&owner, Role::Admin, "").unwrap();
        let session = engine.accept_invite(&link.token, "Eve").unwrap();
        let Some(crate::SessionState::Active(eve)) = engine.session(&session).unwrap() else {
            panic!("a member");
        };
        assert_eq!(
            engine
                .start_sharing(&eve, &request("https://evil.example", None))
                .map(|_| ()),
            Err(EngineError::Forbidden)
        );

        engine.stop_sharing(&owner).unwrap();
        assert!(!engine.remote_open());
        assert_eq!(engine.tool_url().as_deref(), Some("http://localhost:5280"));
        assert_eq!(engine.local_actor(), owner, "the owner stays");
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
        let mut on = request("", Some("Kim"));
        on.mode = ShareMode::Funnel;
        assert!(matches!(
            engine.start_sharing(&Actor::Local, &on),
            Err(EngineError::Unavailable(m)) if m.contains("login.tailscale.com")
        ));
        assert_eq!(
            engine.local_actor(),
            Actor::Local,
            "no owner when nothing opened"
        );
        on.mode = ShareMode::Tailscale;
        let status = engine.start_sharing(&Actor::Local, &on).unwrap();
        assert_eq!(status.sharing.unwrap().url, "https://kim.tail1.ts.net");
        assert_eq!(status.modes.len(), 2);
        assert_eq!(*tunnels.0.lock().unwrap(), vec![ShareMode::Tailscale]);
        let owner = engine.local_actor();
        engine.stop_sharing(&owner).unwrap();
        assert!(tunnels.0.lock().unwrap().is_empty());
    }

    #[test]
    fn ngrok_keeps_the_authtoken_out_of_sight_and_reopens_its_domain() {
        let tunnels = Arc::new(FakeTunnels::default());
        let engine = engine().with_tunnels(tunnels.clone());
        let mut on = request("", Some("Kim"));
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
        let status = engine.sharing(&engine.local_actor()).unwrap();
        assert_eq!(status.sharing.unwrap().url, "https://kim.ngrok.app");
        assert_eq!(status.warning, None);
    }

    #[test]
    fn sharing_survives_a_restart() {
        let engine = engine();
        engine
            .start_sharing(
                &Actor::Local,
                &request("https://b.example.com", Some("Kim")),
            )
            .unwrap();
        engine.write_live(|live| {
            live.sharing = None;
            live.owner = None;
        });
        engine.restore_sharing().unwrap();
        assert!(engine.remote_open());
        assert_eq!(engine.local_actor().name(), "Kim");
    }
}
