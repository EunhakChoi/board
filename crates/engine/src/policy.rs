//! Who may run which command. The single place permissions are decided (`docs/design/security.md`).

use board_model::{Actor, Role, Visibility};

/// Every command that needs a permission check. Names match the command registry (`area.verb`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    /// See a project: screens, flows, versions, presence.
    ProjectView,
    CommentAdd,
    /// Edit by hand or with the agent, request review.
    BranchEdit,
    BranchMerge,
    VersionRestore,
    /// Approve access requests, issue share links, keys, webhooks, project settings.
    Admin,
    /// Export and mark handoff, import into a project (CLI only).
    Handoff,
}

impl Command {
    /// The least role that may run the command.
    pub fn required(self) -> Role {
        match self {
            Self::ProjectView => Role::Viewer,
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
}

/// Decides whether `actor` may run `command` on `target` (`None` = server-wide, e.g. admin settings).
pub fn can(actor: &Actor, command: Command, target: Option<Target<'_>>) -> bool {
    if actor.role() < command.required() {
        return false;
    }
    match (actor, target) {
        (Actor::Local | Actor::User { .. }, None) | (Actor::Local, Some(_)) => true,
        // A share link opens exactly one project and never server-wide settings.
        (Actor::Guest { project, .. }, Some(target)) => {
            project == target.project && command != Command::Admin && command != Command::Handoff
        }
        (Actor::Guest { .. }, None) => false,
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
            email: "u@example.com".into(),
            role,
        }
    }

    fn guest(role: Role, project: &str) -> Actor {
        Actor::Guest {
            id: "g".into(),
            name: "G".into(),
            role,
            project: project.into(),
        }
    }

    const TEAM: Option<Target<'static>> = Some(Target {
        project: "p",
        visibility: Visibility::Team,
        member: false,
    });

    /// Story 02 permission table: each row is a command, columns are admin, editor, commenter, viewer.
    #[test]
    fn role_table() {
        let rows = [
            (Command::ProjectView, [true, true, true, true]),
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
    fn guests_reach_one_project_and_never_admin() {
        let editor = guest(Role::Editor, "p");
        assert!(can(&editor, Command::BranchMerge, TEAM));
        let other = Some(Target {
            project: "q",
            visibility: Visibility::Team,
            member: false,
        });
        assert!(!can(&editor, Command::ProjectView, other));
        assert!(!can(&editor, Command::ProjectView, None));
        assert!(!can(&guest(Role::Admin, "p"), Command::Admin, TEAM));
    }

    #[test]
    fn private_projects_need_membership() {
        let private = Target {
            project: "p",
            visibility: Visibility::Private,
            member: false,
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
