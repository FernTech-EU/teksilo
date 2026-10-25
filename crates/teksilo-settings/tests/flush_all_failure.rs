// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `OpenedSettings::flush_all` with a writer that cannot write. A binary of
//! its own: `flush_all` flushes every writer of the process, so the failing
//! one would fail any other test's `flush_all` running beside it.

use std::time::Duration;

use teksilo_settings::{AppPaths, SettingsBundle, SettingsKey};
use tempfile::tempdir;

const FONT_SIZE: SettingsKey<f32> = SettingsKey::new("editor.font_size", || 14.0);

/// `flush_all` writes every service even when one of them cannot be
/// written: the store's file is a directory here, so its write fails, and
/// the window state queued behind it still reaches the disk; the store's
/// error is the one returned. Returning at the first error left the window
/// state, and every settings file the application opened, unwritten.
#[test]
fn flush_all_writes_every_service_when_one_fails() {
    let dir = tempdir().unwrap();
    let paths = AppPaths::for_testing(dir.path());
    let opened = SettingsBundle::new()
        .with_window_state(true)
        .with_debounce(Duration::from_secs(3600))
        .open(&paths)
        .unwrap();
    std::fs::create_dir_all(opened.store.path()).unwrap();
    opened.store.signal_for(&FONT_SIZE).set(18.0);
    let window_state = opened.window_state.as_ref().unwrap();
    window_state
        .record(teksilo_settings::PerWindowState {
            label: "main".into(),
            x: 10,
            y: 20,
            width: 800,
            height: 600,
            placement: teksilo_core::WindowPlacement::Floating,
        })
        .unwrap();

    let result = opened.flush_all();

    assert!(
        matches!(result, Err(teksilo_settings::SettingsBundleError::Store(_))),
        "the store's failure is reported: {result:?}"
    );
    let written = std::fs::read_to_string(window_state.path()).unwrap_or_default();
    assert!(
        written.contains("main"),
        "the window state was written all the same: {written:?}"
    );
}
