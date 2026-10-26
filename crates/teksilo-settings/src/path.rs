// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! OS-correct path resolution for application config and data directories.
//!
//! [`AppPaths`] is the rest of the crate's single point of truth for where
//! settings files live. In production, [`AppPaths::new`] asks the OS, through
//! `etcetera`; in tests, [`AppPaths::for_testing`] roots everything inside a
//! `tempdir` so no test ever touches the user's real config tree.
//!
//! | Platform | Config directory | Data directory |
//! |---|---|---|
//! | Linux and other Unix | `$XDG_CONFIG_HOME/<app>`, by default `~/.config/<app>` | `$XDG_DATA_HOME/<app>`, by default `~/.local/share/<app>` |
//! | Windows | `%APPDATA%\<Organization>\<Application>\config` | `%APPDATA%\<Organization>\<Application>\data` |
//! | macOS | `~/Library/Application Support/<bundle id>/config` | `~/Library/Application Support/<bundle id>/data` |
//!
//! `<app>` is the application's name in lower case, and the bundle id is
//! `<qualifier>.<organization in lower case>.<Application>`, spaces made
//! hyphens in both: `AppPaths::new("eu", "FernTech", "Skribisto")` gives
//! `~/.config/skribisto` and `~/Library/Application Support/eu.ferntech.Skribisto`.
//!
//! For sockets and lock files, [`AppPaths::runtime_dir`] gives a short,
//! per-user directory emptied at logout or restart, `$XDG_RUNTIME_DIR/<app>`
//! on Linux; its documentation says where it is on each platform.
//!
//! ## The move to Application Support on macOS
//!
//! Up to 0.21.0 a macOS application's config and data went to the Linux
//! directories, `~/.config/<app>` and `~/.local/share/<app>`. On macOS,
//! [`AppPaths::new`] brings each of those it finds to its new place, once:
//! only while the new directory does not exist yet. It is a rename where both
//! are on one volume, and otherwise a copy that leaves the old directory
//! where it was. A move that fails is reported on standard error and leaves
//! the old directory as it was, to be tried again at the next start.
//!
//! ## Usage
//!
//! Pass an `AppPaths` instance to [`SettingsBundle`](crate::SettingsBundle),
//! [`SettingsStore`](crate::SettingsStore), or [`MruList`](crate::MruList); the
//! individual files within it are addressed by name via
//! [`config_file`](AppPaths::config_file) and
//! [`data_file`](AppPaths::data_file).
//!
//! ```ignore
//! use teksilo_settings::AppPaths;
//!
//! // Production: returns None when no home directory is detectable.
//! if let Some(paths) = AppPaths::new("eu", "FernTech", "MyApp") {
//!     let general_toml = paths.config_file("general");
//!     let cache_toml   = paths.data_file("cache");
//! }
//!
//! // Tests: deterministic, tempdir-rooted, never touches user files.
//! let tmp = tempfile::tempdir().unwrap();
//! let paths = AppPaths::for_testing(tmp.path());
//! assert_eq!(paths.config_file("settings"), tmp.path().join("settings.toml"));
//! ```

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use etcetera::{AppStrategy, AppStrategyArgs};

/// Resolved OS-correct application directories (config, data, and the
/// runtime directory for sockets and locks).
///
/// Construct with [`AppPaths::new`] for production code, or
/// [`AppPaths::for_testing`] in tests and headless CI environments.
/// Use [`AppPaths::from_dirs`] when the application manages its own
/// directory layout (e.g. portable mode).
#[derive(Debug, Clone)]
pub struct AppPaths {
    config_dir: PathBuf,
    data_dir: PathBuf,
    runtime_dir: Option<PathBuf>,
}

impl AppPaths {
    /// Resolve directories from the OS; the module documentation says where
    /// they are on each platform. The `(qualifier, organization, application)`
    /// triple feeds [`etcetera::AppStrategyArgs`] as `(top_level_domain,
    /// author, app_name)`, the same fields under other names.
    ///
    /// On macOS it first moves the directories it gave up to 0.21.0, under
    /// `~/.config` and `~/.local/share`, to Application Support, when they
    /// are there and their new places are not.
    ///
    /// Returns `None` when no usable home directory could be detected
    /// (a sandboxed or unconfigured environment). Callers who want to
    /// degrade gracefully should fall back to [`AppPaths::for_testing`]
    /// with an in-process directory.
    pub fn new(qualifier: &str, organization: &str, application: &str) -> Option<Self> {
        Self::from_strategy(AppStrategyArgs {
            top_level_domain: qualifier.to_string(),
            author: organization.to_string(),
            app_name: application.to_string(),
        })
    }

    #[cfg(not(target_os = "macos"))]
    fn from_strategy(args: AppStrategyArgs) -> Option<Self> {
        let strategy = etcetera::choose_app_strategy(args).ok()?;
        Some(Self {
            config_dir: strategy.config_dir(),
            data_dir: strategy.data_dir(),
            // `$XDG_RUNTIME_DIR/<app>` (when set and absolute), none on Windows.
            runtime_dir: strategy.runtime_dir(),
        })
    }

    /// Application Support, where macOS keeps an application's own files, and
    /// not `Library/Preferences`, which it keeps for the preference files its
    /// `cfprefsd` manages. Config and data are apart, as on Windows: an
    /// application may name a config file and a data file alike.
    #[cfg(target_os = "macos")]
    fn from_strategy(args: AppStrategyArgs) -> Option<Self> {
        let root = etcetera::app_strategy::Apple::new(args.clone())
            .ok()?
            .data_dir();
        let runtime_dir = darwin_user_temp_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join(args.bundle_id());
        let paths = Self {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            runtime_dir: Some(runtime_dir),
        };
        // Where `etcetera::choose_app_strategy` put them on macOS up to 0.21.0.
        if let Ok(old) = etcetera::app_strategy::Xdg::new(args) {
            legacy::adopt(&old.config_dir(), &paths.config_dir);
            legacy::adopt(&old.data_dir(), &paths.data_dir);
        }
        Some(paths)
    }

    /// Construct an `AppPaths` rooted at an arbitrary directory. Used by
    /// tests so that no test ever touches the user's real config tree.
    /// `config_dir`, `data_dir` and `runtime_dir` all resolve to `root`.
    pub fn for_testing(root: &Path) -> Self {
        Self {
            config_dir: root.to_path_buf(),
            data_dir: root.to_path_buf(),
            runtime_dir: Some(root.to_path_buf()),
        }
    }

    /// Construct from explicit config and data directories. Useful when
    /// an application wants to override one or both (e.g. portable mode).
    /// It has no [`runtime_dir`](Self::runtime_dir).
    pub fn from_dirs(config_dir: PathBuf, data_dir: PathBuf) -> Self {
        Self {
            config_dir,
            data_dir,
            runtime_dir: None,
        }
    }

    /// The platform-correct config directory; the module documentation says
    /// where it is on each platform.
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// The platform-correct data directory. Used for caches and
    /// per-window state — anything larger than a configuration file.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Resolve a per-concern config file by name (without extension).
    /// `name = "general"` yields `<config_dir>/general.toml`.
    pub fn config_file(&self, name: &str) -> PathBuf {
        self.config_dir.join(format!("{name}.toml"))
    }

    /// Resolve a per-concern data file by name (without extension).
    pub fn data_file(&self, name: &str) -> PathBuf {
        self.data_dir.join(format!("{name}.toml"))
    }

    /// A short, per-user directory for the application's sockets and lock
    /// files, what `$XDG_RUNTIME_DIR` is on Linux. `None` where the platform
    /// gives none: on Windows, where a named pipe needs no directory, and on
    /// a Unix without `$XDG_RUNTIME_DIR`. It is not created here:
    /// [`create_runtime_dir`](Self::create_runtime_dir) does that, private to
    /// the user.
    ///
    /// | Platform | Runtime directory |
    /// |---|---|
    /// | Linux and other Unix | `$XDG_RUNTIME_DIR/<app>`, usually `/run/user/<uid>/<app>` |
    /// | macOS | `<per-user temporary directory>/<bundle id>`, the directory under `/var/folders/` |
    /// | Windows | none |
    ///
    /// Short, because a Unix socket's path has to be: macOS takes 104 bytes
    /// of it with the closing NUL, Linux 108, and `bind` fails beyond that.
    /// The macOS per-user temporary directory is about 49 bytes, while the
    /// data directory under Application Support is over 80 before the user's
    /// name is counted. On macOS it is read from the system
    /// (`confstr(_CS_DARWIN_USER_TEMP_DIR)`), not from `$TMPDIR`, which a
    /// shell can point at the shared `/tmp`.
    ///
    /// What is left there goes when the user logs out or the machine
    /// restarts, and the system may clean up what sits unused for days, so
    /// make a socket or a lock afresh at each start.
    pub fn runtime_dir(&self) -> Option<&Path> {
        self.runtime_dir.as_deref()
    }

    /// Create [`runtime_dir`](Self::runtime_dir) if it does not exist, private
    /// to the user, and return it; `Ok(None)` where there is none. On Unix it
    /// is made `0700`, and one that exists already is taken only when it is a
    /// directory, not a symbolic link, that the user owns; it is made `0700`
    /// if it is not. The directory it goes in must exist, as
    /// `$XDG_RUNTIME_DIR` and the macOS per-user temporary directory do.
    pub fn create_runtime_dir(&self) -> io::Result<Option<&Path>> {
        let Some(dir) = self.runtime_dir.as_deref() else {
            return Ok(None);
        };
        create_private_dir(dir)?;
        Ok(Some(dir))
    }
}

/// The per-user temporary directory macOS gives each user, under
/// `/var/folders/`.
#[cfg(target_os = "macos")]
fn darwin_user_temp_dir() -> Option<PathBuf> {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let mut buf = vec![0u8; 1024];
    // SAFETY: `buf` is writable for the length passed; `confstr` writes at
    // most that many bytes, the closing NUL included.
    let len = unsafe {
        libc::confstr(
            libc::_CS_DARWIN_USER_TEMP_DIR,
            buf.as_mut_ptr().cast(),
            buf.len(),
        )
    };
    // 0 is an error; a length past the buffer means it did not fit.
    if len == 0 || len > buf.len() {
        return None;
    }
    buf.truncate(len - 1);
    Some(PathBuf::from(OsStr::from_bytes(&buf)))
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

    match fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => return Ok(()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    // `symlink_metadata`: a link to a directory the user owns is still not a
    // directory of theirs, and what it points at could change.
    let meta = fs::symlink_metadata(dir)?;
    if !meta.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} exists and is not a directory", dir.display()),
        ));
    }
    // SAFETY: `geteuid` takes nothing and cannot fail.
    if meta.uid() != unsafe { libc::geteuid() } {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} belongs to another user", dir.display()),
        ));
    }
    if meta.permissions().mode() & 0o077 != 0 {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)
}

/// Bringing a macOS application's directories from where [`AppPaths::new`]
/// put them up to 0.21.0 to where it puts them now. Unix-only code, built on
/// macOS and in the tests of every Unix.
#[cfg(any(target_os = "macos", all(test, unix)))]
mod legacy {
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};

    /// What [`adopt`] did.
    #[derive(Debug, PartialEq, Eq)]
    pub(super) enum Adopted {
        /// Nothing to bring: `new` exists, or `old` is not a directory.
        Nothing,
        /// `old` was renamed to `new`.
        Moved,
        /// `old` was copied to `new` and left in place.
        Copied,
    }

    /// Bring the directory `old` to `new` when `new` does not exist yet, and
    /// say so on standard error. A failure is reported there too, and leaves
    /// `old` as it was and `new` absent, so the next start tries again.
    pub(super) fn adopt(old: &Path, new: &Path) -> Adopted {
        match try_adopt(old, new) {
            Ok(Adopted::Nothing) => Adopted::Nothing,
            Ok(Adopted::Moved) => {
                eprintln!(
                    "teksilo-settings: moved {} to {}",
                    old.display(),
                    new.display()
                );
                Adopted::Moved
            }
            Ok(Adopted::Copied) => {
                eprintln!(
                    "teksilo-settings: copied {} to {}; the old directory is left in place",
                    old.display(),
                    new.display()
                );
                Adopted::Copied
            }
            Err(e) => {
                eprintln!(
                    "teksilo-settings: could not move {} to {} ({e}); it stays where it was \
                     and is tried again at the next start",
                    old.display(),
                    new.display()
                );
                Adopted::Nothing
            }
        }
    }

    fn try_adopt(old: &Path, new: &Path) -> io::Result<Adopted> {
        if exists(new) || !old.is_dir() {
            return Ok(Adopted::Nothing);
        }
        if let Some(parent) = new.parent() {
            fs::create_dir_all(parent)?;
        }
        if fs::rename(old, new).is_ok() {
            return Ok(Adopted::Moved);
        }
        // Another process of the application may have got there first.
        if exists(new) || !old.is_dir() {
            return Ok(Adopted::Nothing);
        }
        // Another volume (`$XDG_CONFIG_HOME` may point anywhere).
        copy_into_place(old, new)
    }

    /// Copy `old` beside `new` and rename the copy into place, so nothing ever
    /// finds half a directory at `new`.
    pub(super) fn copy_into_place(old: &Path, new: &Path) -> io::Result<Adopted> {
        let staging = staging_path(new);
        let _ = fs::remove_dir_all(&staging);
        let copied = copy_dir(old, &staging).and_then(|()| fs::rename(&staging, new));
        match copied {
            Ok(()) => Ok(Adopted::Copied),
            Err(e) => {
                let _ = fs::remove_dir_all(&staging);
                if exists(new) {
                    Ok(Adopted::Nothing)
                } else {
                    Err(e)
                }
            }
        }
    }

    fn staging_path(new: &Path) -> PathBuf {
        let name = new
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        new.with_file_name(format!("{name}.moving-{}", std::process::id()))
    }

    /// Directories, regular files and symbolic links. A socket or a pipe left
    /// behind is no file to keep and cannot be copied, so it is left out.
    fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
        fs::create_dir(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let target = to.join(entry.file_name());
            if kind.is_dir() {
                copy_dir(&entry.path(), &target)?;
            } else if kind.is_symlink() {
                std::os::unix::fs::symlink(fs::read_link(entry.path())?, &target)?;
            } else if kind.is_file() {
                fs::copy(entry.path(), &target)?;
            }
        }
        Ok(())
    }

    fn exists(path: &Path) -> bool {
        path.symlink_metadata().is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn for_testing_routes_everything_to_root() {
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_testing(dir.path());

        assert_eq!(paths.config_dir(), dir.path());
        assert_eq!(paths.data_dir(), dir.path());
        assert_eq!(
            paths.config_file("recents"),
            dir.path().join("recents.toml")
        );
        assert_eq!(paths.data_file("cache"), dir.path().join("cache.toml"));
        assert_eq!(paths.runtime_dir(), Some(dir.path()));
    }

    #[test]
    fn from_dirs_keeps_separate_paths() {
        let cfg = tempdir().unwrap();
        let data = tempdir().unwrap();
        let paths = AppPaths::from_dirs(cfg.path().to_path_buf(), data.path().to_path_buf());

        assert_eq!(paths.config_dir(), cfg.path());
        assert_eq!(paths.data_dir(), data.path());
        assert_eq!(paths.runtime_dir(), None);
        assert_eq!(paths.create_runtime_dir().unwrap(), None);
    }

    /// On Linux the runtime directory is the application's own under
    /// `$XDG_RUNTIME_DIR`, and there is none without it.
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn the_runtime_directory_is_the_applications_own_under_xdg_runtime_dir() {
        let paths = AppPaths::new("eu", "FernTech", "Test App").unwrap();
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|base| base.is_absolute());
        assert_eq!(
            paths.runtime_dir(),
            base.map(|base| base.join("test-app")).as_deref()
        );
    }

    #[cfg(unix)]
    mod runtime_directory {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::path::Path;

        use super::super::AppPaths;

        fn at(dir: &Path) -> AppPaths {
            AppPaths {
                config_dir: dir.to_path_buf(),
                data_dir: dir.to_path_buf(),
                runtime_dir: Some(dir.to_path_buf()),
            }
        }

        fn mode(path: &Path) -> u32 {
            fs::metadata(path).unwrap().permissions().mode() & 0o777
        }

        #[test]
        fn it_is_made_private_to_the_user() {
            let tmp = tempfile::tempdir().unwrap();
            let dir = tmp.path().join("test-app");
            assert_eq!(at(&dir).create_runtime_dir().unwrap(), Some(dir.as_path()));
            assert_eq!(mode(&dir), 0o700);
        }

        /// One left open by an earlier `create_dir_all` (the umask's `0755`)
        /// is the user's own, and is closed.
        #[test]
        fn an_open_one_of_the_users_is_closed() {
            let tmp = tempfile::tempdir().unwrap();
            let dir = tmp.path().join("test-app");
            fs::create_dir(&dir).unwrap();
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
            assert_eq!(at(&dir).create_runtime_dir().unwrap(), Some(dir.as_path()));
            assert_eq!(mode(&dir), 0o700);
        }

        /// A link, even to a directory of the user's, and a file are refused:
        /// a socket made there would not be where the user's own directory
        /// keeps it.
        #[test]
        fn a_symbolic_link_or_a_file_is_refused() {
            let tmp = tempfile::tempdir().unwrap();
            let target = tmp.path().join("elsewhere");
            fs::create_dir(&target).unwrap();
            let link = tmp.path().join("test-app");
            std::os::unix::fs::symlink(&target, &link).unwrap();
            assert!(at(&link).create_runtime_dir().is_err());
            let file = tmp.path().join("a-file");
            fs::write(&file, "").unwrap();
            assert!(at(&file).create_runtime_dir().is_err());
        }
    }

    /// The move to Application Support: an old directory, with what an
    /// application keeps in it, arrives whole at its new place.
    #[cfg(unix)]
    mod move_to_application_support {
        use std::fs;
        use std::path::{Path, PathBuf};

        use super::super::legacy::{Adopted, adopt, copy_into_place};

        fn places(root: &Path) -> (PathBuf, PathBuf) {
            (
                root.join(".config/skribisto"),
                root.join("Library/Application Support/eu.ferntech.Skribisto/config"),
            )
        }

        fn fill(dir: &Path) {
            fs::create_dir_all(dir.join("nested")).unwrap();
            fs::write(dir.join("general.toml"), "a = 1\n").unwrap();
            fs::write(dir.join("nested/recent.toml"), "b = 2\n").unwrap();
            std::os::unix::fs::symlink("general.toml", dir.join("link.toml")).unwrap();
        }

        fn assert_filled(dir: &Path) {
            assert_eq!(
                fs::read_to_string(dir.join("general.toml")).unwrap(),
                "a = 1\n"
            );
            assert_eq!(
                fs::read_to_string(dir.join("nested/recent.toml")).unwrap(),
                "b = 2\n"
            );
            assert_eq!(
                fs::read_link(dir.join("link.toml")).unwrap(),
                Path::new("general.toml")
            );
        }

        #[test]
        fn an_old_directory_is_moved_to_its_new_place() {
            let tmp = tempfile::tempdir().unwrap();
            let (old, new) = places(tmp.path());
            fill(&old);
            assert_eq!(adopt(&old, &new), Adopted::Moved);
            assert_filled(&new);
            assert!(!old.exists());
        }

        /// Moved once already, or written by this version before: the new
        /// place wins, and the old directory is not touched.
        #[test]
        fn a_new_place_that_exists_is_left_alone() {
            let tmp = tempfile::tempdir().unwrap();
            let (old, new) = places(tmp.path());
            fill(&old);
            fs::create_dir_all(&new).unwrap();
            fs::write(new.join("general.toml"), "a = 9\n").unwrap();
            assert_eq!(adopt(&old, &new), Adopted::Nothing);
            assert_eq!(
                fs::read_to_string(new.join("general.toml")).unwrap(),
                "a = 9\n"
            );
            assert_filled(&old);
        }

        #[test]
        fn nothing_is_made_without_an_old_directory() {
            let tmp = tempfile::tempdir().unwrap();
            let (old, new) = places(tmp.path());
            assert_eq!(adopt(&old, &new), Adopted::Nothing);
            assert!(!new.exists());
        }

        /// Where the rename fails (another volume), a copy goes into place
        /// whole and the old directory stays. A socket left behind is no file
        /// to keep, and does not stop the copy.
        #[test]
        fn across_volumes_the_old_directory_is_copied_and_kept() {
            let tmp = tempfile::tempdir().unwrap();
            let (old, new) = places(tmp.path());
            fill(&old);
            let _socket = std::os::unix::net::UnixListener::bind(old.join("app.sock")).unwrap();
            fs::create_dir_all(new.parent().unwrap()).unwrap();
            assert_eq!(copy_into_place(&old, &new).unwrap(), Adopted::Copied);
            assert_filled(&new);
            assert!(!new.join("app.sock").exists());
            assert_filled(&old);
            let beside: Vec<_> = fs::read_dir(new.parent().unwrap())
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            assert_eq!(beside, ["config"], "the staging copy is gone");
        }
    }
}
