//! Read-only views of a revision for the web tool: its manifest (pages, screens, connections,
//! flows, stories) and its source files.

use board_model::{Actor, valid_path};

use crate::policy::Command;
use crate::{BuildState, Engine, EngineError};

/// Largest file the code view returns.
const MAX_FILE: usize = 2 * 1024 * 1024;

impl Engine {
    /// The `.board/manifest.json` of `rev`'s build. Starts the build when there is none.
    pub fn manifest(
        &self,
        actor: &Actor,
        key: &str,
        rev: &str,
    ) -> Result<(String, serde_json::Value), EngineError> {
        let preview = self.preview(actor, key, rev, false)?;
        match preview.build.state {
            BuildState::Ready => {}
            BuildState::Failed => {
                return Err(EngineError::Invalid(format!(
                    "the preview build failed: {}",
                    preview.build.error
                )));
            }
            BuildState::Queued | BuildState::Running => {
                return Err(EngineError::Pending("the preview is still building".into()));
            }
        }
        let file = self
            .ports
            .workspace
            .file(key, &preview.commit, ".board/manifest.json")?
            .ok_or_else(|| EngineError::Invalid("the build has no .board/manifest.json".into()))?;
        let manifest = serde_json::from_slice(&file.body)
            .map_err(|e| EngineError::Invalid(format!("bad .board/manifest.json: {e}")))?;
        Ok((preview.commit, manifest))
    }

    /// Every file path of `rev`.
    pub fn files(
        &self,
        actor: &Actor,
        key: &str,
        rev: &str,
    ) -> Result<(String, Vec<String>), EngineError> {
        self.visible(actor, key, Command::ProjectView)?;
        let commit = self.resolve(key, rev)?;
        let paths = self.ports.store.list(key, &commit)?;
        Ok((commit, paths))
    }

    /// One file of `rev`.
    pub fn file(
        &self,
        actor: &Actor,
        key: &str,
        rev: &str,
        path: &str,
    ) -> Result<(String, Vec<u8>), EngineError> {
        self.visible(actor, key, Command::ProjectView)?;
        if !valid_path(path) {
            return Err(EngineError::Invalid(format!("bad path {path:?}")));
        }
        let commit = self.resolve(key, rev)?;
        let bytes = self
            .ports
            .store
            .read(key, &commit, path)?
            .ok_or(EngineError::NotFound("file"))?;
        if bytes.len() > MAX_FILE {
            return Err(EngineError::Invalid(format!(
                "{path} is larger than {} MB",
                MAX_FILE / 1024 / 1024
            )));
        }
        Ok((commit, bytes))
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{engine_with_fakes, file};
    use crate::{BuildState, EngineError};
    use board_model::Actor;

    #[test]
    fn manifest_and_files() {
        let (engine, fakes) = engine_with_fakes();
        let import = crate::Import {
            files: vec![file("src/a.tsx", "export {}"), file("board.json", "{}")],
            source: "t".into(),
        };
        engine
            .create_project(&Actor::Local, "shop", "Shop", Some(import))
            .unwrap();
        assert!(matches!(
            engine.manifest(&Actor::Local, "shop", "main"),
            Err(EngineError::Pending(_))
        ));
        fakes.workspace.finish(BuildState::Ready);
        fakes.workspace.serve(
            ".board/manifest.json",
            r#"{"manifestVersion":1,"screens":[]}"#,
        );
        let (_, manifest) = engine.manifest(&Actor::Local, "shop", "main").unwrap();
        assert_eq!(manifest["manifestVersion"], 1);

        let (commit, paths) = engine.files(&Actor::Local, "shop", "main").unwrap();
        assert_eq!(paths, ["board.json", "src/a.tsx"]);
        let (same, bytes) = engine
            .file(&Actor::Local, "shop", "main", "src/a.tsx")
            .unwrap();
        assert_eq!((same, bytes), (commit, b"export {}".to_vec()));
        assert_eq!(
            engine.file(&Actor::Local, "shop", "main", "nope.ts"),
            Err(EngineError::NotFound("file"))
        );
        assert!(matches!(
            engine.file(&Actor::Local, "shop", "main", "../etc/passwd"),
            Err(EngineError::Invalid(_))
        ));
    }
}
