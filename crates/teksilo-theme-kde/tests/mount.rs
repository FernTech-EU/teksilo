// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Mount every widget the Breeze preset restyles, in both schemes, render
//! it, and check the colours it paints against Plasma's own: the buttons,
//! the check box, the radio button, the switch and the line edit.
//!
//! The unit tests beside each style check its rules. This checks that the
//! styles run: a Tier-3 style is only exercised when its widget is built,
//! and its `make_body` and `paint` reach framework surfaces (derived
//! signals, the binding registry, the paint context's window state) whose
//! preconditions the trait signature does not show.
//!
//! The exact-colour checks read the colours from the render frame, which
//! carries what the style asked for, and compare them with the golden
//! samples of Plasma 6.8 painting the same controls
//! (`tools/kde_golden/vectors/pixels.json`). This file reads that file from
//! the repository, so it is left out of the published package (the
//! `exclude` list in `Cargo.toml`).

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use serde_json::Value;
use teksilo_canvas::{MockTextBackend, RenderFrame, ShapeKind, SizeProposal};
use teksilo_core::event::{Key, Modifiers, PointerButton};
use teksilo_core::signal::Signal;
use teksilo_core::styles::Theme;
use teksilo_core::widget::Widget;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;
use teksilo_theme_kde::styles::indicator::BreezeIndicatorColors;
use teksilo_theme_kde::{self as kde, KdePalette, PlasmaAccent};
use teksilo_tokens::Color;
use teksilo_widgets::primitives::{HStack, IconWidget};
use teksilo_widgets::{
    Button, ButtonVariant, CheckState, Checkbox, IconButton, IconButtonSize, MenuItem, RadioButton,
    SplitButton, TextInput, Toggle, VStack,
};

const PIXELS_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/kde_golden/vectors/pixels.json"
));

const VARIANTS: [ButtonVariant; 7] = [
    ButtonVariant::Filled,
    ButtonVariant::Tinted,
    ButtonVariant::Outlined,
    ButtonVariant::Plain,
    ButtonVariant::Ghost,
    ButtonVariant::Link,
    ButtonVariant::Destructive,
];

const SIZES: [IconButtonSize; 5] = [
    IconButtonSize::Compact,
    IconButtonSize::Default,
    IconButtonSize::Toolbar,
    IconButtonSize::Large,
    IconButtonSize::Hero,
];

fn themes() -> [(&'static str, Theme); 2] {
    [("BreezeLight", kde::light()), ("BreezeDark", kde::dark())]
}

fn tree_with(theme: Theme) -> WidgetTree {
    WidgetTree::new()
        .with_theme(theme)
        .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())))
}

/// Mount `w` under `theme`, lay it out and render it: the number of draw
/// commands, so a caller can assert something was painted.
fn mount(theme: Theme, w: impl Widget + 'static) -> usize {
    let mut tree = tree_with(theme);
    tree.add(w);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.render().draw_order.len()
}

/// A tree holding `w` at its natural size in a row, laid out: the tree and
/// the widget's id.
fn placed(theme: Theme, w: impl Widget + 'static) -> (WidgetTree, WidgetId) {
    let mut tree = tree_with(theme);
    let row = tree.add(HStack::new().child(w));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let id = tree.children(row)[0];
    (tree, id)
}

/// A tree holding `w` at its natural height in a column, laid out: the tree
/// and the widget's id.
fn in_column(theme: Theme, w: impl Widget + 'static) -> (WidgetTree, WidgetId) {
    let mut tree = tree_with(theme);
    let column = tree.add(VStack::new().child(HStack::new().child(w)));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let row = tree.children(column)[0];
    let id = tree.children(row)[0];
    (tree, id)
}

fn rerender(tree: &mut WidgetTree) -> Rc<RenderFrame> {
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.render()
}

fn rgba8(c: [f32; 4]) -> [u8; 4] {
    c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// One painted shape: its colour in 8 bits, its stroke width (0 for a
/// fill), its rectangle, its kind and its top-leading corner radius.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Painted {
    color: [u8; 4],
    stroke: f32,
    screen: [f32; 4],
    kind: ShapeKind,
    radius: f32,
}

fn painted(frame: &RenderFrame) -> Vec<Painted> {
    frame
        .shapes
        .iter()
        .map(|s| Painted {
            color: rgba8(s.color),
            stroke: s.stroke_width,
            screen: s.screen,
            kind: s.shape,
            radius: s.corner_radii[0],
        })
        .collect()
}

fn fills(frame: &RenderFrame) -> Vec<[u8; 4]> {
    painted(frame)
        .into_iter()
        .filter(|p| p.stroke == 0.0)
        .map(|p| p.color)
        .collect()
}

fn strokes(frame: &RenderFrame) -> Vec<[u8; 4]> {
    painted(frame)
        .into_iter()
        .filter(|p| p.stroke > 0.0)
        .map(|p| p.color)
        .collect()
}

/// Black at 12.5 %, the button shadow, in 8 bits.
const SHADOW: [u8; 4] = [0, 0, 0, 32];

fn has_shadow(frame: &RenderFrame) -> bool {
    strokes(frame).contains(&SHADOW)
}

/// The colour Plasma painted at `(x, y)` of the control `id` in `scheme`.
fn sample(scheme: &str, id: &str, x: usize, y: usize) -> [u8; 4] {
    let all: Value = serde_json::from_str(PIXELS_JSON).expect("pixels.json parses");
    let record = all["schemes"][scheme]["records"]
        .as_array()
        .expect("the scheme's records")
        .iter()
        .find(|r| r["id"] == id)
        .unwrap_or_else(|| panic!("{scheme} has no {id}"));
    let (line, at) = if let Some(row) = record["rows"].get(y.to_string()) {
        (row, x)
    } else {
        (
            record["cols"]
                .get(x.to_string())
                .unwrap_or_else(|| panic!("{id} samples neither row {y} nor column {x}")),
            y,
        )
    };
    let run = line
        .as_array()
        .expect("a run-length line")
        .iter()
        .find(|run| {
            let start = run[0].as_u64().unwrap() as usize;
            let len = run[1].as_u64().unwrap() as usize;
            (start..start + len).contains(&at)
        })
        .expect("the line covers the pixel");
    let hex = run[2].as_str().unwrap().trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap();
    [byte(0), byte(2), byte(4), byte(6)]
}

/// `top` composited over the opaque `bottom`, in 8 bits.
fn over(top: [u8; 4], bottom: [u8; 4]) -> [u8; 4] {
    let a = f32::from(top[3]) / 255.0;
    let ch = |i: usize| (f32::from(top[i]) * a + f32::from(bottom[i]) * (1.0 - a)).round() as u8;
    [ch(0), ch(1), ch(2), 255]
}

/// Whether two colours are within one step a channel: Qt's raster painter
/// rounds an 8-bit blend its own way.
fn near(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 1)
}

// ── Every restyled widget mounts and paints ─────────────────────────────

#[test]
fn buttons_mount_in_every_variant() {
    for (_, theme) in themes() {
        for variant in VARIANTS {
            let drawn = mount(theme.clone(), Button::new(lit!("Save")).variant(variant));
            assert!(drawn > 0, "{variant:?} painted nothing");
        }
    }
}

#[test]
fn icon_buttons_mount_at_every_size() {
    for (_, theme) in themes() {
        for size in SIZES {
            let drawn = mount(
                theme.clone(),
                IconButton::new(IconWidget::checkmark(12.0)).size(size),
            );
            assert!(drawn > 0, "{size:?} painted nothing");
            for on in [false, true] {
                let drawn = mount(
                    theme.clone(),
                    IconButton::new(IconWidget::checkmark(12.0))
                        .size(size)
                        .toggle(Signal::new(on)),
                );
                assert!(drawn > 0, "a {size:?} toggle painted nothing");
            }
        }
    }
}

#[test]
fn split_buttons_mount_in_every_variant() {
    for (_, theme) in themes() {
        for variant in VARIANTS {
            let drawn = mount(
                theme.clone(),
                SplitButton::new()
                    .item(MenuItem::new(lit!("Run")))
                    .item(MenuItem::new(lit!("Debug")))
                    .variant(variant),
            );
            assert!(drawn > 0, "{variant:?} painted nothing");
        }
    }
}

#[test]
fn indicators_and_fields_mount_in_every_state_and_variant() {
    use teksilo_core::styles::{CheckboxVariant, RadioVariant, TextInputVariant};
    for (_, theme) in themes() {
        for on in [false, true] {
            assert!(
                mount(
                    theme.clone(),
                    Toggle::new(Signal::new(on)).label(lit!("Wi-Fi"))
                ) > 0
            );
            assert!(mount(theme.clone(), Toggle::new(Signal::new(on)).enabled(false)) > 0);
        }
        for state in [
            CheckState::Unchecked,
            CheckState::Checked,
            CheckState::Indeterminate,
        ] {
            for variant in [
                CheckboxVariant::Square,
                CheckboxVariant::Rounded,
                CheckboxVariant::Circle,
            ] {
                let drawn = mount(
                    theme.clone(),
                    Checkbox::tristate(Signal::new(state))
                        .variant(variant)
                        .label(lit!("Enable")),
                );
                assert!(drawn > 0, "{state:?} {variant:?} painted nothing");
            }
        }
        for variant in [
            RadioVariant::Circle,
            RadioVariant::Square,
            RadioVariant::Rounded,
        ] {
            let selected = Signal::new(0_usize);
            let drawn = mount(
                theme.clone(),
                VStack::new()
                    .child(
                        RadioButton::new(0, selected.clone())
                            .variant(variant)
                            .label(lit!("One")),
                    )
                    .child(
                        RadioButton::new(1, selected.clone())
                            .variant(variant)
                            .label(lit!("Two")),
                    ),
            );
            assert!(drawn > 0, "{variant:?} painted nothing");
        }
        for variant in [
            TextInputVariant::Outlined,
            TextInputVariant::Filled,
            TextInputVariant::Underline,
            TextInputVariant::Bare,
        ] {
            let drawn = mount(
                theme.clone(),
                TextInput::new(Signal::new("hello".to_string()))
                    .placeholder(lit!("Name"))
                    .variant(variant),
            );
            assert!(drawn > 0, "{variant:?} painted nothing");
        }
    }
}

/// The styles read the theme at paint time, so a tree built under Breeze
/// survives a live swap to the other scheme, out to another design language
/// and back.
#[test]
fn a_mounted_tree_survives_a_theme_swap() {
    let mut tree = tree_with(kde::light());
    tree.add(
        VStack::new()
            .child(Button::new(lit!("Save")).variant(ButtonVariant::Filled))
            .child(Button::new(lit!("Cancel")))
            .child(IconButton::new(IconWidget::checkmark(12.0)).toggle(Signal::new(true)))
            .child(SplitButton::new().item(MenuItem::new(lit!("Run"))))
            .child(Toggle::new(Signal::new(true)))
            .child(Checkbox::new(Signal::new(true)).label(lit!("On")))
            .child(RadioButton::new(0, Signal::new(0_usize)).label(lit!("One")))
            .child(TextInput::new(Signal::new("text".to_string()))),
    );
    for theme in [
        kde::light(),
        kde::dark(),
        teksilo_core::presets::intui::light(),
        kde::light(),
    ] {
        tree.set_theme(theme);
        let frame = rerender(&mut tree);
        assert!(!frame.draw_order.is_empty());
    }
}

/// The neutral button colours come from `KdePalette`; without it the styles
/// paint from the theme's roles rather than panicking or painting nothing.
#[test]
fn the_styles_degrade_without_the_palette() {
    let mut bare = kde::light();
    bare.extensions.remove::<KdePalette>();
    assert!(bare.extension::<KdePalette>().is_none());
    for variant in VARIANTS {
        assert!(mount(bare.clone(), Button::new(lit!("Save")).variant(variant)) > 0);
    }
    assert!(mount(bare.clone(), IconButton::new(IconWidget::checkmark(12.0))) > 0);
    assert!(
        mount(
            bare.clone(),
            SplitButton::new().item(MenuItem::new(lit!("Run")))
        ) > 0
    );
    assert!(mount(bare.clone(), Toggle::new(Signal::new(true))) > 0);
    assert!(mount(bare.clone(), Checkbox::new(Signal::new(true))) > 0);
    assert!(mount(bare.clone(), RadioButton::new(0, Signal::new(0_usize))) > 0);
    assert!(mount(bare.clone(), TextInput::new(Signal::new(String::new()))) > 0);
    // The push button still has a face and an outline, from the roles.
    let (mut tree, _) = placed(bare.clone(), Button::new(lit!("Save")));
    let frame = rerender(&mut tree);
    assert!(fills(&frame).contains(&rgba8(bare.colors.surface_content.to_array())));
    assert!(strokes(&frame).contains(&rgba8(bare.colors.border.to_array())));
}

#[test]
fn every_swatch_mounts() {
    for accent in PlasmaAccent::ALL {
        for theme in [
            kde::light_with_accent(accent.color()),
            kde::dark_with_accent(accent.color()),
        ] {
            let drawn = mount(
                theme,
                VStack::new()
                    .child(Button::new(lit!("Save")).variant(ButtonVariant::Filled))
                    .child(Button::new(lit!("Cancel")))
                    .child(IconButton::new(IconWidget::checkmark(12.0)))
                    .child(
                        SplitButton::new()
                            .item(MenuItem::new(lit!("Run")))
                            .variant(ButtonVariant::Filled),
                    )
                    .child(Toggle::new(Signal::new(true)))
                    .child(Checkbox::new(Signal::new(true)).label(lit!("On")))
                    .child(RadioButton::new(0, Signal::new(0_usize)).label(lit!("One")))
                    .child(TextInput::new(Signal::new(String::new()))),
            );
            assert!(drawn > 0, "{accent:?} painted nothing");
        }
    }
}

// ── Breeze's colours, checked in the render frame ───────────────────────

/// A push button at rest: Plasma's button face and outline, and the shadow,
/// black at 12.5 %, which over the window is the line Plasma paints below
/// the button.
#[test]
fn the_push_button_paints_breezes_face_outline_and_shadow() {
    for (scheme, theme) in themes() {
        let window = rgba8(theme.colors.surface_main.to_array());
        let (mut tree, id) = placed(theme, Button::new(lit!("OK")));
        let frame = rerender(&mut tree);
        assert!(
            fills(&frame).contains(&sample(scheme, "push_button.normal", 42, 17)),
            "{scheme}: no face {:?} in {:?}",
            sample(scheme, "push_button.normal", 42, 17),
            fills(&frame)
        );
        assert!(
            strokes(&frame).contains(&sample(scheme, "push_button.normal", 1, 17)),
            "{scheme}: no outline in {:?}",
            strokes(&frame)
        );
        assert!(has_shadow(&frame), "{scheme}: no shadow");
        assert!(near(
            over(SHADOW, window),
            sample(scheme, "push_button.normal", 42, 33)
        ));
        // The outline is stroked on the node inset by 1.5 with radius 4.5,
        // and the shadow is that outline one pixel lower.
        let b = tree.bounds(id);
        let outline = painted(&frame)
            .into_iter()
            .find(|p| p.color == sample(scheme, "push_button.normal", 1, 17) && p.stroke > 0.0)
            .unwrap();
        assert_eq!(
            outline.screen,
            [b.x + 1.5, b.y + 1.5, b.width - 3.0, b.height - 3.0]
        );
        let shadow = painted(&frame)
            .into_iter()
            .find(|p| p.color == SHADOW)
            .unwrap();
        assert_eq!(shadow.screen[1], outline.screen[1] + 1.0);
        assert_eq!(shadow.screen[0], outline.screen[0]);
    }
}

/// The default button's fill is the button colour 20 % of the way to the
/// highlight and its outline half-way from the highlight to the button
/// outline, as Plasma paints them.
#[test]
fn the_default_button_blends_the_button_toward_the_highlight() {
    for (scheme, theme) in themes() {
        let (mut tree, _) = placed(
            theme.clone(),
            Button::new(lit!("OK")).variant(ButtonVariant::Filled),
        );
        let frame = rerender(&mut tree);
        let face = sample(scheme, "push_button.default", 42, 17);
        let outline = sample(scheme, "push_button.default", 1, 17);
        assert!(
            fills(&frame).contains(&face),
            "{scheme}: {:?}",
            fills(&frame)
        );
        assert!(strokes(&frame).contains(&outline), "{scheme}");
        assert!(has_shadow(&frame), "{scheme}: the default button casts one");
        // Its label is the primary text, not the on-accent one.
        let text = rgba8(theme.colors.text_primary.to_array());
        assert!(
            frame.glyphs.iter().all(|g| rgba8(g.color) == text),
            "{scheme}: the default button's label is not the primary text"
        );

        // In an inactive window it is a push button, and casts no shadow.
        tree.set_window_active(false);
        let frame = rerender(&mut tree);
        assert!(!fills(&frame).contains(&face), "{scheme}");
        assert!(fills(&frame).contains(&sample(scheme, "push_button.normal", 42, 17)));
        assert!(!has_shadow(&frame), "{scheme}");
    }
}

/// Under the pointer the outline turns to the highlight, and the face stays.
#[test]
fn a_hovered_button_is_outlined_in_the_highlight() {
    for (scheme, theme) in themes() {
        let highlight = sample(scheme, "push_button.hover", 1, 17);
        assert_eq!(highlight, rgba8(theme.colors.accent.to_array()));
        let (mut tree, id) = placed(theme, Button::new(lit!("OK")));
        let frame = rerender(&mut tree);
        assert!(!strokes(&frame).contains(&highlight), "{scheme}: at rest");

        tree.pointer_move(tree.bounds(id).center());
        let frame = rerender(&mut tree);
        assert!(strokes(&frame).contains(&highlight), "{scheme}: hovered");
        assert!(fills(&frame).contains(&sample(scheme, "push_button.hover", 42, 17)));
        assert!(
            has_shadow(&frame),
            "{scheme}: a hovered button keeps its shadow"
        );
    }
}

/// Pressing fills the button 30 % toward the highlight and drops the
/// shadow.
#[test]
fn a_pressed_button_casts_no_shadow() {
    for (scheme, theme) in themes() {
        let (mut tree, id) = placed(theme, Button::new(lit!("OK")));
        let center = tree.bounds(id).center();
        tree.pointer_move(center);
        tree.pointer_down_button(center, PointerButton::Primary);
        let frame = rerender(&mut tree);
        assert!(
            fills(&frame).contains(&sample(scheme, "push_button.pressed", 42, 17)),
            "{scheme}: {:?}",
            fills(&frame)
        );
        assert!(strokes(&frame).contains(&sample(scheme, "push_button.pressed", 1, 17)));
        assert!(
            !has_shadow(&frame),
            "{scheme}: a pressed button cast a shadow"
        );
        tree.pointer_up_button(center, PointerButton::Primary);
    }
}

/// The shadow is an active window's only.
#[test]
fn an_inactive_window_drops_the_shadow() {
    for (scheme, theme) in themes() {
        let (mut tree, _) = placed(theme, Button::new(lit!("OK")));
        assert!(has_shadow(&rerender(&mut tree)), "{scheme}");
        tree.set_window_active(false);
        let frame = rerender(&mut tree);
        assert!(!has_shadow(&frame), "{scheme}: inactive");
        // The rest is Plasma's inactive push button.
        assert!(fills(&frame).contains(&sample(scheme, "push_button.inactive_window", 42, 17)));
        tree.set_window_active(true);
        assert!(has_shadow(&rerender(&mut tree)), "{scheme}: active again");
    }
}

/// A flat button paints nothing at rest, never a shadow, and a highlight
/// outline with no fill under the pointer.
#[test]
fn a_flat_button_paints_nothing_until_hovered_and_never_a_shadow() {
    for (scheme, theme) in themes() {
        let (mut tree, id) = placed(theme, Button::new(lit!("OK")).variant(ButtonVariant::Ghost));
        let frame = rerender(&mut tree);
        assert!(frame.shapes.is_empty(), "{scheme}: {:?}", painted(&frame));
        tree.pointer_move(tree.bounds(id).center());
        let frame = rerender(&mut tree);
        assert!(
            fills(&frame).is_empty(),
            "{scheme}: a hovered flat button has no face"
        );
        assert!(strokes(&frame).contains(&sample(scheme, "push_button.flat_hover", 1, 17)));
        assert!(!has_shadow(&frame), "{scheme}");
    }
}

/// An `IconButton` is the auto-raise tool button: invisible at rest, the
/// highlight outline under the pointer, and a toggle that is on fills with
/// the button text at 12.5 % in the plain outline, as Plasma paints a
/// checked tool button.
#[test]
fn an_icon_button_is_breezes_tool_button() {
    for (scheme, theme) in themes() {
        let window = rgba8(theme.colors.surface_main.to_array());
        let (mut tree, id) = placed(theme.clone(), IconButton::new(IconWidget::checkmark(12.0)));
        let frame = rerender(&mut tree);
        assert!(
            frame.shapes.is_empty(),
            "{scheme}: an idle tool button painted {:?}",
            painted(&frame)
        );
        tree.pointer_move(tree.bounds(id).center());
        let frame = rerender(&mut tree);
        assert!(strokes(&frame).contains(&sample(scheme, "tool_button.hover", 1, 14)));
        assert!(!has_shadow(&frame));

        let (mut tree, _) = placed(
            theme.clone(),
            IconButton::new(IconWidget::checkmark(12.0)).toggle(Signal::new(true)),
        );
        let frame = rerender(&mut tree);
        let checked = painted(&frame)
            .into_iter()
            .find(|p| p.stroke == 0.0 && p.color[3] == 32)
            .unwrap_or_else(|| panic!("{scheme}: no checked fill in {:?}", painted(&frame)));
        assert!(near(
            over(checked.color, window),
            sample(scheme, "tool_button.checked", 14, 14)
        ));
        assert!(strokes(&frame).contains(&sample(scheme, "tool_button.checked", 1, 14)));
        assert!(!has_shadow(&frame));
    }
}

/// A split button is framed as a push button, its filled variants in the
/// accent so their on-accent label reads.
#[test]
fn a_split_button_is_framed_as_a_push_button() {
    for (scheme, theme) in themes() {
        let (mut tree, _) = placed(
            theme.clone(),
            SplitButton::new().item(MenuItem::new(lit!("Run"))),
        );
        let frame = rerender(&mut tree);
        assert!(fills(&frame).contains(&sample(scheme, "push_button.normal", 42, 17)));
        assert!(strokes(&frame).contains(&sample(scheme, "push_button.normal", 1, 17)));
        assert!(has_shadow(&frame));

        let (mut tree, _) = placed(
            theme.clone(),
            SplitButton::new()
                .item(MenuItem::new(lit!("Run")))
                .variant(ButtonVariant::Filled),
        );
        let frame = rerender(&mut tree);
        assert!(fills(&frame).contains(&rgba8(theme.colors.accent.to_array())));
        let on_accent: Color = theme.colors.text_on_accent;
        assert!(
            frame
                .glyphs
                .iter()
                .any(|g| rgba8(g.color) == rgba8(on_accent.to_array())),
            "{scheme}: the filled split button's label is not the on-accent text"
        );
    }
}

/// A filled split button's frame covers exactly what a push button's does:
/// its face and its outline, in the accent, lie on the same rectangles, so
/// the outline hides the shadow's sides and meets the shadow line below.
#[test]
fn a_filled_split_button_frame_has_the_push_buttons_bounds() {
    // Each shape's screen rectangle relative to the node.
    let local = |p: Painted, b: teksilo_canvas::Rect| {
        [
            p.screen[0] - b.x,
            p.screen[1] - b.y,
            p.screen[2],
            p.screen[3],
        ]
    };
    for (scheme, theme) in themes() {
        let (mut tree, id) = placed(
            theme.clone(),
            SplitButton::new().item(MenuItem::new(lit!("Run"))),
        );
        let frame = rerender(&mut tree);
        let b = tree.bounds(id);
        let face = sample(scheme, "push_button.normal", 42, 17);
        let outline = sample(scheme, "push_button.normal", 1, 17);
        let push = painted(&frame);
        let push_face = *push
            .iter()
            .find(|p| p.color == face && p.stroke == 0.0)
            .unwrap();
        let push_outline = *push
            .iter()
            .find(|p| p.color == outline && p.stroke > 0.0)
            .unwrap();

        let (mut tree, id) = placed(
            theme.clone(),
            SplitButton::new()
                .item(MenuItem::new(lit!("Run")))
                .variant(ButtonVariant::Filled),
        );
        let frame = rerender(&mut tree);
        let fb = tree.bounds(id);
        assert_eq!((fb.width, fb.height), (b.width, b.height), "{scheme}");
        let accent = rgba8(theme.colors.accent.to_array());
        let filled = painted(&frame);
        let accent_face = *filled
            .iter()
            .find(|p| p.color == accent && p.stroke == 0.0)
            .unwrap_or_else(|| panic!("{scheme}: no accent face in {filled:?}"));
        let accent_outline = *filled
            .iter()
            .find(|p| p.color == accent && p.stroke > 0.0)
            .unwrap_or_else(|| panic!("{scheme}: no accent outline in {filled:?}"));
        assert_eq!(accent_outline.stroke, push_outline.stroke, "{scheme}");
        assert_eq!(local(accent_face, fb), local(push_face, b), "{scheme}");
        assert_eq!(
            local(accent_outline, fb),
            local(push_outline, b),
            "{scheme}"
        );
        // The shadow is the outline one pixel lower, as under a push button.
        let shadow = *filled.iter().find(|p| p.color == SHADOW).unwrap();
        assert_eq!(shadow.screen[0], accent_outline.screen[0], "{scheme}");
        assert_eq!(shadow.screen[2], accent_outline.screen[2], "{scheme}");
        assert_eq!(shadow.screen[1], accent_outline.screen[1] + 1.0, "{scheme}");
    }
}

/// Keyboard focus adds the 2 px focus frame, centred on the node edge, in
/// the theme's focus colour.
#[test]
fn keyboard_focus_draws_the_focus_frame() {
    for (scheme, theme) in themes() {
        let ring = rgba8(theme.colors.focus_ring.to_array());
        let mut tree = tree_with(theme);
        let row = tree.add(
            HStack::new()
                .child(Button::new(lit!("One")))
                .child(Button::new(lit!("Two"))),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        tree.press_key(Key::Tab, Modifiers::NONE);
        let frame = rerender(&mut tree);
        let first = tree.bounds(tree.children(row)[0]);
        let focus = painted(&frame)
            .into_iter()
            .find(|p| p.color == ring && p.stroke == 2.0)
            .unwrap_or_else(|| panic!("{scheme}: no focus frame in {:?}", painted(&frame)));
        assert_eq!(focus.screen, [first.x, first.y, first.width, first.height]);
    }
}

// ── Check boxes, radio buttons, switches and fields ─────────────────────

/// The 15 x 15 shapes a painted box or disc lies on, filled or stroked.
fn box_shapes(frame: &RenderFrame) -> Vec<Painted> {
    painted(frame)
        .into_iter()
        .filter(|p| (p.screen[2] - 15.0).abs() < 0.01 && (p.screen[3] - 15.0).abs() < 0.01)
        .collect()
}

/// The centre of the painted box of the one indicator in `frame`.
fn box_centre(frame: &RenderFrame) -> teksilo_canvas::Point {
    let b = box_shapes(frame)
        .into_iter()
        .next()
        .expect("a 15 px box is painted");
    teksilo_canvas::Point::new(b.screen[0] + 7.5, b.screen[1] + 7.5)
}

/// An unchecked box: Plasma's button-coloured face on the stroked 15 px
/// box at 2.5 inside the indicator, outlined in the lifted outline (D6).
#[test]
fn an_unchecked_box_is_breezes_face_in_the_lifted_outline() {
    for (scheme, theme) in themes() {
        let (mut tree, id) = in_column(
            theme.clone(),
            Checkbox::new(Signal::new(false)).label(lit!("Check")),
        );
        let frame = rerender(&mut tree);
        let shapes = box_shapes(&frame);
        let face = sample(scheme, "check_box.off", 8, 8);
        assert!(
            shapes.iter().any(|p| p.stroke == 0.0 && p.color == face),
            "{scheme}: no face {face:?} in {shapes:?}"
        );
        let outline = rgba8(theme.colors.border_strong.to_array());
        let stroke = shapes
            .iter()
            .find(|p| p.stroke > 0.0)
            .unwrap_or_else(|| panic!("{scheme}: no outline"));
        assert_eq!(stroke.color, outline, "{scheme}");
        assert_eq!(stroke.stroke, 1.0);
        // The indicator hugs the node's leading edge, 2 px down in the 24 px
        // row: the box at 2.5 in it.
        let b = tree.bounds(id);
        assert_eq!(b.height, 24.0, "{scheme}: D8");
        assert_eq!(
            [stroke.screen[0] - b.x, stroke.screen[1] - b.y],
            [2.5, 4.5],
            "{scheme}"
        );
        // Plasma's own outline is the separator, which D6 lifts.
        let plasma = sample(scheme, "check_box.off", 2, 8);
        assert_ne!(plasma, outline, "{scheme}: the premise of D6");
    }
}

/// A checked box: the highlight outline, the highlight at 30 % over the
/// face, and the check in the view's text, drawn with a 2 px pen and mitred
/// joins.
#[test]
fn a_checked_box_is_washed_in_the_highlight_and_marked_in_the_text() {
    use teksilo_canvas::LineJoin;
    for (scheme, theme) in themes() {
        let (mut tree, id) = in_column(
            theme.clone(),
            Checkbox::new(Signal::new(true)).label(lit!("Check")),
        );
        let frame = rerender(&mut tree);
        let shapes = box_shapes(&frame);
        let face = sample(scheme, "check_box.off", 8, 8);
        let wash = shapes
            .iter()
            .find(|p| p.stroke == 0.0 && p.color[3] == 77)
            .unwrap_or_else(|| panic!("{scheme}: no wash in {shapes:?}"));
        assert!(
            near(over(wash.color, face), sample(scheme, "check_box.on", 8, 8)),
            "{scheme}: {:?}",
            over(wash.color, face)
        );
        assert!(
            shapes
                .iter()
                .any(|p| p.stroke > 0.0 && p.color == sample(scheme, "check_box.on", 2, 8)),
            "{scheme}: no highlight outline"
        );
        let mark = frame
            .paths
            .iter()
            .find(|p| p.stroke_style.width > 0.0)
            .unwrap_or_else(|| panic!("{scheme}: no check mark"));
        assert_eq!(
            rgba8(mark.color),
            sample(scheme, "check_box.on", 13, 8),
            "{scheme}"
        );
        assert_eq!(mark.stroke_style.width, 2.0);
        assert_eq!(mark.stroke_style.line_join, LineJoin::Miter);
        // The mark's three points, in the indicator.
        let b = tree.bounds(id);
        let points: Vec<[f32; 2]> =
            mark.path
                .commands()
                .iter()
                .filter_map(|c| match c {
                    teksilo_canvas::PathCommand::MoveTo(p)
                    | teksilo_canvas::PathCommand::LineTo(p) => Some([p.x - b.x, p.y - b.y]),
                    _ => None,
                })
                .collect();
        assert_eq!(
            points,
            vec![[6.0, 12.0], [9.0, 15.0], [14.5, 9.5]],
            "{scheme}: (6, 10), (9, 13), (14.5, 7.5) of an indicator 2 px down"
        );
    }
}

/// A partial box: three 2 px squares of the view's text, 4 px apart.
#[test]
fn a_partial_box_shows_three_squares() {
    for (scheme, theme) in themes() {
        let (mut tree, _) = placed(
            theme,
            Checkbox::tristate(Signal::new(CheckState::Indeterminate)).label(lit!("Some")),
        );
        let frame = rerender(&mut tree);
        let text = sample(scheme, "check_box.partial", 5, 10);
        let squares: Vec<[f32; 4]> = frame
            .decorations
            .iter()
            .filter(|d| rgba8(d.color) == text)
            .map(|d| d.rect)
            .collect();
        assert_eq!(squares.len(), 3, "{scheme}: {squares:?}");
        for s in &squares {
            assert_eq!((s[2], s[3]), (2.0, 2.0));
        }
        assert_eq!(squares[1][0] - squares[0][0], 4.0);
        assert_eq!(squares[2][0] - squares[1][0], 4.0);
    }
}

/// Under the pointer the box gains a 1 px ring of the focus colour; pressed,
/// its face darkens to Plasma's pressed face.
#[test]
fn a_hovered_box_rings_and_a_pressed_one_darkens() {
    for (scheme, theme) in themes() {
        let (mut tree, _) = placed(
            theme,
            Checkbox::new(Signal::new(false)).label(lit!("Check")),
        );
        let frame = rerender(&mut tree);
        let ring = sample(scheme, "check_box.hover_off", 2, 8);
        assert!(!strokes(&frame).contains(&ring), "{scheme}: at rest");
        let centre = box_centre(&frame);
        tree.pointer_move(centre);
        let frame = rerender(&mut tree);
        assert!(
            box_shapes(&frame)
                .iter()
                .any(|p| p.stroke > 0.0 && p.color == ring),
            "{scheme}: no hover ring"
        );
        tree.pointer_down_button(centre, PointerButton::Primary);
        let frame = rerender(&mut tree);
        let pressed = sample(scheme, "check_box.pressed_off", 8, 8);
        assert!(
            box_shapes(&frame)
                .iter()
                .any(|p| p.stroke == 0.0 && near(p.color, pressed)),
            "{scheme}: no pressed face {pressed:?} in {:?}",
            box_shapes(&frame)
        );
        tree.pointer_up_button(centre, PointerButton::Primary);
    }
}

/// A checked box in a backdrop window is outlined in the scheme's muted
/// highlight, not the active one.
#[test]
fn a_checked_box_in_a_backdrop_window_uses_the_muted_highlight() {
    for (scheme, theme) in themes() {
        let active = rgba8(theme.colors.accent.to_array());
        let (mut tree, _) = placed(
            theme.clone(),
            Checkbox::new(Signal::new(true)).label(lit!("Check")),
        );
        tree.set_window_active(false);
        let frame = rerender(&mut tree);
        let muted = theme.for_paint_state(teksilo_core::styles::PaintState {
            window_active: false,
            high_contrast: false,
        });
        let muted = rgba8(muted.colors.accent.to_array());
        assert_ne!(muted, active, "{scheme}: premise");
        let outlines: Vec<[u8; 4]> = box_shapes(&frame)
            .into_iter()
            .filter(|p| p.stroke > 0.0)
            .map(|p| p.color)
            .collect();
        assert!(outlines.contains(&muted), "{scheme}: {outlines:?}");
        assert!(!outlines.contains(&active), "{scheme}");
    }
}

/// Keyboard focus draws the 2 px focus frame outside the 16 px box.
#[test]
fn keyboard_focus_frames_the_box() {
    for (scheme, theme) in themes() {
        let ring = rgba8(theme.colors.focus_ring.to_array());
        let (mut tree, id) = in_column(theme, Checkbox::new(Signal::new(false)).label(lit!("A")));
        tree.press_key(Key::Tab, Modifiers::NONE);
        let frame = rerender(&mut tree);
        let focus = painted(&frame)
            .into_iter()
            .find(|p| p.color == ring && p.stroke == 2.0)
            .unwrap_or_else(|| panic!("{scheme}: no focus frame in {:?}", painted(&frame)));
        let b = tree.bounds(id);
        // The indicator at (0, 2): the ring's centre line 1 px outside the
        // box at (2, 2), so at (1, 3), 18 px wide.
        assert_eq!(
            [
                focus.screen[0] - b.x,
                focus.screen[1] - b.y,
                focus.screen[2],
                focus.screen[3]
            ],
            [1.0, 3.0, 18.0, 18.0],
            "{scheme}"
        );
    }
}

/// A radio button: the check box's face and lifted outline on a disc, and,
/// selected, the highlight outline, the wash and a 6 px dot of the view's
/// text.
#[test]
fn a_radio_button_is_breezes_disc_and_dot() {
    for (scheme, theme) in themes() {
        let selected = Signal::new(1_usize);
        let (mut tree, _) = placed(
            theme.clone(),
            RadioButton::new(0, selected.clone()).label(lit!("One")),
        );
        let frame = rerender(&mut tree);
        let shapes = box_shapes(&frame);
        assert!(
            shapes
                .iter()
                .any(|p| p.stroke == 0.0 && p.color == sample(scheme, "radio_button.off", 10, 10)),
            "{scheme}: no face in {shapes:?}"
        );
        assert!(
            shapes
                .iter()
                .any(|p| p.stroke > 0.0 && p.color == rgba8(theme.colors.border_strong.to_array())),
            "{scheme}: no lifted outline"
        );

        let (mut tree, _) = placed(
            theme.clone(),
            RadioButton::new(0, Signal::new(0_usize)).label(lit!("One")),
        );
        let frame = rerender(&mut tree);
        let shapes = box_shapes(&frame);
        let face = sample(scheme, "radio_button.off", 10, 10);
        let wash = shapes
            .iter()
            .find(|p| p.stroke == 0.0 && p.color[3] == 77)
            .unwrap_or_else(|| panic!("{scheme}: no wash"));
        assert!(near(
            over(wash.color, face),
            sample(scheme, "radio_button.on", 5, 10)
        ));
        assert!(
            shapes
                .iter()
                .any(|p| p.stroke > 0.0 && p.color == sample(scheme, "radio_button.on", 2, 10)),
            "{scheme}: no highlight outline"
        );
        let dot = painted(&frame)
            .into_iter()
            .find(|p| (p.screen[2] - 6.0).abs() < 0.01 && (p.screen[3] - 6.0).abs() < 0.01)
            .unwrap_or_else(|| panic!("{scheme}: no 6 px dot"));
        assert_eq!(dot.color, sample(scheme, "radio_button.on", 10, 10));
        let disc = shapes[0];
        assert_eq!(
            [dot.screen[0] + 3.0, dot.screen[1] + 3.0],
            [disc.screen[0] + 7.5, disc.screen[1] + 7.5],
            "{scheme}: the dot is centred"
        );
    }
}

/// A switch: the 12 px track in the button colour moved 10 % toward the
/// button text, an 18 px handle in the button colour, and, on, the
/// highlight at 50 % up to the handle, which sits at the trailing end; in
/// right-to-left at the leading one.
#[test]
fn a_switch_is_the_desktop_styles_track_and_handle() {
    use teksilo_core::environment::LayoutDirection;
    use teksilo_kde_scheme::KdeColor;
    use teksilo_kde_scheme::math::mix;
    for (scheme, theme) in themes() {
        let p = theme.extension::<KdePalette>().unwrap();
        let b = p.colors(teksilo_kde_scheme::ColorState::Active);
        let track = mix(
            KdeColor::from_color(b.button),
            KdeColor::from_color(b.button_text),
            0.1,
        )
        .to_color();
        let track = rgba8(track.to_array());
        let button = rgba8(b.button.to_array());
        for direction in [LayoutDirection::LeftToRight, LayoutDirection::RightToLeft] {
            for on in [false, true] {
                let mut tree = tree_with(theme.clone());
                tree.set_layout_direction(direction);
                let row = tree.add(HStack::new().child(Toggle::new(Signal::new(on))));
                tree.layout(SizeProposal::exact(400.0, 300.0));
                let id = tree.children(row)[0];
                let node = tree.bounds(id);
                let frame = rerender(&mut tree);
                let all = painted(&frame);
                let t = all
                    .iter()
                    .find(|s| s.stroke == 0.0 && s.screen[2] == 36.0 && s.screen[3] == 12.0)
                    .unwrap_or_else(|| panic!("{scheme}: no 36 x 12 track in {all:?}"));
                assert_eq!(t.color, track, "{scheme}");
                let handle = all
                    .iter()
                    .find(|s| s.stroke == 0.0 && s.screen[2] == 18.0 && s.screen[3] == 18.0)
                    .unwrap_or_else(|| panic!("{scheme}: no 18 px handle"));
                assert_eq!(handle.color, button, "{scheme}");
                let at_trailing = handle.screen[0] - t.screen[0] == 18.0;
                let rtl = direction == LayoutDirection::RightToLeft;
                assert_eq!(at_trailing, on != rtl, "{scheme} {direction:?} on {on}");
                let checked = all.iter().any(|s| s.stroke == 0.0 && s.color[3] == 128);
                assert_eq!(checked, on, "{scheme}: the checked fill");
                // The node is the density's target; the indicator is centred.
                assert_eq!((node.width, node.height), (36.0, 24.0));
                assert_eq!(t.screen[1] - node.y, 6.0);
            }
        }
    }
}

/// A line edit: Plasma's view fill on the node inset by 0.5 at radius 4.5,
/// the lifted outline at rest (D6) and the lifted focus colour while
/// focused, at the same 1 px. (The hover outline is the style's rule, tested
/// beside it: `TextInput` does not report the pointer over it.)
#[test]
fn a_field_is_breezes_line_edit() {
    for (scheme, theme) in themes() {
        let mut tree = tree_with(theme.clone());
        tree.add(
            VStack::new()
                .child(Button::new(lit!("Before")))
                .child(TextInput::new(Signal::new(String::new()))),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let frame = rerender(&mut tree);
        let fill = sample(scheme, "line_edit.normal", 75, 16);
        let rest = rgba8(theme.colors.border_strong.to_array());
        let outline = painted(&frame)
            .into_iter()
            .find(|p| p.stroke > 0.0 && p.color == rest)
            .unwrap_or_else(|| panic!("{scheme}: no lifted outline in {:?}", painted(&frame)));
        assert_eq!(outline.stroke, 1.0);
        assert_eq!(
            outline.screen[3], 31.0,
            "{scheme}: a 32 px field inset by 0.5"
        );
        assert!(
            painted(&frame)
                .iter()
                .any(|p| p.stroke == 0.0 && p.color == fill && p.screen == outline.screen),
            "{scheme}: no view fill on the frame"
        );
        assert_ne!(
            sample(scheme, "line_edit.normal", 0, 16),
            rest,
            "{scheme}: the premise of D6"
        );

        tree.press_key(Key::Tab, Modifiers::NONE);
        tree.press_key(Key::Tab, Modifiers::NONE);
        let frame = rerender(&mut tree);
        let focus = rgba8(theme.colors.border_focused.to_array());
        let focused = painted(&frame)
            .into_iter()
            .find(|p| p.color == focus && p.screen == outline.screen)
            .unwrap_or_else(|| panic!("{scheme}: no focus outline in {:?}", painted(&frame)));
        assert_eq!(focused.stroke, 1.0, "{scheme}: focus keeps the 1 px width");
    }
}

/// A disabled field: Plasma's disabled view fill in the disabled outline.
#[test]
fn a_disabled_field_paints_the_disabled_colours() {
    for (scheme, theme) in themes() {
        let (mut tree, _) = placed(
            theme.clone(),
            TextInput::new(Signal::new(String::new())).enabled(false),
        );
        let frame = rerender(&mut tree);
        let fill = sample(scheme, "line_edit.disabled", 75, 16);
        assert!(
            fills(&frame).contains(&fill),
            "{scheme}: no {fill:?} in {:?}",
            fills(&frame)
        );
        let outline = rgba8(theme.colors.border_disabled.to_array());
        assert!(strokes(&frame).contains(&outline), "{scheme}");
        assert!(near(outline, sample(scheme, "line_edit.disabled", 0, 16)));
    }
}

// ── Indicator geometry, focus, disabled and right-to-left ──────────────

/// The colours the indicator styles paint from, in an active window.
fn indicator_colors(theme: &Theme, disabled: bool) -> BreezeIndicatorColors {
    BreezeIndicatorColors::resolve(theme, true, false, disabled)
}

fn rgba8_of(c: Color) -> [u8; 4] {
    rgba8(c.to_array())
}

/// Every node under `root` whose type is `name`.
fn nodes_named(tree: &WidgetTree, root: WidgetId, name: &str) -> Vec<WidgetId> {
    fn walk(tree: &WidgetTree, id: WidgetId, name: &str, out: &mut Vec<WidgetId>) {
        if tree
            .widget_type_name(id)
            .is_some_and(|t| t.rsplit("::").next() == Some(name))
        {
            out.push(id);
        }
        for c in tree.children(id) {
            walk(tree, c, name, out);
        }
    }
    let mut out = Vec::new();
    walk(tree, root, name, &mut out);
    out
}

/// The track fill and the handle face of the one switch in `frame`.
fn switch_parts(frame: &RenderFrame) -> (Painted, Painted) {
    let all = painted(frame);
    let track = *all
        .iter()
        .find(|s| s.stroke == 0.0 && s.screen[2] == 36.0 && s.screen[3] == 12.0)
        .unwrap_or_else(|| panic!("no 36 x 12 track in {all:?}"));
    let handle = *all
        .iter()
        .find(|s| s.stroke == 0.0 && s.kind == ShapeKind::Circle && s.screen[2] == 18.0)
        .unwrap_or_else(|| panic!("no 18 px handle in {all:?}"));
    (track, handle)
}

/// The handle's leading edge from the track's, left to right.
fn handle_offset(frame: &RenderFrame) -> f32 {
    let (track, handle) = switch_parts(frame);
    handle.screen[0] - track.screen[0]
}

/// The handle's 1 px outline, a 17 px circle.
fn handle_outline(frame: &RenderFrame) -> Painted {
    *painted(frame)
        .iter()
        .find(|s| s.stroke > 0.0 && s.kind == ShapeKind::Circle && s.screen[2] == 17.0)
        .unwrap_or_else(|| panic!("no 17 px handle outline in {:?}", painted(frame)))
}

/// The track's 1 px outline, 35 x 11.
fn track_outline(frame: &RenderFrame) -> Painted {
    *painted(frame)
        .iter()
        .find(|s| s.stroke > 0.0 && s.screen[2] == 35.0 && s.screen[3] == 11.0)
        .unwrap_or_else(|| panic!("no 35 x 11 track outline in {:?}", painted(frame)))
}

/// The track's outline is inset by half its pen at radius 5.5, the handle's
/// is the 17 px circle inside the handle, and both take the D6 colours; the
/// track is outlined in the lifted highlight when on. Keyboard focus adds
/// the 2 px focus frame around the 36 x 18 indicator and turns the handle's
/// outline to the button set's hover colour.
#[test]
fn a_switch_is_outlined_and_framed_where_the_desktop_style_puts_it() {
    for (scheme, theme) in themes() {
        let c = indicator_colors(&theme, false);
        let ring = rgba8_of(theme.colors.focus_ring);
        for on in [false, true] {
            let (mut tree, id) = on_the_clock(theme.clone(), false, Toggle::new(Signal::new(on)));
            let frame = tick(&mut tree, 0);
            let node = tree.bounds(id);
            let (track, handle) = switch_parts(&frame);
            let t = track_outline(&frame);
            assert_eq!(t.kind, ShapeKind::RoundedRect, "{scheme}");
            assert_eq!(
                t.screen,
                [track.screen[0] + 0.5, track.screen[1] + 0.5, 35.0, 11.0],
                "{scheme} on {on}"
            );
            assert_eq!(t.radius, 5.5, "{scheme}");
            assert_eq!(t.stroke, 1.0);
            let expected = if on { c.checked_outline } else { c.outline };
            assert_eq!(t.color, rgba8_of(expected), "{scheme} on {on}: D6");
            let h = handle_outline(&frame);
            assert_eq!(
                h.screen,
                [handle.screen[0] + 0.5, handle.screen[1] + 0.5, 17.0, 17.0],
                "{scheme}"
            );
            assert_eq!(h.stroke, 1.0);
            assert_eq!(h.color, rgba8_of(c.button_outline), "{scheme}: at rest");
            assert!(
                !painted(&frame)
                    .iter()
                    .any(|p| p.color == ring && p.stroke == 2.0),
                "{scheme}: no focus frame unfocused"
            );

            tree.press_key(Key::Tab, Modifiers::NONE);
            let frame = tick(&mut tree, 100);
            let focus = painted(&frame)
                .into_iter()
                .find(|p| p.color == ring && p.stroke == 2.0)
                .unwrap_or_else(|| panic!("{scheme}: no focus frame in {:?}", painted(&frame)));
            // The indicator at (0, 3) of the 36 x 24 node: the frame's centre
            // line 1 px outside it, at the indicator's radius plus 1.
            assert_eq!(
                [
                    focus.screen[0] - node.x,
                    focus.screen[1] - node.y,
                    focus.screen[2],
                    focus.screen[3]
                ],
                [-1.0, 2.0, 38.0, 20.0],
                "{scheme} on {on}"
            );
            assert_eq!(focus.radius, 10.0, "{scheme}");
            assert_eq!(
                handle_outline(&frame).color,
                rgba8_of(c.button_hover),
                "{scheme}: keyboard focus outlines the handle in the hover colour"
            );
        }
    }
}

/// Keyboard focus draws the radio button's focus frame through the widget:
/// a circle 1 px outside the 16 px disc.
#[test]
fn keyboard_focus_frames_the_disc_in_a_circle() {
    for (scheme, theme) in themes() {
        let ring = rgba8_of(theme.colors.focus_ring);
        let (mut tree, id) = in_column(
            theme,
            RadioButton::new(0, Signal::new(1_usize)).label(lit!("A")),
        );
        tree.press_key(Key::Tab, Modifiers::NONE);
        let frame = rerender(&mut tree);
        let focus = painted(&frame)
            .into_iter()
            .find(|p| p.color == ring && p.stroke == 2.0)
            .unwrap_or_else(|| panic!("{scheme}: no focus frame in {:?}", painted(&frame)));
        let b = tree.bounds(id);
        assert_eq!(
            [
                focus.screen[0] - b.x,
                focus.screen[1] - b.y,
                focus.screen[2],
                focus.screen[3]
            ],
            [1.0, 3.0, 18.0, 18.0],
            "{scheme}"
        );
        assert_eq!(focus.radius, 9.0, "{scheme}: half its side, a circle");
    }
}

/// A disabled checked box and selected disc, against Plasma's disabled
/// samples: the disabled highlight's wash over the disabled face, the mark
/// and the dot in the disabled view text, the outline in the disabled
/// highlight, and no hover ring under the pointer.
#[test]
fn disabled_indicators_paint_plasmas_disabled_colours() {
    for (scheme, theme) in themes() {
        let c = indicator_colors(&theme, true);
        let highlight = rgba8_of(theme.colors.accent_disabled);

        let id = "check_box.disabled_on";
        let (mut tree, _) = placed(
            theme.clone(),
            Checkbox::new(Signal::new(true))
                .label(lit!("Check"))
                .enabled(false),
        );
        let frame = rerender(&mut tree);
        let shapes = box_shapes(&frame);
        let face = shapes
            .iter()
            .find(|p| p.stroke == 0.0 && p.color[3] == 255)
            .unwrap_or_else(|| panic!("{scheme}: no face in {shapes:?}"));
        assert_eq!(face.color, rgba8_of(c.button), "{scheme}");
        let wash = shapes
            .iter()
            .find(|p| p.stroke == 0.0 && p.color[3] == 77)
            .unwrap_or_else(|| panic!("{scheme}: no wash in {shapes:?}"));
        assert!(
            near(over(wash.color, face.color), sample(scheme, id, 4, 12)),
            "{scheme}: {:?} against {:?}",
            over(wash.color, face.color),
            sample(scheme, id, 4, 12)
        );
        let outline = shapes
            .iter()
            .find(|p| p.stroke > 0.0)
            .unwrap_or_else(|| panic!("{scheme}: no outline"));
        assert_eq!(outline.color, highlight, "{scheme}");
        let mark = frame
            .paths
            .iter()
            .find(|p| p.stroke_style.width > 0.0)
            .unwrap_or_else(|| panic!("{scheme}: no check mark"));
        assert!(
            near(rgba8(mark.color), sample(scheme, id, 13, 8)),
            "{scheme}: {:?} against {:?}",
            rgba8(mark.color),
            sample(scheme, id, 13, 8)
        );
        tree.pointer_move(box_centre(&frame));
        let frame = rerender(&mut tree);
        assert_eq!(
            box_shapes(&frame).iter().filter(|p| p.stroke > 0.0).count(),
            1,
            "{scheme}: no hover ring on a disabled box"
        );

        let id = "radio_button.disabled_on";
        let (mut tree, _) = placed(
            theme.clone(),
            RadioButton::new(0, Signal::new(0_usize))
                .label(lit!("One"))
                .enabled(false),
        );
        let frame = rerender(&mut tree);
        let shapes = box_shapes(&frame);
        let face = shapes
            .iter()
            .find(|p| p.stroke == 0.0 && p.color[3] == 255)
            .unwrap_or_else(|| panic!("{scheme}: no disc face in {shapes:?}"));
        let wash = shapes
            .iter()
            .find(|p| p.stroke == 0.0 && p.color[3] == 77)
            .unwrap_or_else(|| panic!("{scheme}: no disc wash"));
        assert!(
            near(over(wash.color, face.color), sample(scheme, id, 4, 10)),
            "{scheme}: {:?} against {:?}",
            over(wash.color, face.color),
            sample(scheme, id, 4, 10)
        );
        assert!(
            shapes
                .iter()
                .any(|p| p.stroke > 0.0 && p.color == highlight),
            "{scheme}: no disabled highlight outline"
        );
        let dot = painted(&frame)
            .into_iter()
            .find(|p| p.kind == ShapeKind::Circle && (p.screen[2] - 6.0).abs() < 0.01)
            .unwrap_or_else(|| panic!("{scheme}: no 6 px dot"));
        assert!(
            near(dot.color, sample(scheme, id, 10, 10)),
            "{scheme}: {:?} against {:?}",
            dot.color,
            sample(scheme, id, 10, 10)
        );
    }
}

/// Right to left, the box and the disc hug the node's trailing (right)
/// edge, the label ends 24 px from it, and the partial squares are
/// centred on the box, the first to appear on the leading (right) side.
#[test]
fn right_to_left_mirrors_the_indicators() {
    use teksilo_core::environment::LayoutDirection;
    for (scheme, theme) in themes() {
        let rows: [(&str, Box<dyn Widget>); 2] = [
            (
                "Checkbox",
                Box::new(Checkbox::new(Signal::new(false)).label(lit!("Check"))),
            ),
            (
                "RadioButton",
                Box::new(RadioButton::new(0, Signal::new(1_usize)).label(lit!("Radio"))),
            ),
        ];
        for (name, w) in rows {
            let mut tree = tree_with(theme.clone());
            tree.set_layout_direction(LayoutDirection::RightToLeft);
            let root = tree.add(VStack::new().child(HStack::new().child(w)));
            tree.layout(SizeProposal::exact(400.0, 300.0));
            let node = nodes_named(&tree, root, name);
            assert_eq!(node.len(), 1, "{scheme}: one {name}");
            let b = tree.bounds(node[0]);
            let frame = rerender(&mut tree);
            let stroke = box_shapes(&frame)
                .into_iter()
                .find(|p| p.stroke > 0.0)
                .unwrap_or_else(|| panic!("{scheme}: no {name} outline"));
            assert_eq!(
                [
                    b.x + b.width - (stroke.screen[0] + stroke.screen[2]),
                    stroke.screen[1] - b.y
                ],
                [2.5, 4.5],
                "{scheme} {name}: the box mirrored"
            );
            let label = nodes_named(&tree, node[0], "TextWidget");
            assert_eq!(label.len(), 1, "{scheme}: one label");
            let l = tree.bounds(label[0]);
            assert_eq!(
                b.x + b.width - (l.x + l.width),
                24.0,
                "{scheme} {name}: the label 24 px from the trailing edge"
            );
        }

        let state = Signal::new(CheckState::Unchecked);
        let mut tree = tree_with(theme.clone());
        tree.set_layout_direction(LayoutDirection::RightToLeft);
        tree.advance_time(Duration::ZERO);
        tree.add(
            VStack::new()
                .child(HStack::new().child(Checkbox::tristate(state.clone()).label(lit!("Some")))),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        tree.render();
        let text = sample(scheme, "check_box.partial", 5, 10);
        state.set(CheckState::Indeterminate);
        let squares = |f: &RenderFrame| -> Vec<[f32; 4]> {
            f.decorations
                .iter()
                .filter(|d| rgba8(d.color) == text && d.rect[2] == 2.0)
                .map(|d| d.rect)
                .collect()
        };
        tick(&mut tree, 0);
        let frame = tick(&mut tree, 40);
        let centre = box_centre(&frame);
        let first = squares(&frame);
        assert_eq!(first.len(), 1, "{scheme}: {first:?}");
        assert_eq!(first[0][0] + 1.0 - centre.x, 4.0, "{scheme}: on the right");
        let frame = tick(&mut tree, 60);
        let all = squares(&frame);
        assert_eq!(all.len(), 3, "{scheme}");
        let mut xs: Vec<f32> = all.iter().map(|s| s[0] + 1.0 - centre.x).collect();
        xs.sort_by(f32::total_cmp);
        assert_eq!(xs, vec![-4.0, 0.0, 4.0], "{scheme}: centred on the box");
    }
}

/// The hover ring is the view's focus decoration, not the highlight. With a
/// Plasma accent applied the two differ (the highlight is the accent at
/// 70 % over the view, the focus decoration the accent itself), so the
/// ring's colour tells them apart.
#[test]
fn the_hover_ring_is_the_focus_colour_not_the_highlight() {
    for accent in PlasmaAccent::ALL {
        for theme in [
            kde::light_with_accent(accent.color()),
            kde::dark_with_accent(accent.color()),
        ] {
            let c = indicator_colors(&theme, false);
            let focus = rgba8_of(c.focus);
            let highlight = rgba8_of(c.highlight);
            assert_ne!(focus, highlight, "{accent:?}: premise");
            for checked in [false, true] {
                let (mut tree, _) = placed(
                    theme.clone(),
                    Checkbox::new(Signal::new(checked)).label(lit!("Check")),
                );
                let frame = rerender(&mut tree);
                tree.pointer_move(box_centre(&frame));
                let frame = rerender(&mut tree);
                let outlines: Vec<[u8; 4]> = box_shapes(&frame)
                    .into_iter()
                    .filter(|p| p.stroke > 0.0)
                    .map(|p| p.color)
                    .collect();
                assert!(
                    outlines.contains(&focus),
                    "{accent:?} checked {checked}: {outlines:?}"
                );
                assert_eq!(
                    outlines.contains(&highlight),
                    checked,
                    "{accent:?} checked {checked}: the highlight outlines only a checked box"
                );
            }
            let (mut tree, _) = placed(
                theme.clone(),
                RadioButton::new(0, Signal::new(1_usize)).label(lit!("One")),
            );
            let frame = rerender(&mut tree);
            tree.pointer_move(box_centre(&frame));
            let frame = rerender(&mut tree);
            assert!(
                box_shapes(&frame)
                    .iter()
                    .any(|p| p.stroke > 0.0 && p.color == focus),
                "{accent:?}: no focus ring on the hovered disc"
            );
        }
    }
}

// ── Motion, on the tree's simulated clock ───────────────────────────────

/// A tree on the simulated clock, with or without the reduced-motion
/// preference, holding `w` in a row in a column, laid out and rendered
/// once: the tree and the widget's id.
fn on_the_clock(
    theme: Theme,
    reduced_motion: bool,
    w: impl Widget + 'static,
) -> (WidgetTree, WidgetId) {
    let mut tree = tree_with(theme);
    tree.set_accessibility_preferences(false, reduced_motion, 1.0);
    tree.advance_time(Duration::ZERO);
    let column = tree.add(VStack::new().child(HStack::new().child(w)));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let row = tree.children(column)[0];
    let id = tree.children(row)[0];
    tree.render();
    (tree, id)
}

/// Move the clock on by `ms` and render. An animation samples the clock
/// at most once a frame (about 16 ms), so the steps the tests take are at
/// least that long wherever the value is read.
fn tick(tree: &mut WidgetTree, ms: u64) -> Rc<RenderFrame> {
    tree.advance_time(Duration::from_millis(ms));
    rerender(tree)
}

/// The check mark's points, in the node's coordinates.
fn mark_points(frame: &RenderFrame, node: teksilo_canvas::Rect) -> Vec<[f32; 2]> {
    frame
        .paths
        .iter()
        .find(|p| p.stroke_style.width > 0.0)
        .map(|mark| {
            mark.path
                .commands()
                .iter()
                .filter_map(|c| match c {
                    teksilo_canvas::PathCommand::MoveTo(p)
                    | teksilo_canvas::PathCommand::LineTo(p) => Some([p.x - node.x, p.y - node.y]),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The diameter of the radio dot in `frame`, if one is painted.
fn dot_diameter(frame: &RenderFrame, text: [u8; 4]) -> Option<f32> {
    painted(frame)
        .into_iter()
        .find(|p| p.kind == ShapeKind::Circle && p.stroke == 0.0 && p.color == text)
        .map(|p| p.screen[2])
}

/// The whole mark, an indicator 2 px down in the 24 px row.
const WHOLE_MARK: [[f32; 2]; 3] = [[6.0, 12.0], [9.0, 15.0], [14.5, 9.5]];

/// The handle travels its 18 px in 90 ms (k.8: 100 ms, or 18 px at Qt's
/// 200 px a second if that is shorter), half way at the half time on its
/// ease-in-out, and back the same way.
#[test]
fn the_switch_handle_travels_in_ninety_milliseconds() {
    for (scheme, theme) in themes() {
        let on = Signal::new(false);
        let (mut tree, _) = on_the_clock(theme, false, Toggle::new(on.clone()));
        on.set(true);
        assert_eq!(handle_offset(&tick(&mut tree, 0)), 0.0, "{scheme}: at 0");
        let mid = handle_offset(&tick(&mut tree, 45));
        assert!(
            (8.5..9.5).contains(&mid),
            "{scheme}: half way at 45 ms, {mid}"
        );
        let late = handle_offset(&tick(&mut tree, 25));
        assert!(
            late > mid && late < 18.0,
            "{scheme}: still moving at 70 ms, {late}"
        );
        assert_eq!(
            handle_offset(&tick(&mut tree, 20)),
            18.0,
            "{scheme}: at 90 ms"
        );
        on.set(false);
        tick(&mut tree, 0);
        let back = handle_offset(&tick(&mut tree, 45));
        assert!((8.5..9.5).contains(&back), "{scheme}: {back}");
        assert_eq!(
            handle_offset(&tick(&mut tree, 45)),
            0.0,
            "{scheme}: back at 90 ms"
        );
    }
}

/// Under the pointer the handle's outline turns to the hover colour over the
/// short duration, 100 ms, on the ease-in cubic: an eighth of the way at
/// half the time.
#[test]
fn the_switch_handle_outline_turns_over_a_hundred_milliseconds() {
    for (scheme, theme) in themes() {
        let c = indicator_colors(&theme, false);
        let rest = rgba8_of(c.button_outline);
        let hover = rgba8_of(c.button_hover);
        let (mut tree, _) = on_the_clock(theme, false, Toggle::new(Signal::new(false)));
        let frame = tick(&mut tree, 0);
        let (_, handle) = switch_parts(&frame);
        assert_eq!(handle_outline(&frame).color, rest, "{scheme}");
        tree.pointer_move(teksilo_canvas::Point::new(
            handle.screen[0] + 9.0,
            handle.screen[1] + 9.0,
        ));
        assert_eq!(handle_outline(&tick(&mut tree, 0)).color, rest, "{scheme}");
        let half = handle_outline(&tick(&mut tree, 50)).color;
        let eighth = rgba8_of(c.button_outline.mix(c.button_hover, 0.125));
        assert!(near(half, eighth), "{scheme}: {half:?}, not {eighth:?}");
        let late = handle_outline(&tick(&mut tree, 30)).color;
        assert_ne!(late, hover, "{scheme}: still turning at 80 ms");
        assert_eq!(
            handle_outline(&tick(&mut tree, 20)).color,
            hover,
            "{scheme}"
        );
    }
}

/// The check mark draws along its length over 100 ms, linearly: part of its
/// first stroke at 25 ms, into its second at 50, whole at 100.
#[test]
fn the_check_mark_draws_itself_over_a_hundred_milliseconds() {
    for (scheme, theme) in themes() {
        let checked = Signal::new(false);
        let (mut tree, id) = on_the_clock(
            theme,
            false,
            Checkbox::new(checked.clone()).label(lit!("Check")),
        );
        let node = tree.bounds(id);
        checked.set(true);
        assert!(
            mark_points(&tick(&mut tree, 0), node).is_empty(),
            "{scheme}"
        );
        let quarter = mark_points(&tick(&mut tree, 25), node);
        assert_eq!(quarter.len(), 2, "{scheme}: {quarter:?}");
        assert_eq!(quarter[0], WHOLE_MARK[0]);
        assert!(
            quarter[1][0] > 6.0 && quarter[1][0] < 9.0,
            "{scheme}: {quarter:?}"
        );
        let half = mark_points(&tick(&mut tree, 25), node);
        assert_eq!(half.len(), 3, "{scheme}: {half:?}");
        assert_eq!(&half[..2], &WHOLE_MARK[..2]);
        assert!(half[2][0] > 9.0 && half[2][0] < 14.5, "{scheme}: {half:?}");
        let late = mark_points(&tick(&mut tree, 30), node);
        assert_ne!(late, WHOLE_MARK.to_vec(), "{scheme}: not whole at 80 ms");
        assert_eq!(mark_points(&tick(&mut tree, 20), node), WHOLE_MARK.to_vec());
    }
}

/// The partial squares appear one at each third of the 100 ms.
#[test]
fn the_partial_squares_appear_one_at_each_third() {
    for (scheme, theme) in themes() {
        let text = sample(scheme, "check_box.partial", 5, 10);
        let state = Signal::new(CheckState::Unchecked);
        let (mut tree, _) = on_the_clock(
            theme,
            false,
            Checkbox::tristate(state.clone()).label(lit!("Some")),
        );
        state.set(CheckState::Indeterminate);
        let count = |f: &RenderFrame| {
            f.decorations
                .iter()
                .filter(|d| rgba8(d.color) == text && d.rect[2] == 2.0)
                .count()
        };
        for (ms, shown) in [(0, 0), (20, 0), (20, 1), (30, 2), (30, 3)] {
            let frame = tick(&mut tree, ms);
            assert_eq!(count(&frame), shown, "{scheme}: after {ms} more ms");
        }
    }
}

/// The radio dot grows to its 6 px over 100 ms, linearly.
#[test]
fn the_radio_dot_grows_over_a_hundred_milliseconds() {
    for (scheme, theme) in themes() {
        let text = sample(scheme, "radio_button.on", 10, 10);
        let selected = Signal::new(1_usize);
        let (mut tree, _) = on_the_clock(
            theme,
            false,
            RadioButton::new(0, selected.clone()).label(lit!("One")),
        );
        selected.set(0);
        assert_eq!(dot_diameter(&tick(&mut tree, 0), text), None, "{scheme}");
        let half = dot_diameter(&tick(&mut tree, 50), text).expect("a dot at 50 ms");
        assert!((half - 3.0).abs() < 0.01, "{scheme}: {half}");
        let late = dot_diameter(&tick(&mut tree, 30), text).expect("a dot at 80 ms");
        assert!((late - 4.8).abs() < 0.01, "{scheme}: {late}");
        let whole = dot_diameter(&tick(&mut tree, 20), text).expect("a dot at 100 ms");
        assert!((whole - 6.0).abs() < 0.01, "{scheme}: {whole}");
    }
}

/// Under the reduced-motion preference every indicator snaps: the handle,
/// its outline, the mark, the partial squares and the dot reach their end
/// with no time passing.
#[test]
fn reduced_motion_snaps_every_indicator() {
    for (scheme, theme) in themes() {
        let c = indicator_colors(&theme, false);

        let on = Signal::new(false);
        let (mut tree, _) = on_the_clock(theme.clone(), true, Toggle::new(on.clone()));
        on.set(true);
        let frame = tick(&mut tree, 0);
        assert_eq!(handle_offset(&frame), 18.0, "{scheme}: the handle");
        let (_, handle) = switch_parts(&frame);
        tree.pointer_move(teksilo_canvas::Point::new(
            handle.screen[0] + 9.0,
            handle.screen[1] + 9.0,
        ));
        assert_eq!(
            handle_outline(&tick(&mut tree, 0)).color,
            rgba8_of(c.button_hover),
            "{scheme}: the handle's outline"
        );

        let checked = Signal::new(false);
        let (mut tree, id) = on_the_clock(
            theme.clone(),
            true,
            Checkbox::new(checked.clone()).label(lit!("Check")),
        );
        let node = tree.bounds(id);
        checked.set(true);
        assert_eq!(
            mark_points(&tick(&mut tree, 0), node),
            WHOLE_MARK.to_vec(),
            "{scheme}: the mark"
        );

        let state = Signal::new(CheckState::Unchecked);
        let (mut tree, _) = on_the_clock(
            theme.clone(),
            true,
            Checkbox::tristate(state.clone()).label(lit!("Some")),
        );
        state.set(CheckState::Indeterminate);
        let text = sample(scheme, "check_box.partial", 5, 10);
        let frame = tick(&mut tree, 0);
        let squares = frame
            .decorations
            .iter()
            .filter(|d| rgba8(d.color) == text && d.rect[2] == 2.0)
            .count();
        assert_eq!(squares, 3, "{scheme}: the partial squares");

        let selected = Signal::new(1_usize);
        let (mut tree, _) = on_the_clock(
            theme.clone(),
            true,
            RadioButton::new(0, selected.clone()).label(lit!("One")),
        );
        selected.set(0);
        let text = sample(scheme, "radio_button.on", 10, 10);
        let d = dot_diameter(&tick(&mut tree, 0), text).expect("a dot");
        assert!((d - 6.0).abs() < 0.01, "{scheme}: the dot, {d}");
    }
}
