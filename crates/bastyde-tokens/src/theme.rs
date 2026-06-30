// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::os_theme_colors::OsThemeColors;

/// Semantic color tokens for a theme, structured around the JetBrains Int UI design system.
///
/// Surfaces are differentiated by **role**, not by elevation. Borders are uniformly 1 dp;
/// emphasis comes from color, not thickness.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColorTokens {
    // ── Surfaces ────────────────────────────────────────────────────────────
    pub surface_main: Color,
    pub surface_content: Color,
    pub surface_raised: Color,
    pub surface_sunken: Color,
    pub surface_hover: Color,
    pub surface_pressed: Color,
    pub surface_selected: Color,
    pub surface_selected_inactive: Color,
    /// Alternating-row background — a subtle hairline tint above
    /// `surface_content` used for table / list zebra striping. Distinct from
    /// `surface_sunken`, which is used for scroll-container chrome and is
    /// noticeably darker than the row contrast TableView wants.
    pub surface_alt_row: Color,

    // ── Text / foreground ───────────────────────────────────────────────────
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_disabled: Color,
    pub text_on_accent: Color,
    pub text_link: Color,
    pub text_link_hover: Color,
    pub text_link_visited: Color,
    pub text_error: Color,
    pub text_warning: Color,
    pub text_success: Color,

    // ── Accent ──────────────────────────────────────────────────────────────
    pub accent: Color,
    pub accent_hover: Color,
    pub accent_pressed: Color,
    pub accent_disabled: Color,
    pub accent_subtle_bg: Color,

    // ── Borders / dividers ──────────────────────────────────────────────────
    pub border: Color,
    pub border_strong: Color,
    pub border_focused: Color,
    pub border_error: Color,
    pub border_warning: Color,
    pub divider: Color,
    pub divider_strong: Color,

    // ── Status (banners, badges, validation) ────────────────────────────────
    pub status_info_fg: Color,
    pub status_info_bg: Color,
    pub status_success_fg: Color,
    pub status_success_bg: Color,
    pub status_warning_fg: Color,
    pub status_warning_bg: Color,
    pub status_error_fg: Color,
    pub status_error_bg: Color,

    // ── Selection (text, lists) ─────────────────────────────────────────────
    pub selection_bg_active: Color,
    pub selection_text_active: Color,
    pub selection_bg_inactive: Color,
    pub selection_text_inactive: Color,
    pub search_match_bg: Color,
    pub search_match_current_bg: Color,

    // ── Scrollbar (overlay) ─────────────────────────────────────────────────
    pub scrollbar_track: Color,
    pub scrollbar_track_hover: Color,
    pub scrollbar_thumb: Color,
    pub scrollbar_thumb_hover: Color,
    pub scrollbar_thumb_pressed: Color,

    // ── Tooltip (dark in BOTH themes — intentional Int UI house style) ──────
    pub tooltip_bg: Color,
    pub tooltip_text: Color,
    pub tooltip_border: Color,
    pub tooltip_shortcut: Color,

    // ── Editor (text editor pane — architecturally separate from UI chrome)
    //
    // IntelliJ treats the editor as its own color scheme, applied on top of
    // the UI theme. The values here differ from the general UI tokens in
    // subtle but important ways: `editor_fg` is dimmer than `text_primary`
    // to reduce eye strain on long reading sessions, `editor_selection_bg`
    // is more saturated than `surface_selected` (list-row selection), and
    // `editor_current_line_bg` is its own subtle highlight with no UI
    // equivalent. A `TextEditor` widget should consume these, not the
    // general `surface_content` / `text_primary` pair.
    pub editor_bg: Color,
    pub editor_fg: Color,
    pub editor_caret: Color,
    pub editor_current_line_bg: Color,
    pub editor_gutter_fg: Color,
    pub editor_selection_bg: Color,
    /// Background painted behind fenced code blocks inside a rich-text
    /// editor. Sits a hair above `editor_bg` in dark mode and a hair
    /// below in light mode, so monospaced runs stay readable without
    /// stealing focus from prose. Consumed by `RichTextEditor` via
    /// `RichTextEngine::set_code_block_background`.
    pub editor_code_block_bg: Color,
    /// Foreground used for monospaced runs inside fenced code blocks
    /// and `inline code` spans. Sits slightly off `editor_fg` so code
    /// reads as its own register against prose — a touch darker in
    /// light mode, a touch lighter in dark mode. Consumed by
    /// `RichTextEditor` via `RichTextEngine::set_code_block_foreground`.
    pub editor_code_block_fg: Color,

    // ── Misc ────────────────────────────────────────────────────────────────
    pub focus_ring: Color,
    pub focus_ring_error: Color,
    pub scrim: Color,

    // ── Cross-design-language roles ───────────────────────────────────────────
    // Universal slots every design language (Material 3, Fluent, macOS,
    // GTK4/Adwaita) needs but IntUI's flat role model didn't carry. Design-
    // language-specific colors (M3 secondary/tertiary triads, the full tonal
    // ladder, inverse colors) stay in per-theme extensions, not here.
    /// Text/icon painted on top of an error surface (white on the error fill).
    pub text_on_error: Color,
    /// Text painted on top of the (pale) error-container surface.
    pub text_on_error_container: Color,
    /// A softened error-tinted container surface (error banners, invalid
    /// field backgrounds). Pairs with `text_on_error_container`.
    pub surface_error_container: Color,
    /// Neutral container surface — a subtly distinguished panel/card level
    /// between `surface_main` and `surface_raised`.
    pub surface_container: Color,
    /// Raised container surface — elevated card/panel (one step above
    /// `surface_container`).
    pub surface_container_raised: Color,
    /// Sunken container surface — recessed below `surface_container`
    /// (headerbars, inset wells).
    pub surface_container_sunken: Color,

    // ── Charts ──────────────────────────────────────────────────────────────
    /// Series-color palette used by `bastyde-charts` BarChart / LineChart /
    /// PieChart when an individual series carries no explicit color.
    /// Defaults to the colorblind-safe Okabe-Ito sequence (see
    /// Okabe & Ito 2008) — used by ggplot2, seaborn, and others.
    /// Themes may override with brand colors. Charts wrap when the
    /// series count exceeds the palette length.
    pub chart_palette: Vec<Color>,
}

impl ColorTokens {
    /// Fraction by which the accent family is desaturated toward graphite for
    /// an inactive window. `0.0` keeps the accent, `1.0` is fully gray. See
    /// [`for_inactive_window`](Self::for_inactive_window).
    pub const INACTIVE_ACCENT_DESATURATION: f32 = 0.70;

    /// Project this palette for an **inactive window**: the accent-control
    /// family and focus indicators are desaturated toward graphite, matching
    /// the macOS / Qt `QPalette::Inactive` convention where accent-coloured
    /// controls grey out when the window loses focus — the default button,
    /// `Toggle` track, checked `Checkbox` / `RadioButton`, the selected
    /// `TabBar` / `SegmentedControl` item, `Slider` fill, `ProgressBar`, and
    /// focus rings. Because every themed control resolves its accent from the
    /// live `ColorTokens` at paint time, swapping in this projection
    /// (done by the paint walker when the window is inactive) desaturates them
    /// all with no per-widget code — and it applies to *any* preset that
    /// populates these fields (IntUI, Material 3, …), not just one.
    ///
    /// Deliberately untouched: `selection_*` / `surface_selected*` /
    /// `editor_selection_bg` (selection desaturation is *view-focus*-dependent
    /// and handled per widget via `SurfaceRole::SelectedInactive`); `status_*`
    /// (status colours keep their meaning); `text_*` / links.
    pub fn for_inactive_window(&self) -> ColorTokens {
        let d = Self::INACTIVE_ACCENT_DESATURATION;
        ColorTokens {
            accent: self.accent.desaturated(d),
            accent_hover: self.accent_hover.desaturated(d),
            accent_pressed: self.accent_pressed.desaturated(d),
            accent_disabled: self.accent_disabled.desaturated(d),
            accent_subtle_bg: self.accent_subtle_bg.desaturated(d),
            border_focused: self.border_focused.desaturated(d),
            focus_ring: self.focus_ring.desaturated(d),
            ..self.clone()
        }
    }

    pub fn light_default() -> Self {
        Self {
            // Surfaces
            surface_main: Color::from_hex("#F7F8FA"),
            surface_content: Color::from_hex("#FFFFFF"),
            surface_raised: Color::from_hex("#FFFFFF"),
            surface_sunken: Color::from_hex("#EBECF0"),
            surface_hover: Color::from_hex("#EBECF0"),
            surface_pressed: Color::from_hex("#DFE1E5"),
            surface_selected: Color::from_hex("#D4F0F5"),
            // A muted teal (the selection hue, dimmed) — distinct from both the
            // active `surface_selected` and the neutral-gray `surface_hover`
            // (which it previously duplicated).
            surface_selected_inactive: Color::from_hex("#DCE9EC"),
            surface_alt_row: Color::from_hex("#F7F8FA"),

            // Text — cross-checked against Jewel IntUiLightTheme.kt:
            //   text.normal   = gray( 1) = #000000
            //   text.info     = gray( 7) = #818594   ← "secondary"
            //   text.disabled = gray( 8) = #A8ADBD
            // The v2 reference doc listed #6C707E (gray(6)) for
            // text_secondary, which is slightly off — Jewel uses gray(7).
            text_primary: Color::from_hex("#000000"),
            text_secondary: Color::from_hex("#818594"),
            text_disabled: Color::from_hex("#A8ADBD"),
            text_on_accent: Color::from_hex("#FFFFFF"),
            text_link: Color::from_hex("#0FB5CC"),
            text_link_hover: Color::from_hex("#0E9BB0"),
            text_link_visited: Color::from_hex("#7C4DFF"),
            text_error: Color::from_hex("#DB3B4B"),
            text_warning: Color::from_hex("#A07527"),
            text_success: Color::from_hex("#369650"),

            // Accent — FernTech teal, pulled from the middle of the brand
            // gradient (#0088DD → #00B8F0 → #10C9F4 → #00E5CC) and desaturated
            // ~10% from the pure logo cyan for use at 13 sp on dense UI.
            // Distinct from text_success (#369650) by ~50° in hue.
            accent: Color::from_hex("#0FB5CC"),
            accent_hover: Color::from_hex("#0E9BB0"),
            accent_pressed: Color::from_hex("#0C8294"),
            accent_disabled: Color::from_hex("#A8DDE5"),
            accent_subtle_bg: Color::from_hex("#E6F7FA"),

            // Borders
            border: Color::from_hex("#EBECF0"),
            border_strong: Color::from_hex("#A8ADBD"),
            border_focused: Color::from_hex("#0FB5CC"),
            border_error: Color::from_hex("#DB3B4B"),
            border_warning: Color::from_hex("#E8A33D"),
            divider: Color::from_hex("#EBECF0"),
            divider_strong: Color::from_hex("#DFE1E5"),

            // Status
            status_info_fg: Color::from_hex("#0FB5CC"),
            status_info_bg: Color::from_hex("#E6F7FA"),
            status_success_fg: Color::from_hex("#369650"),
            status_success_bg: Color::from_hex("#E6F5E8"),
            status_warning_fg: Color::from_hex("#E8A33D"),
            status_warning_bg: Color::from_hex("#FFF6DA"),
            status_error_fg: Color::from_hex("#DB3B4B"),
            status_error_bg: Color::from_hex("#FFE2E3"),

            // Selection
            selection_bg_active: Color::from_hex("#0F8FA3"),
            selection_text_active: Color::from_hex("#FFFFFF"),
            selection_bg_inactive: Color::from_hex("#D4D4D4"),
            selection_text_inactive: Color::from_hex("#000000"),
            search_match_bg: Color::from_hex("#FED277"),
            search_match_current_bg: Color::from_hex("#FFC15A"),

            // Scrollbar
            scrollbar_track: Color::from_rgba(0.0, 0.0, 0.0, 0.0),
            scrollbar_track_hover: Color::from_rgba(0.0, 0.0, 0.0, 0.047),
            scrollbar_thumb: Color::from_rgba(0.0, 0.0, 0.0, 0.239),
            scrollbar_thumb_hover: Color::from_rgba(0.0, 0.0, 0.0, 0.361),
            scrollbar_thumb_pressed: Color::from_rgba(0.0, 0.0, 0.0, 0.502),

            // Tooltip — DARK in light theme (intentional)
            tooltip_bg: Color::from_hex("#1E1F22"),
            tooltip_text: Color::from_hex("#DFE1E5"),
            tooltip_border: Color::from_hex("#393B40"),
            tooltip_shortcut: Color::from_hex("#9DA0A8"),

            // Editor — IntelliJ new UI default light editor color scheme.
            editor_bg: Color::from_hex("#FFFFFF"),
            editor_fg: Color::from_hex("#000000"),
            editor_caret: Color::from_hex("#000000"),
            editor_current_line_bg: Color::from_hex("#FFFEEB"),
            editor_gutter_fg: Color::from_hex("#C9CCD6"),
            editor_selection_bg: Color::from_hex("#A8E0E8"),
            // Light editor: light-grey card behind code blocks, dipped
            // from #FFFFFF by ~5% so monospaced runs read as a distinct
            // surface without darkening the page. Code foreground is a
            // touch darker than editor_fg so monospace prints as its
            // own register against prose.
            editor_code_block_bg: Color::from_hex("#F2F3F5"),
            editor_code_block_fg: Color::from_hex("#1F2024"),

            // Misc
            focus_ring: Color::from_hex("#0FB5CC"),
            focus_ring_error: Color::from_hex("#DB3B4B"),
            scrim: Color::from_rgba(0.0, 0.0, 0.0, 0.32),

            // Cross-design-language roles (neutral IntUI defaults, often
            // aliasing the nearest existing slot — IntUI has no real
            // elevation ladder).
            text_on_error: Color::from_hex("#FFFFFF"),
            text_on_error_container: Color::from_hex("#8B1C26"),
            surface_error_container: Color::from_hex("#FFE2E3"),
            surface_container: Color::from_hex("#F3F4F6"),
            surface_container_raised: Color::from_hex("#FFFFFF"),
            surface_container_sunken: Color::from_hex("#EBECF0"),

            // Chart palette — Okabe-Ito (light: 8th color is black).
            chart_palette: okabe_ito_palette(false),
        }
    }

    pub fn dark_default() -> Self {
        Self {
            // Surfaces
            surface_main: Color::from_hex("#2B2D30"),
            surface_content: Color::from_hex("#1E1F22"),
            surface_raised: Color::from_hex("#3C3F41"),
            surface_sunken: Color::from_hex("#1E1F22"),
            surface_hover: Color::from_hex("#393B40"),
            surface_pressed: Color::from_hex("#43454A"),
            surface_selected: Color::from_hex("#1A3D47"),
            // A muted teal (the selection hue, dimmed) — distinct from both the
            // active `surface_selected` and the neutral-gray `surface_hover`
            // (which it previously duplicated, making inactive selection
            // indistinguishable from hover).
            surface_selected_inactive: Color::from_hex("#2C4A54"),
            surface_alt_row: Color::from_hex("#26282E"),

            // Text — values cross-checked against the Jewel standalone
            // theme source (IntUiGlobalColors.kt + IntUiDarkTheme.kt).
            // Jewel dark:
            //   text.normal   = gray(12) = #DFE1E5
            //   text.info     = gray( 7) = #6F737A   ← this is "secondary"
            //   text.disabled = gray( 6) = #5A5D63
            // The Int UI v2 reference doc's `text_secondary` picked
            // gray(9) (#9DA0A8) by mistake, which makes caption labels
            // read way too bright. Use Jewel's actual gray(7).
            text_primary: Color::from_hex("#BDBFC5"),
            text_secondary: Color::from_hex("#6F737A"),
            text_disabled: Color::from_hex("#5A5D63"),
            text_on_accent: Color::from_hex("#FDFEFF"),
            text_link: Color::from_hex("#19BDD4"),
            text_link_hover: Color::from_hex("#3DD0E0"),
            text_link_visited: Color::from_hex("#B49DFF"),
            text_error: Color::from_hex("#E55765"),
            text_warning: Color::from_hex("#E8A33D"),
            text_success: Color::from_hex("#5FAD65"),

            // Accent — dark-mode FernTech teal. Brighter and more saturated
            // than the light variant so it carries against dark surfaces.
            // Distinct from text_success (#5FAD65) by ~50° in hue.
            accent: Color::from_hex("#19BDD4"),
            accent_hover: Color::from_hex("#1499AD"),
            accent_pressed: Color::from_hex("#107E8F"),
            accent_disabled: Color::from_hex("#2D5A63"),
            accent_subtle_bg: Color::from_hex("#1A3A42"),

            // Borders
            border: Color::from_hex("#393B40"),
            border_strong: Color::from_hex("#5A5D63"),
            border_focused: Color::from_hex("#19BDD4"),
            border_error: Color::from_hex("#E55765"),
            border_warning: Color::from_hex("#E8A33D"),
            divider: Color::from_hex("#393B40"),
            divider_strong: Color::from_hex("#43454A"),

            // Status
            status_info_fg: Color::from_hex("#19BDD4"),
            status_info_bg: Color::from_hex("#1A3A42"),
            status_success_fg: Color::from_hex("#5FAD65"),
            status_success_bg: Color::from_hex("#1F4D2B"),
            status_warning_fg: Color::from_hex("#E8A33D"),
            status_warning_bg: Color::from_hex("#5A4318"),
            status_error_fg: Color::from_hex("#E55765"),
            status_error_bg: Color::from_hex("#7E353C"),

            // Selection
            selection_bg_active: Color::from_hex("#1F4D57"),
            selection_text_active: Color::from_hex("#DFE1E5"),
            selection_bg_inactive: Color::from_hex("#43454A"),
            selection_text_inactive: Color::from_hex("#DFE1E5"),
            search_match_bg: Color::from_hex("#876D29"),
            search_match_current_bg: Color::from_hex("#A07527"),

            // Scrollbar
            scrollbar_track: Color::from_rgba(0.0, 0.0, 0.0, 0.0),
            scrollbar_track_hover: Color::from_rgba(1.0, 1.0, 1.0, 0.047),
            scrollbar_thumb: Color::from_rgba(1.0, 1.0, 1.0, 0.239),
            scrollbar_thumb_hover: Color::from_rgba(1.0, 1.0, 1.0, 0.361),
            scrollbar_thumb_pressed: Color::from_rgba(1.0, 1.0, 1.0, 0.502),

            // Tooltip — DARK in dark theme too
            tooltip_bg: Color::from_hex("#1E1F22"),
            tooltip_text: Color::from_hex("#DFE1E5"),
            tooltip_border: Color::from_hex("#393B40"),
            tooltip_shortcut: Color::from_hex("#9DA0A8"),

            // Editor — IntelliJ new UI default dark editor color scheme.
            // Key differences from UI tokens:
            //   * editor_fg #BCBEC4 is noticeably dimmer than text_primary
            //     #DFE1E5. Long reading sessions want a lower-contrast
            //     foreground to reduce eye strain.
            //   * editor_selection_bg #1A4D5C is more saturated than
            //     surface_selected #1A3D47 (the list-row selection) and
            //     doesn't replace the glyph color — selected text keeps
            //     its editor_fg and shows the blue through.
            //   * editor_current_line_bg #26282E is a dedicated row
            //     highlight for the caret line; no UI equivalent.
            editor_bg: Color::from_hex("#1E1F22"),
            editor_fg: Color::from_hex("#BCBEC4"),
            editor_caret: Color::from_hex("#CED0D6"),
            editor_current_line_bg: Color::from_hex("#26282E"),
            editor_gutter_fg: Color::from_hex("#4E5157"),
            editor_selection_bg: Color::from_hex("#1A4D5C"),
            // Dark editor: code blocks lift off editor_bg #1E1F22 by a
            // hair (#2B2D30), code foreground brightens above editor_fg
            // so monospace stays legible inside the lifted card.
            editor_code_block_bg: Color::from_hex("#2B2D30"),
            editor_code_block_fg: Color::from_hex("#DFE1E5"),

            // Misc
            focus_ring: Color::from_hex("#19BDD4"),
            focus_ring_error: Color::from_hex("#E55765"),
            scrim: Color::from_rgba(0.0, 0.0, 0.0, 0.64),

            // Cross-design-language roles (neutral IntUI dark defaults).
            text_on_error: Color::from_hex("#FFFFFF"),
            text_on_error_container: Color::from_hex("#FFB3B9"),
            surface_error_container: Color::from_hex("#7E353C"),
            surface_container: Color::from_hex("#26282E"),
            surface_container_raised: Color::from_hex("#3C3F41"),
            surface_container_sunken: Color::from_hex("#1E1F22"),

            // Chart palette — Okabe-Ito (dark: 8th color is white).
            chart_palette: okabe_ito_palette(true),
        }
    }

    /// Build a full color token set from OS-reported colors.
    ///
    /// Starts from the matching built-in theme (light or dark) based on
    /// `color_scheme`, then overlays any color fields actually reported by
    /// the OS. Missing fields keep their built-in default values.
    pub fn from_os_colors(os: &OsThemeColors) -> Self {
        let is_dark = os.color_scheme.is_dark();
        let mut tokens = if is_dark {
            Self::dark_default()
        } else {
            Self::light_default()
        };

        // Accent → accent family + focus + status_info + link
        if let Some(accent) = os.accent {
            tokens.accent = accent;
            tokens.accent_hover = if is_dark {
                accent.lighten(0.10)
            } else {
                accent.darken(0.10)
            };
            tokens.accent_pressed = if is_dark {
                accent.lighten(0.20)
            } else {
                accent.darken(0.20)
            };
            tokens.accent_disabled = accent.with_alpha(0.4);
            tokens.text_on_accent = if accent.relative_luminance() > 0.4 {
                Color::from_hex("#000000")
            } else {
                Color::WHITE
            };
            tokens.focus_ring = accent;
            tokens.border_focused = accent;
            tokens.status_info_fg = accent;
            tokens.text_link = accent;
        }

        // Window background → surface family
        if let Some(bg) = os.window_bg {
            tokens.surface_main = bg;
            tokens.surface_content = if is_dark {
                bg.darken(0.04)
            } else {
                bg.lighten(0.04).mix(Color::WHITE, 0.5)
            };
            tokens.surface_sunken = if is_dark {
                bg.darken(0.08)
            } else {
                bg.darken(0.04)
            };
            tokens.surface_raised = if is_dark {
                bg.lighten(0.10)
            } else {
                Color::WHITE
            };
            tokens.surface_hover = if is_dark {
                bg.lighten(0.05)
            } else {
                bg.darken(0.04)
            };
            tokens.surface_pressed = if is_dark {
                bg.lighten(0.10)
            } else {
                bg.darken(0.08)
            };
            // Alt-row tracks the bg with a hairline-tinted lift, matching
            // the built-in light/dark presets. Slightly stronger in dark mode
            // because dark backgrounds need more contrast for the eye to
            // perceive striping.
            tokens.surface_alt_row = if is_dark {
                bg.lighten(0.04)
            } else {
                bg.darken(0.02)
            };
            // Editor background tracks surface_content (the editor slot).
            // Current-line highlight is a subtle lift off the editor bg.
            tokens.editor_bg = tokens.surface_content;
            tokens.editor_current_line_bg = if is_dark {
                tokens.editor_bg.lighten(0.06)
            } else {
                tokens.editor_bg.darken(0.03)
            };
            // Code block bg: a slightly stronger lift than the
            // current-line highlight so the card reads as a distinct
            // surface; same direction (lighten in dark, darken in
            // light) to stay coherent with the editor surface.
            tokens.editor_code_block_bg = if is_dark {
                tokens.editor_bg.lighten(0.08)
            } else {
                tokens.editor_bg.darken(0.05)
            };
        }

        // Window foreground → text family (+ editor foreground / caret / gutter)
        if let Some(fg) = os.window_fg {
            tokens.text_primary = fg;
            tokens.text_secondary = if is_dark {
                fg.darken(0.35)
            } else {
                fg.lighten(0.35)
            };
            tokens.text_disabled = if is_dark {
                fg.darken(0.6)
            } else {
                fg.lighten(0.6)
            };
            // Editor foreground is intentionally dimmer than label foreground
            // for reading-session ergonomics, matching IntelliJ's convention
            // that the editor scheme and the UI theme have distinct foregrounds.
            tokens.editor_fg = if is_dark { fg.darken(0.15) } else { fg };
            tokens.editor_caret = tokens.editor_fg;
            tokens.editor_gutter_fg = if is_dark {
                fg.darken(0.55)
            } else {
                fg.lighten(0.55)
            };
            // Code-block foreground brightens past editor_fg in dark
            // mode and darkens past it in light mode so monospace
            // reads as its own register against prose.
            tokens.editor_code_block_fg = if is_dark { fg } else { fg.darken(0.1) };
        }

        // Editor selection tracks the list/tree selection background when
        // the OS supplies one — it's the closest available signal.
        if let Some(sel_bg) = os.selection_bg {
            tokens.editor_selection_bg = sel_bg;
        }

        // Selection colors
        if let Some(sel_bg) = os.selection_bg {
            tokens.selection_bg_active = sel_bg;
            tokens.surface_selected = sel_bg;
            tokens.selection_text_active = os.selection_fg.unwrap_or(tokens.text_primary);
            // If no accent was provided, derive it from the selection color
            if os.accent.is_none() {
                tokens.accent = sel_bg;
                tokens.accent_hover = if is_dark {
                    sel_bg.lighten(0.10)
                } else {
                    sel_bg.darken(0.10)
                };
                tokens.accent_pressed = if is_dark {
                    sel_bg.lighten(0.20)
                } else {
                    sel_bg.darken(0.20)
                };
                tokens.text_on_accent = if sel_bg.relative_luminance() > 0.4 {
                    Color::from_hex("#000000")
                } else {
                    Color::WHITE
                };
                tokens.focus_ring = sel_bg;
                tokens.border_focused = sel_bg;
                tokens.status_info_fg = sel_bg;
                tokens.text_link = sel_bg;
            }
        }

        // Tooltip colors (rare; the Int UI default is dark in both themes)
        if let Some(tt_bg) = os.tooltip_bg {
            tokens.tooltip_bg = tt_bg;
        }
        if let Some(tt_fg) = os.tooltip_fg {
            tokens.tooltip_text = tt_fg;
        }

        // Derive border + divider from surface if we have OS surfaces
        if os.window_bg.is_some() || os.window_fg.is_some() {
            tokens.border = tokens.text_primary.with_alpha(0.12);
            tokens.border_strong = tokens.text_primary.with_alpha(0.25);
            tokens.divider = tokens.border;
            tokens.divider_strong = tokens.border_strong;
            tokens.scrim = Color::new(0.0, 0.0, 0.0, if is_dark { 0.64 } else { 0.32 });
        }

        tokens
    }
}

/// Okabe-Ito 8-color colorblind-safe palette (Okabe & Ito 2008).
///
/// The first 7 colors are identical in light and dark themes; the 8th
/// flips between black (light) and white (dark) so the rest-of-spectrum
/// fallback color contrasts with the surface.
fn okabe_ito_palette(dark: bool) -> Vec<Color> {
    vec![
        Color::from_hex("#E69F00"), // Orange
        Color::from_hex("#56B4E9"), // Sky blue
        Color::from_hex("#009E73"), // Bluish green
        Color::from_hex("#F0E442"), // Yellow
        Color::from_hex("#0072B2"), // Blue
        Color::from_hex("#D55E00"), // Vermilion
        Color::from_hex("#CC79A7"), // Reddish purple
        if dark {
            Color::from_hex("#FFFFFF")
        } else {
            Color::from_hex("#000000")
        },
    ]
}

// `Theme` aggregator lives in `bastyde-core` (not here) so it can co-locate
// with the per-widget style trait protocols and the typed `Rc<dyn …Style>`
// `ComponentStyleSlots` slot bag. bastyde-tokens stays a pure-data leaf crate.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_and_dark_have_different_surfaces() {
        let light = ColorTokens::light_default();
        let dark = ColorTokens::dark_default();
        assert_ne!(light.surface_main, dark.surface_main);
    }

    #[test]
    fn for_inactive_window_desaturates_accent_not_selection() {
        for c in [ColorTokens::light_default(), ColorTokens::dark_default()] {
            let inactive = c.for_inactive_window();

            // Accent family + focus indicators are desaturated toward graphite.
            assert_ne!(inactive.accent, c.accent);
            assert_eq!(
                inactive.accent,
                c.accent
                    .desaturated(ColorTokens::INACTIVE_ACCENT_DESATURATION)
            );
            assert_ne!(inactive.accent_hover, c.accent_hover);
            assert_ne!(inactive.accent_pressed, c.accent_pressed);
            assert_ne!(inactive.border_focused, c.border_focused);
            assert_ne!(inactive.focus_ring, c.focus_ring);

            // Selection / status / text are deliberately left untouched —
            // selection desaturation is view-focus-dependent (per widget) and
            // status colours must keep their meaning.
            assert_eq!(inactive.surface_selected, c.surface_selected);
            assert_eq!(
                inactive.surface_selected_inactive,
                c.surface_selected_inactive
            );
            assert_eq!(inactive.selection_bg_active, c.selection_bg_active);
            assert_eq!(inactive.selection_bg_inactive, c.selection_bg_inactive);
            assert_eq!(inactive.editor_selection_bg, c.editor_selection_bg);
            assert_eq!(inactive.status_error_bg, c.status_error_bg);
            assert_eq!(inactive.text_primary, c.text_primary);
        }
    }

    #[test]
    fn alt_row_is_distinct_from_content_in_both_themes() {
        // TableView / TreeTableView zebra striping needs a perceptible delta
        // between alternating rows and the body background. If a future
        // refactor accidentally aliases these, regression here.
        let light = ColorTokens::light_default();
        let dark = ColorTokens::dark_default();
        assert_ne!(light.surface_alt_row, light.surface_content);
        assert_ne!(dark.surface_alt_row, dark.surface_content);
    }

    #[test]
    fn alt_row_role_resolves_to_token() {
        use crate::roles::SurfaceRole;
        let colors = ColorTokens::light_default();
        assert_eq!(SurfaceRole::AltRow.resolve(&colors), colors.surface_alt_row);
    }

    #[test]
    fn text_on_accent_contrasts_with_accent() {
        let colors = ColorTokens::light_default();
        assert_ne!(colors.accent, colors.text_on_accent);
    }

    #[test]
    fn tooltip_is_dark_in_both_themes() {
        let light = ColorTokens::light_default();
        let dark = ColorTokens::dark_default();
        // Int UI house style: tooltip background is dark in both themes.
        assert_eq!(light.tooltip_bg, dark.tooltip_bg);
        assert!(light.tooltip_bg.relative_luminance() < 0.1);
    }

    #[test]
    fn from_os_colors_no_colors_returns_light_default() {
        let os = OsThemeColors::default();
        let tokens = ColorTokens::from_os_colors(&os);
        let light = ColorTokens::light_default();
        assert_eq!(tokens.surface_main, light.surface_main);
        assert_eq!(tokens.accent, light.accent);
    }

    #[test]
    fn from_os_colors_dark_scheme_returns_dark_base() {
        let os = OsThemeColors {
            color_scheme: crate::os_theme_colors::ColorSchemePreference::Dark,
            ..Default::default()
        };
        let tokens = ColorTokens::from_os_colors(&os);
        let dark = ColorTokens::dark_default();
        assert_eq!(tokens.surface_main, dark.surface_main);
    }

    #[test]
    fn from_os_colors_accent_overrides_accent_family() {
        let accent = Color::from_hex("#FF5500");
        let os = OsThemeColors {
            accent: Some(accent),
            ..Default::default()
        };
        let tokens = ColorTokens::from_os_colors(&os);
        assert_eq!(tokens.accent, accent);
        assert_ne!(tokens.accent, tokens.text_on_accent);
        assert_eq!(tokens.focus_ring, accent);
    }

    #[test]
    fn from_os_colors_window_bg_overrides_surfaces() {
        let bg = Color::from_hex("#AABBCC");
        let os = OsThemeColors {
            window_bg: Some(bg),
            ..Default::default()
        };
        let tokens = ColorTokens::from_os_colors(&os);
        assert_eq!(tokens.surface_main, bg);
        assert_ne!(tokens.surface_main, tokens.surface_sunken);
    }

    #[test]
    fn from_os_colors_selection_derives_accent_when_no_accent() {
        let sel = Color::from_hex("#3DAEE9");
        let os = OsThemeColors {
            selection_bg: Some(sel),
            ..Default::default()
        };
        let tokens = ColorTokens::from_os_colors(&os);
        assert_eq!(tokens.accent, sel);
    }

    #[test]
    fn light_and_dark_palette_share_first_seven_colors() {
        let light = ColorTokens::light_default();
        let dark = ColorTokens::dark_default();
        for i in 0..7 {
            assert_eq!(light.chart_palette[i], dark.chart_palette[i]);
        }
        // 8th differs: black for light, white for dark.
        assert_eq!(light.chart_palette[7], Color::from_hex("#000000"));
        assert_eq!(dark.chart_palette[7], Color::from_hex("#FFFFFF"));
    }

    #[test]
    fn chart_palette_has_eight_colors() {
        assert_eq!(ColorTokens::light_default().chart_palette.len(), 8);
        assert_eq!(ColorTokens::dark_default().chart_palette.len(), 8);
    }

    #[test]
    fn from_os_colors_accent_takes_precedence_over_selection() {
        let accent = Color::from_hex("#FF0000");
        let sel = Color::from_hex("#00FF00");
        let os = OsThemeColors {
            accent: Some(accent),
            selection_bg: Some(sel),
            ..Default::default()
        };
        let tokens = ColorTokens::from_os_colors(&os);
        assert_eq!(tokens.accent, accent);
    }
}
