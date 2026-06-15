// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Shared helpers for querying Linux desktop settings via subprocess calls.
//!
//! Used by both `accessibility_prefs` and `os_theme` modules.
//! These are `pub(crate)` — internal to bastyde-platform.

/// Read a u32 value from the XDG Desktop Portal Settings via `busctl`.
///
/// The portal method `org.freedesktop.portal.Settings.ReadOne` returns
/// a `Variant<Variant<u32>>`. `busctl` prints this as e.g. `v u 1`.
pub(crate) fn read_portal_u32(namespace: &str, key: &str) -> Option<u32> {
    let output = std::process::Command::new("busctl")
        .args([
            "--user",
            "call",
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Settings",
            "ReadOne",
            "ss",
            namespace,
            key,
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    // Output format: "v u <value>\n" — extract the last whitespace-separated token
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.split_whitespace().last()?.parse::<u32>().ok()
}

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

/// Read an RGB tuple from the XDG Desktop Portal via `busctl`.
///
/// The portal `accent-color` key returns `(ddd)` — a struct of three doubles.
/// `busctl` prints this as e.g. `v "(ddd)" 0.2078 0.5176 0.8941`.
pub(crate) fn read_portal_rgb(namespace: &str, key: &str) -> Option<(f64, f64, f64)> {
    let output = std::process::Command::new("busctl")
        .args([
            "--user",
            "call",
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Settings",
            "ReadOne",
            "ss",
            namespace,
            key,
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Parse the three doubles from the output.
    // busctl wraps in variant layers: could be `v "(ddd)" R G B`
    // or `v v "(ddd)" R G B`. Parse from the end to be robust.
    let parts: Vec<&str> = stdout.split_whitespace().collect();
    if parts.len() >= 3 {
        let b = parts[parts.len() - 1].parse::<f64>().ok()?;
        let g = parts[parts.len() - 2].parse::<f64>().ok()?;
        let r = parts[parts.len() - 3].parse::<f64>().ok()?;
        // Sanity check: RGB doubles should be in 0.0..=1.0
        if (0.0..=1.0).contains(&r) && (0.0..=1.0).contains(&g) && (0.0..=1.0).contains(&b) {
            Some((r, g, b))
        } else {
            None
        }
    } else {
        None
    }
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
