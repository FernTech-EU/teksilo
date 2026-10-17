// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Shared helpers for querying Linux desktop settings: the desktop in use,
//! and GNOME's settings through the `gsettings` tool.
//!
//! Used by both `accessibility_prefs` and `os_theme` modules.
//! These are `pub(crate)`, internal to teksilo-platform. The D-Bus reads (the
//! XDG desktop portal and AT-SPI) are in `linux_portal`.

/// Read a gsettings value via the `gsettings` CLI tool.
///
/// Returns the raw string output (trimmed, with surrounding quotes stripped).
pub(crate) fn read_gsettings(schema: &str, key: &str) -> Option<String> {
    let output = std::process::Command::new("gsettings")
        .args(["get", schema, key])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // gsettings wraps strings in single quotes: 'Adwaita'
    Some(value.trim_matches('\'').to_string())
}

/// Detect the current desktop environment from `$XDG_CURRENT_DESKTOP`.
#[allow(dead_code)]
pub(crate) enum Desktop {
    Gnome,
    Kde,
    Cinnamon,
    Other(String),
}

pub(crate) fn detect_desktop() -> Desktop {
    let xdg = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    // XDG_CURRENT_DESKTOP can contain colon-separated values, e.g. "ubuntu:GNOME"
    let upper = xdg.to_uppercase();
    if upper.contains("GNOME") {
        Desktop::Gnome
    } else if upper.contains("KDE") {
        Desktop::Kde
    } else if upper.contains("CINNAMON") || upper.contains("X-CINNAMON") {
        Desktop::Cinnamon
    } else {
        Desktop::Other(xdg)
    }
}
