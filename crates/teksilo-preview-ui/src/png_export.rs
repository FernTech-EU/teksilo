// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Off-screen PNG snapshot of the current canvas.
//!
//! The rendering itself lives in [`crate::shot`] — shared with the
//! documentation image exporter, so a toolbar export and a `--export-docs`
//! run produce the same picture. This module only resolves *what* to
//! render (the selected widget/variant plus its live knob values) and
//! *where* to save it.

use std::path::PathBuf;

use crate::app_state::AppState;
use crate::shot::{Shooter, ShotOptions, write_png};

/// HiDPI factor for a manual export. Matches the doc exporter so a
/// hand-taken snapshot can be dropped into `docs/widgets/img/` as-is.
const EXPORT_SCALE: f32 = 2.0;

/// Export the currently selected (widget, variant) to a PNG at
/// `~/.teksilo-previewer/exports/<widget>__<variant>__<theme>.png`. The
/// directory is created on demand. Returns the saved path or a
/// human-readable error.
pub fn export_current(state: &AppState) -> Result<PathBuf, String> {
    let (widget_id, variant_name) =
        match (state.selected_widget.get(), state.selected_variant.get()) {
            (Some(w), Some(v)) => (w, v),
            _ => return Err("no widget/variant selected".into()),
        };

    let entry = teksilo_preview::find_by_id(widget_id)
        .ok_or_else(|| format!("no entry registered with id '{}'", widget_id))?;
    let knobs = state.knobs_for(widget_id, variant_name);
    let widget = entry.build(variant_name, &knobs);

    let canvas_theme = state.canvas_theme.get();
    let theme = canvas_theme.theme();

    let mut shooter = Shooter::new(EXPORT_SCALE)?;
    let shot = shooter.capture(widget, theme, &ShotOptions::default())?;

    let out_path = output_path(widget_id, variant_name, canvas_theme)?;
    write_png(&out_path, &shot.rgba, shot.width, shot.height)?;
    Ok(out_path)
}

fn output_path(
    widget_id: &'static str,
    variant_name: &'static str,
    canvas_theme: crate::app_state::CanvasTheme,
) -> Result<PathBuf, String> {
    let mut out_dir = home_dir().ok_or_else(|| "couldn't resolve home directory".to_string())?;
    out_dir.push(".teksilo-previewer");
    out_dir.push("exports");
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("create_dir_all: {}", e))?;
    // Resolve the chosen theme to its concrete light/dark identity
    // for the filename. `Native` resolves at click-time via
    // `theme()`; we mirror that resolution here so an export taken
    // while "Native" is selected still gets a meaningful suffix
    // ("native-light" / "native-dark") rather than a bare "native"
    // that drops information about what was actually rendered.
    let theme_label: &'static str = match canvas_theme {
        crate::app_state::CanvasTheme::Light => "light",
        crate::app_state::CanvasTheme::Dark => "dark",
        crate::app_state::CanvasTheme::Native => {
            if teksilo_platform::os_theme::query_color_scheme().is_dark() {
                "native-dark"
            } else {
                "native-light"
            }
        }
    };
    Ok(out_dir.join(format!(
        "{}__{}__{}.png",
        widget_id, variant_name, theme_label
    )))
}

fn home_dir() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("HOME")
        && !home.is_empty()
    {
        return Some(PathBuf::from(home));
    }
    #[cfg(windows)]
    if let Ok(home) = std::env::var("USERPROFILE") {
        if !home.is_empty() {
            return Some(PathBuf::from(home));
        }
    }
    None
}
