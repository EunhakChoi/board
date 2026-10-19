//! Checks and captures of screens and stories (`docs/design/store-runtime.md` §5).
//!
//! A capture is keyed by (kind, target, theme) and remembers the commit it was taken at. Taking it
//! again at the same commit does nothing. A capture is stale when `main` moved and the change can
//! reach it: a changed file it rendered from, or a changed file no capture rendered from (unknown
//! reach, so every capture counts as stale).

use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;
use std::sync::Mutex;

use board_model::Actor;
use serde::{Deserialize, Serialize};

use crate::policy::Command;
use crate::preview::{base_path, token};
use crate::{BuildState, Capture, EngineError, ImageKind, RenderJob, Shot};
use crate::{Engine, PreviewSettings};

/// Renders at once per command; `board-render` has its own limit too.
const PARALLEL: usize = 4;

pub const THEMES: [&str; 2] = ["light", "dark"];

/// A check or capture: the commit it rendered and one result per target and theme.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rendered {
    pub commit: String,
    pub results: Vec<RenderResult>,
}

/// The result of checking or capturing one target in one theme.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderResult {
    pub kind: String,
    pub target: String,
    pub theme: String,
    /// `true`, `error`, or absent when the page never got ready.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ready: Option<String>,
    pub issues: Vec<String>,
    /// For captures: false when the stored capture was already at this commit.
    pub captured: bool,
}

/// A stored capture and whether `main` has moved past it in a way that can change it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureStatus {
    #[serde(flatten)]
    pub capture: Capture,
    pub stale: bool,
}

/// What to render: a screen or a story at its size.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Target {
    kind: &'static str,
    id: String,
    width: u32,
    height: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    #[serde(rename = "manifestVersion")]
    version: u32,
    screens: Vec<ManifestItem>,
    #[serde(default)]
    stories: Vec<ManifestItem>,
}

#[derive(Deserialize)]
struct ManifestItem {
    id: String,
    size: Size,
}

#[derive(Deserialize)]
struct Size {
    w: u32,
    h: u32,
}

impl Engine {
    /// Opens targets (screen or story ids; empty = all) in `themes` and reports problems.
    pub fn check(
        &self,
        actor: &Actor,
        key: &str,
        rev: &str,
        requested: &[String],
        themes: &[String],
    ) -> Result<Rendered, EngineError> {
        self.visible(actor, key, Command::ProjectView)?;
        let (commit, settings) = self.ready_build(actor, key, rev)?;
        let jobs = self.jobs(key, &commit, requested, themes)?;
        let results = Self::run_parallel(&jobs, |(target, theme)| {
            let shot = self.shoot(settings, key, &commit, target, theme, None)?;
            Ok(result(target, theme, &shot, false))
        });
        Ok(Rendered { commit, results })
    }

    /// Captures targets (empty = all) in `themes`, skipping those already captured at this commit
    /// unless `force`.
    pub fn capture(
        &self,
        actor: &Actor,
        key: &str,
        rev: &str,
        requested: &[String],
        themes: &[String],
        force: bool,
    ) -> Result<Rendered, EngineError> {
        self.visible(actor, key, Command::BranchEdit)?;
        let (commit, settings) = self.ready_build(actor, key, rev)?;
        let jobs = self.jobs(key, &commit, requested, themes)?;
        let stored: HashMap<(String, String, String), Capture> = self
            .ports
            .captures
            .list(key)?
            .into_iter()
            .map(|c| ((c.kind.clone(), c.target.clone(), c.theme.clone()), c))
            .collect();
        let results = Self::run_parallel(&jobs, |(target, theme)| {
            let id = (
                target.kind.to_owned(),
                target.id.clone(),
                (*theme).to_owned(),
            );
            if let Some(old) = stored.get(&id).filter(|c| c.commit == commit && !force) {
                return Ok(RenderResult {
                    kind: old.kind.clone(),
                    target: old.target.clone(),
                    theme: old.theme.clone(),
                    ready: Some("true".into()),
                    issues: old.issues.clone(),
                    captured: false,
                });
            }
            let shot = self.shoot(settings, key, &commit, target, theme, Some(ImageKind::Jpeg))?;
            let Some(image) = &shot.image else {
                // Nothing to show; keep the previous capture and report why.
                return Ok(result(target, theme, &shot, false));
            };
            let elements = shot
                .elements
                .clone()
                .unwrap_or(serde_json::Value::Array(Vec::new()));
            let capture = Capture {
                kind: target.kind.to_owned(),
                target: target.id.clone(),
                theme: (*theme).to_owned(),
                commit: commit.clone(),
                width: target.width,
                height: target.height,
                issues: shot.issues.clone(),
                sources: sources(&elements),
                captured_at: String::new(),
            };
            self.ports.captures.put(key, &capture, image, &elements)?;
            self.reanchor_comments(key, target.kind, &target.id, theme, &elements)?;
            Ok(result(target, theme, &shot, true))
        });
        let captured: Vec<_> = results
            .iter()
            .filter(|r| r.captured)
            .map(|r| serde_json::json!({ "kind": r.kind, "target": r.target, "theme": r.theme }))
            .collect();
        if !captured.is_empty() {
            self.publish(
                key,
                "capture.updated",
                serde_json::json!({ "captures": captured }),
            );
        }
        Ok(Rendered { commit, results })
    }

    /// Stored captures, each marked stale when `main` moved in a way that can change it.
    pub fn captures(&self, actor: &Actor, key: &str) -> Result<Vec<CaptureStatus>, EngineError> {
        self.visible(actor, key, Command::ProjectView)?;
        let captures = self.ports.captures.list(key)?;
        let Some(head) = self.ports.store.head(key)? else {
            return Ok(Vec::new());
        };
        let app = app_dir(self.ports.store.read(key, &head, "board.json")?.as_deref());
        let known: BTreeSet<String> = captures
            .iter()
            .flat_map(|c| c.sources.iter().map(|s| join(&app, s)))
            .collect();
        let mut changed: HashMap<String, Vec<String>> = HashMap::new();
        let mut out = Vec::with_capacity(captures.len());
        for capture in captures {
            let stale = if capture.commit == head {
                false
            } else {
                if !changed.contains_key(&capture.commit) {
                    let paths = self
                        .ports
                        .store
                        .diff(key, &capture.commit, &head)?
                        .into_iter()
                        .map(|c| c.path)
                        .collect();
                    changed.insert(capture.commit.clone(), paths);
                }
                let own: BTreeSet<String> = capture.sources.iter().map(|s| join(&app, s)).collect();
                changed[&capture.commit]
                    .iter()
                    .any(|path| own.contains(path) || !known.contains(path))
            };
            out.push(CaptureStatus { capture, stale });
        }
        Ok(out)
    }

    /// The stored image of a capture (JPEG).
    pub fn capture_image(
        &self,
        actor: &Actor,
        key: &str,
        kind: &str,
        target: &str,
        theme: &str,
    ) -> Result<Vec<u8>, EngineError> {
        self.visible(actor, key, Command::ProjectView)?;
        self.ports
            .captures
            .image(key, kind, target, theme)?
            .ok_or(EngineError::NotFound("capture"))
    }

    /// The element map of a capture.
    pub fn capture_elements(
        &self,
        actor: &Actor,
        key: &str,
        kind: &str,
        target: &str,
        theme: &str,
    ) -> Result<serde_json::Value, EngineError> {
        self.visible(actor, key, Command::ProjectView)?;
        self.ports
            .captures
            .elements(key, kind, target, theme)?
            .ok_or(EngineError::NotFound("capture"))
    }

    /// The commit of `rev` with a ready build, or why there is none yet.
    fn ready_build(
        &self,
        actor: &Actor,
        key: &str,
        rev: &str,
    ) -> Result<(String, &PreviewSettings), EngineError> {
        let preview = self.preview(actor, key, rev, false)?;
        match preview.build.state {
            BuildState::Ready => Ok((preview.commit, self.preview_settings()?)),
            BuildState::Failed => Err(EngineError::Invalid(format!(
                "the preview build failed: {}",
                preview.build.error
            ))),
            BuildState::Queued | BuildState::Running => Err(EngineError::Pending(
                "the preview is still building; try again shortly".into(),
            )),
        }
    }

    /// Every (target, theme) pair to render.
    fn jobs(
        &self,
        key: &str,
        commit: &str,
        requested: &[String],
        themes: &[String],
    ) -> Result<Vec<(Target, &'static str)>, EngineError> {
        let themes: Vec<&'static str> = if themes.is_empty() {
            vec![THEMES[0]]
        } else {
            themes
                .iter()
                .map(|t| {
                    THEMES
                        .iter()
                        .find(|known| *known == t)
                        .copied()
                        .ok_or_else(|| EngineError::Invalid(format!("unknown theme {t:?}")))
                })
                .collect::<Result<_, _>>()?
        };
        let all = self.targets(key, commit)?;
        let targets: Vec<Target> = if requested.is_empty() {
            all
        } else {
            requested
                .iter()
                .map(|id| {
                    let matches: Vec<&Target> = all.iter().filter(|t| &t.id == id).collect();
                    match matches.as_slice() {
                        [one] => Ok((*one).clone()),
                        [] => Err(EngineError::Invalid(format!("no screen or story {id:?}"))),
                        _ => Err(EngineError::Invalid(format!(
                            "{id:?} is both a screen and a story"
                        ))),
                    }
                })
                .collect::<Result<_, _>>()?
        };
        Ok(targets
            .into_iter()
            .flat_map(|t| themes.iter().map(move |theme| (t.clone(), *theme)))
            .collect())
    }

    /// Screens and stories of a build, from its `.board/manifest.json`.
    fn targets(&self, key: &str, commit: &str) -> Result<Vec<Target>, EngineError> {
        let file = self
            .ports
            .workspace
            .file(key, commit, ".board/manifest.json")?
            .ok_or_else(|| EngineError::Invalid("the build has no .board/manifest.json".into()))?;
        let manifest: Manifest = serde_json::from_slice(&file.body)
            .map_err(|e| EngineError::Invalid(format!("bad .board/manifest.json: {e}")))?;
        if manifest.version != 1 {
            return Err(EngineError::Invalid(format!(
                "manifest version {} needs a newer board",
                manifest.version
            )));
        }
        let item = |kind: &'static str| {
            move |i: ManifestItem| Target {
                kind,
                id: i.id,
                width: i.size.w,
                height: i.size.h,
            }
        };
        Ok(manifest
            .screens
            .into_iter()
            .map(item("screen"))
            .chain(manifest.stories.into_iter().map(item("story")))
            .collect())
    }

    fn shoot(
        &self,
        settings: &PreviewSettings,
        key: &str,
        commit: &str,
        target: &Target,
        theme: &str,
        image: Option<ImageKind>,
    ) -> Result<Shot, EngineError> {
        let token = token(settings, key, commit);
        let url = format!(
            "{}{}.board/frame.html?{}={}&theme={theme}&t={token}",
            settings.render_origin,
            base_path(key, commit),
            target.kind,
            encode(&target.id),
        );
        Ok(self.ports.renderer.render(&RenderJob {
            url,
            width: target.width,
            height: target.height,
            image,
            map: image.is_some(),
        })?)
    }

    /// Runs `work` over `jobs` on a few threads, keeping the input order. A failing job becomes a
    /// result carrying the error, so one bad screen never hides the others.
    fn run_parallel<F>(jobs: &[(Target, &'static str)], work: F) -> Vec<RenderResult>
    where
        F: Fn(&(Target, &'static str)) -> Result<RenderResult, EngineError> + Sync,
    {
        let results: Mutex<Vec<Option<RenderResult>>> = Mutex::new(vec![None; jobs.len()]);
        let next = Mutex::new(0usize);
        std::thread::scope(|scope| {
            for _ in 0..PARALLEL.min(jobs.len()) {
                scope.spawn(|| {
                    loop {
                        let index = {
                            let mut next = next
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            let index = *next;
                            *next += 1;
                            index
                        };
                        let Some(job) = jobs.get(index) else { break };
                        let outcome = work(job).unwrap_or_else(|error| RenderResult {
                            kind: job.0.kind.to_owned(),
                            target: job.0.id.clone(),
                            theme: job.1.to_owned(),
                            ready: None,
                            issues: vec![error.to_string()],
                            captured: false,
                        });
                        results
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)[index] =
                            Some(outcome);
                    }
                });
            }
        });
        results
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .into_iter()
            .flatten()
            .collect()
    }
}

fn result(target: &Target, theme: &str, shot: &Shot, captured: bool) -> RenderResult {
    let mut issues = shot.issues.clone();
    if shot.ready.as_deref() == Some("error") && issues.is_empty() {
        issues.push("the frame reported an error".into());
    }
    RenderResult {
        kind: target.kind.to_owned(),
        target: target.id.clone(),
        theme: theme.to_owned(),
        ready: shot.ready.clone(),
        issues,
        captured,
    }
}

/// Source files from an element map's `src` (`path:line:col`), sorted, without duplicates.
fn sources(elements: &serde_json::Value) -> Vec<String> {
    let files: BTreeSet<String> = elements
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| e.get("src")?.as_str())
        .filter_map(|src| {
            let mut parts = src.rsplitn(3, ':');
            let (_, _, path) = (parts.next()?, parts.next()?, parts.next()?);
            Some(path.to_owned())
        })
        .collect();
    files.into_iter().collect()
}

/// The app directory from `board.json` (`app`), `""` for the project root.
fn app_dir(board_json: Option<&[u8]>) -> String {
    board_json
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(bytes).ok())
        .and_then(|v| v.get("app")?.as_str().map(str::to_owned))
        .map(|app| app.trim_matches('/').to_owned())
        .filter(|app| app != ".")
        .unwrap_or_default()
}

fn join(app: &str, path: &str) -> String {
    if app.is_empty() {
        path.to_owned()
    } else {
        format!("{app}/{path}")
    }
}

/// Percent-encodes a query value.
fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BuildState;
    use crate::testing::{Fakes, engine_with_fakes, file};
    use board_model::Actor;

    const MANIFEST: &str = r#"{"manifestVersion":1,"title":"T","pages":[],
        "screens":[{"id":"list","page":"p","title":"L","note":"","route":"/","size":{"w":800,"h":600},"hasSetup":false},
                   {"id":"empty","page":"p","title":"E","note":"","route":"/","size":{"w":800,"h":600},"hasSetup":false}],
        "connections":[],"flows":[],
        "stories":[{"id":"button","component":"Button","title":"B","size":{"w":200,"h":100}}]}"#;

    fn setup() -> (Engine, Fakes) {
        let (engine, fakes) = engine_with_fakes();
        let import = crate::Import {
            files: vec![
                file("board.json", r#"{"app":"web"}"#),
                file("web/src/list.tsx", "list"),
                file("web/src/empty.tsx", "empty"),
                file("web/src/api.ts", "api"),
            ],
            source: "test".into(),
        };
        engine
            .create_project(&Actor::Local, "shop", "Shop", Some(import))
            .unwrap();
        engine
            .preview(&Actor::Local, "shop", "main", false)
            .unwrap();
        fakes.workspace.finish(BuildState::Ready);
        fakes.workspace.serve(".board/manifest.json", MANIFEST);
        (engine, fakes)
    }

    fn ids(results: &[RenderResult]) -> Vec<String> {
        results
            .iter()
            .map(|r| format!("{}:{}:{}:{}", r.kind, r.target, r.theme, r.captured))
            .collect()
    }

    #[test]
    fn check_renders_without_storing() {
        let (engine, fakes) = setup();
        let results = engine
            .check(
                &Actor::Local,
                "shop",
                "main",
                &[],
                &["light".into(), "dark".into()],
            )
            .unwrap();
        assert_eq!(
            ids(&results.results),
            [
                "screen:list:light:false",
                "screen:list:dark:false",
                "screen:empty:light:false",
                "screen:empty:dark:false",
                "story:button:light:false",
                "story:button:dark:false",
            ]
        );
        let jobs = fakes.renderer.jobs();
        assert!(jobs.iter().all(|j| j.image.is_none()));
        let story = jobs
            .iter()
            .find(|j| j.url.contains("story=button") && j.url.contains("theme=light"))
            .unwrap();
        assert_eq!((story.width, story.height), (200, 100));
        assert!(story.url.starts_with("http://render.internal/shop/"));
        assert!(story.url.contains("&theme=light&t="));
        assert_eq!(engine.captures(&Actor::Local, "shop").unwrap(), []);
    }

    #[test]
    fn capture_stores_once_per_commit_and_tracks_staleness() {
        let (engine, fakes) = setup();
        let targets = ["list".to_owned(), "empty".to_owned()];
        let first = engine
            .capture(&Actor::Local, "shop", "main", &targets, &[], false)
            .unwrap();
        assert_eq!(
            ids(&first.results),
            ["screen:list:light:true", "screen:empty:light:true"]
        );
        let again = engine
            .capture(&Actor::Local, "shop", "main", &targets, &[], false)
            .unwrap();
        assert_eq!(
            ids(&again.results),
            ["screen:list:light:false", "screen:empty:light:false"]
        );
        assert_eq!(
            fakes.renderer.jobs().len(),
            2,
            "same commit: no new renders"
        );

        let image = engine
            .capture_image(&Actor::Local, "shop", "screen", "list", "light")
            .unwrap();
        assert_eq!(image, b"jpeg:list");
        let map = engine
            .capture_elements(&Actor::Local, "shop", "screen", "list", "light")
            .unwrap();
        assert_eq!(map[0]["src"], "src/list.tsx:3:5");

        let stale = |engine: &Engine| -> Vec<(String, bool)> {
            engine
                .captures(&Actor::Local, "shop")
                .unwrap()
                .into_iter()
                .map(|c| (c.capture.target, c.stale))
                .collect()
        };
        assert_eq!(
            stale(&engine),
            [("empty".into(), false), ("list".into(), false)]
        );

        // Only list renders from list.tsx.
        let base = engine.ports.store.head("shop").unwrap().unwrap();
        engine
            .push(
                &Actor::Local,
                "shop",
                &base,
                "list",
                &[file("web/src/list.tsx", "v2")],
            )
            .unwrap();
        assert_eq!(
            stale(&engine),
            [("empty".into(), false), ("list".into(), true)]
        );

        // api.ts renders nothing visible on its own: its reach is unknown, so everything is stale.
        let base = engine.ports.store.head("shop").unwrap().unwrap();
        engine
            .push(
                &Actor::Local,
                "shop",
                &base,
                "api",
                &[file("web/src/api.ts", "v2")],
            )
            .unwrap();
        assert_eq!(
            stale(&engine),
            [("empty".into(), true), ("list".into(), true)]
        );
    }

    #[test]
    fn bad_requests_and_unready_builds() {
        let (engine, fakes) = setup();
        let err = engine
            .check(&Actor::Local, "shop", "main", &["ghost".into()], &[])
            .unwrap_err();
        assert_eq!(
            err,
            EngineError::Invalid("no screen or story \"ghost\"".into())
        );
        let err = engine
            .check(&Actor::Local, "shop", "main", &[], &["sepia".into()])
            .unwrap_err();
        assert_eq!(err, EngineError::Invalid("unknown theme \"sepia\"".into()));
        fakes.workspace.finish(BuildState::Running);
        assert!(matches!(
            engine.check(&Actor::Local, "shop", "main", &[], &[]),
            Err(EngineError::Pending(_))
        ));
    }

    #[test]
    fn sources_and_paths() {
        let elements = serde_json::json!([
            {"src": "src/a.tsx:1:2"}, {"src": "src/a.tsx:9:9"}, {"src": "src/we:ird.tsx:3:4"}, {"tag": "div"}
        ]);
        assert_eq!(sources(&elements), ["src/a.tsx", "src/we:ird.tsx"]);
        assert_eq!(app_dir(Some(br#"{"app":"apps/web/"}"#)), "apps/web");
        assert_eq!(app_dir(Some(br#"{"app":"."}"#)), "");
        assert_eq!(app_dir(None), "");
        assert_eq!(encode("a b&c"), "a%20b%26c");
    }
}
