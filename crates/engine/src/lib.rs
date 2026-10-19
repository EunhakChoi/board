//! Commands (merge, restore, comment.add, ...) and permission checks over the model; storage, database and rendering behind traits.

mod access;
mod auth;
mod browse;
mod captures;
mod comments;
mod error;
mod invites;
mod links;
mod notices;
pub mod policy;
mod ports;
mod preview;
mod projects;
mod realtime;
mod sharing;
mod sheet;
mod tokens;

use std::sync::{Arc, RwLock};

use sharing::Live;

pub use access::AccessChange;
pub use auth::{Refusal, SESSION_SECS, SessionState, SignIn, SignInSettings};
pub use captures::{CaptureStatus, RenderResult, Rendered, THEMES};
pub use comments::{At, CommentFilter};
pub use error::EngineError;
pub use invites::{InviteInfo, InviteKind, NewInviteLink};
pub use links::{NewLink, SharedLink};
pub use notices::{Inbox, WEBHOOK_EVENTS, WebhookSecret};
pub use ports::{
    Accounts, AllowRule, AuthRequest, Author, Build, BuildState, BuildStep, BuiltFile, Capture,
    Captures, Change, ChangeKind, Claims, Comments, Device, FileChange, Guest, Identity, ImageKind,
    Invites, Joined, Kick, NewComment, NewInvite, NewNotification, NewReply, NewShareLink, NewUser,
    NewWebhook, Notices, Outgoing, Peer, ProjectStore, Realtime, Records, RenderJob, Renderer,
    Settings, Shot, StoreError, TunnelMode, Tunnels, WebhookSender, Workspace, WorkspaceError,
};
pub use preview::{PREVIEW_PREFIX, Preview, PreviewSettings, preview_path};
pub use projects::{Checkout, Import};
pub use sharing::{Owner, ShareMode, ShareRequest, Sharing, SharingStatus};
pub use sheet::SheetItem;
pub use tokens::{DEVICE_INTERVAL_SECS, DevicePoll, DeviceRequest, DeviceStart, NewCliToken};

/// Everything the engine needs from the outside world.
#[derive(Clone)]
pub struct Ports {
    pub settings: Arc<dyn Settings>,
    pub records: Arc<dyn Records>,
    pub store: Arc<dyn ProjectStore>,
    pub workspace: Arc<dyn Workspace>,
    pub renderer: Arc<dyn Renderer>,
    pub captures: Arc<dyn Captures>,
    pub comments: Arc<dyn Comments>,
    pub accounts: Arc<dyn Accounts>,
    pub realtime: Arc<dyn Realtime>,
    pub notices: Arc<dyn Notices>,
    pub invites: Arc<dyn Invites>,
    pub webhooks: Arc<dyn WebhookSender>,
}

impl std::fmt::Debug for Ports {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ports").finish_non_exhaustive()
    }
}

/// Runs commands. One instance per server, shared by every request.
#[derive(Clone)]
pub struct Engine {
    ports: Ports,
    local_mode: bool,
    preview: Option<Arc<PreviewSettings>>,
    /// What can change while running: the address and sign-in, when sharing turns on or off.
    live: Arc<RwLock<Live>>,
    tunnels: Option<Arc<dyn Tunnels>>,
    pending: Arc<auth::PendingSignIns>,
    secret: Option<Arc<[u8; 32]>>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("local_mode", &self.local_mode)
            .finish_non_exhaustive()
    }
}

impl Engine {
    pub fn new(ports: Ports, local_mode: bool) -> Self {
        Self {
            ports,
            local_mode,
            preview: None,
            live: Arc::default(),
            tunnels: None,
            pending: Arc::default(),
            secret: None,
        }
    }

    /// The server key: webhook signing secrets are derived from it.
    #[must_use]
    pub fn with_secret(mut self, key: [u8; 32]) -> Self {
        self.secret = Some(Arc::new(key));
        self
    }

    /// Turns sign-in on (a public server): accounts come from the identity provider.
    #[must_use]
    pub fn with_sign_in(self, settings: SignInSettings) -> Self {
        self.write_live(|live| live.base_sign_in = Some(Arc::new(settings)));
        self
    }

    /// Enables previews: built commits are served from `settings.origin` with signed tokens.
    #[must_use]
    pub fn with_preview(mut self, settings: PreviewSettings) -> Self {
        self.preview = Some(Arc::new(settings));
        self
    }

    /// Lets sharing run tunnels (Tailscale) instead of only taking an address set up by hand.
    #[must_use]
    pub fn with_tunnels(mut self, tunnels: Arc<dyn Tunnels>) -> Self {
        self.tunnels = Some(tunnels);
        self
    }

    /// Where people open the web tool, for links (`board url`, notifications).
    #[must_use]
    pub fn with_tool_url(self, url: &str) -> Self {
        self.write_live(|live| live.home_url = Some(url.trim_end_matches('/').into()));
        self
    }

    pub(crate) fn read_live(&self) -> std::sync::RwLockReadGuard<'_, Live> {
        self.live
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn write_live(&self, change: impl FnOnce(&mut Live)) {
        let mut live = self
            .live
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        change(&mut live);
    }

    /// Versions, links and whether the internal services answer.
    pub fn system(&self) -> Result<System, StoreError> {
        let state = |result: Result<(), WorkspaceError>| match result {
            Ok(()) => ServiceState::Ok,
            Err(WorkspaceError::Unavailable(e) | WorkspaceError::Failed(e)) => {
                ServiceState::Failing(e)
            }
        };
        Ok(System {
            latest: self.latest_version()?,
            tool_url: self.tool_url(),
            preview_url: self.preview.as_ref().map(|p| p.origin.clone()),
            local_mode: self.local_mode,
            workspace: state(self.ports.workspace.health()),
            render: state(self.ports.renderer.health()),
            sign_in: match &self.sign_in() {
                Some(settings) => Some(SignInStatus {
                    provider: settings.identity.as_ref().map(|i| i.provider().to_owned()),
                    callback_url: settings
                        .identity
                        .as_ref()
                        .map(|i| i.callback_url().to_owned()),
                    state: settings
                        .identity
                        .as_ref()
                        .map_or(ServiceState::Ok, |i| state(i.health())),
                    has_admin: self.ports.accounts.has_admin()?,
                }),
                None => None,
            },
        })
    }

    /// True when previews are served from the tool's own address (one address for both).
    pub fn previews_on_tool(&self) -> bool {
        match (&self.preview, self.tool_url()) {
            (Some(preview), Some(tool)) => preview.origin.trim_end_matches('/') == tool,
            _ => false,
        }
    }

    /// Where people open the web tool, when known.
    pub fn tool_url(&self) -> Option<String> {
        let live = self.read_live();
        live.sharing
            .as_ref()
            .map(|s| s.url.clone())
            .or_else(|| live.home_url.as_deref().map(str::to_owned))
    }

    /// True while the server only listens on localhost: every request acts as the built-in admin.
    pub fn local_mode(&self) -> bool {
        self.local_mode
    }

    /// The latest released version, if an update check has recorded one.
    pub fn latest_version(&self) -> Result<Option<String>, StoreError> {
        self.ports.settings.get(LATEST_VERSION)
    }

    pub fn record_latest_version(&self, version: &str) -> Result<(), StoreError> {
        self.ports.settings.set(LATEST_VERSION, version)
    }
}

const LATEST_VERSION: &str = "update.latest";

/// What `Engine::system` reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct System {
    pub latest: Option<String>,
    pub tool_url: Option<String>,
    pub preview_url: Option<String>,
    pub local_mode: bool,
    pub workspace: ServiceState,
    pub render: ServiceState,
    /// How people sign in, on a public server.
    pub sign_in: Option<SignInStatus>,
}

/// The identity provider as `board doctor` sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignInStatus {
    /// `None`: no provider; people come in through invite links.
    pub provider: Option<String>,
    pub callback_url: Option<String>,
    pub state: ServiceState,
    /// False until someone used the setup link (or an admin address signed in).
    pub has_admin: bool,
}

/// Whether an internal service answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceState {
    Ok,
    Failing(String),
}

#[cfg(test)]
pub(crate) mod testing;

#[cfg(test)]
mod tests {
    use crate::testing::engine;

    #[test]
    fn latest_version_round_trips() {
        let engine = engine();
        assert_eq!(engine.latest_version(), Ok(None));
        engine.record_latest_version("1.2.3").unwrap();
        assert_eq!(engine.latest_version(), Ok(Some("1.2.3".into())));
    }
}
