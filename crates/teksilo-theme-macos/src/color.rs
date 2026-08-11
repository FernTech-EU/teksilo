// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! AppKit semantic colours projected onto Teksilo's [`ColorTokens`].
//!
//! The source values live in [`crate::palette::MacOsPalette`]; this module
//! is the *projection* — one AppKit token chosen per Teksilo role, plus
//! the derivations AppKit has no counterpart for.
//!
//! ## Alpha
//!
//! AppKit publishes most of its foreground vocabulary as an alpha over the
//! appearance's base (`labelColor` is "black at 85 %", not `#262626`), and
//! Teksilo paints flat colours over the parent surface, which composites
//! the same way. Alpha is therefore **kept** wherever the wash is the
//! point — labels, hairlines, hover and press washes, the sheet scrim —
//! and **pre-composited** wherever a Teksilo slot must be opaque: window
//! and editor backgrounds, status backgrounds, and the accent fill family,
//! whose contrast against `text_on_accent` has to be predictable wherever
//! the control is dropped.
//!
//! ## Where this preset departs from AppKit
//!
//! Four departures, each marked at its assignment and covered by a test:
//!
//! - **The label grades are lifted.** AppKit's `secondaryLabelColor` is
//!   50 % black in Aqua and 55 % white in Dark Aqua; neither clears WCAG
//!   SC 1.4.3's 4.5:1 floor on every surface this preset paints, so
//!   `alpha_lifted` raises each until it does. Apple's own numbers stay
//!   on [`MacOsPalette`] — see
//!   [`SECONDARY_LABEL_ALPHA_LIGHT`](crate::palette::SECONDARY_LABEL_ALPHA_LIGHT).
//! - **`border_strong`** is a 45 % wash, not the 10 % `separatorColor`
//!   AppKit strokes an unchecked checkbox with. A control boundary needs
//!   3:1 (WCAG SC 1.4.11); AppKit's own hairline measures 1.25:1.
//! - **Status foregrounds** use the *Accessible* variant of each system
//!   colour rather than the Default one. `systemRed` Default measures
//!   3.55:1 on `textBackgroundColor`; the Accessible variant Apple ships
//!   for the "Increase contrast" setting is the same hue at 5.39:1.
//! - **`search_match_bg`** keeps the IntUI amber. AppKit's
//!   `findHighlightColor` is pure `#FFFF00` in *both* appearances because
//!   AppKit special-cases the text drawn on it to black; Teksilo has no
//!   search-match foreground token, so transcribing it would put white
//!   editor text on yellow in Dark Aqua (1.05:1). The literal is still
//!   carried on [`MacOsPalette::find_highlight`] for apps that control
//!   their own foreground.
//!
//! ## What AppKit has no token for
//!
//! Derived here, each marked at its assignment: `accent_subtle_bg`, the
//! status backgrounds, `text_link_hover`, `text_link_visited`, the
//! editor's current-line and code-block colours, `surface_alt_row`, and
//! the container ladder. `chart_palette` keeps the colourblind-safe
//! Okabe-Ito default — Apple publishes no data-visualisation sequence.

use teksilo_tokens::{Color, ColorTokens};

use crate::palette::{MacOsPalette, black_alpha, over, white_alpha};

/// macOS **Aqua** colour tokens.
pub fn macos_light_colors(p: &MacOsPalette) -> ColorTokens {
    let mut c = ColorTokens::light_default();
    apply(&mut c, p, Appearance::Light);
    c
}

/// macOS **Dark Aqua** colour tokens.
pub fn macos_dark_colors(p: &MacOsPalette) -> ColorTokens {
    let mut c = ColorTokens::dark_default();
    apply(&mut c, p, Appearance::Dark);
    c
}

/// The places the projection genuinely branches on appearance — Aqua
/// darkens to add emphasis, Dark Aqua lightens. Everywhere else the
/// difference already lives in the palette values.
#[derive(Copy, Clone, PartialEq)]
enum Appearance {
    Light,
    Dark,
}

impl Appearance {
    /// A neutral wash of `alpha` that reads as *more* on this appearance:
    /// black in Aqua, white in Dark Aqua.
    fn wash(self, alpha: f32) -> Color {
        match self {
            Appearance::Light => black_alpha(alpha),
            Appearance::Dark => white_alpha(alpha),
        }
    }

    /// Move `c` `amount` in this appearance's emphasis direction — darker
    /// in Aqua, lighter in Dark Aqua.
    fn emphasise(self, c: Color, amount: f32) -> Color {
        match self {
            Appearance::Light => c.darken(amount),
            Appearance::Dark => c.lighten(amount),
        }
    }
}

/// Nudge `fg` in the appearance's emphasis direction until it clears
/// `floor` against **every** surface in `on`.
///
/// AppKit tunes its foreground colours for `textBackgroundColor` — pure
/// white in Aqua — and several of them do not survive the move to the
/// grey `windowBackgroundColor` a Teksilo `Link`, `Badge` or validation
/// label can equally land on. `linkColor` measures 5.26:1 on white but
/// 4.45:1 on `#ECECEC`; the Accessible `systemGreen` measures 4.50:1
/// there, right on the line. Rather than transcribe values that fail
/// where this toolkit actually paints them, each such role is stepped in
/// 3 % increments until it clears the floor everywhere — one step for
/// most, none for the ones already comfortable.
///
/// Gives up after 16 steps (a hue that cannot get there is already at the
/// end of its range) so a pathological custom accent cannot hang the
/// build.
fn readable_on(fg: Color, on: &[Color], floor: f32, appearance: Appearance) -> Color {
    let clears = |c: Color| {
        on.iter()
            .all(|bg| over(c, *bg).contrast_ratio(*bg) >= floor)
    };
    let mut out = fg;
    for _ in 0..16 {
        if clears(out) {
            return out;
        }
        out = appearance.emphasise(out, 0.03);
    }
    out
}

/// Raise the **alpha** of a translucent label until it clears `floor` on
/// every backdrop in `on`.
///
/// The counterpart of [`readable_on`] for AppKit's label grades, which are
/// published as an alpha over the appearance's base rather than as a hue:
/// `labelColor` is "black at 85 %". Nudging the *colour* of pure black
/// does nothing, so the alpha is what has to move.
///
/// It has to move. Apple's `secondaryLabelColor` is 50 % in Aqua, which
/// measures 3.98:1 on `textBackgroundColor` — under WCAG SC 1.4.3's
/// 4.5:1 floor for body text — and its 55 % in Dark Aqua clears every
/// *plain* surface but not the lightest one carrying a press wash. Rather
/// than hand-pick a number per appearance and re-derive it whenever a
/// surface moves, the smallest passing alpha is computed here from the
/// surfaces the preset actually paints. The same lift IntUI applies to
/// Jewel's `text.info`, for the same reason.
///
/// Steps in 1 % increments, giving up after 30 (0.50 → 0.80, far past
/// anything a legible palette needs).
fn alpha_lifted(label: Color, on: &[Color], floor: f32) -> Color {
    let mut alpha = label.a();
    for _ in 0..30 {
        let candidate = label.with_alpha(alpha);
        if on
            .iter()
            .all(|bg| over(candidate, *bg).contrast_ratio(*bg) >= floor)
        {
            return candidate;
        }
        alpha = (alpha + 0.01).min(1.0);
    }
    label.with_alpha(alpha)
}

fn apply(c: &mut ColorTokens, p: &MacOsPalette, appearance: Appearance) {
    let light = appearance == Appearance::Light;
    let window = p.window_background;
    let content = p.control_background;
    // Every surface a free-standing foreground (a link, a validation
    // label, a badge glyph) can be dropped onto. `readable_on` holds each
    // one above the floor on all three.
    let text_surfaces = [window, content, p.panel_background];

    // ── Surfaces ───────────────────────────────────────────────────────
    // The AppKit depth ladder: the window frame is `windowBackgroundColor`,
    // content areas and fields sit on `controlBackgroundColor`, a floating
    // panel is a `.popover` / `.menu` material (opaque fallback), and a
    // document rests on `underPageBackgroundColor`.
    c.surface_main = window;
    c.surface_content = content;
    c.surface_raised = p.panel_background;
    c.surface_sunken = p.under_page_background;
    // Hover and press stay translucent: they are washes over whatever row
    // or control they land on, which is how AppKit's own toolbar-button
    // and list-row highlights behave.
    c.surface_hover = appearance.wash(0.05);
    c.surface_pressed = appearance.wash(0.10);
    // An *emphasised* macOS selection is a solid accent capsule with a
    // white label. This token cannot be that: it is also the fill of the
    // `TableView` / `TreeTableView` selection band and of `GridView`
    // tiles, whose cell content is app-supplied and resolves
    // `TextRole::Primary` — a saturated accent there would leave dark
    // text at 3.5:1. So the shared token is the accent as a *wash* the
    // primary label still clears 4.5:1 on, and the solid capsule is drawn
    // by `MacOsStandardItemStyle`, which owns its label too. Same split
    // Fluent makes for its selection pill.
    c.surface_selected = over(
        p.accent_ramp
            .base
            .with_alpha(if light { 0.30 } else { 0.45 }),
        content,
    );
    // `unemphasizedSelectedContentBackgroundColor`, transcribed — the
    // neutral capsule a row wears when its view has lost focus.
    c.surface_selected_inactive = p.unemphasized_selected_content_background;
    // [derived] `alternatingContentBackgroundColors[1]`. The 4 % wash
    // reproduces AppKit's published `#F5F5F5` on a white content surface
    // and follows the surface in Dark Aqua, where Apple publishes none.
    c.surface_alt_row = over(appearance.wash(0.04), content);
    c.surface_disabled = p.disabled_control_face;

    // [derived] AppKit names no ladder between the window and a popover.
    // A grouped settings box sits between the two; its raised sibling one
    // step further, its sunken sibling on the page background.
    //
    // The **top of the dark ladder is capped**, and not arbitrarily:
    // AppKit's `secondaryLabelColor` is white at 55 %, which clears WCAG
    // 1.4.3's 4.5:1 floor only on Dark Aqua surfaces up to roughly
    // `#3F3F3F`. Every surface Apple actually ships in Dark Aqua sits
    // below that; a *derived* one that drifted above it would force the
    // lift below to move Apple's own token further than it should. See
    // the test that pins the ceiling.
    c.surface_container = window.mix(p.panel_background, 0.5);
    c.surface_container_raised = if light {
        Color::from_hex("#FBFBFB")
    } else {
        Color::from_hex("#3C3C3C")
    };
    c.surface_container_sunken = p.under_page_background;

    // ── Text ───────────────────────────────────────────────────────────
    // Every opaque backdrop a row's own text can land on — each plain
    // surface, and each one carrying the hover or press wash.
    //
    // The washes matter and are easy to forget: a press wash *darkens* a
    // light surface and *lightens* a dark one, and in both directions that
    // moves the surface toward the label rather than away from it. A
    // pressed row's subtitle is where AppKit's published
    // `secondaryLabelColor` first stops clearing the floor.
    let text_backdrops: Vec<Color> = [
        window,
        content,
        p.panel_background,
        p.under_page_background,
        c.surface_container,
        c.surface_container_raised,
        c.surface_alt_row,
    ]
    .into_iter()
    .flat_map(|base| {
        [
            base,
            over(c.surface_hover, base),
            over(c.surface_pressed, base),
        ]
    })
    .collect();

    c.text_primary = alpha_lifted(p.label, &text_backdrops, 4.5);
    c.text_secondary = alpha_lifted(p.secondary_label, &text_backdrops, 4.5);
    c.text_disabled = p.disabled_control_text;
    // `alternateSelectedControlTextColor` — white in both appearances,
    // which is only safe because `accent` is the darkened *fill* shade
    // rather than the raw `controlAccentColor`.
    c.text_on_accent = p.selected_control_text;
    c.text_link = readable_on(p.link, &text_surfaces, 4.5, appearance);
    // [derived] AppKit publishes no hover variant of `linkColor` — macOS
    // signals a hovered link by underlining it. Teksilo's `Link` also
    // needs a colour to move, so the link steps one notch in the
    // appearance's emphasis direction; small enough to read as the same
    // link, large enough to confirm the pointer.
    c.text_link_hover = readable_on(
        appearance.emphasise(p.link, 0.18),
        &text_surfaces,
        4.5,
        appearance,
    );
    // [derived] AppKit has no visited-link colour at all. `systemPurple`
    // is the macOS hue closest to the one every browser uses.
    c.text_link_visited = readable_on(p.system_purple, &text_surfaces, 4.5, appearance);

    // The status hues, each held above the body-text floor on every
    // surface — see `readable_on`.
    let error = readable_on(p.system_red, &text_surfaces, 4.5, appearance);
    let warning = readable_on(p.system_yellow, &text_surfaces, 4.5, appearance);
    let success = readable_on(p.system_green, &text_surfaces, 4.5, appearance);
    let info = readable_on(p.system_blue, &text_surfaces, 4.5, appearance);

    c.text_error = error;
    c.text_warning = warning;
    c.text_success = success;

    // ── Accent ─────────────────────────────────────────────────────────
    // `accent` is `selectedContentBackgroundColor`, not
    // `controlAccentColor`: the raw accent carries white text at only
    // 4.02:1. See `MacOsAccentRamp`.
    c.accent = p.accent_ramp.fill;
    c.accent_hover = p.accent_ramp.hover;
    c.accent_pressed = p.accent_ramp.pressed;
    // A disabled default button on macOS is not a washed-out blue — it is
    // a plain grey bezel, the same one every other disabled control wears.
    c.accent_disabled = p.disabled_control_face;
    // [derived] AppKit's nearest token, `selectedContentBackgroundColor`,
    // is a fully-saturated fill and cannot serve a role whose whole job is
    // "subtle accent tint" (badge fills, info backgrounds).
    c.accent_subtle_bg = p.accent_subtle(if light { 0.14 } else { 0.24 });

    // ── Borders / dividers ─────────────────────────────────────────────
    c.border = p.separator;
    // **A deliberate deviation.** AppKit strokes an unchecked checkbox,
    // radio and switch with a hairline near `separatorColor` — 1.25:1 on
    // white, far under WCAG SC 1.4.11's 3:1 floor for a control boundary.
    // 45 % is the lightest wash that clears it on every surface this
    // preset paints. The same call Fluent makes with
    // `ControlStrongStrokeColorDefault`.
    c.border_strong = appearance.wash(0.45);
    // The signature macOS departure from Fluent: the focus indicator *is*
    // the accent, not a high-contrast neutral ring. `controlAccentColor`
    // rather than the darkened fill, because the ring sits on the window
    // surface and needs its identity, not its contrast-with-white.
    c.border_focused = p.accent_ramp.base;
    c.border_error = error;
    c.border_warning = warning;
    // [derived] one step above `separatorColor`, so a disabled field keeps
    // a legible outline without borrowing the accent.
    c.border_disabled = appearance.wash(0.18);
    c.divider = p.separator;
    // [derived] `gridColor` at double weight, for the emphasised rules
    // Teksilo draws between panes.
    c.divider_strong = appearance.wash(0.20);

    // ── Status ─────────────────────────────────────────────────────────
    // Foregrounds are the *Accessible* system-colour variant — see the
    // module doc. Backgrounds are [derived]: AppKit has no banner tokens,
    // so each is its own hue washed over the window at an alpha that keeps
    // the foreground above 3:1.
    let status_bg_alpha = if light { 0.14 } else { 0.22 };
    let status_bg = |fg: Color| over(fg.with_alpha(status_bg_alpha), window);
    c.status_info_fg = info;
    c.status_info_bg = status_bg(info);
    c.status_success_fg = success;
    c.status_success_bg = status_bg(success);
    c.status_warning_fg = warning;
    c.status_warning_bg = status_bg(warning);
    c.status_error_fg = error;
    c.status_error_bg = status_bg(error);

    // ── Cross-design-language container / on-error roles ───────────────
    c.text_on_error = error.best_contrast_text();
    c.text_on_error_container = error;
    c.surface_error_container = status_bg(error);
    // The container ladder is assigned with the other surfaces above,
    // because the text lift needs it.

    // ── Selection ──────────────────────────────────────────────────────
    // macOS's text selection is the *Highlight colour*, which defaults to
    // the accent and is composited at partial strength precisely so the
    // label underneath stays readable — for the stock blue on white this
    // reproduces the familiar `#B3DAFF`, and it follows a custom accent
    // exactly as macOS does.
    c.selection_bg_active = over(
        p.accent_ramp
            .base
            .with_alpha(if light { 0.30 } else { 0.45 }),
        p.text_background,
    );
    // AppKit does *not* recolour selected text: the wash above is light
    // enough that `labelColor` carries straight through it.
    c.selection_text_active = c.text_primary;
    c.selection_bg_inactive = p.unemphasized_selected_content_background;
    c.selection_text_inactive = c.text_primary;
    // `search_match_bg` / `search_match_current_bg` keep the IntUI amber —
    // see the module doc on `findHighlightColor`.

    // ── Scrollbar ──────────────────────────────────────────────────────
    // The macOS overlay scroller: no track at rest, a translucent thumb
    // that darkens (Aqua) or brightens (Dark Aqua) as it is grabbed.
    c.scrollbar_track = Color::TRANSPARENT;
    c.scrollbar_track_hover = appearance.wash(0.05);
    c.scrollbar_thumb = appearance.wash(if light { 0.32 } else { 0.40 });
    c.scrollbar_thumb_hover = appearance.wash(if light { 0.45 } else { 0.55 });
    c.scrollbar_thumb_pressed = appearance.wash(if light { 0.58 } else { 0.70 });

    // ── Tooltip ────────────────────────────────────────────────────────
    // Unlike IntUI (a dark chip in both appearances), a macOS help tag is
    // a small floating panel: the same surface, text and hairline as a
    // menu, so it tracks the appearance.
    c.tooltip_bg = p.panel_background;
    c.tooltip_text = c.text_primary;
    c.tooltip_border = p.separator;
    // The *lifted* token, not the raw palette field. A tooltip's
    // shortcut chip is body text on a panel surface like any other, and
    // reading `p.secondary_label` here left it 0.4 short of the floor
    // every other secondary label clears.
    c.tooltip_shortcut = c.text_secondary;

    // ── Editor ─────────────────────────────────────────────────────────
    let editor_bg = p.text_background;
    c.editor_bg = editor_bg;
    c.editor_fg = c.text_primary;
    // AppKit's insertion point is the label colour, not the accent — the
    // accent-coloured caret is an Xcode/third-party convention.
    c.editor_caret = c.text_primary;
    // [derived] AppKit has no current-line or code-block token.
    c.editor_current_line_bg = over(appearance.wash(0.04), editor_bg);
    // Line numbers are informative text, so `tertiaryLabelColor`'s 25 %
    // is lifted against the editor's own surfaces the same way every
    // other label grade is. macOS's own gutter is fainter than this; a
    // gutter one cannot read is not a gutter.
    c.editor_gutter_fg = alpha_lifted(
        p.tertiary_label,
        &[editor_bg, over(appearance.wash(0.04), editor_bg)],
        4.5,
    );
    c.editor_selection_bg = over(
        p.accent_ramp
            .base
            .with_alpha(if light { 0.30 } else { 0.45 }),
        editor_bg,
    );
    c.editor_code_block_bg = over(appearance.wash(0.05), editor_bg);
    // `labelColor` is 85 % black / white; code sits one notch further in
    // the same direction so monospaced runs read as their own register.
    c.editor_code_block_fg = if light { Color::BLACK } else { Color::WHITE };

    // ── Misc ───────────────────────────────────────────────────────────
    c.focus_ring = p.accent_ramp.base;
    c.focus_ring_error = error;
    c.scrim = p.sheet_scrim;

    // `chart_palette` keeps the colourblind-safe Okabe-Ito default —
    // Apple publishes no data-visualisation sequence.
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_core::presets::intui;
    use teksilo_tokens::{BorderRole, SurfaceRole};

    fn light() -> ColorTokens {
        macos_light_colors(&MacOsPalette::light())
    }

    fn dark() -> ColorTokens {
        macos_dark_colors(&MacOsPalette::dark())
    }

    /// Contrast of a possibly-translucent foreground against an opaque
    /// background. [`Color::contrast_ratio`] compares raw luminance and so
    /// ignores alpha — but nearly every AppKit foreground token is an
    /// alpha-over-surface wash (`labelColor` is 85 % black, not black), so
    /// the ratio is only meaningful after compositing.
    fn contrast_on(fg: Color, bg: Color) -> f32 {
        over(fg, bg).contrast_ratio(bg)
    }

    #[test]
    fn window_editor_and_accent_surfaces_are_opaque() {
        for c in [light(), dark()] {
            for (name, col) in [
                ("surface_main", c.surface_main),
                ("surface_content", c.surface_content),
                ("surface_raised", c.surface_raised),
                ("surface_sunken", c.surface_sunken),
                ("surface_selected", c.surface_selected),
                ("surface_alt_row", c.surface_alt_row),
                ("surface_container", c.surface_container),
                ("surface_container_raised", c.surface_container_raised),
                ("editor_bg", c.editor_bg),
                ("accent", c.accent),
                ("accent_hover", c.accent_hover),
                ("accent_pressed", c.accent_pressed),
                ("selection_bg_active", c.selection_bg_active),
                ("status_error_bg", c.status_error_bg),
            ] {
                assert!((col.a() - 1.0).abs() < 1e-6, "{name} must be opaque");
            }
        }
    }

    /// A preset that starts from `ColorTokens::*_default()` silently
    /// inherits every token it forgets to map. For the disabled family
    /// that is not cosmetic — a macOS **dark** app would paint a disabled
    /// field in IntUI's *light* grey. Mirrors the Fluent guard.
    #[test]
    fn disabled_family_is_macos_derived_not_the_intui_fallback() {
        for (name, m, base) in [
            ("light", light(), intui::light().colors),
            ("dark", dark(), intui::dark().colors),
        ] {
            assert_ne!(
                m.surface_disabled, base.surface_disabled,
                "{name}: surface_disabled still holds the IntUI fallback"
            );
            assert_ne!(
                m.border_disabled, base.border_disabled,
                "{name}: border_disabled still holds the IntUI fallback"
            );
            assert_ne!(
                m.text_disabled, base.text_disabled,
                "{name}: text_disabled still holds the IntUI fallback"
            );
            assert_ne!(
                m.accent_disabled, base.accent_disabled,
                "{name}: accent_disabled still holds the IntUI fallback"
            );
        }
    }

    #[test]
    fn field_roles_follow_the_macos_palette() {
        let c = dark();
        assert_eq!(SurfaceRole::Field.resolve(&c), c.surface_content);
        assert_eq!(SurfaceRole::Disabled.resolve(&c), c.surface_disabled);
        assert_eq!(BorderRole::Field.resolve(&c), c.border);
        assert_eq!(BorderRole::Disabled.resolve(&c), c.border_disabled);
    }

    /// The signature macOS/Fluent divergence, pinned so a later "let's
    /// unify the presets" edit has to argue with a test.
    #[test]
    fn the_focus_indicator_is_the_accent_itself() {
        for c in [light(), dark()] {
            assert_eq!(c.focus_ring, c.border_focused);
            // …the raw `controlAccentColor`, not the darkened fill.
            assert_ne!(c.focus_ring, c.accent);
        }
    }

    #[test]
    fn the_focus_ring_clears_the_non_text_contrast_floor() {
        // WCAG SC 1.4.11 — a focus indicator needs >= 3:1 against the
        // surface it is drawn on. This is why `MacOsChrome` paints the
        // ring's inner band at full alpha and reserves the translucency
        // for the halo outside it.
        for c in [light(), dark()] {
            for surface in [c.surface_main, c.surface_content, c.surface_raised] {
                assert!(
                    contrast_on(c.focus_ring, surface) >= 3.0,
                    "focus ring is {:.2}:1 on {surface:?}",
                    contrast_on(c.focus_ring, surface)
                );
            }
        }
    }

    #[test]
    fn body_text_clears_the_wcag_aa_floor() {
        for c in [light(), dark()] {
            for surface in [
                c.surface_main,
                c.surface_content,
                c.surface_raised,
                c.surface_sunken,
                c.surface_container,
                c.surface_container_raised,
                c.editor_bg,
            ] {
                assert!(
                    contrast_on(c.text_primary, surface) >= 4.5,
                    "primary text is {:.2}:1 on {surface:?}",
                    contrast_on(c.text_primary, surface)
                );
                assert!(
                    contrast_on(c.text_secondary, surface) >= 4.5,
                    "secondary text is {:.2}:1 on {surface:?}",
                    contrast_on(c.text_secondary, surface)
                );
            }
        }
    }

    #[test]
    fn control_outlines_clear_the_non_text_contrast_floor() {
        // `border_strong` is the outline an unchecked Checkbox / off
        // Toggle draws — a UI component boundary, so WCAG 1.4.11 applies
        // to it as much as to the focus ring. This is the deviation
        // documented in the module header; if it ever stopped being
        // needed, the test would still hold with AppKit's own value.
        for c in [light(), dark()] {
            for surface in [c.surface_main, c.surface_content] {
                assert!(
                    contrast_on(c.border_strong, surface) >= 3.0,
                    "strong border is {:.2}:1 on {surface:?}",
                    contrast_on(c.border_strong, surface)
                );
            }
        }
    }

    /// The label lift happens, is bounded, and only ever goes one way.
    ///
    /// A lift that silently became a no-op would be a regression nobody
    /// notices; one that ran away would produce a palette of near-white
    /// secondary text. Both ends are pinned.
    #[test]
    fn the_label_grades_are_lifted_above_apples_published_alphas() {
        for (name, c, p) in [
            ("light", light(), MacOsPalette::light()),
            ("dark", dark(), MacOsPalette::dark()),
        ] {
            assert!(
                c.text_secondary.a() > p.secondary_label.a(),
                "{name}: the lift did nothing — has AppKit's own value \
                 started passing? If so it can be dropped."
            );
            assert!(
                c.text_secondary.a() < 0.80,
                "{name}: the lift ran away to {}",
                c.text_secondary.a()
            );
            // …and the hue is untouched: only the alpha moves.
            assert_eq!(c.text_secondary.r(), p.secondary_label.r());
            // `labelColor` at 85 % already clears everything, so primary
            // must come through unlifted.
            assert_eq!(c.text_primary.a(), p.label.a());
        }
    }

    /// The lift is computed against the **washed** surfaces, not just the
    /// plain ones — a press wash moves a surface toward the label rather
    /// than away from it, and that is where AppKit's own value first
    /// stops clearing the floor.
    #[test]
    fn text_survives_the_hover_and_press_washes() {
        for c in [light(), dark()] {
            for base in [
                c.surface_main,
                c.surface_content,
                c.surface_raised,
                c.surface_sunken,
                c.surface_container,
                c.surface_container_raised,
                c.surface_alt_row,
            ] {
                for (state, backdrop) in [
                    ("plain", base),
                    ("hover", over(c.surface_hover, base)),
                    ("pressed", over(c.surface_pressed, base)),
                ] {
                    for (role, fg) in [("primary", c.text_primary), ("secondary", c.text_secondary)]
                    {
                        assert!(
                            contrast_on(fg, backdrop) >= 4.5,
                            "{role} text on a {state} {base:?} is {:.2}:1",
                            contrast_on(fg, backdrop)
                        );
                    }
                }
            }
        }
    }

    /// Every token that carries *text* has to read the lifted value, not
    /// the raw palette field. `tooltip_shortcut` was the one that did not.
    #[test]
    fn every_text_token_uses_the_lifted_grade() {
        for c in [light(), dark()] {
            assert_eq!(c.tooltip_text, c.text_primary);
            assert_eq!(c.tooltip_shortcut, c.text_secondary);
            assert_eq!(c.editor_fg, c.text_primary);
            assert_eq!(c.selection_text_active, c.text_primary);
            assert_eq!(c.selection_text_inactive, c.text_primary);
        }
    }

    /// Line numbers are text, so the gutter is lifted too — macOS's own
    /// gutter is fainter, and a gutter one cannot read is not a gutter.
    #[test]
    fn the_editor_gutter_is_readable() {
        for c in [light(), dark()] {
            assert!(
                contrast_on(c.editor_gutter_fg, c.editor_bg) >= 4.5,
                "gutter is {:.2}:1",
                contrast_on(c.editor_gutter_fg, c.editor_bg)
            );
            assert!(contrast_on(c.editor_gutter_fg, c.editor_current_line_bg) >= 4.5);
            // …and still de-emphasised relative to the body text.
            assert!(c.editor_gutter_fg.a() < c.editor_fg.a());
        }
    }

    #[test]
    fn apples_own_hairline_would_not_have_cleared_it() {
        // Pins the premise of the `border_strong` lift.
        let c = light();
        assert!(contrast_on(c.border, c.surface_content) < 3.0);
    }

    #[test]
    fn on_accent_text_clears_the_wcag_aa_floor() {
        for c in [light(), dark()] {
            for fill in [c.accent, c.accent_hover, c.accent_pressed] {
                assert!(
                    contrast_on(c.text_on_accent, fill) >= 4.5,
                    "on-accent label is {:.2}:1 on {fill:?}",
                    contrast_on(c.text_on_accent, fill)
                );
            }
        }
    }

    #[test]
    fn selected_text_stays_readable_without_being_recoloured() {
        // The premise of `selection_text_active == labelColor`: the wash
        // has to be light enough that the label carries straight through.
        for c in [light(), dark()] {
            assert!(
                contrast_on(c.selection_text_active, c.selection_bg_active) >= 4.5,
                "selected text is {:.2}:1",
                contrast_on(c.selection_text_active, c.selection_bg_active)
            );
            assert!(contrast_on(c.selection_text_inactive, c.selection_bg_inactive) >= 4.5);
            assert!(contrast_on(c.editor_fg, c.editor_selection_bg) >= 4.5);
        }
    }

    /// The shared `surface_selected` token is a wash precisely so that
    /// app-supplied `TableView` / `GridView` cell text survives it.
    #[test]
    fn primary_text_survives_the_shared_selection_wash() {
        for c in [light(), dark()] {
            assert!(
                contrast_on(c.text_primary, c.surface_selected) >= 4.5,
                "primary text is {:.2}:1 on the selection wash",
                contrast_on(c.text_primary, c.surface_selected)
            );
            assert!(contrast_on(c.text_primary, c.surface_selected_inactive) >= 4.5);
        }
    }

    #[test]
    fn the_light_selection_wash_reproduces_the_familiar_macos_blue() {
        // `#B3D7FF`-ish: 30 % of the stock accent over white. Off by more
        // than a couple of 8-bit steps means the construction drifted.
        let bg = light().selection_bg_active;
        let expected = Color::from_hex("#B3DAFF");
        for (a, b) in [
            (bg.r(), expected.r()),
            (bg.g(), expected.g()),
            (bg.b(), expected.b()),
        ] {
            assert!((a - b).abs() < 0.02, "{bg:?} strays from {expected:?}");
        }
    }

    #[test]
    fn tooltips_are_panels_not_inverse_chips() {
        // IntUI paints a dark chip in both appearances; a macOS help tag
        // is a floating panel, so it tracks the appearance.
        let l = light();
        let d = dark();
        assert!(l.tooltip_bg.relative_luminance() > 0.5);
        assert!(d.tooltip_bg.relative_luminance() < 0.5);
        for c in [&l, &d] {
            assert!(contrast_on(c.tooltip_text, c.tooltip_bg) >= 4.5);
            assert!(contrast_on(c.tooltip_shortcut, c.tooltip_bg) >= 4.5);
        }
    }

    #[test]
    fn status_backgrounds_are_opaque_and_carry_their_foreground() {
        for c in [light(), dark()] {
            for (name, fg, bg) in [
                ("error", c.status_error_fg, c.status_error_bg),
                ("success", c.status_success_fg, c.status_success_bg),
                ("warning", c.status_warning_fg, c.status_warning_bg),
                ("info", c.status_info_fg, c.status_info_bg),
            ] {
                assert!((bg.a() - 1.0).abs() < 1e-6, "{name} bg must be opaque");
                assert!(
                    contrast_on(fg, bg) >= 3.0,
                    "{name}: {:.2}:1",
                    contrast_on(fg, bg)
                );
            }
            // …and each foreground has to read on the page too, since a
            // validation message is often set on plain chrome.
            for fg in [
                c.status_error_fg,
                c.status_success_fg,
                c.status_warning_fg,
                c.status_info_fg,
            ] {
                assert!(contrast_on(fg, c.surface_main) >= 4.5);
                assert!(contrast_on(fg, c.surface_content) >= 4.5);
            }
        }
    }

    #[test]
    fn the_default_system_colours_would_not_have_passed() {
        // Pins the premise of the Accessible-variant choice: `systemRed`
        // Default is the value a naive transcription would have used.
        let c = light();
        let default_red = Color::from_hex("#FF3B30");
        assert!(default_red.contrast_ratio(c.surface_content) < 4.5);
        assert!(c.status_error_fg.contrast_ratio(c.surface_content) >= 4.5);
    }

    #[test]
    fn links_are_distinguishable_and_readable() {
        for c in [light(), dark()] {
            for surface in [c.surface_main, c.surface_content] {
                assert!(contrast_on(c.text_link, surface) >= 4.5);
                assert!(contrast_on(c.text_link_hover, surface) >= 4.5);
                assert!(contrast_on(c.text_link_visited, surface) >= 4.5);
            }
            // A hover that does not move is a hover the user cannot see.
            assert_ne!(c.text_link, c.text_link_hover);
            assert_ne!(c.text_link, c.text_link_visited);
        }
    }

    #[test]
    fn the_surface_ladder_is_monotonic_in_both_appearances() {
        // A floating panel must read as *in front of* the window in both
        // appearances, and a page must recede behind it.
        // Both appearances raise by *lightening* — Dark Aqua included.
        // See the note on `MacOsPalette::panel_background`.
        for c in [light(), dark()] {
            let main = c.surface_main.relative_luminance();
            let raised = c.surface_raised.relative_luminance();
            let sunken = c.surface_sunken.relative_luminance();
            assert!(raised > main, "a raised surface must not recede");
            assert!(sunken < main, "a sunken surface must not advance");
            assert!(
                c.surface_container.relative_luminance()
                    < c.surface_container_raised.relative_luminance(),
                "the container ladder is inverted"
            );
        }
    }

    #[test]
    fn hover_press_and_selection_are_ordered() {
        // Press must read as more than hover, or the control has no
        // press feedback at all.
        for c in [light(), dark()] {
            assert!(c.surface_pressed.a() > c.surface_hover.a());
            assert!(c.scrollbar_thumb_hover.a() > c.scrollbar_thumb.a());
            assert!(c.scrollbar_thumb_pressed.a() > c.scrollbar_thumb_hover.a());
        }
    }

    #[test]
    fn the_scrollbar_track_is_invisible_at_rest() {
        // macOS overlay scrollers show no track until the pointer arrives.
        for c in [light(), dark()] {
            assert_eq!(c.scrollbar_track.a(), 0.0);
            assert!(c.scrollbar_track_hover.a() > 0.0);
        }
    }

    #[test]
    fn light_and_dark_have_distinct_surfaces_and_accents() {
        assert_ne!(light().surface_main, dark().surface_main);
        assert_ne!(light().accent, dark().accent);
        assert_ne!(light().text_primary, dark().text_primary);
    }

    #[test]
    fn the_search_highlight_keeps_the_baseline_amber() {
        // Documented deviation: transcribing `findHighlightColor` would
        // put white editor text on pure yellow in Dark Aqua.
        for (m, base) in [
            (light(), intui::light().colors),
            (dark(), intui::dark().colors),
        ] {
            assert_eq!(m.search_match_bg, base.search_match_bg);
            assert_eq!(m.search_match_current_bg, base.search_match_current_bg);
        }
        // …and the AppKit literal is still reachable for apps that own
        // their own foreground.
        assert_eq!(
            MacOsPalette::light().find_highlight,
            Color::from_hex("#FFFF00")
        );
    }

    #[test]
    fn a_custom_accent_moves_the_accent_family_and_nothing_else() {
        use crate::palette::{MacOsAccentRamp, SystemAccent};
        let ramp = MacOsAccentRamp::light_from_base(SystemAccent::Green.light());
        let t = macos_light_colors(&MacOsPalette::light_with_accent(ramp));
        let base = light();

        assert_ne!(t.accent, base.accent);
        assert_ne!(t.focus_ring, base.focus_ring);
        assert_ne!(t.selection_bg_active, base.selection_bg_active);
        assert_ne!(t.accent_subtle_bg, base.accent_subtle_bg);

        // Neutrals are untouched…
        assert_eq!(t.surface_main, base.surface_main);
        assert_eq!(t.text_primary, base.text_primary);
        assert_eq!(t.border_strong, base.border_strong);
        // …and so is `linkColor`, which AppKit does not tie to the accent.
        assert_eq!(t.text_link, base.text_link);
    }

    #[test]
    fn a_custom_accent_stays_accessible() {
        use crate::palette::{MacOsAccentRamp, SystemAccent};
        for accent in SystemAccent::ALL {
            let l = macos_light_colors(&MacOsPalette::light_with_accent(
                MacOsAccentRamp::light_from_base(accent.light()),
            ));
            let d = macos_dark_colors(&MacOsPalette::dark_with_accent(
                MacOsAccentRamp::dark_from_base(accent.dark()),
            ));
            for c in [l, d] {
                assert!(
                    contrast_on(c.text_on_accent, c.accent) >= 4.5,
                    "{accent:?}: on-accent label is {:.2}:1",
                    contrast_on(c.text_on_accent, c.accent)
                );
                assert!(
                    contrast_on(c.text_primary, c.surface_selected) >= 4.5,
                    "{accent:?}: primary text on the selection wash is {:.2}:1",
                    contrast_on(c.text_primary, c.surface_selected)
                );
            }
        }
    }
}
