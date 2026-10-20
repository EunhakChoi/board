//! Who may run which command. The single place permissions are decided (`docs/design/security.md`).

use charet_model::{Actor, Role, Visibility, VisitorAccess};

/// Every command that needs a permission check. Names match the command registry (`area.verb`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    /// See a project: screens, flows, code, versions, presence.
    ProjectView,
    /// Download the whole project (`charet checkout`). Members only, never visitors.
    Checkout,
    CommentAdd,
    /// Edit by hand or with the agent, request review.
    BranchEdit,
    BranchMerge,
    VersionRestore,
    /// Members and their tokens, sharing, webhooks, project settings.
    Admin,
    /// Export and mark handoff, import into a project (CLI only).
    Handoff,
}

impl Command {
    /// The least role that may run the command.
    pub fn required(self) -> Role {
        match self {
            Self::ProjectView | Self::Checkout => Role::Viewer,
            Self::CommentAdd => Role::Commenter,
            Self::BranchEdit | Self::BranchMerge | Self::VersionRestore => Role::Editor,
            Self::Admin | Self::Handoff => Role::Admin,
        }
    }
}

/// The project a command acts on, as far as permissions care.
#[derive(Debug, Clone, Copy)]
pub struct Target<'a> {
    pub project: &'a str,
    pub visibility: Visibility,
    /// Whether the actor is listed as a member of this (private) project.
    pub member: bool,
    /// What visitors may do here (the project's setting, else the server's).
    pub visitors: VisitorAccess,
}

/// Decides whether `actor` may run `command` on `target` (`None` = server-wide, e.g. admin settings).
pub fn can(actor: &Actor, command: Command, target: Option<Target<'_>>) -> bool {
    if let Actor::Visitor { .. } = actor {
        // Visitors act only on the team's projects that let them, and nothing server-wide.
        let Some(target) = target else { return false };
        let allowed = match target.visitors {
            VisitorAccess::None => false,
            VisitorAccess::View => command == Command::ProjectView,
            VisitorAccess::Comment => {
                matches!(command, Command::ProjectView | Command::CommentAdd)
            }
        };
        return allowed && target.visibility == Visibility::Team;
    }
    if actor.role() < command.required() {
        return false;
    }
    match (actor, target) {
        (Actor::Local | Actor::User { .. }, None) | (Actor::Local, Some(_)) => true,
        (Actor::Visitor { .. }, _) => false,
        (Actor::User { role, .. }, Some(target)) => {
            *role == Role::Admin || target.visibility == Visibility::Team || target.member
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(role: Role) -> Actor {
        Actor::User {
            id: "u".into(),
            name: "U".into(),
            role,
        }
    }

    const TEAM: Option<Target<'static>> = Some(Target {
        project: "p",
        visibility: Visibility::Team,
        member: false,
        visitors: VisitorAccess::View,
    });

    /// Story 02 permission table: each row is a command, columns are admin, editor, commenter, viewer.
    #[test]
    fn role_table() {
        let rows = [
            (Command::ProjectView, [true, true, true, true]),
            (Command::Checkout, [true, true, true, true]),
            (Command::CommentAdd, [true, true, true, false]),
            (Command::BranchEdit, [true, true, false, false]),
            (Command::BranchMerge, [true, true, false, false]),
            (Command::VersionRestore, [true, true, false, false]),
            (Command::Admin, [true, false, false, false]),
            (Command::Handoff, [true, false, false, false]),
        ];
        let roles = [Role::Admin, Role::Editor, Role::Commenter, Role::Viewer];
        for (command, expected) in rows {
            for (role, allowed) in roles.iter().zip(expected) {
                assert_eq!(
                    can(&user(*role), command, TEAM),
                    allowed,
                    "{role:?} {command:?}"
                );
            }
        }
    }

    #[test]
    fn visitors_do_what_the_project_lets_them() {
        let visitor = Actor::Visitor {
            id: "v".into(),
            name: "V".into(),
        };
        let at = |visitors| {
            Some(Target {
                visitors,
                ..TEAM.unwrap()
            })
        };
        // Columns: none, view, comment.
        let rows = [
            (Command::ProjectView, [false, true, true]),
            (Command::CommentAdd, [false, false, true]),
            (Command::Checkout, [false, false, false]),
            (Command::BranchEdit, [false, false, false]),
            (Command::Admin, [false, false, false]),
        ];
        let levels = [
            VisitorAccess::None,
            VisitorAccess::View,
            VisitorAccess::Comment,
        ];
        for (command, expected) in rows {
            for (level, allowed) in levels.iter().zip(expected) {
                assert_eq!(
                    can(&visitor, command, at(*level)),
                    allowed,
                    "{level:?} {command:?}"
                );
            }
        }
        assert!(!can(&visitor, Command::ProjectView, None));
        let private = Some(Target {
            visibility: Visibility::Private,
            visitors: VisitorAccess::Comment,
            ..TEAM.unwrap()
        });
        assert!(!can(&visitor, Command::ProjectView, private));
    }

    #[test]
    fn private_projects_need_membership() {
        let private = Target {
            visibility: Visibility::Private,
            ..TEAM.unwrap()
        };
        assert!(!can(
            &user(Role::Editor),
            Command::ProjectView,
            Some(private)
        ));
        assert!(can(
            &user(Role::Editor),
            Command::ProjectView,
            Some(Target {
                member: true,
                ..private
            })
        ));
        assert!(can(&user(Role::Admin), Command::ProjectView, Some(private)));
    }

    #[test]
    fn local_mode_can_do_everything() {
        assert!(can(&Actor::Local, Command::Handoff, None));
    }
}
