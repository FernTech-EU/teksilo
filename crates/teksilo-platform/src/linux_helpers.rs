// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Shared helpers for querying Linux desktop settings via subprocess calls.
//!
//! Used by both `accessibility_prefs` and `os_theme` modules.
//! These are `pub(crate)` — internal to teksilo-platform.

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

/// Parse the stdout of `busctl get-property` for a `b` (boolean) property.
///
/// `busctl` prints a boolean property as the type code and the value on one
/// line: `b true` or `b false`. Anything else — a different type code, an error
/// string, an empty read — is `None` rather than a guess, because every caller
/// treats "cannot tell" and "no" differently.
///
/// Split out from [`read_dbus_bool_property`] so the format this depends on can
/// be pinned by a test without a session bus.
pub(crate) fn parse_busctl_bool(stdout: &str) -> Option<bool> {
    let mut tokens = stdout.split_whitespace();
    if tokens.next()? != "b" {
        return None;
    }
    let value = match tokens.next()? {
        "true" => true,
        "false" => false,
        _ => return None,
    };
    // A well-formed boolean reply is exactly two tokens. More means the reply
    // was something else that happens to start `b true`.
    if tokens.next().is_some() {
        return None;
    }
    Some(value)
}

/// Read a boolean D-Bus property from the session bus via `busctl`.
///
/// A different verb from [`read_portal_u32`], which *calls*
/// `org.freedesktop.portal.Settings.ReadOne`; this reads a property directly
/// off an interface, which is how the AT-SPI status object exposes its flags.
///
/// `None` when `busctl` is absent, the service is not running, the sandbox
/// cannot reach the session bus, or the reply is not a boolean. Never panics.
pub(crate) fn read_dbus_bool_property(
    service: &str,
    path: &str,
    interface: &str,
    property: &str,
) -> Option<bool> {
    let output = std::process::Command::new("busctl")
        .args(["--user", "get-property", service, path, interface, property])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    parse_busctl_bool(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(test)]
mod tests {
    use super::parse_busctl_bool;

    #[test]
    fn busctl_bool_replies_parse() {
        assert_eq!(parse_busctl_bool("b true\n"), Some(true));
        assert_eq!(parse_busctl_bool("b false\n"), Some(false));
        // Leading/trailing whitespace is `busctl`'s, not ours.
        assert_eq!(parse_busctl_bool("  b   true  "), Some(true));
    }

    #[test]
    fn non_boolean_replies_are_unknown_not_false() {
        // A different type code: the property exists but is not a boolean.
        assert_eq!(parse_busctl_bool("u 1\n"), None);
        assert_eq!(parse_busctl_bool("s \"true\"\n"), None);
        // An empty or truncated read.
        assert_eq!(parse_busctl_bool(""), None);
        assert_eq!(parse_busctl_bool("b\n"), None);
        // A value that is neither of D-Bus's two boolean spellings.
        assert_eq!(parse_busctl_bool("b 1\n"), None);
        // A longer reply that merely begins like a boolean one.
        assert_eq!(parse_busctl_bool("b true extra\n"), None);
    }
}
