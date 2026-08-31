// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `runtime_override` pointed at a **directory** of `.ftl` files.
//!
//! The single-file form replaces a locale's whole bundle, which is wrong for
//! any application whose catalogue is split across several files per locale:
//! saving `main.ftl` drops every key `tooltips.ftl` defined, and they fall
//! back to the source locale with no error anywhere. These tests pin the
//! directory form that fixes it, and the destructive single-file behaviour
//! that motivates it, so nobody "simplifies" the merge back out.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use teksilo_i18n::{FtlFileWatcher, I18nConfig, I18nManager, ReloadSink};
use unic_langid::LanguageIdentifier;

fn fr() -> LanguageIdentifier {
    "fr-FR".parse().expect("fr-FR parses")
}

/// A fresh temp directory, named after the test so parallel runs cannot
/// collide on one path.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "teksilo-i18n-override-dir-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir creation");
    dir
}

fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, body).expect("fixture write");
    path
}

/// A manager whose `fr-FR` bundle is the two-file merge a real app ships,
/// with `en-US` as the source locale behind it.
fn manager() -> std::rc::Rc<I18nManager> {
    let cfg = I18nConfig::new()
        .source_locale("en-US".parse().expect("en-US parses"))
        .supported_locales(["en-US".parse().expect("en-US parses"), fr()])
        .compile_in(&[
            (
                "en-US",
                &["from-main = From main\n", "from-tooltips = From tooltips\n"],
            ),
            (
                "fr-FR",
                &["from-main = FR main\n", "from-tooltips = FR tooltips\n"],
            ),
        ])
        .auto_detect_os_locale(false);
    let mgr = I18nManager::from_config(&cfg);
    mgr.set_locale(fr());
    mgr
}

/// The bug this whole feature exists for: reloading one file of a
/// multi-file locale wipes the rest of that locale's bundle.
#[test]
fn single_file_reload_drops_the_locales_other_files() {
    let dir = scratch("single-file");
    let main = write(&dir, "main.ftl", "from-main = Depuis main\n");
    write(&dir, "tooltips.ftl", "from-tooltips = Depuis tooltips\n");

    let mgr = manager();
    mgr.reload_from_path(&fr(), &main).expect("reload succeeds");

    assert_eq!(mgr.resolve_app("from-main", &[]), "Depuis main");
    // Not "Depuis tooltips", and not even the compiled-in "FR tooltips":
    // the fr-FR bundle no longer defines the key at all, so resolution
    // falls through to the en-US source bundle.
    assert_eq!(mgr.resolve_app("from-tooltips", &[]), "From tooltips");
}

/// The fix: point the override at the directory and every file is merged
/// into one bundle, so editing one file keeps the others.
#[test]
fn directory_reload_merges_every_ftl_in_the_folder() {
    let dir = scratch("merge");
    write(&dir, "main.ftl", "from-main = Depuis main\n");
    write(&dir, "tooltips.ftl", "from-tooltips = Depuis tooltips\n");

    let mgr = manager();
    mgr.reload_from_path(&fr(), &dir).expect("reload succeeds");

    assert_eq!(mgr.resolve_app("from-main", &[]), "Depuis main");
    assert_eq!(mgr.resolve_app("from-tooltips", &[]), "Depuis tooltips");
}

/// A second save of one file re-reads the whole directory, so the edit
/// lands and its siblings survive. This is the translator's actual loop.
#[test]
fn directory_reload_survives_repeated_edits_to_one_file() {
    let dir = scratch("repeat");
    write(&dir, "main.ftl", "from-main = Depuis main\n");
    write(&dir, "tooltips.ftl", "from-tooltips = Depuis tooltips\n");

    let mgr = manager();
    mgr.reload_from_path(&fr(), &dir).expect("first reload");

    write(&dir, "main.ftl", "from-main = Depuis main, corrigé\n");
    mgr.reload_from_path(&fr(), &dir).expect("second reload");

    assert_eq!(mgr.resolve_app("from-main", &[]), "Depuis main, corrigé");
    assert_eq!(mgr.resolve_app("from-tooltips", &[]), "Depuis tooltips");
}

/// Non-`.ftl` neighbours are ignored rather than fed to the Fluent parser.
/// A locale directory in a real tree also holds editor backups and, in
/// Skribisto's case, sits beside `.djot` help pages.
#[test]
fn directory_reload_ignores_non_ftl_files() {
    let dir = scratch("ignore");
    write(&dir, "main.ftl", "from-main = Depuis main\n");
    write(&dir, "notes.md", "this is not fluent syntax at all: { <<\n");
    write(&dir, "main.ftl~", "from-main = stale backup\n");
    std::fs::create_dir_all(dir.join("nested")).expect("nested dir");
    write(&dir.join("nested"), "deep.ftl", "from-main = too deep\n");

    let mgr = manager();
    mgr.reload_from_path(&fr(), &dir).expect("reload succeeds");

    assert_eq!(mgr.resolve_app("from-main", &[]), "Depuis main");
}

/// Merge order is sorted by file name, not `read_dir` order, so a key
/// defined twice resolves the same way on every platform and every save.
/// Fluent keeps the first definition, so `a.ftl` wins over `b.ftl`.
#[test]
fn directory_reload_merge_order_is_sorted_and_first_wins() {
    let dir = scratch("order");
    write(&dir, "a.ftl", "shared = from a\n");
    write(&dir, "b.ftl", "shared = from b\n");

    let mgr = manager();
    mgr.reload_from_path(&fr(), &dir).expect("reload succeeds");

    assert_eq!(mgr.resolve_app("shared", &[]), "from a");
}

/// A directory holding no `.ftl` at all is a mistyped path (`locales/`
/// rather than `locales/fr-FR/`). It errors and keeps the old bundle
/// rather than installing an empty one, which would look exactly like a
/// missing translation.
#[test]
fn empty_directory_errors_and_keeps_the_previous_bundle() {
    let dir = scratch("empty");

    let mgr = manager();
    let outcome = mgr.reload_from_path(&fr(), &dir);

    assert!(outcome.is_err(), "an .ftl-less directory must not reload");
    assert_eq!(mgr.resolve_app("from-main", &[]), "FR main");
}

/// One malformed file aborts the whole directory reload: the bundle is
/// assembled only after every file parses, so a translator mid-edit never
/// sees a half-applied catalogue that matches no build.
#[test]
fn one_malformed_file_aborts_the_whole_directory_reload() {
    let dir = scratch("malformed");
    write(&dir, "main.ftl", "from-main = Depuis main\n");
    write(&dir, "tooltips.ftl", "= this is not a message id\n");

    let mgr = manager();
    let outcome = mgr.reload_from_path(&fr(), &dir);

    assert!(outcome.is_err(), "a parse error must not reload");
    assert_eq!(mgr.resolve_app("from-main", &[]), "FR main");
    assert_eq!(mgr.resolve_app("from-tooltips", &[]), "FR tooltips");
}

/// The version signal is what drives every `to_signal()` observer to
/// re-resolve, so a successful directory reload must bump it, and a failed
/// one must not.
#[test]
fn directory_reload_bumps_version_only_on_success() {
    let dir = scratch("version");
    write(&dir, "main.ftl", "from-main = Depuis main\n");

    let mgr = manager();
    let before = mgr.version_signal().get();

    mgr.reload_from_path(&fr(), &dir).expect("reload succeeds");
    let after_ok = mgr.version_signal().get();
    assert!(after_ok > before, "a successful reload bumps the version");

    write(&dir, "main.ftl", "= broken\n");
    assert!(mgr.reload_from_path(&fr(), &dir).is_err());
    assert_eq!(
        mgr.version_signal().get(),
        after_ok,
        "a failed reload leaves the version alone"
    );
}

/// The watcher accepts a directory target. It watches that directory
/// itself rather than climbing to its parent, which would wake every
/// locale on any one locale's save.
#[test]
fn watcher_accepts_a_directory_target() {
    let dir = scratch("watch");
    write(&dir, "main.ftl", "from-main = Depuis main\n");

    let sink: ReloadSink = Arc::new(|_locale, _path| {});
    let watcher = FtlFileWatcher::new(vec![(fr(), dir.clone())], sink);

    assert!(watcher.is_ok(), "a directory target must be watchable");
}

/// Block until `probe` returns a value or the budget runs out.
///
/// Filesystem notifications are asynchronous and their latency is the
/// platform's business, so these tests poll rather than sleep a fixed
/// interval and hope.
fn wait_for<T>(mut probe: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if let Some(value) = probe() {
            return Some(value);
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    None
}

/// End-to-end through `notify`: saving a file *inside* a watched directory
/// wakes the sink, and the path it hands back is the **directory**, which is
/// what `reload_from_path` needs in order to rebuild the locale from all of
/// its files. Handing back the changed file instead would reintroduce
/// the single-file bug through the back door.
#[test]
fn saving_a_file_in_a_watched_directory_reports_the_directory() {
    let dir = scratch("fires");
    write(&dir, "main.ftl", "from-main = Depuis main\n");
    write(&dir, "tooltips.ftl", "from-tooltips = Depuis tooltips\n");
    let canonical = dir.canonicalize().expect("canonicalize");

    let seen: Arc<std::sync::Mutex<Vec<(LanguageIdentifier, PathBuf)>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorder = Arc::clone(&seen);
    let sink: ReloadSink = Arc::new(move |locale, path| {
        recorder.lock().expect("sink mutex").push((locale, path));
    });
    let _watcher =
        FtlFileWatcher::new(vec![(fr(), dir.clone())], sink).expect("watcher construction");

    write(
        &dir,
        "tooltips.ftl",
        "from-tooltips = Depuis tooltips, corrigé\n",
    );

    let hit = wait_for(|| seen.lock().expect("sink mutex").first().cloned())
        .expect("a save inside the directory must wake the watcher");
    assert_eq!(hit.0, fr());
    assert_eq!(
        hit.1, canonical,
        "the sink must be handed the directory, not the file that changed"
    );
}

/// A non-`.ftl` write in the same directory is ignored: editors drop swap
/// files and backups next to the real ones, and each would otherwise cost a
/// full re-parse of the locale.
#[test]
fn a_non_ftl_write_in_a_watched_directory_is_ignored() {
    let dir = scratch("noise");
    write(&dir, "main.ftl", "from-main = Depuis main\n");

    let seen: Arc<std::sync::Mutex<Vec<PathBuf>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorder = Arc::clone(&seen);
    let sink: ReloadSink = Arc::new(move |_locale, path| {
        recorder.lock().expect("sink mutex").push(path);
    });
    let _watcher =
        FtlFileWatcher::new(vec![(fr(), dir.clone())], sink).expect("watcher construction");

    write(&dir, "main.ftl.swp", "editor noise\n");
    write(&dir, "notes.md", "more noise\n");

    // Then a real save, to prove the watcher was alive the whole time rather
    // than merely slow: the noise must not appear *before* it.
    write(&dir, "main.ftl", "from-main = Depuis main, corrigé\n");
    let first = wait_for(|| seen.lock().expect("sink mutex").first().cloned())
        .expect("the .ftl save must wake the watcher");

    assert_eq!(
        first,
        dir.canonicalize().expect("canonicalize"),
        "the first wake-up must be the .ftl save, not the noise around it"
    );
}
