// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `tr!` with a single named argument matching the message signature.

use bastyde_i18n::{I18nConfig, I18nManager, tr};

fn main() {
    // Install a minimal manager so the runtime path resolves the
    // argument substitution — the compile-time fallback only covers
    // zero-arg pure-text messages, so this case proves the macro's
    // runtime interop too.
    let cfg = I18nConfig::test_only("en-US", &[("welcome", "Hello, { $name }!")]);
    let mgr = I18nManager::from_config(&cfg);
    bastyde_i18n::thread_local::install(mgr);

    let name = String::from("Alice");
    let ls = tr!(welcome(name = name));
    assert_eq!(ls.resolve_now(), "Hello, Alice!");
}
