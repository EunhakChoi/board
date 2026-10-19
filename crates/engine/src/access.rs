//! Admin commands over who gets in: the allowlist, access requests and members (story 02).

use board_model::{Actor, Role, User, UserStatus};

use crate::policy::{Command, can};
use crate::{AllowRule, Engine, EngineError};

/// What an admin changes about a member. `None` keeps the current value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AccessChange {
    pub role: Option<Role>,
    pub status: Option<UserStatus>,
}

impl Engine {
    pub fn allow_rules(&self, actor: &Actor) -> Result<Vec<AllowRule>, EngineError> {
        admin(actor)?;
        Ok(self.ports.accounts.allow_rules()?)
    }

    /// Adds a `domain` (`example.com`) or `email` rule. Adding one that exists returns it.
    pub fn add_allow_rule(
        &self,
        actor: &Actor,
        kind: &str,
        value: &str,
    ) -> Result<AllowRule, EngineError> {
        admin(actor)?;
        let value = value.trim().trim_start_matches('@').to_lowercase();
        let valid = match kind {
            "domain" => valid_domain(&value),
            "email" => value
                .rsplit_once('@')
                .is_some_and(|(local, domain)| !local.is_empty() && valid_domain(domain)),
            _ => return Err(EngineError::Invalid("kind must be domain or email".into())),
        };
        if !valid {
            return Err(EngineError::Invalid(format!(
                "{value:?} is not a valid {kind}"
            )));
        }
        Ok(self.ports.accounts.add_allow_rule(kind, &value)?)
    }

    pub fn remove_allow_rule(&self, actor: &Actor, id: i64) -> Result<(), EngineError> {
        admin(actor)?;
        if !self.ports.accounts.remove_allow_rule(id)? {
            return Err(EngineError::NotFound("allow rule"));
        }
        Ok(())
    }

    /// Every account: members, access requests (`pending`), declined and removed people.
    pub fn users(&self, actor: &Actor) -> Result<Vec<User>, EngineError> {
        admin(actor)?;
        Ok(self.ports.accounts.users()?)
    }

    /// Approves (as an editor) or declines an access request.
    pub fn decide_access(
        &self,
        actor: &Actor,
        id: &str,
        approve: bool,
    ) -> Result<User, EngineError> {
        admin(actor)?;
        let user = self.account(id)?;
        if user.status != UserStatus::Pending {
            return Err(EngineError::Invalid(format!(
                "{} is not waiting for approval",
                user.email
            )));
        }
        let status = if approve {
            UserStatus::Active
        } else {
            UserStatus::Rejected
        };
        self.ports.accounts.set_access(id, user.role, status)?;
        self.access_changed(id, status);
        self.account(id)
    }

    /// Changes a member's role, removes them (ending their sessions) or lets them back in.
    pub fn change_access(
        &self,
        actor: &Actor,
        id: &str,
        change: AccessChange,
    ) -> Result<User, EngineError> {
        admin(actor)?;
        let user = self.account(id)?;
        let role = change.role.unwrap_or(user.role);
        let status = change.status.unwrap_or(user.status);
        if status == UserStatus::Pending {
            return Err(EngineError::Invalid(
                "an account cannot be put back to waiting".into(),
            ));
        }
        let was_admin = user.role == Role::Admin && user.status == UserStatus::Active;
        let stays_admin = role == Role::Admin && status == UserStatus::Active;
        if was_admin && !stays_admin && self.active_admins()? == 1 {
            return Err(EngineError::Invalid(
                "this is the last admin; make someone else admin first".into(),
            ));
        }
        self.ports.accounts.set_access(id, role, status)?;
        self.access_changed(id, status);
        self.account(id)
    }

    /// Tells admins' Team pages, and closes the connections of someone who lost access.
    fn access_changed(&self, id: &str, status: UserStatus) {
        self.ports
            .realtime
            .publish_admins("access.changed", serde_json::json!({ "user": id }));
        if matches!(status, UserStatus::Rejected | UserStatus::Removed) {
            self.ports.realtime.kick(crate::Kick::Person(id.to_owned()));
        }
    }

    fn account(&self, id: &str) -> Result<User, EngineError> {
        self.ports
            .accounts
            .user(id)?
            .ok_or(EngineError::NotFound("user"))
    }

    fn active_admins(&self) -> Result<usize, EngineError> {
        Ok(self
            .ports
            .accounts
            .users()?
            .iter()
            .filter(|u| u.role == Role::Admin && u.status == UserStatus::Active)
            .count())
    }
}

pub(crate) fn admin(actor: &Actor) -> Result<(), EngineError> {
    if can(actor, Command::Admin, None) {
        Ok(())
    } else {
        Err(EngineError::Forbidden)
    }
}

/// `example.com`, `team.example.co.kr`: dot-separated labels of letters, digits and dashes.
fn valid_domain(value: &str) -> bool {
    value.contains('.')
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
}

#[cfg(test)]
mod tests {
    use board_model::{Role, UserStatus, Visibility};

    use super::*;
    use crate::testing::{FakeIdentity, engine_with_sign_in};
    use crate::{SessionState, SignIn};

    fn join(engine: &Engine, idp: &FakeIdentity, who: &str) -> (String, User) {
        engine.begin_sign_in(None, None).unwrap();
        idp.next(who, &format!("{who}@example.com"), true);
        match engine.finish_sign_in(&idp.last_state(), "c").unwrap() {
            SignIn::Session { token, user, .. } => (token, user),
            SignIn::Refused(r) => panic!("{r:?}"),
        }
    }

    #[test]
    fn allowlist_rules_are_checked_and_listed() {
        let (engine, idp) = engine_with_sign_in(&["boss@example.com"]);
        let (_, boss) = join(&engine, &idp, "boss");
        let admin = boss.actor();
        assert!(
            engine
                .add_allow_rule(&admin, "domain", "Example.org")
                .is_ok()
        );
        assert!(
            engine
                .add_allow_rule(&admin, "domain", "@team.example.co.kr")
                .is_ok()
        );
        assert!(
            engine
                .add_allow_rule(&admin, "email", "ann@other.example")
                .is_ok()
        );
        for (kind, bad) in [
            ("domain", "nodot"),
            ("domain", "a..b"),
            ("email", "@x.y"),
            ("x", "a.b"),
        ] {
            assert!(
                matches!(
                    engine.add_allow_rule(&admin, kind, bad),
                    Err(EngineError::Invalid(_))
                ),
                "{kind} {bad}"
            );
        }
        let rules = engine.allow_rules(&admin).unwrap();
        assert_eq!(rules.len(), 3);
        assert!(rules.iter().any(|r| r.value == "team.example.co.kr"));
        engine.remove_allow_rule(&admin, rules[0].id).unwrap();
        assert_eq!(engine.allow_rules(&admin).unwrap().len(), 2);
        assert_eq!(
            engine.remove_allow_rule(&admin, 999),
            Err(EngineError::NotFound("allow rule"))
        );
    }

    #[test]
    fn admins_approve_decline_and_remove() {
        let (engine, idp) = engine_with_sign_in(&["boss@example.com"]);
        let (_, boss) = join(&engine, &idp, "boss");
        let admin = boss.actor();
        let (eve_token, eve) = join(&engine, &idp, "eve");
        let (_, mal) = join(&engine, &idp, "mal");
        // Members cannot run admin commands.
        assert_eq!(engine.users(&eve.actor()), Err(EngineError::Forbidden));

        let pending: Vec<_> = engine
            .users(&admin)
            .unwrap()
            .into_iter()
            .filter(|u| u.status == UserStatus::Pending)
            .map(|u| u.id)
            .collect();
        assert_eq!(pending, [eve.id.clone(), mal.id.clone()]);

        let eve = engine.decide_access(&admin, &eve.id, true).unwrap();
        assert_eq!(eve.status, UserStatus::Active);
        assert!(matches!(
            engine.session(&eve_token).unwrap(),
            Some(SessionState::Active(_))
        ));
        assert!(
            engine.decide_access(&admin, &eve.id, true).is_err(),
            "decided already"
        );
        let mal = engine.decide_access(&admin, &mal.id, false).unwrap();
        assert_eq!(mal.status, UserStatus::Rejected);

        let eve = engine
            .change_access(
                &admin,
                &eve.id,
                AccessChange {
                    role: Some(Role::Commenter),
                    status: None,
                },
            )
            .unwrap();
        assert_eq!(eve.role, Role::Commenter);
        engine
            .change_access(
                &admin,
                &eve.id,
                AccessChange {
                    role: None,
                    status: Some(UserStatus::Removed),
                },
            )
            .unwrap();
        assert!(matches!(
            engine.session(&eve_token).unwrap(),
            None | Some(SessionState::Refused(_))
        ));
    }

    #[test]
    fn private_projects_show_only_to_members_and_admins() {
        let (engine, idp) = engine_with_sign_in(&["boss@example.com"]);
        let (_, boss) = join(&engine, &idp, "boss");
        let admin = boss.actor();
        crate::testing::allow(&engine, "domain", "example.com");
        let (_, ann) = join(&engine, &idp, "ann");
        let (_, bob) = join(&engine, &idp, "bob");
        engine
            .create_project(&admin, "secret", "Secret", None)
            .unwrap();
        let project = engine
            .update_project(
                &admin,
                "secret",
                Some(" Top secret "),
                Some(Visibility::Private),
            )
            .unwrap();
        assert_eq!(
            (project.title.as_str(), project.visibility),
            ("Top secret", Visibility::Private)
        );
        engine.set_member(&admin, "secret", &ann.id, true).unwrap();
        assert_eq!(
            engine.members(&admin, "secret").unwrap(),
            std::slice::from_ref(&ann.id)
        );

        let keys = |actor: &Actor| -> Vec<String> {
            engine
                .projects(actor)
                .unwrap()
                .into_iter()
                .map(|p| p.key)
                .collect()
        };
        assert_eq!(keys(&ann.actor()), ["secret"]);
        assert_eq!(keys(&bob.actor()), Vec::<String>::new());
        assert_eq!(
            engine.project(&bob.actor(), "secret"),
            Err(EngineError::NotFound("project")),
            "hidden, not forbidden"
        );
        assert_eq!(
            engine.members(&ann.actor(), "secret"),
            Err(EngineError::Forbidden)
        );
        assert_eq!(
            engine.set_member(&admin, "secret", "nobody", true),
            Err(EngineError::NotFound("user"))
        );

        engine.set_member(&admin, "secret", &ann.id, false).unwrap();
        assert_eq!(keys(&ann.actor()), Vec::<String>::new());
        engine
            .update_project(&admin, "secret", None, Some(Visibility::Team))
            .unwrap();
        assert_eq!(keys(&bob.actor()), ["secret"]);
    }

    #[test]
    fn the_last_admin_stays() {
        let (engine, idp) = engine_with_sign_in(&["boss@example.com", "ann@example.com"]);
        let (_, boss) = join(&engine, &idp, "boss");
        let demote = AccessChange {
            role: Some(Role::Editor),
            status: None,
        };
        assert!(matches!(
            engine.change_access(&boss.actor(), &boss.id, demote),
            Err(EngineError::Invalid(_))
        ));
        let (_, ann) = join(&engine, &idp, "ann");
        assert!(
            engine
                .change_access(&boss.actor(), &boss.id, demote)
                .is_ok()
        );
        assert!(
            matches!(
                engine.change_access(&ann.actor(), &ann.id, demote),
                Err(EngineError::Invalid(_))
            ),
            "now ann is the last admin"
        );
    }
}
