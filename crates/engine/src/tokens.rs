//! CLI tokens: a member's coding agent acts as them on a team server. Issued by approving a
//! device login in the browser (`board login`) or created by hand on the web.

use board_model::{Actor, CliToken, UserStatus};

use crate::auth::{hash, now, random_hex, random_token};
use crate::{Engine, EngineError};

/// How long a device login waits for approval.
const DEVICE_SECS: u64 = 600;
/// How often the CLI should ask whether it was approved.
pub const DEVICE_INTERVAL_SECS: u64 = 3;
/// Letters that cannot be misread (no vowels, so no words either).
const CODE_LETTERS: &[u8] = b"BCDFGHJKLMNPQRSTVWXZ";

/// A device login just started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceStart {
    /// Secret the CLI polls with.
    pub device_code: String,
    /// Short code the person checks in the browser (`BCDF-GHJK`).
    pub user_code: String,
    pub expires_in: u64,
}

/// Where a device login stands, as the CLI polls it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DevicePoll {
    Pending,
    Denied,
    Expired,
    /// Approved: the new token (handed out once).
    Approved(String),
}

/// A device login as the approval page shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRequest {
    pub user_code: String,
    pub name: String,
    pub status: String,
}

/// A token just created by hand; `token` is shown once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCliToken {
    pub info: CliToken,
    pub token: String,
}

impl Engine {
    /// Creates a token for the calling member (shown once).
    pub fn create_cli_token(&self, actor: &Actor, name: &str) -> Result<NewCliToken, EngineError> {
        let user = member(actor)?;
        self.issue_cli_token(user, name, "manual")
    }

    pub fn cli_tokens(&self, actor: &Actor) -> Result<Vec<CliToken>, EngineError> {
        Ok(self.ports.accounts.cli_tokens(member(actor)?)?)
    }

    pub fn revoke_cli_token(&self, actor: &Actor, id: &str) -> Result<(), EngineError> {
        if !self.ports.accounts.delete_cli_token(member(actor)?, id)? {
            return Err(EngineError::NotFound("token"));
        }
        Ok(())
    }

    /// Revokes the token itself (`board logout`).
    pub fn revoke_own_token(&self, token: &str) -> Result<(), EngineError> {
        Ok(self.ports.accounts.delete_cli_token_by_hash(&hash(token))?)
    }

    /// The member a CLI token acts for, while they still have access.
    pub fn token_actor(&self, token: &str) -> Result<Option<Actor>, EngineError> {
        Ok(self
            .ports
            .accounts
            .token_user(&hash(token))?
            .filter(|user| user.status == UserStatus::Active)
            .map(|user| user.actor()))
    }

    /// Starts a device login for a CLI that calls itself `name`.
    pub fn start_device_login(&self, name: &str) -> Result<DeviceStart, EngineError> {
        let device_code = random_token();
        let user_code = user_code();
        self.ports.accounts.insert_device(
            &hash(&device_code),
            &user_code,
            &clean_name(name),
            now() + DEVICE_SECS,
        )?;
        Ok(DeviceStart {
            device_code,
            user_code,
            expires_in: DEVICE_SECS,
        })
    }

    /// The waiting login behind `user_code`, for the approval page.
    pub fn device_request(
        &self,
        actor: &Actor,
        user_code: &str,
    ) -> Result<DeviceRequest, EngineError> {
        member(actor)?;
        let device = self
            .ports
            .accounts
            .device_by_user_code(&normalize(user_code))?
            .filter(|d| d.expires_at > now())
            .ok_or(EngineError::NotFound("login request"))?;
        Ok(DeviceRequest {
            user_code: device.user_code,
            name: device.name,
            status: device.status,
        })
    }

    /// Approves or denies a waiting login: approved, the CLI acts as the calling member.
    pub fn decide_device_login(
        &self,
        actor: &Actor,
        user_code: &str,
        approve: bool,
    ) -> Result<(), EngineError> {
        let user = member(actor)?;
        let code = normalize(user_code);
        self.device_request(actor, &code)?;
        if !self.ports.accounts.decide_device(&code, user, approve)? {
            return Err(EngineError::Invalid(
                "this login was decided already".into(),
            ));
        }
        Ok(())
    }

    /// What the CLI gets when it asks about its login. An approval hands out the token once.
    pub fn poll_device_login(&self, device_code: &str) -> Result<DevicePoll, EngineError> {
        let code_hash = hash(device_code);
        let Some(device) = self.ports.accounts.device_by_code(&code_hash)? else {
            return Ok(DevicePoll::Expired);
        };
        if device.expires_at <= now() {
            self.ports.accounts.delete_device(&code_hash)?;
            return Ok(DevicePoll::Expired);
        }
        match (device.status.as_str(), device.user) {
            ("approved", Some(user)) => {
                self.ports.accounts.delete_device(&code_hash)?;
                let token = self.issue_cli_token(&user, &device.name, "device")?;
                Ok(DevicePoll::Approved(token.token))
            }
            ("denied", _) => {
                self.ports.accounts.delete_device(&code_hash)?;
                Ok(DevicePoll::Denied)
            }
            _ => Ok(DevicePoll::Pending),
        }
    }

    fn issue_cli_token(
        &self,
        user: &str,
        name: &str,
        via: &str,
    ) -> Result<NewCliToken, EngineError> {
        let token = format!("board_{}", random_token());
        let info = self.ports.accounts.insert_cli_token(
            &format!("t{}", &random_hex()[..10]),
            user,
            &clean_name(name),
            &hash(&token),
            via,
        )?;
        Ok(NewCliToken { info, token })
    }
}

/// Only signed-in members have tokens (not the local admin, not guests).
fn member(actor: &Actor) -> Result<&str, EngineError> {
    match actor {
        Actor::User { id, .. } => Ok(id),
        Actor::Local => Err(EngineError::Invalid(
            "local mode needs no token; the CLI is the admin already".into(),
        )),
        Actor::Guest { .. } => Err(EngineError::Forbidden),
    }
}

fn clean_name(name: &str) -> String {
    let name: String = name.trim().chars().take(60).collect();
    if name.is_empty() { "CLI".into() } else { name }
}

fn user_code() -> String {
    let hex = random_hex();
    let letters: String = hex
        .as_bytes()
        .chunks(2)
        .take(8)
        .map(|pair| {
            let byte =
                u8::from_str_radix(std::str::from_utf8(pair).unwrap_or("0"), 16).unwrap_or(0);
            char::from(CODE_LETTERS[usize::from(byte) % CODE_LETTERS.len()])
        })
        .collect();
    format!("{}-{}", &letters[..4], &letters[4..])
}

/// `bcdf ghjk`, `BCDFGHJK` and `BCDF-GHJK` are the same code.
fn normalize(code: &str) -> String {
    let letters: String = code
        .chars()
        .filter(char::is_ascii_alphabetic)
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if letters.len() == 8 {
        format!("{}-{}", &letters[..4], &letters[4..])
    } else {
        letters
    }
}

#[cfg(test)]
mod tests {
    use board_model::Role;

    use super::*;
    use crate::testing::{FakeIdentity, engine_with_sign_in};
    use crate::{SessionState, SignIn};

    fn join(engine: &Engine, idp: &FakeIdentity, who: &str) -> Actor {
        engine.begin_sign_in(None, None).unwrap();
        idp.next(who, &format!("{who}@example.com"), true);
        match engine.finish_sign_in(&idp.last_state(), "c").unwrap() {
            SignIn::Session { token, .. } => match engine.session(&token).unwrap() {
                Some(SessionState::Active(actor)) => actor,
                other => panic!("{other:?}"),
            },
            SignIn::Refused(r) => panic!("{r:?}"),
        }
    }

    #[test]
    fn device_login_hands_out_a_token_once() {
        let (engine, idp) = engine_with_sign_in(&["boss@example.com"]);
        let boss = join(&engine, &idp, "boss");
        let start = engine.start_device_login("laptop").unwrap();
        assert_eq!(start.user_code.len(), 9);
        assert_eq!(
            engine.poll_device_login(&start.device_code).unwrap(),
            DevicePoll::Pending
        );

        let lower = start.user_code.to_lowercase().replace('-', " ");
        let request = engine.device_request(&boss, &lower).unwrap();
        assert_eq!(
            (request.name.as_str(), request.status.as_str()),
            ("laptop", "pending")
        );
        engine
            .decide_device_login(&boss, &start.user_code, true)
            .unwrap();
        let DevicePoll::Approved(token) = engine.poll_device_login(&start.device_code).unwrap()
        else {
            panic!("approved");
        };
        assert!(token.starts_with("board_"));
        assert_eq!(
            engine.poll_device_login(&start.device_code).unwrap(),
            DevicePoll::Expired,
            "the token is handed out once"
        );
        let actor = engine.token_actor(&token).unwrap().unwrap();
        assert_eq!(actor.role(), Role::Admin);
        let tokens = engine.cli_tokens(&boss).unwrap();
        assert_eq!((tokens.len(), tokens[0].via.as_str()), (1, "device"));

        engine.revoke_own_token(&token).unwrap();
        assert_eq!(engine.token_actor(&token).unwrap(), None);
    }

    #[test]
    fn denied_logins_and_manual_tokens() {
        let (engine, idp) = engine_with_sign_in(&["boss@example.com"]);
        let boss = join(&engine, &idp, "boss");
        let start = engine.start_device_login("ci").unwrap();
        engine
            .decide_device_login(&boss, &start.user_code, false)
            .unwrap();
        assert!(
            engine
                .decide_device_login(&boss, &start.user_code, true)
                .is_err()
        );
        assert_eq!(
            engine.poll_device_login(&start.device_code).unwrap(),
            DevicePoll::Denied
        );

        let created = engine.create_cli_token(&boss, "  ").unwrap();
        assert_eq!(created.info.name, "CLI");
        assert!(engine.token_actor(&created.token).unwrap().is_some());
        engine.revoke_cli_token(&boss, &created.info.id).unwrap();
        assert_eq!(engine.token_actor(&created.token).unwrap(), None);
        assert_eq!(
            engine.revoke_cli_token(&boss, &created.info.id),
            Err(EngineError::NotFound("token"))
        );
        assert!(matches!(
            engine.create_cli_token(&Actor::Local, "x"),
            Err(EngineError::Invalid(_))
        ));
    }

    #[test]
    fn codes_are_readable() {
        let code = user_code();
        assert!(
            code.chars()
                .all(|c| c == '-' || CODE_LETTERS.contains(&(c as u8)))
        );
        assert_eq!(normalize("bcdf ghjk"), "BCDF-GHJK");
    }
}
