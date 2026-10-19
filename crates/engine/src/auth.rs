//! Signing in: the OIDC round trip, admission (setup link, admin addresses, allowlist, access
//! requests) and sessions (`docs/design/security.md` §2).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use board_model::{Actor, Role, User, UserStatus};
use sha2::{Digest, Sha256};

use crate::{AuthRequest, Engine, EngineError, Identity, NewUser, WorkspaceError};

/// How long a session lasts.
pub const SESSION_SECS: u64 = 30 * 24 * 3600;
/// How long the browser may take at the provider before the round trip is forgotten.
const PENDING_SECS: u64 = 600;

/// Sign-in settings: the provider (optional: without one, people come in through invite links)
/// and who becomes admin on sight.
#[derive(Clone)]
pub struct SignInSettings {
    pub identity: Option<Arc<dyn Identity>>,
    /// Addresses that become admins on their first sign-in (`BOARD_ADMIN_EMAILS`).
    pub admin_emails: Vec<String>,
}

impl std::fmt::Debug for SignInSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignInSettings")
            .field("provider", &self.identity.as_ref().map(|i| i.provider()))
            .field("admin_emails", &self.admin_emails)
            .finish()
    }
}

/// Round trips started but not finished, by `state`.
#[derive(Debug, Default)]
pub(crate) struct PendingSignIns(Mutex<HashMap<String, Pending>>);

#[derive(Debug)]
struct Pending {
    nonce: String,
    verifier: String,
    setup: Option<String>,
    return_to: String,
    started: u64,
}

/// How a finished sign-in went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignIn {
    /// A session for `user` (who may still be waiting for approval). `return_to` is a path on the
    /// tool.
    Session {
        token: String,
        user: User,
        return_to: String,
    },
    Refused(Refusal),
}

/// Why someone was not let in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// No admin yet: only the setup link (or an admin address) gets in.
    SetupRequired,
    /// The provider has not verified the address.
    EmailUnverified,
    /// An admin turned the access request down.
    Rejected,
    /// An admin took access away.
    Removed,
}

impl Refusal {
    pub fn code(self) -> &'static str {
        match self {
            Self::SetupRequired => "setup_required",
            Self::EmailUnverified => "email_unverified",
            Self::Rejected => "rejected",
            Self::Removed => "removed",
        }
    }
}

/// Who a session belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    Active(Actor),
    /// Signed in, waiting for an admin to approve the access request.
    Waiting(User),
    /// Signed in, then turned down or removed.
    Refused(Refusal),
}

impl Engine {
    /// Whether sign-in is configured (always true for a public server).
    pub fn sign_in_enabled(&self) -> bool {
        self.sign_in.is_some()
    }

    /// Starts a sign-in: remembers the round trip and returns the provider URL to send the browser
    /// to. `setup` is the token from the setup link; `return_to` a path on the tool.
    pub fn begin_sign_in(
        &self,
        setup: Option<&str>,
        return_to: Option<&str>,
    ) -> Result<String, EngineError> {
        let identity = self.identity()?;
        let state = random_token();
        let nonce = random_token();
        let verifier = random_token();
        let url = identity
            .authorize_url(&AuthRequest {
                state: state.clone(),
                nonce: nonce.clone(),
                code_challenge: URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
            })
            .map_err(unavailable)?;
        let now = now();
        let mut pending = self.pending.0.lock().map_err(|_| poisoned())?;
        pending.retain(|_, p| now.saturating_sub(p.started) < PENDING_SECS);
        pending.insert(
            state,
            Pending {
                nonce,
                verifier,
                setup: setup.map(str::to_owned),
                return_to: safe_return(return_to),
                started: now,
            },
        );
        Ok(url)
    }

    /// Finishes the round trip the provider sent back (`state`, `code`) and decides admission.
    pub fn finish_sign_in(&self, state: &str, code: &str) -> Result<SignIn, EngineError> {
        let settings = self.sign_in_settings()?;
        let identity = self.identity()?;
        let pending = self
            .pending
            .0
            .lock()
            .map_err(|_| poisoned())?
            .remove(state)
            .filter(|p| now().saturating_sub(p.started) < PENDING_SECS)
            .ok_or_else(|| EngineError::Invalid("sign-in expired; start again".into()))?;
        let claims = identity
            .exchange(code, &pending.verifier, &pending.nonce)
            .map_err(unavailable)?;
        if !claims.email_verified {
            return Ok(SignIn::Refused(Refusal::EmailUnverified));
        }
        let email = claims.email.trim().to_lowercase();
        let name = claims
            .name
            .clone()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| email.split('@').next().unwrap_or(&email).to_owned());
        let accounts = &self.ports.accounts;
        let provider = identity.provider();
        // The setup link makes its first user the admin, whoever they are.
        let setup = match &pending.setup {
            Some(token) => accounts.take_setup_token(&hash(token))?,
            None => false,
        };
        let user = if let Some(user) = accounts.user_by_subject(provider, &claims.subject)? {
            accounts.update_profile(&user.id, &email, &name, claims.picture.as_deref())?;
            if setup {
                accounts.set_access(&user.id, Role::Admin, UserStatus::Active)?;
            }
            accounts
                .user(&user.id)?
                .ok_or(EngineError::NotFound("user"))?
        } else {
            let (role, status) = if setup || is_admin_email(settings, &email) {
                (Role::Admin, UserStatus::Active)
            } else if !accounts.has_admin()? {
                return Ok(SignIn::Refused(Refusal::SetupRequired));
            } else if self.allowed(&email)? {
                (Role::Editor, UserStatus::Active)
            } else {
                (Role::Editor, UserStatus::Pending)
            };
            let user = accounts.insert_user(&NewUser {
                id: format!("u{}", &random_hex()[..12]),
                provider: provider.to_owned(),
                subject: claims.subject.clone(),
                email,
                name,
                avatar_url: claims.picture.clone(),
                role,
                status,
            })?;
            if user.status == UserStatus::Pending {
                self.access_requested(&user);
            }
            user
        };
        if user.status == UserStatus::Pending {
            self.ports.realtime.publish_admins(
                "access.requested",
                serde_json::json!({ "user": user.id, "email": user.email, "name": user.name }),
            );
        }
        match user.status {
            UserStatus::Rejected => return Ok(SignIn::Refused(Refusal::Rejected)),
            UserStatus::Removed => return Ok(SignIn::Refused(Refusal::Removed)),
            UserStatus::Active | UserStatus::Pending => {}
        }
        let token = random_token();
        accounts.insert_session(&hash(&token), &user.id, now() + SESSION_SECS)?;
        Ok(SignIn::Session {
            token,
            user,
            return_to: pending.return_to,
        })
    }

    /// Who the session `token` belongs to, or `None` for an unknown or expired session.
    pub fn session(&self, token: &str) -> Result<Option<SessionState>, EngineError> {
        let Some(user) = self.ports.accounts.session_user(&hash(token), now())? else {
            // Not a member's session: maybe someone who came in through a share link.
            return Ok(self.guest_actor(token)?.map(SessionState::Active));
        };
        Ok(Some(match user.status {
            UserStatus::Active => SessionState::Active(user.actor()),
            UserStatus::Pending => SessionState::Waiting(user),
            UserStatus::Rejected => SessionState::Refused(Refusal::Rejected),
            UserStatus::Removed => SessionState::Refused(Refusal::Removed),
        }))
    }

    pub fn sign_out(&self, token: &str) -> Result<(), EngineError> {
        Ok(self.ports.accounts.delete_session(&hash(token))?)
    }

    /// A fresh setup token while no admin exists (printed once to the terminal), else `None`.
    pub fn issue_setup_token(&self) -> Result<Option<String>, EngineError> {
        if self.ports.accounts.has_admin()? {
            return Ok(None);
        }
        let token = random_token();
        self.ports.accounts.set_setup_token(&hash(&token))?;
        Ok(Some(token))
    }

    pub(crate) fn sign_in_settings(&self) -> Result<&SignInSettings, EngineError> {
        self.sign_in
            .as_deref()
            .ok_or_else(|| EngineError::Unavailable("sign-in is not configured".into()))
    }

    /// The identity provider, when one is set up.
    fn identity(&self) -> Result<&dyn Identity, EngineError> {
        self.sign_in_settings()?.identity.as_deref().ok_or_else(|| {
            EngineError::Unavailable(
                "no sign-in provider is set up; ask an admin for an invite link".into(),
            )
        })
    }

    pub(crate) fn allowed(&self, email: &str) -> Result<bool, EngineError> {
        let domain = email.rsplit_once('@').map_or("", |(_, d)| d);
        Ok(self
            .ports
            .accounts
            .allow_rules()?
            .iter()
            .any(|rule| match rule.kind.as_str() {
                "email" => rule.value.eq_ignore_ascii_case(email),
                "domain" => rule.value.eq_ignore_ascii_case(domain),
                _ => false,
            }))
    }
}

fn is_admin_email(settings: &SignInSettings, email: &str) -> bool {
    settings
        .admin_emails
        .iter()
        .any(|a| a.trim().eq_ignore_ascii_case(email))
}

/// Only paths on the tool; anything else (another host, `//host`) becomes `/`.
fn safe_return(path: Option<&str>) -> String {
    match path {
        Some(p) if p.starts_with('/') && !p.starts_with("//") && !p.contains('\\') => p.to_owned(),
        _ => "/".to_owned(),
    }
}

/// SHA-256 of a token, hex. Only hashes are stored.
pub(crate) fn hash(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

/// 32 random bytes, URL-safe base64.
pub(crate) fn random_token() -> String {
    URL_SAFE_NO_PAD.encode(random_bytes())
}

pub(crate) fn random_hex() -> String {
    hex(&random_bytes())
}

fn random_bytes() -> [u8; 32] {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the OS has randomness");
    bytes
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn unavailable(error: WorkspaceError) -> EngineError {
    match error {
        WorkspaceError::Unavailable(m) | WorkspaceError::Failed(m) => {
            EngineError::Unavailable(format!("sign-in provider: {m}"))
        }
    }
}

fn poisoned() -> EngineError {
    EngineError::Store(crate::StoreError("sign-in state lock poisoned".into()))
}

#[cfg(test)]
mod tests {
    use board_model::{Role, UserStatus};

    use super::*;
    use crate::testing::{FakeIdentity, engine_with_sign_in};

    fn sign_in(engine: &Engine, idp: &FakeIdentity, subject: &str, setup: Option<&str>) -> SignIn {
        engine.begin_sign_in(setup, Some("/p/web")).unwrap();
        let state = idp.last_state();
        idp.next(subject, &format!("{subject}@example.com"), true);
        engine.finish_sign_in(&state, "code").unwrap()
    }

    fn session(result: &SignIn) -> (String, User) {
        match result {
            SignIn::Session { token, user, .. } => (token.clone(), user.clone()),
            SignIn::Refused(r) => panic!("refused: {r:?}"),
        }
    }

    #[test]
    fn nobody_gets_in_before_setup() {
        let (engine, idp) = engine_with_sign_in(&[]);
        assert_eq!(
            sign_in(&engine, &idp, "ann", None),
            SignIn::Refused(Refusal::SetupRequired)
        );
    }

    #[test]
    fn setup_link_makes_the_first_admin_once() {
        let (engine, idp) = engine_with_sign_in(&[]);
        let token = engine.issue_setup_token().unwrap().unwrap();
        let first = sign_in(&engine, &idp, "ann", Some(&token));
        let (session_token, user) = session(&first);
        assert_eq!((user.role, user.status), (Role::Admin, UserStatus::Active));
        assert!(matches!(first, SignIn::Session { ref return_to, .. } if return_to == "/p/web"));
        assert!(matches!(
            engine.session(&session_token).unwrap(),
            Some(SessionState::Active(Actor::User {
                role: Role::Admin,
                ..
            }))
        ));
        // The token is used up and no new one is issued once an admin exists.
        let (_, bob) = session(&sign_in(&engine, &idp, "bob", Some(&token)));
        assert_eq!(bob.status, UserStatus::Pending);
        assert_eq!(engine.issue_setup_token().unwrap(), None);
    }

    #[test]
    fn admin_emails_allowlist_and_access_requests() {
        let (engine, idp) = engine_with_sign_in(&["boss@example.com"]);
        let (_, boss) = session(&sign_in(&engine, &idp, "boss", None));
        assert_eq!(boss.role, Role::Admin);
        crate::testing::allow(&engine, "email", "ann@example.com");
        let (_, ann) = session(&sign_in(&engine, &idp, "ann", None));
        assert_eq!((ann.role, ann.status), (Role::Editor, UserStatus::Active));
        let (token, eve) = session(&sign_in(&engine, &idp, "eve", None));
        assert_eq!(eve.status, UserStatus::Pending);
        assert!(matches!(
            engine.session(&token).unwrap(),
            Some(SessionState::Waiting(_))
        ));
        engine
            .ports
            .accounts
            .set_access(&eve.id, Role::Editor, UserStatus::Rejected)
            .unwrap();
        assert_eq!(
            engine.session(&token).unwrap(),
            Some(SessionState::Refused(Refusal::Rejected))
        );
        assert_eq!(
            sign_in(&engine, &idp, "eve", None),
            SignIn::Refused(Refusal::Rejected)
        );
    }

    #[test]
    fn unverified_addresses_and_stale_states_are_refused() {
        let (engine, idp) = engine_with_sign_in(&["boss@example.com"]);
        engine.begin_sign_in(None, None).unwrap();
        let state = idp.last_state();
        idp.next("boss", "boss@example.com", false);
        assert_eq!(
            engine.finish_sign_in(&state, "code").unwrap(),
            SignIn::Refused(Refusal::EmailUnverified)
        );
        // A state works once.
        assert!(matches!(
            engine.finish_sign_in(&state, "code"),
            Err(EngineError::Invalid(_))
        ));
    }

    #[test]
    fn sign_out_ends_the_session() {
        let (engine, idp) = engine_with_sign_in(&["boss@example.com"]);
        let (token, _) = session(&sign_in(&engine, &idp, "boss", None));
        engine.sign_out(&token).unwrap();
        assert_eq!(engine.session(&token).unwrap(), None);
    }

    #[test]
    fn return_paths_stay_on_the_tool() {
        assert_eq!(safe_return(Some("/p/x?y=1")), "/p/x?y=1");
        assert_eq!(safe_return(Some("//evil.example")), "/");
        assert_eq!(safe_return(Some("https://evil.example")), "/");
        assert_eq!(safe_return(None), "/");
    }
}
