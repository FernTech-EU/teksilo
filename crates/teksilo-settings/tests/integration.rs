// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! End-to-end integration tests:
//!   * Multi-process style reopen (state persists across `Drop`).
//!   * The `SettingsBundle` -> `OpenedSettings` pipeline.
//!   * Generic `MruList<T: MruEntry>` round-trip with a custom item type.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use teksilo_settings::{
    AppPaths, Keyed, MruEntry, MruList, SettingsBundle, SettingsKey, SettingsStore,
};
use tempfile::tempdir;

const FONT_SIZE: SettingsKey<f32> = SettingsKey::new("editor.font_size", || 14.0);
const SHOW_MINIMAP: SettingsKey<bool> = SettingsKey::new("editor.minimap", || true);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct DemoProject {
    path: PathBuf,
    label: String,
    pinned: bool,
}

impl DemoProject {
    fn new(path: &str, label: &str) -> Self {
        Self {
            path: path.into(),
            label: label.into(),
            pinned: false,
        }
    }
}

impl Keyed for DemoProject {
    type Key = PathBuf;
    fn key(&self) -> PathBuf {
        self.path.clone()
    }
}

impl MruEntry for DemoProject {
    fn is_pinned(&self) -> bool {
        self.pinned
    }
    fn set_pinned(&mut self, p: bool) {
        self.pinned = p;
    }
}

#[test]
fn store_signal_clones_share_value() {
    let dir = tempdir().unwrap();
    let store = SettingsStore::open_with_delay(dir.path().join("p.toml"), Duration::ZERO).unwrap();

    let a = store.signal_for(&FONT_SIZE);
    let b = store.signal_for(&FONT_SIZE);
    a.set(20.0);
    assert_eq!(b.get(), 20.0);
}

#[test]
fn store_persists_multiple_keys_across_reopen() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("p.toml");

    {
        let store = SettingsStore::open_with_delay(path.clone(), Duration::ZERO).unwrap();
        store.signal_for(&FONT_SIZE).set(22.0);
        store.signal_for(&SHOW_MINIMAP).set(false);
        store.flush_now().unwrap();
    }

    let store = SettingsStore::open_with_delay(path, Duration::ZERO).unwrap();
    assert_eq!(store.signal_for(&FONT_SIZE).get(), 22.0);
    assert!(!store.signal_for(&SHOW_MINIMAP).get());
}

#[test]
fn bundle_open_creates_only_requested_services() {
    let dir = tempdir().unwrap();
    let paths = AppPaths::for_testing(dir.path());

    let opened = SettingsBundle::new()
        .with_window_state(true)
        .with_debounce(Duration::ZERO)
        .open(&paths)
        .unwrap();

    assert!(opened.window_state.is_some());
}

#[test]
fn mru_list_round_trip_through_open() {
    let dir = tempdir().unwrap();
    let paths = AppPaths::for_testing(dir.path());

    {
        let mru: MruList<DemoProject> =
            MruList::open_with_delay(&paths, "recents", 3, Duration::ZERO).unwrap();
        mru.add(DemoProject::new("/proj/a", "A"));
        mru.add(DemoProject::new("/proj/b", "B"));
        mru.flush_now().unwrap();
    }

    let mru: MruList<DemoProject> =
        MruList::open_with_delay(&paths, "recents", 3, Duration::ZERO).unwrap();
    assert_eq!(mru.model().len(), 2);
}

#[test]
fn bundle_full_round_trip() {
    let dir = tempdir().unwrap();
    let paths = AppPaths::for_testing(dir.path());

    {
        let opened = SettingsBundle::new()
            .with_window_state(true)
            .with_debounce(Duration::ZERO)
            .open(&paths)
            .unwrap();

        opened.store.signal_for(&FONT_SIZE).set(18.0);
        opened
            .window_state
            .as_ref()
            .unwrap()
            .record(teksilo_settings::PerWindowState {
                label: "main".into(),
                x: 10,
                y: 20,
                width: 800,
                height: 600,
                placement: teksilo_core::WindowPlacement::Floating,
            })
            .unwrap();

        opened.flush_all().unwrap();
    }

    let opened = SettingsBundle::new()
        .with_window_state(true)
        .with_debounce(Duration::ZERO)
        .open(&paths)
        .unwrap();

    assert_eq!(opened.store.signal_for(&FONT_SIZE).get(), 18.0);
    assert_eq!(
        opened
            .window_state
            .as_ref()
            .unwrap()
            .state_for("main")
            .unwrap()
            .width,
        800
    );
}

#[test]
fn signal_observation_outside_store_works() {
    let dir = tempdir().unwrap();
    let store = SettingsStore::open_with_delay(dir.path().join("p.toml"), Duration::ZERO).unwrap();

    let sig = store.signal_for(&FONT_SIZE);
    let observed = std::rc::Rc::new(std::cell::Cell::new(0.0_f32));
    let obs_clone = observed.clone();
    let _h = sig.observe(move |v: &f32| obs_clone.set(*v));

    sig.set(25.0);
    assert_eq!(observed.get(), 25.0);
}
