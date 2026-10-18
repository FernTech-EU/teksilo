// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The `cargo install` line that brings a binary to a given release.
//!
//! `status` prints it and `update` runs it, so both build it here: a printed
//! remedy that differs from what `update` does would send a reader to a
//! command that installs somewhere else.

use std::path::{Path, PathBuf};
use std::process::Command;

/// One `cargo install` invocation, as arguments after `cargo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Install {
    args: Vec<String>,
}

impl Install {
    /// `cargo install <name> --version <version> --locked`, into the root the
    /// binary at `located` came from.
    pub fn release(name: &str, version: &str, located: Option<&Path>) -> Self {
        Self::with_root(
            vec![
                "install".into(),
                name.into(),
                "--version".into(),
                version.into(),
                "--locked".into(),
            ],
            located,
        )
    }

    /// `cargo install --path <path> --locked`, for a version a checkout has
    /// and crates.io may not.
    pub fn path(path: &Path, located: Option<&Path>) -> Self {
        Self::with_root(
            vec![
                "install".into(),
                "--path".into(),
                path.display().to_string(),
                "--locked".into(),
            ],
            located,
        )
    }

    fn with_root(mut args: Vec<String>, located: Option<&Path>) -> Self {
        if let Some(root) = located.and_then(non_default_root) {
            args.push("--root".into());
            args.push(root.display().to_string());
        }
        Self { args }
    }

    /// The command as a reader would type it.
    pub fn display(&self) -> String {
        std::iter::once("cargo")
            .chain(self.args.iter().map(String::as_str))
            .map(quote)
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Run it, with cargo's own progress on the terminal.
    pub fn run(&self) -> Result<(), String> {
        // Cargo names itself in `CARGO` for the subcommands it runs, which is
        // the cargo this invocation came through.
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let status = Command::new(cargo)
            .args(&self.args)
            .status()
            .map_err(|e| format!("could not run `{}`: {e}", self.display()))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("`{}` failed", self.display()))
        }
    }
}

/// The install root of a binary at `<root>/bin/<name>`, unless it is the root
/// `cargo install` uses anyway.
///
/// Without it, replacing a binary that lives under a `--root` of its own would
/// install a second copy into `~/.cargo/bin` and leave the one PATH serves as
/// it was. Left out for the default root, so the usual line stays the usual
/// line.
fn non_default_root(binary: &Path) -> Option<PathBuf> {
    let bin = binary.parent()?;
    if bin.file_name()? != "bin" {
        return None;
    }
    let root = bin.parent()?;
    let same = |other: PathBuf| {
        let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        canonical(&other) == canonical(root)
    };
    if default_root().is_some_and(same) {
        None
    } else {
        Some(root.to_path_buf())
    }
}

/// Where `cargo install` puts binaries when given no `--root`.
///
/// `install.root` in a cargo config file is not read: a root set there makes
/// `--root` redundant rather than wrong.
fn default_root() -> Option<PathBuf> {
    if let Some(root) = std::env::var_os("CARGO_INSTALL_ROOT") {
        return Some(root.into());
    }
    if let Some(home) = std::env::var_os("CARGO_HOME") {
        return Some(home.into());
    }
    crate::setup::home_dir().ok().map(|h| h.join(".cargo"))
}

fn quote(word: &str) -> String {
    if word.contains(char::is_whitespace) {
        format!("'{word}'")
    } else {
        word.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_line_names_the_version_and_locks() {
        assert_eq!(
            Install::release("teksilo-automation-mcp", "0.19.0", None).display(),
            "cargo install teksilo-automation-mcp --version 0.19.0 --locked"
        );
    }

    #[test]
    fn a_binary_under_its_own_root_is_replaced_there() {
        let root = tempfile::tempdir().unwrap();
        let binary = root.path().join("bin/cargo-teksilo-fmt");
        let line = Install::release("cargo-teksilo-fmt", "0.19.0", Some(&binary)).display();
        assert!(
            line.ends_with(&format!("--locked --root {}", root.path().display())),
            "{line}"
        );
    }

    #[test]
    fn a_binary_outside_a_bin_directory_gets_no_root() {
        let line = Install::release(
            "cargo-teksilo",
            "0.19.0",
            Some(Path::new("/work/target/debug/cargo-teksilo")),
        )
        .display();
        assert!(!line.contains("--root"), "{line}");
    }

    #[test]
    fn a_checkout_installs_by_path() {
        assert_eq!(
            Install::path(Path::new("/src/my teksilo/crates/cargo-teksilo"), None).display(),
            "cargo install --path '/src/my teksilo/crates/cargo-teksilo' --locked"
        );
    }
}
