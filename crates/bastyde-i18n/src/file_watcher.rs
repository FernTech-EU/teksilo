//! `notify`-based hot-reload watcher for `runtime_override` `.ftl` files.
//!
//! Architecture §12.6: the translator workflow starts the application
//! with one or more `--translation-dev LOCALE=PATH` flags, each of which
//! translates into an `I18nConfig::runtime_override(locale, path)` call.
//! `BastydeAppBuilder::run` then spins up a single `FtlFileWatcher` that
//! watches every registered path and forwards file-changed events to the
//! UI thread via an event sink callback.
//!
//! The sink is a `Fn(LanguageIdentifier, PathBuf) + Send + Sync + 'static`
//! that bastyde-app implements by posting a `AppEvent::I18nReload` through
//! the winit `EventLoopProxy`. Keeping the sink as a plain `Fn` trait
//! object means `bastyde-i18n` does not need to know anything about winit
//! — the coupling lives in bastyde-app.
//!
//! The watcher owns the `notify::RecommendedWatcher` background thread
//! for its whole lifetime; dropping the `FtlFileWatcher` stops the
//! watcher and cleans up.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use unic_langid::LanguageIdentifier;

/// Sink type invoked on the notify worker thread when a watched `.ftl`
/// file changes. Implementations must be thread-safe; bastyde-app's
/// implementation posts the reload request through the winit
/// `EventLoopProxy`, which internally routes back to the UI thread.
pub type ReloadSink = Arc<dyn Fn(LanguageIdentifier, PathBuf) + Send + Sync + 'static>;

/// Active hot-reload watcher. One per `BastydeAppBuilder::run` invocation.
///
/// The watcher is self-contained: it owns the `notify::RecommendedWatcher`,
/// stores the path→locale mapping needed to identify which bundle to
/// reload, and holds the sink callback. Drop the watcher to stop
/// observing.
pub struct FtlFileWatcher {
    _inner: RecommendedWatcher,
    // Kept alive so the background thread can still read the map after
    // construction. Wrapped in an `Arc` so the notify closure owns its
    // own handle independent of this struct.
    _path_map: Arc<HashMap<PathBuf, LanguageIdentifier>>,
}

impl FtlFileWatcher {
    /// Build a watcher from a list of `(locale, path)` pairs and a sink
    /// callback. Non-existent paths are logged and skipped (they can
    /// legitimately be missing at startup; the application logs but
    /// carries on). The returned `FtlFileWatcher` must be kept alive
    /// for the duration of hot-reload observation.
    pub fn new(
        entries: Vec<(LanguageIdentifier, PathBuf)>,
        sink: ReloadSink,
    ) -> Result<Self, notify::Error> {
        // Canonicalize each path so that filesystem events (which arrive
        // with canonical paths on every platform) match our map.
        let mut path_map: HashMap<PathBuf, LanguageIdentifier> = HashMap::new();
        let mut watch_targets: Vec<PathBuf> = Vec::new();
        for (locale, path) in entries {
            let canonical = match path.canonicalize() {
                Ok(p) => p,
                Err(e) => {
                    eprintln!(
                        "bastyde-i18n: cannot watch `{}` ({e}); hot-reload disabled for {locale}",
                        path.display()
                    );
                    continue;
                }
            };
            watch_targets.push(canonical.clone());
            path_map.insert(canonical, locale);
        }

        let path_map = Arc::new(path_map);
        let sink_handle = sink.clone();
        let map_for_closure = path_map.clone();

        let mut watcher = notify::recommended_watcher(
            move |res: Result<notify::Event, notify::Error>| match res {
                Ok(event) if should_reload(&event.kind) => {
                    for path in &event.paths {
                        if let Some(locale) = map_for_closure.get(path) {
                            (sink_handle)(locale.clone(), path.clone());
                        }
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    eprintln!("bastyde-i18n: watcher error: {e}");
                }
            },
        )?;

        for target in &watch_targets {
            // Watch the parent directory rather than the file itself.
            // Many editors save by writing to a temp file and renaming,
            // which invalidates the original inode watch. Watching the
            // parent catches both the rename and the direct write,
            // while the closure's path filter keeps us focused on the
            // specific .ftl files we registered.
            let parent = target.parent().unwrap_or_else(|| Path::new("."));
            watcher.watch(parent, RecursiveMode::NonRecursive)?;
        }

        Ok(Self {
            _inner: watcher,
            _path_map: path_map,
        })
    }
}

/// Return `true` for event kinds that mean a file's content may have
/// changed. `notify` fires many events for other operations (access,
/// metadata, etc.) that do not require a reload.
fn should_reload(kind: &notify::EventKind) -> bool {
    use notify::EventKind::*;
    matches!(kind, Modify(_) | Create(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn construction_with_missing_file_does_not_panic() {
        // The path doesn't exist — canonicalize fails, the entry is
        // skipped with a log, and an empty watcher is returned.
        let sink: ReloadSink = Arc::new(|_loc, _path| {});
        let watcher = FtlFileWatcher::new(
            vec![(
                "fr-FR".parse().unwrap(),
                PathBuf::from("/definitely/does/not/exist.ftl"),
            )],
            sink,
        );
        assert!(watcher.is_ok());
    }

    #[test]
    fn construction_with_real_file_succeeds() {
        let dir = std::env::temp_dir().join(format!("bastyde-i18n-watcher-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("fr-FR.ftl");
        std::fs::write(&path, "k = v\n").unwrap();

        let sink: ReloadSink = Arc::new(|_loc, _path| {});
        let watcher = FtlFileWatcher::new(vec![("fr-FR".parse().unwrap(), path.clone())], sink);
        assert!(watcher.is_ok());

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }
}
