//! Share links: people without an account come into one project with the role the link fixes
//! (story 02 §3). Revoking a link ends every session that came in through it.

use board_model::{Actor, Role, ShareLink};

use crate::auth::{SESSION_SECS, hash, now, random_token};
use crate::policy::Command;
use crate::{Engine, EngineError, NewShareLink};

/// A link just created. `token` is shown once: only its hash is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewLink {
    pub link: ShareLink,
    pub token: String,
}

/// What a link opens, shown before someone enters their name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedLink {
    pub project: String,
    pub project_title: String,
    pub role: Role,
}

/// The longest display name a guest may pick.
const MAX_NAME: usize = 40;

impl Engine {
    /// Creates a link into `project` for `role` (editor, commenter or viewer). Admins only.
    pub fn create_share_link(
        &self,
        actor: &Actor,
        project: &str,
        role: Role,
        label: &str,
    ) -> Result<NewLink, EngineError> {
        self.visible(actor, project, Command::Admin)?;
        if role == Role::Admin {
            return Err(EngineError::Invalid(
                "a share link gives editor, commenter or viewer".into(),
            ));
        }
        let token = random_token();
        let link = self.ports.accounts.insert_share_link(&NewShareLink {
            id: format!("l{}", &crate::auth::random_hex()[..10]),
            project: project.to_owned(),
            token_hash: hash(&token),
            role,
            label: label.trim().chars().take(80).collect(),
            created_by: actor.name().to_owned(),
        })?;
        Ok(NewLink { link, token })
    }

    pub fn share_links(&self, actor: &Actor, project: &str) -> Result<Vec<ShareLink>, EngineError> {
        self.visible(actor, project, Command::Admin)?;
        Ok(self.ports.accounts.share_links(project)?)
    }

    /// Revokes a link; whoever came in through it is out at once.
    pub fn revoke_share_link(
        &self,
        actor: &Actor,
        project: &str,
        id: &str,
    ) -> Result<(), EngineError> {
        self.visible(actor, project, Command::Admin)?;
        if !self.ports.accounts.revoke_share_link(project, id)? {
            return Err(EngineError::NotFound("share link"));
        }
        Ok(())
    }

    /// What `token` opens, for the name page. Revoked and unknown links look the same.
    pub fn shared_link(&self, token: &str) -> Result<SharedLink, EngineError> {
        let link = self.live_link(token)?;
        let project = self
            .ports
            .records
            .project(&link.project)?
            .ok_or(EngineError::NotFound("share link"))?;
        Ok(SharedLink {
            project: project.key,
            project_title: project.title,
            role: link.role,
        })
    }

    /// Lets someone in through a link under `name`; returns their session token.
    pub fn enter_share_link(&self, token: &str, name: &str) -> Result<String, EngineError> {
        let link = self.live_link(token)?;
        let name = name.trim();
        if name.is_empty() || name.chars().count() > MAX_NAME {
            return Err(EngineError::Invalid(format!(
                "a name of 1-{MAX_NAME} characters, please"
            )));
        }
        let guest = format!("g{}", &crate::auth::random_hex()[..10]);
        self.ports.accounts.insert_guest(&guest, &link.id, name)?;
        let session = random_token();
        self.ports
            .accounts
            .insert_guest_session(&hash(&session), &guest, now() + SESSION_SECS)?;
        Ok(session)
    }

    /// The guest actor behind a session token, if it is a live guest session.
    pub(crate) fn guest_actor(&self, token: &str) -> Result<Option<Actor>, EngineError> {
        Ok(self
            .ports
            .accounts
            .guest_session(&hash(token), now())?
            .map(|(guest, link)| Actor::Guest {
                id: guest.id,
                name: guest.name,
                role: link.role,
                project: link.project,
            }))
    }

    fn live_link(&self, token: &str) -> Result<ShareLink, EngineError> {
        self.ports
            .accounts
            .share_link_by_token(&hash(token))?
            .filter(|link| link.revoked_at.is_none())
            .ok_or(EngineError::NotFound("share link"))
    }
}

#[cfg(test)]
mod tests {
    use board_model::Role;

    use super::*;
    use crate::SessionState;
    use crate::policy::{Command, Target, can};
    use crate::testing::{engine, file};

    fn project(engine: &Engine) {
        engine
            .create_project(
                &Actor::Local,
                "web",
                "Web",
                Some(crate::Import {
                    files: vec![file("a.txt", "a")],
                    source: "test".into(),
                }),
            )
            .unwrap();
    }

    #[test]
    fn a_link_lets_guests_into_one_project_until_revoked() {
        let engine = engine();
        project(&engine);
        let created = engine
            .create_share_link(&Actor::Local, "web", Role::Commenter, " review ")
            .unwrap();
        assert_eq!(created.link.label, "review");
        let shared = engine.shared_link(&created.token).unwrap();
        assert_eq!(
            (shared.project_title.as_str(), shared.role),
            ("Web", Role::Commenter)
        );

        assert!(matches!(
            engine.enter_share_link(&created.token, "  "),
            Err(EngineError::Invalid(_))
        ));
        let session = engine.enter_share_link(&created.token, "Dana").unwrap();
        let Some(SessionState::Active(actor)) = engine.session(&session).unwrap() else {
            panic!("guest session");
        };
        assert!(matches!(
            &actor,
            Actor::Guest { name, role: Role::Commenter, project, .. } if name == "Dana" && project == "web"
        ));
        let target = Target {
            project: "web",
            visibility: board_model::Visibility::Team,
            member: false,
        };
        assert!(can(&actor, Command::CommentAdd, Some(target)));
        assert!(!can(&actor, Command::BranchEdit, Some(target)));
        assert_eq!(
            engine.share_links(&Actor::Local, "web").unwrap()[0].guests,
            1
        );

        engine
            .revoke_share_link(&Actor::Local, "web", &created.link.id)
            .unwrap();
        assert_eq!(
            engine.session(&session).unwrap(),
            None,
            "revoking ends the session"
        );
        assert_eq!(
            engine.shared_link(&created.token),
            Err(EngineError::NotFound("share link"))
        );
        assert!(
            engine.share_links(&Actor::Local, "web").unwrap()[0]
                .revoked_at
                .is_some()
        );
    }

    #[test]
    fn only_admins_make_links_and_never_admin_links() {
        let engine = engine();
        project(&engine);
        assert!(matches!(
            engine.create_share_link(&Actor::Local, "web", Role::Admin, ""),
            Err(EngineError::Invalid(_))
        ));
        let editor = Actor::User {
            id: "u".into(),
            name: "U".into(),
            email: "u@example.com".into(),
            role: Role::Editor,
        };
        assert_eq!(
            engine
                .create_share_link(&editor, "web", Role::Viewer, "")
                .map(|_| ()),
            Err(EngineError::Forbidden)
        );
        assert_eq!(
            engine
                .create_share_link(&Actor::Local, "nope", Role::Viewer, "")
                .map(|_| ()),
            Err(EngineError::NotFound("project"))
        );
    }
}
