//! Fluent (Windows 11) theme preset for Bastyde.
//!
//! **Stub.** The constructors return a `bastyde_core::Theme` shaped from
//! the IntUI baseline today; the per-tier customisation work
//! (Mica/Acrylic surfaces, Fluent accent palette, reveal-effect
//! buttons, segoe-fluent-icons typography pairing) lands incrementally
//! as the four-tier styling refactor finishes the remaining widget
//! migrations.
//!
//! Apps that opt into this crate today get a baseline theme they can
//! override piece-by-piece (`theme.colors.accent = …`,
//! `theme.style_slots.button = Some(Rc::new(MyFluentButton))`). The
//! `// TODO(fluent)` markers below name the slots that will be filled
//! in by the dedicated Fluent styles.
//!
//! ```ignore
//! use bastyde_theme_fluent as fluent;
//!
//! BastydeAppBuilder::new()
//!     .theme(fluent::light())
//!     .initial_window(WindowConfig::new().title("Fluent demo"))
//!     .run();
//! ```

use bastyde_core::presets::intui;
use bastyde_core::styles::{Theme, ThemeAppearance};

/// Fluent light theme. Stub — currently returns IntUI light.
pub fn light() -> Theme {
    let mut theme = intui::light();
    apply_fluent_overrides(&mut theme, ThemeAppearance::Light);
    theme
}

/// Fluent dark theme. Stub — currently returns IntUI dark.
pub fn dark() -> Theme {
    let mut theme = intui::dark();
    apply_fluent_overrides(&mut theme, ThemeAppearance::Dark);
    theme
}

fn apply_fluent_overrides(_theme: &mut Theme, _appearance: ThemeAppearance) {
    // TODO(fluent): swap in the Fluent accent palette (the user's
    // Windows accent if exposed via the OS, falling back to the
    // Fluent system blue `#0078D4` for static selection).

    // TODO(fluent): install per-widget Fluent styles in
    // `theme.style_slots.*`. Approximate slot priority:
    //   - button: rounded-rect with the Fluent reveal effect on hover
    //     (subtle radial highlight tracking the cursor — needs a
    //     custom paint scope, which the trait surface supports).
    //   - text_input: bottom-border-only baseline that thickens to
    //     accent on focus (the Fluent under-line variant).
    //   - card: Mica/Acrylic-aware surface (vibrancy-blur when the
    //     compositor supports it; solid `surface_raised` otherwise).
    //   - menu_item: 4 dp corner-radius row with the Fluent chevron
    //     glyph for submenus.
    //   - toggle: Fluent switch (animated track fill, accent thumb).
    //
    // Mica/Acrylic is the big architectural question — true vibrancy
    // needs compositor-side blur, which the wgpu backend doesn't
    // expose today. The stub falls back to solid surfaces.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_appearance_is_light() {
        assert!(light().appearance.is_light());
    }

    #[test]
    fn dark_appearance_is_dark() {
        assert!(dark().appearance.is_dark());
    }
}
