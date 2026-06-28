// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Material 3 theme preset for Bastyde.
//!
//! **Stub.** The constructors return a `bastyde_core::Theme` shaped from
//! the IntUI baseline today; the per-tier customisation work
//! (Material-spec accent palette, `RecipeButtonStyle` with M3
//! pill-shaped 40 dp surfaces, M3 elevation-on-hover, etc.) lands
//! incrementally in later commits as the four-tier styling refactor
//! finishes the remaining widget migrations.
//!
//! Apps that opt into this crate today get a baseline theme they can
//! override piece-by-piece (`theme.colors.accent = …`,
//! `theme.style_slots.button = Some(Rc::new(MyM3Button))`). The
//! `// TODO(material3)` markers below name the slots that will be
//! filled in by the dedicated M3 styles.
//!
//! ```ignore
//! use bastyde_theme_material3 as m3;
//!
//! BastydeAppBuilder::new()
//!     .theme(m3::light())
//!     .initial_window(WindowConfig::new().title("M3 demo"))
//!     .run();
//! ```

use bastyde_core::presets::intui;
use bastyde_core::styles::{Theme, ThemeAppearance};

/// Material 3 light theme. Stub — currently returns IntUI light.
pub fn light() -> Theme {
    let mut theme = intui::light().with_id("material3.light");
    apply_material3_overrides(&mut theme, ThemeAppearance::Light);
    theme
}

/// Material 3 dark theme. Stub — currently returns IntUI dark.
pub fn dark() -> Theme {
    let mut theme = intui::dark().with_id("material3.dark");
    apply_material3_overrides(&mut theme, ThemeAppearance::Dark);
    theme
}

fn apply_material3_overrides(_theme: &mut Theme, _appearance: ThemeAppearance) {
    // TODO(material3): swap in the M3 accent palette here. The M3
    // dynamic-color algorithm derives a 13-step tonal palette from a
    // single source color; for the static preset we'd ship a fixed
    // palette tuned to the M3 reference (purple primary, grey-blue
    // surface tints).

    // TODO(material3): install per-widget M3 styles in
    // `theme.style_slots.*`. Approximate slot priority (highest
    // user-visible impact first):
    //   - button: pill-shaped 40 dp with elevation lift on hover.
    //   - text_input: filled variant with floating label.
    //   - card: elevated surfaces with `shadow_md` + 12 dp radius.
    //   - checkbox: M3 rounded-square with check-glyph animation.
    //   - radio: M3 ring with growing-ripple inner dot.
    //   - toggle: M3 switch with extending track tint.
    //
    // Each goes behind its own commit + visual-fidelity check
    // against the M3 spec (`https://m3.material.io/components/...`).
}
