// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Finding the MCP client, and saying how to get it when it is missing.
//!
//! **An app needs nothing installed to be automatable.** The bridge is compiled
//! into the debug build and binds its own endpoint. `teksilo-automation-mcp` is
//! the *client* half: it turns MCP-over-stdio, which an agent speaks, into the
//! framed protocol in [`wire`](crate::wire).
//!
//! That asymmetry is easy to get wrong from the outside, and every harness that
//! got it wrong wrote its own version of this: where to look, which version to
//! ask for, and what to print when the answer is "nowhere". Skribisto's probe
//! harness carried all three, including an explanation of *this* crate's
//! protocol history, which is not an application's business to know. So the
//! knowledge lives here, next to the bridge that needs the client, and the
//! app's own announce carries it to whoever is reading the log.
//!
//! Pure `std`: a `PATH` walk and some string formatting. Nothing here spawns a
//! process, because the only caller is on an app's startup path.

use std::path::{Path, PathBuf};

/// The client executable's name, without any platform extension.
pub const CLIENT_BIN: &str = "teksilo-automation-mcp";

/// The version of the client that matches this toolkit.
///
/// The whole workspace shares one version (`release.toml`), so the client built
/// from the same tag as this crate is the one that speaks its protocol. That
/// matters more than it looks: the framing is not frozen, and 0.9.3 replaced
/// the socket announce with an endpoint descriptor and bounded the token
/// handshake. A client older than the app fails at *connect* time, with a
/// symptom naming neither version.
pub const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The file name to look for, `.exe` included where the platform wants it.
pub fn client_file_name() -> String {
    if cfg!(windows) {
        format!("{CLIENT_BIN}.exe")
    } else {
        CLIENT_BIN.to_string()
    }
}

/// The command that installs a matching client.
///
/// `--locked` on purpose: the client and the app exchange framed JSON, and a
/// dependency resolved differently on the two sides is one more way for them to
/// disagree about the wire.
pub fn install_command() -> String {
    format!("cargo install {CLIENT_BIN} --version {CLIENT_VERSION} --locked")
}

/// Look for the client on `$PATH`. `None` means "not installed".
///
/// A plain `PATH` walk rather than a crate: this is four lines of `std`, and
/// [`teksilo-automation`](crate) is deliberately dependency-light so a CI
/// harness, a headless test and the in-app bridge can all share it.
pub fn find_client() -> Option<PathBuf> {
    find_client_in(std::env::var_os("PATH").as_deref())
}

/// [`find_client`], with the search path handed in, so it is testable.
pub fn find_client_in(path: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let name = client_file_name();
    std::env::split_paths(path?)
        .map(|dir| dir.join(&name))
        .find(|candidate| is_executable_file(candidate))
}

/// A real, runnable file. Split per platform rather than branched inside one
/// body: the permission bit only exists on Unix, and one `cfg` per function
/// keeps each arm a plain expression.
#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.metadata().is_ok_and(|meta| meta.is_file())
}

/// What to print after the bridge has announced its endpoint.
///
/// One line when the client is installed, and when it is not, three more that
/// answer the question the first line has just raised. The absence is stated
/// where it is discovered, rather than left to surface later as a connection
/// that never happens.
pub fn attach_hint(pid: u32, endpoint: &str, token: &str) -> String {
    let mut hint = format!(
        "attach with `{CLIENT_BIN} --attach-pid {pid}` \
         (or --connect {endpoint} --token {token})"
    );
    if find_client().is_none() {
        hint.push_str(&format!(
            "\nteksilo-automation: …but `{CLIENT_BIN}` is not on $PATH. \
             This app needs nothing installed;\nteksilo-automation: \
             that binary is only the client an agent drives it through. \
             Install it with:\nteksilo-automation:     {}",
            install_command()
        ));
    }
    hint
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn install_command_names_this_workspace_version() {
        let cmd = install_command();
        assert!(cmd.contains(CLIENT_BIN), "{cmd}");
        assert!(cmd.contains(CLIENT_VERSION), "{cmd}");
        assert!(cmd.contains("--locked"), "{cmd}");
    }

    #[test]
    fn file_name_carries_the_platform_extension() {
        let name = client_file_name();
        assert_eq!(name.ends_with(".exe"), cfg!(windows), "{name}");
    }

    #[test]
    fn an_empty_path_finds_nothing() {
        assert!(find_client_in(Some(&OsString::from(""))).is_none());
        assert!(find_client_in(None).is_none());
    }

    #[test]
    fn a_directory_named_like_the_client_is_not_the_client() {
        // The check is `is_file` + the executable bit, not "the name exists":
        // a directory on `$PATH` called `teksilo-automation-mcp` would
        // otherwise be reported as an installed client, and the hint would
        // then tell a reader to run something that cannot run.
        let dir = std::env::temp_dir().join(format!("tk-client-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let masquerading = dir.join(client_file_name());
        std::fs::create_dir_all(&masquerading).expect("temp dir");
        let found = find_client_in(Some(&OsString::from(dir.as_os_str())));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(found.is_none(), "a directory was reported as the client");
    }

    #[test]
    fn the_hint_always_names_both_ways_in() {
        let hint = attach_hint(4321, "/run/x.sock", "tok-1");
        assert!(hint.contains("--attach-pid 4321"), "{hint}");
        assert!(
            hint.contains("--connect /run/x.sock --token tok-1"),
            "{hint}"
        );
    }
}
