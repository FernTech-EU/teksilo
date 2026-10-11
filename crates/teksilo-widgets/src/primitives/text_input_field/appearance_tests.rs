// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The field's appearance knobs: its text style, the alignment of a text
//! narrower than the field, and the text colour.
//!
//! The field shapes through its own `RichTextEngine` — a private one here, the
//! embedded Inter at 14 px — while the tree's text backend is the mock one, so
//! every geometry below is read back from the engine rather than assumed.

use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::environment::LayoutDirection;
use teksilo_core::event::PointerButton;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_tokens::{Color, FontWeight, HAlignment, TextRole, TextStyle, TextStyleRole};

use super::state::{PRIVATE_ENGINE_BASE_SIZE, SharedState};
use super::*;

/// The field's window origin. Off the origin, so a conversion that forgot the
/// viewport cannot pass by accident.
fn origin() -> Point {
    Point::new(20.0, 120.0)
}

/// The field's width: the 400 dp viewport less 20 dp either side.
const FIELD_WIDTH: f32 = 360.0;

struct Field {
    tree: WidgetTree,
    state: SharedState,
    field: WidgetId,
}

impl Field {
    fn new(field: TextInputField) -> Self {
        Self::with_tree(
            WidgetTree::new().with_theme(teksilo_core::presets::intui::light()),
            field,
        )
    }

    fn with_tree(mut tree: WidgetTree, field: TextInputField) -> Self {
        let slot = field.state_slot.clone();
        // In a `VStack`, which lays the field out at the height it asks for
        // rather than stretching it over the viewport.
        tree.add(
            crate::primitives::VStack::new()
                .child(crate::primitives::Padding::symmetric(120.0, 20.0).child(field)),
        );
        let state = slot.borrow().clone().expect("the field was built");
        let field = state
            .borrow()
            .field_widget_id
            .expect("the field is mounted");
        let mut built = Self { tree, state, field };
        built.render();
        built
    }

    fn render(&mut self) -> std::rc::Rc<teksilo_canvas::RenderFrame> {
        self.tree.layout(SizeProposal::exact(400.0, 400.0));
        self.tree.render()
    }

    fn height(&self) -> f32 {
        self.tree.bounds(self.field).height
    }

    /// The window x of the caret boundary before character `offset`.
    fn x_of(&self, offset: usize) -> f32 {
        let st = self.state.borrow();
        origin().x + st.engine.caret_rect(offset, CursorAffinity::Downstream)[0] - st.scroll_x
    }

    /// The width the text and a caret after it take up.
    fn run_width(&self) -> f32 {
        let st = self.state.borrow();
        let caret = st
            .engine
            .caret_rect(st.cursor.position(), CursorAffinity::Downstream);
        st.engine.max_content_width() + caret[2].max(1.0)
    }
}

fn fixed_style(size: f32, line_height: f32) -> TextStyle {
    TextStyle {
        family: "Inter".to_string(),
        size,
        weight: FontWeight::REGULAR,
        line_height,
        letter_spacing: 0.0,
    }
}

// ---------------------------------------------------------------------------
// Text style
// ---------------------------------------------------------------------------

/// A style sizes the field to one line of `size × line_height`: a small style
/// a short field, a large one a tall field. Without one the field keeps the
/// default height it has always had.
#[test]
fn a_text_style_sets_the_fields_height_from_its_line() {
    let plain = Field::new(TextInputField::new(Signal::new("abc".into())));
    assert_eq!(
        plain.height(),
        DEFAULT_TEXT_HEIGHT,
        "the default is untouched"
    );

    let small = Field::new(
        TextInputField::new(Signal::new("abc".into())).text_style(fixed_style(11.0, 1.3)),
    );
    assert!(
        (small.height() - 11.0 * 1.3).abs() < 0.01,
        "{}",
        small.height()
    );

    let large = Field::new(
        TextInputField::new(Signal::new("abc".into())).text_style(fixed_style(20.0, 1.4)),
    );
    assert!((large.height() - 28.0).abs() < 0.01, "{}", large.height());
}

/// An explicit `text_height` is what a composite asked for, and wins.
#[test]
fn an_explicit_text_height_wins_over_the_styles_line() {
    let field = Field::new(
        TextInputField::new(Signal::new("abc".into()))
            .text_style(fixed_style(20.0, 1.4))
            .text_height(18.0),
    );
    assert_eq!(field.height(), 18.0);
}

/// The style's size reaches the glyphs, and the global text scale still
/// multiplies it.
#[test]
fn a_text_style_size_reaches_the_engine_and_the_text_scale_multiplies_it() {
    let mut field = Field::new(
        TextInputField::new(Signal::new("abc".into())).text_style(fixed_style(21.0, 1.4)),
    );
    let expected = 21.0 / PRIVATE_ENGINE_BASE_SIZE;
    let scale = field.state.borrow().engine.font_scale();
    assert!((scale - expected).abs() < 1e-4, "font scale {scale}");

    field.tree.set_user_text_scale(1.5);
    field.render();
    let scale = field.state.borrow().engine.font_scale();
    assert!((scale - expected * 1.5).abs() < 1e-4, "font scale {scale}");
    assert!(
        (field.height() - 21.0 * 1.4 * 1.5).abs() < 0.01,
        "the field grows with the text: {}",
        field.height()
    );
}

/// A role is resolved against the theme, and again when the theme changes.
#[test]
fn a_style_role_follows_the_theme() {
    let mut field =
        Field::new(TextInputField::new(Signal::new("abc".into())).text_style(TextStyleRole::Small));
    let theme = teksilo_core::presets::intui::light();
    let small = theme.typography.small.clone();
    assert!((field.height() - small.size * small.line_height).abs() < 0.01);

    let mut bigger = theme.clone();
    bigger.typography.small.size = 24.0;
    field.tree.set_theme(bigger);
    field.render();
    assert!(
        (field.height() - 24.0 * small.line_height).abs() < 0.01,
        "the height follows the new size: {}",
        field.height()
    );
    let scale = field.state.borrow().engine.font_scale();
    assert!(
        (scale - 24.0 / PRIVATE_ENGINE_BASE_SIZE).abs() < 1e-4,
        "and so do the glyphs: {scale}"
    );
}

/// The advance width of `text` shaped in `style`.
fn shaped_width(text: &str, style: TextStyle) -> f32 {
    let field = Field::new(TextInputField::new(Signal::new(text.into())).text_style(style));
    field.state.borrow().engine.max_content_width()
}

/// The family reaches the shaper: the same letters are much wider in the
/// monospaced face the engine registers beside Inter.
#[test]
fn a_text_style_family_reaches_the_shaper() {
    let inter = shaped_width("iiiiii", fixed_style(14.0, 1.4));
    let mono = shaped_width(
        "iiiiii",
        TextStyle {
            family: "JetBrains Mono".to_string(),
            ..fixed_style(14.0, 1.4)
        },
    );
    assert!(mono > inter * 1.5, "mono {mono} vs inter {inter}");
}

/// The total ink width of the glyphs `text` paints in `style`.
fn inked_width(text: &str, style: TextStyle) -> f32 {
    let mut field = Field::new(TextInputField::new(Signal::new(text.into())).text_style(style));
    field.render().glyphs.iter().map(|g| g.screen[2]).sum()
}

/// The weight reaches the glyphs. The engine sets it on the variable face's
/// `wght` axis when it rasterises, not when it shapes, so the advances stay
/// those of the regular face and the strokes thicken: a bold run inks wider
/// glyph boxes over the same pen positions.
#[test]
fn a_text_style_weight_reaches_the_glyphs() {
    let regular = inked_width("Hamburgefonts", fixed_style(14.0, 1.4));
    let bold = inked_width(
        "Hamburgefonts",
        TextStyle {
            weight: FontWeight::BOLD,
            ..fixed_style(14.0, 1.4)
        },
    );
    assert!(bold > regular + 1.0, "bold {bold} vs regular {regular}");
}

/// The line is centred in the height the style gave the field, and the caret,
/// the IME area and a click all follow it. Without a style nothing moves.
#[test]
fn a_styled_line_is_centred_and_the_caret_follows_it() {
    let plain = Field::new(TextInputField::new(Signal::new("abc".into())));
    assert_eq!(plain.state.borrow().text_offset_y, 0.0);

    let mut field = Field::new(
        TextInputField::new(Signal::new("abc".into())).text_style(fixed_style(20.0, 1.6)),
    );
    let (offset, line) = {
        let st = field.state.borrow();
        (st.text_offset_y, st.engine.content_height())
    };
    assert!(
        offset > 1.0,
        "a 1.6 line leaves slack to centre in: {offset}"
    );
    assert!((offset - (field.height() - line) / 2.0).abs() < 0.01);

    field.tree.focus(field.field);
    field.render();
    let ime = field
        .state
        .borrow()
        .last_ime_area
        .expect("focus reports it");
    let caret_y = field
        .state
        .borrow()
        .engine
        .caret_rect(0, CursorAffinity::Downstream)[1];
    assert!((ime.y - (origin().y + offset + caret_y)).abs() < 0.01);

    // A press on the line's lower half still lands on the line.
    let at = Point::new(field.x_of(2), origin().y + offset + line * 0.9);
    field.tree.pointer_down_button(at, PointerButton::Primary);
    assert_eq!(field.state.borrow().cursor.position(), 2);
}

/// The suffix is shaped in the field's style, so it is as large as the text
/// it annotates.
#[test]
fn the_suffix_follows_the_text_style() {
    let plain = Field::new(TextInputField::new(Signal::new("12".into())).suffix(" kg"));
    let large = Field::new(
        TextInputField::new(Signal::new("12".into()))
            .suffix(" kg")
            .text_style(fixed_style(28.0, 1.4)),
    );
    let plain_width = plain.state.borrow().suffix_width;
    let large_width = large.state.borrow().suffix_width;
    assert!(
        large_width > plain_width * 1.5,
        "{large_width} vs {plain_width}"
    );
}

// ---------------------------------------------------------------------------
// Alignment
// ---------------------------------------------------------------------------

/// A centred short text: the caret and the first glyph sit at the centred
/// offset, and a click maps to the character under it.
#[test]
fn a_centred_short_text_is_drawn_and_hit_at_the_centre() {
    let mut field = Field::new(
        TextInputField::new(Signal::new("abcd".into())).text_alignment(HAlignment::Center),
    );
    let run = field.run_width();
    let start = origin().x + (FIELD_WIDTH - run) / 2.0;
    assert!(
        (field.x_of(0) - start).abs() < 0.01,
        "caret 0 at {} not {start}",
        field.x_of(0)
    );

    let frame = field.render();
    let first = frame
        .glyphs
        .iter()
        .map(|g| g.screen[0])
        .fold(f32::INFINITY, f32::min);
    assert!(
        first >= start - 1.0 && first < start + 4.0,
        "the first glyph is drawn at {first}, the text starts at {start}"
    );

    // A press just right of the boundary before `c` puts the caret there.
    let at = Point::new(field.x_of(2) + 0.5, origin().y + 8.0);
    field.tree.pointer_down_button(at, PointerButton::Primary);
    assert_eq!(field.state.borrow().cursor.position(), 2);
}

/// Trailing puts the end of the text, caret included, against the end of the
/// field.
#[test]
fn a_trailing_short_text_ends_at_the_fields_end() {
    let field = Field::new(
        TextInputField::new(Signal::new("abcd".into())).text_alignment(HAlignment::Trailing),
    );
    let run = field.run_width();
    assert!((field.x_of(0) - (origin().x + FIELD_WIDTH - run)).abs() < 0.01);
}

/// Leading is the start side: the right edge in a right-to-left layout.
#[test]
fn leading_is_the_start_side_in_a_right_to_left_layout() {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    tree.set_layout_direction(LayoutDirection::RightToLeft);
    let field = Field::with_tree(
        tree,
        TextInputField::new(Signal::new("abcd".into())).text_alignment(HAlignment::Leading),
    );
    let run = field.run_width();
    assert!(
        (field.x_of(0) - (origin().x + FIELD_WIDTH - run)).abs() < 0.01,
        "{}",
        field.x_of(0)
    );
}

/// Text wider than the field scrolls to follow the caret exactly as an
/// unaligned field's does — including a field that was centring a short text a
/// moment before, whose offset must not survive as a scroll.
#[test]
fn an_overflowing_aligned_text_scrolls_as_before() {
    let long = "the quick brown fox jumps over the lazy dog ".repeat(3);
    let text = Signal::new("ab".to_string());
    let centred_text = Signal::new("ab".to_string());
    let mut plain = Field::new(TextInputField::new(text.clone()));
    let mut centred =
        Field::new(TextInputField::new(centred_text.clone()).text_alignment(HAlignment::Center));
    assert!(
        centred.state.borrow().scroll_x < 0.0,
        "a short text is centred"
    );

    let scrolls = |field: &mut Field, signal: &Signal<String>, caret: usize| {
        field.tree.focus(field.field);
        signal.set(long.clone());
        field.tree.request_frame();
        field
            .tree
            .tick_animations(std::time::Duration::from_millis(16));
        field.render();
        field
            .state
            .borrow()
            .cursor
            .set_position(caret, teksilo_text::text_document::MoveMode::MoveAnchor);
        field.render();
        field.state.borrow().scroll_x
    };
    for caret in [0, long.chars().count()] {
        let plain_scroll = scrolls(&mut plain, &text, caret);
        let centred_scroll = scrolls(&mut centred, &centred_text, caret);
        assert_eq!(centred_scroll, plain_scroll, "caret at {caret}");
    }
    assert!(plain.state.borrow().scroll_x > 0.0, "the text overflows");
}

/// An offset that placed a short text is not a scroll: once the text
/// overflows, the field never keeps an empty band before its first character,
/// whichever way the caret got where it is.
#[test]
fn a_placement_offset_never_survives_an_overflow() {
    let long = "the quick brown fox jumps over the lazy dog ".repeat(3);
    let mut field = Field::new(
        TextInputField::new(Signal::new(long.clone())).text_alignment(HAlignment::Center),
    );
    field.tree.focus(field.field);
    field.render();
    {
        let mut st = field.state.borrow_mut();
        // A caret well inside the window, and the offset a centred short text
        // had a moment before: neither scroll branch is triggered.
        st.cursor
            .set_position(10, teksilo_text::text_document::MoveMode::MoveAnchor);
        st.scroll_x = -40.0;
    }
    field.tree.mark_needs_paint(field.field);
    field.render();
    assert!(field.state.borrow().scroll_x >= 0.0);
}

// ---------------------------------------------------------------------------
// Colour
// ---------------------------------------------------------------------------

fn glyph_colors(frame: &teksilo_canvas::RenderFrame) -> Vec<[f32; 4]> {
    frame.glyphs.iter().map(|g| g.color).collect()
}

#[test]
fn the_text_colour_is_painted_and_the_default_is_the_primary_text_colour() {
    let theme = teksilo_core::presets::intui::light();
    let mut plain = Field::new(TextInputField::new(Signal::new("abc".into())));
    let colors = glyph_colors(&plain.render());
    assert!(!colors.is_empty());
    assert!(
        colors
            .iter()
            .all(|c| *c == theme.colors.text_primary.to_array())
    );

    let red = Color::new(0.8, 0.0, 0.0, 1.0);
    let mut field = Field::new(TextInputField::new(Signal::new("abc".into())).text_color(red));
    let colors = glyph_colors(&field.render());
    assert!(!colors.is_empty());
    assert!(colors.iter().all(|c| *c == red.to_array()), "{colors:?}");
}

/// The pattern the docs give: a role that follows the field's focus.
#[test]
fn a_focus_driven_role_recolours_the_text_when_focus_moves() {
    let theme = teksilo_core::presets::intui::light();
    let field = TextInputField::new(Signal::new("abc".into()));
    let focused = field.handle().focused_signal();
    let field = field.text_color(focused.map(|f| {
        if *f {
            TextRole::Primary
        } else {
            TextRole::Secondary
        }
    }));
    let mut field = Field::new(field);
    let rest = glyph_colors(&field.render());
    assert!(
        rest.iter()
            .all(|c| *c == theme.colors.text_secondary.to_array())
    );

    field.tree.focus(field.field);
    let editing = glyph_colors(&field.render());
    assert!(!editing.is_empty());
    assert!(
        editing
            .iter()
            .all(|c| *c == theme.colors.text_primary.to_array()),
        "{editing:?}"
    );
}

/// A colour signal repaints the field when it changes, with nothing else
/// asking for a frame.
#[test]
fn a_colour_signal_repaints_the_field() {
    let red = Color::new(0.8, 0.0, 0.0, 1.0);
    let blue = Color::new(0.0, 0.0, 0.8, 1.0);
    let color = Signal::new(red);
    let mut field =
        Field::new(TextInputField::new(Signal::new("abc".into())).text_color(color.clone()));
    assert!(
        glyph_colors(&field.render())
            .iter()
            .all(|c| *c == red.to_array())
    );
    color.set(blue);
    let colors = glyph_colors(&field.render());
    assert!(!colors.is_empty());
    assert!(colors.iter().all(|c| *c == blue.to_array()), "{colors:?}");
}

/// A disabled field greys out whatever colour the app chose.
#[test]
fn a_disabled_field_keeps_the_disabled_colour() {
    let theme = teksilo_core::presets::intui::light();
    let red = Color::new(0.8, 0.0, 0.0, 1.0);
    let mut field = Field::new(
        TextInputField::new(Signal::new("abc".into()))
            .text_color(red)
            .enabled(false),
    );
    let colors = glyph_colors(&field.render());
    assert!(!colors.is_empty());
    assert!(
        colors
            .iter()
            .all(|c| *c == theme.colors.text_disabled.to_array())
    );
}
