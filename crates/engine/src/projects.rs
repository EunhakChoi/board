//! Project commands: create, import, delete, checkout, push, versions, diff.

use charet_model::{Actor, Origin, Project, Version, VersionKind, Visibility, valid_path};

use crate::policy::{Command, Target, can};
use crate::{Author, Change, Engine, EngineError, FileChange, NewVersion};

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
            .filter(|project| {
                self.target(actor, project)
                    .is_ok_and(|target| can(actor, Command::ProjectView, Some(target)))
            })
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
        if actor.role() >= charet_model::Role::Editor {
            // Shown as "working locally" until they push.
            self.ports.realtime.working(key, &actor.person(), true);
        }
        Ok(Checkout { commit, archive })
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

    /// Deletes a project: its history, versions, captures and comments. Admins only; there is no
    /// undo.
    pub fn delete_project(&self, actor: &Actor, key: &str) -> Result<(), EngineError> {
        self.visible(actor, key, Command::ProjectView)?;
        if !can(actor, Command::Admin, None) {
            return Err(EngineError::Forbidden);
        }
        // The record first: once it is gone the project is gone, and leftover files are only
        // cleared again (a new project with the same key starts from an empty store).
        if !self.ports.records.delete_project(key)? {
            return Err(EngineError::NotFound("project"));
        }
        self.publish(key, "project.deleted", serde_json::json!({}));
        self.ports.store.remove(key)?;
        self.ports.captures.remove_project(key)?;
        self.ports.comments.remove_project(key)?;
        Ok(())
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
        let target = self.target(actor, &project)?;
        if !can(actor, Command::ProjectView, Some(target)) {
            // Hidden projects look missing rather than forbidden.
            return Err(EngineError::NotFound("project"));
        }
        if !can(actor, command, Some(target)) {
            return Err(EngineError::Forbidden);
        }
        Ok(project)
    }

    /// The project as permissions see it for `actor`: membership only matters when it is private.
    fn target<'a>(&self, actor: &Actor, project: &'a Project) -> Result<Target<'a>, EngineError> {
        let member = match actor {
            Actor::User { id, .. } if project.visibility == Visibility::Private => {
                self.ports.records.is_member(&project.key, id)?
            }
            _ => false,
        };
        Ok(Target {
            project: &project.key,
            visibility: project.visibility,
            member,
        })
    }

    /// Renames a project or changes who can see it. Admins only.
    pub fn update_project(
        &self,
        actor: &Actor,
        key: &str,
        title: Option<&str>,
        visibility: Option<Visibility>,
    ) -> Result<Project, EngineError> {
        let project = self.visible(actor, key, Command::Admin)?;
        let title = title.map_or(project.title.as_str(), str::trim);
        if title.is_empty() {
            return Err(EngineError::Invalid("a project needs a title".into()));
        }
        self.ports
            .records
            .update_project(key, title, visibility.unwrap_or(project.visibility))?;
        self.publish(key, "project.updated", serde_json::json!({}));
        self.ports
            .records
            .project(key)?
            .ok_or(EngineError::NotFound("project"))
    }

    /// Who may see a private project besides admins (user ids).
    pub fn members(&self, actor: &Actor, key: &str) -> Result<Vec<String>, EngineError> {
        self.visible(actor, key, Command::Admin)?;
        Ok(self.ports.records.members(key)?)
    }

    /// Adds or removes a member of a project (it only matters while the project is private).
    pub fn set_member(
        &self,
        actor: &Actor,
        key: &str,
        user: &str,
        member: bool,
    ) -> Result<(), EngineError> {
        self.visible(actor, key, Command::Admin)?;
        if member && self.ports.accounts.user(user)?.is_none() {
            return Err(EngineError::NotFound("user"));
        }
        Ok(self.ports.records.set_member(key, user, member)?)
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
        self.record_version(
            key,
            actor,
            &NewVersion {
                commit: commit.to_owned(),
                kind,
                actor: actor.name().to_owned(),
                note: note.to_owned(),
                branch: None,
                restored: None,
            },
        )
    }

    /// Records a version of `main`, tags it and tells everyone.
    pub(crate) fn record_version(
        &self,
        key: &str,
        actor: &Actor,
        new: &NewVersion,
    ) -> Result<Version, EngineError> {
        let (commit, note) = (new.commit.as_str(), new.note.as_str());
        let version = self.ports.records.add_version(key, new)?;
        self.ports.store.tag_version(key, version.number, commit)?;
        self.publish(
            key,
            "version.created",
            serde_json::json!({ "number": version.number, "actor": actor.name() }),
        );
        if let Ok(Some(project)) = self.ports.records.project(key) {
            self.version_created(actor, &project, version.number, note);
        }
        Ok(version)
    }
}

pub(crate) fn author(actor: &Actor) -> Author {
    match actor {
        Actor::Local => Author {
            name: "Local admin".into(),
            email: "local@charet.invalid".into(),
        },
        Actor::User { id, name, .. } => Author {
            name: name.clone(),
            email: format!("{id}@member.invalid"),
        },
        Actor::Visitor { id, name } => Author {
            name: name.clone(),
            email: format!("{id}@visitor.invalid"),
        },
    }
}

pub(crate) fn check_files(files: &[FileChange]) -> Result<(), EngineError> {
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
    use charet_model::Role;

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
    fn import_then_merge_a_branch() {
        let engine = engine();
        let import = Import {
            files: vec![file("a.txt", "1")],
            source: "local".into(),
        };
        let (_, v1) = engine
            .create_project(&Actor::Local, "web", "", Some(import))
            .unwrap();
        assert_eq!(v1.kind, VersionKind::Import);
        engine
            .propose(
                &Actor::Local,
                "web",
                &v1.commit,
                "edit",
                &[file("a.txt", "2")],
            )
            .unwrap();
        let v2 = engine.merge_branch(&Actor::Local, "web", 1).unwrap();
        assert_eq!((v2.number, v2.kind), (2, VersionKind::Merge));
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
            engine.propose(&viewer(), "web", &head, "", &[file("a", "1")]),
            Err(EngineError::Forbidden)
        );
        assert!(matches!(
            engine.propose(&Actor::Local, "web", &head, "", &[file("../a", "1")]),
            Err(EngineError::Invalid(_))
        ));
        assert!(matches!(
            engine.propose(&Actor::Local, "web", &head, "", &[]),
            Err(EngineError::Invalid(_))
        ));
        assert_eq!(
            engine.project(&Actor::Local, "nope"),
            Err(EngineError::NotFound("project"))
        );
    }

    #[test]
    fn delete_project_removes_everything_and_frees_the_key() {
        let engine = engine();
        let import = Import {
            files: vec![file("a.txt", "1")],
            source: "local".into(),
        };
        engine
            .create_project(&Actor::Local, "web", "", Some(import))
            .unwrap();
        assert_eq!(
            engine.delete_project(&viewer(), "web"),
            Err(EngineError::Forbidden)
        );
        engine.delete_project(&Actor::Local, "web").unwrap();
        assert_eq!(engine.projects(&Actor::Local).unwrap(), Vec::new());
        assert_eq!(
            engine.delete_project(&Actor::Local, "web"),
            Err(EngineError::NotFound("project"))
        );
        // The key can be used again, from a clean history.
        let (_, v1) = engine
            .create_project(&Actor::Local, "web", "", None)
            .unwrap();
        assert_eq!(v1.number, 1);
        assert_eq!(engine.versions(&Actor::Local, "web").unwrap().len(), 1);
    }
}
