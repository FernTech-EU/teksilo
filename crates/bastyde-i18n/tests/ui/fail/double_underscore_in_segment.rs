// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Compile-fail: `__` inside a path segment is reserved as the
//! nested-module separator, so the macro refuses to accept it.

use bastyde_i18n::tr;

fn main() {
    // Written as a single segment `foo__bar` — but `__` is reserved
    // for nesting. The macro expects either `foo_bar` (single
    // underscore inside one segment) or `foo::bar` (two segments).
    let _ = tr!(foo__bar());
}
