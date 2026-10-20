//! Members and how people come in (`docs/design/security.md` §2).
//!
//! A member is a name, a role and a token an admin hands over. The token signs a browser in once
//! (a session cookie follows) and is what the CLI sends. Someone without a token is a visitor, a
//! viewer with a made-up name, unless an admin turned visitors off. The person at the machine
//! that runs a local charet needs no token: the local door acts as the owner, an admin member
//! made at first start.

use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use charet_model::{Actor, Member, Role, User, VisitorAccess};
use sha2::{Digest, Sha256};

use crate::policy::{Command, can};
use crate::{Engine, EngineError, Kick, NewUser};

/// How long a browser stays signed in.
pub const SESSION_SECS: u64 = 365 * 24 * 3600;
/// Longest member name.
const MAX_NAME: usize = 60;
/// The member the local door acts as.
pub(crate) const OWNER: &str = "sharing.owner";
/// `false` when an admin turned visitors off, `comment` when they may comment; else they look.
const VISITORS: &str = "access.visitors";

/// Who a browser session belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    Active(Actor),
    /// The same token signed in from another browser since.
    Replaced,
}

/// A browser that came in with a token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedIn {
    /// The session cookie's value.
    pub session: String,
    pub user: User,
}

/// What admins change about a member. `None` keeps the current value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemberChange {
    pub name: Option<String>,
    pub role: Option<Role>,
}

impl Engine {
    /// Every member with their token. Admins only.
    pub fn members_list(&self, actor: &Actor) -> Result<Vec<Member>, EngineError> {
        admin(actor)?;
        let owner = self.owner_id()?;
        self.ports
            .accounts
            .users()?
            .into_iter()
            .map(|user| self.member(user, owner.as_deref()))
            .collect()
    }

    pub fn add_member(&self, actor: &Actor, name: &str, role: Role) -> Result<Member, EngineError> {
        admin(actor)?;
        let user = self.insert_member(&check_name(name)?, role)?;
        self.members_changed(&user.id);
        self.member(user, self.owner_id()?.as_deref())
    }

    /// Renames a member or changes their role. The owner and the last admin stay admins.
    pub fn change_member(
        &self,
        actor: &Actor,
        id: &str,
        change: &MemberChange,
    ) -> Result<Member, EngineError> {
        admin(actor)?;
        let user = self.account(id)?;
        let name = match &change.name {
            Some(name) => check_name(name)?,
            None => user.name.clone(),
        };
        let role = change.role.unwrap_or(user.role);
        if user.role == Role::Admin && role != Role::Admin {
            self.keep_admin(&user)?;
        }
        self.ports.accounts.update_user(id, &name, role)?;
        // Open pages pick up the new name or role when they reconnect.
        self.ports.realtime.kick(Kick::Person(id.to_owned()));
        self.members_changed(id);
        self.refresh_owner()?;
        self.member(self.account(id)?, self.owner_id()?.as_deref())
    }

    /// A new token for the member: the old one stops working and their browsers are signed out.
    pub fn reissue_token(&self, actor: &Actor, id: &str) -> Result<Member, EngineError> {
        admin(actor)?;
        let user = self.account(id)?;
        self.ports.accounts.set_user_token(id, &new_token())?;
        self.ports.accounts.delete_sessions(id)?;
        self.ports.realtime.kick(Kick::Person(id.to_owned()));
        self.members_changed(id);
        self.member(user, self.owner_id()?.as_deref())
    }

    pub fn remove_member(&self, actor: &Actor, id: &str) -> Result<(), EngineError> {
        admin(actor)?;
        let user = self.account(id)?;
        if user.role == Role::Admin {
            self.keep_admin(&user)?;
        }
        if self.owner_id()?.as_deref() == Some(id) {
            return Err(EngineError::Invalid(
                "this is the admin of the machine charet runs on; it stays".into(),
            ));
        }
        self.ports.accounts.delete_user(id)?;
        self.ports.realtime.kick(Kick::Person(id.to_owned()));
        self.members_changed(id);
        Ok(())
    }

    /// Signs a browser in with a member's token. Another browser signed in with the same token
    /// is signed out (the CLI is not).
    pub fn sign_in_with_token(&self, token: &str) -> Result<SignedIn, EngineError> {
        let user = self
            .ports
            .accounts
            .user_by_token(token.trim())?
            .ok_or(EngineError::NotFound("token"))?;
        let session = random_token();
        let session_hash = hash(&session);
        self.ports
            .accounts
            .insert_session(&session_hash, &user.id, now() + SESSION_SECS)?;
        self.ports
            .accounts
            .replace_sessions(&user.id, &session_hash)?;
        // The other browser's live connections close; its next request says it was replaced.
        self.ports.realtime.kick(Kick::Person(user.id.clone()));
        Ok(SignedIn { session, user })
    }

    /// Who a browser session belongs to, or `None` for an unknown or expired one.
    pub fn session(&self, session: &str) -> Result<Option<SessionState>, EngineError> {
        Ok(self
            .ports
            .accounts
            .session(&hash(session), now())?
            .map(|row| {
                if row.replaced {
                    SessionState::Replaced
                } else {
                    SessionState::Active(row.user.actor())
                }
            }))
    }

    pub fn sign_out(&self, session: &str) -> Result<(), EngineError> {
        Ok(self.ports.accounts.delete_session(&hash(session))?)
    }

    /// The member behind a token the CLI sent.
    pub fn token_actor(&self, token: &str) -> Result<Option<Actor>, EngineError> {
        Ok(self
            .ports
            .accounts
            .user_by_token(token.trim())?
            .map(|u| u.actor()))
    }

    /// What people without a token may do, unless a project says otherwise.
    pub fn server_visitors(&self) -> Result<VisitorAccess, EngineError> {
        Ok(match self.ports.settings.get(VISITORS)?.as_deref() {
            Some("false") => VisitorAccess::None,
            Some("comment") => VisitorAccess::Comment,
            _ => VisitorAccess::View,
        })
    }

    /// [`Self::server_visitors`], for the admins' page.
    pub fn visitors_setting(&self, actor: &Actor) -> Result<VisitorAccess, EngineError> {
        admin(actor)?;
        self.server_visitors()
    }

    pub fn set_server_visitors(
        &self,
        actor: &Actor,
        access: VisitorAccess,
    ) -> Result<(), EngineError> {
        admin(actor)?;
        let value = match access {
            VisitorAccess::None => "false",
            VisitorAccess::View => "true",
            VisitorAccess::Comment => "comment",
        };
        self.ports.settings.set(VISITORS, value)?;
        self.visitor_access_changed(None)
    }

    /// Whether a browser without a token gets in at all: the server lets visitors look, or some
    /// team project does.
    pub fn visitors_admitted(&self) -> Result<bool, EngineError> {
        let server = self.server_visitors()?;
        if server != VisitorAccess::None {
            return Ok(true);
        }
        Ok(self.ports.records.projects()?.into_iter().any(|project| {
            crate::projects::with_access(project, server).visitor_access != VisitorAccess::None
        }))
    }

    /// After a visitor setting changed (on `project`, or server-wide): visitors no longer let in
    /// are sent away, and pages reload what they may do.
    pub(crate) fn visitor_access_changed(&self, project: Option<&str>) -> Result<(), EngineError> {
        if !self.visitors_admitted()? {
            self.ports.realtime.kick(Kick::Visitors);
        }
        let keys = match project {
            Some(key) => vec![key.to_owned()],
            None => self
                .ports
                .records
                .projects()?
                .into_iter()
                .map(|p| p.key)
                .collect(),
        };
        for key in keys {
            self.publish(&key, "project.updated", serde_json::json!({}));
        }
        self.ports
            .realtime
            .publish_admins("access.changed", serde_json::json!({}));
        Ok(())
    }

    /// A visitor known by the id their browser keeps, while visitors are allowed.
    pub fn visitor(&self, id: &str) -> Result<Option<Actor>, EngineError> {
        if !valid_visitor_id(id) || !self.visitors_admitted()? {
            return Ok(None);
        }
        Ok(Some(Actor::Visitor {
            id: id.to_owned(),
            name: visitor_name(id),
        }))
    }

    /// Makes the owner (an admin member named `Admin`) unless there is one, and picks it up for
    /// the local door. Returns the owner's token when it was just made.
    pub fn ensure_owner(&self) -> Result<Option<String>, EngineError> {
        let created = if self.owner()?.is_some() {
            None
        } else {
            let user = self.insert_member("Admin", Role::Admin)?;
            self.ports.settings.set(OWNER, &user.id)?;
            self.ports.accounts.user_token(&user.id)?
        };
        self.refresh_owner()?;
        Ok(created)
    }

    /// The owner's token (`charet-server admin-token`).
    pub fn owner_token(&self) -> Result<Option<String>, EngineError> {
        match self.owner()? {
            Some(user) => Ok(self.ports.accounts.user_token(&user.id)?),
            None => Ok(None),
        }
    }

    /// Who a request through the local door acts as: the owner once there is one.
    pub fn local_actor(&self) -> Actor {
        self.read_live().owner.clone().unwrap_or(Actor::Local)
    }

    fn owner(&self) -> Result<Option<User>, EngineError> {
        match self.owner_id()? {
            Some(id) => Ok(self.ports.accounts.user(&id)?),
            None => Ok(None),
        }
    }

    fn owner_id(&self) -> Result<Option<String>, EngineError> {
        Ok(self.ports.settings.get(OWNER)?.filter(|id| !id.is_empty()))
    }

    /// Reloads the owner the local door acts as (after a rename).
    fn refresh_owner(&self) -> Result<(), EngineError> {
        let owner = self.owner()?.map(|u| u.actor());
        self.write_live(|live| live.owner = owner);
        Ok(())
    }

    pub(crate) fn insert_member(&self, name: &str, role: Role) -> Result<User, EngineError> {
        Ok(self.ports.accounts.insert_user(&NewUser {
            id: format!("u{}", &random_hex()[..12]),
            name: name.to_owned(),
            role,
            token: new_token(),
        })?)
    }

    fn member(&self, user: User, owner: Option<&str>) -> Result<Member, EngineError> {
        let token = self
            .ports
            .accounts
            .user_token(&user.id)?
            .unwrap_or_default();
        let owner = owner == Some(user.id.as_str());
        Ok(Member { user, token, owner })
    }

    fn account(&self, id: &str) -> Result<User, EngineError> {
        self.ports
            .accounts
            .user(id)?
            .ok_or(EngineError::NotFound("member"))
    }

    fn keep_admin(&self, user: &User) -> Result<(), EngineError> {
        if self.owner_id()?.as_deref() == Some(user.id.as_str()) {
            return Err(EngineError::Invalid(
                "the admin of the machine charet runs on stays an admin".into(),
            ));
        }
        let admins = self
            .ports
            .accounts
            .users()?
            .iter()
            .filter(|u| u.role == Role::Admin)
            .count();
        if admins <= 1 {
            return Err(EngineError::Invalid(
                "this is the last admin; make someone else admin first".into(),
            ));
        }
        Ok(())
    }

    /// Tells admins' Team pages.
    fn members_changed(&self, id: &str) {
        self.ports
            .realtime
            .publish_admins("access.changed", serde_json::json!({ "user": id }));
    }
}

pub(crate) fn admin(actor: &Actor) -> Result<(), EngineError> {
    if can(actor, Command::Admin, None) {
        Ok(())
    } else {
        Err(EngineError::Forbidden)
    }
}

pub(crate) fn check_name(name: &str) -> Result<String, EngineError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return Err(EngineError::Invalid(format!(
            "a name of 1-{MAX_NAME} characters, please"
        )));
    }
    Ok(name.to_owned())
}

/// Visitor ids are what the browser keeps: 16 hex digits.
fn valid_visitor_id(id: &str) -> bool {
    id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A fresh visitor id.
pub fn new_visitor_id() -> String {
    random_hex()[..16].to_owned()
}

/// Kept in step with `web.visitor.adjectives` and `web.visitor.animals`: the web shows the name
/// in the page's language from the same id.
const ADJECTIVES: [&str; 16] = [
    "Curious", "Quiet", "Brave", "Gentle", "Clever", "Happy", "Calm", "Swift", "Bright", "Cozy",
    "Lucky", "Merry", "Sunny", "Witty", "Kind", "Bold",
];
const ANIMALS: [&str; 16] = [
    "Otter", "Fox", "Panda", "Koala", "Penguin", "Rabbit", "Owl", "Dolphin", "Hedgehog",
    "Squirrel", "Seal", "Deer", "Turtle", "Tiger", "Whale", "Raccoon",
];

/// The made-up name of a visitor: an adjective and an animal picked by the id.
pub fn visitor_name(id: &str) -> String {
    let pick = |range: std::ops::Range<usize>| {
        id.get(range)
            .and_then(|hex| usize::from_str_radix(hex, 16).ok())
            .unwrap_or(0)
    };
    format!(
        "{} {}",
        ADJECTIVES[pick(0..4) % ADJECTIVES.len()],
        ANIMALS[pick(4..8) % ANIMALS.len()]
    )
}

/// A member token: 24 random bytes, URL-safe base64 (32 characters).
fn new_token() -> String {
    URL_SAFE_NO_PAD.encode(&random_bytes()[..24])
}

/// SHA-256 of a token, hex. Session tokens are stored as hashes only.
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

#[cfg(test)]
mod tests {
    use charet_model::Visibility;

    use super::*;
    use crate::testing::{engine, engine_with_fakes};

    fn owner(engine: &Engine) -> Actor {
        engine.ensure_owner().unwrap();
        engine.local_actor()
    }

    #[test]
    fn the_owner_is_made_once_and_acts_for_the_local_door() {
        let engine = engine();
        assert_eq!(engine.local_actor(), Actor::Local);
        let token = engine.ensure_owner().unwrap().expect("made now");
        assert_eq!(engine.ensure_owner().unwrap(), None, "made once");
        assert_eq!(engine.owner_token().unwrap(), Some(token.clone()));
        let actor = engine.local_actor();
        assert_eq!((actor.name(), actor.role()), ("Admin", Role::Admin));
        assert_eq!(engine.token_actor(&token).unwrap(), Some(actor.clone()));
        // The owner stays an admin and stays a member.
        let Actor::User { id, .. } = &actor else {
            panic!("{actor:?}")
        };
        let demote = MemberChange {
            role: Some(Role::Editor),
            ..MemberChange::default()
        };
        engine.add_member(&actor, "Ann", Role::Admin).unwrap();
        assert!(matches!(
            engine.change_member(&actor, id, &demote),
            Err(EngineError::Invalid(_))
        ));
        assert!(matches!(
            engine.remove_member(&actor, id),
            Err(EngineError::Invalid(_))
        ));
        // A rename shows on the local door.
        let rename = MemberChange {
            name: Some(" Dana ".into()),
            ..MemberChange::default()
        };
        engine.change_member(&actor, id, &rename).unwrap();
        assert_eq!(engine.local_actor().name(), "Dana");
    }

    #[test]
    fn admins_manage_members_and_tokens() {
        let engine = engine();
        let admin = owner(&engine);
        let ann = engine.add_member(&admin, " Ann ", Role::Editor).unwrap();
        assert_eq!((ann.user.name.as_str(), ann.owner), ("Ann", false));
        assert_eq!(ann.token.len(), 32);
        let ann_actor = engine.token_actor(&ann.token).unwrap().unwrap();
        assert_eq!(ann_actor.role(), Role::Editor);
        // Members cannot manage members.
        assert_eq!(engine.members_list(&ann_actor), Err(EngineError::Forbidden));
        assert_eq!(
            engine.add_member(&ann_actor, "Eve", Role::Admin),
            Err(EngineError::Forbidden)
        );
        assert!(matches!(
            engine.add_member(&admin, "  ", Role::Viewer),
            Err(EngineError::Invalid(_))
        ));

        let ann2 = engine
            .change_member(
                &admin,
                &ann.user.id,
                &MemberChange {
                    role: Some(Role::Commenter),
                    ..MemberChange::default()
                },
            )
            .unwrap();
        assert_eq!(ann2.user.role, Role::Commenter);
        assert_eq!(ann2.token, ann.token, "a role change keeps the token");

        let signed = engine.sign_in_with_token(&ann.token).unwrap();
        let ann3 = engine.reissue_token(&admin, &ann.user.id).unwrap();
        assert_ne!(ann3.token, ann.token);
        assert_eq!(engine.token_actor(&ann.token).unwrap(), None);
        assert_eq!(engine.session(&signed.session).unwrap(), None);

        engine.remove_member(&admin, &ann.user.id).unwrap();
        assert_eq!(engine.token_actor(&ann3.token).unwrap(), None);
        let names: Vec<_> = engine
            .members_list(&admin)
            .unwrap()
            .into_iter()
            .map(|m| (m.user.name, m.owner))
            .collect();
        assert_eq!(names, [("Admin".to_owned(), true)]);
    }

    #[test]
    fn a_second_browser_replaces_the_first() {
        let (engine, fakes) = engine_with_fakes();
        let admin = owner(&engine);
        let ann = engine.add_member(&admin, "Ann", Role::Viewer).unwrap();
        assert!(matches!(
            engine.sign_in_with_token("nope"),
            Err(EngineError::NotFound("token"))
        ));
        let first = engine.sign_in_with_token(&ann.token).unwrap();
        assert!(matches!(
            engine.session(&first.session).unwrap(),
            Some(SessionState::Active(Actor::User { .. }))
        ));
        let second = engine
            .sign_in_with_token(&format!(" {} ", ann.token))
            .unwrap();
        assert_eq!(
            engine.session(&first.session).unwrap(),
            Some(SessionState::Replaced)
        );
        assert!(matches!(
            engine.session(&second.session).unwrap(),
            Some(SessionState::Active(_))
        ));
        // The CLI keeps working alongside.
        assert!(engine.token_actor(&ann.token).unwrap().is_some());
        assert!(
            fakes
                .realtime
                .kicks()
                .contains(&Kick::Person(ann.user.id.clone()))
        );
        engine.sign_out(&second.session).unwrap();
        assert_eq!(engine.session(&second.session).unwrap(), None);
    }

    #[test]
    fn visitors_look_at_team_projects_until_turned_off() {
        let (engine, fakes) = engine_with_fakes();
        let admin = owner(&engine);
        engine.create_project(&admin, "web", "Web", None).unwrap();
        engine.create_project(&admin, "secret", "S", None).unwrap();
        engine
            .update_project(&admin, "secret", None, Some(Visibility::Private))
            .unwrap();
        assert_eq!(engine.visitor("not-an-id").unwrap(), None);
        let id = new_visitor_id();
        let visitor = engine.visitor(&id).unwrap().unwrap();
        assert_eq!(visitor.role(), Role::Viewer);
        assert_eq!(visitor.name(), visitor_name(&id));
        let keys: Vec<_> = engine
            .projects(&visitor)
            .unwrap()
            .into_iter()
            .map(|p| p.key)
            .collect();
        assert_eq!(keys, ["web"]);

        // Looking is not taking the whole project away.
        assert!(matches!(
            engine.checkout(&visitor, "web", None),
            Err(EngineError::Forbidden)
        ));

        assert_eq!(engine.server_visitors().unwrap(), VisitorAccess::View);
        engine
            .set_server_visitors(&admin, VisitorAccess::None)
            .unwrap();
        assert_eq!(engine.visitor(&id).unwrap(), None);
        assert!(fakes.realtime.kicks().contains(&Kick::Visitors));
        assert_eq!(
            engine.set_server_visitors(&visitor, VisitorAccess::View),
            Err(EngineError::Forbidden)
        );

        // A token is needed, but one project still lets visitors in, and only there.
        let web = engine
            .set_project_visitors(&admin, "web", Some(VisitorAccess::Comment))
            .unwrap();
        assert_eq!(
            (web.visitors, web.visitor_access),
            (Some(VisitorAccess::Comment), VisitorAccess::Comment)
        );
        let visitor = engine.visitor(&id).unwrap().unwrap();
        engine.create_project(&admin, "other", "O", None).unwrap();
        let keys: Vec<_> = engine
            .projects(&visitor)
            .unwrap()
            .into_iter()
            .map(|p| (p.key, p.visitor_access))
            .collect();
        assert_eq!(keys, [("web".to_owned(), VisitorAccess::Comment)]);
        // Private wins over the project's visitor setting.
        engine
            .set_project_visitors(&admin, "secret", Some(VisitorAccess::Comment))
            .unwrap();
        assert!(engine.project(&visitor, "secret").is_err());
        engine.set_project_visitors(&admin, "web", None).unwrap();
        assert_eq!(engine.visitor(&id).unwrap(), None);
    }

    #[test]
    fn visitor_names_follow_the_id() {
        assert_eq!(visitor_name("0000000000000000"), "Curious Otter");
        assert_eq!(visitor_name("0001000f00000000"), "Quiet Raccoon");
        assert_eq!(visitor_name("x"), "Curious Otter");
    }

    #[test]
    fn the_last_admin_stays() {
        let engine = engine();
        // No owner: the local actor is the built-in admin (a public server's first admin).
        let ann = engine
            .add_member(&Actor::Local, "Ann", Role::Admin)
            .unwrap();
        let demote = MemberChange {
            role: Some(Role::Editor),
            ..MemberChange::default()
        };
        assert!(matches!(
            engine.change_member(&Actor::Local, &ann.user.id, &demote),
            Err(EngineError::Invalid(_))
        ));
        assert!(matches!(
            engine.remove_member(&Actor::Local, &ann.user.id),
            Err(EngineError::Invalid(_))
        ));
    }
}
