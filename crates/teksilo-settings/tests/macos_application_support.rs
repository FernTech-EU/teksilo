// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! On macOS, `AppPaths::new` gives directories in Application Support and
//! brings there the ones it gave up to 0.21.0 (`~/.config/<app>`,
//! `~/.local/share/<app>`). One test in a binary of its own, so that pointing
//! `HOME` at a temporary directory reaches no other test.

#![cfg(target_os = "macos")]

use std::fs;

use teksilo_settings::AppPaths;

#[test]
fn the_old_directories_move_to_application_support_once() {
    let home = tempfile::tempdir().unwrap();
    // SAFETY: the only test of this binary; no other thread reads the
    // environment while it runs.
    unsafe {
        std::env::set_var("HOME", home.path());
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::remove_var("XDG_DATA_HOME");
    }
    let old_config = home.path().join(".config/test-app");
    let old_data = home.path().join(".local/share/test-app");
    fs::create_dir_all(&old_config).unwrap();
    fs::create_dir_all(&old_data).unwrap();
    fs::write(old_config.join("general.toml"), "a = 1\n").unwrap();
    fs::write(old_data.join("window_state.toml"), "b = 2\n").unwrap();

    let paths = AppPaths::new("eu", "FernTech", "Test App").unwrap();
    let root = home
        .path()
        .join("Library/Application Support/eu.ferntech.Test-App");
    assert_eq!(paths.config_dir(), root.join("config"));
    assert_eq!(paths.data_dir(), root.join("data"));
    assert_eq!(
        fs::read_to_string(paths.config_file("general")).unwrap(),
        "a = 1\n"
    );
    assert_eq!(
        fs::read_to_string(paths.data_file("window_state")).unwrap(),
        "b = 2\n"
    );
    assert!(!old_config.exists());
    assert!(!old_data.exists());

    // The next start finds the new places and moves nothing, even when the
    // old ones have been written again (by an earlier version, say).
    fs::create_dir_all(&old_config).unwrap();
    fs::write(old_config.join("general.toml"), "a = 9\n").unwrap();
    let again = AppPaths::new("eu", "FernTech", "Test App").unwrap();
    assert_eq!(
        fs::read_to_string(again.config_file("general")).unwrap(),
        "a = 1\n"
    );
    assert!(old_config.join("general.toml").exists());
}
