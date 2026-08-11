// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Mount every widget the Fluent preset restyles, in both appearances, and
//! render it.
//!
//! The unit tests next to each style check its *numbers*. This checks that
//! the styles actually run — which is a different failure mode, and the one
//! that bites. A Tier-3 style is only exercised when its widget is built,
//! and `make_body` reaches for framework surfaces (`BuildContext::effect`,
//! `animated_signal`, the binding registry) whose preconditions are not
//! visible in the trait signature. `Signal::observe`, for instance, panics
//! on a *derived* signal, and several `*StyleConfig`s hand out nothing but
//! derived signals — a style that installs an effect on one compiles
//! cleanly, passes every unit test, and then takes the app down on the first
//! frame. That is exactly how the radio's dot animation was caught.
//!
//! Rendering, not just laying out, matters too: `paint` is where the styles
//! read the `FluentPalette` extension and the theme roles.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{MockTextBackend, SizeProposal};
use teksilo_core::signal::Signal;
use teksilo_core::styles::Theme;
use teksilo_core::widget::Widget;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;
use teksilo_widgets::{
    Badge, Button, ButtonVariant, Card, Checkbox, ComboBox, IconButton, Link, MenuItem, Panel,
    ProgressBar, RadioButton, SegmentedControl, Slider, StandardListItem, TextInput, Toggle,
    VStack,
};

/// Build a tree under `theme`, mount `w`, lay it out and render it.
///
/// Returns the number of draw commands, so a caller can assert the widget
/// actually put something on screen rather than silently painting nothing.
/// A text backend is installed because several Fluent surfaces are
/// deliberately transparent at rest — a `SubtleButton`, a menu row, an
/// unselected list row — and their glyphs are the only thing they draw.
fn mount(theme: Theme, w: impl Widget + 'static) -> usize {
    let mut tree = WidgetTree::new()
        .with_theme(theme)
        .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
    tree.add(w);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.render().draw_order.len()
}

fn themes() -> [Theme; 2] {
    [teksilo_theme_fluent::light(), teksilo_theme_fluent::dark()]
}

#[test]
fn buttons_mount_in_every_variant() {
    for theme in themes() {
        for variant in [
            ButtonVariant::Filled,
            ButtonVariant::Tinted,
            ButtonVariant::Outlined,
            ButtonVariant::Plain,
            ButtonVariant::Ghost,
            ButtonVariant::Link,
            ButtonVariant::Destructive,
        ] {
            let drawn = mount(theme.clone(), Button::new(lit!("Save")).variant(variant));
            assert!(drawn > 0, "{variant:?} painted nothing");
        }
    }
}

#[test]
fn toggle_mounts() {
    for theme in themes() {
        for on in [false, true] {
            assert!(mount(theme.clone(), Toggle::new(Signal::new(on))) > 0);
        }
    }
}

#[test]
fn checkbox_mounts() {
    for theme in themes() {
        for checked in [false, true] {
            assert!(
                mount(
                    theme.clone(),
                    Checkbox::new(Signal::new(checked)).label(lit!("Enable"))
                ) > 0
            );
        }
    }
}

/// The regression guard: every `RadioStyleConfig` signal is derived, so a
/// style that observes one panics the moment a radio is built.
#[test]
fn radio_mounts() {
    for theme in themes() {
        let selected = Signal::new(0_usize);
        assert!(
            mount(
                theme.clone(),
                VStack::new()
                    .child(RadioButton::new(0, selected.clone()).label(lit!("One")))
                    .child(RadioButton::new(1, selected.clone()).label(lit!("Two"))),
            ) > 0
        );
    }
}

#[test]
fn text_input_mounts() {
    for theme in themes() {
        assert!(
            mount(
                theme.clone(),
                TextInput::new(Signal::new("hello".to_string())).placeholder(lit!("Name"))
            ) > 0
        );
    }
}

#[test]
fn slider_mounts() {
    for theme in themes() {
        assert!(mount(theme.clone(), Slider::new(Signal::new(0.4), 0.0, 1.0)) > 0);
    }
}

#[test]
fn menu_item_mounts() {
    for theme in themes() {
        assert!(mount(theme.clone(), MenuItem::new(lit!("Open…"))) > 0);
    }
}

#[test]
fn standard_list_item_mounts() {
    for theme in themes() {
        assert!(mount(theme.clone(), StandardListItem::new(lit!("Row"))) > 0);
    }
}

/// The widgets whose Fluent styling is a re-dimensioned `Recipe*Style`
/// rather than a bespoke impl. Cheaper to break than the custom ones — a
/// recipe field that stops existing is a compile error — but a recipe with
/// a nonsensical dimension can still make a widget paint nothing.
#[test]
fn recipe_styled_widgets_mount() {
    for theme in themes() {
        for (name, drawn) in [
            ("card", mount(theme.clone(), Card::new().content(FixedLeaf))),
            ("panel", mount(theme.clone(), Panel::new().child(FixedLeaf))),
            (
                "combo_box",
                mount(
                    theme.clone(),
                    ComboBox::new(["A", "B"], Signal::new(Some("A".to_string()))),
                ),
            ),
            ("icon_button", mount(theme.clone(), IconButton::expand())),
            ("link", mount(theme.clone(), Link::new(lit!("Docs")))),
            ("badge", mount(theme.clone(), Badge::new(lit!("9")))),
            ("progress_bar", mount(theme.clone(), ProgressBar::new(0.5))),
            (
                "segmented_control",
                mount(
                    theme.clone(),
                    SegmentedControl::indexed(Signal::new(0_usize))
                        .segment(lit!("One"))
                        .segment(lit!("Two")),
                ),
            ),
        ] {
            assert!(drawn > 0, "{name} painted nothing");
        }
    }
}

/// A list row must fill the width it is given, not shrink to its content.
///
/// The Fluent style stacks a selection pill over the delegated row. Doing
/// that with a `ZStack` looks right and is silently wrong: `ZStack`
/// measures its children at an *unspecified* width by design, and the
/// delegated row's content sits behind a zero-basis `Expand`, so it reports
/// only its padding as its natural width — a 24 dp row centred inside a
/// 1200 dp list, with its label spilling out of it. It took a screenshot to
/// notice; this notices in 8 ms.
///
/// The label's leading edge is the tell: laid out correctly it sits
/// `padding_horizontal` from the row's left (the selection rect's own
/// `bg_horizontal_inset` is a separate, narrower inset), and if the row has
/// collapsed it is somewhere near the middle instead.
#[test]
fn a_list_row_fills_the_width_it_is_given() {
    const WIDTH: f32 = 600.0;
    let recipe = teksilo_theme_fluent::styles::standard_item::fluent_standard_item_recipe();
    let expected_x = recipe.padding_horizontal;

    for theme in themes() {
        let mut tree = WidgetTree::new()
            .with_theme(theme)
            .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
        tree.add(StandardListItem::new(lit!("A row label")));
        tree.layout(SizeProposal::exact(WIDTH, 60.0));

        let frame = tree.render();
        let first_glyph_x = frame
            .glyphs
            .iter()
            .map(|g| g.screen[0])
            .fold(f32::INFINITY, f32::min);
        assert!(
            first_glyph_x.is_finite(),
            "the row painted no text at all — it did not lay out"
        );
        assert!(
            (first_glyph_x - expected_x).abs() < 2.0,
            "label starts at x={first_glyph_x}, expected ~{expected_x}: the row \
             collapsed to its natural width instead of filling {WIDTH}"
        );
        // Belt and braces: a collapsed row centres its content, so the label
        // would land near the middle whatever the padding happened to be.
        assert!(
            first_glyph_x < WIDTH * 0.25,
            "label is near the centre — the row is not filling its width"
        );
    }
}

/// A `TextInputStyle` must not add vertical padding of its own.
///
/// `TextInput` has **already** wrapped the editor in its own vertical inset
/// by the time a style sees it — "wrapped in vertical padding so slots
/// (IconButton etc.) sit flush against top/bottom of the inner border area",
/// per the widget — which is exactly why the shipped `RecipeTextInputStyle`
/// pads *horizontally only*, with a comment saying so. Transcribing WinUI's
/// `TextControlThemePadding` (`10,5,6,6`) wholesale looks like fidelity and
/// is not: it double-applies the inset and the field grows 8 dp past the
/// control height, so it no longer lines up with anything beside it.
///
/// Comparing the two styles under the *same* theme isolates that: same
/// typography, same widget, same editor. The baseline is height-matched to
/// Fluent's 32 dp so the only thing left that can differ is the chrome's own
/// vertical inset.
///
/// (A `TextInput`'s reported height runs 4 dp above its frame in every
/// theme — `TEXT_FIELD_VALIDATION_STRIP_GAP`, the `VStack` gap reserved for
/// the inline validation strip even while it is Pristine and zero-height.
/// It cancels out of this comparison.)
#[test]
fn the_field_chrome_adds_no_vertical_inset() {
    use teksilo_widgets::styles::{RecipeTextInputStyle, TextInputRecipe};

    for theme in themes() {
        let height_of = |w: Box<dyn Widget>| {
            let mut tree = WidgetTree::new()
                .with_theme(theme.clone())
                .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
            let id = tree.add_boxed(w);
            tree.layout(SizeProposal::unspecified());
            tree.bounds(id).height
        };

        // The theme's own slot-installed Fluent chrome…
        let fluent = height_of(Box::new(TextInput::new(Signal::new(String::new()))));
        // …versus the shipped chrome at the same 32 dp floor, which is known
        // to add no vertical inset of its own.
        let baseline = height_of(Box::new(TextInput::new(Signal::new(String::new())).style(
            RecipeTextInputStyle::new(TextInputRecipe {
                height: 32.0,
                ..TextInputRecipe::default()
            }),
        )));

        assert!(
            (fluent - baseline).abs() < 0.5,
            "the Fluent field is {fluent} dp where the shipped chrome at the \
             same height gives {baseline} dp — the style is adding {} dp of \
             vertical inset the widget had already applied",
            fluent - baseline,
        );
    }
}

/// The 32 dp control height still has to be a real floor: a Fluent field is
/// never *shorter* than the button beside it.
#[test]
fn a_text_field_is_at_least_the_control_height() {
    for theme in themes() {
        let height_of = |w: Box<dyn Widget>| {
            let mut tree = WidgetTree::new()
                .with_theme(theme.clone())
                .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
            let id = tree.add_boxed(w);
            tree.layout(SizeProposal::unspecified());
            tree.bounds(id).height
        };

        let button = height_of(Box::new(Button::new(lit!("Save"))));
        let field = height_of(Box::new(TextInput::new(Signal::new(String::new()))));

        assert!(
            (button - 32.0).abs() < 0.5,
            "button is {button}, expected the 32 dp Fluent control height"
        );
        assert!(
            field >= button - 0.5,
            "text field {field} is shorter than the button {button}"
        );
        // Teksilo sizes a field from its editor's line box, so it runs a
        // little taller than a button under every preset (IntUI: 32 vs 24).
        // What must not happen is the field drifting *further* than that
        // intrinsic difference — which is what a double-applied inset does.
        assert!(
            field - button <= 8.0,
            "text field {field} is {} dp taller than the button — beyond the \
             framework's own intrinsic difference",
            field - button
        );
    }
}

/// Every shape colour a render pass emitted, rounded to 8-bit so a literal
/// token can be compared against it.
fn shape_colors(frame: &teksilo_canvas::RenderFrame) -> Vec<[u8; 4]> {
    frame
        .shapes
        .iter()
        .map(|s| {
            let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            [q(s.color[0]), q(s.color[1]), q(s.color[2]), q(s.color[3])]
        })
        .collect()
}

fn rgba8(c: teksilo_tokens::Color) -> [u8; 4] {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    [q(c.r()), q(c.g()), q(c.b()), q(c.a())]
}

/// The chrome must paint the exact WinUI token, not something near it.
///
/// Screenshots are the wrong instrument for this — a capture goes through
/// compositing, colour management and the window's own backdrop, so
/// "close to `#0067C0`" is all one can read off them. The render frame
/// carries the colour the style actually asked for.
#[test]
fn an_accent_button_paints_the_exact_accent_fill() {
    for theme in themes() {
        let expected = rgba8(theme.colors.accent);
        let mut tree = WidgetTree::new()
            .with_theme(theme)
            .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
        tree.add(Button::new(lit!("Save")).variant(ButtonVariant::Filled));
        tree.layout(SizeProposal::exact(200.0, 60.0));
        let colors = shape_colors(&tree.render());
        assert!(
            colors.contains(&expected),
            "no shape painted the accent fill {expected:?}; painted {colors:?}"
        );
    }
}

/// The elevation edge is the preset's signature, and it is a *relationship*
/// between two strokes: the emphasised one has to be heavier than the plain
/// hairline, and it has to be on the bottom in light and the top in dark.
#[test]
fn the_elevation_edge_is_heavier_than_the_hairline_and_flips_with_appearance() {
    use teksilo_theme_fluent::{FluentEdgeSide, FluentPalette};

    for (theme, expected_side) in [
        (teksilo_theme_fluent::light(), FluentEdgeSide::Bottom),
        (teksilo_theme_fluent::dark(), FluentEdgeSide::Top),
    ] {
        let p = *theme.extension::<FluentPalette>().expect("palette");
        assert_eq!(p.elevation_edge, expected_side);
        // `ControlStrokeColorSecondary` carries the emphasis, so it must be
        // the more opaque of the two.
        assert!(
            p.control_stroke_secondary.a() > p.control_stroke_default.a(),
            "the elevation edge is not heavier than the hairline"
        );

        let hairline = rgba8(p.control_stroke_default);
        let edge = rgba8(p.control_stroke_secondary);
        let mut tree = WidgetTree::new()
            .with_theme(theme)
            .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
        tree.add(Button::new(lit!("Save")));
        tree.layout(SizeProposal::exact(200.0, 60.0));
        let colors = shape_colors(&tree.render());
        assert!(colors.contains(&hairline), "no hairline painted");
        assert!(colors.contains(&edge), "no elevation edge painted");
    }
}

/// A minimal leaf so the container tests have content without dragging in
/// another widget's own styling.
#[derive(Debug)]
struct FixedLeaf;

impl Widget for FixedLeaf {
    fn layout_response(
        &self,
        _proposal: SizeProposal,
        _ctx: &teksilo_core::widget::LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        teksilo_canvas::Size::new(40.0, 20.0).into()
    }
}

/// A style installed in a slot must survive a live `set_theme` swap — the
/// switcher in the widget catalog does exactly this, and a style that baked
/// a colour at build time would keep painting the old palette.
#[test]
fn a_mounted_tree_survives_a_theme_swap() {
    let mut tree = WidgetTree::new().with_theme(teksilo_theme_fluent::light());
    tree.add(
        VStack::new()
            .child(Button::new(lit!("Save")).variant(ButtonVariant::Filled))
            .child(Toggle::new(Signal::new(true)))
            .child(Checkbox::new(Signal::new(true)).label(lit!("On")))
            .child(Slider::new(Signal::new(0.5), 0.0, 1.0)),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let light = tree.render().draw_order.len();
    assert!(light > 0);

    tree.set_theme(teksilo_theme_fluent::dark());
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert!(!tree.render().draw_order.is_empty());

    // …and back to a different design language entirely.
    tree.set_theme(teksilo_core::presets::intui::light());
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert!(!tree.render().draw_order.is_empty());
}

/// The custom styles read `FluentPalette` through
/// `Theme::extension`, with a role fallback for every value. Mounting them
/// under a theme that has no such extension must not panic.
#[test]
fn custom_styles_degrade_without_the_palette_extension() {
    let mut bare = teksilo_theme_fluent::light();
    bare.extensions
        .remove::<teksilo_theme_fluent::FluentPalette>();
    assert!(
        bare.extension::<teksilo_theme_fluent::FluentPalette>()
            .is_none()
    );

    assert!(mount(bare.clone(), Button::new(lit!("Save"))) > 0);
    assert!(mount(bare.clone(), Toggle::new(Signal::new(true))) > 0);
    assert!(mount(bare.clone(), Checkbox::new(Signal::new(true))) > 0);
    assert!(mount(bare.clone(), RadioButton::new(0, Signal::new(0_usize))) > 0);
    assert!(mount(bare.clone(), Slider::new(Signal::new(0.5), 0.0, 1.0)) > 0);
    assert!(mount(bare, TextInput::new(Signal::new(String::new()))) > 0);
}
