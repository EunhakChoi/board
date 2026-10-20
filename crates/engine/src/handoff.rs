//! Where the project meets the product repo (story 07): handing merged versions over and bringing
//! the repo's own changes back in. Both are admin work from the CLI.
//!
//! The **baseline** is the newest version known to match a product repo commit: an import, or a
//! version an admin marked as handed off. Handoff exports what `main` changed since the baseline;
//! an update import lays the repo's files on the baseline and joins that into `main`, so merged
//! work not handed off yet stays.

use charet_model::{Actor, Version, VersionKind};

use crate::policy::Command;
use crate::projects::{Import, author, check_files};
use crate::{Change, Engine, EngineError, FileChange, Merged, NewVersion};

/// The project's own settings file; it travels with imports but is not part of a handoff.
const SETTINGS: &str = "charet.json";

/// Whether `path` belongs to charet rather than the product (screens, mocks, frame).
pub fn charet_only(path: &str) -> bool {
    path == SETTINGS || path.split('/').any(|part| part == ".charet")
}

/// The newest version that matches a product repo commit.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Baseline {
    pub version: u32,
    /// Its commit in the project store.
    pub commit: String,
    /// The product repo commit it matches, when known (imports from outside git have none).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_commit: Option<String>,
    /// `import` or `handoff`.
    pub kind: &'static str,
}

/// Where the project stands against the product repo.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffStatus {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline: Option<Baseline>,
    /// Versions whose changes the repo does not have yet, oldest first: those after the last one
    /// wholly in the repo, imports left out. An import that kept merged work leaves it here.
    pub pending: Vec<u32>,
    /// Product files `main` changed since the baseline: what a handoff would carry.
    pub changed: Vec<Change>,
}

/// What `main` changed since a version, ready to apply to the product repo.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Export {
    pub from_version: u32,
    pub to_version: u32,
    pub from: String,
    pub to: String,
    /// The product repo commit the patch applies to, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_commit: Option<String>,
    pub changes: Vec<Change>,
    /// `git apply` input; empty when nothing changed.
    pub patch: String,
}

impl Engine {
    pub fn handoff_status(&self, actor: &Actor, key: &str) -> Result<HandoffStatus, EngineError> {
        self.visible(actor, key, Command::ProjectView)?;
        let versions = self.ports.records.versions(key)?;
        let baseline = baseline(&versions);
        let changed: Vec<Change> = match (&baseline, versions.first()) {
            (Some(base), Some(latest)) => self
                .ports
                .store
                .diff(key, &base.commit, &latest.commit)?
                .into_iter()
                .filter(|c| !charet_only(&c.path))
                .collect(),
            _ => Vec::new(),
        };
        // Everything up to the newest version wholly in the repo (a handoff, or an import that
        // joined no other work) is there; later merges are not, even when an import came after.
        let covered = versions
            .iter()
            .find(|v| {
                v.handoff.is_some() || (v.kind == VersionKind::Import && v.source_tree.is_none())
            })
            .map_or(0, |v| v.number);
        let mut pending: Vec<u32> = if changed.is_empty() {
            Vec::new()
        } else {
            versions
                .iter()
                .filter(|v| v.number > covered && v.kind != VersionKind::Import)
                .map(|v| v.number)
                .collect()
        };
        pending.sort_unstable();
        Ok(HandoffStatus {
            baseline,
            pending,
            changed,
        })
    }

    /// The patch from `since` (a version; default the baseline) to the newest version. charet's
    /// own files (`.charet/`, `charet.json`) are left out unless `with_mocks`.
    pub fn export(
        &self,
        actor: &Actor,
        key: &str,
        since: Option<u32>,
        with_mocks: bool,
    ) -> Result<Export, EngineError> {
        self.visible(actor, key, Command::Handoff)?;
        let versions = self.ports.records.versions(key)?;
        let latest = versions.first().ok_or(EngineError::NotFound("version"))?;
        let start = if let Some(number) = since {
            versions
                .iter()
                .find(|v| v.number == number)
                .ok_or(EngineError::NotFound("version"))?
                .clone()
        } else {
            let base = baseline(&versions).ok_or_else(|| {
                EngineError::Invalid(
                    "nothing to start from: no import or handoff yet; pass a version".into(),
                )
            })?;
            let mut version = versions
                .iter()
                .find(|v| v.number == base.version)
                .ok_or(EngineError::NotFound("version"))?
                .clone();
            // An import joined with other work: start from the repo's own files.
            version.commit = base.commit;
            version
        };
        let changes: Vec<Change> = self
            .ports
            .store
            .diff(key, &start.commit, &latest.commit)?
            .into_iter()
            .filter(|c| with_mocks || !charet_only(&c.path))
            .collect();
        let patch = if changes.is_empty() {
            Vec::new()
        } else {
            let paths: Vec<String> = changes.iter().map(|c| c.path.clone()).collect();
            self.ports
                .store
                .patch(key, &start.commit, &latest.commit, &paths)?
        };
        Ok(Export {
            from_version: start.number,
            to_version: latest.number,
            from: start.commit.clone(),
            to: latest.commit.clone(),
            repo_commit: repo_commit_of(&start),
            changes,
            patch: String::from_utf8_lossy(&patch).into_owned(),
        })
    }

    /// Marks version `number` (and so every one before it) as in the product repo at `commit`.
    pub fn mark_handoff(
        &self,
        actor: &Actor,
        key: &str,
        number: u32,
        commit: &str,
    ) -> Result<Version, EngineError> {
        self.visible(actor, key, Command::Handoff)?;
        let commit = commit.trim();
        if !(7..=64).contains(&commit.len()) || !commit.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(EngineError::Invalid(format!(
                "{commit:?} is not a commit id (7 to 64 hex digits)"
            )));
        }
        if !self
            .ports
            .records
            .mark_handoff(key, number, commit, actor.name())?
        {
            return Err(EngineError::NotFound("version"));
        }
        let version = self
            .ports
            .records
            .versions(key)?
            .into_iter()
            .find(|v| v.number == number)
            .ok_or(EngineError::NotFound("version"))?;
        self.publish(
            key,
            "version.handoff",
            serde_json::json!({ "number": number, "commit": commit }),
        );
        Ok(version)
    }

    /// Brings the product repo's files (the whole imported scope) in again as an `import` version.
    /// They are laid on the baseline and joined into `main`; files changed on both sides clash.
    /// charet's own `.charet/` files stay as they are.
    pub fn update_import(
        &self,
        actor: &Actor,
        key: &str,
        import: &Import,
    ) -> Result<Version, EngineError> {
        self.visible(actor, key, Command::Handoff)?;
        check_files(&import.files)?;
        let versions = self.ports.records.versions(key)?;
        let base = baseline(&versions).ok_or_else(|| {
            EngineError::Invalid("the project was not imported from a product repo".into())
        })?;
        let store = &self.ports.store;
        let incoming: Vec<&FileChange> = import
            .files
            .iter()
            .filter(|f| f.path == SETTINGS || !charet_only(&f.path))
            .collect();
        let mut changes: Vec<FileChange> = store
            .list(key, &base.commit)?
            .into_iter()
            .filter(|path| !charet_only(path) && !incoming.iter().any(|f| &f.path == path))
            .map(|path| FileChange {
                path,
                content: None,
                executable: false,
            })
            .collect();
        changes.extend(incoming.into_iter().cloned());
        let note = format!("Imported from {}", import.source);
        let repo = store.commit_detached(key, &base.commit, &changes, &author(actor), &note)?;
        if store.diff(key, &base.commit, &repo)?.is_empty() {
            return Err(EngineError::Invalid(
                "the product repo has nothing new since the last import or handoff".into(),
            ));
        }
        let main = self
            .ports
            .store
            .head(key)?
            .ok_or(EngineError::NotFound("main"))?;
        let joined = if main == base.commit {
            repo.clone()
        } else {
            match store.merge(key, &main, &repo, &author(actor), &note)? {
                Merged::Clean(commit) => commit,
                Merged::Conflict(files) => return Err(EngineError::Clash(files)),
            }
        };
        if !store.move_line(key, "main", &joined, Some(&main))? {
            let head = store.head(key)?.unwrap_or_default();
            return Err(EngineError::Conflict { head });
        }
        let source_tree = (joined != repo).then_some(repo);
        let version = self.record_version(
            key,
            actor,
            &NewVersion {
                commit: joined,
                kind: VersionKind::Import,
                actor: actor.name().to_owned(),
                note,
                branch: None,
                restored: None,
                source_commit: import.commit.clone(),
                source_tree,
            },
        )?;
        self.catch_up_all(actor, key)?;
        Ok(version)
    }
}

/// The newest import or handed-off version.
fn baseline(versions: &[Version]) -> Option<Baseline> {
    versions.iter().find_map(|v| {
        if let Some(handoff) = &v.handoff {
            Some(Baseline {
                version: v.number,
                commit: v.commit.clone(),
                repo_commit: Some(handoff.commit.clone()),
                kind: "handoff",
            })
        } else if v.kind == VersionKind::Import {
            Some(Baseline {
                version: v.number,
                commit: v.source_tree.clone().unwrap_or_else(|| v.commit.clone()),
                repo_commit: v.source_commit.clone(),
                kind: "import",
            })
        } else {
            None
        }
    })
}

fn repo_commit_of(version: &Version) -> Option<String> {
    version
        .handoff
        .as_ref()
        .map(|h| h.commit.clone())
        .or_else(|| version.source_commit.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{engine, file};
    use charet_model::Role;

    fn import(files: Vec<FileChange>, commit: &str) -> Import {
        Import {
            files,
            source: "git@example:shop.git".into(),
            commit: Some(commit.into()),
        }
    }

    fn setup() -> (Engine, Version) {
        let engine = engine();
        let (_, v1) = engine
            .create_project(
                &Actor::Local,
                "shop",
                "",
                Some(import(
                    vec![
                        file("charet.json", r#"{"app":"web"}"#),
                        file("web/src/a.tsx", "a1"),
                        file("web/src/b.tsx", "b1"),
                        file("web/.charet/project.tsx", "scenes"),
                    ],
                    "aaaaaaa",
                )),
            )
            .unwrap();
        (engine, v1)
    }

    fn merged(engine: &Engine, title: &str, changes: &[FileChange]) -> Version {
        let main = engine.ports.store.head("shop").unwrap().unwrap();
        let branch = engine
            .propose(&Actor::Local, "shop", &main, title, changes)
            .unwrap();
        engine
            .merge_branch(&Actor::Local, "shop", branch.number)
            .unwrap()
    }

    fn read(engine: &Engine, path: &str) -> Option<String> {
        let main = engine.ports.store.head("shop").unwrap().unwrap();
        engine
            .ports
            .store
            .read("shop", &main, path)
            .unwrap()
            .map(|b| String::from_utf8(b).unwrap())
    }

    #[test]
    fn charet_files_are_its_own() {
        for path in ["charet.json", ".charet/a.ts", "apps/web/.charet/mocks.ts"] {
            assert!(charet_only(path), "{path}");
        }
        for path in ["apps/web/src/a.tsx", "charet.json.bak", "docs/.charetrc"] {
            assert!(!charet_only(path), "{path}");
        }
    }

    #[test]
    fn export_since_the_baseline_then_mark() {
        let (engine, v1) = setup();
        let status = engine.handoff_status(&Actor::Local, "shop").unwrap();
        assert_eq!(
            status.baseline.as_ref().map(|b| (b.version, b.kind)),
            Some((1, "import"))
        );
        assert_eq!(
            status.baseline.unwrap().repo_commit.as_deref(),
            Some("aaaaaaa")
        );
        assert_eq!(status.pending, Vec::<u32>::new());

        merged(
            &engine,
            "Rows",
            &[
                file("web/src/a.tsx", "a2"),
                file("web/.charet/project.tsx", "scenes 2"),
            ],
        );
        let v3 = merged(&engine, "New page", &[file("web/src/c.tsx", "c1")]);
        assert_eq!(
            engine
                .handoff_status(&Actor::Local, "shop")
                .unwrap()
                .pending,
            [2, 3]
        );

        let export = engine.export(&Actor::Local, "shop", None, false).unwrap();
        assert_eq!((export.from_version, export.to_version), (1, 3));
        assert_eq!(export.repo_commit.as_deref(), Some("aaaaaaa"));
        let paths: Vec<&str> = export.changes.iter().map(|c| c.path.as_str()).collect();
        assert_eq!(paths, ["web/src/a.tsx", "web/src/c.tsx"]);
        assert_eq!(
            export.patch,
            "Modified web/src/a.tsx\nAdded web/src/c.tsx\n"
        );
        let mocks = engine.export(&Actor::Local, "shop", Some(1), true).unwrap();
        assert_eq!(mocks.changes.len(), 3);
        assert_eq!(export.from, v1.commit);

        // Only admins hand off; a mark needs a real commit id.
        let editor = Actor::User {
            id: "ann".into(),
            name: "Ann".into(),
            role: Role::Editor,
        };
        assert_eq!(
            engine.export(&editor, "shop", None, false),
            Err(EngineError::Forbidden)
        );
        assert!(matches!(
            engine.mark_handoff(&Actor::Local, "shop", 3, "main"),
            Err(EngineError::Invalid(_))
        ));
        let marked = engine
            .mark_handoff(&Actor::Local, "shop", v3.number, "bbbbbbb")
            .unwrap();
        assert_eq!(marked.handoff.unwrap().commit, "bbbbbbb");
        let status = engine.handoff_status(&Actor::Local, "shop").unwrap();
        assert_eq!(
            status.baseline.map(|b| (b.version, b.kind)),
            Some((3, "handoff"))
        );
        assert_eq!(status.pending, Vec::<u32>::new());
        assert_eq!(
            engine
                .export(&Actor::Local, "shop", None, false)
                .unwrap()
                .patch,
            ""
        );
    }

    #[test]
    fn update_import_keeps_merged_work_not_handed_off() {
        let (engine, _) = setup();
        // Merged here, not in the repo yet.
        let rows = merged(&engine, "Rows", &[file("web/src/a.tsx", "a2")]);
        // Meanwhile the repo changed b, removed nothing, added d; its copy of .charet is ignored.
        let update = import(
            vec![
                file("charet.json", r#"{"app":"web","source":"new"}"#),
                file("web/src/a.tsx", "a1"),
                file("web/src/b.tsx", "b2"),
                file("web/src/d.tsx", "d1"),
                file("web/.charet/project.tsx", "repo copy"),
            ],
            "ccccccc",
        );
        let version = engine
            .update_import(&Actor::Local, "shop", &update)
            .unwrap();
        assert_eq!(
            (version.kind, version.source_commit.as_deref()),
            (VersionKind::Import, Some("ccccccc"))
        );
        assert_eq!(read(&engine, "web/src/a.tsx").as_deref(), Some("a2"));
        assert_eq!(read(&engine, "web/src/b.tsx").as_deref(), Some("b2"));
        assert_eq!(read(&engine, "web/src/d.tsx").as_deref(), Some("d1"));
        assert_eq!(
            read(&engine, "web/.charet/project.tsx").as_deref(),
            Some("scenes")
        );
        assert_eq!(
            read(&engine, "charet.json").as_deref(),
            Some(r#"{"app":"web","source":"new"}"#)
        );
        // The import is the new baseline, but the merged change to a is still to hand off.
        let status = engine.handoff_status(&Actor::Local, "shop").unwrap();
        assert_eq!(status.baseline.map(|b| b.version), Some(version.number));
        assert_eq!(status.pending, [rows.number]);
        let still: Vec<&str> = status.changed.iter().map(|c| c.path.as_str()).collect();
        assert_eq!(still, ["web/src/a.tsx"]);
        let export = engine.export(&Actor::Local, "shop", None, false).unwrap();
        assert_eq!(export.patch, "Modified web/src/a.tsx\n");
        assert_eq!(export.repo_commit.as_deref(), Some("ccccccc"));

        // Nothing new: refused. A file both sides changed: clash.
        assert!(matches!(
            engine.update_import(&Actor::Local, "shop", &update),
            Err(EngineError::Invalid(_))
        ));
        merged(&engine, "More", &[file("web/src/b.tsx", "b3")]);
        let clash = import(
            vec![
                file("web/src/a.tsx", "a2"),
                file("web/src/b.tsx", "b4"),
                file("web/src/d.tsx", "d1"),
            ],
            "ddddddd",
        );
        assert_eq!(
            engine.update_import(&Actor::Local, "shop", &clash),
            Err(EngineError::Clash(vec!["web/src/b.tsx".into()]))
        );
    }
}
