// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `notify`-based hot-reload watcher for `runtime_override` `.ftl` files.
//!
//! Architecture §12.6: the translator workflow starts the application
//! with one or more `--translation-dev LOCALE=PATH` flags, each of which
//! translates into an `I18nConfig::runtime_override(locale, path)` call.
//! `PATH` is either a single `.ftl` file or a **directory** of them; the
//! directory form is what an application whose catalogue is split across
//! several files per locale needs, because reloading one file of such a
//! set replaces the locale's whole bundle (see
//! [`I18nManager::reload_from_path`](crate::I18nManager::reload_from_path)).
//! `TeksiloAppBuilder::run` then spins up a single `FtlFileWatcher` that
//! watches every registered path and forwards file-changed events to the
//! UI thread via an event sink callback.
//!
//! The sink is a `Fn(LanguageIdentifier, PathBuf) + Send + Sync + 'static`
//! that teksilo-app implements by posting a `AppEvent::I18nReload` through
//! the winit `EventLoopProxy`. Keeping the sink as a plain `Fn` trait
//! object means `teksilo-i18n` does not need to know anything about winit
//! — the coupling lives in teksilo-app.
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
/// file changes. Implementations must be thread-safe; teksilo-app's
/// implementation posts the reload request through the winit
/// `EventLoopProxy`, which internally routes back to the UI thread.
pub type ReloadSink = Arc<dyn Fn(LanguageIdentifier, PathBuf) + Send + Sync + 'static>;

/// Active hot-reload watcher. One per `TeksiloAppBuilder::run` invocation.
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
    /// callback. Each path is either a single `.ftl` file or a directory
    /// of them. Non-existent paths are logged and skipped (they can
    /// legitimately be missing at startup; the application logs but
    /// carries on). The returned `FtlFileWatcher` must be kept alive
    /// for the duration of hot-reload observation.
    pub fn new(
        entries: Vec<(LanguageIdentifier, PathBuf)>,
        sink: ReloadSink,
    ) -> Result<Self, notify::Error> {
        // Canonicalize each path so that filesystem events (which arrive
        // with canonical paths on every platform) match our map. The map
        // is keyed on what was *registered*: a file for a file override, the
        // directory itself for a directory one. That is the path handed back
        // to `reload_from_path`, which needs the whole directory to rebuild
        // the locale's bundle from all of its files.
        let mut path_map: HashMap<PathBuf, LanguageIdentifier> = HashMap::new();
        let mut watch_targets: Vec<PathBuf> = Vec::new();
        for (locale, path) in entries {
            let canonical = match path.canonicalize() {
                Ok(p) => p,
                Err(e) => {
                    eprintln!(
                        "teksilo-i18n: cannot watch `{}` ({e}); hot-reload disabled for {locale}",
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
                        if let Some((target, locale)) = resolve_target(&map_for_closure, path) {
                            (sink_handle)(locale, target);
                        }
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    eprintln!("teksilo-i18n: watcher error: {e}");
                }
            },
        )?;

        for target in &watch_targets {
            // For a *file* target, watch the parent directory rather than
            // the file itself. Many editors save by writing to a temp file
            // and renaming, which invalidates the original inode watch.
            // Watching the parent catches both the rename and the direct
            // write, while `resolve_target`'s filter keeps us focused on
            // the specific .ftl files we registered.
            //
            // A *directory* target is already that directory, so watch it
            // as-is: climbing to its parent would watch `locales/` and
            // wake every locale on any locale's save.
            let watch_at = if target.is_dir() {
                target.as_path()
            } else {
                target.parent().unwrap_or_else(|| Path::new("."))
            };
            watcher.watch(watch_at, RecursiveMode::NonRecursive)?;
        }

        Ok(Self {
            _inner: watcher,
            _path_map: path_map,
        })
    }
}

/// Map a filesystem event path back to the override it belongs to,
/// returning the *registered* path (what `reload_from_path` must be given)
/// and its locale.
///
/// Two shapes, tried in that order:
///
/// 1. **Exact hit**: the event names a registered file override.
/// 2. **Parent hit**: the event names a `.ftl` file inside a registered
///    *directory* override, so the whole directory reloads.
///
/// The `.ftl` extension test guards the second case only. A directory watch
/// reports every write in the folder, and editors litter it with swap files,
/// `.ftl~` backups and atomic-rename temporaries; without the filter each of
/// those would trigger a full re-parse of the locale. A file override needs
/// no such test, having matched a path we registered by name.
fn resolve_target(
    map: &HashMap<PathBuf, LanguageIdentifier>,
    event_path: &Path,
) -> Option<(PathBuf, LanguageIdentifier)> {
    if let Some(locale) = map.get(event_path) {
        return Some((event_path.to_path_buf(), locale.clone()));
    }
    if event_path.extension().is_some_and(|ext| ext == "ftl")
        && let Some(dir) = event_path.parent()
        && let Some(locale) = map.get(dir)
    {
        return Some((dir.to_path_buf(), locale.clone()));
    }
    None
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
        let dir = std::env::temp_dir().join(format!("teksilo-i18n-watcher-{}", std::process::id()));
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
