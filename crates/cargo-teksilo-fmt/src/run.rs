// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Run-loop: discover files, format each, write or check-only.
//!
//! Atomic writes: the formatted text goes into a `NamedTempFile` next
//! to the target file, then `persist`s via rename — same pattern used
//! by `teksilo-settings`'s flush path. This keeps interrupted runs from
//! leaving truncated source files on disk.

use std::io::Write;
use std::path::Path;

use teksilo_fmt::{FmtConfig, FmtError, format_file};
use tempfile::NamedTempFile;

use crate::Config;
use crate::walk;

pub struct Outcome {
    pub changed: usize,
    pub unchanged: usize,
    pub errors: usize,
    /// True under `--check` and any file would change. Causes exit 1.
    pub check_failed: bool,
}

impl Outcome {
    pub fn exit_code(&self) -> i32 {
        if self.errors > 0 || self.check_failed {
            1
        } else {
            0
        }
    }
}

pub fn run(cfg: &Config) -> Outcome {
    let files = match walk::collect(&cfg.paths) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            return Outcome {
                changed: 0,
                unchanged: 0,
                errors: 1,
                check_failed: false,
            };
        }
    };

    let mut outcome = Outcome {
        changed: 0,
        unchanged: 0,
        errors: 0,
        check_failed: false,
    };
    let fmt_cfg = FmtConfig::default();

    for file in &files {
        match process_one(file, &fmt_cfg, cfg.check) {
            Ok(FileResult::Unchanged) => {
                outcome.unchanged += 1;
            }
            Ok(FileResult::Changed) => {
                outcome.changed += 1;
                if cfg.check {
                    outcome.check_failed = true;
                    println!("Would reformat: {}", file.display());
                } else if !cfg.quiet {
                    println!("Reformatted: {}", file.display());
                }
            }
            Ok(FileResult::NoTeksiMacros) => {
                outcome.unchanged += 1;
            }
            Err(e) => {
                outcome.errors += 1;
                eprintln!("error: {}: {e}", file.display());
            }
        }
    }

    if !cfg.quiet {
        let scanned = files.len();
        if cfg.check {
            if outcome.check_failed {
                println!(
                    "{} of {scanned} file(s) would be reformatted",
                    outcome.changed
                );
            } else {
                println!("{scanned} file(s) already formatted");
            }
        } else {
            println!(
                "{} reformatted, {} unchanged{}",
                outcome.changed,
                outcome.unchanged,
                if outcome.errors > 0 {
                    format!(", {} errored", outcome.errors)
                } else {
                    String::new()
                }
            );
        }
    }

    outcome
}

enum FileResult {
    Unchanged,
    Changed,
    NoTeksiMacros,
}

#[derive(Debug, thiserror::Error)]
enum ProcessError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Fmt(#[from] FmtError),
    #[error("{0}")]
    Persist(#[from] tempfile::PersistError),
}

fn process_one(
    path: &Path,
    fmt_cfg: &FmtConfig,
    check_only: bool,
) -> Result<FileResult, ProcessError> {
    let source = std::fs::read_to_string(path)?;
    // Cheap pre-filter: if the file has no `teksu!` token, skip parsing.
    // The host-file syn::parse_file inside format_file is the dominant
    // cost; this guard turns the no-op case into a single string scan.
    if !source.contains("teksu!") {
        return Ok(FileResult::NoTeksiMacros);
    }
    let formatted = format_file(&source, fmt_cfg)?;
    if formatted == source {
        return Ok(FileResult::Unchanged);
    }
    if check_only {
        return Ok(FileResult::Changed);
    }
    write_atomic(path, &formatted)?;
    Ok(FileResult::Changed)
}

fn write_atomic(path: &Path, content: &str) -> Result<(), ProcessError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    // Snapshot the source's permissions before we replace the file.
    // `NamedTempFile::persist` uses the temp file's mode (0600 on Unix
    // by default) for the destination after rename, which silently
    // tightens permissions on a previously world-readable .rs file.
    // Re-apply the snapshot after persist to leave mode untouched.
    let original_perms = std::fs::metadata(path).ok().map(|m| m.permissions());

    let mut tmp = NamedTempFile::new_in(parent)?;
    tmp.as_file_mut().write_all(content.as_bytes())?;
    tmp.as_file_mut().sync_all()?;
    tmp.persist(path)?;

    if let Some(perms) = original_perms {
        // A failure here means the file was rewritten with the wrong
        // mode but the content is correct — ignore quietly. The user
        // can chmod by hand if it matters; reporting it would create
        // noise on every run.
        let _ = std::fs::set_permissions(path, perms);
    }
    Ok(())
}
