// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! One running instance per user: a second launch hands its command line to
//! the running application and exits.
//!
//! Call [`acquire`] in `main`, before building anything. A
//! [`Secondary`](Election::Secondary) forwards its command line
//! ([`SecondaryInstance::forward_command_line`]) and exits, without opening
//! a window or touching the settings. A [`Primary`](Election::Primary) hands
//! its [`PrimaryInstance`] to
//! [`TeksiloAppBuilder::single_instance`](crate::TeksiloAppBuilder::single_instance)
//! and runs; each later launch then reaches its
//! [`on_external_with_ctx`](crate::TeksiloAppBuilder::on_external_with_ctx)
//! handler as an [`InstanceMessage`] payload. `docs/multi-window.md`,
//! "Single instance", has the whole `main`.
//!
//! An application that offers a way to start a separate instance (a
//! `--new-instance` option, say) does not call [`acquire`] for it; nothing
//! here reads the command line.
//!
//! Same user only, and an [`InstanceMessage`] is untrusted input: any
//! process of the user can send anything. See
//! [`teksilo_platform::single_instance`] for the election, its bounds and
//! where it works (Unix and Windows).

use std::io;

use teksilo_settings::AppPaths;

pub use teksilo_platform::single_instance::{
    Election, InstanceMessage, PrimaryInstance, SecondaryInstance,
};

use crate::WithActivationToken;

/// Hold the single-instance election for `key` in the application's runtime
/// directory ([`AppPaths::create_runtime_dir`], which this creates private to
/// the user), or, on Windows, for its data directory
/// ([`AppPaths::data_dir`]).
///
/// `key` names the instance, one to 64 characters from `A`-`Z`, `a`-`z`,
/// `0`-`9`, `.`, `_` and `-`; the application's name will do, or a name
/// made from something more (a hash of a data directory, for one instance
/// per data directory).
///
/// On Windows the election is a named pipe, which needs no directory: the
/// data directory's path only goes into the pipe's name, so two installations
/// with their own data directories (a portable one, say) are two
/// applications. Nothing is created there.
///
/// Elsewhere it fails with [`io::ErrorKind::Unsupported`] when `paths` has
/// no runtime directory (on a Unix without `$XDG_RUNTIME_DIR`, and for
/// [`AppPaths::from_dirs`]): call
/// [`teksilo_platform::single_instance::acquire`] with a directory of your
/// choosing there, one of the user's own closed to everyone else. The other
/// failures are those of that function.
pub fn acquire(paths: &AppPaths, key: &str) -> io::Result<Election> {
    #[cfg(windows)]
    {
        teksilo_platform::single_instance::acquire(paths.data_dir(), key)
    }
    #[cfg(not(windows))]
    {
        let Some(dir) = paths.create_runtime_dir()? else {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "these application paths have no runtime directory to hold the \
                 single-instance election in",
            ));
        };
        teksilo_platform::single_instance::acquire(dir, key)
    }
}

/// What the event loop is posted for `message`: the message itself, wrapped
/// in a [`WithActivationToken`] when it carries a token, so the token goes to
/// the first window the handler raises or opens.
pub(crate) fn payload(message: InstanceMessage) -> Box<dyn std::any::Any + Send> {
    match message.activation_token.clone() {
        Some(token) => Box::new(WithActivationToken::new(token, message)),
        None => Box::new(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::Path;
    use std::sync::mpsc;
    use std::time::Duration;

    fn primary(election: Election) -> PrimaryInstance {
        match election {
            Election::Primary(p) => p,
            Election::Secondary(_) => panic!("expected to be the primary"),
        }
    }

    fn secondary(election: Election) -> SecondaryInstance {
        match election {
            Election::Secondary(s) => s,
            Election::Primary(_) => panic!("expected to be a secondary"),
        }
    }

    /// What `TeksiloAppBuilder::run` posts, end to end from a second launch:
    /// the message, wrapped with its token when it has one.
    #[cfg(any(unix, windows))]
    #[test]
    fn a_second_launch_arrives_as_the_payload_the_handler_downcasts() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::for_testing(root.path());
        let mut first = primary(acquire(&paths, "app").unwrap());
        let (tx, rx) = mpsc::channel();
        first
            .listen(move |message| {
                let _ = tx.send(payload(message));
            })
            .unwrap();

        let args = || vec![OsString::from("app"), OsString::from("doc.txt")];
        secondary(acquire(&paths, "app").unwrap())
            .forward(args(), Path::new("/work"), None)
            .unwrap();
        let posted = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let message = posted
            .downcast_ref::<InstanceMessage>()
            .expect("no token: the message itself");
        assert_eq!(message.args, args());
        assert_eq!(message.cwd, Path::new("/work"));

        secondary(acquire(&paths, "app").unwrap())
            .forward(args(), Path::new("/work"), Some("tok"))
            .unwrap();
        let posted = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let wrapped = posted
            .downcast_ref::<WithActivationToken>()
            .expect("a token: wrapped with it");
        assert_eq!(wrapped.token(), "tok");
        let message = wrapped
            .payload()
            .downcast_ref::<InstanceMessage>()
            .expect("the message inside");
        assert_eq!(message.args, args());
    }

    #[cfg(not(windows))]
    #[test]
    fn paths_without_a_runtime_directory_are_unsupported() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_dirs(root.path().join("c"), root.path().join("d"));
        let err = acquire(&paths, "app").expect_err("no runtime directory");
        assert_eq!(err.kind(), io::ErrorKind::Unsupported);
    }

    /// On Windows the data directory names the election, and is not created.
    #[cfg(windows)]
    #[test]
    fn on_windows_the_data_directory_names_the_election() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("d");
        let paths = AppPaths::from_dirs(root.path().join("c"), data.clone());
        let _first = primary(acquire(&paths, "app").unwrap());
        let _second = secondary(acquire(&paths, "app").unwrap());
        let other = AppPaths::from_dirs(root.path().join("c"), root.path().join("e"));
        let _other = primary(acquire(&other, "app").unwrap());
        assert!(!data.exists());
    }

    /// The builder holds the instance it is given, and a headless app, which
    /// serves nothing, releases it.
    #[cfg(any(unix, windows))]
    #[test]
    fn the_builder_holds_the_primary_until_it_is_dropped() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::for_testing(root.path());
        let builder = crate::TeksiloAppBuilder::new()
            .single_instance(primary(acquire(&paths, "app").unwrap()));
        let _second = secondary(acquire(&paths, "app").unwrap());
        drop(builder.build_headless());
        let _again = primary(acquire(&paths, "app").unwrap());
    }
}
