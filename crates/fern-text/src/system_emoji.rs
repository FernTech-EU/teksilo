//! Runtime loader for the platform's installed color-emoji font.
//!
//! Enabled by the `system-emoji` Cargo feature. Unlike the embedded
//! `fonts-*` fallbacks in [`typesetter_bridge`](crate::typesetter_bridge),
//! this module does not ship any font data: color-emoji fonts are
//! typically 20-30 MB and would bloat the binary. Instead, at
//! startup we probe a short per-OS list of well-known filesystem
//! paths and register the first one found. Silent on miss — the
//! caller sees `None` and the shaper's `.notdef` fallback loop
//! simply has no emoji font to consult.

#[cfg(target_os = "linux")]
const CANDIDATES: &[&str] = &[
    "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf",
    "/usr/share/fonts/noto/NotoColorEmoji.ttf",
    "/usr/share/fonts/NotoColorEmoji.ttf",
    "/usr/local/share/fonts/NotoColorEmoji.ttf",
];

#[cfg(target_os = "macos")]
const CANDIDATES: &[&str] = &["/System/Library/Fonts/Apple Color Emoji.ttc"];

#[cfg(target_os = "windows")]
const CANDIDATES: &[&str] = &["C:/Windows/Fonts/seguiemj.ttf"];

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
const CANDIDATES: &[&str] = &[];

/// Return the bytes of the first emoji font found at a well-known
/// system path, or `None` if none of the candidates exist.
pub(crate) fn load_system_emoji_data() -> Option<Vec<u8>> {
    CANDIDATES.iter().find_map(|path| std::fs::read(path).ok())
}
