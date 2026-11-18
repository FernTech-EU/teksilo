// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Opening a link or a file with the desktop's handler.
//!
//! A link the user activates (`https:`, `mailto:`, `xmpp:` …) opens in
//! the application the desktop has for it: the browser, the mail client,
//! the chat client. A file opens in its default application.
//!
//! - [`EventContextOpenUriExt`] adds `ctx.open_uri(uri)` and
//!   `ctx.open_path(path)` to every handler.
//! - [`open_uri`] and [`open_path`] do the same from code with no
//!   context.
//! - [`OpenUriHandle`], registered in app-state, replaces the desktop's
//!   handler for the context methods: a test records what would have been
//!   opened, a sandboxed application routes it its own way.
//!
//! # What is checked before anything starts
//!
//! A URI must be absolute (`scheme:rest`, the scheme being
//! `[A-Za-z][A-Za-z0-9+.-]*` and `rest` not empty), at most 8 KiB long,
//! and hold no control character and no whitespace, so it can never
//! start with `-` and be read as an option by the helper program. Any
//! scheme is opened except four, refused case-insensitively:
//! `javascript:`, `vbscript:` and `data:`, which carry code or content
//! rather than name a resource, and `file:`, which reaches local files
//! by a string a remote party may have written: open a file with
//! [`open_path`], which takes a path, so that opening one is a decision
//! of the application. A one-letter scheme is refused too, because on
//! Windows `C:\…` is a drive letter, not a scheme.
//!
//! No allowlist is applied beyond that. An application opening a link
//! that came from elsewhere (a chat message, a document) decides which
//! schemes it shows as links.
//!
//! # How each platform opens it
//!
//! Never through a shell, always as one argument:
//!
//! | Platform | Program |
//! | --- | --- |
//! | Linux and the BSDs | `xdg-open <uri or path>` (inside Flatpak, `xdg-open` goes through the `OpenURI` portal) |
//! | macOS | `/usr/bin/open <uri or path>` |
//! | Windows | `ShellExecuteW` with the `open` verb |
//!
//! On Linux and macOS the helper is started detached (no input, no
//! output, its own process group) and a short-lived thread waits for it,
//! so it never becomes a zombie and the UI thread never waits for it.
//! Only a failure to start it is reported: what the helper does next
//! (no handler for the scheme, the file cannot be read) is said on its
//! standard error, not in the result. On Windows `ShellExecuteW` returns
//! once the handler is started, and its failures are reported.
//!
//! ```no_run
//! use teksilo_platform::open_uri::EventContextOpenUriExt;
//! # fn handler(ctx: &mut teksilo_core::widget::EventContext) {
//! if let Err(error) = ctx.open_uri("https://example.org/") {
//!     eprintln!("could not open the link: {error}");
//! }
//! # }
//! ```

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use teksilo_core::widget::EventContext;

/// The longest URI [`open_uri`] accepts, in bytes.
pub const MAX_URI_LEN: usize = 8 * 1024;

/// Schemes [`open_uri`] never opens, lowercase.
const REFUSED_SCHEMES: [&str; 4] = ["javascript", "vbscript", "data", "file"];

/// Why a link or a file was not opened.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum OpenUriError {
    /// The URI is longer than [`MAX_URI_LEN`] bytes.
    #[error("the URI is {len} bytes long, more than the 8 KiB allowed")]
    TooLong {
        /// The URI's length in bytes.
        len: usize,
    },
    /// The URI holds a control character or whitespace.
    #[error(
        "the URI holds {character:?} at byte {index}: control characters and whitespace are not allowed"
    )]
    ForbiddenCharacter {
        /// The first offending character.
        character: char,
        /// Its byte offset in the URI.
        index: usize,
    },
    /// The URI has no scheme, or nothing after it.
    #[error(
        "not an absolute URI: it needs a scheme (`https:`, `mailto:` …) and something after it"
    )]
    NotAbsolute,
    /// The scheme is one this service never opens (`javascript:`,
    /// `vbscript:`, `data:`, `file:`, or a one-letter scheme).
    #[error("the `{scheme}:` scheme is not opened (a file is opened with `open_path`)")]
    RefusedScheme {
        /// The scheme, lowercase.
        scheme: String,
    },
    /// [`open_path`] was given a relative path.
    #[error("not an absolute path: {}", path.display())]
    RelativePath {
        /// The path as given.
        path: PathBuf,
    },
    /// [`open_path`] was given a path that does not exist.
    #[error("no such file or folder: {}", path.display())]
    PathNotFound {
        /// The path as given.
        path: PathBuf,
    },
    /// The desktop has no program to open it (on Linux, `xdg-open` is not
    /// installed; on Windows, nothing is associated with the scheme or the
    /// file type).
    #[error("no program to open it was found")]
    NoHandler,
    /// The handler could not be started.
    #[error("the handler could not be started")]
    Launch(#[source] std::io::Error),
}

/// What is opened, once checked. An [`OpenUriHandle`] receives it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpenTarget {
    /// A URI accepted by [`open_uri`].
    Uri(String),
    /// An existing absolute path accepted by [`open_path`].
    Path(PathBuf),
}

impl OpenTarget {
    fn as_os_str(&self) -> &OsStr {
        match self {
            Self::Uri(uri) => OsStr::new(uri),
            Self::Path(path) => path.as_os_str(),
        }
    }
}

/// Replaces the desktop's handler for [`EventContextOpenUriExt`].
///
/// Register one in app-state (`TeksiloAppBuilder::app_state`) and the
/// context methods hand it every target they accept, instead of starting
/// the platform's helper. A test records what would have been opened; an
/// application with its own route (a sandbox broker, a confirmation
/// dialog) takes over. The checks of [`open_uri`] and [`open_path`] run
/// before it is called, so it never sees a refused URI.
///
/// The free functions [`open_uri`] and [`open_path`] do not consult it:
/// they have no context to find it in.
#[derive(Clone)]
pub struct OpenUriHandle {
    opener: Rc<Opener>,
}

type Opener = dyn Fn(&OpenTarget) -> Result<(), OpenUriError>;

impl OpenUriHandle {
    /// A handle that opens every target with `opener`.
    pub fn new(opener: impl Fn(&OpenTarget) -> Result<(), OpenUriError> + 'static) -> Self {
        Self {
            opener: Rc::new(opener),
        }
    }

    /// Open `target` with this handle's opener.
    pub fn open(&self, target: &OpenTarget) -> Result<(), OpenUriError> {
        (self.opener)(target)
    }
}

impl fmt::Debug for OpenUriHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenUriHandle").finish_non_exhaustive()
    }
}

/// `ctx.open_uri(…)` and `ctx.open_path(…)` in every handler.
///
/// Both check their argument as [`open_uri`] and [`open_path`] do, then
/// hand it to the [`OpenUriHandle`] in app-state when there is one, and
/// to the desktop's handler otherwise. Call them from the handler of the
/// user's own gesture (a click on the link, the "Open" menu row): a link
/// opened on the user's behalf is what the desktop expects.
pub trait EventContextOpenUriExt {
    /// Open `uri` with the desktop's handler for its scheme. See
    /// [`open_uri`] for what is checked and refused.
    fn open_uri(&mut self, uri: &str) -> Result<(), OpenUriError>;

    /// Open the file or folder at `path` with its default application.
    /// See [`open_path`].
    fn open_path(&mut self, path: &Path) -> Result<(), OpenUriError>;
}

impl EventContextOpenUriExt for EventContext<'_> {
    fn open_uri(&mut self, uri: &str) -> Result<(), OpenUriError> {
        validate_uri(uri)?;
        open_with(self, &OpenTarget::Uri(uri.to_owned()))
    }

    fn open_path(&mut self, path: &Path) -> Result<(), OpenUriError> {
        validate_path(path)?;
        open_with(self, &OpenTarget::Path(path.to_owned()))
    }
}

fn open_with(ctx: &EventContext<'_>, target: &OpenTarget) -> Result<(), OpenUriError> {
    match ctx.app_state::<OpenUriHandle>() {
        Some(handle) => handle.open(target),
        None => launch(target),
    }
}

/// Open `uri` with the desktop's handler for its scheme, from code with no
/// [`EventContext`] (in a handler, prefer `ctx.open_uri`, which an
/// [`OpenUriHandle`] can replace).
///
/// Refused before anything starts: a URI with no scheme, longer than
/// [`MAX_URI_LEN`], holding a control character or whitespace, or whose
/// scheme is `javascript:`, `vbscript:`, `data:`, `file:` or a single
/// letter. See the [module documentation](self).
pub fn open_uri(uri: &str) -> Result<(), OpenUriError> {
    validate_uri(uri)?;
    launch(&OpenTarget::Uri(uri.to_owned()))
}

/// Open the file or folder at `path` with its default application (a
/// folder opens in the file manager), from code with no [`EventContext`].
///
/// The path must be absolute and exist. It is handed to the platform as a
/// path, never turned into a `file:` URI.
pub fn open_path(path: &Path) -> Result<(), OpenUriError> {
    validate_path(path)?;
    launch(&OpenTarget::Path(path.to_owned()))
}

fn validate_uri(uri: &str) -> Result<(), OpenUriError> {
    if uri.len() > MAX_URI_LEN {
        return Err(OpenUriError::TooLong { len: uri.len() });
    }
    if let Some((index, character)) = uri
        .char_indices()
        .find(|(_, c)| c.is_control() || c.is_whitespace())
    {
        return Err(OpenUriError::ForbiddenCharacter { character, index });
    }
    // A scheme starts with a letter, so an accepted URI never starts with
    // `-` and no helper can take it for an option.
    let Some((scheme, rest)) = uri.split_once(':') else {
        return Err(OpenUriError::NotAbsolute);
    };
    if !is_scheme(scheme) || rest.is_empty() {
        return Err(OpenUriError::NotAbsolute);
    }
    let scheme = scheme.to_ascii_lowercase();
    if scheme.len() == 1 || REFUSED_SCHEMES.contains(&scheme.as_str()) {
        return Err(OpenUriError::RefusedScheme { scheme });
    }
    Ok(())
}

fn is_scheme(scheme: &str) -> bool {
    let mut bytes = scheme.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_alphabetic())
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'.' | b'-'))
}

fn validate_path(path: &Path) -> Result<(), OpenUriError> {
    // An absolute path never starts with `-` either.
    if !path.is_absolute() {
        return Err(OpenUriError::RelativePath {
            path: path.to_owned(),
        });
    }
    if !path.exists() {
        return Err(OpenUriError::PathNotFound {
            path: path.to_owned(),
        });
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn launcher_command(target: &OpenTarget) -> std::process::Command {
    let mut command = std::process::Command::new("xdg-open");
    command.arg(target.as_os_str());
    command
}

#[cfg(target_os = "macos")]
fn launcher_command(target: &OpenTarget) -> std::process::Command {
    let mut command = std::process::Command::new("/usr/bin/open");
    command.arg(target.as_os_str());
    command
}

#[cfg(unix)]
fn launch(target: &OpenTarget) -> Result<(), OpenUriError> {
    spawn_detached(launcher_command(target))
}

#[cfg(unix)]
fn spawn_detached(mut command: std::process::Command) -> Result<(), OpenUriError> {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;

    // Standard error stays the application's: it is where the helper says
    // why nothing opened. Its own process group keeps a Ctrl+C meant for
    // the application from reaching a browser started in the foreground.
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .process_group(0);
    let mut child = command.spawn().map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => OpenUriError::NoHandler,
        _ => OpenUriError::Launch(error),
    })?;
    // The handler is already started, so a thread that cannot be created
    // fails nothing: the child then stays a zombie until the application
    // exits.
    let _ = std::thread::Builder::new()
        .name("teksilo-open-uri".into())
        .spawn(move || {
            let _ = child.wait();
        });
    Ok(())
}

#[cfg(target_os = "windows")]
fn launch(target: &OpenTarget) -> Result<(), OpenUriError> {
    use windows::Win32::UI::Shell::{SE_ERR_NOASSOC, ShellExecuteW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, w};

    let file = HSTRING::from(target.as_os_str());
    // SAFETY: both strings are NUL-terminated and outlive the call.
    let instance = unsafe { ShellExecuteW(None, w!("open"), &file, None, None, SW_SHOWNORMAL) };
    // Not a handle: a value above 32 is success, anything else an error code.
    let code = instance.0 as usize;
    if code > 32 {
        Ok(())
    } else if code == SE_ERR_NOASSOC as usize {
        Err(OpenUriError::NoHandler)
    } else {
        Err(OpenUriError::Launch(std::io::Error::other(format!(
            "ShellExecuteW failed with code {code}"
        ))))
    }
}

#[cfg(not(any(unix, windows)))]
fn launch(_target: &OpenTarget) -> Result<(), OpenUriError> {
    Err(OpenUriError::NoHandler)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused_scheme(uri: &str) -> Option<String> {
        match validate_uri(uri) {
            Err(OpenUriError::RefusedScheme { scheme }) => Some(scheme),
            _ => None,
        }
    }

    #[test]
    fn ordinary_links_are_accepted() {
        for uri in [
            "https://example.org/",
            "http://example.org/a?b=c&d=e#f",
            "mailto:someone@example.org",
            "xmpp:room@conference.example.org?join",
            "tel:+33123456789",
            "HTTPS://EXAMPLE.ORG/",
            "web+app.v-2:payload",
            "https://example.org/caf\u{e9}",
        ] {
            assert!(validate_uri(uri).is_ok(), "{uri:?} must be accepted");
        }
    }

    #[test]
    fn a_uri_without_a_scheme_is_not_absolute() {
        for uri in [
            "",
            "example.org",
            ":nothing-before",
            "https:",
            "1http://example.org",
            "+x:y",
            "ht_tp://example.org",
            "ht/tp:x",
            "/usr/bin/evil",
            "//example.org/",
        ] {
            assert!(
                matches!(validate_uri(uri), Err(OpenUriError::NotAbsolute)),
                "{uri:?} must be refused as not absolute",
            );
        }
    }

    #[test]
    fn a_leading_dash_is_refused() {
        for uri in ["-x:y", "--help", "-a"] {
            assert!(
                matches!(validate_uri(uri), Err(OpenUriError::NotAbsolute)),
                "{uri:?} could be read as an option",
            );
        }
    }

    #[test]
    fn control_characters_and_whitespace_are_refused() {
        for (uri, character, index) in [
            ("https://a b", ' ', 9),
            ("https://a\tb", '\t', 9),
            ("https://a\nb", '\n', 9),
            ("https://a\rb", '\r', 9),
            ("https://a\0b", '\0', 9),
            ("https://a\u{7f}b", '\u{7f}', 9),
            ("https://a\u{85}b", '\u{85}', 9),
            ("https://a\u{a0}b", '\u{a0}', 9),
            ("https://a\u{2028}b", '\u{2028}', 9),
            (" https://a", ' ', 0),
            ("https://a\n", '\n', 9),
        ] {
            match validate_uri(uri) {
                Err(OpenUriError::ForbiddenCharacter {
                    character: c,
                    index: i,
                }) => assert_eq!((c, i), (character, index), "{uri:?}"),
                other => panic!("{uri:?} must be refused, got {other:?}"),
            }
        }
    }

    #[test]
    fn length_is_bounded() {
        let prefix = "https://example.org/";
        let at_limit = format!("{prefix}{}", "a".repeat(MAX_URI_LEN - prefix.len()));
        assert!(validate_uri(&at_limit).is_ok());
        let over = format!("{at_limit}a");
        assert!(matches!(
            validate_uri(&over),
            Err(OpenUriError::TooLong { len }) if len == MAX_URI_LEN + 1
        ));
    }

    #[test]
    fn dangerous_schemes_are_refused_in_any_case() {
        for (uri, scheme) in [
            ("javascript:alert(1)", "javascript"),
            ("JavaScript:alert(1)", "javascript"),
            ("vbscript:msgbox", "vbscript"),
            ("VBScript:msgbox", "vbscript"),
            ("data:text/html,<b>x</b>", "data"),
            ("DATA:text/html,x", "data"),
            ("file:///etc/passwd", "file"),
            ("File:///etc/passwd", "file"),
        ] {
            assert_eq!(refused_scheme(uri).as_deref(), Some(scheme), "{uri:?}");
        }
    }

    #[test]
    fn a_one_letter_scheme_is_refused_as_a_drive_letter() {
        assert_eq!(
            refused_scheme("C:\\Windows\\system32\\calc.exe").as_deref(),
            Some("c")
        );
        assert_eq!(refused_scheme("c:/Windows").as_deref(), Some("c"));
    }

    #[test]
    fn a_path_must_be_absolute_and_exist() {
        assert!(matches!(
            validate_path(Path::new("-rf")),
            Err(OpenUriError::RelativePath { .. })
        ));
        assert!(matches!(
            validate_path(Path::new("Cargo.toml")),
            Err(OpenUriError::RelativePath { .. })
        ));
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert!(validate_path(&here.join("Cargo.toml")).is_ok());
        assert!(validate_path(here).is_ok());
        assert!(matches!(
            validate_path(&here.join("no-such-file.teksilo")),
            Err(OpenUriError::PathNotFound { .. })
        ));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn xdg_open_receives_the_target_as_its_only_argument() {
        let command = launcher_command(&OpenTarget::Uri("https://a/?b&c;d|e".into()));
        assert_eq!(command.get_program(), "xdg-open");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [OsStr::new("https://a/?b&c;d|e")]
        );

        let command = launcher_command(&OpenTarget::Path("/tmp/a b.txt".into()));
        assert_eq!(command.get_program(), "xdg-open");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [OsStr::new("/tmp/a b.txt")]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_missing_helper_is_no_handler() {
        let command = std::process::Command::new("teksilo-no-such-helper-program");
        assert!(matches!(
            spawn_detached(command),
            Err(OpenUriError::NoHandler)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_started_helper_is_success() {
        assert!(spawn_detached(std::process::Command::new("true")).is_ok());
    }

    mod context {
        use std::any::{Any, TypeId};
        use std::cell::RefCell;
        use std::collections::HashMap;

        use teksilo_core::event_source::TreeAppContext;
        use teksilo_core::widget_tree::WidgetTree;
        use teksilo_core::window::NoopWindowOps;

        use super::*;

        /// A tree whose app-state holds a handle recording every target.
        fn recording_tree() -> (WidgetTree, Rc<RefCell<Vec<OpenTarget>>>) {
            let opened = Rc::new(RefCell::new(Vec::new()));
            let sink = opened.clone();
            let handle = OpenUriHandle::new(move |target| {
                sink.borrow_mut().push(target.clone());
                Ok(())
            });
            let mut registry: HashMap<TypeId, Box<dyn Any>> = HashMap::new();
            registry.insert(TypeId::of::<OpenUriHandle>(), Box::new(handle));
            let mut tree = WidgetTree::new();
            tree.set_app_context(Rc::new(TreeAppContext::empty().with_app_state(registry)));
            (tree, opened)
        }

        #[test]
        fn the_handle_in_app_state_receives_what_is_accepted() {
            let (mut tree, opened) = recording_tree();
            let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
            let mut results = Vec::new();
            tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
                results.push(ctx.open_uri("xmpp:room@example.org?join").is_ok());
                results.push(ctx.open_path(&file).is_ok());
            });
            assert_eq!(results, [true, true]);
            assert_eq!(
                *opened.borrow(),
                [
                    OpenTarget::Uri("xmpp:room@example.org?join".into()),
                    OpenTarget::Path(file),
                ]
            );
        }

        #[test]
        fn the_handle_never_sees_a_refused_target() {
            let (mut tree, opened) = recording_tree();
            let mut errors = Vec::new();
            tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
                errors.push(ctx.open_uri("javascript:alert(1)").err());
                errors.push(ctx.open_uri("https://a b").err());
                errors.push(ctx.open_path(Path::new("relative.txt")).err());
            });
            assert!(matches!(
                errors.as_slice(),
                [
                    Some(OpenUriError::RefusedScheme { .. }),
                    Some(OpenUriError::ForbiddenCharacter { .. }),
                    Some(OpenUriError::RelativePath { .. }),
                ]
            ));
            assert!(opened.borrow().is_empty());
        }
    }
}
