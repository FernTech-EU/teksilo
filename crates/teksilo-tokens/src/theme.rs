// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::contrast::{composite_contrast, composite_over, lift_until_readable};
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
    /// Inert fill for a **disabled interactive control that is not
    /// accent-filled** — text fields, spin boxes, combo boxes. The neutral
    /// counterpart of `accent_disabled` (which washes out an accent-filled
    /// Button). Needed as its own token because a field frame and a passive
    /// `Panel` both paint `SurfaceRole::Content`, so the disabled-role
    /// substitution in `ColorProp::resolve` cannot tell them apart and
    /// deliberately leaves neutral surfaces alone — a disabled panel keeps
    /// its surface. Interactive controls opt in by painting
    /// [`crate::roles::SurfaceRole::Field`], which substitutes to
    /// [`crate::roles::SurfaceRole::Disabled`] in a disabled subtree.
    pub surface_disabled: Color,

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
    /// Outline for a disabled neutral control — the border twin of
    /// `surface_disabled`. Keeps a greyed-out field's shape legible without
    /// the accent tint `accent_disabled` would impose.
    pub border_disabled: Color,
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
    // KDE, GTK4/Adwaita) needs but IntUI's flat role model didn't carry. Design-
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
    /// Series-color palette used by `teksilo-charts` BarChart / LineChart /
    /// PieChart when an individual series carries no explicit color.
    /// Defaults to the colorblind-safe Okabe-Ito sequence (see
    /// Okabe & Ito 2008) — used by ggplot2, seaborn, and others.
    /// Themes may override with brand colors. Charts wrap when the
    /// series count exceeds the palette length.
    pub chart_palette: Vec<Color>,
}

/// WCAG SC 1.4.6's floor for text, which [`ColorTokens::for_high_contrast`]
/// holds every text pair it changes to.
const HC_TEXT: f32 = 7.0;

/// The floor [`ColorTokens::for_high_contrast`] holds the focus indicator and,
/// where its text allows, the accent to on the surfaces: SC 1.4.6's 4.5:1,
/// beyond SC 1.4.11's 3:1.
const HC_NON_TEXT: f32 = 4.5;

/// How far [`ColorTokens::for_high_contrast`] moves the selection fill off
/// `surface_content`: farther than a default wash (IntUI's stands 1.19:1 off
/// white), and no farther than leaves secondary text room to differ from the
/// primary. On a fill 1.5:1 off white a text reaches 7:1 at a luminance of
/// 0.05 and below; on one 3:1 off white only black does.
const HC_SELECTION: f32 = 1.5;

/// The steps [`ColorTokens::for_high_contrast`] takes where it walks a colour
/// itself, as [`lift_until_readable`] does: 1 % of the way per step.
const HC_STEPS: u32 = 100;

/// `fg` lifted until it reaches `floor` on every colour in `on`, as
/// [`lift_until_readable`] lifts it, gaining opacity in 1 % steps when no
/// lift at its own alpha can get there (a 58 % black secondary text is
/// already as dark as that alpha allows). Returns the most opaque attempt
/// when even an opaque lift falls short.
fn strengthen(fg: Color, on: &[Color], floor: f32) -> Color {
    let reaches = |c: Color| on.iter().all(|&bg| composite_contrast(c, bg) >= floor);
    let lifted = lift_until_readable(fg, on, floor);
    // A NaN alpha composites as opaque (see `composite_contrast`).
    let translucent = fg.a() < 1.0;
    if reaches(lifted) || !translucent {
        return lifted;
    }
    let mut attempt = lifted;
    for step in 1..=HC_STEPS {
        let alpha = fg.a() + (1.0 - fg.a()) * step as f32 / HC_STEPS as f32;
        attempt = lift_until_readable(fg.with_alpha(alpha), on, floor);
        if reaches(attempt) {
            break;
        }
    }
    attempt
}

impl ColorTokens {
    /// Fraction by which the accent family is desaturated toward graphite for
    /// an inactive window. `0.0` keeps the accent, `1.0` is fully gray. See
    /// [`for_inactive_window`](Self::for_inactive_window).
    pub const INACTIVE_ACCENT_DESATURATION: f32 = 0.70;

    /// Chart-palette-specific inactive-window desaturation. Lighter than
    /// `INACTIVE_ACCENT_DESATURATION` — the Okabe-Ito sequence's whole purpose
    /// is inter-series hue separation; fully desaturating it like a single accent
    /// control would defeat that even in a background window. See `for_inactive_window`.
    pub const INACTIVE_CHART_DESATURATION: f32 = 0.35;

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
        let cd = Self::INACTIVE_CHART_DESATURATION;
        ColorTokens {
            accent: self.accent.desaturated(d),
            accent_hover: self.accent_hover.desaturated(d),
            accent_pressed: self.accent_pressed.desaturated(d),
            accent_disabled: self.accent_disabled.desaturated(d),
            accent_subtle_bg: self.accent_subtle_bg.desaturated(d),
            border_focused: self.border_focused.desaturated(d),
            focus_ring: self.focus_ring.desaturated(d),
            chart_palette: self
                .chart_palette
                .iter()
                .map(|c| c.desaturated(cd))
                .collect(),
            ..self.clone()
        }
    }

    /// The opacity at which full-colour content, a picture, shows in a
    /// disabled subtree: the one that turns `text_primary` into
    /// `text_disabled` over `surface_content`, in linear light, so dimmed
    /// ink sits where disabled text sits. A palette that defines disabled
    /// text as an alpha of its primary ink gets that alpha ratio exactly,
    /// whatever its surface (Material 3: 0.38). Clamped to 0.25..=0.75: the
    /// floor is the lowest disabled-content opacity of the shipped design
    /// systems (macOS), the ceiling keeps a visible difference. 0.38 when
    /// the primary text and the surface have the same luminance.
    pub fn disabled_content_opacity(&self) -> f32 {
        // `relative_luminance` ignores alpha: composite over the surface.
        let surface = self.surface_content.relative_luminance();
        let over = |c: Color| c.a() * c.relative_luminance() + (1.0 - c.a()) * surface;
        let span = over(self.text_primary) - surface;
        if span.abs() < 1e-3 {
            return 0.38;
        }
        ((over(self.text_disabled) - surface) / span).clamp(0.25, 0.75)
    }

    /// Project this palette into a **high-contrast** variant for the OS
    /// "increase contrast" preference (WCAG SC 1.4.6 Enhanced tier / EN 301 549
    /// §11.7), applied at paint time when `prefers_high_contrast` is set
    /// (parallel to [`for_inactive_window`](Self::for_inactive_window)).
    ///
    /// The projection keeps the palette's own colours and moves each one only
    /// as far as a floor requires, so a preset keeps its accent's hue and its
    /// ring's character (an accent ring stays the accent, a neutral ring stays
    /// neutral). Every move is a [`lift_until_readable`] step toward black or
    /// white in sRGB, which keeps the hue, and contrast is measured in WCAG
    /// relative luminance with [`composite_contrast`], so a translucent token
    /// is measured as the colour it paints. "The surfaces" below are
    /// `surface_main`, `surface_content` and `surface_raised`. The pairs it
    /// guarantees, and the only tokens it changes:
    ///
    /// - **Text on the accent, 7:1.** `text_on_accent` is the palette's own
    ///   when it reaches 7:1 on `accent`, otherwise whichever of black and
    ///   white contrasts more with it. `accent`, `accent_hover` and
    ///   `accent_pressed` are each lifted until that text reaches 7:1 on them.
    ///   Each is lifted further, until it reaches 4.5:1 on the surfaces, when
    ///   the text still reaches 7:1 afterwards.
    /// - **The focus indicator, 4.5:1.** `focus_ring` and `border_focused` are
    ///   each lifted until they reach 4.5:1 on the surfaces. A translucent one
    ///   gains opacity when no lift at its own alpha can get there.
    /// - **The selection, 1.5:1, under text at 7:1.** `surface_selected` and
    ///   `surface_selected_inactive` (the fill a row keeps while its view is
    ///   unfocused or its window inactive) are each composited over
    ///   `surface_content` into an opaque fill and moved away from
    ///   `surface_content` until they stand 1.5:1 off it, stopping short of
    ///   that where `text_primary` would fall below 7:1 on them. A palette
    ///   whose selection already fails that text moves it toward
    ///   `surface_content` instead, until the text reaches 7:1.
    /// - **Secondary text, 7:1.** `text_secondary` is lifted, gaining opacity
    ///   as the focus indicator does, until it reaches 7:1 on the surfaces and
    ///   on both projected selections, so a selected row's subtitle stays
    ///   readable whether its view has focus or not.
    ///
    /// The accent and the focus indicator stay separate tokens because one
    /// colour cannot always serve both. Whether it can depends on the tone of
    /// the accent's text against the surfaces: where they are alike (white
    /// text in a light theme, black text in a dark one) the accent clears both
    /// floors; where they are opposite, the usual light-theme case of black
    /// text on a light accent, the accent must stay light for its text while
    /// a ring on near-white surfaces must be dark (black text at 7:1 needs a
    /// luminance of at least 0.30, a ring at 4.5:1 on white one of at most
    /// 0.18), so the accent is held to its text and the ring alone carries
    /// 4.5:1.
    ///
    /// Applied after [`for_inactive_window`](Self::for_inactive_window) by
    /// the generic paint chain, so an inactive window's desaturated accent is
    /// lifted to the same floors.
    ///
    /// [`lift_until_readable`]: crate::contrast::lift_until_readable
    /// [`composite_contrast`]: crate::contrast::composite_contrast
    pub fn for_high_contrast(&self) -> ColorTokens {
        let surfaces = [self.surface_main, self.surface_content, self.surface_raised];
        // The accent family is a fill the text is read on: measure it as the
        // opaque colour it paints over the content surface (unchanged when it
        // is already opaque).
        let opaque = |c: Color| composite_over(c, self.surface_content);
        let accent = opaque(self.accent);
        let text_on_accent = if composite_contrast(self.text_on_accent, accent) >= HC_TEXT {
            self.text_on_accent
        } else {
            accent.best_contrast_text()
        };
        let fill = |c: Color| {
            let readable = lift_until_readable(opaque(c), &[text_on_accent], HC_TEXT);
            let standing = lift_until_readable(readable, &surfaces, HC_NON_TEXT);
            if composite_contrast(text_on_accent, standing) >= HC_TEXT {
                standing
            } else {
                readable
            }
        };
        let surface_selected = self.high_contrast_selection(self.surface_selected);
        let surface_selected_inactive =
            self.high_contrast_selection(self.surface_selected_inactive);
        let text_surfaces = [
            self.surface_main,
            self.surface_content,
            self.surface_raised,
            surface_selected,
            surface_selected_inactive,
        ];
        ColorTokens {
            accent: fill(self.accent),
            accent_hover: fill(self.accent_hover),
            accent_pressed: fill(self.accent_pressed),
            border_focused: strengthen(self.border_focused, &surfaces, HC_NON_TEXT),
            focus_ring: strengthen(self.focus_ring, &surfaces, HC_NON_TEXT),
            text_on_accent,
            text_secondary: strengthen(self.text_secondary, &text_surfaces, HC_TEXT),
            surface_selected,
            surface_selected_inactive,
            ..self.clone()
        }
    }

    /// A selection fill of [`for_high_contrast`](Self::for_high_contrast)
    /// (`surface_selected` or `surface_selected_inactive`):
    /// `selected` made opaque over `surface_content` and moved toward
    /// black or white, whichever is farther from `surface_content`, until it
    /// stands [`HC_SELECTION`]:1 off it, never past the last step on which
    /// `text_primary` keeps [`HC_TEXT`]:1. A fill on which that text already
    /// falls short moves toward `surface_content` instead, until the text
    /// reaches [`HC_TEXT`]:1 on it.
    fn high_contrast_selection(&self, selected: Color) -> Color {
        let content = self.surface_content.with_alpha(1.0);
        let base = composite_over(selected, content);
        let reads = |c: Color| composite_contrast(self.text_primary, c) >= HC_TEXT;
        let step = |to: Color, i: u32| base.mix(to, i as f32 / HC_STEPS as f32);
        if !reads(base) {
            return (1..=HC_STEPS)
                .map(|i| step(content, i))
                .find(|&c| reads(c))
                .unwrap_or(content);
        }
        let toward = content.best_contrast_text();
        let mut chosen = base;
        for i in 1..=HC_STEPS {
            if chosen.contrast_ratio(content) >= HC_SELECTION {
                break;
            }
            let candidate = step(toward, i);
            if !reads(candidate) {
                break;
            }
            chosen = candidate;
        }
        chosen
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
            // gray(12) — a disabled field reads as inert grey against the
            // white `surface_content` an enabled one uses.
            surface_disabled: Color::from_hex("#EBECF0"),

            // Text — cross-checked against Jewel IntUiLightTheme.kt:
            //   text.normal   = gray( 1) = #000000
            //   text.info     = gray( 7) = #818594   ← "secondary"
            //   text.disabled = gray( 8) = #A8ADBD
            // The v2 reference doc listed #6C707E (gray(6)) for
            // text_secondary, which is slightly off — Jewel uses gray(7).
            // WCAG override: Jewel's gray(7) #818594 only reaches 3.67:1 on
            // white / 3.46:1 on surface_main, below SC 1.4.3's 4.5:1 floor, so
            // text_secondary is darkened to #5F636F (6.00:1 / 5.64:1).
            text_primary: Color::from_hex("#000000"),
            text_secondary: Color::from_hex("#5F636F"),
            text_disabled: Color::from_hex("#A8ADBD"),
            // WCAG: black on accent = 8.50:1; white (#FFFFFF) was 2.47:1 (fails 1.4.3).
            text_on_accent: Color::from_hex("#000000"),
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
            // WCAG SC 1.4.11: the accent #0FB5CC only reaches 2.47:1 on white,
            // below the 3:1 non-text floor; the focus indicator reuses the
            // darker accent_pressed #0C8294 (4.53:1 / 4.26:1).
            border_focused: Color::from_hex("#0C8294"),
            border_error: Color::from_hex("#DB3B4B"),
            border_warning: Color::from_hex("#E8A33D"),
            // gray(11) — one step darker than `surface_disabled` so the
            // outline still delineates the greyed-out field.
            border_disabled: Color::from_hex("#DFE1E5"),
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
            // WCAG SC 1.4.11: matches border_focused — #0C8294 (4.53:1 on
            // white) clears the 3:1 non-text floor that the accent #0FB5CC
            // (2.47:1) fails.
            focus_ring: Color::from_hex("#0C8294"),
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
            // Lifted off the near-black `surface_content` an enabled field
            // uses, so a disabled one reads as filled-and-inert rather than
            // as an empty hole.
            surface_disabled: Color::from_hex("#2B2D30"),

            // Text — values cross-checked against the Jewel standalone
            // theme source (IntUiGlobalColors.kt + IntUiDarkTheme.kt).
            // Jewel dark:
            //   text.normal   = gray(12) = #DFE1E5
            //   text.info     = gray( 7) = #6F737A   ← this is "secondary"
            //   text.disabled = gray( 6) = #5A5D63
            // The Int UI v2 reference doc's `text_secondary` picked
            // gray(9) (#9DA0A8) by mistake, which makes caption labels
            // read way too bright. Use Jewel's actual gray(7).
            // `text_primary` = gray(12) = #DFE1E5 (expUI/New-UI `*.foreground`).
            // The prior #BDBFC5 contradicted the comment above and read like
            // `editor_fg` (#BCBEC4) — noticeably dimmer than the spec.
            text_primary: Color::from_hex("#DFE1E5"),
            // WCAG SC 1.4.3: #6F737A only reached 3.46:1 / 2.90:1 on the dark
            // surfaces; lightened to #9198A0 (5.65:1 / 4.74:1).
            text_secondary: Color::from_hex("#9198A0"),
            text_disabled: Color::from_hex("#5A5D63"),
            // WCAG: black on the bright dark-mode accent = 9.27:1; #FDFEFF was 2.24:1.
            text_on_accent: Color::from_hex("#000000"),
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
            // A hair above `surface_disabled` — the same delineating role
            // gray(11) plays in the light preset.
            border_disabled: Color::from_hex("#43454A"),
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
    ///
    /// The content-view and semantic fields map as follows, each lifted to
    /// WCAG's floor on `surface_main` and `surface_content` when the OS
    /// colour falls short:
    ///
    /// | Field | Tokens |
    /// |---|---|
    /// | `view_bg` | `surface_content`, `editor_bg` (and the editor's current-line and code-block tints) |
    /// | `view_fg` | `editor_fg`, `editor_caret` |
    /// | `alternate_bg` | `surface_alt_row` |
    /// | `inactive_fg` | `text_secondary` (4.5:1) |
    /// | `link` | `text_link`, `text_link_hover` (4.5:1) |
    /// | `link_visited` | `text_link_visited` (4.5:1) |
    /// | `error`, `warning`, `success` | `text_error` / `_warning` / `_success` (4.5:1), and `status_*_fg` (3:1, also on its `status_*_bg`) |
    /// | `focus` | `focus_ring`, `border_focused` (3:1) |
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
            // Pick the black/white foreground with the higher WCAG contrast
            // against the live OS accent (exact ~0.179 crossover, not a cutoff).
            tokens.text_on_accent = accent.best_contrast_text();
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
                tokens.text_on_accent = sel_bg.best_contrast_text();
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

        tokens.apply_os_view_colors(os, is_dark);
        tokens
    }

    /// Overlay the content-view and semantic colours of an OS report, after
    /// the window, selection and accent families are in place (they win
    /// over the derivations above, which they refine).
    ///
    /// Each field is used only when present, so a report without them leaves
    /// the tokens exactly as before. A foreground the desktop supplies is
    /// checked against the surfaces it is painted on and lifted to WCAG's
    /// floor when it falls short (4.5:1 for text, 3:1 for the focus ring and
    /// the status glyphs): a desktop scheme was tuned for its own widgets,
    /// not for every Teksilo surface.
    fn apply_os_view_colors(&mut self, os: &OsThemeColors, is_dark: bool) {
        use crate::contrast::lift_until_readable;
        const TEXT: f32 = 4.5;
        const NON_TEXT: f32 = 3.0;

        if let Some(bg) = os.view_bg {
            self.surface_content = bg;
            self.editor_bg = bg;
            // The current-line and code-block tints follow the editor
            // surface, as they do from the window background.
            self.editor_current_line_bg = if is_dark {
                bg.lighten(0.06)
            } else {
                bg.darken(0.03)
            };
            self.editor_code_block_bg = if is_dark {
                bg.lighten(0.08)
            } else {
                bg.darken(0.05)
            };
        }
        if let Some(fg) = os.view_fg {
            self.editor_fg = fg;
            self.editor_caret = fg;
        }
        if let Some(alt) = os.alternate_bg {
            self.surface_alt_row = alt;
        }

        let surfaces = [self.surface_main, self.surface_content];
        if let Some(fg) = os.inactive_fg {
            self.text_secondary = lift_until_readable(fg, &surfaces, TEXT);
        }
        if let Some(link) = os.link {
            let link = lift_until_readable(link, &surfaces, TEXT);
            self.text_link = link;
            // Hover moves away from the surface, so it reads at least as
            // well as the link itself.
            self.text_link_hover = lift_until_readable(
                if is_dark {
                    link.lighten(0.10)
                } else {
                    link.darken(0.10)
                },
                &surfaces,
                TEXT,
            );
        }
        if let Some(visited) = os.link_visited {
            self.text_link_visited = lift_until_readable(visited, &surfaces, TEXT);
        }
        if let Some(error) = os.error {
            self.text_error = lift_until_readable(error, &surfaces, TEXT);
            self.status_error_fg = lift_until_readable(
                error,
                &[
                    self.status_error_bg,
                    self.surface_main,
                    self.surface_content,
                ],
                NON_TEXT,
            );
        }
        if let Some(warning) = os.warning {
            self.text_warning = lift_until_readable(warning, &surfaces, TEXT);
            self.status_warning_fg = lift_until_readable(
                warning,
                &[
                    self.status_warning_bg,
                    self.surface_main,
                    self.surface_content,
                ],
                NON_TEXT,
            );
        }
        if let Some(success) = os.success {
            self.text_success = lift_until_readable(success, &surfaces, TEXT);
            self.status_success_fg = lift_until_readable(
                success,
                &[
                    self.status_success_bg,
                    self.surface_main,
                    self.surface_content,
                ],
                NON_TEXT,
            );
        }
        if let Some(focus) = os.focus {
            let ring = lift_until_readable(focus, &surfaces, NON_TEXT);
            self.focus_ring = ring;
            self.border_focused = ring;
        }
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

// `Theme` aggregator lives in `teksilo-core` (not here) so it can co-locate
// with the per-widget style trait protocols and the typed `Rc<dyn …Style>`
// `ComponentStyleSlots` slot bag. teksilo-tokens stays a pure-data leaf crate.

#[cfg(test)]
mod tests {
    use super::*;

    /// `base` with its primary and disabled text and its content surface
    /// replaced.
    fn palette(primary: &str, disabled: &str, surface: &str) -> ColorTokens {
        ColorTokens {
            text_primary: Color::from_hex(primary),
            text_disabled: Color::from_hex(disabled),
            surface_content: Color::from_hex(surface),
            ..ColorTokens::light_default()
        }
    }

    #[test]
    fn disabled_ink_as_an_alpha_of_the_primary_gives_that_alpha() {
        // Material 3: `on_surface`, and the same at 0.38, on any surface.
        // 0x61 / 255 = 0.380.
        for (ink, surface) in [
            ("#1D1B20", "#FEF7FF"),
            ("#E6E0E9", "#141218"),
            ("#1D1B20", "#7F7F7F"),
        ] {
            let disabled = format!("{ink}61");
            let f = palette(ink, &disabled, surface).disabled_content_opacity();
            assert!((f - 0.380).abs() < 0.001, "{ink} on {surface}: {f}");
        }
        // Fluent: black ink at 0xE4 and 0x5C in light, white at 0xFF and
        // 0x5D in dark: the ratio of the alphas.
        let light = palette("#000000E4", "#0000005C", "#F3F3F3").disabled_content_opacity();
        assert!((light - 92.0 / 228.0).abs() < 0.001, "{light}");
        let dark = palette("#FFFFFFFF", "#FFFFFF5D", "#202020").disabled_content_opacity();
        assert!((dark - 93.0 / 255.0).abs() < 0.001, "{dark}");
    }

    #[test]
    fn the_intui_palettes_give_their_factors() {
        // Light: black on white, disabled #A8ADBD. Dark: #DFE1E5 on #1E1F22,
        // disabled #5A5D63, which the floor lifts from 0.129.
        let light = ColorTokens::light_default().disabled_content_opacity();
        assert!((light - 0.581).abs() < 0.001, "{light}");
        let dark = ColorTokens::dark_default().disabled_content_opacity();
        assert_eq!(dark, 0.25);
        let raw = {
            let t = ColorTokens::dark_default();
            let s = t.surface_content.relative_luminance();
            (t.text_disabled.relative_luminance() - s) / (t.text_primary.relative_luminance() - s)
        };
        assert!((raw - 0.129).abs() < 0.001, "{raw}");
        // The window projections keep both text tokens, so the factor.
        for base in [ColorTokens::light_default(), ColorTokens::dark_default()] {
            let f = base.disabled_content_opacity();
            assert_eq!(base.for_inactive_window().disabled_content_opacity(), f);
        }
    }

    #[test]
    fn text_as_light_as_its_surface_gives_the_material_factor() {
        let flat = palette("#808080", "#808080", "#808080");
        assert_eq!(flat.disabled_content_opacity(), 0.38);
        // And no factor escapes the clamp.
        let faint = palette("#000000", "#FDFDFD", "#FFFFFF").disabled_content_opacity();
        assert_eq!(faint, 0.25);
        let strong = palette("#000000", "#000000", "#FFFFFF").disabled_content_opacity();
        assert_eq!(strong, 0.75);
    }

    #[test]
    fn light_and_dark_have_different_surfaces() {
        let light = ColorTokens::light_default();
        let dark = ColorTokens::dark_default();
        assert_ne!(light.surface_main, dark.surface_main);
    }

    /// Every pair [`ColorTokens::for_high_contrast`] documents, below its
    /// floor in `hc`, named with its ratio.
    fn high_contrast_shortfalls(hc: &ColorTokens) -> Vec<String> {
        use crate::contrast::composite_contrast;
        let surfaces = [
            ("surface_main", hc.surface_main),
            ("surface_content", hc.surface_content),
            ("surface_raised", hc.surface_raised),
        ];
        let mut out = Vec::new();
        let mut check = |name: String, fg: Color, bg: Color, floor: f32| {
            let r = composite_contrast(fg, bg);
            if r < floor {
                out.push(format!("{name} = {r:.2}, floor {floor}"));
            }
        };
        for (fill, c) in [
            ("accent", hc.accent),
            ("accent_hover", hc.accent_hover),
            ("accent_pressed", hc.accent_pressed),
        ] {
            check(
                format!("text_on_accent on {fill}"),
                hc.text_on_accent,
                c,
                7.0,
            );
        }
        for (s, bg) in surfaces {
            check(format!("focus_ring on {s}"), hc.focus_ring, bg, 4.5);
            check(format!("border_focused on {s}"), hc.border_focused, bg, 4.5);
            check(format!("text_secondary on {s}"), hc.text_secondary, bg, 7.0);
        }
        for (fill, c) in [
            ("surface_selected", hc.surface_selected),
            ("surface_selected_inactive", hc.surface_selected_inactive),
        ] {
            check(
                format!("text_secondary on {fill}"),
                hc.text_secondary,
                c,
                7.0,
            );
            check(format!("text_primary on {fill}"), hc.text_primary, c, 7.0);
            check(
                format!("{fill} on surface_content"),
                c,
                hc.surface_content,
                1.5,
            );
        }
        out
    }

    #[test]
    fn high_contrast_variant_exceeds_aa_on_both_presets() {
        // Audit G13 (WCAG 1.4.6 Enhanced / EN 301 549 11.7): the opt-in
        // high-contrast projection clears >= 7:1 text and >= 4.5:1 non-text on
        // both the light and dark bases, for an active and an inactive window
        // (the paint chain projects the inactive tokens).
        for (name, base) in [
            ("light", ColorTokens::light_default()),
            ("dark", ColorTokens::dark_default()),
        ] {
            for (state, tokens) in [
                ("active", base.clone()),
                ("inactive", base.for_inactive_window()),
            ] {
                let short = high_contrast_shortfalls(&tokens.for_high_contrast());
                assert!(short.is_empty(), "{name} {state}: {short:#?}");
            }
        }
    }

    #[test]
    fn high_contrast_keeps_an_accent_that_already_meets_its_floors() {
        // IntUI's accents already carry their black text at 7:1 (8.50 light,
        // 9.27 dark), and the dark one stands 4.5:1 on the dark surfaces, so
        // neither moves. Before the projection read the palette, dark mode
        // swapped in #4FCCE0 whatever the accent was.
        for base in [ColorTokens::light_default(), ColorTokens::dark_default()] {
            let hc = base.for_high_contrast();
            assert_eq!(hc.accent, base.accent);
            assert_eq!(hc.text_on_accent, base.text_on_accent);
        }
        // A token the projection does not document is never touched.
        let base = ColorTokens::dark_default();
        let hc = base.for_high_contrast();
        assert_eq!(hc.text_primary, base.text_primary);
        assert_eq!(hc.text_link, base.text_link);
        assert_eq!(hc.accent_subtle_bg, base.accent_subtle_bg);
        assert_eq!(hc.surface_main, base.surface_main);
        assert_eq!(hc.chart_palette, base.chart_palette);
    }

    #[test]
    fn high_contrast_lifts_a_short_accent_and_keeps_its_hue() {
        // macOS's Aqua accent, #0063E1 under white text, measures 3.87:1 with
        // the black text the old projection forced on it.
        let base = ColorTokens {
            accent: Color::from_hex("#0063E1"),
            accent_hover: Color::from_hex("#005ED6"),
            accent_pressed: Color::from_hex("#0054BF"),
            text_on_accent: Color::WHITE,
            ..ColorTokens::light_default()
        };
        let hc = base.for_high_contrast();
        assert_eq!(hc.text_on_accent, Color::WHITE);
        for (before, after) in [
            (base.accent, hc.accent),
            (base.accent_hover, hc.accent_hover),
            (base.accent_pressed, hc.accent_pressed),
        ] {
            let ratio = Color::WHITE.contrast_ratio(after);
            assert!((7.0..7.3).contains(&ratio), "lifted to {ratio:.2}");
            let (h0, ..) = before.to_hsv();
            let (h1, ..) = after.to_hsv();
            assert!((h0 - h1).abs() < 0.5, "hue {h0} became {h1}");
        }
        // White text and light surfaces are alike in tone, so the accent
        // also stands 4.5:1 on them.
        for s in [hc.surface_main, hc.surface_content, hc.surface_raised] {
            assert!(hc.accent.contrast_ratio(s) >= 4.5);
        }
    }

    #[test]
    fn high_contrast_holds_a_light_accent_to_its_black_text() {
        // The documented divergence: black text on IntUI's light accent
        // needs it at a luminance of 0.30 or more, a ring at 4.5:1 on white
        // one of 0.18 or less, so the accent stays light and only the ring
        // darkens.
        let hc = ColorTokens::light_default().for_high_contrast();
        assert!(hc.accent.relative_luminance() >= 0.30);
        assert!(hc.focus_ring.relative_luminance() <= 0.183);
        assert!(hc.accent.contrast_ratio(Color::WHITE) < 4.5);
        assert!(hc.focus_ring.contrast_ratio(Color::WHITE) >= 4.5);
    }

    #[test]
    fn high_contrast_keeps_a_neutral_ring_neutral() {
        // Fluent's ring is 89 % black, never the accent: it already clears
        // 4.5:1 and is kept as it is, alpha included.
        let ring = Color::BLACK.with_alpha(0.89);
        let base = ColorTokens {
            focus_ring: ring,
            border_focused: ring,
            ..ColorTokens::light_default()
        };
        let hc = base.for_high_contrast();
        assert_eq!(hc.focus_ring, ring);
        assert_eq!(hc.border_focused, ring);
    }

    #[test]
    fn high_contrast_gives_translucent_secondary_text_more_opacity() {
        // macOS's secondary label is 58 % black, about 5.3:1 on white:
        // already black, so only its alpha can carry it to 7:1.
        let base = ColorTokens {
            text_secondary: Color::BLACK.with_alpha(0.58),
            ..ColorTokens::light_default()
        };
        let hc = base.for_high_contrast();
        assert!(hc.text_secondary.a() > 0.58 && hc.text_secondary.a() < 1.0);
        assert_eq!(
            [
                hc.text_secondary.r(),
                hc.text_secondary.g(),
                hc.text_secondary.b()
            ],
            [0.0, 0.0, 0.0]
        );
        assert!(high_contrast_shortfalls(&hc).is_empty());
    }

    #[test]
    fn high_contrast_selection_moves_off_the_surface_and_keeps_its_text() {
        use crate::contrast::composite_contrast;
        let base = ColorTokens::light_default();
        let hc = base.for_high_contrast();
        // IntUI's wash stands 1.19:1 off white; the projection moves it to
        // 1.5:1 and no farther than one step past.
        let before = base.surface_selected.contrast_ratio(base.surface_content);
        let after = hc.surface_selected.contrast_ratio(hc.surface_content);
        assert!(before < 1.2, "{before}");
        assert!((1.5..1.55).contains(&after), "{after}");
        // A translucent wash (Fluent's 9 % black) becomes an opaque fill.
        let wash = ColorTokens {
            surface_selected: Color::BLACK.with_alpha(0.09),
            ..ColorTokens::light_default()
        }
        .for_high_contrast();
        assert_eq!(wash.surface_selected.a(), 1.0);
        assert!(wash.surface_selected.contrast_ratio(wash.surface_content) >= 1.5);
        // A selection the primary text already falls short on moves toward
        // the surface until the text reads.
        let dark = ColorTokens::dark_default();
        let short = ColorTokens {
            surface_selected: Color::from_hex("#3A6A7A"),
            ..dark.clone()
        };
        assert!(composite_contrast(short.text_primary, short.surface_selected) < 7.0);
        let hc = short.for_high_contrast();
        let r = composite_contrast(hc.text_primary, hc.surface_selected);
        assert!((7.0..7.3).contains(&r), "{r}");
    }

    #[test]
    fn high_contrast_moves_the_inactive_selection_too() {
        // A row in an unfocused view, or in an inactive window, paints
        // `surface_selected_inactive`; it must stand off the page as the
        // focused selection does. IntUI's light wash stands 1.24:1 off
        // white, so it moves to 1.5:1 and no farther than one step past.
        let light = ColorTokens::light_default();
        let before = light
            .surface_selected_inactive
            .contrast_ratio(light.surface_content);
        assert!(before < 1.25, "{before}");
        for tokens in [light.clone(), light.for_inactive_window()] {
            let hc = tokens.for_high_contrast();
            let after = hc
                .surface_selected_inactive
                .contrast_ratio(hc.surface_content);
            assert!((1.5..1.55).contains(&after), "{after}");
            assert!(high_contrast_shortfalls(&hc).is_empty());
        }
        // The dark one already stands 1.74:1 off the dark content surface
        // under text at 7:1, so it is kept.
        let dark = ColorTokens::dark_default();
        for tokens in [dark.clone(), dark.for_inactive_window()] {
            let hc = tokens.for_high_contrast();
            assert_eq!(hc.surface_selected_inactive, dark.surface_selected_inactive);
            assert!(high_contrast_shortfalls(&hc).is_empty());
        }
        // A translucent inactive wash becomes an opaque fill, and the
        // secondary text is lifted on it.
        let wash = ColorTokens {
            surface_selected_inactive: Color::BLACK.with_alpha(0.06),
            text_secondary: Color::BLACK.with_alpha(0.58),
            ..ColorTokens::light_default()
        }
        .for_high_contrast();
        assert_eq!(wash.surface_selected_inactive.a(), 1.0);
        assert!(high_contrast_shortfalls(&wash).is_empty());
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
    fn for_inactive_window_desaturates_chart_palette_lightly() {
        for c in [ColorTokens::light_default(), ColorTokens::dark_default()] {
            let inactive = c.for_inactive_window();
            assert_eq!(inactive.chart_palette.len(), c.chart_palette.len());
            let n = c.chart_palette.len();
            for i in 0..n.saturating_sub(1) {
                assert_eq!(
                    inactive.chart_palette[i],
                    c.chart_palette[i].desaturated(ColorTokens::INACTIVE_CHART_DESATURATION)
                );
                // The Okabe-Ito palette's non-achromatic entries should actually change.
                assert_ne!(inactive.chart_palette[i], c.chart_palette[i]);
            }
            // The last Okabe-Ito entry is achromatic (black/white) — desaturation is a no-op on it.
            if n > 0 {
                assert_eq!(inactive.chart_palette[n - 1], c.chart_palette[n - 1]);
            }
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
    fn default_themes_meet_wcag_contrast_minimums() {
        // WCAG 2.1 SC 1.4.3 (text >= 4.5:1) and SC 1.4.11 (non-text UI
        // components / focus indicators >= 3:1), enforced on BOTH shipped
        // presets so a regression in the token hex values fails CI rather
        // than shipping a non-conformant default theme.
        for (name, c) in [
            ("light", ColorTokens::light_default()),
            ("dark", ColorTokens::dark_default()),
        ] {
            // Text on the primary / destructive button fill.
            let r = c.text_on_accent.contrast_ratio(c.accent);
            assert!(
                r >= 4.5,
                "{name}: text_on_accent vs accent = {r:.2}, need >= 4.5"
            );

            // Secondary text over both surface levels it renders on.
            for (sname, surface) in [
                ("surface_content", c.surface_content),
                ("surface_main", c.surface_main),
            ] {
                let r = c.text_secondary.contrast_ratio(surface);
                assert!(
                    r >= 4.5,
                    "{name}: text_secondary vs {sname} = {r:.2}, need >= 4.5"
                );
            }

            // Keyboard focus indicator (border + ring) over both surfaces.
            for (fname, fg) in [
                ("border_focused", c.border_focused),
                ("focus_ring", c.focus_ring),
            ] {
                for (sname, surface) in [
                    ("surface_content", c.surface_content),
                    ("surface_main", c.surface_main),
                ] {
                    let r = fg.contrast_ratio(surface);
                    assert!(r >= 3.0, "{name}: {fname} vs {sname} = {r:.2}, need >= 3.0");
                }
            }
        }
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

    use crate::contrast::composite_contrast;
    use crate::os_theme_colors::ColorSchemePreference as Scheme;

    fn os(color_scheme: Scheme) -> OsThemeColors {
        OsThemeColors::new(color_scheme)
    }

    /// The names of the tokens that differ between `a` and `b`, sorted, so
    /// a test pins both what a field changes and that it changes nothing
    /// else.
    fn changed_fields(a: &ColorTokens, b: &ColorTokens) -> Vec<String> {
        let a = serde_json::to_value(a).unwrap();
        let b = serde_json::to_value(b).unwrap();
        let (a, b) = (a.as_object().unwrap(), b.as_object().unwrap());
        let mut out: Vec<String> = a
            .iter()
            .filter(|(k, v)| b.get(k.as_str()) != Some(v))
            .map(|(k, _)| k.clone())
            .collect();
        out.sort_unstable();
        out
    }

    #[test]
    fn from_os_colors_view_bg_overrides_the_content_surface() {
        for scheme in [Scheme::Light, Scheme::Dark] {
            let base = ColorTokens::from_os_colors(&os(scheme));
            let bg = Color::from_hex("#FCFCFC");
            let mut report = os(scheme);
            report.view_bg = Some(bg);
            let t = ColorTokens::from_os_colors(&report);
            assert_eq!(t.surface_content, bg);
            assert_eq!(t.editor_bg, bg);
            assert_ne!(t.editor_current_line_bg, bg);
            assert_eq!(
                changed_fields(&t, &base),
                [
                    "editor_bg",
                    "editor_code_block_bg",
                    "editor_current_line_bg",
                    "surface_content"
                ]
            );
        }
    }

    #[test]
    fn from_os_colors_view_bg_wins_over_the_window_derivation() {
        let mut report = os(Scheme::Light);
        report.window_bg = Some(Color::from_hex("#EFF0F1"));
        report.view_bg = Some(Color::from_hex("#FFFFFF"));
        let t = ColorTokens::from_os_colors(&report);
        assert_eq!(t.surface_main, Color::from_hex("#EFF0F1"));
        assert_eq!(t.surface_content, Color::from_hex("#FFFFFF"));
        assert_eq!(t.editor_bg, Color::from_hex("#FFFFFF"));
    }

    #[test]
    fn from_os_colors_view_fg_overrides_the_editor_text() {
        let base = ColorTokens::from_os_colors(&os(Scheme::Dark));
        let fg = Color::from_hex("#FCFCFC");
        let mut report = os(Scheme::Dark);
        report.view_fg = Some(fg);
        let t = ColorTokens::from_os_colors(&report);
        assert_eq!(t.editor_fg, fg);
        assert_eq!(t.editor_caret, fg);
        assert_eq!(changed_fields(&t, &base), ["editor_caret", "editor_fg"]);
    }

    #[test]
    fn from_os_colors_alternate_bg_overrides_the_alt_row() {
        let base = ColorTokens::from_os_colors(&os(Scheme::Light));
        let alt = Color::from_hex("#F7F7F7");
        let mut report = os(Scheme::Light);
        report.alternate_bg = Some(alt);
        let t = ColorTokens::from_os_colors(&report);
        assert_eq!(t.surface_alt_row, alt);
        assert_eq!(changed_fields(&t, &base), ["surface_alt_row"]);
    }

    #[test]
    fn from_os_colors_inactive_fg_overrides_secondary_text_and_is_lifted() {
        let base = ColorTokens::from_os_colors(&os(Scheme::Light));
        // Readable already: taken as is.
        let mut report = os(Scheme::Light);
        report.inactive_fg = Some(Color::from_hex("#4D4F52"));
        let t = ColorTokens::from_os_colors(&report);
        assert_eq!(t.text_secondary, Color::from_hex("#4D4F52"));
        assert_eq!(changed_fields(&t, &base), ["text_secondary"]);
        // Breeze's #707D8A on a white view is about 4.3:1: lifted.
        report.view_bg = Some(Color::WHITE);
        report.inactive_fg = Some(Color::from_hex("#707D8A"));
        let t = ColorTokens::from_os_colors(&report);
        assert_ne!(t.text_secondary, Color::from_hex("#707D8A"));
        for bg in [t.surface_main, t.surface_content] {
            assert!(composite_contrast(t.text_secondary, bg) >= 4.5);
        }
    }

    #[test]
    fn from_os_colors_links_override_and_are_lifted() {
        for scheme in [Scheme::Light, Scheme::Dark] {
            let base = ColorTokens::from_os_colors(&os(scheme));
            let mut report = os(scheme);
            report.link = Some(Color::from_hex("#2980B9"));
            report.link_visited = Some(Color::from_hex("#9B59B6"));
            let t = ColorTokens::from_os_colors(&report);
            assert_eq!(
                changed_fields(&t, &base),
                ["text_link", "text_link_hover", "text_link_visited"]
            );
            for c in [t.text_link, t.text_link_hover, t.text_link_visited] {
                for bg in [t.surface_main, t.surface_content] {
                    assert!(composite_contrast(c, bg) >= 4.5, "{scheme:?} {c:?}");
                }
            }
            assert_ne!(t.text_link, t.text_link_hover);
        }
        // A readable link is taken exactly.
        let mut report = os(Scheme::Light);
        report.link = Some(Color::from_hex("#1D5E8A"));
        let t = ColorTokens::from_os_colors(&report);
        assert_eq!(t.text_link, Color::from_hex("#1D5E8A"));
    }

    #[test]
    fn from_os_colors_semantic_colours_override_text_and_status() {
        for scheme in [Scheme::Light, Scheme::Dark] {
            let base = ColorTokens::from_os_colors(&os(scheme));
            let mut report = os(scheme);
            report.error = Some(Color::from_hex("#DA4453"));
            report.warning = Some(Color::from_hex("#F67400"));
            report.success = Some(Color::from_hex("#27AE60"));
            let t = ColorTokens::from_os_colors(&report);
            assert_eq!(
                changed_fields(&t, &base),
                [
                    "status_error_fg",
                    "status_success_fg",
                    "status_warning_fg",
                    "text_error",
                    "text_success",
                    "text_warning"
                ]
            );
            for (text, status, status_bg) in [
                (t.text_error, t.status_error_fg, t.status_error_bg),
                (t.text_warning, t.status_warning_fg, t.status_warning_bg),
                (t.text_success, t.status_success_fg, t.status_success_bg),
            ] {
                for bg in [t.surface_main, t.surface_content] {
                    assert!(composite_contrast(text, bg) >= 4.5, "{scheme:?} {text:?}");
                    assert!(
                        composite_contrast(status, bg) >= 3.0,
                        "{scheme:?} {status:?}"
                    );
                }
                assert!(composite_contrast(status, status_bg) >= 3.0);
            }
        }
        // Readable values are taken exactly.
        let mut report = os(Scheme::Light);
        report.error = Some(Color::from_hex("#A0202C"));
        let t = ColorTokens::from_os_colors(&report);
        assert_eq!(t.text_error, Color::from_hex("#A0202C"));
        assert_eq!(t.status_error_fg, Color::from_hex("#A0202C"));
    }

    #[test]
    fn from_os_colors_focus_overrides_the_ring_and_is_lifted() {
        for scheme in [Scheme::Light, Scheme::Dark] {
            let base = ColorTokens::from_os_colors(&os(scheme));
            let mut report = os(scheme);
            report.focus = Some(Color::from_hex("#3DAEE9"));
            let t = ColorTokens::from_os_colors(&report);
            assert_eq!(changed_fields(&t, &base), ["border_focused", "focus_ring"]);
            assert_eq!(t.focus_ring, t.border_focused);
            for bg in [t.surface_main, t.surface_content] {
                assert!(composite_contrast(t.focus_ring, bg) >= 3.0);
            }
        }
        // The focus field wins over the accent's ring.
        let mut report = os(Scheme::Dark);
        report.accent = Some(Color::from_hex("#FF5500"));
        report.focus = Some(Color::from_hex("#3DAEE9"));
        let t = ColorTokens::from_os_colors(&report);
        assert_eq!(t.accent, Color::from_hex("#FF5500"));
        assert_eq!(t.focus_ring, Color::from_hex("#3DAEE9"));
    }

    #[test]
    fn from_os_colors_without_the_new_fields_is_unchanged() {
        // The pre-existing fields alone give the same tokens whether the
        // report is built with `new` or as a literal: no new field leaks in.
        let mut report = os(Scheme::Dark);
        report.window_bg = Some(Color::from_hex("#202326"));
        report.window_fg = Some(Color::from_hex("#FCFCFC"));
        report.selection_bg = Some(Color::from_hex("#3DAEE9"));
        report.accent = Some(Color::from_hex("#3DAEE9"));
        let literal = OsThemeColors {
            color_scheme: Scheme::Dark,
            accent: Some(Color::from_hex("#3DAEE9")),
            window_bg: Some(Color::from_hex("#202326")),
            window_fg: Some(Color::from_hex("#FCFCFC")),
            selection_bg: Some(Color::from_hex("#3DAEE9")),
            ..Default::default()
        };
        assert_eq!(
            ColorTokens::from_os_colors(&report),
            ColorTokens::from_os_colors(&literal)
        );
        assert_eq!(
            ColorTokens::from_os_colors(&os(Scheme::Light)),
            ColorTokens::light_default()
        );
        assert_eq!(
            ColorTokens::from_os_colors(&os(Scheme::Dark)),
            ColorTokens::dark_default()
        );
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
