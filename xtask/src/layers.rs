//! Dependency layering (docs/architecture.md): a crate may depend only on workspace crates in
//! lower layers, and only the UI layer may use a UI toolkit or GPU crate.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

/// Where a workspace crate sits. `None` is exempt (apps and tooling).
pub const TABLE: &[(&str, Option<u8>)] = &[
    ("geom", Some(0)),
    ("render", Some(1)),
    ("model", Some(2)),
    ("io", Some(3)),
    ("engine", Some(4)),
    ("gpu", Some(5)),
    ("mcp", Some(5)),
    ("ui-egui", Some(6)),
    // apps and tooling
    ("structurecraft", None),
    ("cli", None),
    ("web", None),
    ("xtask", None),
];

/// External crates that are a UI toolkit, window or GPU dependency.
pub const UI_CRATES: &[&str] = &["egui", "eframe", "egui-wgpu", "wgpu", "winit", "rfd"];

/// First layer allowed to use UI crates.
pub const UI_MIN_LAYER: u8 = 5;

pub fn short_name(pkg: &str) -> &str {
    pkg.strip_prefix("structurecraft-").unwrap_or(pkg)
}

fn layer_of(pkg: &str) -> Result<Option<u8>, String> {
    let s = short_name(pkg);
    TABLE.iter().find(|(n, _)| *n == s).map(|(_, l)| *l).ok_or_else(|| format!("`{pkg}` is not in the layer table (xtask/src/layers.rs)"))
}

/// A workspace crate and its normal and build dependencies: `(name, is_workspace_member)`.
pub struct Crate {
    pub name: String,
    pub deps: Vec<(String, bool)>,
}

pub fn violations(crates: &[Crate]) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for c in crates {
        let Some(layer) = layer_of(&c.name)? else { continue };
        for (dep, workspace) in &c.deps {
            if *workspace {
                match layer_of(dep)? {
                    None => out.push(format!("{} (L{layer}) depends on the app or tool `{dep}`", c.name)),
                    Some(l) if l >= layer => out.push(format!("{} (L{layer}) depends on {dep} (L{l}): only lower layers", c.name)),
                    Some(_) => {}
                }
            } else if layer < UI_MIN_LAYER && UI_CRATES.contains(&dep.as_str()) {
                out.push(format!("{} (L{layer}) uses the UI crate `{dep}`: only L{UI_MIN_LAYER} and the apps may", c.name));
            }
        }
    }
    Ok(out)
}

fn from_metadata(root: &Path) -> Result<Vec<Crate>, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let out = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("cannot run cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    let meta: Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let packages = meta["packages"].as_array().ok_or("cargo metadata has no packages")?;
    let members: Vec<&str> = packages.iter().filter_map(|p| p["name"].as_str()).collect();
    Ok(packages
        .iter()
        .map(|p| Crate {
            name: p["name"].as_str().unwrap_or_default().to_owned(),
            deps: p["dependencies"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|d| d["kind"].as_str() != Some("dev"))
                .filter_map(|d| d["name"].as_str())
                .map(|n| (n.to_owned(), members.contains(&n)))
                .collect(),
        })
        .collect())
}

pub fn run(root: &Path) -> Result<(), String> {
    let v = violations(&from_metadata(root)?)?;
    if v.is_empty() {
        eprintln!("layers: ok");
        Ok(())
    } else {
        Err(format!("layering violations:\n  {}", v.join("\n  ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn krate(name: &str, deps: &[(&str, bool)]) -> Crate {
        Crate { name: name.to_owned(), deps: deps.iter().map(|(n, w)| ((*n).to_owned(), *w)).collect() }
    }

    #[test]
    fn upward_and_sideways_dependencies_are_violations() {
        let crates = [
            krate("structurecraft-model", &[("structurecraft-geom", true)]),
            krate("structurecraft-geom", &[("structurecraft-model", true)]),
            krate("structurecraft-mcp", &[("structurecraft-ui-egui", true)]),
            krate("structurecraft-render", &[("wgpu", false)]),
            krate("structurecraft", &[("structurecraft-ui-egui", true), ("eframe", false)]),
        ];
        assert_eq!(violations(&crates).unwrap().len(), 3);
    }

    #[test]
    fn unknown_crates_must_be_classified() {
        assert!(violations(&[krate("structurecraft-new", &[])]).is_err());
    }
}
