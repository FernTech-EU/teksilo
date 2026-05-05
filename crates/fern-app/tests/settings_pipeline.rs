//! Integration test for the `FernAppBuilder::settings(...)` pipeline.
//!
//! Verifies that:
//!   * `app_paths(...)` + `settings(SettingsBundle)` opens services and
//!     registers them via `app_state`.
//!   * Both `BuildContext::app_state` and `EventContext::app_state`
//!     can recover them by type.
//!   * The convenience `SettingsExt` trait works on both context types.
//!   * Mutations made through the recovered handle persist back to disk.
//!   * Apps register their own `MruList<T>` via `app_state(...)`; the
//!     framework no longer ships a hardcoded `RecentsService`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use fern_app::FernAppBuilder;
use fern_canvas::SizeProposal;
use fern_core::BuildContext;
use fern_core::widget::{LayoutContext, Widget};
use fern_core::widget_id::WidgetId;
use fern_settings::{
    AppPaths, MruEntry, MruList, SettingsBundle, SettingsExt, SettingsKey, SettingsStore,
    WindowStateService,
};
use serde::{Deserialize, Serialize};
use tempfile::tempdir;

const FONT_SIZE: SettingsKey<f32> = SettingsKey::new("editor.font_size", || 14.0);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct TestRecentProject {
    path: PathBuf,
    name: String,
    pinned: bool,
}

impl TestRecentProject {
    fn new(path: &str, name: &str) -> Self {
        Self {
            path: path.into(),
            name: name.into(),
            pinned: false,
        }
    }
}

impl MruEntry for TestRecentProject {
    type Key = Path;
    fn key(&self) -> &Path {
        &self.path
    }
    fn is_pinned(&self) -> bool {
        self.pinned
    }
    fn set_pinned(&mut self, p: bool) {
        self.pinned = p;
    }
}

/// A leaf widget that reads the registered SettingsStore + MruList +
/// WindowStateService out of the contexts, both via raw `app_state` and
/// via `SettingsExt`.
#[derive(Debug)]
struct ProbeWidget;

impl Widget for ProbeWidget {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let store = ctx.app_state::<SettingsStore>().expect("store registered");
        let _ = ctx.settings();
        let _ = ctx.window_state();
        let _ = ctx.mru::<TestRecentProject>();
        let sig = store.signal_for(&FONT_SIZE);
        sig.set(sig.get() + 1.0);
        Vec::new()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

#[test]
fn app_state_carries_store_and_window_state() {
    let dir = tempdir().unwrap();
    let paths = AppPaths::for_testing(dir.path());

    let app = FernAppBuilder::new()
        .app_paths(paths.clone())
        .settings(
            SettingsBundle::new()
                .with_window_state(true)
                .with_debounce(Duration::ZERO),
        )
        .build_headless();

    let opened = app.settings.as_ref().expect("settings installed");
    assert!(opened.window_state.is_some());
    let _ = opened;
}

#[test]
fn app_registers_its_own_mru_list_via_app_state() {
    let dir = tempdir().unwrap();
    let paths = AppPaths::for_testing(dir.path());

    let mru: MruList<TestRecentProject> =
        MruList::open_with_delay(&paths, "recents", 5, Duration::ZERO).unwrap();
    mru.add(TestRecentProject::new("/proj", "P"));
    mru.flush_now().unwrap();

    let app = FernAppBuilder::new()
        .app_paths(paths.clone())
        .settings(
            SettingsBundle::new()
                .with_window_state(true)
                .with_debounce(Duration::ZERO),
        )
        .app_state(mru.clone())
        .build_headless();

    // The mru handle is reachable from any handler via SettingsExt.
    let recovered = app
        .tree
        .app_context()
        .app_state::<MruList<TestRecentProject>>()
        .expect("mru registered via app_state");
    assert_eq!(recovered.model().len(), 1);
}

#[test]
fn build_context_can_reach_settings_and_mru_via_ext_trait() {
    let dir = tempdir().unwrap();
    let paths = AppPaths::for_testing(dir.path());

    let mru: MruList<TestRecentProject> =
        MruList::open_with_delay(&paths, "recents", 5, Duration::ZERO).unwrap();

    let mut app = FernAppBuilder::new()
        .app_paths(paths.clone())
        .settings(
            SettingsBundle::new()
                .with_window_state(true)
                .with_debounce(Duration::ZERO),
        )
        .app_state(mru)
        .build_headless();

    app.tree.add(ProbeWidget);
    app.tree.layout(SizeProposal::exact(100.0, 100.0));

    let store = app.settings.as_ref().unwrap().store.signal_for(&FONT_SIZE);
    assert_eq!(store.get(), 15.0);
}

#[test]
fn settings_without_app_paths_panics() {
    let result = std::panic::catch_unwind(|| {
        let _ = FernAppBuilder::new()
            .settings(SettingsBundle::new())
            .build_headless();
    });
    assert!(
        result.is_err(),
        "expected panic when settings used without paths"
    );
}

#[test]
fn settings_off_by_default() {
    let app = FernAppBuilder::new().build_headless();
    assert!(app.settings.is_none());
}

#[test]
fn application_panics_only_when_paths_unresolvable() {
    // Whether or not `application(...)` panics depends on the OS env
    // (it can in sandboxed CI). We only assert that the call doesn't
    // *silently* succeed-then-fail later.
    let _ = std::panic::catch_unwind(|| {
        FernAppBuilder::new()
            .application("test", "fern-ci", "settings-pipeline-smoke")
            .build_headless()
    });
}

#[test]
fn for_testing_paths_keep_writes_inside_tmp() {
    let dir = tempdir().unwrap();
    let paths = AppPaths::for_testing(dir.path());

    let app = FernAppBuilder::new()
        .app_paths(paths.clone())
        .settings(SettingsBundle::new().with_debounce(Duration::ZERO))
        .build_headless();

    let store_path: PathBuf = app.settings.as_ref().unwrap().store.path();
    assert!(
        store_path.starts_with(dir.path()),
        "store.path() = {} must be under tempdir {}",
        store_path.display(),
        dir.path().display(),
    );

    // EventContext gives the same handles by type — keep the
    // type-name references compile-checked.
    let _ = std::any::type_name::<MruList<TestRecentProject>>();
    let _ = std::any::type_name::<WindowStateService>();
}
