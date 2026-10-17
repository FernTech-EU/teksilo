// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The macOS slider standing vertical.
//!
//! A vertical `Slider` puts its minimum at the bottom and its maximum at the
//! top: a press near the top sets a value near the maximum, `ArrowUp` raises
//! the value, and the thumb it reports through `target_regions` rises with
//! it. The style has to paint the knob and the fill the same way, or the
//! knob sits opposite to where a press puts the value and `ArrowUp` moves it
//! down. These tests mount the real widget under the macOS theme and read
//! what was painted.

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
use teksilo_widgets::slider::SLIDER_PART_THUMB;
use teksilo_widgets::{Slider, VStack};

/// The knob's diameter (dp), `MacOsSliderStyle`'s.
const KNOB: f32 = 18.0;
/// The track's thickness (dp).
const TRACK: f32 = 4.0;
/// The slider's height (dp).
const LENGTH: f32 = 200.0;
/// The type name of the node the style paints.
const BODY: &str = "MacOsSliderBody";
/// The type name of the widget that maps the pointer and the keys.
const SLIDER: &str = "Slider";

fn themes() -> [(&'static str, Theme); 2] {
    [
        ("Aqua", teksilo_theme_macos::light()),
        ("Dark Aqua", teksilo_theme_macos::dark()),
    ]
}

fn rgba8(c: [f32; 4]) -> [u8; 4] {
    c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// A mounted vertical slider, `LENGTH` dp tall.
struct Mounted {
    tree: WidgetTree,
    /// The `Slider` itself.
    slider: WidgetId,
    /// The bounds of the node the style paints.
    body: Rect,
    value: Signal<f32>,
}

/// A tree holding a vertical slider at `value` (0 to 1) laid out in
/// `direction`.
fn mount(theme: &Theme, value: f32, direction: LayoutDirection) -> Mounted {
    let value = Signal::new(value);
    let mut tree = WidgetTree::new()
        .with_theme(theme.clone())
        .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
    tree.set_layout_direction(direction);
    let slider = Slider::new(value.clone(), 0.0, 1.0)
        .orientation(Orientation::Vertical)
        .jump_on_press(true);
    let root = tree.add(VStack::new().child(FixedSize::new().height(LENGTH).child(slider)));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let body = tree.bounds(only(&tree, root, BODY));
    let slider = only(&tree, root, SLIDER);
    Mounted {
        tree,
        slider,
        body,
        value,
    }
}

/// The one node named `name` under `root`.
fn only(tree: &WidgetTree, root: WidgetId, name: &str) -> WidgetId {
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
    assert_eq!(out.len(), 1, "one {name}");
    out[0]
}

fn rerender(tree: &mut WidgetTree) -> Rc<RenderFrame> {
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.render()
}

/// The centre of the thumb the widget reports, the one its pointer mapping
/// and its value readout agree with.
fn widget_thumb_centre(m: &Mounted) -> Point {
    let regions = m.tree.widget_target_regions(m.slider);
    let thumb = regions
        .iter()
        .find(|r| r.part == SLIDER_PART_THUMB)
        .expect("the slider reports its thumb");
    Point::new(
        thumb.rect.x + thumb.rect.width * 0.5,
        thumb.rect.y + thumb.rect.height * 0.5,
    )
}

/// The centre of the painted knob: the one filled disc of its diameter.
fn knob_centre(frame: &RenderFrame) -> Point {
    let discs: Vec<_> = frame
        .shapes
        .iter()
        .filter(|s| s.shape == ShapeKind::Circle && s.stroke_width == 0.0)
        .filter(|s| (s.screen[2] - KNOB).abs() < 1e-3)
        .collect();
    assert_eq!(discs.len(), 1, "one knob face in {:?}", frame.shapes);
    let s = discs[0].screen;
    Point::new(s[0] + s[2] * 0.5, s[1] + s[3] * 0.5)
}

/// The painted fill, the accent part of the track, `[x, y, width, height]`;
/// `None` when nothing is filled.
fn fill_rect(frame: &RenderFrame, theme: &Theme) -> Option<[f32; 4]> {
    let accent = rgba8(theme.colors.accent.to_array());
    let fills: Vec<_> = frame
        .shapes
        .iter()
        .filter(|s| s.shape != ShapeKind::Circle && s.stroke_width == 0.0)
        .filter(|s| rgba8(s.color) == accent)
        .filter(|s| (s.screen[2] - TRACK).abs() < 1e-3)
        .collect();
    assert!(fills.len() <= 1, "at most one fill in {:?}", frame.shapes);
    fills.first().map(|s| s.screen)
}

/// At every value, and in either direction, the knob is painted where the
/// widget reports its thumb: the maximum at the top, the minimum at the
/// bottom.
#[test]
fn the_painted_knob_is_the_thumb_the_widget_reports() {
    let half = KNOB * 0.5;
    for (scheme, theme) in themes() {
        for direction in [LayoutDirection::LeftToRight, LayoutDirection::RightToLeft] {
            for value in [0.0_f32, 0.25, 0.5, 0.8, 1.0] {
                let mut m = mount(&theme, value, direction);
                assert_eq!(m.body.height, LENGTH, "{scheme}");
                assert_eq!(m.tree.bounds(m.slider), m.body, "{scheme}");
                let knob = knob_centre(&rerender(&mut m.tree));
                let thumb = widget_thumb_centre(&m);
                assert!(
                    (knob.x - thumb.x).abs() < 1e-3 && (knob.y - thumb.y).abs() < 1e-3,
                    "{scheme} {direction:?} at {value}: the knob is painted at \
                     {knob:?}, the widget's thumb is at {thumb:?}"
                );
                let from_bottom = m.body.y + m.body.height - knob.y;
                assert!(
                    (from_bottom - half - (LENGTH - KNOB) * value).abs() < 1e-3,
                    "{scheme} {direction:?} at {value}: the minimum is at the \
                     bottom, the knob {from_bottom} from it"
                );
            }
        }
    }
}

/// The fill runs from the bottom end of the track, the minimum's, up to the
/// knob.
#[test]
fn the_fill_rises_from_the_bottom_to_the_knob() {
    let half = KNOB * 0.5;
    for (scheme, theme) in themes() {
        let mut m = mount(&theme, 0.0, LayoutDirection::LeftToRight);
        assert_eq!(
            fill_rect(&rerender(&mut m.tree), &theme),
            None,
            "{scheme}: nothing is filled at the minimum"
        );
        for value in [0.25_f32, 0.5, 0.8, 1.0] {
            let mut m = mount(&theme, value, LayoutDirection::LeftToRight);
            let frame = rerender(&mut m.tree);
            let knob = knob_centre(&frame);
            let fill = fill_rect(&frame, &theme).expect("a fill");
            let bottom = m.body.y + m.body.height - half;
            assert!(
                (fill[1] + fill[3] - bottom).abs() < 1e-3,
                "{scheme} at {value}: the fill {fill:?} ends at the track's \
                 bottom end {bottom}"
            );
            assert!(
                (fill[1] - knob.y).abs() < 1e-3,
                "{scheme} at {value}: the fill {fill:?} reaches the knob at {}",
                knob.y
            );
            assert!(
                (fill[3] - (LENGTH - KNOB) * value).abs() < 1e-3,
                "{scheme} at {value}: the fill is as long as the value"
            );
        }
    }
}

/// A press near the top sets a value near the maximum, and the knob is then
/// painted under the pointer; a drag down lowers the value and the knob
/// follows.
#[test]
fn a_press_near_the_top_puts_the_knob_under_the_pointer_near_the_maximum() {
    let half = KNOB * 0.5;
    let usable = LENGTH - KNOB;
    for (scheme, theme) in themes() {
        let mut m = mount(&theme, 0.5, LayoutDirection::LeftToRight);
        rerender(&mut m.tree);

        let at = Point::new(
            m.body.x + m.body.width * 0.5,
            m.body.y + half + usable * 0.1,
        );
        m.tree.pointer_move(at);
        m.tree.pointer_down_button(at, PointerButton::Primary);
        let v = m.value.get();
        assert!(
            (v - 0.9).abs() < 1e-3,
            "{scheme}: a press a tenth of the way down is 90 %, {v}"
        );
        let knob = knob_centre(&rerender(&mut m.tree));
        assert!(
            (knob.y - at.y).abs() < 0.01,
            "{scheme}: the knob at {} is under the pointer at {}",
            knob.y,
            at.y
        );

        // The first move past the slop starts the drag at the press; the
        // second moves it, to 70 %, on the 1 % grid a stepless slider snaps
        // to.
        let to = Point::new(at.x, at.y + usable * 0.2);
        m.tree.pointer_move(Point::new(at.x, at.y + usable * 0.05));
        m.tree.pointer_move(to);
        let lowered = m.value.get();
        assert!(
            lowered < v,
            "{scheme}: a downward drag lowers, {v} to {lowered}"
        );
        let knob = knob_centre(&rerender(&mut m.tree));
        assert!(
            (knob.y - to.y).abs() < 0.01,
            "{scheme}: the knob at {} follows the pointer to {}",
            knob.y,
            to.y
        );
        m.tree.pointer_up_button(to, PointerButton::Primary);
    }
}

/// `ArrowUp` raises the value and moves the painted knob up, the fill
/// growing from the same bottom end; `ArrowDown` the opposite.
#[test]
fn arrow_up_moves_the_painted_knob_up() {
    for (scheme, theme) in themes() {
        let mut m = mount(&theme, 0.5, LayoutDirection::LeftToRight);
        rerender(&mut m.tree);
        m.tree.press_key(Key::Tab, Modifiers::NONE);
        let frame = rerender(&mut m.tree);
        let start = knob_centre(&frame);
        let start_fill = fill_rect(&frame, &theme).expect("a fill");

        m.tree.press_key(Key::ArrowUp, Modifiers::NONE);
        assert!(
            m.value.get() > 0.5,
            "{scheme}: ArrowUp raises, {}",
            m.value.get()
        );
        let frame = rerender(&mut m.tree);
        let up = knob_centre(&frame);
        assert!(up.y < start.y, "{scheme}: and moves the knob up");
        let fill = fill_rect(&frame, &theme).expect("a fill");
        assert!(fill[3] > start_fill[3], "{scheme}: the fill grows");
        assert!(
            (fill[1] + fill[3] - (start_fill[1] + start_fill[3])).abs() < 1e-3,
            "{scheme}: from the same bottom end"
        );

        m.tree.press_key(Key::ArrowDown, Modifiers::NONE);
        m.tree.press_key(Key::ArrowDown, Modifiers::NONE);
        assert!(
            m.value.get() < 0.5,
            "{scheme}: ArrowDown lowers, {}",
            m.value.get()
        );
        let down = knob_centre(&rerender(&mut m.tree));
        assert!(down.y > start.y, "{scheme}: and moves the knob down");
    }
}
