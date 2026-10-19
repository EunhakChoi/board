//! Coming in without an identity provider: the setup link makes the first admin, invite links
//! let new members join with a fixed role, and sign-in links put an existing member on another
//! device. Every link works once and expires; only token hashes are kept.

use board_model::{Actor, Invite, Role, User, UserStatus};

use crate::access::admin;
use crate::auth::{Refusal, SESSION_SECS, hash, now, random_hex, random_token};
use crate::{Engine, EngineError, NewInvite, NewUser};

/// How long an invite for a new member stays open.
const INVITE_SECS: u64 = 7 * 24 * 3600;
/// How long a sign-in link an admin made for a member stays open.
const MEMBER_LINK_SECS: u64 = 7 * 24 * 3600;
/// How long a link someone made for their own other device stays open.
const DEVICE_LINK_SECS: u64 = 15 * 60;
/// The longest display name.
const MAX_NAME: usize = 40;
/// Accounts made without a provider carry this as their provider.
pub(crate) const INVITE_PROVIDER: &str = "invite";

/// A link just made. `token` is shown once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewInviteLink {
    pub invite: Invite,
    pub token: String,
}

/// What a link does, shown before it is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InviteKind {
    /// A new member joins (they pick a name).
    Join,
    /// An existing member signs in.
    SignIn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InviteInfo {
    pub kind: InviteKind,
    pub role: Role,
    /// For a sign-in link, whose account it opens.
    pub name: Option<String>,
}

impl Engine {
    /// An invite for a new member with `role`. Admins only.
    pub fn create_invite(
        &self,
        actor: &Actor,
        role: Role,
        label: &str,
    ) -> Result<NewInviteLink, EngineError> {
        admin(actor)?;
        self.sign_in_settings()?;
        self.insert_invite(actor, role, None, label, INVITE_SECS)
    }

    /// Open invites for new members (used and expired ones too, for the list).
    pub fn invites(&self, actor: &Actor) -> Result<Vec<Invite>, EngineError> {
        admin(actor)?;
        Ok(self.ports.invites.invites()?)
    }

    pub fn revoke_invite(&self, actor: &Actor, id: &str) -> Result<(), EngineError> {
        admin(actor)?;
        if self.ports.invites.delete_invite(id)? {
            Ok(())
        } else {
            Err(EngineError::NotFound("invite"))
        }
    }

    /// A sign-in link for member `user`: an admin's for anyone (7 days), or a member's for
    /// themselves, to sign in on another device (15 minutes).
    pub fn create_sign_in_link(
        &self,
        actor: &Actor,
        user: &str,
    ) -> Result<NewInviteLink, EngineError> {
        self.sign_in_settings()?;
        let own = matches!(actor, Actor::User { id, .. } if id == user);
        if !own {
            admin(actor)?;
        }
        let member = self
            .ports
            .accounts
            .user(user)?
            .filter(|u| u.status == UserStatus::Active)
            .ok_or(EngineError::NotFound("user"))?;
        let secs = if own {
            DEVICE_LINK_SECS
        } else {
            MEMBER_LINK_SECS
        };
        self.insert_invite(actor, member.role, Some(member.id), "", secs)
    }

    /// What `token` opens. Used, expired and unknown links look the same.
    pub fn invite(&self, token: &str) -> Result<InviteInfo, EngineError> {
        let invite = self.live_invite(token)?;
        let name = match &invite.user {
            Some(id) => Some(
                self.ports
                    .accounts
                    .user(id)?
                    .ok_or(EngineError::NotFound("invite"))?
                    .name,
            ),
            None => None,
        };
        Ok(InviteInfo {
            kind: if invite.user.is_some() {
                InviteKind::SignIn
            } else {
                InviteKind::Join
            },
            role: invite.role,
            name,
        })
    }

    /// Uses `token`: a new member joins as `name` (ignored for a sign-in link). Returns a session.
    pub fn accept_invite(&self, token: &str, name: &str) -> Result<String, EngineError> {
        self.sign_in_settings()?;
        let invite = self.live_invite(token)?;
        let accounts = &self.ports.accounts;
        let user = match &invite.user {
            Some(id) => {
                let user = accounts.user(id)?.ok_or(EngineError::NotFound("invite"))?;
                refuse(&user)?;
                user
            }
            None => self.new_member(name, invite.role)?,
        };
        if !self.ports.invites.use_invite(&invite.id, &user.id)? {
            // Someone used it a moment earlier; undo the account we just made.
            if invite.user.is_none() {
                accounts.set_access(&user.id, user.role, UserStatus::Removed)?;
            }
            return Err(EngineError::NotFound("invite"));
        }
        self.session_for(&user)
    }

    /// The setup link without a provider: whoever opens it first becomes the admin as `name`.
    pub fn finish_setup(&self, token: &str, name: &str) -> Result<String, EngineError> {
        self.sign_in_settings()?;
        let name = check_name(name)?;
        if !self.ports.accounts.take_setup_token(&hash(token))? {
            return Err(EngineError::NotFound("setup link"));
        }
        let user = self.insert_member(&name, Role::Admin)?;
        self.session_for(&user)
    }

    fn new_member(&self, name: &str, role: Role) -> Result<User, EngineError> {
        let name = check_name(name)?;
        self.insert_member(&name, role)
    }

    fn insert_member(&self, name: &str, role: Role) -> Result<User, EngineError> {
        let id = format!("u{}", &random_hex()[..12]);
        Ok(self.ports.accounts.insert_user(&NewUser {
            id: id.clone(),
            provider: INVITE_PROVIDER.to_owned(),
            subject: id,
            email: String::new(),
            name: name.to_owned(),
            avatar_url: None,
            role,
            status: UserStatus::Active,
        })?)
    }

    fn session_for(&self, user: &User) -> Result<String, EngineError> {
        let token = random_token();
        self.ports
            .accounts
            .insert_session(&hash(&token), &user.id, now() + SESSION_SECS)?;
        Ok(token)
    }

    fn insert_invite(
        &self,
        actor: &Actor,
        role: Role,
        user: Option<String>,
        label: &str,
        secs: u64,
    ) -> Result<NewInviteLink, EngineError> {
        let token = random_token();
        let invite = self.ports.invites.insert_invite(&NewInvite {
            id: format!("i{}", &random_hex()[..10]),
            token_hash: hash(&token),
            role,
            user,
            label: label.trim().chars().take(80).collect(),
            created_by: actor.name().to_owned(),
            expires_at: now() + secs,
        })?;
        Ok(NewInviteLink { invite, token })
    }

    fn live_invite(&self, token: &str) -> Result<Invite, EngineError> {
        self.ports
            .invites
            .invite_by_token(&hash(token))?
            .filter(|i| i.used_at.is_none() && i.expires_at > now())
            .ok_or(EngineError::NotFound("invite"))
    }
}

fn check_name(name: &str) -> Result<String, EngineError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return Err(EngineError::Invalid(format!(
            "a name of 1-{MAX_NAME} characters, please"
        )));
    }
    Ok(name.to_owned())
}

/// A member who was removed or turned down cannot sign in through a link either.
fn refuse(user: &User) -> Result<(), EngineError> {
    match user.status {
        UserStatus::Active => Ok(()),
        UserStatus::Pending => Err(EngineError::Forbidden),
        UserStatus::Rejected => Err(EngineError::Invalid(Refusal::Rejected.code().into())),
        UserStatus::Removed => Err(EngineError::Invalid(Refusal::Removed.code().into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SessionState;
    use crate::testing::engine_without_provider;

    fn active(engine: &Engine, session: &str) -> Actor {
        match engine.session(session).unwrap() {
            Some(SessionState::Active(actor)) => actor,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn setup_link_makes_the_first_admin_once() {
        let engine = engine_without_provider();
        let token = engine.issue_setup_token().unwrap().unwrap();
        assert!(matches!(
            engine.finish_setup(&token, " "),
            Err(EngineError::Invalid(_))
        ));
        let session = engine.finish_setup(&token, "Boss").unwrap();
        let boss = active(&engine, &session);
        assert_eq!((boss.name(), boss.role()), ("Boss", Role::Admin));
        assert_eq!(
            engine.finish_setup(&token, "Eve"),
            Err(EngineError::NotFound("setup link"))
        );
        assert_eq!(engine.issue_setup_token().unwrap(), None);
        assert!(
            engine.begin_sign_in(None, None).is_err(),
            "no provider to send anyone to"
        );
    }

    #[test]
    fn invites_let_one_new_member_in_with_their_role() {
        let engine = engine_without_provider();
        let token = engine.issue_setup_token().unwrap().unwrap();
        let boss = active(&engine, &engine.finish_setup(&token, "Boss").unwrap());

        let made = engine
            .create_invite(&boss, Role::Commenter, " Dana ")
            .unwrap();
        assert_eq!(made.invite.label, "Dana");
        assert_eq!(
            engine.invite(&made.token).unwrap(),
            InviteInfo {
                kind: InviteKind::Join,
                role: Role::Commenter,
                name: None
            }
        );
        let dana = active(&engine, &engine.accept_invite(&made.token, "Dana").unwrap());
        assert_eq!((dana.name(), dana.role()), ("Dana", Role::Commenter));
        assert_eq!(
            engine.accept_invite(&made.token, "Eve"),
            Err(EngineError::NotFound("invite")),
            "works once"
        );
        let listed = engine.invites(&boss).unwrap();
        assert!(listed[0].used_by.is_some());
        assert_eq!(
            engine.create_invite(&dana, Role::Viewer, "").map(|_| ()),
            Err(EngineError::Forbidden)
        );
        let open = engine.create_invite(&boss, Role::Viewer, "").unwrap();
        engine.revoke_invite(&boss, &open.invite.id).unwrap();
        assert_eq!(
            engine.invite(&open.token),
            Err(EngineError::NotFound("invite"))
        );
    }

    #[test]
    fn sign_in_links_open_an_existing_account_on_another_device() {
        let engine = engine_without_provider();
        let token = engine.issue_setup_token().unwrap().unwrap();
        let boss = active(&engine, &engine.finish_setup(&token, "Boss").unwrap());
        let made = engine.create_invite(&boss, Role::Editor, "").unwrap();
        let ann = active(&engine, &engine.accept_invite(&made.token, "Ann").unwrap());
        let Actor::User { id: ann_id, .. } = &ann else {
            panic!("a member")
        };

        // Ann's own link for her phone; the name typed there does not matter.
        let own = engine.create_sign_in_link(&ann, ann_id).unwrap();
        assert_eq!(
            engine.invite(&own.token).unwrap().name.as_deref(),
            Some("Ann")
        );
        let phone = active(&engine, &engine.accept_invite(&own.token, "x").unwrap());
        assert_eq!(phone.name(), "Ann");

        // Only admins make links for others; a removed member's link stops working.
        let Actor::User { id: boss_id, .. } = &boss else {
            panic!("a member")
        };
        assert_eq!(
            engine.create_sign_in_link(&ann, boss_id).map(|_| ()),
            Err(EngineError::Forbidden)
        );
        let for_ann = engine.create_sign_in_link(&boss, ann_id).unwrap();
        engine
            .ports
            .accounts
            .set_access(ann_id, Role::Editor, UserStatus::Removed)
            .unwrap();
        assert!(engine.accept_invite(&for_ann.token, "").is_err());
    }
}
