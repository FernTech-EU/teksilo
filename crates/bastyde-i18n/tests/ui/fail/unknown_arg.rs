// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Compile-fail: `welcome` only declares `$name`, not `$extra`.

use bastyde_i18n::tr;

fn main() {
    let name = String::from("A");
    let extra = String::from("B");
    let _ = tr!(welcome(name = name, extra = extra));
}
