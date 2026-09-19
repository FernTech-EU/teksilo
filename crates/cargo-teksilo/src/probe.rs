// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Putting the automation probe harness into the consumer's repository.
//!
//! The harness is Python, embedded in this binary and written out into
//! `scripts/teksilo_probe/`. Four reasons it is materialised rather than
//! installed from an index, the last being the real one:
//!
//! 1. **No Python packaging.** No pip, no venv, no `requirements.txt`, no
//!    "which python3 does this project mean".
//! 2. **Version-matched by construction.** This binary is versioned with the
//!    framework and only runs when the app resolved a matching teksilo, so the
//!    harness can never be older than the bridge it drives. The prototype's
//!    worst outage was exactly that skew: 0.9.3 renamed the bridge's announce
//!    line from `socket` to `endpoint`, every probe's regex missed it, waited
//!    60 seconds and reported "no bridge socket".
//! 3. **It lands in the repository** — greppable, editable, committed, and
//!    diffable when it changes.
//! 4. **The agent reads it as a worked example.** A harness in `site-packages`
//!    is invisible to an agent reading the repository it is working in;
//!    `scripts/` is not. That is how the prototype's 74 probes actually teach:
//!    the author of probe 75 copies probe 31.
//!
//! ## What is ours and what is theirs
//!
//! Everything under `scripts/teksilo_probe/` is generated and owned by this
//! tool. The consumer's own probes live one level up, in `scripts/`, and are
//! never read, written or considered. A file we generated that has since been
//! edited is **not** silently overwritten: the manifest records what we wrote,
//! so an edit is detectable, and replacing it needs `--force`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use include_dir::{Dir, include_dir};

/// The harness, embedded at build time.
static PROBE: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/embedded/probe");

/// Where the harness is written, relative to the project root.
///
/// This directory **is** the importable `teksilo_probe` package, so the embedded
/// `teksilo_probe/` prefix is stripped on the way out (see [`shipped_path`]).
/// A probe at `scripts/my_probe.py` puts `scripts/` on `sys.path` and does
/// `import teksilo_probe`; nesting the package one level deeper would make that
/// import fail for a reason nothing in the error message would explain.
const DEST: &str = "scripts/teksilo_probe";

/// Records what this tool wrote, so a later run can tell its own output from
/// a consumer's edit. Lives inside the generated tree, because it describes
/// that tree and should vanish with it.
const MANIFEST: &str = ".teksilo-probe-manifest";

#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("could not write the probe harness: {0}")]
    Io(#[from] std::io::Error),

    #[error(
        "{count} generated file(s) have local edits:\n{files}\n\
         Re-run with `--force` to overwrite them, or move your changes into\n\
         `scripts/` (outside `{DEST}`), which this tool never touches."
    )]
    LocallyModified { count: usize, files: String },

    #[error("could not update {0}: {1}")]
    Manifest(PathBuf, String),
}

/// Outcome of a materialisation, for reporting.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Written {
    pub created: Vec<String>,
    pub updated: Vec<String>,
    pub unchanged: Vec<String>,
}

impl Written {
    pub fn total(&self) -> usize {
        self.created.len() + self.updated.len() + self.unchanged.len()
    }
}

/// Write the harness into `project`, honouring local edits unless `force`.
pub fn materialise(project: &Path, force: bool) -> Result<Written, ProbeError> {
    let dest = project.join(DEST);
    let previous = read_manifest(&dest);

    // Refuse as a set rather than one file at a time: a consumer who edited
    // three files wants to hear about three, not to re-run and be stopped
    // again by the next one.
    if !force && !previous.is_empty() {
        let modified = locally_modified(&dest, &previous);
        if !modified.is_empty() {
            return Err(ProbeError::LocallyModified {
                count: modified.len(),
                files: modified
                    .iter()
                    .map(|f| format!("  {DEST}/{f}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            });
        }
    }

    let mut written = Written::default();
    let mut manifest = BTreeMap::new();

    for file in walk(&PROBE) {
        let rel = shipped_path(file.path());
        let body = file.contents();
        let target = dest.join(&rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let existed = target.exists();
        let same = existed && std::fs::read(&target).map(|c| c == body).unwrap_or(false);
        if !same {
            std::fs::write(&target, body)?;
        }
        manifest.insert(rel.clone(), digest(body));

        if !existed {
            written.created.push(rel);
        } else if same {
            written.unchanged.push(rel);
        } else {
            written.updated.push(rel);
        }
    }

    // Anything we wrote last time and no longer ship is ours to remove; a file
    // the consumer added under the generated tree is not in the manifest and
    // so is left alone.
    for old in previous.keys() {
        if !manifest.contains_key(old) {
            let _ = std::fs::remove_file(dest.join(old));
        }
    }

    write_manifest(&dest, &manifest)?;
    Ok(written)
}

/// Record which teksilo this harness came from, in the consumer's manifest.
///
/// Provenance only — never content. A later run compares this against the
/// resolved teksilo and warns on drift, which is how someone finds out their
/// harness predates a framework upgrade before a probe fails obscurely.
pub fn record_provenance(project: &Path, version: &str) -> Result<(), ProbeError> {
    let manifest_path = project.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest_path)?;
    let mut doc = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| ProbeError::Manifest(manifest_path.clone(), e.to_string()))?;

    // `package.metadata` is the documented place for third-party tool state,
    // and cargo ignores everything under it.
    let meta = doc
        .entry("package")
        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()))
        .as_table_mut()
        .ok_or_else(|| {
            ProbeError::Manifest(manifest_path.clone(), "[package] is not a table".into())
        })?
        .entry("metadata")
        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()))
        .as_table_mut()
        .ok_or_else(|| {
            ProbeError::Manifest(
                manifest_path.clone(),
                "[package.metadata] is not a table".into(),
            )
        })?
        .entry("teksilo")
        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()))
        .as_table_mut()
        .ok_or_else(|| {
            ProbeError::Manifest(
                manifest_path.clone(),
                "[package.metadata.teksilo] is not a table".into(),
            )
        })?;

    meta["probe"] = toml_edit::value(version);
    std::fs::write(&manifest_path, doc.to_string())?;
    Ok(())
}

/// The teksilo version the harness in `project` was written for, if any.
pub fn recorded_provenance(project: &Path) -> Option<String> {
    let text = std::fs::read_to_string(project.join("Cargo.toml")).ok()?;
    let doc = text.parse::<toml_edit::DocumentMut>().ok()?;
    doc.get("package")?
        .get("metadata")?
        .get("teksilo")?
        .get("probe")?
        .as_str()
        .map(str::to_string)
}

/// The files that belong in a consumer's repository.
///
/// Not everything beside the library ships. `tests/` exercises the harness
/// against fixtures that live in this repository, and `generate_tools.py`
/// regenerates `tools.py` from `teksilo-automation`'s `mcp_schema.rs` — which a
/// consumer does not have. Materialising either puts files in someone's project
/// that cannot run there, which reads as a broken install rather than as
/// scaffolding they were never meant to use.
fn ships(path: &Path) -> bool {
    let p = path.to_string_lossy();
    !p.starts_with("tests/") && p != "generate_tools.py"
}

/// The path a shipped file takes inside [`DEST`].
///
/// `teksilo_probe/session.py` ships as `session.py`, because `DEST` is itself
/// the package directory. Anything else keeps its own path.
fn shipped_path(path: &Path) -> String {
    let p = path.to_string_lossy();
    p.strip_prefix("teksilo_probe/").unwrap_or(&p).to_string()
}

fn walk<'a>(dir: &'a Dir<'a>) -> Vec<&'a include_dir::File<'a>> {
    let mut out: Vec<_> = dir.files().filter(|f| ships(f.path())).collect();
    for d in dir.dirs() {
        if ships(d.path()) {
            out.extend(walk(d));
        }
    }
    out.sort_by_key(|f| f.path());
    out
}

/// A content digest that needs no dependency.
///
/// FNV-1a: this detects an accidental edit, which is all it is for. It is not
/// a security boundary and nothing here pretends otherwise.
fn digest(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn read_manifest(dest: &Path) -> BTreeMap<String, String> {
    let Ok(text) = std::fs::read_to_string(dest.join(MANIFEST)) else {
        return BTreeMap::new();
    };
    text.lines()
        .filter(|line| !line.trim_start().starts_with('#') && !line.trim().is_empty())
        .filter_map(|line| line.split_once("  "))
        .map(|(h, f)| (f.trim().to_string(), h.trim().to_string()))
        .collect()
}

fn write_manifest(dest: &Path, manifest: &BTreeMap<String, String>) -> Result<(), ProbeError> {
    let mut out = String::from(
        "# Written by `cargo teksilo probe`. Do not edit.\n\
         # Lists what this tool generated, so a later run can tell its own\n\
         # output from your edits. Your own probes belong in scripts/, not here.\n",
    );
    for (file, hash) in manifest {
        out.push_str(&format!("{hash}  {file}\n"));
    }
    std::fs::write(dest.join(MANIFEST), out)?;
    Ok(())
}

fn locally_modified(dest: &Path, previous: &BTreeMap<String, String>) -> Vec<String> {
    let mut modified: Vec<String> = previous
        .iter()
        .filter(|(file, recorded)| {
            std::fs::read(dest.join(file))
                .map(|body| digest(&body) != **recorded)
                .unwrap_or(false) // a deleted file is not a conflicting edit
        })
        .map(|(file, _)| file.clone())
        .collect();
    modified.sort();
    modified
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(
            t.path().join("Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
             [dependencies]\nteksilo = \"0.12\"\n",
        )
        .unwrap();
        t
    }

    #[test]
    fn a_first_run_creates_the_whole_harness() {
        let p = project();
        let w = materialise(p.path(), false).unwrap();
        assert!(w.total() > 5, "expected a real harness, got {}", w.total());
        assert_eq!(w.updated.len(), 0);
        assert_eq!(w.unchanged.len(), 0);
        assert!(p.path().join(DEST).join("session.py").is_file());
        assert!(p.path().join(DEST).join(MANIFEST).is_file());
    }

    #[test]
    fn a_second_run_changes_nothing() {
        let p = project();
        materialise(p.path(), false).unwrap();
        let w = materialise(p.path(), false).unwrap();
        assert_eq!(w.created.len(), 0);
        assert_eq!(w.updated.len(), 0);
        assert!(w.unchanged.len() > 5);
    }

    #[test]
    fn a_local_edit_is_refused_not_overwritten() {
        let p = project();
        materialise(p.path(), false).unwrap();
        let edited = p.path().join(DEST).join("session.py");
        std::fs::write(&edited, b"# mine now\n").unwrap();

        let err = materialise(p.path(), false).unwrap_err();
        assert!(matches!(err, ProbeError::LocallyModified { count: 1, .. }));
        // The point of refusing: the edit survives.
        assert_eq!(std::fs::read_to_string(&edited).unwrap(), "# mine now\n");
    }

    #[test]
    fn force_overwrites_a_local_edit() {
        let p = project();
        materialise(p.path(), false).unwrap();
        let edited = p.path().join(DEST).join("session.py");
        std::fs::write(&edited, b"# mine now\n").unwrap();

        let w = materialise(p.path(), true).unwrap();
        assert!(w.updated.contains(&"session.py".to_string()));
        assert_ne!(std::fs::read_to_string(&edited).unwrap(), "# mine now\n");
    }

    #[test]
    fn a_consumers_own_file_under_the_tree_is_left_alone() {
        // Not in the manifest, so not ours to remove — even though it sits
        // inside the generated directory.
        let p = project();
        materialise(p.path(), false).unwrap();
        let theirs = p.path().join(DEST).join("my_notes.md");
        std::fs::write(&theirs, b"notes").unwrap();
        materialise(p.path(), false).unwrap();
        assert!(theirs.is_file());
    }

    #[test]
    fn provenance_round_trips_and_preserves_formatting() {
        let p = project();
        let before = std::fs::read_to_string(p.path().join("Cargo.toml")).unwrap();
        record_provenance(p.path(), "0.12.1").unwrap();
        assert_eq!(recorded_provenance(p.path()).as_deref(), Some("0.12.1"));

        let after = std::fs::read_to_string(p.path().join("Cargo.toml")).unwrap();
        // The consumer's own content must survive verbatim — we add a key, we
        // do not restyle their manifest.
        assert!(after.contains("name = \"app\""));
        assert!(after.contains("teksilo = \"0.12\""));
        assert!(after.len() > before.len());
    }

    #[test]
    fn provenance_is_idempotent() {
        let p = project();
        record_provenance(p.path(), "0.12.1").unwrap();
        let once = std::fs::read_to_string(p.path().join("Cargo.toml")).unwrap();
        record_provenance(p.path(), "0.12.1").unwrap();
        let twice = std::fs::read_to_string(p.path().join("Cargo.toml")).unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn provenance_updates_in_place_on_upgrade() {
        let p = project();
        record_provenance(p.path(), "0.12.0").unwrap();
        record_provenance(p.path(), "0.12.1").unwrap();
        assert_eq!(recorded_provenance(p.path()).as_deref(), Some("0.12.1"));
        let text = std::fs::read_to_string(p.path().join("Cargo.toml")).unwrap();
        assert_eq!(
            text.matches("probe =").count(),
            1,
            "must update, not append"
        );
    }

    #[test]
    fn no_provenance_recorded_reads_as_none() {
        let p = project();
        assert_eq!(recorded_provenance(p.path()), None);
    }

    #[test]
    fn the_embedded_harness_carries_the_real_modules() {
        let names: Vec<String> = walk(&PROBE)
            .iter()
            .map(|f| shipped_path(f.path()))
            .collect();
        for required in [
            "session.py",
            "bridge.py",
            "navigate.py",
            "tools.py",
            "report.py",
        ] {
            assert!(
                names.iter().any(|n| n == required),
                "missing {required} in {names:?}"
            );
        }
        // Repo-only scaffolding must not land in a consumer's project.
        assert!(
            !names.iter().any(|n| n.starts_with("tests/")),
            "the harness test-suite must not ship: {names:?}"
        );
        assert!(!names.iter().any(|n| n == "generate_tools.py"));
    }

    #[test]
    fn digest_distinguishes_content() {
        assert_eq!(digest(b"abc"), digest(b"abc"));
        assert_ne!(digest(b"abc"), digest(b"abd"));
    }
}
