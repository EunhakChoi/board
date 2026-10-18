//! Project commands: create, import, checkout, push, versions, diff.

use board_model::{Actor, Origin, Project, Version, VersionKind, Visibility, valid_path};

use crate::policy::{Command, Target, can};
use crate::{Author, Change, Engine, EngineError, FileChange};

/// Files brought in from a product repo, with a note on where they came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub files: Vec<FileChange>,
    /// Shown on version 1, e.g. `git@host:org/repo.git @ 1a2b3c4`.
    pub source: String,
}

/// A project's files at one commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkout {
    pub commit: String,
    /// Gzipped tar of the files.
    pub archive: Vec<u8>,
}

impl Engine {
    /// Creates a project, empty or from imported files. Admins only.
    pub fn create_project(
        &self,
        actor: &Actor,
        key: &str,
        title: &str,
        import: Option<Import>,
    ) -> Result<(Project, Version), EngineError> {
        if !can(actor, Command::Admin, None) {
            return Err(EngineError::Forbidden);
        }
        if !Project::valid_key(key) {
            return Err(EngineError::Invalid(format!(
                "project key {key:?} must be 1-63 lowercase letters, digits or dashes"
            )));
        }
        let title = if title.trim().is_empty() {
            key
        } else {
            title.trim()
        };
        let (origin, files, note) = match import {
            Some(import) => {
                check_files(&import.files)?;
                (
                    Origin::Import,
                    import.files,
                    format!("Imported from {}", import.source),
                )
            }
            None => (Origin::Empty, Vec::new(), "Created".to_owned()),
        };
        let project = Project {
            key: key.to_owned(),
            title: title.to_owned(),
            visibility: Visibility::Team,
            origin,
            created_at: String::new(),
        };
        if !self.ports.records.insert_project(&project)? {
            return Err(EngineError::Exists("project"));
        }
        self.ports.store.create(key)?;
        let commit = self
            .ports
            .store
            .commit_main(key, None, &files, &author(actor), &note)?
            .ok_or_else(|| EngineError::Invalid("project store was not empty".into()))?;
        let kind = if origin == Origin::Import {
            VersionKind::Import
        } else {
            VersionKind::Initial
        };
        let version = self.record(key, &commit, kind, actor, &note)?;
        let project = self
            .ports
            .records
            .project(key)?
            .ok_or(EngineError::NotFound("project"))?;
        Ok((project, version))
    }

    /// Projects the actor can see.
    pub fn projects(&self, actor: &Actor) -> Result<Vec<Project>, EngineError> {
        Ok(self
            .ports
            .records
            .projects()?
            .into_iter()
            .filter(|project| can(actor, Command::ProjectView, Some(target(project))))
            .collect())
    }

    pub fn project(&self, actor: &Actor, key: &str) -> Result<Project, EngineError> {
        self.visible(actor, key, Command::ProjectView)
    }

    /// The files at `rev` (`main` when `None`).
    pub fn checkout(
        &self,
        actor: &Actor,
        key: &str,
        rev: Option<&str>,
    ) -> Result<Checkout, EngineError> {
        self.visible(actor, key, Command::ProjectView)?;
        let commit = self.resolve(key, rev.unwrap_or("main"))?;
        let archive = self.ports.store.archive(key, &commit)?;
        Ok(Checkout { commit, archive })
    }

    /// Commits `changes` to `main` on top of `base` and records a version. Fails with
    /// [`EngineError::Conflict`] when `main` has moved since `base`.
    pub fn push(
        &self,
        actor: &Actor,
        key: &str,
        base: &str,
        message: &str,
        changes: &[FileChange],
    ) -> Result<Version, EngineError> {
        self.visible(actor, key, Command::BranchEdit)?;
        if changes.is_empty() {
            return Err(EngineError::Invalid("nothing to push".into()));
        }
        check_files(changes)?;
        let head = self
            .ports
            .store
            .head(key)?
            .ok_or(EngineError::NotFound("main"))?;
        if head != base {
            return Err(EngineError::Conflict { head });
        }
        let message = if message.trim().is_empty() {
            "Push"
        } else {
            message.trim()
        };
        let commit = self
            .ports
            .store
            .commit_main(key, Some(base), changes, &author(actor), message)?
            .ok_or_else(|| EngineError::Conflict {
                head: self
                    .ports
                    .store
                    .head(key)
                    .ok()
                    .flatten()
                    .unwrap_or_default(),
            })?;
        self.record(key, &commit, VersionKind::Push, actor, message)
    }

    /// Versions of `main`, newest first.
    pub fn versions(&self, actor: &Actor, key: &str) -> Result<Vec<Version>, EngineError> {
        self.visible(actor, key, Command::ProjectView)?;
        Ok(self.ports.records.versions(key)?)
    }

    /// Paths changed from `from` to `to` (each `main`, `v<n>` or a commit).
    pub fn diff(
        &self,
        actor: &Actor,
        key: &str,
        from: &str,
        to: &str,
    ) -> Result<Vec<Change>, EngineError> {
        self.visible(actor, key, Command::ProjectView)?;
        let from = self.resolve(key, from)?;
        let to = self.resolve(key, to)?;
        Ok(self.ports.store.diff(key, &from, &to)?)
    }

    pub(crate) fn visible(
        &self,
        actor: &Actor,
        key: &str,
        command: Command,
    ) -> Result<Project, EngineError> {
        let project = self
            .ports
            .records
            .project(key)?
            .ok_or(EngineError::NotFound("project"))?;
        if !can(actor, Command::ProjectView, Some(target(&project))) {
            // Hidden projects look missing rather than forbidden.
            return Err(EngineError::NotFound("project"));
        }
        if !can(actor, command, Some(target(&project))) {
            return Err(EngineError::Forbidden);
        }
        Ok(project)
    }

    pub(crate) fn resolve(&self, key: &str, rev: &str) -> Result<String, EngineError> {
        self.ports
            .store
            .resolve(key, rev)?
            .ok_or(EngineError::NotFound("revision"))
    }

    fn record(
        &self,
        key: &str,
        commit: &str,
        kind: VersionKind,
        actor: &Actor,
        note: &str,
    ) -> Result<Version, EngineError> {
        let version = self
            .ports
            .records
            .add_version(key, commit, kind, actor.name(), note)?;
        self.ports.store.tag_version(key, version.number, commit)?;
        Ok(version)
    }
}

fn target(project: &Project) -> Target<'_> {
    // Membership of private projects arrives with sign-in (P1).
    Target {
        project: &project.key,
        visibility: project.visibility,
        member: false,
    }
}

fn author(actor: &Actor) -> Author {
    match actor {
        Actor::Local => Author {
            name: "Local admin".into(),
            email: "local@board.invalid".into(),
        },
        Actor::User { name, email, .. } => Author {
            name: name.clone(),
            email: email.clone(),
        },
        Actor::Guest { id, name, .. } => Author {
            name: name.clone(),
            email: format!("{id}@guest.board.invalid"),
        },
    }
}

fn check_files(files: &[FileChange]) -> Result<(), EngineError> {
    let mut seen = std::collections::HashSet::new();
    for file in files {
        if !valid_path(&file.path) {
            return Err(EngineError::Invalid(format!(
                "invalid path {:?}",
                file.path
            )));
        }
        if !seen.insert(file.path.as_str()) {
            return Err(EngineError::Invalid(format!(
                "path {:?} appears twice",
                file.path
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::engine;
    use board_model::Role;

    fn file(path: &str, content: &str) -> FileChange {
        FileChange {
            path: path.into(),
            content: Some(content.as_bytes().to_vec()),
            executable: false,
        }
    }

    fn viewer() -> Actor {
        Actor::User {
            id: "v".into(),
            name: "V".into(),
            email: "v@example.com".into(),
            role: Role::Viewer,
        }
    }

    #[test]
    fn create_empty_project_records_version_one() {
        let engine = engine();
        let (project, version) = engine
            .create_project(&Actor::Local, "web", " Web ", None)
            .unwrap();
        assert_eq!(
            (project.title.as_str(), project.origin),
            ("Web", Origin::Empty)
        );
        assert_eq!((version.number, version.kind), (1, VersionKind::Initial));
        assert_eq!(engine.projects(&Actor::Local).unwrap().len(), 1);
    }

    #[test]
    fn import_then_push_then_conflict() {
        let engine = engine();
        let import = Import {
            files: vec![file("a.txt", "1")],
            source: "local".into(),
        };
        let (_, v1) = engine
            .create_project(&Actor::Local, "web", "", Some(import))
            .unwrap();
        assert_eq!(v1.kind, VersionKind::Import);
        let v2 = engine
            .push(
                &Actor::Local,
                "web",
                &v1.commit,
                "edit",
                &[file("a.txt", "2")],
            )
            .unwrap();
        assert_eq!(v2.number, 2);
        // A second push from the stale base is refused with the current head.
        let stale = engine.push(
            &Actor::Local,
            "web",
            &v1.commit,
            "late",
            &[file("b.txt", "x")],
        );
        assert_eq!(
            stale,
            Err(EngineError::Conflict {
                head: v2.commit.clone()
            })
        );
        assert_eq!(engine.versions(&Actor::Local, "web").unwrap()[0].number, 2);
        let changes = engine.diff(&Actor::Local, "web", "v1", "main").unwrap();
        assert_eq!(
            changes,
            vec![Change {
                path: "a.txt".into(),
                kind: crate::ChangeKind::Modified
            }]
        );
    }

    #[test]
    fn rules() {
        let engine = engine();
        assert_eq!(
            engine.create_project(&viewer(), "web", "", None),
            Err(EngineError::Forbidden)
        );
        assert!(matches!(
            engine.create_project(&Actor::Local, "Bad Key", "", None),
            Err(EngineError::Invalid(_))
        ));
        engine
            .create_project(&Actor::Local, "web", "", None)
            .unwrap();
        assert_eq!(
            engine.create_project(&Actor::Local, "web", "", None),
            Err(EngineError::Exists("project"))
        );
        let head = engine.checkout(&viewer(), "web", None).unwrap().commit;
        assert_eq!(
            engine.push(&viewer(), "web", &head, "", &[file("a", "1")]),
            Err(EngineError::Forbidden)
        );
        assert!(matches!(
            engine.push(&Actor::Local, "web", &head, "", &[file("../a", "1")]),
            Err(EngineError::Invalid(_))
        ));
        assert!(matches!(
            engine.push(&Actor::Local, "web", &head, "", &[]),
            Err(EngineError::Invalid(_))
        ));
        assert_eq!(
            engine.project(&Actor::Local, "nope"),
            Err(EngineError::NotFound("project"))
        );
    }
}
