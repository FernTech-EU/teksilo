// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Compile-fail: `welcome` requires a `$name` argument.

use bastyde_i18n::tr;

fn main() {
    let _ = tr!(welcome());
}
