// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Mount every widget the macOS preset restyles, in both appearances, and
//! render it.
//!
//! The unit tests next to each style check its *numbers*. This checks that
//! the styles actually run — a different failure mode, and the one that
//! bites. A Tier-3 style is only exercised when its widget is built, and
//! `make_body` reaches for framework surfaces (`BuildContext::effect`,
//! `animated_signal`, the binding registry) whose preconditions are not
//! visible in the trait signature. `Signal::observe`, for instance, panics
//! on a *derived* signal, and several `*StyleConfig`s hand out nothing but
//! derived signals — a style that installs an effect on one compiles
//! cleanly, passes every unit test, and then takes the app down on the
//! first frame.
//!
//! Rendering, not just laying out, matters too: `paint` is where the
//! styles read the `MacOsPalette` extension, the theme roles, and the
//! gradient paint path.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{MockTextBackend, RenderFrame, SizeProposal};
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
/// A text backend is installed because several macOS surfaces are
/// deliberately transparent at rest — a borderless button, an unselected
/// row — and their glyphs are the only thing they draw.
fn mount(theme: Theme, w: impl Widget + 'static) -> usize {
    let mut tree = WidgetTree::new()
        .with_theme(theme)
        .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
    tree.add(w);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.render().draw_order.len()
}

/// Render `w` under `theme` at `size` and hand back the whole frame, so a
/// test can inspect the exact colours and paints the styles asked for.
fn frame_of(theme: Theme, w: impl Widget + 'static, width: f32, height: f32) -> Rc<RenderFrame> {
    let mut tree = WidgetTree::new()
        .with_theme(theme)
        .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
    tree.add(w);
    tree.layout(SizeProposal::exact(width, height));
    tree.render()
}

fn themes() -> [Theme; 2] {
    [teksilo_theme_macos::light(), teksilo_theme_macos::dark()]
}

fn rgba8(c: teksilo_tokens::Color) -> [u8; 4] {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    [q(c.r()), q(c.g()), q(c.b()), q(c.a())]
}

/// Every shape colour a render pass emitted, rounded to 8-bit so a literal
/// token can be compared against it.
fn shape_colors(frame: &RenderFrame) -> Vec<[u8; 4]> {
    frame
        .shapes
        .iter()
        .map(|s| {
            let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            [q(s.color[0]), q(s.color[1]), q(s.color[2]), q(s.color[3])]
        })
        .collect()
}

/// Every glyph colour a render pass emitted.
fn glyph_colors(frame: &RenderFrame) -> Vec<[u8; 4]> {
    frame
        .glyphs
        .iter()
        .map(|g| {
            let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            [q(g.color[0]), q(g.color[1]), q(g.color[2]), q(g.color[3])]
        })
        .collect()
}

// ── Every restyled widget mounts and paints ─────────────────────────────

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
        // …including the two-line form, which takes a different height
        // branch and a second text role.
        assert!(
            mount(
                theme.clone(),
                StandardListItem::new(lit!("Row")).subtitle(lit!("Detail"))
            ) > 0
        );
    }
}

/// The widgets whose macOS styling is a re-dimensioned `Recipe*Style`
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

/// The rest of the recipe-styled slots. Less likely to break than the
/// bespoke styles — a recipe field that stops existing is a compile error
/// — but a *nonsensical* dimension still compiles and can make a widget
/// paint nothing, and none of these widgets appear above.
#[test]
fn the_remaining_recipe_styled_widgets_mount() {
    use teksilo_widgets::{Avatar, Calendar, SearchField, SpinBox};

    for theme in themes() {
        for (name, drawn) in [
            (
                "calendar",
                mount(theme.clone(), Calendar::single(Signal::new(None))),
            ),
            (
                "search_field",
                mount(theme.clone(), SearchField::new(Signal::new(String::new()))),
            ),
            (
                "spin_box",
                mount(theme.clone(), SpinBox::new(Signal::new(3_i32), 0, 10)),
            ),
            (
                "avatar",
                mount(theme.clone(), Avatar::with_name(lit!("Jane Doe"))),
            ),
        ] {
            assert!(drawn > 0, "{name} painted nothing");
        }
    }
}

// ── The signature macOS details, checked in the render frame ────────────

/// The chrome must paint the exact AppKit token, not something near it.
///
/// Screenshots are the wrong instrument for this — a capture goes through
/// compositing, colour management and the window's own backdrop, so "close
/// to `#0063E1`" is all one can read off them. The render frame carries the
/// colour the style actually asked for.
#[test]
fn the_default_button_paints_the_exact_accent_fill() {
    for theme in themes() {
        let expected = rgba8(theme.colors.accent);
        let frame = frame_of(
            theme,
            Button::new(lit!("Save")).variant(ButtonVariant::Filled),
            200.0,
            60.0,
        );
        let colors = shape_colors(&frame);
        assert!(
            colors.contains(&expected),
            "no shape painted the accent fill {expected:?}; painted {colors:?}"
        );
    }
}

/// The bezel is this preset's signature, and it is a *gradient*, not a
/// flat fill — the one thing that makes a macOS control read as a physical
/// object. A style that quietly fell back to a solid face would still look
/// plausible in a screenshot and would still pass every dimension test.
#[test]
fn an_ordinary_button_paints_a_graded_face() {
    use teksilo_canvas::PaintData;

    for theme in themes() {
        let frame = frame_of(theme.clone(), Button::new(lit!("Save")), 200.0, 60.0);
        let gradients: Vec<_> = frame
            .shapes
            .iter()
            .filter_map(|s| match &s.paint_data {
                PaintData::LinearGradient { start, end, stops } => {
                    Some((*start, *end, stops.clone()))
                }
                _ => None,
            })
            .collect();
        assert!(
            !gradients.is_empty(),
            "the bezel painted no gradient at all"
        );
        let (start, end, stops) = &gradients[0];
        assert_eq!(start[0], end[0], "the face gradient must be vertical");
        assert!(end[1] > start[1], "…and run top to bottom");
        assert_eq!(stops.len(), 2);
        assert!(
            stops[0].color.relative_luminance() > stops[1].color.relative_luminance(),
            "a macOS face is lighter at the top"
        );
    }
}

/// …and the *default* button must not get one. Doubling the elevation cues
/// (accent fill plus a bezel) is the classic way to make a macOS default
/// button look wrong.
#[test]
fn the_default_button_is_flat_not_bezelled() {
    use teksilo_canvas::PaintData;

    for theme in themes() {
        let frame = frame_of(
            theme,
            Button::new(lit!("Save")).variant(ButtonVariant::Filled),
            200.0,
            60.0,
        );
        assert!(
            frame
                .shapes
                .iter()
                .all(|s| matches!(s.paint_data, PaintData::Solid)),
            "the accent-filled button grew a bezel gradient"
        );
    }
}

/// A macOS control casts a small shadow. It is the third layer of the
/// bezel and the easiest to lose in a refactor, because nothing else
/// changes visibly when it goes.
#[test]
fn a_bezelled_control_casts_a_shadow() {
    for theme in themes() {
        let frame = frame_of(theme, Button::new(lit!("Save")), 200.0, 60.0);
        assert!(
            !frame.shadows.is_empty(),
            "the bezel cast no shadow — the control will read as flat"
        );
    }
}

/// The selection capsule and its white label are one mechanism: the fill
/// is only legible because [`StandardItemStyle::selected_label_role`]
/// flips the text with it. Both halves are checked in the same frame, so
/// a change that keeps one and drops the other fails here.
#[test]
fn a_selected_row_paints_an_accent_capsule_with_an_on_accent_label() {
    for theme in themes() {
        let accent = rgba8(theme.colors.accent);
        let on_accent = rgba8(theme.colors.text_on_accent);
        let frame = frame_of(
            theme,
            StandardListItem::new(lit!("Row")).selected(Signal::new(true)),
            300.0,
            40.0,
        );
        assert!(
            shape_colors(&frame).contains(&accent),
            "the selected row painted no accent capsule"
        );
        assert!(
            glyph_colors(&frame).contains(&on_accent),
            "the selected row's label did not flip to the on-accent role"
        );
    }
}

/// …and an *unselected* row must not, or every row in the list would read
/// as chosen.
#[test]
fn an_unselected_row_paints_no_capsule() {
    for theme in themes() {
        let accent = rgba8(theme.colors.accent);
        let frame = frame_of(theme, StandardListItem::new(lit!("Row")), 300.0, 40.0);
        assert!(
            !shape_colors(&frame).contains(&accent),
            "an unselected row painted the selection capsule"
        );
    }
}

/// A list row must fill the width it is given, not shrink to its content.
///
/// The macOS style stacks a selection capsule under the delegated row.
/// Doing that with a `ZStack` looks right and is silently wrong: `ZStack`
/// measures its children at an *unspecified* width by design, and the
/// delegated row's content sits behind a zero-basis `Expand`, so it
/// reports only its padding as its natural width — a 20 dp row centred
/// inside a 1200 dp list, with its label spilling out of it. It took a
/// screenshot to notice in the Fluent preset; this notices in 8 ms.
///
/// The label's leading edge is the tell: laid out correctly it sits
/// `padding_horizontal` from the row's left (the capsule's own
/// `bg_horizontal_inset` is a separate, narrower inset that moves only the
/// background), and if the row has collapsed it is somewhere near the
/// middle instead.
#[test]
fn a_list_row_fills_the_width_it_is_given() {
    const WIDTH: f32 = 600.0;
    let recipe = teksilo_theme_macos::styles::standard_item::macos_standard_item_recipe();
    let expected_x = recipe.padding_horizontal;

    for theme in themes() {
        let frame = frame_of(
            theme,
            StandardListItem::new(lit!("A row label")),
            WIDTH,
            60.0,
        );
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
        // Belt and braces: a collapsed row centres its content, so the
        // label would land near the middle whatever the padding was.
        assert!(
            first_glyph_x < WIDTH * 0.25,
            "label is near the centre — the row is not filling its width"
        );
    }
}

/// A highlighted menu row fills with the accent and flips its label —
/// the loudest single difference from the Fluent and IntUI menus, and the
/// second consumer of the label-role hook.
#[test]
fn the_menu_row_declares_an_on_accent_highlight() {
    use teksilo_core::styles::MenuItemStyle;
    use teksilo_tokens::TextRole;

    let style = teksilo_theme_macos::styles::menu_item::MacOsMenuItemStyle;
    assert_eq!(style.highlighted_label_role(), Some(TextRole::OnAccent));
    // …and the theme actually installs that style rather than a recipe.
    for theme in themes() {
        assert!(theme.style_slots.menu_item.is_some());
    }
}

// ── Field geometry ──────────────────────────────────────────────────────

/// A `TextInputStyle` must not add vertical padding of its own.
///
/// `TextInput` has **already** wrapped the editor in its own vertical
/// inset by the time a style sees it — "wrapped in vertical padding so
/// slots (IconButton etc.) sit flush against top/bottom of the inner
/// border area", per the widget — which is exactly why the shipped
/// `RecipeTextInputStyle` pads *horizontally only*, with a comment saying
/// so. Transcribing an AppKit field's full inset wholesale looks like
/// fidelity and is not: it double-applies and the field grows past the
/// control height, so it no longer lines up with anything beside it.
///
/// Comparing the two styles under the *same* theme isolates that: same
/// typography, same widget, same editor. The baseline is height-matched to
/// the macOS control height so the only thing left that can differ is the
/// chrome's own vertical inset.
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

        // The theme's own slot-installed macOS chrome…
        let macos = height_of(Box::new(TextInput::new(Signal::new(String::new()))));
        // …versus the shipped chrome at the same height floor, which is
        // known to add no vertical inset of its own.
        let baseline = height_of(Box::new(TextInput::new(Signal::new(String::new())).style(
            RecipeTextInputStyle::new(TextInputRecipe {
                height: teksilo_theme_macos::shape::MACOS_CONTROL_HEIGHT,
                ..TextInputRecipe::default()
            }),
        )));

        assert!(
            (macos - baseline).abs() < 0.5,
            "the macOS field is {macos} dp where the shipped chrome at the \
             same height gives {baseline} dp — the style is adding {} dp of \
             vertical inset the widget had already applied",
            macos - baseline,
        );
    }
}

/// The control height still has to be a real floor: a macOS field is never
/// *shorter* than the button beside it.
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
        let expected = teksilo_theme_macos::shape::MACOS_CONTROL_HEIGHT;

        assert!(
            (button - expected).abs() < 0.5,
            "button is {button}, expected the {expected} dp macOS control height"
        );
        assert!(
            field >= button - 0.5,
            "text field {field} is shorter than the button {button}"
        );
        // It *is* taller, and that is the framework's doing rather than
        // the preset's: `TextInput` sizes itself from its editor's line
        // box plus its own vertical inset plus the reserved validation
        // strip, none of which a style can shorten, so the 22 dp floor
        // never bites. The gap therefore widens as the control height
        // falls — under IntUI's 24 dp button it is 8 dp, under this
        // preset's 22 dp button it is 10 — and bounding it against the
        // button would be measuring the wrong thing. What actually has to
        // hold is that the *chrome* adds nothing on top, which
        // `the_field_chrome_adds_no_vertical_inset` pins directly.
        assert!(
            field > button,
            "the field is no longer taller than the button — the framework's \
             intrinsic field height may have changed, and the sibling test's \
             baseline comparison should be re-read"
        );
    }
}

/// The whole point of the 22 dp control height: a macOS row of controls is
/// visibly denser than the same row under Fluent.
#[test]
fn a_row_of_controls_is_denser_than_the_fluent_equivalent() {
    let height_of = |theme: Theme, w: Box<dyn Widget>| {
        let mut tree = WidgetTree::new()
            .with_theme(theme)
            .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
        let id = tree.add_boxed(w);
        tree.layout(SizeProposal::unspecified());
        tree.bounds(id).height
    };

    let macos = height_of(
        teksilo_theme_macos::light(),
        Box::new(Button::new(lit!("Save"))),
    );
    let intui = height_of(
        teksilo_core::presets::intui::light(),
        Box::new(Button::new(lit!("Save"))),
    );
    // IntUI's button is 24 dp; macOS's is 22. Not a large gap, but it must
    // be on the right side of it — a preset that drifted to Fluent's 32
    // would fail loudly here.
    assert!(
        macos <= intui,
        "the macOS button ({macos}) is taller than IntUI's ({intui})"
    );
    assert!(macos < 32.0, "…and it must stay well under Fluent's 32 dp");
}

// ── Robustness ──────────────────────────────────────────────────────────

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
/// switcher in the widget catalog does exactly this, and a style that
/// baked a colour at build time would keep painting the old palette.
#[test]
fn a_mounted_tree_survives_a_theme_swap() {
    let mut tree = WidgetTree::new()
        .with_theme(teksilo_theme_macos::light())
        .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
    tree.add(
        VStack::new()
            .child(Button::new(lit!("Save")).variant(ButtonVariant::Filled))
            .child(Toggle::new(Signal::new(true)))
            .child(Checkbox::new(Signal::new(true)).label(lit!("On")))
            .child(Slider::new(Signal::new(0.5), 0.0, 1.0))
            .child(StandardListItem::new(lit!("Row"))),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert!(!tree.render().draw_order.is_empty());

    tree.set_theme(teksilo_theme_macos::dark());
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert!(!tree.render().draw_order.is_empty());

    // …and out to a different design language entirely.
    tree.set_theme(teksilo_core::presets::intui::light());
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert!(!tree.render().draw_order.is_empty());
}

/// The custom styles read `MacOsPalette` through `Theme::extension`, with
/// a role fallback for every value. Mounting them under a theme that has
/// no such extension must not panic.
#[test]
fn custom_styles_degrade_without_the_palette_extension() {
    let mut bare = teksilo_theme_macos::light();
    bare.extensions
        .remove::<teksilo_theme_macos::MacOsPalette>();
    assert!(
        bare.extension::<teksilo_theme_macos::MacOsPalette>()
            .is_none()
    );

    assert!(mount(bare.clone(), Button::new(lit!("Save"))) > 0);
    assert!(mount(bare.clone(), Toggle::new(Signal::new(true))) > 0);
    assert!(mount(bare.clone(), Checkbox::new(Signal::new(true))) > 0);
    assert!(mount(bare.clone(), RadioButton::new(0, Signal::new(0_usize))) > 0);
    assert!(mount(bare.clone(), Slider::new(Signal::new(0.5), 0.0, 1.0)) > 0);
    assert!(mount(bare.clone(), TextInput::new(Signal::new(String::new()))) > 0);
    assert!(mount(bare.clone(), MenuItem::new(lit!("Open…"))) > 0);
    assert!(mount(bare, StandardListItem::new(lit!("Row"))) > 0);
}

/// Every one of the eight System Settings accents has to produce a tree
/// that builds and paints — including Yellow and Graphite, whose fills are
/// darkened further than the others so their labels stay readable.
#[test]
fn every_system_accent_mounts() {
    use teksilo_theme_macos::SystemAccent;

    for accent in SystemAccent::ALL {
        for theme in [
            teksilo_theme_macos::light_with_accent(accent.light()),
            teksilo_theme_macos::dark_with_accent(accent.dark()),
        ] {
            let drawn = mount(
                theme,
                VStack::new()
                    .child(Button::new(lit!("Save")).variant(ButtonVariant::Filled))
                    .child(Toggle::new(Signal::new(true)))
                    .child(StandardListItem::new(lit!("Row")).selected(Signal::new(true))),
            );
            assert!(drawn > 0, "{accent:?} painted nothing");
        }
    }
}
