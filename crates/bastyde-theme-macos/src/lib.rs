// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! macOS theme preset for Bastyde.
//!
//! **Stub.** The constructors return a `bastyde_core::Theme` shaped from
//! the IntUI baseline today; the per-tier customisation work
//! (Aqua-style accent colors, vibrancy-aware surfaces, capsule pill
//! buttons, soft pull-down menus) lands incrementally as the
//! four-tier styling refactor finishes the remaining widget
//! migrations.
//!
//! Apps that opt into this crate today get a baseline theme they can
//! override piece-by-piece (`theme.colors.accent = …`,
//! `theme.style_slots.button = Some(Rc::new(MyMacButton))`). The
//! `// TODO(macos)` markers below name the slots that will be filled
//! in by the dedicated macOS styles.
//!
//! ```ignore
//! use bastyde_theme_macos as macos;
//!
//! BastydeAppBuilder::new()
//!     .theme(macos::light())
//!     .initial_window(WindowConfig::new().title("macOS demo"))
//!     .run();
//! ```

use bastyde_core::presets::intui;
use bastyde_core::styles::{Theme, ThemeAppearance};

/// macOS light theme. Stub — currently returns IntUI light.
pub fn light() -> Theme {
    let mut theme = intui::light();
    apply_macos_overrides(&mut theme, ThemeAppearance::Light);
    theme
}

/// macOS dark theme. Stub — currently returns IntUI dark.
pub fn dark() -> Theme {
    let mut theme = intui::dark();
    apply_macos_overrides(&mut theme, ThemeAppearance::Dark);
    theme
}

fn apply_macos_overrides(_theme: &mut Theme, _appearance: ThemeAppearance) {
    // TODO(macos): swap in the Aqua accent palette (system blue by
    // default, user-configurable via the OS Appearance pref pane —
    // we'd surface a `macos::with_accent(SystemAccent::Graphite)`
    // helper for static selection without OS hooks).

    // TODO(macos): install per-widget macOS styles in
    // `theme.style_slots.*`. Approximate slot priority:
    //   - button: capsule pill with subtle inner highlight + outer
    //     ring on focus (matches macOS 12+ button).
    //   - toggle: macOS Switch (no knob track-fill, knob slides on
    //     subtle blue track).
    //   - text_input: rounded-bezel field with focus-ring blue glow.
    //   - menu_item: low-contrast hover (system grey 5), chevron
    //     using the SF Pro arrow glyph.
    //   - card: vibrancy-aware translucent surface (when wgpu
    //     compositing supports it; degrades to solid `surface_main`
    //     otherwise).
    //
    // Vibrancy is the big architectural question — true macOS
    // vibrancy needs the compositor to blur the background behind
    // the window, which the wgpu backend doesn't expose today. The
    // stub falls back to solid surfaces.
}
