// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Where *this* app's teksilo actually lives, and which version it is.
//!
//! Every other command depends on this answer, so it is worth stating how it
//! is obtained: by asking cargo for the **resolved dependency graph**
//! (`cargo metadata`), never by parsing a manifest.
//!
//! That is not a style preference. The prototype this module generalises read
//! `[dependencies].teksilo` out of a crate manifest and understood a bare
//! string or a `{ version = "…" }` table — which stopped working the day the
//! app switched to `teksilo = { workspace = true, features = [...] }`, because
//! the real pin then lived in `[workspace.dependencies]` of a *different*
//! file. The function returned `None`, the version check downstream of it
//! read `if not want: return`, and the entire "refuse a stale client"
//! protection went silently dead while still being described at length in its
//! own docstrings. Nothing noticed, because a check that does nothing passes.
//!
//! `cargo metadata` has already done the resolution — workspace inheritance,
//! patches, renames, path-vs-registry-vs-git — so there is no manifest shape
//! left to mis-parse.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

/// One resolved teksilo-family package in the consumer's graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCrate {
    /// Cargo package name, e.g. `teksilo-widgets`.
    pub name: String,
    /// The version cargo resolved, e.g. `0.12.1`.
    pub version: String,
    /// Directory containing the package's `Cargo.toml`.
    pub dir: PathBuf,
}

impl ResolvedCrate {
    /// The package's `src/` directory, which may not exist before a fetch.
    pub fn src(&self) -> PathBuf {
        self.dir.join("src")
    }

    /// Whether the package's sources are actually on disk.
    ///
    /// A freshly-cloned, never-built project resolves versions perfectly well
    /// and has no source to read.
    pub fn has_src(&self) -> bool {
        self.src().is_dir()
    }
}

/// Everything the commands need to know about the consumer's teksilo.
#[derive(Debug, Clone)]
pub struct Resolution {
    /// Every `teksilo*` package in the graph, keyed by package name.
    pub crates: BTreeMap<String, ResolvedCrate>,
    /// The framework version this app resolved.
    pub version: String,
    /// `Some` when teksilo is an optional dependency that the app's default
    /// features leave off, holding the app's own features that turn it on.
    ///
    /// The list can be empty: teksilo may come in through an optional
    /// dependency of a dependency, which no feature of the app names directly.
    pub enabled_by: Option<Vec<String>>,
}

impl Resolution {
    /// Look up one package by its cargo name.
    pub fn get(&self, name: &str) -> Option<&ResolvedCrate> {
        self.crates.get(name)
    }

    /// The `teksilo-widgets` package, which `symbol` needs to do anything useful.
    pub fn widgets(&self) -> Option<&ResolvedCrate> {
        self.get("teksilo-widgets")
    }

    /// A teksilo checkout reachable from a resolved crate, if there is one.
    ///
    /// A path dependency inside the framework repo puts `crates/<name>/` two
    /// levels below a root that ships `tools/`. That checkout's own extractor
    /// is authoritative — it is version-matched by construction and richer
    /// than any vendored copy — so callers prefer it when it exists.
    pub fn checkout_root(&self) -> Option<PathBuf> {
        let widgets = self.widgets()?;
        let root = widgets.dir.parent()?.parent()?;
        let has_tool = root.join("tools").join("extract_widget_api.py").is_file();
        let has_crates = root.join("crates").join("teksilo-widgets").is_dir();
        (has_tool && has_crates).then(|| root.to_path_buf())
    }
}

/// Why resolution failed, in terms the caller can act on.
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("`cargo metadata` failed — run this from inside your app's crate or workspace.\n{0}")]
    Metadata(String),

    #[error("could not parse `cargo metadata` output: {0}")]
    Parse(#[from] serde_json::Error),

    #[error("could not run cargo: {0}")]
    Spawn(#[from] std::io::Error),

    #[error(
        "`teksilo` is not in this app's dependency graph, even with every feature enabled.\n\
         Add it with `cargo add teksilo`, or run this from inside the app's crate or workspace."
    )]
    NotADependency,

    #[error("there is no Cargo.lock yet, and this command does not create one")]
    NoLockfile,

    #[error(
        "multiple resolved packages named {0}; run from a single app and remove ambiguous framework dependencies"
    )]
    Ambiguous(String),

    #[error("teksilo source is not on disk at {0}.\nRun `cargo fetch` in your app and try again.")]
    SourceMissing(PathBuf),
}

/// Ask cargo where this app's teksilo is, from `dir`.
///
/// `dir` is normally the current directory; it is a parameter so tests can
/// point at a fixture app without changing the process's working directory.
pub fn resolve(dir: &Path) -> Result<Resolution, ResolveError> {
    resolve_features(dir, false)
}

/// [`resolve`], guaranteed not to write.
///
/// `cargo metadata` **resolves**, and resolving writes: with no `Cargo.lock`
/// it creates one, and with a stale one it rewrites it. That is right for
/// `symbol` and `search`, which are about to answer a question that needs a
/// resolved graph — but `status` promises to report without touching
/// anything, and a command that silently created a lockfile in someone's
/// repository would be breaking its own headline claim.
///
/// `--locked` is the whole fix: cargo refuses rather than writes. A project
/// with no lockfile therefore gets an honest "unknown" instead of a lockfile
/// it did not ask for.
pub fn resolve_locked(dir: &Path) -> Result<Resolution, ResolveError> {
    resolve_features(dir, true)
}

/// Resolve with the default features, then with all of them if teksilo is
/// missing.
///
/// `cargo metadata` resolves the graph for the default features, so an app
/// that keeps teksilo behind a feature — the usual way to grow a second UI
/// beside an existing one — has no teksilo in that graph, and every command
/// would refuse with "not a dependency". `--all-features` brings it back without
/// changing the version: the lockfile pins versions for every feature at
/// once. It is the fallback, not the first call, because the default
/// features are what the app builds: an app that chooses between two
/// teksilos through mutually exclusive features resolves to one only that
/// way, where all features together would be ambiguous.
fn resolve_features(dir: &Path, locked: bool) -> Result<Resolution, ResolveError> {
    let metadata = run_cargo_metadata(dir, locked, false)?;
    match resolution_from_metadata(&metadata) {
        Err(ResolveError::NotADependency) => {}
        resolved => return resolved,
    }
    let metadata = run_cargo_metadata(dir, locked, true)?;
    let mut resolution = resolution_from_metadata(&metadata)?;
    resolution.enabled_by = Some(enabling_features(&metadata));
    Ok(resolution)
}

fn run_cargo_metadata(
    dir: &Path,
    locked: bool,
    all_features: bool,
) -> Result<serde_json::Value, ResolveError> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut args = vec!["metadata", "--format-version", "1", "--quiet"];
    if locked {
        args.push("--locked");
    }
    if all_features {
        args.push("--all-features");
    }
    let out = Command::new(&cargo).args(&args).current_dir(dir).output()?;
    if !out.status.success() {
        // cargo reports a missing lockfile and a stale one with the same
        // "needs to be updated" error. Only the first has a remedy as plain
        // as running any other command, so it is told apart here.
        if locked && workspace_lockfile(&cargo, dir).is_some_and(|lock| !lock.exists()) {
            return Err(ResolveError::NoLockfile);
        }
        return Err(ResolveError::Metadata(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }
    Ok(serde_json::from_slice(&out.stdout)?)
}

/// Where the workspace containing `dir` keeps its `Cargo.lock`, which is not
/// `dir` itself for a workspace member.
fn workspace_lockfile(cargo: &std::ffi::OsStr, dir: &Path) -> Option<PathBuf> {
    let out = Command::new(cargo)
        .args(["locate-project", "--workspace", "--message-format", "plain"])
        .current_dir(dir)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let manifest = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    Some(manifest.parent()?.join("Cargo.lock"))
}

/// The features of the selected app that turn on an optional teksilo
/// dependency of its own, sorted.
///
/// Read from the metadata's `features` table rather than from the manifest:
/// cargo has already turned an implicit optional-dependency feature into an
/// explicit `"teksilo": ["dep:teksilo"]` entry there, and resolved renames.
/// A weak `teksilo?/feature` enables nothing on its own and is not counted.
fn enabling_features(meta: &serde_json::Value) -> Vec<String> {
    let roots: Vec<&str> = match meta["resolve"]["root"].as_str() {
        Some(root) => vec![root],
        None => meta["workspace_members"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str())
            .collect(),
    };
    let mut features = BTreeSet::new();
    for pkg in meta["packages"].as_array().into_iter().flatten() {
        if !pkg["id"].as_str().is_some_and(|id| roots.contains(&id)) {
            continue;
        }
        let optional: Vec<&str> = pkg["dependencies"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|dep| dep["optional"].as_bool() == Some(true))
            .filter(|dep| dep["name"].as_str().is_some_and(is_teksilo_package))
            .filter_map(|dep| dep["rename"].as_str().or(dep["name"].as_str()))
            .collect();
        for (feature, enables) in pkg["features"].as_object().into_iter().flatten() {
            let turns_on = |value: &str| {
                optional.iter().any(|dep| {
                    value.strip_prefix("dep:") == Some(*dep)
                        || value
                            .strip_prefix(*dep)
                            .is_some_and(|rest| rest.starts_with('/'))
                })
            };
            if enables
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str())
                .any(turns_on)
            {
                features.insert(feature.clone());
            }
        }
    }
    features.into_iter().collect()
}

/// The pure half: metadata JSON in, resolution out.
///
/// Split from the process call so the interesting logic is testable without a
/// cargo invocation or a fixture app on disk.
pub fn resolution_from_metadata(meta: &serde_json::Value) -> Result<Resolution, ResolveError> {
    let packages = meta
        .get("packages")
        .and_then(|p| p.as_array())
        .ok_or(ResolveError::NotADependency)?;

    // Metadata includes every workspace member, even when invoked in one app.
    // Restrict to the selected root's transitive dependencies; virtual roots
    // cover all members and reject conflicting versions instead of guessing.
    let reachable = meta.get("resolve").map(|graph| {
        let mut pending: Vec<&str> = match graph.get("root").and_then(|v| v.as_str()) {
            Some(root) => vec![root],
            None => meta["workspace_members"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str())
                .collect(),
        };
        let mut seen = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !seen.insert(id) {
                continue;
            }
            if let Some(node) = graph["nodes"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|node| node["id"].as_str() == Some(id))
            {
                pending.extend(
                    node["dependencies"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|v| v.as_str()),
                );
            }
        }
        seen
    });
    let mut crates = BTreeMap::new();
    for pkg in packages {
        if reachable
            .as_ref()
            .is_some_and(|ids| pkg["id"].as_str().is_none_or(|id| !ids.contains(id)))
        {
            continue;
        }
        let Some(name) = pkg.get("name").and_then(|n| n.as_str()) else {
            continue;
        };
        if !is_teksilo_package(name) {
            continue;
        }
        let Some(version) = pkg.get("version").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(manifest) = pkg.get("manifest_path").and_then(|m| m.as_str()) else {
            continue;
        };
        let dir = Path::new(manifest)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        if crates.contains_key(name) {
            return Err(ResolveError::Ambiguous(name.to_string()));
        }
        crates.insert(
            name.to_string(),
            ResolvedCrate {
                name: name.to_string(),
                version: version.to_string(),
                dir,
            },
        );
    }

    if crates.is_empty() {
        return Err(ResolveError::NotADependency);
    }

    let version = framework_version(&crates).ok_or(ResolveError::NotADependency)?;
    Ok(Resolution {
        crates,
        version,
        enabled_by: None,
    })
}

/// The two external siblings whose types a consumer reaches through teksilo.
///
/// `teksilo-text` re-exports `text_document` wholesale — an app writes
/// `teksilo::text_document::TextDocument` — and ~20 `text_typeset` types by
/// name. They are ordinary crates.io dependencies on their own release
/// cadence, so they are not `teksilo-…` and were filtered out of the
/// resolution; the consequence was `symbol TextDocument` reporting that a
/// public, documented type does not exist.
///
/// Admitting them here is enough: everything downstream is already generic
/// over "a resolved crate with sources", and `extract_widget_api.py` carries a
/// `CRATE_SPECS` entry for each.
const EXTERNAL_SIBLINGS: [&str; 2] = ["text-document", "text-typeset"];

/// `teksilo` itself, any `teksilo-…` member, or one of the two
/// [`EXTERNAL_SIBLINGS`] — but not an unrelated package that merely starts
/// with the same letters (`teksilonium`).
fn is_teksilo_package(name: &str) -> bool {
    name == "teksilo" || name.starts_with("teksilo-") || EXTERNAL_SIBLINGS.contains(&name)
}

/// Which package's version *is* "the teksilo version".
///
/// The umbrella crate when present, because that is what an app pins and what
/// `cargo install cargo-teksilo --version X` must match. Failing that, the
/// widget crate; failing that, any member — an app depending only on
/// `teksilo-data` still deserves a correct answer. Every teksilo crate shares
/// one workspace version, so the fallbacks agree whenever they apply.
fn framework_version(crates: &BTreeMap<String, ResolvedCrate>) -> Option<String> {
    for key in ["teksilo", "teksilo-widgets", "teksilo-core"] {
        if let Some(c) = crates.get(key) {
            return Some(c.version.clone());
        }
    }
    crates
        .values()
        .find(|c| c.name.starts_with("teksilo-"))
        .map(|c| c.version.clone())
}

/// Resolve, and make sure all resolved API sources are readable.
///
/// Runs `cargo fetch` once if the sources are absent, which is the ordinary
/// state of a checkout that has never been built.
pub fn resolve_with_sources(dir: &Path) -> Result<Resolution, ResolveError> {
    let resolution = resolve(dir)?;
    if resolution.crates.values().all(ResolvedCrate::has_src) {
        return Ok(resolution);
    }

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let _ = Command::new(cargo).arg("fetch").current_dir(dir).output();

    // Re-resolve rather than trusting the stale path: a fetch can move nothing,
    // but it can also populate a registry directory that did not exist when the
    // first metadata call ran.
    let resolution = resolve(dir)?;
    if let Some(missing) = resolution.crates.values().find(|c| !c.has_src()) {
        return Err(ResolveError::SourceMissing(missing.src()));
    }
    Ok(resolution)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(packages: serde_json::Value) -> serde_json::Value {
        serde_json::json!({ "packages": packages })
    }

    fn pkg(name: &str, version: &str, manifest: &str) -> serde_json::Value {
        serde_json::json!({
            "name": name, "version": version, "manifest_path": manifest
        })
    }

    fn workspace_graph(root: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "packages": [
                {"id":"old", "name":"teksilo", "version":"0.13.0", "manifest_path":"/old/Cargo.toml"},
                {"id":"new", "name":"teksilo", "version":"0.14.2", "manifest_path":"/new/Cargo.toml"}
            ],
            "workspace_members": ["app-old", "app-new"],
            "resolve": {"root":root, "nodes":[
                {"id":"app-old", "dependencies":["old"]},
                {"id":"app-new", "dependencies":["new"]},
                {"id":"old", "dependencies":[]},
                {"id":"new", "dependencies":[]}
            ]}
        })
    }

    #[test]
    fn selected_app_excludes_other_workspace_dependencies() {
        let r = resolution_from_metadata(&workspace_graph(serde_json::json!("app-old"))).unwrap();
        assert_eq!(r.version, "0.13.0");
        assert_eq!(r.get("teksilo").unwrap().dir, Path::new("/old"));
    }

    #[test]
    fn conflicting_versions_are_reported_at_virtual_root_or_within_one_app() {
        let mut meta = workspace_graph(serde_json::Value::Null);
        assert!(matches!(
            resolution_from_metadata(&meta),
            Err(ResolveError::Ambiguous(_))
        ));
        meta["resolve"]["root"] = serde_json::json!("app-old");
        meta["resolve"]["nodes"][0]["dependencies"] = serde_json::json!(["old", "new"]);
        assert!(matches!(
            resolution_from_metadata(&meta),
            Err(ResolveError::Ambiguous(_))
        ));
    }

    #[test]
    fn enabling_features_name_what_turns_an_optional_teksilo_on() {
        let meta = serde_json::json!({
            "packages": [
                {"id": "app", "name": "app",
                 "dependencies": [
                     {"name": "teksilo", "rename": null, "optional": true},
                     {"name": "teksilo-data", "rename": "data", "optional": true},
                     {"name": "teksilo-core", "rename": null, "optional": false},
                     {"name": "serde", "rename": null, "optional": true}
                 ],
                 "features": {
                     "default": ["iced"],
                     "ui": ["dep:teksilo"],
                     "teksilo": ["dep:teksilo"],
                     "models": ["data/serde"],
                     "weak": ["teksilo?/inspector"],
                     "core-extra": ["teksilo-core/inspector"],
                     "ser": ["dep:serde"]
                 }},
                {"id": "other", "name": "other",
                 "dependencies": [{"name": "teksilo", "rename": null, "optional": true}],
                 "features": {"elsewhere": ["dep:teksilo"]}}
            ],
            "workspace_members": ["app", "other"],
            "resolve": {"root": "app", "nodes": []}
        });
        assert_eq!(enabling_features(&meta), ["models", "teksilo", "ui"]);
    }

    #[test]
    fn external_sibling_alone_is_not_a_framework_dependency() {
        let m = meta(serde_json::json!([pkg(
            "text-document",
            "1.0.0",
            "/text/Cargo.toml"
        )]));
        assert!(matches!(
            resolution_from_metadata(&m),
            Err(ResolveError::NotADependency)
        ));
    }

    #[test]
    fn member_only_sources_resolve_without_widgets() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir(t.path().join("src")).unwrap();
        std::fs::write(t.path().join("src/lib.rs"), "pub struct Example;").unwrap();
        std::fs::write(t.path().join("Cargo.toml"),
            "[package]\nname = \"teksilo-data\"\nversion = \"0.14.2\"\nedition = \"2021\"\n[workspace]\n").unwrap();
        let r = resolve_with_sources(t.path()).unwrap();
        assert!(r.get("teksilo-data").unwrap().has_src());
        assert!(r.widgets().is_none());
    }

    #[test]
    fn workspace_inherited_dependency_resolves() {
        // The case that silently broke the prototype: the consumer declares
        // `teksilo = { workspace = true }` and no manifest anywhere near the
        // crate names a version. cargo has already resolved it, so we see the
        // real one regardless of how it was declared.
        let m = meta(serde_json::json!([
            pkg("my-app", "0.1.0", "/app/Cargo.toml"),
            pkg("teksilo", "0.12.1", "/reg/teksilo-0.12.1/Cargo.toml"),
            pkg(
                "teksilo-widgets",
                "0.12.1",
                "/reg/teksilo-widgets-0.12.1/Cargo.toml"
            ),
        ]));
        let r = resolution_from_metadata(&m).unwrap();
        assert_eq!(r.version, "0.12.1");
        assert_eq!(
            r.widgets().unwrap().dir,
            Path::new("/reg/teksilo-widgets-0.12.1")
        );
    }

    #[test]
    fn umbrella_version_wins_over_member_order() {
        // BTreeMap order would put `teksilo-core` first alphabetically-ish;
        // the umbrella is what the app pinned, so it decides.
        let m = meta(serde_json::json!([
            pkg(
                "teksilo-core",
                "0.12.1",
                "/reg/teksilo-core-0.12.1/Cargo.toml"
            ),
            pkg("teksilo", "0.12.1", "/reg/teksilo-0.12.1/Cargo.toml"),
        ]));
        assert_eq!(resolution_from_metadata(&m).unwrap().version, "0.12.1");
    }

    #[test]
    fn a_member_only_app_still_resolves() {
        let m = meta(serde_json::json!([pkg(
            "teksilo-data",
            "0.12.1",
            "/reg/teksilo-data-0.12.1/Cargo.toml"
        ),]));
        let r = resolution_from_metadata(&m).unwrap();
        assert_eq!(r.version, "0.12.1");
        assert!(r.widgets().is_none());
    }

    #[test]
    fn no_teksilo_is_a_named_error() {
        let m = meta(serde_json::json!([pkg(
            "serde",
            "1.0.0",
            "/reg/serde/Cargo.toml"
        )]));
        assert!(matches!(
            resolution_from_metadata(&m),
            Err(ResolveError::NotADependency)
        ));
    }

    #[test]
    fn a_lookalike_package_is_not_teksilo() {
        assert!(is_teksilo_package("teksilo"));
        assert!(is_teksilo_package("teksilo-widgets"));
        assert!(!is_teksilo_package("teksilonium"));
        assert!(!is_teksilo_package("not-teksilo"));
    }

    #[test]
    fn checkout_root_needs_both_markers() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let widgets_dir = root.join("crates").join("teksilo-widgets");
        std::fs::create_dir_all(&widgets_dir).unwrap();

        let mut crates = BTreeMap::new();
        crates.insert(
            "teksilo-widgets".to_string(),
            ResolvedCrate {
                name: "teksilo-widgets".into(),
                version: "0.12.1".into(),
                dir: widgets_dir,
            },
        );
        let r = Resolution {
            crates,
            version: "0.12.1".into(),
            enabled_by: None,
        };

        // crates/ exists but tools/ does not yet: not a usable checkout.
        assert_eq!(r.checkout_root(), None);

        std::fs::create_dir_all(root.join("tools")).unwrap();
        std::fs::write(root.join("tools").join("extract_widget_api.py"), b"#").unwrap();
        assert_eq!(r.checkout_root().unwrap(), root);
    }

    #[test]
    fn a_registry_dependency_is_not_a_checkout() {
        // The common case: sources under ~/.cargo/registry have no tools/ above
        // them, so the staged-monorepo path must be taken instead.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("registry").join("teksilo-widgets-0.12.1");
        std::fs::create_dir_all(&dir).unwrap();
        let mut crates = BTreeMap::new();
        crates.insert(
            "teksilo-widgets".to_string(),
            ResolvedCrate {
                name: "teksilo-widgets".into(),
                version: "0.12.1".into(),
                dir,
            },
        );
        let r = Resolution {
            crates,
            version: "0.12.1".into(),
            enabled_by: None,
        };
        assert_eq!(r.checkout_root(), None);
    }
}
