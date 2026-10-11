// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The `teksilo-automation-mcp` on the user's PATH, and whether it matches.
//!
//! A server built from another release "will answer confidently about tools
//! and semantics the app does not have", so `status` says which one an agent
//! would be handed. The PATH lookup and the version query are injected into
//! [`inspect`], so a test never depends on the machine it runs on.

use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// The binary name `cargo install teksilo-automation-mcp` produces.
pub const BINARY: &str = "teksilo-automation-mcp";

/// How long `--version` may take before the server is reported as unreadable.
const VERSION_TIMEOUT: Duration = Duration::from_secs(2);

/// What `status` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Server {
    /// Nothing named [`BINARY`] on PATH.
    NotInstalled,
    /// On PATH, same major and minor as the app (a patch difference is fine).
    Matching { version: String },
    /// On PATH, but built from another minor or major release.
    Mismatch { version: String, expected: String },
    /// On PATH, but `--version` failed, timed out or printed nothing usable.
    Unknown,
}

impl Server {
    /// The `state` of the JSON field.
    pub fn state(&self) -> &'static str {
        match self {
            Server::NotInstalled => "missing",
            Server::Matching { .. } => "matching",
            Server::Mismatch { .. } => "mismatch",
            Server::Unknown => "unknown",
        }
    }

    /// The version the server reported, when it did.
    pub fn version(&self) -> Option<&str> {
        match self {
            Server::Matching { version } | Server::Mismatch { version, .. } => Some(version),
            _ => None,
        }
    }

    /// The text after the `MCP` label of the human report.
    pub fn line(&self) -> String {
        match self {
            Server::NotInstalled => "not installed".into(),
            Server::Matching { version } => format!("{version} (matches)"),
            Server::Mismatch { version, expected } => {
                format!("{version} (differs from {expected})")
            }
            Server::Unknown => "installed (version unreadable)".into(),
        }
    }

    /// The remedy, only for a mismatch.
    pub fn help(&self) -> Option<String> {
        match self {
            Server::Mismatch { expected, .. } => Some(format!(
                "help: the {BINARY} on your PATH was built from another release; \
                 `cargo install {BINARY} --version {expected}` replaces it"
            )),
            _ => None,
        }
    }

    /// The JSON value of the report's `automation_server` field.
    pub fn json(&self, expected: &str) -> serde_json::Value {
        serde_json::json!({
            "state": self.state(),
            "version": self.version(),
            "expected": expected,
        })
    }
}

/// Classify the server against `expected`, the version the app resolved.
///
/// `locate` finds the binary and `version_of` asks it for its version; the
/// latter is called only when the former found something.
pub fn inspect(
    expected: &str,
    locate: impl FnOnce() -> Option<PathBuf>,
    version_of: impl FnOnce(&Path) -> Option<String>,
) -> Server {
    let Some(path) = locate() else {
        return Server::NotInstalled;
    };
    let Some(version) = version_of(&path) else {
        return Server::Unknown;
    };
    match (
        crate::guard::semver_parts(&version),
        crate::guard::semver_parts(expected),
    ) {
        (Some((a, b, _)), Some((c, d, _))) if (a, b) == (c, d) => Server::Matching { version },
        (Some(_), Some(_)) => Server::Mismatch {
            version,
            expected: expected.to_string(),
        },
        _ => Server::Unknown,
    }
}

/// [`inspect`] against the real PATH.
pub fn inspect_path(expected: &str) -> Server {
    inspect(
        expected,
        || crate::symbol::which(BINARY),
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

    #[test]
    fn a_missing_binary_is_not_queried() {
        let server = inspect(
            "0.15.1",
            || None,
            |_| panic!("the version of a binary that is not there"),
        );
        assert_eq!(server, Server::NotInstalled);
        assert_eq!(server.line(), "not installed");
        assert!(server.help().is_none());
    }

    #[test]
    fn the_same_release_matches_and_a_patch_difference_does_too() {
        for version in ["0.15.1", "0.15.0", "0.15.9"] {
            let server = inspect("0.15.1", found, |_| Some(version.into()));
            assert_eq!(
                server,
                Server::Matching {
                    version: version.into()
                }
            );
            assert!(server.help().is_none());
        }
    }

    #[test]
    fn another_minor_or_major_names_the_install_command() {
        for version in ["0.14.3", "0.16.0", "1.15.1"] {
            let server = inspect("0.15.1", found, |_| Some(version.into()));
            assert_eq!(server.state(), "mismatch", "{version}");
            assert_eq!(
                server.help().unwrap(),
                "help: the teksilo-automation-mcp on your PATH was built from another \
                 release; `cargo install teksilo-automation-mcp --version 0.15.1` replaces it"
            );
        }
    }

    #[test]
    fn an_unreadable_version_is_unknown_not_a_mismatch() {
        assert_eq!(inspect("0.15.1", found, |_| None), Server::Unknown);
        assert_eq!(
            inspect("0.15.1", found, |_| Some("garbage".into())),
            Server::Unknown
        );
        assert_eq!(
            inspect("unknown", found, |_| Some("0.15.1".into())),
            Server::Unknown
        );
    }

    #[test]
    fn the_json_field_carries_state_version_and_expected() {
        let server = Server::Mismatch {
            version: "0.14.0".into(),
            expected: "0.15.1".into(),
        };
        assert_eq!(
            server.json("0.15.1"),
            serde_json::json!({"state":"mismatch","version":"0.14.0","expected":"0.15.1"})
        );
        assert_eq!(
            Server::NotInstalled.json("0.15.1"),
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
