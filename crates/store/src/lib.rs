//! Project store: one git repository per project (main, branches, versions).
//!
//! Drives the `git` command line on bare repositories under `<root>/<project>/repo.git`.
//! Commits are written with `git fast-import` into a scratch ref, then `main` is moved with a
//! compare-and-swap `update-ref`, so a commit lands only if `main` did not move meanwhile.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use charet_engine::{Author, Change, ChangeKind, FileChange, Merged, ProjectStore, StoreError};

const MAIN: &str = "refs/heads/main";
const ZERO: &str = "0000000000000000000000000000000000000000";

/// The git ref of a line: `main` or `b/<number>`.
fn line_ref(line: &str) -> Result<String, StoreError> {
    let ok = line == "main"
        || line.strip_prefix("b/").is_some_and(|n| {
            !n.is_empty() && n.len() <= 9 && n.bytes().all(|b| b.is_ascii_digit())
        });
    if ok {
        Ok(format!("refs/heads/{line}"))
    } else {
        Err(StoreError(format!("bad line {line:?}")))
    }
}

/// Git repositories on the local disk.
#[derive(Debug, Clone)]
pub struct GitStore {
    root: PathBuf,
}

impl GitStore {
    /// Keeps repositories under `root` (created on demand).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn repo(&self, project: &str) -> PathBuf {
        self.root.join(project).join("repo.git")
    }

    fn git(&self, project: &str) -> Git {
        Git {
            dir: self.repo(project),
        }
    }
}

impl ProjectStore for GitStore {
    fn remove(&self, project: &str) -> Result<(), StoreError> {
        if !charet_model::Project::valid_key(project) {
            return Err(StoreError(format!("bad project key {project:?}")));
        }
        let dir = self.root.join(project);
        match std::fs::remove_dir_all(&dir) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(err(e)),
            _ => Ok(()),
        }
    }

    fn create(&self, project: &str) -> Result<(), StoreError> {
        let dir = self.repo(project);
        if dir.exists() {
            return Err(StoreError(format!("{} already exists", dir.display())));
        }
        std::fs::create_dir_all(&dir).map_err(err)?;
        let git = Git { dir };
        // With --git-dir set and no directory argument, init creates the repository there.
        git.run(
            &["init", "--quiet", "--bare", "--initial-branch=main"],
            None,
        )?;
        // Never run hooks from a project store.
        git.run(&["config", "core.hooksPath", "/dev/null"], None)?;
        Ok(())
    }

    fn line_head(&self, project: &str, line: &str) -> Result<Option<String>, StoreError> {
        Ok(self.git(project).rev_parse(&line_ref(line)?))
    }

    fn commit_line(
        &self,
        project: &str,
        line: &str,
        parent: Option<&str>,
        changes: &[FileChange],
        author: &Author,
        message: &str,
    ) -> Result<Option<String>, StoreError> {
        let name = line_ref(line)?;
        let git = self.git(project);
        let scratch = format!("refs/charet/scratch/{}", unique());
        let stream = fast_import_stream(&scratch, parent, changes, author, message)?;
        git.run(&["fast-import", "--quiet", "--done"], Some(&stream))?;
        let commit = git
            .rev_parse(&scratch)
            .ok_or_else(|| StoreError("fast-import wrote no commit".into()))?;
        // Compare-and-swap: an all-zero old value means "the line must not exist yet".
        let moved = git
            .run(
                &["update-ref", &name, &commit, parent.unwrap_or(ZERO)],
                None,
            )
            .is_ok();
        git.run(&["update-ref", "-d", &scratch], None)?;
        Ok(moved.then_some(commit))
    }

    fn commit_detached(
        &self,
        project: &str,
        parent: &str,
        changes: &[FileChange],
        author: &Author,
        message: &str,
    ) -> Result<String, StoreError> {
        let git = self.git(project);
        let scratch = format!("refs/charet/scratch/{}", unique());
        let stream = fast_import_stream(&scratch, Some(parent), changes, author, message)?;
        git.run(&["fast-import", "--quiet", "--done"], Some(&stream))?;
        let commit = git
            .rev_parse(&scratch)
            .ok_or_else(|| StoreError("fast-import wrote no commit".into()));
        git.run(&["update-ref", "-d", &scratch], None)?;
        commit
    }

    fn move_line(
        &self,
        project: &str,
        line: &str,
        new: &str,
        old: Option<&str>,
    ) -> Result<bool, StoreError> {
        let name = line_ref(line)?;
        Ok(self
            .git(project)
            .run(&["update-ref", &name, new, old.unwrap_or(ZERO)], None)
            .is_ok())
    }

    fn merge(
        &self,
        project: &str,
        ours: &str,
        theirs: &str,
        author: &Author,
        message: &str,
    ) -> Result<Merged, StoreError> {
        let git = self.git(project);
        if git.is_ancestor(theirs, ours) {
            return Ok(Merged::Clean(ours.to_owned()));
        }
        let tree = Worktree::add(&git, ours)?;
        let joined = tree.run(
            &["merge", "--no-ff", "--no-edit", "-m", message, theirs],
            author,
        );
        tree.finish(joined, &["merge", "--abort"])
    }

    fn revert(
        &self,
        project: &str,
        onto: &str,
        commit: &str,
        author: &Author,
        message: &str,
    ) -> Result<Merged, StoreError> {
        let git = self.git(project);
        let parents = git.run(&["rev-list", "--parents", "-n", "1", commit], None)?;
        let merge = String::from_utf8_lossy(&parents).split_whitespace().count() > 2;
        let tree = Worktree::add(&git, onto)?;
        let mut args = vec!["revert", "--no-commit"];
        if merge {
            args.extend(["-m", "1"]);
        }
        args.push(commit);
        let undone = tree
            .run(&args, author)
            .and_then(|_| tree.run(&["commit", "--allow-empty", "-m", message], author));
        tree.finish(undone, &["revert", "--abort"])
    }

    fn merge_base(&self, project: &str, a: &str, b: &str) -> Result<Option<String>, StoreError> {
        Ok(self
            .git(project)
            .run(&["merge-base", a, b], None)
            .ok()
            .map(|out| String::from_utf8_lossy(&out).trim().to_owned())
            .filter(|c| !c.is_empty()))
    }

    fn tag_version(&self, project: &str, number: u32, commit: &str) -> Result<(), StoreError> {
        self.git(project).run(
            &["update-ref", &format!("refs/tags/v/{number}"), commit],
            None,
        )?;
        Ok(())
    }

    fn resolve(&self, project: &str, rev: &str) -> Result<Option<String>, StoreError> {
        let name = if rev == "main" {
            MAIN.to_owned()
        } else if let Some(number) = rev.strip_prefix('b').filter(|n| n.parse::<u32>().is_ok()) {
            format!("refs/heads/b/{number}")
        } else if let Some(number) = rev.strip_prefix('v').filter(|n| n.parse::<u32>().is_ok()) {
            format!("refs/tags/v/{number}")
        } else if !rev.is_empty() && rev.len() <= 64 && rev.bytes().all(|b| b.is_ascii_hexdigit()) {
            rev.to_owned()
        } else {
            return Ok(None);
        };
        Ok(self.git(project).rev_parse(&format!("{name}^{{commit}}")))
    }

    fn list(&self, project: &str, commit: &str) -> Result<Vec<String>, StoreError> {
        let out = self.git(project).run(
            &["ls-tree", "-r", "--name-only", "-z", "--full-tree", commit],
            None,
        )?;
        let mut paths: Vec<String> = out
            .split(|b| *b == 0)
            .filter(|p| !p.is_empty())
            .map(|p| String::from_utf8_lossy(p).into_owned())
            .collect();
        paths.sort();
        Ok(paths)
    }

    fn read(&self, project: &str, commit: &str, path: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let object = format!("{commit}:{path}");
        let git = self.git(project);
        // `cat-file -t` fails for a missing path; only blobs are files.
        match git.run(&["cat-file", "-t", &object], None) {
            Ok(kind) if kind.trim_ascii() == b"blob" => {
                git.run(&["cat-file", "blob", &object], None).map(Some)
            }
            _ => Ok(None),
        }
    }

    fn archive(&self, project: &str, commit: &str) -> Result<Vec<u8>, StoreError> {
        self.git(project)
            .run(&["archive", "--format=tar.gz", commit], None)
    }

    fn patch(
        &self,
        project: &str,
        from: &str,
        to: &str,
        paths: &[String],
    ) -> Result<Vec<u8>, StoreError> {
        let mut args = vec![
            "diff",
            "--binary",
            "--full-index",
            "--no-renames",
            "--no-color",
            "--no-ext-diff",
            from,
            to,
            "--",
        ];
        // Literal paths: no pathspec magic or globbing from file names.
        let literal: Vec<String> = paths.iter().map(|p| format!(":(literal){p}")).collect();
        args.extend(literal.iter().map(String::as_str));
        self.git(project).run(&args, None)
    }

    fn diff(&self, project: &str, from: &str, to: &str) -> Result<Vec<Change>, StoreError> {
        let out = self.git(project).run(
            &["diff", "--name-status", "--no-renames", "-z", from, to],
            None,
        )?;
        let mut fields = out.split(|b| *b == 0).filter(|f| !f.is_empty());
        let mut changes = Vec::new();
        while let (Some(status), Some(path)) = (fields.next(), fields.next()) {
            let kind = match status.first() {
                Some(b'A') => ChangeKind::Added,
                Some(b'D') => ChangeKind::Deleted,
                _ => ChangeKind::Modified,
            };
            changes.push(Change {
                path: String::from_utf8_lossy(path).into_owned(),
                kind,
            });
        }
        Ok(changes)
    }
}

/// Builds a `git fast-import` stream that commits `changes` on `parent` into `scratch`.
fn fast_import_stream(
    scratch: &str,
    parent: Option<&str>,
    changes: &[FileChange],
    author: &Author,
    message: &str,
) -> Result<Vec<u8>, StoreError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(err)?
        .as_secs();
    let who = format!(
        "{} <{}> {now} +0000",
        clean(&author.name),
        clean(&author.email)
    );
    let mut out = Vec::new();
    writeln!(out, "commit {scratch}").map_err(err)?;
    writeln!(out, "author {who}").map_err(err)?;
    writeln!(out, "committer {who}").map_err(err)?;
    data(&mut out, message.as_bytes())?;
    if let Some(parent) = parent {
        writeln!(out, "from {parent}").map_err(err)?;
    }
    for change in changes {
        if change.path.contains('\n') || change.path.starts_with('"') {
            return Err(StoreError(format!("unsupported path {:?}", change.path)));
        }
        match &change.content {
            Some(content) => {
                let mode = if change.executable {
                    "100755"
                } else {
                    "100644"
                };
                writeln!(out, "M {mode} inline {}", change.path).map_err(err)?;
                data(&mut out, content)?;
            }
            None => writeln!(out, "D {}", change.path).map_err(err)?,
        }
    }
    writeln!(out, "done").map_err(err)?;
    Ok(out)
}

fn data(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), StoreError> {
    writeln!(out, "data {}", bytes.len()).map_err(err)?;
    out.extend_from_slice(bytes);
    out.push(b'\n');
    Ok(())
}

/// Removes characters that would break a fast-import identity line.
fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(c, '<' | '>' | '\n'))
        .collect()
}

fn unique() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!(
        "{}-{nanos}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// One bare repository, driven through the `git` binary with user and system config ignored.
struct Git {
    dir: PathBuf,
}

impl Git {
    fn run(&self, args: &[&str], stdin: Option<&[u8]>) -> Result<Vec<u8>, StoreError> {
        let mut child = Command::new("git")
            .current_dir(&self.dir)
            .arg("--git-dir")
            .arg(&self.dir)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", null_device())
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| StoreError(format!("cannot run git: {e}")))?;
        if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
            pipe.write_all(input).map_err(err)?;
        }
        let output = child.wait_with_output().map_err(err)?;
        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(StoreError(format!(
                "git {} failed: {}",
                args.first().copied().unwrap_or_default(),
                String::from_utf8_lossy(&output.stderr).trim()
            )))
        }
    }

    /// The commit id `name` points at, or `None` if it does not resolve.
    fn rev_parse(&self, name: &str) -> Option<String> {
        self.run(&["rev-parse", "--verify", "--quiet", name], None)
            .ok()
            .map(|out| String::from_utf8_lossy(&out).trim().to_owned())
    }
}

impl Git {
    fn is_ancestor(&self, ancestor: &str, of: &str) -> bool {
        self.run(&["merge-base", "--is-ancestor", ancestor, of], None)
            .is_ok()
    }
}

/// A scratch checkout of one commit, for merges and reverts that need a working tree. Removed
/// when done; nothing it does moves a line.
struct Worktree<'a> {
    git: &'a Git,
    dir: PathBuf,
}

impl<'a> Worktree<'a> {
    fn add(git: &'a Git, commit: &str) -> Result<Self, StoreError> {
        let dir = std::env::temp_dir().join(format!("charet-tree-{}", unique()));
        let path = dir.to_string_lossy().into_owned();
        git.run(
            &["worktree", "add", "--quiet", "--detach", &path, commit],
            None,
        )?;
        Ok(Self { git, dir })
    }

    /// Runs git in the tree, committing as `author`.
    fn run(&self, args: &[&str], author: &Author) -> Result<Vec<u8>, StoreError> {
        let (name, email) = (clean(&author.name), clean(&author.email));
        let output = Command::new("git")
            .current_dir(&self.dir)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", null_device())
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", &name)
            .env("GIT_AUTHOR_EMAIL", &email)
            .env("GIT_COMMITTER_NAME", &name)
            .env("GIT_COMMITTER_EMAIL", &email)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| StoreError(format!("cannot run git: {e}")))?;
        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(StoreError(format!(
                "git {} failed: {}",
                args.first().copied().unwrap_or_default(),
                String::from_utf8_lossy(&output.stderr).trim()
            )))
        }
    }

    /// The new commit when `outcome` succeeded, else the clashing files (after `abort`), or the
    /// failure itself when nothing clashed. Removes the tree.
    fn finish(
        self,
        outcome: Result<Vec<u8>, StoreError>,
        abort: &[&str],
    ) -> Result<Merged, StoreError> {
        let nobody = Author {
            name: "charet".into(),
            email: "charet@localhost".into(),
        };
        let result = if outcome.is_ok() {
            self.run(&["rev-parse", "HEAD"], &nobody)
                .map(|out| Merged::Clean(String::from_utf8_lossy(&out).trim().to_owned()))
        } else {
            let out = self.run(&["diff", "--name-only", "--diff-filter=U", "-z"], &nobody);
            let _ = self.run(abort, &nobody);
            out.and_then(|out| {
                let files: Vec<String> = out
                    .split(|b| *b == 0)
                    .filter(|p| !p.is_empty())
                    .map(|p| String::from_utf8_lossy(p).into_owned())
                    .collect();
                match outcome {
                    Err(error) if files.is_empty() => Err(error),
                    _ => Ok(Merged::Conflict(files)),
                }
            })
        };
        let path = self.dir.to_string_lossy().into_owned();
        let _ = self
            .git
            .run(&["worktree", "remove", "--force", &path], None);
        let _ = std::fs::remove_dir_all(&self.dir);
        let _ = self.git.run(&["worktree", "prune"], None);
        result
    }
}

fn null_device() -> &'static Path {
    Path::new("/dev/null")
}

fn err(error: impl std::fmt::Display) -> StoreError {
    StoreError(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(case: &str) -> (GitStore, PathBuf) {
        let root = std::env::temp_dir().join(format!("charet-store-{}-{case}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        (GitStore::new(&root), root)
    }

    fn author() -> Author {
        Author {
            name: "Me <x>".into(),
            email: "me@example.com".into(),
        }
    }

    fn write(path: &str, content: &str) -> FileChange {
        FileChange {
            path: path.into(),
            content: Some(content.as_bytes().to_vec()),
            executable: false,
        }
    }

    #[test]
    fn commits_versions_and_diffs() {
        let (store, root) = store("flow");
        store.create("web").unwrap();
        assert!(store.create("web").is_err());
        assert_eq!(store.head("web").unwrap(), None);

        let files = [write("a.txt", "1"), write("dir/b file.txt", "2")];
        let c1 = store
            .commit_main("web", None, &files, &author(), "first")
            .unwrap()
            .unwrap();
        assert_eq!(store.head("web").unwrap().as_deref(), Some(c1.as_str()));
        store.tag_version("web", 1, &c1).unwrap();

        let delete = FileChange {
            path: "a.txt".into(),
            content: None,
            executable: false,
        };
        let c2 = store
            .commit_main(
                "web",
                Some(&c1),
                &[delete, write("c.sh", "#!/bin/sh")],
                &author(),
                "second",
            )
            .unwrap()
            .unwrap();

        // A commit on a stale parent is written but main does not move.
        let stale = store
            .commit_main("web", Some(&c1), &[write("x", "y")], &author(), "late")
            .unwrap();
        assert_eq!(stale, None);
        assert_eq!(store.head("web").unwrap().as_deref(), Some(c2.as_str()));

        assert_eq!(
            store.resolve("web", "v1").unwrap().as_deref(),
            Some(c1.as_str())
        );
        assert_eq!(
            store.resolve("web", "main").unwrap().as_deref(),
            Some(c2.as_str())
        );
        assert_eq!(
            store.resolve("web", &c1[..10]).unwrap().as_deref(),
            Some(c1.as_str())
        );
        assert_eq!(store.resolve("web", "v9").unwrap(), None);
        assert_eq!(
            store.read("web", &c1, "dir/b file.txt").unwrap().as_deref(),
            Some(&b"2"[..])
        );
        assert_eq!(store.read("web", &c2, "a.txt").unwrap(), None, "deleted");
        assert_eq!(store.list("web", &c2).unwrap(), ["c.sh", "dir/b file.txt"]);
        assert_eq!(store.read("web", &c1, "dir").unwrap(), None, "a tree");
        assert_eq!(store.resolve("web", "--help").unwrap(), None);

        let changes = store.diff("web", &c1, &c2).unwrap();
        assert_eq!(
            changes,
            vec![
                Change {
                    path: "a.txt".into(),
                    kind: ChangeKind::Deleted
                },
                Change {
                    path: "c.sh".into(),
                    kind: ChangeKind::Added
                },
            ]
        );

        let archive = store.archive("web", &c2).unwrap();
        assert_eq!(&archive[..2], &[0x1f, 0x8b], "gzip magic");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn patches_only_the_paths_asked_for_and_detached_commits() {
        let (store, root) = store("patch");
        store.create("web").unwrap();
        let c1 = store
            .commit_main(
                "web",
                None,
                &[write("src/a.tsx", "a1\n"), write(".charet/m.ts", "m1\n")],
                &author(),
                "first",
            )
            .unwrap()
            .unwrap();
        let c2 = store
            .commit_detached(
                "web",
                &c1,
                &[write("src/a.tsx", "a2\n"), write(".charet/m.ts", "m2\n")],
                &author(),
                "second",
            )
            .unwrap();
        assert_eq!(
            store.head("web").unwrap(),
            Some(c1.clone()),
            "no line moved"
        );
        let patch = String::from_utf8(
            store
                .patch("web", &c1, &c2, &["src/a.tsx".to_owned()])
                .unwrap(),
        )
        .unwrap();
        assert!(
            patch.starts_with("diff --git a/src/a.tsx b/src/a.tsx\n"),
            "{patch}"
        );
        assert!(patch.contains("-a1\n+a2\n"), "{patch}");
        assert!(!patch.contains(".charet"), "{patch}");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn removes_a_project_so_its_key_starts_over() {
        let (store, root) = store("remove");
        store.create("web").unwrap();
        store
            .commit_main("web", None, &[write("a.txt", "1")], &author(), "first")
            .unwrap();
        store.remove("web").unwrap();
        store.remove("web").unwrap();
        assert!(store.remove("../x").is_err());
        store.create("web").unwrap();
        assert_eq!(store.head("web").unwrap(), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn branches_merge_clash_and_revert() {
        let (store, root) = store("lines");
        store.create("web").unwrap();
        let c1 = store
            .commit_main(
                "web",
                None,
                &[write("a.txt", "one\n"), write("b.txt", "b\n")],
                &author(),
                "first",
            )
            .unwrap()
            .unwrap();
        assert!(store.move_line("web", "b/1", &c1, None).unwrap());
        assert!(
            !store.move_line("web", "b/1", &c1, None).unwrap(),
            "exists already"
        );
        assert!(store.move_line("web", "../x", &c1, None).is_err());
        let b1 = store
            .commit_line(
                "web",
                "b/1",
                Some(&c1),
                &[write("a.txt", "two\n")],
                &author(),
                "edit a",
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            store.resolve("web", "b1").unwrap().as_deref(),
            Some(b1.as_str())
        );
        let m1 = store
            .commit_main(
                "web",
                Some(&c1),
                &[write("b.txt", "bb\n")],
                &author(),
                "edit b",
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            store.merge_base("web", &b1, &m1).unwrap().as_deref(),
            Some(c1.as_str())
        );

        // Different files join cleanly into a commit with both parents.
        let Merged::Clean(joined) = store.merge("web", &m1, &b1, &author(), "Merge b1").unwrap()
        else {
            panic!("clean merge expected");
        };
        assert_eq!(
            store.read("web", &joined, "a.txt").unwrap().as_deref(),
            Some(&b"two\n"[..])
        );
        assert_eq!(
            store.read("web", &joined, "b.txt").unwrap().as_deref(),
            Some(&b"bb\n"[..])
        );
        assert_eq!(
            store.head("web").unwrap().as_deref(),
            Some(m1.as_str()),
            "no line moved"
        );
        assert_eq!(
            store
                .merge("web", &joined, &b1, &author(), "again")
                .unwrap(),
            Merged::Clean(joined.clone()),
            "already in"
        );

        // The same line changed on both sides clashes.
        let b2 = store
            .commit_line(
                "web",
                "b/1",
                Some(&b1),
                &[write("b.txt", "mine\n")],
                &author(),
                "b too",
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            store.merge("web", &m1, &b2, &author(), "clash").unwrap(),
            Merged::Conflict(vec!["b.txt".into()])
        );

        let worktrees = store
            .git("web")
            .run(&["worktree", "list", "--porcelain"], None)
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&worktrees)
                .matches("worktree ")
                .count(),
            1,
            "cleaned up"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn revert_undoes_a_merge() {
        let (store, root) = store("revert");
        store.create("web").unwrap();
        let c1 = store
            .commit_main(
                "web",
                None,
                &[write("a.txt", "one\n"), write("b.txt", "b\n")],
                &author(),
                "first",
            )
            .unwrap()
            .unwrap();
        let m1 = store
            .commit_main(
                "web",
                Some(&c1),
                &[write("b.txt", "bb\n")],
                &author(),
                "edit b",
            )
            .unwrap()
            .unwrap();
        assert!(store.move_line("web", "b/1", &c1, None).unwrap());
        let b1 = store
            .commit_line(
                "web",
                "b/1",
                Some(&c1),
                &[write("a.txt", "two\n")],
                &author(),
                "edit a",
            )
            .unwrap()
            .unwrap();
        let Merged::Clean(joined) = store.merge("web", &m1, &b1, &author(), "Merge b1").unwrap()
        else {
            panic!("clean merge expected");
        };
        // Undoing the merge brings back what main had before it.
        let Merged::Clean(undone) = store
            .revert("web", &joined, &joined, &author(), "Restore")
            .unwrap()
        else {
            panic!("clean revert expected");
        };
        assert_eq!(
            store.read("web", &undone, "a.txt").unwrap().as_deref(),
            Some(&b"one\n"[..])
        );
        assert_eq!(
            store.read("web", &undone, "b.txt").unwrap().as_deref(),
            Some(&b"bb\n"[..])
        );
        let worktrees = store
            .git("web")
            .run(&["worktree", "list", "--porcelain"], None)
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&worktrees)
                .matches("worktree ")
                .count(),
            1,
            "cleaned up"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
