// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Off-screen PNG snapshot of the current canvas.
//!
//! The rendering itself lives in [`crate::shot`] — shared with the
//! documentation image exporter, so a toolbar export and a `--export-docs`
//! run produce the same picture. This module only resolves *what* to
//! render (the selected widget/variant plus its live knob values, at the
//! toolbar's live density) and *where* to save it.

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
    // The canvas is showing the widget at the toolbar's density, so the
    // export has to be that widget: a theme resolved without one is the
    // Compact ladder, which is a different picture.
    let pass = teksilo_preview::PreviewPass::new(state.density.get());
    let theme = canvas_theme.theme_at(pass.density());
    let label = theme_label(canvas_theme, &theme);

    let mut shooter = Shooter::new(EXPORT_SCALE)?;
    let shot = shooter.capture(widget, theme, &ShotOptions::default().with_pass(pass))?;

    let out_path = output_path(widget_id, variant_name, label, pass)?;
    write_png(&out_path, &shot.rgba, shot.width, shot.height)?;
    Ok(out_path)
}

/// The theme part of an export's file name for `canvas_theme`, which
/// resolved to `theme`. A `Native` canvas is named for what it rendered,
/// "native-light" or "native-dark", read from `theme` itself: the desktop
/// is read once, by [`CanvasTheme::theme_at`](crate::app_state::CanvasTheme::theme_at),
/// so the name cannot disagree with the picture.
fn theme_label(
    canvas_theme: crate::app_state::CanvasTheme,
    theme: &teksilo_core::Theme,
) -> &'static str {
    match canvas_theme {
        crate::app_state::CanvasTheme::Light => "light",
        crate::app_state::CanvasTheme::Dark => "dark",
        crate::app_state::CanvasTheme::Native if theme.is_dark() => "native-dark",
        crate::app_state::CanvasTheme::Native => "native-light",
    }
}

fn output_path(
    widget_id: &'static str,
    variant_name: &'static str,
    theme_label: &'static str,
    pass: teksilo_preview::PreviewPass,
) -> Result<PathBuf, String> {
    let mut out_dir = home_dir().ok_or_else(|| "couldn't resolve home directory".to_string())?;
    out_dir.push(".teksilo-previewer");
    out_dir.push("exports");
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("create_dir_all: {}", e))?;
    // The density rides in the stem the same way it does for the catalog
    // images, so a Touch export cannot silently overwrite a Compact one.
    Ok(out_dir.join(format!(
        "{}.png",
        pass.image_stem(&format!("{}__{}__{}", widget_id, variant_name, theme_label))
    )))
}

fn home_dir() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("HOME")
        && !home.is_empty()
    {
        return Some(PathBuf::from(home));
    }
    #[cfg(windows)]
    if let Ok(home) = std::env::var("USERPROFILE")
        && !home.is_empty()
    {
        return Some(PathBuf::from(home));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state::CanvasTheme;
    use teksilo_core::presets::intui;

    #[test]
    fn a_native_export_is_named_for_the_theme_it_rendered() {
        assert_eq!(
            theme_label(CanvasTheme::Native, &intui::dark()),
            "native-dark"
        );
        assert_eq!(
            theme_label(CanvasTheme::Native, &intui::light()),
            "native-light"
        );
        assert_eq!(theme_label(CanvasTheme::Light, &intui::light()), "light");
        assert_eq!(theme_label(CanvasTheme::Dark, &intui::dark()), "dark");
    }
}
