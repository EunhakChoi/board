//! Project store: one git repository per project (main, branches, versions).
//!
//! Drives the `git` command line on bare repositories under `<root>/<project>/repo.git`.
//! Commits are written with `git fast-import` into a scratch ref, then `main` is moved with a
//! compare-and-swap `update-ref`, so a commit lands only if `main` did not move meanwhile.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use board_engine::{Author, Change, ChangeKind, FileChange, ProjectStore, StoreError};

const MAIN: &str = "refs/heads/main";

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

    fn head(&self, project: &str) -> Result<Option<String>, StoreError> {
        Ok(self.git(project).rev_parse(MAIN))
    }

    fn commit_main(
        &self,
        project: &str,
        parent: Option<&str>,
        changes: &[FileChange],
        author: &Author,
        message: &str,
    ) -> Result<Option<String>, StoreError> {
        let git = self.git(project);
        let scratch = format!("refs/board/scratch/{}", unique());
        let stream = fast_import_stream(&scratch, parent, changes, author, message)?;
        git.run(&["fast-import", "--quiet", "--done"], Some(&stream))?;
        let commit = git
            .rev_parse(&scratch)
            .ok_or_else(|| StoreError("fast-import wrote no commit".into()))?;
        // Compare-and-swap: an all-zero old value means "main must not exist yet".
        let old = parent.map_or_else(|| "0".repeat(40), str::to_owned);
        let moved = git.run(&["update-ref", MAIN, &commit, &old], None).is_ok();
        git.run(&["update-ref", "-d", &scratch], None)?;
        Ok(moved.then_some(commit))
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
        let root = std::env::temp_dir().join(format!("board-store-{}-{case}", std::process::id()));
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
}
