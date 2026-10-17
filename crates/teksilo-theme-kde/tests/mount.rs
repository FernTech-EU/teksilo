// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Mount every widget the Breeze preset restyles, in both schemes, render
//! it, and check the colours it paints against Plasma's own.
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

use serde_json::Value;
use teksilo_canvas::{MockTextBackend, RenderFrame, SizeProposal};
use teksilo_core::event::PointerButton;
use teksilo_core::signal::Signal;
use teksilo_core::styles::Theme;
use teksilo_core::widget::Widget;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;
use teksilo_theme_kde::{self as kde, KdePalette, PlasmaAccent};
use teksilo_tokens::Color;
use teksilo_widgets::primitives::{HStack, IconWidget};
use teksilo_widgets::{
    Button, ButtonVariant, IconButton, IconButtonSize, MenuItem, SplitButton, VStack,
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

fn rerender(tree: &mut WidgetTree) -> Rc<RenderFrame> {
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.render()
}

fn rgba8(c: [f32; 4]) -> [u8; 4] {
    c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// One painted shape: its colour in 8 bits and its stroke width (0 for a
/// fill).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Painted {
    color: [u8; 4],
    stroke: f32,
    screen: [f32; 4],
}

fn painted(frame: &RenderFrame) -> Vec<Painted> {
    frame
        .shapes
        .iter()
        .map(|s| Painted {
            color: rgba8(s.color),
            stroke: s.stroke_width,
            screen: s.screen,
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
            .child(SplitButton::new().item(MenuItem::new(lit!("Run")))),
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
                    ),
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
    use teksilo_core::event::{Key, Modifiers};
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
