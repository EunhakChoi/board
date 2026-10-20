//! Branches: drafts and changes under review on top of `main`, merged into versions or archived
//! (story 06). A version can be restored: a new version that undoes it.
//!
//! - Open (a draft): only its author (or an admin) pushes; others duplicate it to carry on.
//! - In review: any editor pushes, merges or archives it.
//! - Merging joins it into `main` as a new version; every other active branch then catches up
//!   with `main`, and clashes are only marked, for their authors to settle.

use charet_model::{Actor, Branch, BranchStatus, Role, Version, VersionKind};

use crate::policy::Command;
use crate::projects::{author, check_files};
use crate::{BranchEvent, Change, Engine, EngineError, FileChange, Merged, NewBranch, NewVersion};

/// A branch with what it changes and what happened to it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchDetail {
    #[serde(flatten)]
    pub branch: Branch,
    /// Files changed against where it started from `main`.
    pub changes: Vec<Change>,
    /// The commit those changes are counted from (where it last met `main`).
    pub since: String,
    /// `main` has moved on since the branch last caught up.
    pub behind: bool,
    /// Protected files it changes that an admin has not approved yet.
    pub unapproved: Vec<String>,
    /// Screens and stories (`kind:id`) its changes can reach, as far as `main`'s captures tell;
    /// absent when a change may reach any of them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affected: Option<Vec<String>>,
    pub events: Vec<BranchEvent>,
}

impl Engine {
    /// Branches of a project, newest first.
    pub fn branches(&self, actor: &Actor, key: &str) -> Result<Vec<Branch>, EngineError> {
        self.visible(actor, key, Command::ProjectView)?;
        Ok(self.ports.records.branches(key)?)
    }

    pub fn branch(
        &self,
        actor: &Actor,
        key: &str,
        number: u32,
    ) -> Result<BranchDetail, EngineError> {
        self.visible(actor, key, Command::ProjectView)?;
        let branch = self.find_branch(key, number)?;
        let main = self.main_head(key)?;
        let since = self
            .ports
            .store
            .merge_base(key, &branch.head, &main)?
            .unwrap_or_else(|| branch.base.clone());
        let changes = self.ports.store.diff(key, &since, &branch.head)?;
        let behind = branch.status.active() && since != main;
        let events = self.ports.records.branch_events(key, number)?;
        let unapproved = if branch.status.active() {
            self.unapproved(key, &branch, &since)?
        } else {
            Vec::new()
        };
        let affected = self
            .reach(key, &since, &branch.head)?
            .map(|reached| reached.into_iter().collect());
        Ok(BranchDetail {
            branch,
            changes,
            since,
            behind,
            unapproved,
            affected,
            events,
        })
    }

    /// Starts a branch at `main` (or at `from`, a revision), for the actor.
    pub fn create_branch(
        &self,
        actor: &Actor,
        key: &str,
        title: &str,
        from: Option<&str>,
    ) -> Result<Branch, EngineError> {
        self.visible(actor, key, Command::BranchEdit)?;
        let start = self.resolve(key, from.unwrap_or("main"))?;
        self.start_branch(actor, key, title, &start, "created", "")
    }

    /// Pushes changes made on a checkout of `base` (a `main` commit) as a new branch.
    pub fn propose(
        &self,
        actor: &Actor,
        key: &str,
        base: &str,
        title: &str,
        changes: &[FileChange],
    ) -> Result<Branch, EngineError> {
        self.visible(actor, key, Command::BranchEdit)?;
        check_push(changes)?;
        let base = self.resolve(key, base)?;
        let branch = self.start_branch(actor, key, title, &base, "created", "")?;
        self.push_branch(actor, key, branch.number, &base, title, changes)
    }

    /// Commits changes on top of `base`, which must be the branch's head.
    pub fn push_branch(
        &self,
        actor: &Actor,
        key: &str,
        number: u32,
        base: &str,
        message: &str,
        changes: &[FileChange],
    ) -> Result<Branch, EngineError> {
        self.visible(actor, key, Command::BranchEdit)?;
        check_push(changes)?;
        let mut branch = self.find_branch(key, number)?;
        Self::may_change(actor, &branch)?;
        if branch.head != base {
            return Err(EngineError::Conflict { head: branch.head });
        }
        let message = if message.trim().is_empty() {
            "Push"
        } else {
            message.trim()
        };
        let line = Branch::line(number);
        let commit = self
            .ports
            .store
            .commit_line(key, &line, Some(base), changes, &author(actor), message)?
            .ok_or_else(|| EngineError::Conflict {
                head: self
                    .ports
                    .store
                    .line_head(key, &line)
                    .ok()
                    .flatten()
                    .unwrap_or_default(),
            })?;
        branch.head = commit;
        self.save_branch(actor, key, &branch, "push", message)?;
        self.ports.realtime.working(key, &actor.person(), false);
        Ok(branch)
    }

    /// Asks the team to review an open branch (its author or an admin).
    pub fn request_review(
        &self,
        actor: &Actor,
        key: &str,
        number: u32,
    ) -> Result<Branch, EngineError> {
        self.visible(actor, key, Command::BranchEdit)?;
        let mut branch = self.find_branch(key, number)?;
        if branch.status != BranchStatus::Open {
            return Err(EngineError::Invalid(format!(
                "branch {number} is not a draft"
            )));
        }
        if !owns(actor, &branch) {
            return Err(EngineError::Forbidden);
        }
        branch.status = BranchStatus::InReview;
        self.save_branch(actor, key, &branch, "review", "")?;
        self.tell(actor, key, &branch, "branch.review");
        Ok(branch)
    }

    /// Joins a branch into `main` as a new version. Clashing files stop the merge (and are marked
    /// on the branch); afterwards every other active branch catches up with `main`.
    pub fn merge_branch(
        &self,
        actor: &Actor,
        key: &str,
        number: u32,
    ) -> Result<Version, EngineError> {
        self.visible(actor, key, Command::BranchMerge)?;
        let mut branch = self.find_branch(key, number)?;
        if !branch.status.active() {
            return Err(EngineError::Invalid(format!("branch {number} is closed")));
        }
        let main = self.main_head(key)?;
        let since = self
            .ports
            .store
            .merge_base(key, &branch.head, &main)?
            .unwrap_or_else(|| branch.base.clone());
        let unapproved = self.unapproved(key, &branch, &since)?;
        if !unapproved.is_empty() {
            // An admin merging approves as they go.
            if actor.role() != Role::Admin {
                return Err(EngineError::ApprovalNeeded(unapproved));
            }
            self.approve(actor, key, &mut branch)?;
        }
        let joined = self.check_merge(actor, key, &mut branch, &main)?;
        if !self
            .ports
            .store
            .move_line(key, "main", &joined, Some(&main))?
        {
            let head = self.main_head(key)?;
            return Err(EngineError::Conflict { head });
        }
        let version = self.record_version(
            key,
            actor,
            &NewVersion {
                commit: joined.clone(),
                kind: VersionKind::Merge,
                actor: actor.name().to_owned(),
                note: branch.title.clone(),
                branch: Some(number),
                restored: None,
            },
        )?;
        branch.status = BranchStatus::Merged;
        branch.base = joined;
        branch.conflicts.clear();
        branch.merged_version = Some(version.number);
        self.save_branch(
            actor,
            key,
            &branch,
            "merge",
            &format!("v{}", version.number),
        )?;
        self.tell(actor, key, &branch, "branch.merged");
        self.catch_up_all(actor, key)?;
        Ok(version)
    }

    /// Closes a branch without merging it: its author or an admin, or any editor once it is in
    /// review (turning it down). Nothing is deleted.
    pub fn archive_branch(
        &self,
        actor: &Actor,
        key: &str,
        number: u32,
    ) -> Result<Branch, EngineError> {
        self.visible(actor, key, Command::BranchEdit)?;
        let mut branch = self.find_branch(key, number)?;
        if !branch.status.active() {
            return Err(EngineError::Invalid(format!("branch {number} is closed")));
        }
        if branch.status == BranchStatus::Open && !owns(actor, &branch) {
            return Err(EngineError::Forbidden);
        }
        branch.status = BranchStatus::Archived;
        self.save_branch(actor, key, &branch, "archive", "")?;
        Ok(branch)
    }

    /// Brings an archived branch back as a draft (its author or an admin).
    pub fn reopen_branch(
        &self,
        actor: &Actor,
        key: &str,
        number: u32,
    ) -> Result<Branch, EngineError> {
        self.visible(actor, key, Command::BranchEdit)?;
        let mut branch = self.find_branch(key, number)?;
        if branch.status != BranchStatus::Archived {
            return Err(EngineError::Invalid(format!(
                "branch {number} is not archived"
            )));
        }
        if !owns(actor, &branch) {
            return Err(EngineError::Forbidden);
        }
        branch.status = BranchStatus::Open;
        self.save_branch(actor, key, &branch, "reopen", "")?;
        Ok(branch)
    }

    /// A new draft of the actor's own that starts where branch `number` is now.
    pub fn duplicate_branch(
        &self,
        actor: &Actor,
        key: &str,
        number: u32,
    ) -> Result<Branch, EngineError> {
        self.visible(actor, key, Command::BranchEdit)?;
        let source = self.find_branch(key, number)?;
        self.start_branch(
            actor,
            key,
            &source.title,
            &source.head,
            "duplicate",
            &format!("#{number}"),
        )
    }

    /// Joins the newest `main` into the branch. Clashes are marked, not forced.
    pub fn catch_up(&self, actor: &Actor, key: &str, number: u32) -> Result<Branch, EngineError> {
        self.visible(actor, key, Command::BranchEdit)?;
        let branch = self.find_branch(key, number)?;
        Self::may_change(actor, &branch)?;
        self.catch_up_one(actor, key, branch)
    }

    /// A new version that undoes what version `number` changed.
    pub fn restore_version(
        &self,
        actor: &Actor,
        key: &str,
        number: u32,
    ) -> Result<Version, EngineError> {
        self.visible(actor, key, Command::VersionRestore)?;
        let target = self
            .ports
            .records
            .versions(key)?
            .into_iter()
            .find(|v| v.number == number)
            .ok_or(EngineError::NotFound("version"))?;
        if matches!(target.kind, VersionKind::Initial | VersionKind::Import) {
            return Err(EngineError::Invalid(format!(
                "v{number} is where the project started"
            )));
        }
        let main = self.main_head(key)?;
        let note = format!("Restore v{number}");
        let commit =
            match self
                .ports
                .store
                .revert(key, &main, &target.commit, &author(actor), &note)?
            {
                Merged::Clean(commit) => commit,
                Merged::Conflict(files) => return Err(EngineError::Clash(files)),
            };
        if self.ports.store.diff(key, &main, &commit)?.is_empty() {
            return Err(EngineError::Invalid(format!("v{number} is already undone")));
        }
        if !self
            .ports
            .store
            .move_line(key, "main", &commit, Some(&main))?
        {
            return Err(EngineError::Conflict {
                head: self.main_head(key)?,
            });
        }
        let version = self.record_version(
            key,
            actor,
            &NewVersion {
                commit,
                kind: VersionKind::Restore,
                actor: actor.name().to_owned(),
                note,
                branch: None,
                restored: Some(number),
            },
        )?;
        self.catch_up_all(actor, key)?;
        Ok(version)
    }

    fn start_branch(
        &self,
        actor: &Actor,
        key: &str,
        title: &str,
        start: &str,
        event: &str,
        detail: &str,
    ) -> Result<Branch, EngineError> {
        let title = title.trim();
        let title = if title.is_empty() {
            "Untitled change"
        } else {
            title
        };
        let base = self
            .ports
            .store
            .merge_base(key, start, &self.main_head(key)?)?
            .unwrap_or_else(|| start.to_owned());
        let branch = self.ports.records.insert_branch(
            key,
            &NewBranch {
                title: title.to_owned(),
                author: actor.person(),
                base,
                head: start.to_owned(),
            },
        )?;
        if !self
            .ports
            .store
            .move_line(key, &Branch::line(branch.number), start, None)?
        {
            return Err(EngineError::Exists("branch"));
        }
        self.log(actor, key, branch.number, event, detail)?;
        self.publish(
            key,
            "branch.updated",
            serde_json::json!({ "number": branch.number }),
        );
        Ok(branch)
    }

    fn catch_up_all(&self, actor: &Actor, key: &str) -> Result<(), EngineError> {
        for branch in self.ports.records.branches(key)? {
            if branch.status.active() {
                self.catch_up_one(actor, key, branch)?;
            }
        }
        Ok(())
    }

    fn catch_up_one(
        &self,
        actor: &Actor,
        key: &str,
        mut branch: Branch,
    ) -> Result<Branch, EngineError> {
        let main = self.main_head(key)?;
        let line = Branch::line(branch.number);
        let message = format!("Catch up with main ({})", short(&main));
        match self
            .ports
            .store
            .merge(key, &branch.head, &main, &author(actor), &message)?
        {
            Merged::Clean(commit) => {
                if commit != branch.head
                    && !self
                        .ports
                        .store
                        .move_line(key, &line, &commit, Some(&branch.head))?
                {
                    // Someone pushed meanwhile; the next catch-up will take it.
                    return Ok(branch);
                }
                let changed =
                    commit != branch.head || !branch.conflicts.is_empty() || branch.base != main;
                branch.head = commit;
                branch.base = main;
                branch.conflicts.clear();
                if changed {
                    self.save_branch(actor, key, &branch, "catch_up", "")?;
                }
            }
            Merged::Conflict(files) => {
                if branch.conflicts != files {
                    branch.conflicts.clone_from(&files);
                    self.save_branch(actor, key, &branch, "conflict", &files.join(", "))?;
                    self.tell(actor, key, &branch, "branch.clash");
                }
            }
        }
        Ok(branch)
    }

    pub(crate) fn save_branch(
        &self,
        actor: &Actor,
        key: &str,
        branch: &Branch,
        event: &str,
        detail: &str,
    ) -> Result<(), EngineError> {
        self.ports.records.update_branch(key, branch)?;
        self.log(actor, key, branch.number, event, detail)?;
        self.publish(
            key,
            "branch.updated",
            serde_json::json!({ "number": branch.number, "event": event }),
        );
        Ok(())
    }

    fn log(
        &self,
        actor: &Actor,
        key: &str,
        number: u32,
        kind: &str,
        detail: &str,
    ) -> Result<(), EngineError> {
        Ok(self.ports.records.add_branch_event(
            key,
            number,
            &BranchEvent {
                actor: actor.person(),
                kind: kind.to_owned(),
                detail: detail.to_owned(),
                at: String::new(),
            },
        )?)
    }

    /// Notifications and webhooks for a branch event (best effort).
    pub(crate) fn tell(&self, actor: &Actor, key: &str, branch: &Branch, kind: &'static str) {
        if let Ok(Some(project)) = self.ports.records.project(key) {
            self.branch_event(actor, &project, branch, kind);
        }
    }

    pub(crate) fn find_branch(&self, key: &str, number: u32) -> Result<Branch, EngineError> {
        self.ports
            .records
            .branch(key, number)?
            .ok_or(EngineError::NotFound("branch"))
    }

    fn main_head(&self, key: &str) -> Result<String, EngineError> {
        self.ports
            .store
            .head(key)?
            .ok_or(EngineError::NotFound("main"))
    }

    /// Drafts belong to their author; branches in review to every editor.
    fn may_change(actor: &Actor, branch: &Branch) -> Result<(), EngineError> {
        match branch.status {
            BranchStatus::Open if owns(actor, branch) => Ok(()),
            BranchStatus::Open => Err(EngineError::Forbidden),
            BranchStatus::InReview => Ok(()),
            BranchStatus::Merged | BranchStatus::Archived => Err(EngineError::Invalid(format!(
                "branch {} is closed",
                branch.number
            ))),
        }
    }
}

/// The author, or an admin.
fn owns(actor: &Actor, branch: &Branch) -> bool {
    actor.role() == Role::Admin || actor.person().id == branch.author.id
}

fn check_push(changes: &[FileChange]) -> Result<(), EngineError> {
    if changes.is_empty() {
        return Err(EngineError::Invalid("nothing to push".into()));
    }
    check_files(changes)
}

fn short(commit: &str) -> &str {
    commit.get(..7).unwrap_or(commit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Import;
    use crate::testing::{engine, file};

    fn person(id: &str, role: Role) -> Actor {
        Actor::User {
            id: id.into(),
            name: id.to_uppercase(),
            role,
        }
    }

    fn project() -> (Engine, String) {
        let engine = engine();
        let (_, v1) = engine
            .create_project(
                &Actor::Local,
                "web",
                "",
                Some(Import {
                    files: vec![file("a.txt", "1"), file("b.txt", "1")],
                    source: "local".into(),
                }),
            )
            .unwrap();
        (engine, v1.commit)
    }

    #[test]
    fn propose_review_merge_and_restore() {
        let (engine, base) = project();
        let ann = person("ann", Role::Editor);
        let bob = person("bob", Role::Editor);
        let b1 = engine
            .propose(&ann, "web", &base, "Bigger a", &[file("a.txt", "2")])
            .unwrap();
        assert_eq!((b1.number, b1.status), (1, BranchStatus::Open));
        assert_eq!(b1.title, "Bigger a");
        assert_eq!(
            engine.checkout(&ann, "web", Some("main")).unwrap().commit,
            base,
            "main untouched"
        );

        // A draft is its author's: others duplicate it instead.
        assert_eq!(
            engine.push_branch(&bob, "web", 1, &b1.head, "", &[file("a.txt", "3")]),
            Err(EngineError::Forbidden)
        );
        let copy = engine.duplicate_branch(&bob, "web", 1).unwrap();
        assert_eq!((copy.number, copy.head.as_str()), (2, b1.head.as_str()));
        assert_eq!(copy.author.id, "bob");

        let b1 = engine.request_review(&ann, "web", 1).unwrap();
        assert_eq!(b1.status, BranchStatus::InReview);
        // In review, any editor may add to it.
        engine
            .push_branch(&bob, "web", 1, &b1.head, "tweak", &[file("c.txt", "new")])
            .unwrap();
        let detail = engine.branch(&ann, "web", 1).unwrap();
        assert_eq!(
            detail
                .changes
                .iter()
                .map(|c| c.path.as_str())
                .collect::<Vec<_>>(),
            ["a.txt", "c.txt"]
        );
        assert!(!detail.behind);
        assert_eq!(
            detail
                .events
                .iter()
                .map(|e| e.kind.as_str())
                .collect::<Vec<_>>(),
            ["created", "push", "review", "push"]
        );

        let viewer = person("vic", Role::Commenter);
        assert_eq!(
            engine.merge_branch(&viewer, "web", 1),
            Err(EngineError::Forbidden)
        );
        let v2 = engine.merge_branch(&bob, "web", 1).unwrap();
        assert_eq!(
            (v2.number, v2.kind, v2.branch),
            (2, VersionKind::Merge, Some(1))
        );
        let merged = engine.branch(&ann, "web", 1).unwrap().branch;
        assert_eq!(
            (merged.status, merged.merged_version),
            (BranchStatus::Merged, Some(2))
        );
        assert!(matches!(
            engine.merge_branch(&bob, "web", 1),
            Err(EngineError::Invalid(_))
        ));

        // The duplicate caught up with main: it already holds a.txt = 2, so nothing clashes.
        let copy = engine.branch(&bob, "web", 2).unwrap();
        assert!(!copy.behind && copy.branch.conflicts.is_empty(), "{copy:?}");

        let v3 = engine.restore_version(&ann, "web", 2).unwrap();
        assert_eq!((v3.kind, v3.restored), (VersionKind::Restore, Some(2)));
        let changes = engine.diff(&ann, "web", "v1", "main").unwrap();
        assert!(changes.is_empty(), "back to v1: {changes:?}");
        assert!(matches!(
            engine.restore_version(&ann, "web", 2),
            Err(EngineError::Invalid(_))
        ));
        assert!(matches!(
            engine.restore_version(&ann, "web", 1),
            Err(EngineError::Invalid(_))
        ));
    }

    #[test]
    fn clashes_are_marked_and_stop_the_merge() {
        let (engine, base) = project();
        let ann = person("ann", Role::Editor);
        let one = engine
            .propose(&ann, "web", &base, "one", &[file("b.txt", "one")])
            .unwrap();
        let two = engine
            .propose(&ann, "web", &base, "two", &[file("b.txt", "two")])
            .unwrap();
        engine.merge_branch(&ann, "web", one.number).unwrap();
        let two = engine.branch(&ann, "web", two.number).unwrap();
        assert_eq!(two.branch.conflicts, ["b.txt"]);
        assert!(two.behind);
        assert_eq!(
            engine.merge_branch(&ann, "web", 2),
            Err(EngineError::Clash(vec!["b.txt".into()]))
        );
        // Settled by hand on top of its head: the clash goes once main is joined in.
        let settled = engine
            .push_branch(
                &ann,
                "web",
                2,
                &two.branch.head,
                "",
                &[file("b.txt", "one")],
            )
            .unwrap();
        let caught = engine.catch_up(&ann, "web", settled.number).unwrap();
        assert_eq!(caught.conflicts, Vec::<String>::new());
        assert_eq!(
            engine.push_branch(&ann, "web", 2, &settled.head, "", &[file("x", "1")]),
            Err(EngineError::Conflict {
                head: caught.head.clone()
            }),
            "stale base"
        );
    }

    #[test]
    fn archive_and_reopen() {
        let (engine, base) = project();
        let ann = person("ann", Role::Editor);
        let bob = person("bob", Role::Editor);
        engine
            .propose(&ann, "web", &base, "", &[file("a.txt", "9")])
            .unwrap();
        assert_eq!(
            engine.branches(&bob, "web").unwrap()[0].title,
            "Untitled change"
        );
        assert_eq!(
            engine.archive_branch(&bob, "web", 1),
            Err(EngineError::Forbidden)
        );
        engine.request_review(&ann, "web", 1).unwrap();
        let archived = engine.archive_branch(&bob, "web", 1).unwrap();
        assert_eq!(archived.status, BranchStatus::Archived);
        assert_eq!(
            engine.reopen_branch(&bob, "web", 1),
            Err(EngineError::Forbidden)
        );
        assert_eq!(
            engine.reopen_branch(&ann, "web", 1).unwrap().status,
            BranchStatus::Open
        );
        assert_eq!(
            engine.branch(&ann, "web", 9),
            Err(EngineError::NotFound("branch"))
        );
    }
}
