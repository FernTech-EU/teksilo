// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Window icon.
//!
//! A raw RGBA8 bitmap plus dimensions. The app-level window manager
//! converts this into the winit `Icon` type at window-creation time.

/// Window icon as raw RGBA8 bytes (4 bytes per pixel, row-major,
/// top-left origin).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowIcon {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl WindowIcon {
    /// Construct an icon from a row-major RGBA8 buffer.
    ///
    /// The buffer must contain exactly `width * height * 4` bytes. The
    /// app-level manager validates this when converting to the
    /// platform icon and logs + drops the icon on mismatch — the
    /// window still opens, just without a custom icon.
    pub fn from_rgba(rgba: Vec<u8>, width: u32, height: u32) -> Self {
        Self {
            rgba,
            width,
            height,
        }
    }

    /// Expected buffer size in bytes for `width × height` RGBA8.
    pub fn expected_len(&self) -> usize {
        (self.width as usize) * (self.height as usize) * 4
    }

    /// `true` when `rgba.len()` matches `width × height × 4`.
    pub fn is_valid(&self) -> bool {
        self.rgba.len() == self.expected_len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_icon_passes_check() {
        let icon = WindowIcon::from_rgba(vec![0; 16 * 16 * 4], 16, 16);
        assert!(icon.is_valid());
    }

    #[test]
    fn mismatched_len_is_invalid() {
        let icon = WindowIcon::from_rgba(vec![0; 100], 16, 16);
        assert!(!icon.is_valid());
    }
}
