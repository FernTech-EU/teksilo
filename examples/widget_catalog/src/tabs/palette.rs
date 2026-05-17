//! Palette tab — exhaustive showcase of `SurfaceRole`, `TextRole`, and
//! the editor-pane roles. Includes a rich-text + emoji pangram so
//! color-emoji font fallback is visibly exercised under each theme.

use bastyde::prelude::*;
use bastyde::widgets::primitives::TrackSize;
use bastyde::widgets::{
    Divider, FixedSize, Grid, HStack, Padding, Panel, Spacer, TextWidget, VStack,
};

use crate::shared::{Signals, section, tab_header};

pub fn title() -> LocalizedString {
    tr!(tab_palette_title())
}

pub fn refs() -> LocalizedString {
    tr!(tab_palette_refs())
}

const PANGRAM: &str = "The quick brown 🦊 jumps over the lazy 🐶 🎉";

fn surface_swatch(
    bg: impl Into<ColorProp>,
    name: &str,
    text_role: &str,
    text_color: impl Into<ColorProp>,
) -> impl Widget + 'static {
    VStack::new()
        .spacing(4.0)
        .child(
            Panel::new()
                .background(bg)
                .border_color(BorderRole::Strong)
                .border_width(1.0)
                .corner_radius(4.0)
                .padding(10.0)
                .child(
                    TextWidget::new_literal(name)
                        .style(TextStyleRole::Small)
                        .color(text_color),
                ),
        )
        .child(
            TextWidget::new_literal(text_role)
                .style(TextStyleRole::Tiny)
                .color(TextRole::Secondary),
        )
}

fn text_sample(
    name: &str,
    color: impl Into<ColorProp>,
    description: &str,
) -> impl Widget + 'static {
    HStack::new()
        .spacing(12.0)
        .child(
            TextWidget::new_literal(PANGRAM)
                .style(TextStyleRole::Body)
                .color(color),
        )
        .child(Spacer::new())
        .child(
            TextWidget::new_literal(name)
                .style(TextStyleRole::Tiny)
                .color(TextRole::Secondary),
        )
        .child(
            TextWidget::new_literal(description)
                .style(TextStyleRole::Tiny)
                .color(TextRole::Secondary),
        )
}

fn editor_line(line_no: &str, code: &str) -> impl Widget + 'static {
    HStack::new()
        .spacing(12.0)
        .child(
            FixedSize::new().bind_width(24.0_f32).child(
                TextWidget::new_literal(line_no)
                    .style(TextStyleRole::Mono)
                    .color(TextRole::EditorGutterFg),
            ),
        )
        .child(
            TextWidget::new_literal(code)
                .style(TextStyleRole::Mono)
                .color(TextRole::EditorFg),
        )
}

fn editor_swatch(
    bg: impl Into<ColorProp>,
    name: &str,
    sample_color: impl Into<ColorProp>,
) -> impl Widget + 'static {
    VStack::new()
        .spacing(4.0)
        .child(
            Panel::new()
                .background(bg)
                .border_color(BorderRole::Strong)
                .border_width(1.0)
                .corner_radius(4.0)
                .padding(10.0)
                .child(
                    TextWidget::new_literal("Aa Bb 123")
                        .style(TextStyleRole::Mono)
                        .color(sample_color),
                ),
        )
        .child(
            TextWidget::new_literal(name)
                .style(TextStyleRole::Tiny)
                .color(TextRole::Secondary),
        )
}

fn surfaces_grid() -> impl Widget + 'static {
    Grid::new()
        .columns(vec![
            TrackSize::Fractional(1.0),
            TrackSize::Fractional(1.0),
            TrackSize::Fractional(1.0),
            TrackSize::Fractional(1.0),
        ])
        .column_gap(12.0)
        .row_gap(12.0)
        .rows(vec![
            TrackSize::Auto,
            TrackSize::Auto,
            TrackSize::Auto,
            TrackSize::Auto,
        ])
        .child(surface_swatch(
            SurfaceRole::Main,
            "surface_main",
            "text_primary",
            TextRole::Primary,
        ))
        .child(surface_swatch(
            SurfaceRole::Content,
            "surface_content",
            "text_primary",
            TextRole::Primary,
        ))
        .child(surface_swatch(
            SurfaceRole::Raised,
            "surface_raised",
            "text_primary",
            TextRole::Primary,
        ))
        .child(surface_swatch(
            SurfaceRole::Sunken,
            "surface_sunken",
            "text_secondary",
            TextRole::Secondary,
        ))
        .child(surface_swatch(
            SurfaceRole::Hover,
            "surface_hover",
            "text_primary",
            TextRole::Primary,
        ))
        .child(surface_swatch(
            SurfaceRole::Pressed,
            "surface_pressed",
            "text_primary",
            TextRole::Primary,
        ))
        .child(surface_swatch(
            SurfaceRole::Selected,
            "surface_selected",
            "text_primary",
            TextRole::Primary,
        ))
        .child(surface_swatch(
            SurfaceRole::SelectedInactive,
            "surface_selected_inactive",
            "text_primary",
            TextRole::Primary,
        ))
        .child(surface_swatch(
            SurfaceRole::AltRow,
            "surface_alt_row",
            "text_primary",
            TextRole::Primary,
        ))
        .child(surface_swatch(
            SurfaceRole::AccentSubtle,
            "accent_subtle_bg",
            "text_primary",
            TextRole::Primary,
        ))
        .child(surface_swatch(
            SurfaceRole::StatusInfo,
            "status_info_bg",
            "text_primary",
            TextRole::Primary,
        ))
        .child(surface_swatch(
            SurfaceRole::StatusSuccess,
            "status_success_bg",
            "text_primary",
            TextRole::Success,
        ))
        .child(surface_swatch(
            SurfaceRole::StatusWarning,
            "status_warning_bg",
            "text_primary",
            TextRole::Warning,
        ))
        .child(surface_swatch(
            SurfaceRole::StatusError,
            "status_error_bg",
            "text_primary",
            TextRole::Error,
        ))
        .child(surface_swatch(
            SurfaceRole::Accent,
            "accent",
            "text_on_accent",
            TextRole::OnAccent,
        ))
        .child(surface_swatch(
            SurfaceRole::AccentHover,
            "accent_hover",
            "text_on_accent",
            TextRole::OnAccent,
        ))
}

fn text_samples_panel() -> impl Widget + 'static {
    Panel::new()
        .background(SurfaceRole::Main)
        .border_color(BorderRole::Default)
        .border_width(1.0)
        .corner_radius(8.0)
        .padding(16.0)
        .child(
            VStack::new()
                .spacing(6.0)
                .child(text_sample(
                    "text_primary",
                    TextRole::Primary,
                    "body, main labels",
                ))
                .child(text_sample(
                    "text_secondary",
                    TextRole::Secondary,
                    "hints, captions, placeholders",
                ))
                .child(text_sample(
                    "text_disabled",
                    TextRole::Disabled,
                    "disabled labels",
                ))
                .child(text_sample("text_link", TextRole::Link, "hyperlinks"))
                .child(text_sample(
                    "text_error",
                    TextRole::Error,
                    "validation errors",
                ))
                .child(text_sample(
                    "text_warning",
                    TextRole::Warning,
                    "validation warnings",
                ))
                .child(text_sample(
                    "text_success",
                    TextRole::Success,
                    "success messages",
                )),
        )
}

fn text_on_accent_row() -> impl Widget + 'static {
    Panel::new()
        .background(SurfaceRole::Accent)
        .corner_radius(4.0)
        .padding(12.0)
        .child(
            HStack::new()
                .spacing(12.0)
                .child(
                    TextWidget::new_literal(PANGRAM)
                        .style(TextStyleRole::Body)
                        .color(TextRole::OnAccent),
                )
                .child(Spacer::new())
                .child(
                    TextWidget::new_literal("text_on_accent on accent")
                        .style(TextStyleRole::Tiny)
                        .color(TextRole::OnAccent),
                ),
        )
}

fn mock_editor() -> impl Widget + 'static {
    let current_line = HStack::new()
        .spacing(12.0)
        .child(
            FixedSize::new().bind_width(24.0_f32).child(
                TextWidget::new_literal("2")
                    .style(TextStyleRole::Mono)
                    .color(TextRole::EditorGutterFg),
            ),
        )
        .child(
            TextWidget::new_literal("    let ")
                .style(TextStyleRole::Mono)
                .color(TextRole::EditorFg),
        )
        .child(
            Panel::new()
                .background(SurfaceRole::EditorSelectionBg)
                .corner_radius(2.0)
                .padding(0.0)
                .border_width(0.0)
                .child(
                    Padding::symmetric(1.0, 2.0).child(
                        TextWidget::new_literal("x")
                            .style(TextStyleRole::Mono)
                            .color(TextRole::EditorFg),
                    ),
                ),
        )
        .child(
            TextWidget::new_literal(" = 42;")
                .style(TextStyleRole::Mono)
                .color(TextRole::EditorFg),
        )
        .child(
            FixedSize::new()
                .bind_width(1.5_f32)
                .bind_height(16.0_f32)
                .child(
                    Panel::new()
                        .background(SurfaceRole::EditorCaret)
                        .corner_radius(0.0)
                        .border_width(0.0)
                        .padding(0.0)
                        .child(Spacer::new()),
                ),
        );

    let current_line_bg = Panel::new()
        .background(SurfaceRole::EditorCurrentLineBg)
        .corner_radius(0.0)
        .border_width(0.0)
        .padding(4.0)
        .child(current_line);

    Panel::new()
        .background(SurfaceRole::EditorBg)
        .border_color(BorderRole::Strong)
        .border_width(1.0)
        .corner_radius(6.0)
        .padding(8.0)
        .child(
            VStack::new()
                .spacing(2.0)
                .child(editor_line("1", "fn main() {"))
                .child(current_line_bg)
                .child(editor_line("3", "    println!(\"{}\", x);"))
                .child(editor_line("4", "}")),
        )
}

fn editor_swatches_grid() -> impl Widget + 'static {
    Grid::new()
        .columns(vec![
            TrackSize::Fractional(1.0),
            TrackSize::Fractional(1.0),
            TrackSize::Fractional(1.0),
            TrackSize::Fractional(1.0),
        ])
        .column_gap(12.0)
        .row_gap(12.0)
        .rows(vec![TrackSize::Auto, TrackSize::Auto])
        .child(editor_swatch(
            SurfaceRole::EditorBg,
            "editor_bg",
            TextRole::EditorFg,
        ))
        .child(editor_swatch(
            TextRole::EditorFg,
            "editor_fg",
            SurfaceRole::EditorBg,
        ))
        .child(editor_swatch(
            SurfaceRole::EditorCaret,
            "editor_caret",
            SurfaceRole::EditorBg,
        ))
        .child(editor_swatch(
            SurfaceRole::EditorCurrentLineBg,
            "editor_current_line_bg",
            TextRole::EditorFg,
        ))
        .child(editor_swatch(
            TextRole::EditorGutterFg,
            "editor_gutter_fg",
            SurfaceRole::EditorBg,
        ))
        .child(editor_swatch(
            SurfaceRole::EditorSelectionBg,
            "editor_selection_bg",
            TextRole::EditorFg,
        ))
}

/// Body for the "Text" section: pangram samples panel + on-accent row.
fn text_section_body() -> impl Widget + 'static {
    VStack::new()
        .spacing(8.0)
        .child(text_samples_panel())
        .child(text_on_accent_row())
}

/// Body for the "Editor" section: mock code pane + role swatch grid.
fn editor_section_body() -> impl Widget + 'static {
    VStack::new()
        .spacing(8.0)
        .child(mock_editor())
        .child(editor_swatches_grid())
}

pub fn classic(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    let header = tab_header(ctx, title(), refs());
    let surfaces = section(ctx, "Surfaces", surfaces_grid());
    let text = section(ctx, "Text", text_section_body());
    let editor = section(ctx, "Editor", editor_section_body());

    ctx.add(
        VStack::new()
            .spacing(20.0)
            .add_child(header)
            .child(Divider::new())
            .add_child(surfaces)
            .add_child(text)
            .add_child(editor),
    )
}

/// Surface-swatch entries: (background role, name to print, text-role
/// caption, foreground role for the printed name). Driven by a `for`
/// loop in the bati! body.
const SURFACES: &[(SurfaceRole, &str, &str, TextRole)] = &[
    (
        SurfaceRole::Main,
        "surface_main",
        "text_primary",
        TextRole::Primary,
    ),
    (
        SurfaceRole::Content,
        "surface_content",
        "text_primary",
        TextRole::Primary,
    ),
    (
        SurfaceRole::Raised,
        "surface_raised",
        "text_primary",
        TextRole::Primary,
    ),
    (
        SurfaceRole::Sunken,
        "surface_sunken",
        "text_secondary",
        TextRole::Secondary,
    ),
    (
        SurfaceRole::Hover,
        "surface_hover",
        "text_primary",
        TextRole::Primary,
    ),
    (
        SurfaceRole::Pressed,
        "surface_pressed",
        "text_primary",
        TextRole::Primary,
    ),
    (
        SurfaceRole::Selected,
        "surface_selected",
        "text_primary",
        TextRole::Primary,
    ),
    (
        SurfaceRole::SelectedInactive,
        "surface_selected_inactive",
        "text_primary",
        TextRole::Primary,
    ),
    (
        SurfaceRole::AltRow,
        "surface_alt_row",
        "text_primary",
        TextRole::Primary,
    ),
    (
        SurfaceRole::AccentSubtle,
        "accent_subtle_bg",
        "text_primary",
        TextRole::Primary,
    ),
    (
        SurfaceRole::StatusInfo,
        "status_info_bg",
        "text_primary",
        TextRole::Primary,
    ),
    (
        SurfaceRole::StatusSuccess,
        "status_success_bg",
        "text_primary",
        TextRole::Success,
    ),
    (
        SurfaceRole::StatusWarning,
        "status_warning_bg",
        "text_primary",
        TextRole::Warning,
    ),
    (
        SurfaceRole::StatusError,
        "status_error_bg",
        "text_primary",
        TextRole::Error,
    ),
    (
        SurfaceRole::Accent,
        "accent",
        "text_on_accent",
        TextRole::OnAccent,
    ),
    (
        SurfaceRole::AccentHover,
        "accent_hover",
        "text_on_accent",
        TextRole::OnAccent,
    ),
];

/// `text_samples_panel` rows: (printed name, foreground role, English
/// description). Driven by a `for` loop in the bati! body.
type TextSampleRow = (&'static str, TextRole, &'static str);
const TEXT_SAMPLES: &[TextSampleRow] = &[
    ("text_primary", TextRole::Primary, "body, main labels"),
    (
        "text_secondary",
        TextRole::Secondary,
        "hints, captions, placeholders",
    ),
    ("text_disabled", TextRole::Disabled, "disabled labels"),
    ("text_link", TextRole::Link, "hyperlinks"),
    ("text_error", TextRole::Error, "validation errors"),
    ("text_warning", TextRole::Warning, "validation warnings"),
    ("text_success", TextRole::Success, "success messages"),
];

/// Editor-swatch entries shown in `editor_swatches_grid`: (background
/// role for the panel, name, foreground role for the "Aa Bb 123" sample).
type EditorSwatchEntry = (ColorProp, &'static str, ColorProp);
fn editor_swatches() -> [EditorSwatchEntry; 6] {
    [
        (
            SurfaceRole::EditorBg.into(),
            "editor_bg",
            TextRole::EditorFg.into(),
        ),
        (
            TextRole::EditorFg.into(),
            "editor_fg",
            SurfaceRole::EditorBg.into(),
        ),
        (
            SurfaceRole::EditorCaret.into(),
            "editor_caret",
            SurfaceRole::EditorBg.into(),
        ),
        (
            SurfaceRole::EditorCurrentLineBg.into(),
            "editor_current_line_bg",
            TextRole::EditorFg.into(),
        ),
        (
            TextRole::EditorGutterFg.into(),
            "editor_gutter_fg",
            SurfaceRole::EditorBg.into(),
        ),
        (
            SurfaceRole::EditorSelectionBg.into(),
            "editor_selection_bg",
            TextRole::EditorFg.into(),
        ),
    ]
}

pub fn bati(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    bati!(ctx => VStack {
            spacing: 20.0

            // ── tab header ──────────────────────────────────────────
            VStack {
                spacing: 4.0
                TextWidget::new(tr!(tab_palette_title())) {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new(tr!(tab_palette_refs())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }
            Divider

            // ── Surfaces section ────────────────────────────────────
            VStack {
                spacing: 6.0
                TextWidget::new_literal("Surfaces") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Grid {
                    columns: vec![
                        TrackSize::Fractional(1.0),
                        TrackSize::Fractional(1.0),
                        TrackSize::Fractional(1.0),
                        TrackSize::Fractional(1.0),
                    ]
                    rows: vec![
                        TrackSize::Auto,
                        TrackSize::Auto,
                        TrackSize::Auto,
                        TrackSize::Auto,
                    ]
                    column_gap: 12.0
                    row_gap: 12.0
                    for (bg, name, role_caption, fg) in SURFACES.iter() {
                        let bg = *bg;
                        let name = *name;
                        let role_caption = *role_caption;
                        let fg = *fg;
                        VStack {
                            spacing: 4.0
                            Panel {
                                background: bg
                                border_color: BorderRole::Strong
                                border_width: 1.0
                                corner_radius: 4.0
                                padding: 10.0
                                TextWidget::new_literal(name) {
                                    style: TextStyleRole::Small
                                    color: fg
                                }
                            }
                            TextWidget::new_literal(role_caption) {
                                style: TextStyleRole::Tiny
                                color: TextRole::Secondary
                            }
                        }
                    }
                }
            }

            // ── Text section ────────────────────────────────────────
            VStack {
                spacing: 6.0
                TextWidget::new_literal("Text") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                VStack {
                    spacing: 8.0
                    Panel {
                        background: SurfaceRole::Main
                        border_color: BorderRole::Default
                        border_width: 1.0
                        corner_radius: 8.0
                        padding: 16.0
                        VStack {
                            spacing: 6.0
                            for (name, fg, description) in TEXT_SAMPLES.iter() {
                                let name = *name;
                                let fg = *fg;
                                let description = *description;
                                HStack {
                                    spacing: 12.0
                                    TextWidget::new_literal(PANGRAM) {
                                        style: TextStyleRole::Body
                                        color: fg
                                    }
                                    Spacer {}
                                    TextWidget::new_literal(name) {
                                        style: TextStyleRole::Tiny
                                        color: TextRole::Secondary
                                    }
                                    TextWidget::new_literal(description) {
                                        style: TextStyleRole::Tiny
                                        color: TextRole::Secondary
                                    }
                                }
                            }
                        }
                    }
                    Panel {
                        background: SurfaceRole::Accent
                        corner_radius: 4.0
                        padding: 12.0
                        HStack {
                            spacing: 12.0
                            TextWidget::new_literal(PANGRAM) {
                                style: TextStyleRole::Body
                                color: TextRole::OnAccent
                            }
                            Spacer
                            TextWidget::new_literal("text_on_accent on accent") {
                                style: TextStyleRole::Tiny
                                color: TextRole::OnAccent
                            }
                        }
                    }
                }
            }

            // ── Editor section ──────────────────────────────────────
            VStack {
                spacing: 6.0
                TextWidget::new_literal("Editor") {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                VStack {
                    spacing: 8.0
                    // Mock editor — fn main() with current-line highlight + caret.
                    Panel {
                        background: SurfaceRole::EditorBg
                        border_color: BorderRole::Strong
                        border_width: 1.0
                        corner_radius: 6.0
                        padding: 8.0
                        VStack {
                            spacing: 2.0
                            HStack {
                                spacing: 12.0
                                FixedSize {
                                    bind_width: 24.0_f32
                                    TextWidget::new_literal("1") {
                                        style: TextStyleRole::Mono
                                        color: TextRole::EditorGutterFg
                                    }
                                }
                                TextWidget::new_literal("fn main() {") {
                                    style: TextStyleRole::Mono
                                    color: TextRole::EditorFg
                                }
                            }
                            // Current line: gutter, "    let ", selected "x",
                            // " = 42;", caret bar.
                            Panel {
                                background: SurfaceRole::EditorCurrentLineBg
                                corner_radius: 0.0
                                border_width: 0.0
                                padding: 4.0
                                HStack {
                                    spacing: 12.0
                                    FixedSize {
                                        bind_width: 24.0_f32
                                        TextWidget::new_literal("2") {
                                            style: TextStyleRole::Mono
                                            color: TextRole::EditorGutterFg
                                        }
                                    }
                                    TextWidget::new_literal("    let ") {
                                        style: TextStyleRole::Mono
                                        color: TextRole::EditorFg
                                    }
                                    Panel {
                                        background: SurfaceRole::EditorSelectionBg
                                        corner_radius: 2.0
                                        padding: 0.0
                                        border_width: 0.0
                                        Padding::symmetric(1.0, 2.0) {
                                            TextWidget::new_literal("x") {
                                                style: TextStyleRole::Mono
                                                color: TextRole::EditorFg
                                            }
                                        }
                                    }
                                    TextWidget::new_literal(" = 42;") {
                                        style: TextStyleRole::Mono
                                        color: TextRole::EditorFg
                                    }
                                    FixedSize {
                                        bind_width: 1.5_f32
                                        bind_height: 16.0_f32
                                        Panel {
                                            background: SurfaceRole::EditorCaret
                                            corner_radius: 0.0
                                            border_width: 0.0
                                            padding: 0.0
                                            Spacer
                                        }
                                    }
                                }
                            }
                            HStack {
                                spacing: 12.0
                                FixedSize {
                                    bind_width: 24.0_f32
                                    TextWidget::new_literal("3") {
                                        style: TextStyleRole::Mono
                                        color: TextRole::EditorGutterFg
                                    }
                                }
                                TextWidget::new_literal("    println!(\"{}\", x);") {
                                    style: TextStyleRole::Mono
                                    color: TextRole::EditorFg
                                }
                            }
                            HStack {
                                spacing: 12.0
                                FixedSize {
                                    bind_width: 24.0_f32
                                    TextWidget::new_literal("4") {
                                        style: TextStyleRole::Mono
                                        color: TextRole::EditorGutterFg
                                    }
                                }
                                TextWidget::new_literal("}") {
                                    style: TextStyleRole::Mono
                                    color: TextRole::EditorFg
                                }
                            }
                        }
                    }
                    // Editor swatches grid — six role swatches.
                    Grid {
                        columns: vec![
                            TrackSize::Fractional(1.0),
                            TrackSize::Fractional(1.0),
                            TrackSize::Fractional(1.0),
                            TrackSize::Fractional(1.0),
                        ]
                        rows: vec![TrackSize::Auto, TrackSize::Auto]
                        column_gap: 12.0
                        row_gap: 12.0
                        for (bg, name, fg) in editor_swatches().into_iter() {
                            let bg_clone = bg.clone();
                            let fg_clone = fg.clone();
                            VStack {
                                spacing: 4.0
                                Panel {
                                    background: bg_clone
                                    border_color: BorderRole::Strong
                                    border_width: 1.0
                                    corner_radius: 4.0
                                    padding: 10.0
                                    TextWidget::new_literal("Aa Bb 123") {
                                        style: TextStyleRole::Mono
                                        color: fg_clone
                                    }
                                }
                                TextWidget::new_literal(name) {
                                    style: TextStyleRole::Tiny
                                    color: TextRole::Secondary
                                }
                            }
                        }
                    }
                }
            }
        }
    )
}
