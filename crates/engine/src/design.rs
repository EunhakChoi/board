//! Hand edits (story 05): change one element of a screen on a draft, by the rules of
//! `charet-design`. Each edit is one commit on the person's draft; it is checked like a merge
//! (the draft builds, the screens it can reach open without new problems) and undone on its own
//! when the check fails. Undo and redo put a file back as it was at an earlier commit.
//!
//! An element comes with up to two places: `src`, the host element as written (maybe inside a
//! shared component), and `use`, where the component that renders it is used on the screen.
//! Editing `use` changes that one instance; editing `src` changes the component everywhere.

use std::collections::BTreeMap;

use charet_design::{Edit, Element, Place, Variant};
use charet_model::{Actor, Branch};

use crate::captures::app_dir;
use crate::policy::Command;
use crate::{BuildState, Engine, EngineError, FileChange};

/// One place an element can be edited at.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesignTarget {
    /// `path:line:col`, the project path.
    pub at: String,
    pub element: Element,
    /// `cva` variants of the component, when it is one and its file defines them.
    pub variants: BTreeMap<String, Variant>,
    /// Stored captures rendered from this file: how many screens a change here can reach.
    pub screens: usize,
}

/// What a hand edit can do to an element: on this instance and in its own source.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesignInfo {
    /// The revision looked at.
    pub commit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<DesignTarget>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original: Option<DesignTarget>,
}

/// A hand edit that was saved: the commit, the file and where the element is now.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Edited {
    pub branch: Branch,
    pub path: String,
    /// The head before the edit and the edit's own commit (for undo and the check).
    pub before: String,
    pub commit: String,
    /// The element's place after the edit (`path:line:col`).
    pub at: String,
}

/// How the automatic check of an edit went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EditState {
    Passed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditCheck {
    pub state: EditState,
    pub issues: Vec<String>,
    /// The edit was taken back (its file is as it was before it).
    pub reverted: bool,
}

/// A hand edit asked for.
#[derive(Debug, Clone)]
pub struct HandEdit {
    /// The draft to edit; `None` starts a new one called `title`.
    pub branch: Option<u32>,
    pub title: String,
    /// `path:line:col` (a project path), as `DesignTarget::at` gave it.
    pub at: String,
    pub edit: Edit,
}

fn split(at: &str) -> Result<(&str, Place), EngineError> {
    Place::parse_location(at).ok_or_else(|| EngineError::Invalid(format!("not a place: {at:?}")))
}

fn location(path: &str, place: Place) -> String {
    format!("{path}:{}:{}", place.line, place.column)
}

#[allow(clippy::needless_pass_by_value)] // the shape `map_err` hands it over in
fn design_error(error: charet_design::DesignError) -> EngineError {
    EngineError::Invalid(error.to_string())
}

/// `@/x` and relative specifiers as a project path without extension; `None` for packages.
fn module_path(app: &str, from: &str, specifier: &str) -> Option<String> {
    let joined = if let Some(rest) = specifier
        .strip_prefix("@/")
        .or(specifier.strip_prefix("~/"))
    {
        if app.is_empty() {
            format!("src/{rest}")
        } else {
            format!("{app}/src/{rest}")
        }
    } else if specifier.starts_with("./") || specifier.starts_with("../") {
        let dir = from.rsplit_once('/').map_or("", |(d, _)| d);
        format!("{dir}/{specifier}")
    } else {
        return None;
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

impl Engine {
    fn text_at(&self, key: &str, commit: &str, path: &str) -> Result<String, EngineError> {
        let bytes = self
            .ports
            .store
            .read(key, commit, path)?
            .ok_or(EngineError::NotFound("file"))?;
        String::from_utf8(bytes).map_err(|_| EngineError::Invalid(format!("{path} is not text")))
    }

    /// A data-src path (relative to the app) as a project path.
    fn project_path(&self, key: &str, commit: &str, src: &str) -> Result<String, EngineError> {
        let app = app_dir(
            self.ports
                .store
                .read(key, commit, "charet.json")?
                .as_deref(),
        );
        let (path, place) = split(src)?;
        let path = if app.is_empty() {
            path.to_owned()
        } else {
            format!("{app}/{path}")
        };
        Ok(location(&path, place))
    }

    fn design_target(
        &self,
        key: &str,
        commit: &str,
        at: &str,
    ) -> Result<DesignTarget, EngineError> {
        let (path, place) = split(at)?;
        let source = self.text_at(key, commit, path)?;
        let element = charet_design::inspect(path, &source, place).map_err(design_error)?;
        let app = app_dir(
            self.ports
                .store
                .read(key, commit, "charet.json")?
                .as_deref(),
        );
        let mut variants = BTreeMap::new();
        if element.component {
            let local = element.name.split('.').next().unwrap_or_default();
            let imports = charet_design::imports(path, &source).unwrap_or_default();
            let file = imports
                .get(local)
                .and_then(|specifier| module_path(&app, path, specifier));
            if let Some(file) = file {
                for candidate in [".tsx", ".ts", "/index.tsx", "/index.ts", ".jsx"] {
                    let name = format!("{file}{candidate}");
                    if let Some(bytes) = self.ports.store.read(key, commit, &name)? {
                        let text = String::from_utf8_lossy(&bytes).into_owned();
                        variants = charet_design::variants(&name, &text).unwrap_or_default();
                        break;
                    }
                }
            }
        }
        let relative = path
            .strip_prefix(&format!("{app}/"))
            .filter(|_| !app.is_empty())
            .unwrap_or(path);
        let screens = self
            .ports
            .captures
            .list(key)?
            .iter()
            .filter(|c| c.theme == "light" && c.sources.iter().any(|s| s == relative))
            .count();
        Ok(DesignTarget {
            at: at.to_owned(),
            element,
            variants,
            screens,
        })
    }

    /// What a hand edit can change on an element of `rev`: `src` and `use` as the element map
    /// has them (relative to the app).
    pub fn design(
        &self,
        actor: &Actor,
        key: &str,
        rev: &str,
        src: Option<&str>,
        used: Option<&str>,
    ) -> Result<DesignInfo, EngineError> {
        self.visible(actor, key, Command::BranchEdit)?;
        let commit = self.resolve(key, rev)?;
        let original = src
            .map(|s| self.project_path(key, &commit, s))
            .transpose()?
            .map(|at| self.design_target(key, &commit, &at))
            .transpose()?;
        // The instance only counts when it is somewhere else than the element itself.
        let instance = match used {
            Some(u) => {
                let at = self.project_path(key, &commit, u)?;
                let same_file = original.as_ref().is_some_and(|o| {
                    split(&o.at).ok().map(|x| x.0) == split(&at).ok().map(|x| x.0)
                });
                if same_file {
                    None
                } else {
                    self.design_target(key, &commit, &at).ok()
                }
            }
            None => None,
        };
        if original.is_none() && instance.is_none() {
            return Err(EngineError::NotFound("element"));
        }
        Ok(DesignInfo {
            commit,
            instance,
            original,
        })
    }

    /// Applies one hand edit on the actor's draft (a new one when `branch` is `None`).
    pub fn hand_edit(
        &self,
        actor: &Actor,
        key: &str,
        request: &HandEdit,
    ) -> Result<Edited, EngineError> {
        self.visible(actor, key, Command::BranchEdit)?;
        let branch = if let Some(number) = request.branch {
            let branch = self.find_branch(key, number)?;
            Self::may_change(actor, &branch)?;
            branch
        } else {
            // Check the edit applies before starting a draft for it.
            let main = self.resolve(key, "main")?;
            let (path, place) = split(&request.at)?;
            let source = self.text_at(key, &main, path)?;
            charet_design::apply(path, &source, place, &request.edit).map_err(design_error)?;
            let title = if request.title.trim().is_empty() {
                "Hand edits"
            } else {
                request.title.trim()
            };
            self.create_branch(actor, key, title, None)?
        };
        let (path, place) = split(&request.at)?;
        let source = self.text_at(key, &branch.head, path)?;
        let applied =
            charet_design::apply(path, &source, place, &request.edit).map_err(design_error)?;
        if applied.source == source {
            return Err(EngineError::Invalid("that changes nothing".into()));
        }
        let message = summary(&request.edit);
        let before = branch.head.clone();
        let executable = false;
        let pushed = self.push_branch(
            actor,
            key,
            branch.number,
            &before,
            &message,
            &[FileChange {
                path: path.to_owned(),
                content: Some(applied.source.into_bytes()),
                executable,
            }],
        )?;
        Ok(Edited {
            commit: pushed.head.clone(),
            branch: pushed,
            path: path.to_owned(),
            before,
            at: location(path, applied.place),
        })
    }

    /// Checks an edit (`before` → `commit` on `branch`) like a merge: the draft builds and the
    /// screens it can reach open without new problems. On failure the edit's file goes back to
    /// how it was, unless someone changed it again since. Pending while the draft builds.
    pub fn check_edit(
        &self,
        actor: &Actor,
        key: &str,
        number: u32,
        path: &str,
        before: &str,
        commit: &str,
    ) -> Result<EditCheck, EngineError> {
        self.visible(actor, key, Command::BranchEdit)?;
        let branch = self.find_branch(key, number)?;
        let issues = if self.preview.is_none() {
            Vec::new()
        } else {
            let preview = self.preview(actor, key, commit, false)?;
            match preview.build.state {
                BuildState::Queued | BuildState::Running => {
                    return Err(EngineError::Pending(
                        "building the draft with the edit; try again shortly".into(),
                    ));
                }
                BuildState::Failed => vec![format!("build: {}", preview.build.error.trim())],
                BuildState::Ready => self.check_reach(key, before, &preview.commit)?,
            }
        };
        if issues.is_empty() {
            return Ok(EditCheck {
                state: EditState::Passed,
                issues,
                reverted: false,
            });
        }
        let edited = self.ports.store.read(key, commit, path)?;
        let earlier = self.ports.store.read(key, before, path)?;
        let now = self.ports.store.read(key, &branch.head, path)?;
        if now == edited && branch.status.active() {
            self.push_branch(
                actor,
                key,
                number,
                &branch.head,
                &format!("Undo a hand edit that failed its check ({path})"),
                &[FileChange {
                    path: path.to_owned(),
                    content: earlier.clone(),
                    executable: false,
                }],
            )?;
        }
        let reverted = self
            .ports
            .store
            .read(key, &self.find_branch(key, number)?.head, path)?
            == earlier;
        Ok(EditCheck {
            state: EditState::Failed,
            issues,
            reverted,
        })
    }

    /// Undo or redo: puts `path` back as it was at `to`, when it is still as it was at `expect`
    /// (nobody changed it since).
    pub fn restore_edit(
        &self,
        actor: &Actor,
        key: &str,
        number: u32,
        path: &str,
        to: &str,
        expect: &str,
    ) -> Result<Edited, EngineError> {
        self.visible(actor, key, Command::BranchEdit)?;
        let branch = self.find_branch(key, number)?;
        Self::may_change(actor, &branch)?;
        let now = self.ports.store.read(key, &branch.head, path)?;
        if now != self.ports.store.read(key, expect, path)? {
            return Err(EngineError::Conflict { head: branch.head });
        }
        let content = self.ports.store.read(key, to, path)?;
        if content == now {
            return Err(EngineError::Invalid("that changes nothing".into()));
        }
        let before = branch.head.clone();
        let pushed = self.push_branch(
            actor,
            key,
            number,
            &before,
            &format!("Undo / redo a hand edit ({path})"),
            &[FileChange {
                path: path.to_owned(),
                content,
                executable: false,
            }],
        )?;
        Ok(Edited {
            commit: pushed.head.clone(),
            branch: pushed,
            path: path.to_owned(),
            before,
            at: String::new(),
        })
    }
}

fn summary(edit: &Edit) -> String {
    match edit {
        Edit::Text(text) => format!(
            "Hand edit: text \"{}\"",
            text.chars().take(40).collect::<String>()
        ),
        Edit::ClassName(classes) => format!("Hand edit: classes \"{classes}\""),
        Edit::Prop {
            name,
            value: Some(value),
        } => format!("Hand edit: {name}=\"{value}\""),
        Edit::Prop { name, value: None } => format!("Hand edit: remove {name}"),
        Edit::Move(by) if *by < 0 => "Hand edit: move up".into(),
        Edit::Move(_) => "Hand edit: move down".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Fakes, engine_with_fakes, file};
    use charet_model::Actor;

    const PAGE: &str = "import { Button } from \"@/components/ui/button\";\nexport function Page() {\n  return (\n    <main className=\"p-4\">\n      <h1>Tasks</h1>\n      <Button variant=\"outline\">Open</Button>\n    </main>\n  );\n}\n";
    const BUTTON: &str = "import { cva } from \"class-variance-authority\";\nconst buttonVariants = cva(\"x\", { variants: { variant: { default: \"\", outline: \"\" }, size: { sm: \"\", lg: \"\" } } });\nexport function Button(props) {\n  return <button className={buttonVariants(props)} {...props} />;\n}\n";

    fn ann() -> Actor {
        Actor::User {
            id: "u-ann".into(),
            name: "Ann".into(),
            role: charet_model::Role::Editor,
        }
    }

    fn setup() -> (Engine, Fakes) {
        let (engine, fakes) = engine_with_fakes();
        let import = crate::Import {
            files: vec![
                file("charet.json", r#"{"app":"web"}"#),
                file("web/src/page.tsx", PAGE),
                file("web/src/components/ui/button.tsx", BUTTON),
            ],
            source: "test".into(),
            commit: None,
        };
        engine
            .create_project(&Actor::Local, "shop", "Shop", Some(import))
            .unwrap();
        (engine, fakes)
    }

    fn read(engine: &Engine, rev: &str, path: &str) -> String {
        let commit = engine.resolve("shop", rev).unwrap();
        engine.text_at("shop", &commit, path).unwrap()
    }

    #[test]
    fn an_instance_offers_its_variants_and_the_original_its_own_source() {
        let (engine, _) = setup();
        let info = engine
            .design(
                &ann(),
                "shop",
                "main",
                Some("src/components/ui/button.tsx:4:10"),
                Some("src/page.tsx:6:7"),
            )
            .unwrap();
        let instance = info.instance.unwrap();
        assert_eq!(instance.at, "web/src/page.tsx:6:7");
        assert_eq!(instance.element.name, "Button");
        assert_eq!(instance.variants["variant"].options, ["default", "outline"]);
        assert_eq!(instance.variants["size"].options, ["sm", "lg"]);
        let original = info.original.unwrap();
        assert_eq!(original.element.name, "button");
        assert_eq!(original.element.class_name, charet_design::Value::Computed);
    }

    #[test]
    fn edits_go_on_one_draft_and_can_be_undone() {
        let (engine, _) = setup();
        let first = engine
            .hand_edit(
                &ann(),
                "shop",
                &HandEdit {
                    branch: None,
                    title: "Tidy the page".into(),
                    at: "web/src/page.tsx:5:7".into(),
                    edit: Edit::Text("Today".into()),
                },
            )
            .unwrap();
        assert_eq!(first.branch.title, "Tidy the page");
        assert_eq!(first.at, "web/src/page.tsx:5:7");
        let number = first.branch.number;
        let second = engine
            .hand_edit(
                &ann(),
                "shop",
                &HandEdit {
                    branch: Some(number),
                    title: String::new(),
                    at: "web/src/page.tsx:5:7".into(),
                    edit: Edit::Move(1),
                },
            )
            .unwrap();
        assert_eq!(second.at, "web/src/page.tsx:6:7");
        let head = read(&engine, &format!("b{number}"), "web/src/page.tsx");
        assert!(head.find("<Button").unwrap() < head.find("<h1>Today</h1>").unwrap());
        assert!(
            read(&engine, "main", "web/src/page.tsx").contains("<h1>Tasks</h1>"),
            "main untouched"
        );

        // Undo the move: the file goes back to how the first edit left it.
        let undone = engine
            .restore_edit(
                &ann(),
                "shop",
                number,
                &second.path,
                &second.before,
                &second.commit,
            )
            .unwrap();
        let head = read(&engine, &format!("b{number}"), "web/src/page.tsx");
        assert!(head.find("<h1>Today</h1>").unwrap() < head.find("<Button").unwrap());
        // Redo once nobody changed it since.
        engine
            .restore_edit(
                &ann(),
                "shop",
                number,
                &second.path,
                &second.commit,
                &undone.commit,
            )
            .unwrap();
        assert!(matches!(
            engine.restore_edit(
                &ann(),
                "shop",
                number,
                &second.path,
                &second.commit,
                &undone.commit
            ),
            Err(EngineError::Conflict { .. })
        ));
    }

    #[test]
    fn a_computed_value_is_refused_before_any_draft_is_made() {
        let (engine, _) = setup();
        let refused = engine.hand_edit(
            &ann(),
            "shop",
            &HandEdit {
                branch: None,
                title: String::new(),
                at: "web/src/components/ui/button.tsx:4:10".into(),
                edit: Edit::ClassName("px-6".into()),
            },
        );
        assert!(matches!(refused, Err(EngineError::Invalid(r)) if r.contains("computed")));
        assert_eq!(engine.branches(&ann(), "shop").unwrap(), []);
    }

    #[test]
    fn someone_else_s_draft_is_not_hand_edited() {
        let (engine, _) = setup();
        let mine = engine
            .hand_edit(
                &ann(),
                "shop",
                &HandEdit {
                    branch: None,
                    title: String::new(),
                    at: "web/src/page.tsx:5:7".into(),
                    edit: Edit::Text("Today".into()),
                },
            )
            .unwrap();
        let bo = Actor::User {
            id: "u-bo".into(),
            name: "Bo".into(),
            role: charet_model::Role::Editor,
        };
        let theirs = engine.hand_edit(
            &bo,
            "shop",
            &HandEdit {
                branch: Some(mine.branch.number),
                title: String::new(),
                at: "web/src/page.tsx:5:7".into(),
                edit: Edit::Text("Mine".into()),
            },
        );
        assert!(matches!(theirs, Err(EngineError::Forbidden)));
    }

    #[test]
    fn an_edit_that_breaks_a_screen_is_taken_back() {
        const MANIFEST: &str = r#"{"manifestVersion":1,"title":"T","pages":[],
            "screens":[{"id":"page","page":"p","title":"P","note":"","route":"/","size":{"w":800,"h":600},"hasSetup":false}],
            "connections":[],"flows":[],"stories":[]}"#;
        let (engine, fakes) = setup();
        engine
            .preview(&Actor::Local, "shop", "main", false)
            .unwrap();
        fakes.workspace.finish(BuildState::Ready);
        fakes.workspace.serve(".charet/manifest.json", MANIFEST);
        engine
            .capture(&Actor::Local, "shop", "main", &[], &[], false)
            .unwrap();
        let edit = |text: &str, branch| {
            engine
                .hand_edit(
                    &ann(),
                    "shop",
                    &HandEdit {
                        branch,
                        title: String::new(),
                        at: "web/src/page.tsx:5:7".into(),
                        edit: Edit::Text(text.into()),
                    },
                )
                .unwrap()
        };
        let good = edit("Today", None);
        let number = good.branch.number;
        let check =
            |e: &Edited| engine.check_edit(&ann(), "shop", number, &e.path, &e.before, &e.commit);
        assert!(
            matches!(check(&good), Err(EngineError::Pending(_))),
            "the draft builds first"
        );
        fakes.workspace.finish(BuildState::Ready);
        assert_eq!(check(&good).unwrap().state, EditState::Passed);

        let bad = edit("Broken", Some(number));
        assert!(matches!(check(&bad), Err(EngineError::Pending(_))));
        fakes.workspace.finish(BuildState::Ready);
        fakes.renderer.set_issues("page", &["page error: boom"]);
        let failed = check(&bad).unwrap();
        assert_eq!(failed.state, EditState::Failed);
        assert_eq!(failed.issues, ["screen page: page error: boom"]);
        assert!(failed.reverted);
        assert!(
            read(&engine, &format!("b{number}"), "web/src/page.tsx").contains("<h1>Today</h1>")
        );
        // Asking again does not take back anything more.
        let again = check(&bad).unwrap();
        assert!(again.reverted);
        assert!(
            read(&engine, &format!("b{number}"), "web/src/page.tsx").contains("<h1>Today</h1>")
        );
    }

    #[test]
    fn module_paths_follow_the_alias_and_relative_imports() {
        assert_eq!(
            module_path("web", "web/src/page.tsx", "@/components/ui/button").as_deref(),
            Some("web/src/components/ui/button")
        );
        assert_eq!(
            module_path("", "src/pages/a.tsx", "../ui/b").as_deref(),
            Some("src/ui/b")
        );
        assert_eq!(module_path("", "src/a.tsx", "react"), None);
    }
}
