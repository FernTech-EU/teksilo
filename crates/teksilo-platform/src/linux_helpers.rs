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
    detect_desktop_in(|name| std::env::var(name).ok())
}

/// The desktop the environment `var` describes. A KDE session is
/// recognised first, by KDE's own rule
/// ([`teksilo_kde_scheme::is_kde_session`]: `KDE` among the
/// colon-separated `XDG_CURRENT_DESKTOP` names, ignoring ASCII case, or
/// `KDE_FULL_SESSION=true`); the other desktops by a case-insensitive
/// substring of `XDG_CURRENT_DESKTOP`.
pub(crate) fn detect_desktop_in(var: impl Fn(&str) -> Option<String>) -> Desktop {
    if teksilo_kde_scheme::is_kde_session(&var) {
        return Desktop::Kde;
    }
    let xdg = var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    // XDG_CURRENT_DESKTOP can contain colon-separated values, e.g. "ubuntu:GNOME"
    let upper = xdg.to_uppercase();
    if upper.contains("GNOME") {
        Desktop::Gnome
    } else if upper.contains("CINNAMON") || upper.contains("X-CINNAMON") {
        Desktop::Cinnamon
    } else {
        Desktop::Other(xdg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desktop(vars: &[(&str, &str)]) -> Desktop {
        detect_desktop_in(|name| {
            vars.iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| (*v).to_owned())
        })
    }

    #[test]
    fn a_kde_session_is_kde_in_the_desktop_list_or_a_full_session() {
        for vars in [
            &[("XDG_CURRENT_DESKTOP", "KDE")][..],
            &[("XDG_CURRENT_DESKTOP", "kde")],
            &[("XDG_CURRENT_DESKTOP", "neon:KDE")],
            &[("XDG_CURRENT_DESKTOP", "Kde:Plasma")],
            &[("KDE_FULL_SESSION", "true")],
            &[
                ("XDG_CURRENT_DESKTOP", "GNOME"),
                ("KDE_FULL_SESSION", "true"),
            ],
        ] {
            assert!(matches!(desktop(vars), Desktop::Kde), "{vars:?}");
        }
        for vars in [
            &[("XDG_CURRENT_DESKTOP", "X-KDE-Other")][..],
            &[("XDG_CURRENT_DESKTOP", "KDEx")],
            &[("KDE_FULL_SESSION", "1")],
            &[("KDE_FULL_SESSION", "false")],
            &[],
        ] {
            assert!(!matches!(desktop(vars), Desktop::Kde), "{vars:?}");
        }
    }

    #[test]
    fn the_other_desktops_are_read_from_the_desktop_list() {
        assert!(matches!(
            desktop(&[("XDG_CURRENT_DESKTOP", "ubuntu:GNOME")]),
            Desktop::Gnome
        ));
        assert!(matches!(
            desktop(&[("XDG_CURRENT_DESKTOP", "X-Cinnamon")]),
            Desktop::Cinnamon
        ));
        assert!(matches!(
            desktop(&[("XDG_CURRENT_DESKTOP", "XFCE")]),
            Desktop::Other(name) if name == "XFCE"
        ));
    }
}
