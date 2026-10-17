// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Fluent slider under right-to-left layout.
//!
//! A horizontal `Slider` reads its pointer and its arrow keys from the
//! leading edge, the right one under right to left. The style has to paint
//! the thumb and the fill from the same edge, or the thumb slides away from
//! the pointer dragging it and the arrow that raises the value moves the thumb
//! towards the minimum. These tests mount the real widget under the Fluent
//! theme in both directions and read what was painted.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{MockTextBackend, Point, Rect, RenderFrame, ShapeKind, SizeProposal};
use teksilo_core::environment::LayoutDirection;
use teksilo_core::event::{Key, Modifiers, PointerButton};
use teksilo_core::signal::Signal;
use teksilo_core::styles::Theme;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_tokens::Orientation;
use teksilo_widgets::primitives::FixedSize;
use teksilo_widgets::{Slider, VStack};

/// The thumb's outer disc (dp), `FluentSliderStyle`'s.
const THUMB: f32 = 22.0;
/// The track's thickness (dp).
const TRACK: f32 = 4.0;
/// The slider's length along its main axis (dp).
const LENGTH: f32 = 200.0;
/// The type name of the node the style paints.
const BODY: &str = "FluentSliderBody";

const LTR: LayoutDirection = LayoutDirection::LeftToRight;
const RTL: LayoutDirection = LayoutDirection::RightToLeft;

fn themes() -> [(&'static str, Theme); 2] {
    [
        ("Fluent light", teksilo_theme_fluent::light()),
        ("Fluent dark", teksilo_theme_fluent::dark()),
    ]
}

fn rgba8(c: [f32; 4]) -> [u8; 4] {
    c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// A tree holding a `LENGTH` dp slider at `value` (0 to 1) laid out in
/// `direction`, `orientation`: the tree, the style's node bounds and the
/// value.
fn slider_tree(
    theme: &Theme,
    value: f32,
    direction: LayoutDirection,
    orientation: Orientation,
) -> (WidgetTree, Rect, Signal<f32>) {
    let value = Signal::new(value);
    let mut tree = WidgetTree::new()
        .with_theme(theme.clone())
        .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
    tree.set_layout_direction(direction);
    let slider = Slider::new(value.clone(), 0.0, 1.0)
        .orientation(orientation)
        .jump_on_press(true);
    let sized = match orientation {
        Orientation::Horizontal => FixedSize::new().width(LENGTH).child(slider),
        Orientation::Vertical => FixedSize::new().height(LENGTH).child(slider),
    };
    let root = tree.add(VStack::new().child(sized));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let bounds = body_bounds(&tree, root);
    (tree, bounds, value)
}

/// The bounds of the one node the style paints under `root`.
fn body_bounds(tree: &WidgetTree, root: WidgetId) -> Rect {
    fn walk(tree: &WidgetTree, id: WidgetId, out: &mut Vec<WidgetId>) {
        if tree
            .widget_type_name(id)
            .is_some_and(|t| t.rsplit("::").next() == Some(BODY))
        {
            out.push(id);
        }
        for c in tree.children(id) {
            walk(tree, c, out);
        }
    }
    let mut out = Vec::new();
    walk(tree, root, &mut out);
    assert_eq!(out.len(), 1, "one {BODY}");
    tree.bounds(out[0])
}

fn rerender(tree: &mut WidgetTree) -> Rc<RenderFrame> {
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.render()
}

/// The centre of the painted thumb: the one filled disc of its diameter.
fn thumb_centre(frame: &RenderFrame) -> Point {
    let discs: Vec<_> = frame
        .shapes
        .iter()
        .filter(|s| s.shape == ShapeKind::Circle && s.stroke_width == 0.0)
        .filter(|s| (s.screen[2] - THUMB).abs() < 1e-3)
        .collect();
    assert_eq!(discs.len(), 1, "one thumb face in {:?}", frame.shapes);
    let s = discs[0].screen;
    Point::new(s[0] + s[2] * 0.5, s[1] + s[3] * 0.5)
}

/// The painted fill: the accent part of the track, `[x, y, width, height]`.
fn fill_rect(frame: &RenderFrame, theme: &Theme, orientation: Orientation) -> [f32; 4] {
    let accent = rgba8(theme.colors.accent.to_array());
    let fills: Vec<_> = frame
        .shapes
        .iter()
        .filter(|s| s.shape != ShapeKind::Circle && s.stroke_width == 0.0)
        .filter(|s| rgba8(s.color) == accent)
        .filter(|s| {
            let cross = match orientation {
                Orientation::Horizontal => s.screen[3],
                Orientation::Vertical => s.screen[2],
            };
            (cross - TRACK).abs() < 1e-3
        })
        .collect();
    assert_eq!(fills.len(), 1, "one fill in {:?}", frame.shapes);
    fills[0].screen
}

/// The same value paints the thumb and the fill mirrored about the slider's
/// centre: the fill runs from the right end of the track to the thumb.
#[test]
fn right_to_left_mirrors_the_thumb_and_the_fill() {
    let half = THUMB * 0.5;
    for (scheme, theme) in themes() {
        for value in [0.0_f32, 0.3, 0.75, 1.0] {
            let (mut l_tree, lb, _) = slider_tree(&theme, value, LTR, Orientation::Horizontal);
            let (mut r_tree, rb, _) = slider_tree(&theme, value, RTL, Orientation::Horizontal);
            assert_eq!(rb.width, LENGTH, "{scheme}");
            let l_frame = rerender(&mut l_tree);
            let r_frame = rerender(&mut r_tree);

            let (l, r) = (thumb_centre(&l_frame), thumb_centre(&r_frame));
            let from_left = l.x - lb.x;
            let from_right = rb.x + rb.width - r.x;
            assert!(
                (from_left - from_right).abs() < 1e-3,
                "{scheme} at {value}: the thumb is {from_left} from the left LTR \
                 and {from_right} from the right RTL"
            );
            assert!(
                (from_right - half - (LENGTH - THUMB) * value).abs() < 1e-3,
                "{scheme} at {value}: the minimum is at the right"
            );
            assert!((l.y - lb.y - (r.y - rb.y)).abs() < 1e-3, "{scheme}");

            if value == 0.0 {
                continue;
            }
            let lf = fill_rect(&l_frame, &theme, Orientation::Horizontal);
            let rf = fill_rect(&r_frame, &theme, Orientation::Horizontal);
            assert!(
                (lf[0] - (lb.x + half)).abs() < 1e-3,
                "{scheme} at {value}: LTR fills from the left end of the track"
            );
            assert!(
                (rf[0] + rf[2] - (rb.x + rb.width - half)).abs() < 1e-3,
                "{scheme} at {value}: RTL fills from the right end of the track"
            );
            assert!((rf[0] - r.x).abs() < 1e-3, "{scheme}: up to the thumb");
            assert!((lf[2] - rf[2]).abs() < 1e-3, "{scheme}: the same length");
            assert!(
                (lf[1] - lb.y - (rf[1] - rb.y)).abs() < 1e-3 && lf[3] == rf[3],
                "{scheme}: on the same line"
            );
        }
    }
}

/// A press puts the value where the thumb is then painted, and a drag
/// leftward raises the value with the thumb under the pointer.
#[test]
fn right_to_left_keeps_the_thumb_under_the_pointer() {
    let half = THUMB * 0.5;
    let usable = LENGTH - THUMB;
    for (scheme, theme) in themes() {
        let (mut tree, b, value) = slider_tree(&theme, 0.5, RTL, Orientation::Horizontal);
        rerender(&mut tree);

        // A quarter of the travel in from the left is three quarters of the
        // range under right to left.
        let at = Point::new(b.x + half + usable * 0.25, b.y + b.height * 0.5);
        tree.pointer_move(at);
        tree.pointer_down_button(at, PointerButton::Primary);
        let v = value.get();
        assert!(
            (v - 0.75).abs() < 1e-3,
            "{scheme}: the left is the high end, {v}"
        );
        let thumb = thumb_centre(&rerender(&mut tree));
        assert!(
            (thumb.x - at.x).abs() < 0.01,
            "{scheme}: the thumb at {} is under the pointer at {}",
            thumb.x,
            at.x
        );

        // Dragging further left raises the value, and the thumb follows. The
        // first move past the slop starts the drag at the press; the second
        // moves it, to 90 %, a value on the 1 % grid a stepless slider snaps
        // to.
        let to = Point::new(at.x - usable * 0.15, at.y);
        tree.pointer_move(Point::new(at.x - usable * 0.05, at.y));
        tree.pointer_move(to);
        let raised = value.get();
        assert!(
            raised > v,
            "{scheme}: a leftward drag raises, {v} to {raised}"
        );
        let thumb = thumb_centre(&rerender(&mut tree));
        assert!(
            (thumb.x - to.x).abs() < 0.01,
            "{scheme}: the thumb at {} follows the pointer to {}",
            thumb.x,
            to.x
        );
        tree.pointer_up_button(to, PointerButton::Primary);
    }
}

/// The arrow that raises the value under right to left (the leftward one)
/// moves the thumb left, and the fill grows from the right with it.
#[test]
fn right_to_left_arrow_keys_move_the_thumb_the_way_they_point() {
    for (scheme, theme) in themes() {
        let (mut tree, _, value) = slider_tree(&theme, 0.5, RTL, Orientation::Horizontal);
        rerender(&mut tree);
        tree.press_key(Key::Tab, Modifiers::NONE);
        let frame = rerender(&mut tree);
        let start = thumb_centre(&frame);
        let start_fill = fill_rect(&frame, &theme, Orientation::Horizontal);

        tree.press_key(Key::ArrowLeft, Modifiers::NONE);
        assert!(
            value.get() > 0.5,
            "{scheme}: ArrowLeft raises, {}",
            value.get()
        );
        let frame = rerender(&mut tree);
        let left = thumb_centre(&frame);
        assert!(left.x < start.x, "{scheme}: and moves the thumb left");
        let fill = fill_rect(&frame, &theme, Orientation::Horizontal);
        assert!(fill[2] > start_fill[2], "{scheme}: the fill grows");
        assert!(
            (fill[0] + fill[2] - (start_fill[0] + start_fill[2])).abs() < 1e-3,
            "{scheme}: from the same right end"
        );

        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert!(
            value.get() < 0.5,
            "{scheme}: ArrowRight lowers, {}",
            value.get()
        );
        let right = thumb_centre(&rerender(&mut tree));
        assert!(right.x > start.x, "{scheme}: and moves the thumb right");
    }
}

/// A vertical slider has no leading edge: it paints the same in both
/// directions.
#[test]
fn a_vertical_slider_paints_the_same_in_both_directions() {
    for (scheme, theme) in themes() {
        let (mut l_tree, lb, _) = slider_tree(&theme, 0.3, LTR, Orientation::Vertical);
        let (mut r_tree, rb, _) = slider_tree(&theme, 0.3, RTL, Orientation::Vertical);
        let l_frame = rerender(&mut l_tree);
        let r_frame = rerender(&mut r_tree);
        let (l, r) = (thumb_centre(&l_frame), thumb_centre(&r_frame));
        assert!(
            (l.x - lb.x - (r.x - rb.x)).abs() < 1e-3 && (l.y - lb.y - (r.y - rb.y)).abs() < 1e-3,
            "{scheme}: the thumb at {l:?} in {lb:?} and {r:?} in {rb:?}"
        );
        let lf = fill_rect(&l_frame, &theme, Orientation::Vertical);
        let rf = fill_rect(&r_frame, &theme, Orientation::Vertical);
        assert!(
            (lf[0] - lb.x - (rf[0] - rb.x)).abs() < 1e-3
                && (lf[1] - lb.y - (rf[1] - rb.y)).abs() < 1e-3
                && lf[2] == rf[2]
                && lf[3] == rf[3],
            "{scheme}: the fill {lf:?} in {lb:?} and {rf:?} in {rb:?}"
        );
    }
}
