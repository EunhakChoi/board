//! Repository tasks.
//!
//! - `layers`  a crate may depend only on board crates in lower layers

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Layer of every board crate. A crate may depend only on crates in strictly lower layers.
const LAYERS: &[(&str, u8)] = &[
    ("board-model", 0),
    ("board-engine", 1),
    ("board-store", 2),
    ("board-db", 2),
    ("board-collab", 2),
    ("board-api", 3),
    ("board-server", 4),
];

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() != Some("layers") {
        eprintln!("usage: cargo xtask layers");
        return ExitCode::FAILURE;
    }
    match layers(&root()) {
        Ok(count) => {
            println!("layers: {count} crates ok");
            ExitCode::SUCCESS
        }
        Err(problems) => {
            for problem in problems {
                eprintln!("layers: {problem}");
            }
            ExitCode::FAILURE
        }
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// Checks every board crate under `crates/` and `apps/`. Returns the number of crates checked.
fn layers(root: &Path) -> Result<usize, Vec<String>> {
    let layer: BTreeMap<&str, u8> = LAYERS.iter().copied().collect();
    let mut problems = Vec::new();
    let mut count = 0;
    for manifest in manifests(root) {
        let Ok(text) = std::fs::read_to_string(&manifest) else {
            problems.push(format!("cannot read {}", manifest.display()));
            continue;
        };
        let Some((name, deps)) = parse(&text) else {
            problems.push(format!("cannot parse {}", manifest.display()));
            continue;
        };
        if !name.starts_with("board-") {
            continue;
        }
        count += 1;
        let Some(&own) = layer.get(name.as_str()) else {
            problems.push(format!("{name} has no layer; add it to LAYERS in xtask"));
            continue;
        };
        for dep in deps.iter().filter(|dep| dep.starts_with("board-")) {
            match layer.get(dep.as_str()) {
                Some(&lower) if lower < own => {}
                Some(&other) => problems.push(format!(
                    "{name} (L{own}) depends on {dep} (L{other}); only lower layers are allowed"
                )),
                None => problems.push(format!("{name} depends on {dep}, which has no layer")),
            }
        }
    }
    if problems.is_empty() {
        Ok(count)
    } else {
        Err(problems)
    }
}

fn manifests(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for dir in ["crates", "apps"] {
        let Ok(entries) = std::fs::read_dir(root.join(dir)) else {
            continue;
        };
        for entry in entries.flatten() {
            let manifest = entry.path().join("Cargo.toml");
            if manifest.is_file() {
                found.push(manifest);
            }
        }
    }
    found.sort();
    found
}

/// Returns the package name and the names of its normal and build dependencies.
fn parse(text: &str) -> Option<(String, Vec<String>)> {
    let doc: toml::Table = text.parse().ok()?;
    let name = doc.get("package")?.get("name")?.as_str()?.to_owned();
    let deps = ["dependencies", "build-dependencies"]
        .iter()
        .filter_map(|section| doc.get(*section).and_then(toml::Value::as_table))
        .flat_map(|table| table.keys().cloned())
        .collect();
    Some((name, deps))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(name: &str, deps: &[&str]) -> String {
        let mut text = format!("[package]\nname = \"{name}\"\n\n[dependencies]\n");
        for dep in deps {
            text.push_str(dep);
            text.push_str(" = { path = \"x\" }\n");
        }
        text
    }

    fn check(case: &str, crates: &[(&str, &[&str])]) -> Result<usize, Vec<String>> {
        let dir = std::env::temp_dir().join(format!("xtask-layers-{}-{case}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (name, deps) in crates {
            let crate_dir = dir.join("crates").join(name);
            std::fs::create_dir_all(&crate_dir).unwrap();
            std::fs::write(crate_dir.join("Cargo.toml"), manifest(name, deps)).unwrap();
        }
        let result = layers(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        result
    }

    #[test]
    fn lower_layers_are_allowed() {
        assert_eq!(
            check(
                "lower",
                &[
                    ("board-model", &[]),
                    ("board-engine", &["board-model", "serde"])
                ]
            ),
            Ok(2)
        );
    }

    #[test]
    fn same_or_higher_layers_are_rejected() {
        let problems = check(
            "higher",
            &[
                ("board-model", &["board-engine"]),
                ("board-store", &["board-db"]),
            ],
        )
        .unwrap_err();
        assert_eq!(problems.len(), 2);
        assert!(problems[0].contains("board-model (L0) depends on board-engine (L1)"));
    }

    #[test]
    fn unknown_crates_are_rejected() {
        let problems = check("unknown", &[("board-extra", &[])]).unwrap_err();
        assert!(problems[0].contains("has no layer"));
    }

    #[test]
    fn this_repository_is_layered() {
        assert!(layers(&root()).is_ok());
    }
}
