// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `tr!` with a zero-argument key that exists in the fixture.

use bastyde_i18n::tr;

fn main() {
    let ls = tr!(greeting());
    // Without an installed `I18nManager`, the compile-time fallback
    // baked in by the proc macro (Phase C + Phase E) returns the
    // source-language text.
    assert_eq!(ls.resolve_now(), "Hello");
}
