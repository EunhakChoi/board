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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShareMode {
    /// An address the person set up themselves (a reverse proxy or tunnel to the remote door).
    Address,
}

/// Sharing that is on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sharing {
    pub mode: ShareMode,
    /// The address people open, without a trailing slash.
    pub url: String,
}

/// What the sharing page shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharingStatus {
    /// Set by the server's environment (`--public`): the page only shows it.
    pub fixed: bool,
    pub sharing: Option<Sharing>,
    /// The account the local door acts as, once sharing was turned on.
    pub owner: Option<Owner>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner {
    pub id: String,
    pub name: String,
}

/// A request to turn sharing on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareRequest {
    pub mode: ShareMode,
    pub url: String,
    /// The owner's name, needed the first time.
    pub owner_name: Option<String>,
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
}

const SHARING: &str = "sharing";
const OWNER: &str = "sharing.owner";

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
        let sharing = match self.ports.settings.get(SHARING)? {
            Some(text) => serde_json::from_str::<Sharing>(&text).ok(),
            None => None,
        };
        self.write_live(|live| {
            live.owner = owner;
            live.sharing = sharing;
        });
        Ok(())
    }

    pub fn sharing(&self, actor: &Actor) -> Result<SharingStatus, EngineError> {
        admin(actor)?;
        let live = self.read_live();
        Ok(SharingStatus {
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
        let url = request.url.trim().trim_end_matches('/').to_owned();
        if !(url.starts_with("https://") || url.starts_with("http://")) || url.contains(' ') {
            return Err(EngineError::Invalid(
                "the address is an http(s) URL, e.g. https://board.example.com".into(),
            ));
        }
        if self.read_live().owner.is_none() {
            let name = request.owner_name.as_deref().unwrap_or_default();
            let user = self.insert_member(&crate::invites::check_name(name)?, Role::Admin)?;
            self.ports.settings.set(OWNER, &user.id)?;
            let owner = self.owner_actor()?;
            self.write_live(|live| live.owner = owner);
        }
        let sharing = Sharing {
            mode: request.mode,
            url,
        };
        let text =
            serde_json::to_string(&sharing).map_err(|e| EngineError::Invalid(e.to_string()))?;
        self.ports.settings.set(SHARING, &text)?;
        self.write_live(|live| live.sharing = Some(sharing));
        self.sharing(&self.local_actor())
    }

    /// Turns sharing off: the remote door closes. Accounts and sessions stay for next time.
    pub fn stop_sharing(&self, actor: &Actor) -> Result<SharingStatus, EngineError> {
        self.check_changeable(actor)?;
        self.ports.settings.set(SHARING, "")?;
        self.write_live(|live| live.sharing = None);
        self.sharing(&self.local_actor())
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
