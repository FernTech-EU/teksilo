// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Runtime loader for the platform's installed color-emoji font.
//!
//! Enabled by the `system-emoji` Cargo feature. Unlike the embedded
//! `fonts-*` fallbacks in [`typesetter_bridge`](crate::typesetter_bridge),
//! this module does not ship any font data: color-emoji fonts are
//! typically 20-30 MB (Linux/Windows) or ~180 MB (macOS) and would
//! bloat the binary. Instead, at startup we probe a short per-OS list
//! of well-known filesystem paths and `mmap` the first one found.
//! Silent on miss — the caller sees `None` and the shaper's `.notdef`
//! fallback loop simply has no emoji font to consult.
//!
//! ## Why mmap instead of `fs::read`
//!
//! Apple Color Emoji.ttc is ~183 MB. With `fs::read` the kernel reads
//! the whole file into an owned `Vec<u8>` on the heap — that's 183 MB
//! of anonymous memory that lives for the process lifetime, even
//! though shaping touches only a fraction of the glyph tables.
//! `memmap2::Mmap` maps the file pages into the address space; the
//! kernel pages them in on demand and can reclaim them under memory
//! pressure. text-typeset's `register_font_shared` accepts an
//! `Arc<Mmap>` directly, so the bytes are never copied to the heap.

use std::path::Path;
use std::sync::Arc;

use text_typeset::SharedFontData;

#[cfg(target_os = "linux")]
const CANDIDATES: &[&str] = &[
    "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf",
    "/usr/share/fonts/noto/NotoColorEmoji.ttf",
    "/usr/share/fonts/NotoColorEmoji.ttf",
    "/usr/local/share/fonts/NotoColorEmoji.ttf",
    // openSUSE.
    "/usr/share/fonts/truetype/NotoColorEmoji.ttf",
    // Fedora up to 43: the CBDT build.
    "/usr/share/fonts/google-noto-emoji/NotoColorEmoji.ttf",
    "/usr/share/fonts/google-noto-color-emoji-fonts/NotoColorEmoji.ttf",
    // Fedora 44: the COLRv1 build, which text-typeset paints (`atlas/colr.rs`).
    // After the bitmap builds, which every renderer draws.
    "/usr/share/fonts/google-noto-color-emoji-fonts/Noto-COLRv1.ttf",
];

#[cfg(target_os = "macos")]
const CANDIDATES: &[&str] = &["/System/Library/Fonts/Apple Color Emoji.ttc"];

#[cfg(target_os = "windows")]
const CANDIDATES: &[&str] = &["C:/Windows/Fonts/seguiemj.ttf"];

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
const CANDIDATES: &[&str] = &[];

/// Map the first emoji font found at a well-known system path into
/// the process address space, returning a shared handle suitable for
/// [`text_typeset::TextFontService::register_font_shared`]. Returns
/// `None` if no candidate exists.
pub(crate) fn load_system_emoji_data() -> Option<SharedFontData> {
    let path = first_existing(CANDIDATES, |path| Path::new(path).is_file())?;
    mmap_font(path)
}

/// The first of `candidates` that `exists`, in list order.
fn first_existing<'a>(candidates: &[&'a str], exists: impl Fn(&str) -> bool) -> Option<&'a str> {
    candidates.iter().copied().find(|path| exists(path))
}

fn mmap_font(path: &str) -> Option<SharedFontData> {
    let file = std::fs::File::open(path).ok()?;
    // SAFETY: mmap is unsafe because if another process mutates the
    // file under us, the mapped bytes change too. System emoji fonts
    // live in protected OS directories (`/System/...`, `/usr/share/...`,
    // `C:/Windows/...`) that only an admin can rewrite, so the risk
    // is the same as for any other system asset we load.
    let mmap = unsafe { memmap2::Mmap::map(&file).ok()? };
    Some(Arc::new(mmap))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_candidate_that_exists_wins() {
        let list = ["/a/one.ttf", "/b/two.ttf", "/c/three.ttf"];
        assert_eq!(
            first_existing(&list, |p| p != "/a/one.ttf"),
            Some("/b/two.ttf")
        );
        assert_eq!(first_existing(&list, |_| true), Some("/a/one.ttf"));
        assert_eq!(first_existing(&list, |_| false), None);
    }

    /// A file that is in the list but absent is skipped, not an end of the
    /// search: a distribution has one of the paths, not the first.
    #[cfg(target_os = "linux")]
    #[test]
    fn fedora_44_is_found_where_only_its_font_exists() {
        const FEDORA: &str = "/usr/share/fonts/google-noto-color-emoji-fonts/Noto-COLRv1.ttf";
        assert_eq!(first_existing(CANDIDATES, |p| p == FEDORA), Some(FEDORA));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_bitmap_build_is_preferred_to_the_colrv1_one() {
        let colr = "Noto-COLRv1.ttf";
        let found = first_existing(CANDIDATES, |_| true).unwrap();
        assert!(!found.ends_with(colr));
        let last = CANDIDATES.last().unwrap();
        assert!(last.ends_with(colr));
    }
}
