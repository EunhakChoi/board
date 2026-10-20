//! Guards on merging (`docs/design/security.md` §4, story 06).
//!
//! - Protected files (package manifests, lockfiles, build and compiler settings) run code on the
//!   server when installed or built. Changing them on a branch needs an admin's approval before it
//!   merges, and until then the branch's previews build with those files as `main` had them.
//! - Before `main` moves, the merged result is built and the screens the change can reach are
//!   opened. New problems hold the merge back.

use charet_model::{Actor, Branch, BranchStatus, MergeCheck, MergeCheckState};

use crate::policy::Command;
use crate::projects::author;
use crate::{Author, BuildState, Engine, EngineError, FileChange, Merged};

/// File names that are protected wherever they sit.
const NAMES: &[&str] = &[
    "package.json",
    "package-lock.json",
    "pnpm-lock.yaml",
    "pnpm-workspace.yaml",
    "yarn.lock",
    ".yarnrc.yml",
    "bun.lock",
    "bun.lockb",
    ".npmrc",
    "charet.json",
];

/// Name starts of protected config files (`vite.config.ts`, `tsconfig.app.json`, ...).
const STARTS: &[&str] = &[
    "vite.config.",
    "vitest.config.",
    "postcss.config.",
    "tailwind.config.",
    "tsconfig",
];

/// Whether changing `path` needs an admin's approval.
pub fn is_protected(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    NAMES.contains(&name)
        || STARTS.iter().any(|start| {
            name.starts_with(start)
                && (*start != "tsconfig"
                    || std::path::Path::new(name)
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("json")))
        })
}

impl Engine {
    /// Approves the branch's protected-file changes as they are at its head (admins). A later push
    /// that changes them again needs a new approval.
    pub fn approve_branch(
        &self,
        actor: &Actor,
        key: &str,
        number: u32,
    ) -> Result<Branch, EngineError> {
        self.visible(actor, key, Command::Admin)?;
        let mut branch = self.find_branch(key, number)?;
        if !branch.status.active() {
            return Err(EngineError::Invalid(format!("branch {number} is closed")));
        }
        self.approve(actor, key, &mut branch)?;
        Ok(branch)
    }

    pub(crate) fn approve(
        &self,
        actor: &Actor,
        key: &str,
        branch: &mut Branch,
    ) -> Result<(), EngineError> {
        branch.approved = Some(branch.head.clone());
        let head = branch.head.get(..7).unwrap_or(&branch.head).to_owned();
        self.save_branch(actor, key, branch, "approve", &head)
    }

    /// Protected files changed from `since` to the branch head that no approval covers.
    pub(crate) fn unapproved(
        &self,
        key: &str,
        branch: &Branch,
        since: &str,
    ) -> Result<Vec<String>, EngineError> {
        let changed = self.protected_changes(key, since, &branch.head)?;
        if changed.is_empty() || self.approved_as(key, branch, &branch.head, &changed)? {
            return Ok(Vec::new());
        }
        Ok(changed)
    }

    /// Checks what merging the branch into `main` (at `main`) gives and returns the joined commit
    /// once it passes. The last check is kept on the branch: the same `main` and head reuse it, so
    /// asking again while the build runs picks up where it was.
    pub(crate) fn check_merge(
        &self,
        actor: &Actor,
        key: &str,
        branch: &mut Branch,
        main: &str,
    ) -> Result<String, EngineError> {
        let last = branch
            .check
            .clone()
            .filter(|c| c.main == main && c.head == branch.head)
            .filter(|c| matches!(self.ports.store.resolve(key, &c.commit), Ok(Some(_))));
        let commit = match last {
            Some(check) if check.state == MergeCheckState::Passed => return Ok(check.commit),
            Some(check) => check.commit,
            None => {
                let title = format!("{} (#{})", branch.title, branch.number);
                match self
                    .ports
                    .store
                    .merge(key, main, &branch.head, &author(actor), &title)?
                {
                    Merged::Clean(commit) if commit == main => {
                        return Err(EngineError::Invalid(format!(
                            "branch {} changes nothing",
                            branch.number
                        )));
                    }
                    Merged::Clean(commit) => commit,
                    Merged::Conflict(files) => {
                        if branch.conflicts != files {
                            branch.conflicts.clone_from(&files);
                            self.save_branch(actor, key, branch, "conflict", &files.join(", "))?;
                            self.tell(actor, key, branch, "branch.clash");
                        }
                        return Err(EngineError::Clash(files));
                    }
                }
            }
        };
        // Without previews nothing here can build, so there is nothing to check.
        if self.preview.is_none() || !self.merge_check {
            return Ok(commit);
        }
        // Its protected files are main's or approved (the caller made sure), so it builds as is.
        let preview = self.preview_of(key, commit.clone(), false)?;
        let (state, issues) = match preview.build.state {
            BuildState::Queued | BuildState::Running => (MergeCheckState::Checking, Vec::new()),
            BuildState::Failed => (
                MergeCheckState::Failed,
                vec![format!("build: {}", preview.build.error.trim())],
            ),
            BuildState::Ready => {
                let issues = self.check_reach(key, main, &preview.commit)?;
                let state = if issues.is_empty() {
                    MergeCheckState::Passed
                } else {
                    MergeCheckState::Failed
                };
                (state, issues)
            }
        };
        let check = MergeCheck {
            main: main.to_owned(),
            head: branch.head.clone(),
            commit: commit.clone(),
            state,
            issues: issues.clone(),
        };
        if branch.check.as_ref() != Some(&check) {
            branch.check = Some(check);
            self.ports.records.update_branch(key, branch)?;
            self.publish(
                key,
                "branch.updated",
                serde_json::json!({ "number": branch.number, "event": "check" }),
            );
        }
        match state {
            MergeCheckState::Passed => Ok(commit),
            MergeCheckState::Checking => Err(EngineError::Pending(
                "checking what merging gives (building it); try again shortly".into(),
            )),
            MergeCheckState::Failed => Err(EngineError::CheckFailed(issues)),
        }
    }

    /// The commit to build for `commit`: itself, unless it carries protected-file changes against
    /// `main` that no admin approved. Then a commit with those files as they were where it left
    /// `main`, made once and remembered.
    pub(crate) fn build_commit(&self, key: &str, commit: &str) -> Result<String, EngineError> {
        let store = &self.ports.store;
        let Some(main) = store.head(key)? else {
            return Ok(commit.to_owned());
        };
        let Some(base) = store.merge_base(key, &main, commit)? else {
            return Ok(commit.to_owned());
        };
        if base == commit {
            // Part of main's history.
            return Ok(commit.to_owned());
        }
        let changed = self.protected_changes(key, &base, commit)?;
        if changed.is_empty() {
            return Ok(commit.to_owned());
        }
        for branch in self.ports.records.branches(key)? {
            if branch.head == commit && self.approved_as(key, &branch, commit, &changed)? {
                return Ok(commit.to_owned());
            }
        }
        let memo = format!("safe-build:{key}:{commit}");
        if let Some(safe) = self.ports.settings.get(&memo)?
            && store.resolve(key, &safe)?.is_some()
        {
            return Ok(safe);
        }
        let restore = changed
            .iter()
            .map(|path| {
                Ok(FileChange {
                    path: path.clone(),
                    content: store.read(key, &base, path)?,
                    executable: false,
                })
            })
            .collect::<Result<Vec<_>, EngineError>>()?;
        let safe = store.commit_detached(
            key,
            commit,
            &restore,
            &Author {
                name: "charet".into(),
                email: "charet@charet.invalid".into(),
            },
            "Protected files as main had them, until an admin approves",
        )?;
        self.ports.settings.set(&memo, &safe)?;
        Ok(safe)
    }

    fn protected_changes(
        &self,
        key: &str,
        from: &str,
        to: &str,
    ) -> Result<Vec<String>, EngineError> {
        Ok(self
            .ports
            .store
            .diff(key, from, to)?
            .into_iter()
            .map(|c| c.path)
            .filter(|p| is_protected(p))
            .collect())
    }

    /// Whether `commit` has the `changed` protected files exactly as the branch's approval saw them.
    fn approved_as(
        &self,
        key: &str,
        branch: &Branch,
        commit: &str,
        changed: &[String],
    ) -> Result<bool, EngineError> {
        if branch.status == BranchStatus::Archived {
            return Ok(false);
        }
        let Some(approved) = &branch.approved else {
            return Ok(false);
        };
        if approved == commit {
            return Ok(true);
        }
        for path in changed {
            if self.ports.store.read(key, approved, path)?
                != self.ports.store.read(key, commit, path)?
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Import;
    use crate::testing::{Fakes, engine_with_fakes, file};
    use charet_model::{Role, VersionKind};

    const MANIFEST: &str = r#"{"manifestVersion":1,"title":"T","pages":[],
        "screens":[{"id":"list","page":"p","title":"L","note":"","route":"/","size":{"w":800,"h":600},"hasSetup":false},
                   {"id":"empty","page":"p","title":"E","note":"","route":"/","size":{"w":800,"h":600},"hasSetup":false}],
        "connections":[],"flows":[],"stories":[]}"#;

    fn ann() -> Actor {
        Actor::User {
            id: "ann".into(),
            name: "Ann".into(),
            role: Role::Editor,
        }
    }

    /// A project whose `main` is built and captured: `list` renders from `web/src/list.tsx`.
    fn setup() -> (Engine, Fakes, String) {
        let (engine, fakes) = engine_with_fakes();
        let (_, v1) = engine
            .create_project(
                &Actor::Local,
                "shop",
                "Shop",
                Some(Import {
                    files: vec![
                        file("charet.json", r#"{"app":"web"}"#),
                        file("web/package.json", "{}"),
                        file("web/src/list.tsx", "list"),
                        file("web/src/empty.tsx", "empty"),
                    ],
                    source: "test".into(),
                    commit: None,
                }),
            )
            .unwrap();
        engine
            .preview(&Actor::Local, "shop", "main", false)
            .unwrap();
        fakes.workspace.finish(BuildState::Ready);
        fakes.workspace.serve(".charet/manifest.json", MANIFEST);
        engine
            .capture(&Actor::Local, "shop", "main", &[], &[], false)
            .unwrap();
        (engine, fakes, v1.commit)
    }

    fn read(engine: &Engine, commit: &str, path: &str) -> Option<String> {
        engine
            .ports
            .store
            .read("shop", commit, path)
            .unwrap()
            .map(|b| String::from_utf8(b).unwrap())
    }

    #[test]
    fn protected_changes_wait_for_an_admin() {
        let (engine, fakes, base) = setup();
        let branch = engine
            .propose(
                &ann(),
                "shop",
                &base,
                "New dependency",
                &[
                    file("web/package.json", r#"{"dependencies":{"x":"1"}}"#),
                    file("web/src/list.tsx", "list 2"),
                ],
            )
            .unwrap();
        let detail = engine.branch(&ann(), "shop", 1).unwrap();
        assert_eq!(detail.unapproved, ["web/package.json"]);

        // Its previews build with main's package.json until approved.
        let preview = engine.preview(&ann(), "shop", "b1", false).unwrap();
        assert_ne!(preview.commit, branch.head);
        assert_eq!(
            read(&engine, &preview.commit, "web/package.json").as_deref(),
            Some("{}")
        );
        assert_eq!(
            read(&engine, &preview.commit, "web/src/list.tsx").as_deref(),
            Some("list 2")
        );
        let again = engine.preview(&ann(), "shop", "b1", false).unwrap();
        assert_eq!(again.commit, preview.commit, "made once");

        assert_eq!(
            engine.merge_branch(&ann(), "shop", 1),
            Err(EngineError::ApprovalNeeded(vec!["web/package.json".into()]))
        );
        assert_eq!(
            engine.approve_branch(&ann(), "shop", 1),
            Err(EngineError::Forbidden)
        );
        engine.approve_branch(&Actor::Local, "shop", 1).unwrap();
        assert_eq!(
            engine.branch(&ann(), "shop", 1).unwrap().unapproved,
            Vec::<String>::new()
        );
        assert_eq!(
            engine.preview(&ann(), "shop", "b1", false).unwrap().commit,
            branch.head
        );

        // A push that changes it again needs a new approval.
        let pushed = engine
            .push_branch(
                &ann(),
                "shop",
                1,
                &branch.head,
                "More",
                &[file("web/package.json", r#"{"dependencies":{"y":"1"}}"#)],
            )
            .unwrap();
        assert_eq!(
            engine.branch(&ann(), "shop", 1).unwrap().unapproved,
            ["web/package.json"]
        );
        assert_ne!(
            engine.preview(&ann(), "shop", "b1", false).unwrap().commit,
            pushed.head
        );

        // An admin merging approves as they go; the merge waits for its check.
        assert!(matches!(
            engine.merge_branch(&Actor::Local, "shop", 1),
            Err(EngineError::Pending(_))
        ));
        fakes.workspace.finish(BuildState::Ready);
        let version = engine.merge_branch(&Actor::Local, "shop", 1).unwrap();
        assert_eq!(version.kind, VersionKind::Merge);
        let merged = engine.branch(&ann(), "shop", 1).unwrap();
        assert_eq!(
            merged.branch.approved.as_deref(),
            Some(pushed.head.as_str())
        );
        assert_eq!(merged.branch.check.unwrap().state, MergeCheckState::Passed);
    }

    #[test]
    fn the_merge_check_holds_back_new_problems() {
        let (engine, fakes, base) = setup();
        fakes.renderer.set_issues("empty", &["console: old"]);
        engine
            .capture(&Actor::Local, "shop", "main", &["empty".into()], &[], true)
            .unwrap();
        engine
            .propose(
                &ann(),
                "shop",
                &base,
                "Rows",
                &[file("web/src/list.tsx", "list 2")],
            )
            .unwrap();
        assert_eq!(
            engine.branch(&ann(), "shop", 1).unwrap().affected,
            Some(vec!["screen:list".to_owned()])
        );

        fakes
            .renderer
            .set_issues("list", &["console: boom", "console: boom"]);
        assert!(matches!(
            engine.merge_branch(&ann(), "shop", 1),
            Err(EngineError::Pending(_))
        ));
        let checking = engine
            .branch(&ann(), "shop", 1)
            .unwrap()
            .branch
            .check
            .unwrap();
        assert_eq!(checking.state, MergeCheckState::Checking);
        fakes.workspace.finish(BuildState::Ready);
        let jobs = fakes.renderer.jobs().len();
        assert_eq!(
            engine.merge_branch(&ann(), "shop", 1),
            Err(EngineError::CheckFailed(vec![
                "screen list: console: boom".into()
            ]))
        );
        assert_eq!(
            fakes.renderer.jobs().len(),
            jobs + 1,
            "only the screen it reaches"
        );
        let failed = engine
            .branch(&ann(), "shop", 1)
            .unwrap()
            .branch
            .check
            .unwrap();
        assert_eq!(failed.state, MergeCheckState::Failed);

        // Fixed (or flaky): asking again checks again; a problem main already had is no news.
        fakes.renderer.set_issues("list", &[]);
        let version = engine.merge_branch(&ann(), "shop", 1).unwrap();
        assert_eq!(version.commit, failed.commit);
        let main = engine.ports.store.head("shop").unwrap().unwrap();
        assert_eq!(
            read(&engine, &main, "web/src/list.tsx").as_deref(),
            Some("list 2")
        );
    }

    #[test]
    fn protected_files_are_build_and_install_settings() {
        for path in [
            "package.json",
            "apps/web/package.json",
            "pnpm-lock.yaml",
            "apps/web/vite.config.ts",
            "tsconfig.json",
            "apps/web/tsconfig.app.json",
            ".npmrc",
            "charet.json",
        ] {
            assert!(is_protected(path), "{path}");
        }
        for path in [
            "apps/web/src/page.tsx",
            "tsconfig-notes.md",
            "src/package.json.md",
            ".charet/scenes.tsx",
        ] {
            assert!(!is_protected(path), "{path}");
        }
    }
}
