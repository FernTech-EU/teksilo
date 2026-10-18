// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The companion binaries on the user's PATH, and whether they match the app.
//!
//! `teksilo-automation-mcp` and `cargo-teksilo-fmt` are installed on their own
//! and nothing moves them when the app does. A server built from another
//! release "will answer confidently about tools and semantics the app does not
//! have", and a formatter from another release formats a `teksu!` grammar the
//! app does not have, so `status` says which one is ahead on the PATH and
//! `update` replaces it. The PATH lookup and the version query are injected
//! into [`inspect`], so a test never depends on the machine it runs on.

use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use crate::install::Install;

/// A binary published from this workspace beside cargo-teksilo, whose crate
/// and binary share one name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Companion {
    pub binary: &'static str,
}

/// The automation MCP server.
pub const MCP: Companion = Companion {
    binary: "teksilo-automation-mcp",
};

/// The `teksu!` formatter.
pub const FMT: Companion = Companion {
    binary: "cargo-teksilo-fmt",
};

/// How long `--version` may take before the binary is reported as unreadable.
const VERSION_TIMEOUT: Duration = Duration::from_secs(2);

/// How the binary on PATH relates to the app's release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Nothing of that name on PATH.
    NotInstalled,
    /// Same major and minor as the app, and the same patch or a later one.
    Matching { version: String },
    /// Same major and minor as the app, earlier patch.
    Older { version: String },
    /// Built from another minor or major release.
    Mismatch { version: String },
    /// On PATH, but `--version` failed, timed out or printed nothing usable.
    Unknown,
}

/// What [`inspect`] found for one companion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub companion: Companion,
    /// The version the app resolved.
    pub expected: String,
    /// Where the binary is, when it is anywhere.
    pub path: Option<PathBuf>,
    pub state: State,
}

impl Found {
    /// The `state` of the JSON field.
    pub fn state(&self) -> &'static str {
        match self.state {
            State::NotInstalled => "missing",
            State::Matching { .. } => "matching",
            State::Older { .. } => "older",
            State::Mismatch { .. } => "mismatch",
            State::Unknown => "unknown",
        }
    }

    /// The version the binary reported, when it did.
    pub fn version(&self) -> Option<&str> {
        match &self.state {
            State::Matching { version }
            | State::Older { version }
            | State::Mismatch { version } => Some(version),
            _ => None,
        }
    }

    /// The text after the label of the human report.
    pub fn line(&self) -> String {
        let expected = &self.expected;
        match &self.state {
            State::NotInstalled => "not installed".into(),
            State::Matching { version } => format!("{version} (matches)"),
            State::Older { version } => format!("{version} (older than {expected})"),
            State::Mismatch { version } => format!("{version} (differs from {expected})"),
            State::Unknown => "installed (version unreadable)".into(),
        }
    }

    /// The command that brings the binary to the app's release, when it needs one.
    pub fn install(&self) -> Option<Install> {
        match self.state {
            State::Older { .. } | State::Mismatch { .. } | State::Unknown => Some(
                Install::release(self.companion.binary, &self.expected, self.path.as_deref()),
            ),
            State::NotInstalled | State::Matching { .. } => None,
        }
    }

    /// The remedy, for every state [`install`](Self::install) has a command for.
    pub fn help(&self) -> Option<String> {
        let command = self.install()?.display();
        let binary = self.companion.binary;
        Some(match &self.state {
            State::Older { version } => format!(
                "help: {binary} {version} is older than Teksilo {}; `{command}` updates it",
                self.expected
            ),
            State::Mismatch { .. } => format!(
                "help: the {binary} on your PATH was built from another release; \
                 `{command}` replaces it"
            ),
            _ => format!(
                "help: the {binary} on your PATH does not report its version; \
                 `{command}` replaces it"
            ),
        })
    }

    /// The JSON value of the report's field for this companion.
    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "state": self.state(),
            "version": self.version(),
            "expected": self.expected,
        })
    }
}

/// Classify `companion` against `expected`, the version the app resolved.
///
/// `locate` finds the binary and `version_of` asks it for its version; the
/// latter is called only when the former found something.
pub fn inspect(
    companion: Companion,
    expected: &str,
    locate: impl FnOnce() -> Option<PathBuf>,
    version_of: impl FnOnce(&Path) -> Option<String>,
) -> Found {
    let path = locate();
    let state = match &path {
        None => State::NotInstalled,
        Some(path) => match version_of(path) {
            None => State::Unknown,
            Some(version) => classify(version, expected),
        },
    };
    Found {
        companion,
        expected: expected.to_string(),
        path,
        state,
    }
}

fn classify(version: String, expected: &str) -> State {
    match (
        crate::guard::semver_parts(&version),
        crate::guard::semver_parts(expected),
    ) {
        (Some((a, b, p)), Some((c, d, q))) if (a, b) == (c, d) => {
            if p < q {
                State::Older { version }
            } else {
                State::Matching { version }
            }
        }
        (Some(_), Some(_)) => State::Mismatch { version },
        _ => State::Unknown,
    }
}

/// [`inspect`] against the real PATH.
pub fn inspect_path(companion: Companion, expected: &str) -> Found {
    inspect(
        companion,
        expected,
        || crate::symbol::which(companion.binary),
        |path| run_version(path, VERSION_TIMEOUT),
    )
}

/// Run `<path> --version` and parse its output, giving up after `timeout`.
fn run_version(path: &Path, timeout: Duration) -> Option<String> {
    let mut child = Command::new(path)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    let mut out = String::new();
    child
        .stdout
        .take()?
        .take(4096)
        .read_to_string(&mut out)
        .ok()?;
    parse_version(&out)
}

/// `teksilo-automation-mcp 0.15.1` → `0.15.1`.
fn parse_version(output: &str) -> Option<String> {
    let word = output.lines().next()?.split_whitespace().last()?;
    word.starts_with(|c: char| c.is_ascii_digit())
        .then(|| word.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found() -> Option<PathBuf> {
        Some(PathBuf::from("/nowhere/teksilo-automation-mcp"))
    }

    fn mcp(expected: &str, version: Option<&str>) -> Found {
        inspect(MCP, expected, found, |_| version.map(String::from))
    }

    #[test]
    fn a_missing_binary_is_not_queried() {
        let server = inspect(
            MCP,
            "0.15.1",
            || None,
            |_| panic!("the version of a binary that is not there"),
        );
        assert_eq!(server.state, State::NotInstalled);
        assert_eq!(server.line(), "not installed");
        assert!(server.help().is_none());
        assert!(server.install().is_none());
    }

    #[test]
    fn the_same_release_or_a_later_patch_matches() {
        for version in ["0.15.1", "0.15.9"] {
            let server = mcp("0.15.1", Some(version));
            assert_eq!(
                server.state,
                State::Matching {
                    version: version.into()
                }
            );
            assert!(server.help().is_none());
        }
    }

    #[test]
    fn an_earlier_patch_is_older_and_names_the_install_command() {
        let server = mcp("0.15.1", Some("0.15.0"));
        assert_eq!(server.state(), "older");
        assert_eq!(server.line(), "0.15.0 (older than 0.15.1)");
        assert_eq!(
            server.help().unwrap(),
            "help: teksilo-automation-mcp 0.15.0 is older than Teksilo 0.15.1; \
             `cargo install teksilo-automation-mcp --version 0.15.1 --locked` updates it"
        );
    }

    #[test]
    fn another_minor_or_major_names_the_install_command() {
        for version in ["0.14.3", "0.16.0", "1.15.1"] {
            let server = mcp("0.15.1", Some(version));
            assert_eq!(server.state(), "mismatch", "{version}");
            assert_eq!(
                server.help().unwrap(),
                "help: the teksilo-automation-mcp on your PATH was built from another \
                 release; `cargo install teksilo-automation-mcp --version 0.15.1 --locked` \
                 replaces it"
            );
        }
    }

    #[test]
    fn the_formatter_is_checked_under_its_own_name() {
        let fmt = inspect(
            FMT,
            "0.15.1",
            || Some(PathBuf::from("/nowhere/cargo-teksilo-fmt")),
            |_| Some("0.15.0".into()),
        );
        assert!(
            fmt.help()
                .unwrap()
                .contains("`cargo install cargo-teksilo-fmt --version 0.15.1 --locked`")
        );
    }

    #[test]
    fn an_unreadable_version_is_unknown_not_a_mismatch() {
        assert_eq!(mcp("0.15.1", None).state, State::Unknown);
        assert_eq!(mcp("0.15.1", Some("garbage")).state, State::Unknown);
        assert_eq!(mcp("unknown", Some("0.15.1")).state, State::Unknown);
        assert!(mcp("0.15.1", None).help().unwrap().contains("replaces it"));
    }

    #[test]
    fn the_json_field_carries_state_version_and_expected() {
        assert_eq!(
            mcp("0.15.1", Some("0.14.0")).json(),
            serde_json::json!({"state":"mismatch","version":"0.14.0","expected":"0.15.1"})
        );
        assert_eq!(
            inspect(MCP, "0.15.1", || None, |_| None).json(),
            serde_json::json!({"state":"missing","version":null,"expected":"0.15.1"})
        );
    }

    #[test]
    fn clap_version_output_parses() {
        assert_eq!(
            parse_version("teksilo-automation-mcp 0.15.1\n").as_deref(),
            Some("0.15.1")
        );
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("usage: nothing"), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_server_that_hangs_is_given_up_on() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("hang");
        std::fs::write(&script, "#!/bin/sh\nexec sleep 30\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let started = Instant::now();
        assert_eq!(run_version(&script, Duration::from_millis(200)), None);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[test]
    fn a_server_that_answers_is_read() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("answer");
        std::fs::write(&script, "#!/bin/sh\necho 'teksilo-automation-mcp 0.15.1'\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            run_version(&script, Duration::from_secs(5)).as_deref(),
            Some("0.15.1")
        );
    }
}
