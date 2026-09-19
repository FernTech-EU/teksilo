// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The exact public API of a teksilo type, for the version *this* app pins.
//!
//! Two paths, in preference order:
//!
//! 1. **A reachable checkout.** If the resolved crates sit inside a teksilo
//!    repository that ships `tools/extract_widget_api.py`, run *that*
//!    extractor. It is authoritative — version-matched by construction, and
//!    never staler than the sources beside it.
//! 2. **A staged monorepo.** Otherwise the crates came from the registry (or
//!    git), where there is no `tools/` above them. Build a throwaway directory
//!    shaped like the framework repo, put the embedded extractor in its
//!    `tools/`, point `crates/<name>` at each resolved source, and run it
//!    there. Same curated output, no checkout required.
//!
//! Both paths read the **resolved** sources, so the answer is always for the
//! version the app actually depends on rather than the newest one that exists.
//!
//! ## Two details that look like style and are not
//!
//! The extractor derives its repository root from `Path(__file__).resolve()`.
//! So the tool is **copied** into the staging directory, never symlinked — a
//! symlinked tool resolves its root back to wherever the original lives and
//! then looks for `crates/` *there*, quietly extracting from the wrong tree or
//! finding nothing at all.
//!
//! Crate sources are symlinked rather than copied, because copying
//! `teksilo-widgets` alone means 356 files on every invocation. On Windows,
//! though, creating a directory symlink requires Developer Mode or elevation,
//! so a failure there falls back to copying just the `src/` subtree — the only
//! part the extractor reads.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::guard::{self, Verdict};
use crate::resolve::{self, Resolution, ResolvedCrate};

/// The extractor, embedded so the tool works with no checkout.
///
/// Byte-identical to `tools/extract_widget_api.py`; CI enforces that, which is
/// what replaced the manual `cp` this copy used to depend on.
const EMBEDDED_EXTRACTOR: &str = include_str!("../embedded/extract_widget_api.py");

#[derive(Debug, thiserror::Error)]
pub enum SymbolError {
    #[error(transparent)]
    Resolve(#[from] resolve::ResolveError),

    #[error("{0}")]
    Refused(String),

    #[error(
        "no Python 3 interpreter found on PATH.\n\
         The API extractor is a Python script; install Python 3 and try again."
    )]
    NoPython,

    /// Staging the throwaway repo, or launching the interpreter, failed.
    #[error("could not run the API extractor: {0}")]
    Io(#[from] std::io::Error),
}

/// Run a symbol lookup with `args` (the extractor's own CLI: names, `--list`,
/// `-f json`, `--crate data`, …), from the app directory `dir`.
pub fn run(dir: &Path, args: &[String]) -> Result<i32, SymbolError> {
    let resolution = resolve::resolve_with_sources(dir)?;

    let verdict = guard::check(&resolution.version);
    if !verdict.may_answer() {
        let Verdict::Refuse { app, tool } = &verdict else {
            unreachable!()
        };
        return Err(SymbolError::Refused(guard::refusal_text(
            app,
            tool,
            "symbol lookup",
        )));
    }
    if let Some(note) = verdict.note() {
        eprintln!("{note}");
    }

    let python = find_python().ok_or(SymbolError::NoPython)?;

    if let Some(root) = resolution.checkout_root() {
        let tool = root.join("tools").join("extract_widget_api.py");
        return exec_extractor(&python, &tool, args, &root);
    }

    let stage = tempfile::Builder::new()
        .prefix("cargo-teksilo-")
        .tempdir()?;
    let tool = stage_monorepo(stage.path(), &resolution)?;
    exec_extractor(&python, &tool, args, stage.path())
}

/// Build the throwaway repo shape and return the path of the staged tool.
fn stage_monorepo(stage: &Path, resolution: &Resolution) -> Result<PathBuf, SymbolError> {
    let tools = stage.join("tools");
    let crates = stage.join("crates");
    std::fs::create_dir_all(&tools)?;
    std::fs::create_dir_all(&crates)?;

    // COPY, never symlink — see the module docs.
    let tool = tools.join("extract_widget_api.py");
    std::fs::write(&tool, EMBEDDED_EXTRACTOR)?;

    for c in resolution.crates.values() {
        if !c.has_src() {
            continue;
        }
        link_or_copy_crate(c, &crates.join(&c.name))?;
    }
    Ok(tool)
}

/// Point `dest` at the crate's source, cheaply where the platform allows.
fn link_or_copy_crate(c: &ResolvedCrate, dest: &Path) -> Result<(), SymbolError> {
    if symlink_dir(&c.dir, dest).is_ok() {
        return Ok(());
    }
    // Windows without Developer Mode, or a filesystem that refuses links.
    // The extractor only ever reads `src/`, so that is all that is copied.
    std::fs::create_dir_all(dest)?;
    copy_tree(&c.src(), &dest.join("src"))?;
    Ok(())
}

#[cfg(unix)]
fn symlink_dir(src: &Path, dest: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(src, dest)
}

#[cfg(windows)]
fn symlink_dir(src: &Path, dest: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(src, dest)
}

#[cfg(not(any(unix, windows)))]
fn symlink_dir(_src: &Path, _dest: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other(
        "symlinks unsupported on this platform",
    ))
}

fn copy_tree(src: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

fn exec_extractor(
    python: &Path,
    tool: &Path,
    args: &[String],
    cwd: &Path,
) -> Result<i32, SymbolError> {
    let status = Command::new(python)
        .arg(tool)
        .args(args)
        .current_dir(cwd)
        .status()?;
    Ok(status.code().unwrap_or(1))
}

/// A Python 3 interpreter, by the names it goes by on each platform.
fn find_python() -> Option<PathBuf> {
    for name in ["python3", "python"] {
        if let Some(p) = which(name)
            && is_python3(&p)
        {
            return Some(p);
        }
    }
    None
}

fn is_python3(python: &Path) -> bool {
    Command::new(python)
        .args([
            "-c",
            "import sys; sys.exit(0 if sys.version_info[0] == 3 else 1)",
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Minimal PATH search — avoids a dependency for one well-defined job.
fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
            .split(';')
            .map(|e| e.to_ascii_lowercase())
            .collect()
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&path) {
        for ext in &exts {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn fake_crate(root: &Path, name: &str, files: &[(&str, &str)]) -> ResolvedCrate {
        let dir = root.join(name);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        for (rel, body) in files {
            let p = dir.join("src").join(rel);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(p, body).unwrap();
        }
        ResolvedCrate {
            name: name.into(),
            version: "0.12.1".into(),
            dir,
        }
    }

    fn resolution_of(crates: Vec<ResolvedCrate>) -> Resolution {
        let mut map = BTreeMap::new();
        for c in crates {
            map.insert(c.name.clone(), c);
        }
        Resolution {
            crates: map,
            version: "0.12.1".into(),
        }
    }

    #[test]
    fn staging_puts_a_real_copy_of_the_tool_in_place() {
        // Load-bearing: the extractor resolves its repo root from __file__, so a
        // symlinked tool would look for crates/ next to the ORIGINAL and extract
        // from the wrong tree.
        let src = tempfile::tempdir().unwrap();
        let stage = tempfile::tempdir().unwrap();
        let c = fake_crate(
            src.path(),
            "teksilo-widgets",
            &[("lib.rs", "pub struct Button;")],
        );
        let tool = stage_monorepo(stage.path(), &resolution_of(vec![c])).unwrap();

        assert!(tool.is_file());
        let meta = std::fs::symlink_metadata(&tool).unwrap();
        assert!(
            !meta.file_type().is_symlink(),
            "the tool must be copied, not linked"
        );
        assert_eq!(std::fs::read_to_string(&tool).unwrap(), EMBEDDED_EXTRACTOR);
    }

    #[test]
    fn staging_exposes_every_resolved_crate_source() {
        let src = tempfile::tempdir().unwrap();
        let stage = tempfile::tempdir().unwrap();
        let r = resolution_of(vec![
            fake_crate(
                src.path(),
                "teksilo-widgets",
                &[("lib.rs", "pub struct Button;")],
            ),
            fake_crate(
                src.path(),
                "teksilo-data",
                &[("lib.rs", "pub struct ListModel;")],
            ),
        ]);
        stage_monorepo(stage.path(), &r).unwrap();

        for name in ["teksilo-widgets", "teksilo-data"] {
            let lib = stage
                .path()
                .join("crates")
                .join(name)
                .join("src")
                .join("lib.rs");
            assert!(lib.is_file(), "{name} source not reachable at {lib:?}");
        }
        let body = std::fs::read_to_string(stage.path().join("crates/teksilo-widgets/src/lib.rs"))
            .unwrap();
        assert_eq!(body, "pub struct Button;");
    }

    #[test]
    fn a_crate_with_no_source_on_disk_is_skipped_not_fatal() {
        let stage = tempfile::tempdir().unwrap();
        let ghost = ResolvedCrate {
            name: "teksilo-ghost".into(),
            version: "0.12.1".into(),
            dir: PathBuf::from("/nonexistent/teksilo-ghost"),
        };
        stage_monorepo(stage.path(), &resolution_of(vec![ghost])).unwrap();
        assert!(!stage.path().join("crates/teksilo-ghost").exists());
    }

    #[test]
    fn the_copy_fallback_reproduces_the_source_tree() {
        // The Windows-without-Developer-Mode path, exercised everywhere.
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let c = fake_crate(
            src.path(),
            "teksilo-widgets",
            &[
                ("lib.rs", "pub struct Button;"),
                ("primitives/stack.rs", "pub struct HStack;"),
            ],
        );
        copy_tree(&c.src(), &dest.path().join("src")).unwrap();
        assert!(dest.path().join("src/lib.rs").is_file());
        assert!(dest.path().join("src/primitives/stack.rs").is_file());
    }

    #[test]
    fn the_embedded_extractor_is_the_real_tool() {
        assert!(EMBEDDED_EXTRACTOR.contains("CRATE_SPECS"));
        assert!(EMBEDDED_EXTRACTOR.contains("SPDX-License-Identifier"));
        assert!(
            EMBEDDED_EXTRACTOR.len() > 50_000,
            "suspiciously small extractor"
        );
    }

    #[test]
    fn python3_is_discoverable_in_this_environment() {
        // Not a property of the code so much as a precondition for the tool to
        // work at all; worth failing loudly in CI if it stops holding.
        assert!(find_python().is_some(), "no python3 on PATH");
    }
}
