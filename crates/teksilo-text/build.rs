// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Decide which bundled script fonts are actually available to embed.
//!
//! Each `fonts-*` feature names one Noto Sans face that
//! `TypesetterBridge::register_default_font` embeds with `include_bytes!`.
//! `include_bytes!` resolves at compile time, so a feature whose file is not
//! in `fonts/` used to be a **hard build error** — and because
//! `--all-features` turns on every feature, the whole workspace could not be
//! built that way on any revision. Five of the seven faces have never been
//! committed, so five features were permanently unbuildable and the two
//! meta-features that include them (`fonts-all`, `fonts-all-non-cjk`) with
//! them.
//!
//! So the file's presence is decided here instead. A feature that is on and
//! whose face is present emits its `font_*` cfg and embeds as before; one
//! whose face is missing emits a build **warning** naming the exact path to
//! drop it at, and registers nothing. The reminder the old hard error existed
//! to give is kept; what is dropped is its power to break an unrelated build.
//!
//! Adding a face is therefore only ever: put the file in `fonts/`, add its row
//! below and its `#[cfg(font_*)]` block in `typesetter_bridge.rs`.

use std::path::Path;

/// `(cargo feature, cfg emitted when present, path from the crate root)`.
///
/// The path carries its `fonts/` directory rather than a bare filename because
/// `typos` skips a path-like string and flags a bare one, and these names
/// encode OpenType axis tags that read as misspelled English words.
const FACES: &[(&str, &str, &str)] = &[
    (
        "FONTS_ARABIC",
        "font_arabic",
        "fonts/NotoSansArabic-VariableFont_wdth,wght.ttf",
    ),
    (
        "FONTS_HEBREW",
        "font_hebrew",
        "fonts/NotoSansHebrew-VariableFont_wdth,wght.ttf",
    ),
    (
        "FONTS_THAI",
        "font_thai",
        "fonts/NotoSansThai-VariableFont_wdth,wght.ttf",
    ),
    (
        "FONTS_DEVANAGARI",
        "font_devanagari",
        "fonts/NotoSansDevanagari-VariableFont_wdth,wght.ttf",
    ),
    (
        "FONTS_CJK_SC",
        "font_cjk_sc",
        "fonts/NotoSansSC-VariableFont_wght.ttf",
    ),
    (
        "FONTS_CJK_JP",
        "font_cjk_jp",
        "fonts/NotoSansJP-VariableFont_wght.ttf",
    ),
    (
        "FONTS_CJK_KR",
        "font_cjk_kr",
        "fonts/NotoSansKR-VariableFont_wght.ttf",
    ),
];

fn main() {
    // A face dropped into place must take effect without a clean build.
    println!("cargo::rerun-if-changed=fonts");
    println!("cargo::rerun-if-changed=build.rs");

    for (_, cfg, _) in FACES {
        println!("cargo::rustc-check-cfg=cfg({cfg})");
    }

    for (feature, cfg, file) in FACES {
        if std::env::var_os(format!("CARGO_FEATURE_{feature}")).is_none() {
            continue;
        }
        if Path::new(file).exists() {
            println!("cargo::rustc-cfg={cfg}");
        } else {
            // Not an error: see the module doc. Named precisely enough to act
            // on without opening this file.
            println!(
                "cargo::warning=teksilo-text: feature `{}` is enabled but \
                 `crates/teksilo-text/{}` is not present, so that script \
                 has no embedded fallback. Drop the file in to enable it, or \
                 turn the feature off.",
                feature.to_lowercase().replace('_', "-"),
                file
            );
        }
    }
}
