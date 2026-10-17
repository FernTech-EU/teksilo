// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A dimension this preset writes must be the dimension the widget renders,
//! and at the Compact density it must be the one Plasma's Breeze reports.
//!
//! The unit tests beside each style check the numbers the styles are built
//! with. These mount the real widgets under the real preset, at every
//! density, inside a tappable row (the geometry that denies the miss-only
//! slop pass, so a target too small fails rather than being topped up), and
//! measure the laid-out nodes. The Compact sizes are checked against what Qt
//! measured for Breeze at Plasma's default font
//! (`tools/kde_golden/vectors/sizes.json`); this file reads that file from
//! the repository, so it is left out of the published package (the
//! `exclude` list in `Cargo.toml`).

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::Value;
use teksilo_canvas::{MockTextBackend, SizeProposal};
use teksilo_core::accessibility::target_audit::{
    PIN_TOLERANCE, gate::ALL_DENSITIES, measure_targets, target_audit,
};
use teksilo_core::signal::Signal;
use teksilo_core::styles::Theme;
use teksilo_core::widget::Widget;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;
use teksilo_target_conformance::in_tappable_row;
use teksilo_theme_kde::geometry::BreezeGeometry;
use teksilo_theme_kde::{self as kde};
use teksilo_tokens::TargetDensity;
use teksilo_widgets::primitives::{FixedSize, IconWidget};
use teksilo_widgets::{
    Button, ButtonVariant, Checkbox, ComboBox, IconButton, IconButtonSize, MenuItem, MenuList,
    RadioButton, Slider, SpinBox, SplitButton, TextInput, Toggle,
};

const SIZES_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/kde_golden/vectors/sizes.json"
));

/// The size hint Qt reported for the Breeze control `id` labelled `text`
/// (or unlabelled) at Plasma's default Noto Sans 10.
fn qt_size(id: &str, text: Option<&str>) -> (f32, f32) {
    let all: Value = serde_json::from_str(SIZES_JSON).expect("sizes.json parses");
    let widget = all["widgets"]
        .as_array()
        .expect("the measured widgets")
        .iter()
        .find(|w| {
            w["id"] == id
                && w["font"] == "Noto Sans 10"
                && text.is_none_or(|t| w["config"]["text"] == t)
        })
        .unwrap_or_else(|| panic!("sizes.json has no {id}"));
    let hint = &widget["size_hint"];
    (
        hint[0].as_f64().unwrap() as f32,
        hint[1].as_f64().unwrap() as f32,
    )
}

/// A tree under `theme` at `density` holding `w` in a tappable row, laid out.
fn in_a_row(
    theme: &Theme,
    density: TargetDensity,
    w: impl Widget + 'static,
) -> (WidgetTree, WidgetId) {
    let mut tree = WidgetTree::new()
        .with_theme(theme.with_density(density))
        .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
    let root = in_tappable_row(&mut tree, w);
    tree.layout(SizeProposal::exact(600.0, 200.0));
    (tree, root)
}

/// Every node under `root` whose type name is `name`, in tree order.
fn nodes_named(tree: &WidgetTree, root: WidgetId, name: &str) -> Vec<WidgetId> {
    let mut out = Vec::new();
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
    walk(tree, root, name, &mut out);
    out
}

/// The laid-out `(width, height)` of the one `name` node in a row holding
/// `w`, built at `density`, and whether the row passes the conformance gate.
fn measured(
    theme: &Theme,
    density: TargetDensity,
    name: &str,
    w: impl Widget + 'static,
) -> ((f32, f32), usize) {
    let (tree, root) = in_a_row(theme, density, w);
    let nodes = nodes_named(&tree, root, name);
    assert_eq!(nodes.len(), 1, "one {name} in the row");
    let b = tree.bounds(nodes[0]);
    let failures = target_audit(&tree, density)
        .into_iter()
        .filter(|v| v.rule.is_conformance_failure())
        .count();
    ((b.width, b.height), failures)
}

fn themes() -> [Theme; 2] {
    [kde::light(), kde::dark()]
}

// ── Push buttons ───────────────────────────────────────────────────────

/// A push button is 84 x 34 at Compact, Qt's size for Breeze's "OK" and
/// "Cancel" buttons, and keeps that size whether it is the default button or
/// a flat one. Above Compact its height rises to the density's target size
/// (34 already clears Comfortable's 32, and Touch makes it 44); the 84 px
/// minimum width is above every target.
///
/// The labels are short enough to sit inside the minimum width under the
/// mock text metrics, so the width measured is Breeze's own floor rather
/// than the mock's glyph widths.
#[test]
fn a_push_button_measures_breezes_size_at_every_density() {
    let compact = qt_size("push_button", Some("OK"));
    assert_eq!(compact, (84.0, 34.0));
    assert_eq!(qt_size("push_button", Some("Cancel")), compact);
    assert_eq!(qt_size("push_button.default", Some("OK")), compact);
    assert_eq!(qt_size("push_button.flat", Some("OK")), compact);
    let expected = [
        (TargetDensity::Compact, compact),
        (TargetDensity::Comfortable, (84.0, 34.0)),
        (TargetDensity::Touch, (84.0, 44.0)),
    ];
    assert_eq!(expected.len(), ALL_DENSITIES.len(), "every density");
    for (density, size) in expected {
        for theme in themes() {
            for variant in [
                ButtonVariant::Plain,
                ButtonVariant::Filled,
                ButtonVariant::Ghost,
                ButtonVariant::Destructive,
            ] {
                for label in ["OK", "Cancel"] {
                    let (got, failures) = measured(
                        &theme,
                        density,
                        "Button",
                        Button::new(lit!(label)).variant(variant),
                    );
                    assert_eq!(got, size, "{density:?} {variant:?} {label}");
                    assert_eq!(
                        failures, 0,
                        "{density:?} {variant:?}: a conformance failure"
                    );
                }
            }
        }
    }
}

/// A label wider than the minimum grows the button by Breeze's 8 px gutter
/// on each side, scaled by the density's spacing.
#[test]
fn a_long_label_grows_the_button_by_the_breeze_gutters() {
    // Under the mock metrics a character is 8 px wide.
    let label = "Apply all the changes";
    let text = label.chars().count() as f32 * 8.0;
    for (density, gutter) in [
        (TargetDensity::Compact, 8.0),
        (TargetDensity::Comfortable, 9.2),
        (TargetDensity::Touch, 10.4),
    ] {
        let ((w, _), _) = measured(&kde::light(), density, "Button", Button::new(lit!(label)));
        assert!(
            (w - (text + 2.0 * gutter)).abs() < 1e-3,
            "{density:?}: {w}, want {}",
            text + 2.0 * gutter
        );
    }
}

// ── Tool buttons ───────────────────────────────────────────────────────

/// An `IconButton` is Breeze's auto-raise tool button: 28 px around a 16 px
/// icon and 34 px around the 22 px toolbar icon at Compact, as Qt measures
/// them, rising to the density's target size above it. Every rung clears the
/// conformance floor at every density.
#[test]
fn an_icon_button_measures_breezes_tool_button_at_every_density() {
    assert_eq!(qt_size("tool_button.auto_raise.16", None), (28.0, 28.0));
    assert_eq!(qt_size("tool_button.auto_raise.22", None), (34.0, 34.0));
    let expected = [
        (TargetDensity::Compact, [24.0, 28.0, 34.0, 44.0, 60.0]),
        (TargetDensity::Comfortable, [32.0, 32.0, 34.0, 44.0, 60.0]),
        (TargetDensity::Touch, [44.0, 44.0, 44.0, 44.0, 60.0]),
    ];
    let sizes = [
        IconButtonSize::Compact,
        IconButtonSize::Default,
        IconButtonSize::Toolbar,
        IconButtonSize::Large,
        IconButtonSize::Hero,
    ];
    for (density, squares) in expected {
        for theme in themes() {
            for (size, square) in sizes.into_iter().zip(squares) {
                let (tree, _) = in_a_row(
                    &theme,
                    density,
                    IconButton::new(IconWidget::checkmark(12.0)).size(size),
                );
                let button = measure_targets(&tree, density)
                    .into_iter()
                    .find(|m| m.widget.ends_with("IconButton") && m.part.is_none())
                    .unwrap_or_else(|| panic!("{density:?}: the row holds an icon button"));
                assert!(
                    (button.size.width - square).abs() < 1e-3
                        && (button.size.height - square).abs() < 1e-3,
                    "{density:?} {size:?}: {:?}, want {square}",
                    button.size
                );
                let floor = button.conformance_floor;
                assert!(
                    button.expanded.width + PIN_TOLERANCE >= floor
                        && button.expanded.height + PIN_TOLERANCE >= floor,
                    "{density:?} {size:?}: {:?} against {floor}",
                    button.expanded
                );
                assert_eq!(
                    target_audit(&tree, density)
                        .into_iter()
                        .filter(|v| v.rule.is_conformance_failure())
                        .count(),
                    0,
                    "{density:?} {size:?}: a conformance failure",
                );
            }
        }
    }
    // The two rungs Qt measured are the geometry's.
    let g = BreezeGeometry::breeze_default();
    assert_eq!(g.tool_button_height, 28.0);
    assert_eq!(g.tool_button_height_medium, 34.0);
}

/// The framed tool button, which `IconButton` has no form of, is still
/// recorded at Qt's sizes.
#[test]
fn the_framed_tool_button_heights_are_qts() {
    let g = BreezeGeometry::breeze_default();
    assert_eq!(
        qt_size("tool_button.framed.16", None),
        (g.framed_tool_button_height, g.framed_tool_button_height)
    );
    assert_eq!(
        qt_size("tool_button.framed.22", None),
        (
            g.framed_tool_button_height_medium,
            g.framed_tool_button_height_medium
        )
    );
}

// ── Split buttons ──────────────────────────────────────────────────────

/// A `SplitButton` is framed as a push button: as tall as one at every
/// density (34, 34, 44), and at least its 84 px wide.
#[test]
fn a_split_button_is_as_tall_as_a_push_button_at_every_density() {
    for (density, height) in [
        (TargetDensity::Compact, 34.0),
        (TargetDensity::Comfortable, 34.0),
        (TargetDensity::Touch, 44.0),
    ] {
        for theme in themes() {
            for variant in [
                ButtonVariant::Plain,
                ButtonVariant::Filled,
                ButtonVariant::Ghost,
            ] {
                let ((w, h), failures) = measured(
                    &theme,
                    density,
                    "SplitButton",
                    SplitButton::new()
                        .item(MenuItem::new(lit!("Run")))
                        .variant(variant),
                );
                assert_eq!(h, height, "{density:?} {variant:?}");
                assert!(w >= 84.0, "{density:?} {variant:?}: {w} wide");
                assert_eq!(
                    failures, 0,
                    "{density:?} {variant:?}: a conformance failure"
                );
            }
        }
    }
}

// ── Check boxes and radio buttons ──────────────────────────────────────

/// The row height a check box or radio button is raised to at `density`:
/// the density's target size, which the widgets' own minimum gives them.
const INDICATOR_ROWS: [(TargetDensity, f32); 3] = [
    (TargetDensity::Compact, 24.0),
    (TargetDensity::Comfortable, 32.0),
    (TargetDensity::Touch, 44.0),
];

/// Measure the indicator control `name` in a row holding `w`: its node, the
/// leading edge of its label from the node's, the painted 15 px box from the
/// node's corner, and the number of conformance failures.
///
/// The row is laid out with no height proposed, so it takes its natural
/// height: in a taller one the widget fills the height it is given, whatever
/// the theme.
fn indicator_row(
    theme: &Theme,
    density: TargetDensity,
    name: &str,
    w: impl Widget + 'static,
) -> ((f32, f32), f32, [f32; 2], usize) {
    let mut tree = WidgetTree::new()
        .with_theme(theme.with_density(density))
        .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
    let root = in_tappable_row(&mut tree, w);
    tree.layout(SizeProposal {
        width: Some(600.0),
        height: None,
    });
    let node = nodes_named(&tree, root, name);
    assert_eq!(node.len(), 1, "one {name} in the row");
    let b = tree.bounds(node[0]);
    let label = nodes_named(&tree, node[0], "TextWidget");
    assert_eq!(label.len(), 1, "one label");
    let label_x = tree.bounds(label[0]).x - b.x;
    let failures = target_audit(&tree, density)
        .into_iter()
        .filter(|v| v.rule.is_conformance_failure())
        .count();
    let frame = tree.render();
    let painted = frame
        .shapes
        .iter()
        .find(|s| (s.screen[2] - 15.0).abs() < 0.01 && (s.screen[3] - 15.0).abs() < 0.01)
        .unwrap_or_else(|| panic!("{density:?}: no 15 px box painted"));
    let at = [painted.screen[0] - b.x, painted.screen[1] - b.y];
    ((b.width, b.height), label_x, at, failures)
}

/// D8. Qt lays a labelled Breeze check box out in a 22 px row, its 20 px
/// indicator 1 px down. The node here is the density's target size, 24 px at
/// Compact, raised by the widget's own minimum; the indicator is still
/// painted at 20 (its 15 px box at 2.5 inside it), now centred 2 px down, and
/// the label starts 24 px from the leading edge, as Qt measures it, at every
/// density.
#[test]
fn d8_a_check_box_row_is_raised_and_its_indicator_is_breezes() {
    assert_eq!(qt_size("check_box", Some("Check")).1, 22.0, "Breeze's row");
    assert_eq!(qt_size("check_box.no_text", None), (20.0, 20.0));
    let qt_contents_x = 24.0;
    for (density, row) in INDICATOR_ROWS {
        for theme in themes() {
            let ((_, h), label_x, at, failures) = indicator_row(
                &theme,
                density,
                "Checkbox",
                Checkbox::new(Signal::new(false)).label(lit!("Check")),
            );
            assert_eq!(h, row, "{density:?}");
            assert_eq!(label_x, qt_contents_x, "{density:?}: the label");
            assert_eq!(at, [2.5, (row - 20.0) * 0.5 + 2.5], "{density:?}: the box");
            assert_eq!(failures, 0, "{density:?}: a conformance failure");
        }
    }
}

/// The radio button is the check box's twin: Qt's 22 px row raised the same
/// way, the same indicator, the label at 24.
#[test]
fn d8_a_radio_button_row_is_raised_and_its_indicator_is_breezes() {
    assert_eq!(qt_size("radio_button", Some("Radio")).1, 22.0);
    assert_eq!(qt_size("radio_button.no_text", None), (20.0, 20.0));
    for (density, row) in INDICATOR_ROWS {
        for theme in themes() {
            let ((_, h), label_x, at, failures) = indicator_row(
                &theme,
                density,
                "RadioButton",
                RadioButton::new(0, Signal::new(1_usize)).label(lit!("Radio")),
            );
            assert_eq!(h, row, "{density:?}");
            assert_eq!(label_x, 24.0, "{density:?}: the label");
            assert_eq!(at, [2.5, (row - 20.0) * 0.5 + 2.5], "{density:?}: the disc");
            assert_eq!(failures, 0, "{density:?}: a conformance failure");
        }
    }
}

// ── Switches ───────────────────────────────────────────────────────────

/// The switch's 36 x 18 indicator is painted unchanged, its 36 x 12 track
/// centred, in a node raised to the density's target on both axes.
#[test]
fn a_switch_paints_its_indicator_in_a_node_at_the_target_size() {
    for (density, node) in [
        (TargetDensity::Compact, (36.0, 24.0)),
        (TargetDensity::Comfortable, (36.0, 32.0)),
        (TargetDensity::Touch, (44.0, 44.0)),
    ] {
        for theme in themes() {
            let (mut tree, root) = in_a_row(&theme, density, Toggle::new(Signal::new(true)));
            let toggle = nodes_named(&tree, root, "Toggle");
            assert_eq!(toggle.len(), 1);
            let b = tree.bounds(toggle[0]);
            assert_eq!((b.width, b.height), node, "{density:?}");
            assert_eq!(
                target_audit(&tree, density)
                    .into_iter()
                    .filter(|v| v.rule.is_conformance_failure())
                    .count(),
                0,
                "{density:?}: a conformance failure"
            );
            let frame = tree.render();
            let track = frame
                .shapes
                .iter()
                .find(|s| s.stroke_width == 0.0 && s.screen[2] == 36.0 && s.screen[3] == 12.0)
                .unwrap_or_else(|| panic!("{density:?}: no 36 x 12 track"));
            assert_eq!(
                [track.screen[0] - b.x, track.screen[1] - b.y],
                [(node.0 - 36.0) * 0.5, (node.1 - 12.0) * 0.5],
                "{density:?}: centred"
            );
        }
    }
}

// ── Text fields ────────────────────────────────────────────────────────

/// A text field's frame is 32 px at Compact, Qt's line edit, rising to the
/// density's target size above it (32 already clears Comfortable's 32, and
/// Touch makes it 44).
#[test]
fn a_text_field_measures_breezes_line_edit_at_every_density() {
    assert_eq!(qt_size("line_edit", None).1, 32.0);
    let g = BreezeGeometry::breeze_default();
    assert_eq!(g.line_edit_height, 32.0);
    for (density, height) in [
        (TargetDensity::Compact, 32.0),
        (TargetDensity::Comfortable, 32.0),
        (TargetDensity::Touch, 44.0),
    ] {
        for theme in themes() {
            let (tree, root) =
                in_a_row(&theme, density, TextInput::new(Signal::new(String::new())));
            let input = nodes_named(&tree, root, "TextInput");
            assert_eq!(input.len(), 1);
            // The framed field is the first child of the composite's root,
            // above the validation strip it reserves.
            let composite = tree.children(input[0])[0];
            let frame = tree.children(composite)[0];
            assert_eq!(tree.bounds(frame).height, height, "{density:?}");
            assert_eq!(
                target_audit(&tree, density)
                    .into_iter()
                    .filter(|v| v.rule.is_conformance_failure())
                    .count(),
                0,
                "{density:?}: a conformance failure"
            );
        }
    }
}

// ── Combo boxes ────────────────────────────────────────────────────────

/// A combo box is 32 px tall at Compact, Qt's Breeze combo box, rising to
/// the density's target size above it, with its arrow box 2 px inside the
/// trailing edge and its label 6 px inside the leading one (more at the
/// denser ladders, by their spacing).
#[test]
fn a_combo_box_measures_breezes_at_every_density() {
    assert_eq!(qt_size("combo_box", None).1, 32.0);
    let g = BreezeGeometry::breeze_default();
    assert_eq!(g.combo_box_height, 32.0);
    for (density, height, leading) in [
        (TargetDensity::Compact, 32.0, 6.0),
        (TargetDensity::Comfortable, 32.0, 6.9),
        (TargetDensity::Touch, 44.0, 7.8),
    ] {
        for theme in themes() {
            let (tree, root) = in_a_row(
                &theme,
                density,
                ComboBox::new(["Item", "Other"], Signal::new(Some("Item".to_string()))),
            );
            let layout = nodes_named(&tree, root, "BreezeComboLayout");
            assert_eq!(layout.len(), 1);
            let b = tree.bounds(layout[0]);
            assert_eq!(b.height, height, "{density:?}");
            let arrow = nodes_named(&tree, layout[0], "BreezeComboArrow");
            let a = tree.bounds(arrow[0]);
            assert_eq!(
                [b.x + b.width - (a.x + a.width), a.width, a.height],
                [2.0, 20.0, 20.0],
                "{density:?}: the arrow box"
            );
            assert_eq!(a.y - b.y, (height - 20.0) * 0.5, "{density:?}: centred");
            let label = nodes_named(&tree, layout[0], "TextWidget");
            assert!(
                (tree.bounds(label[0]).x - b.x - leading).abs() < 1e-3,
                "{density:?}: the label"
            );
            assert_eq!(
                target_audit(&tree, density)
                    .into_iter()
                    .filter(|v| v.rule.is_conformance_failure())
                    .count(),
                0,
                "{density:?}: a conformance failure"
            );
        }
    }
}

// ── Spin boxes ─────────────────────────────────────────────────────────

/// A spin box's frame is 32 px at Compact, Qt's Breeze spin box, rising to
/// the density's target size, and its arrow column is Breeze's at every
/// density: 20 px wide, 2 px inside the trailing edge, two 10 px halves
/// centred in the frame, where the step buttons press.
///
/// The halves are under the target size, at every density; the value has
/// keyboard, wheel and assistive-action equivalents, so they are the only
/// conformance failures the row reports, one per half.
#[test]
fn a_spin_box_measures_breezes_at_every_density() {
    assert_eq!(qt_size("spin_box.0_99", None).1, 32.0);
    let g = BreezeGeometry::breeze_default();
    assert_eq!(g.spin_box_height, 32.0);
    for (density, height) in [
        (TargetDensity::Compact, 32.0),
        (TargetDensity::Comfortable, 32.0),
        (TargetDensity::Touch, 44.0),
    ] {
        for theme in themes() {
            let (tree, root) = in_a_row(&theme, density, SpinBox::new(Signal::new(3_i32), 0, 10));
            let layout = nodes_named(&tree, root, "BreezeSpinLayout");
            assert_eq!(layout.len(), 1);
            let b = tree.bounds(layout[0]);
            assert_eq!(b.height, height, "{density:?}");
            let steps = nodes_named(&tree, layout[0], "StepButton");
            assert_eq!(steps.len(), 2);
            let top = (height - 20.0) * 0.5;
            for (step, y) in steps.iter().zip([top, top + 10.0]) {
                let s = tree.bounds(*step);
                assert_eq!(
                    [
                        b.x + b.width - (s.x + s.width),
                        s.y - b.y,
                        s.width,
                        s.height
                    ],
                    [2.0, y, 20.0, 10.0],
                    "{density:?}: a half"
                );
            }
            let failures: Vec<_> = target_audit(&tree, density)
                .into_iter()
                .filter(|v| v.rule.is_conformance_failure())
                .collect();
            assert_eq!(failures.len(), 2, "{density:?}: {failures:?}");
            assert!(
                failures.iter().all(|v| v.widget.ends_with("StepButton")),
                "{density:?}: {failures:?}"
            );
        }
    }
}

// ── Sliders ────────────────────────────────────────────────────────────

/// A slider is 84 px long when nothing proposes a length, Qt's size hint,
/// and 20 px thick (23 with a row of ticks), raised to the density's target
/// size across the groove: 24, 32 and 44.
#[test]
fn a_slider_measures_breezes_at_every_density() {
    assert_eq!(qt_size("slider.horizontal", None), (84.0, 20.0));
    assert_eq!(qt_size("slider.horizontal.ticks_below", None), (84.0, 23.0));
    assert_eq!(qt_size("slider.vertical", None), (20.0, 84.0));
    use teksilo_tokens::Orientation;
    for (density, cross) in [
        (TargetDensity::Compact, 24.0),
        (TargetDensity::Comfortable, 32.0),
        (TargetDensity::Touch, 44.0),
    ] {
        for theme in themes() {
            for ticks in [None, Some(5)] {
                for orientation in [Orientation::Horizontal, Orientation::Vertical] {
                    let mut slider =
                        Slider::new(Signal::new(0.5), 0.0, 1.0).orientation(orientation);
                    if let Some(n) = ticks {
                        slider = slider.tick_count(n);
                    }
                    let (tree, root) = in_a_row(&theme, density, slider);
                    let body = nodes_named(&tree, root, "BreezeSliderBody");
                    assert_eq!(body.len(), 1);
                    let b = tree.bounds(body[0]);
                    let (along, across) = match orientation {
                        Orientation::Horizontal => (b.width, b.height),
                        Orientation::Vertical => (b.height, b.width),
                    };
                    assert_eq!(across, cross, "{density:?} {orientation:?} {ticks:?}");
                    if orientation == Orientation::Horizontal {
                        assert_eq!(along, 84.0, "{density:?} {ticks:?}");
                    }
                    assert_eq!(
                        target_audit(&tree, density)
                            .into_iter()
                            .filter(|v| v.rule.is_conformance_failure())
                            .count(),
                        0,
                        "{density:?} {orientation:?}: a conformance failure"
                    );
                }
            }
        }
    }
}

// ── Menus ──────────────────────────────────────────────────────────────

/// The rows Qt laid out for Breeze's menu with icons at Plasma's default
/// font: each row's kind and its `(x, y, height)` in the menu.
fn qt_menu_rows() -> Vec<(String, (f32, f32, f32))> {
    let all: Value = serde_json::from_str(SIZES_JSON).expect("sizes.json parses");
    let menu = all["widgets"]
        .as_array()
        .expect("the measured widgets")
        .iter()
        .find(|w| w["id"] == "menu.with_icon" && w["font"] == "Noto Sans 10")
        .expect("sizes.json has the menu");
    menu["rows"]
        .as_array()
        .expect("the menu's rows")
        .iter()
        .map(|r| {
            let g = |i: usize| r["geometry"][i].as_f64().unwrap() as f32;
            (r["kind"].as_str().unwrap().to_string(), (g(0), g(1), g(3)))
        })
        .collect()
}

/// A menu's rows are 29 px at Compact, Qt's height for every Breeze menu
/// item, and rise to the density's target size above it; a separator is
/// Breeze's 5 px, its gap scaled by the density's spacing; the rows sit
/// 4 px inside the frame, the menu margin, and every label starts after
/// the 12 px margin, the 16 px icon column and 3 px more. No row fails the
/// conformance gate at any density.
#[test]
fn a_menu_measures_breezes_rows_at_every_density() {
    let qt = qt_menu_rows();
    let height_of = |kind: &str| {
        qt.iter()
            .find(|(k, _)| k == kind)
            .unwrap_or_else(|| panic!("Qt measured no {kind} row"))
            .1
    };
    for kind in ["item", "shortcut", "checkable", "submenu"] {
        assert_eq!(height_of(kind).2, 29.0, "{kind}");
    }
    assert_eq!(height_of("separator").2, 5.0);
    assert_eq!(height_of("section").2, 28.0);
    let (x, y, _) = height_of("item");
    assert_eq!((x, y), (4.0, 4.0), "Qt's menu margin");
    let compact = teksilo_theme_kde::styles::metrics::breeze_menu_metrics();
    assert_eq!(compact.section_height, height_of("section").2);

    for (density, item, separator, spacing) in [
        (TargetDensity::Compact, 29.0, 5.0, 1.0),
        (TargetDensity::Comfortable, 32.0, 5.75, 1.15),
        (TargetDensity::Touch, 44.0, 6.5, 1.3),
    ] {
        for theme in themes() {
            let menu = MenuList::new()
                .item(MenuItem::new(lit!("Open")))
                .item(MenuItem::new(lit!("Save")).shortcut_label("Ctrl+S"))
                .separator()
                .item(MenuItem::new(lit!("Checkable")).checked(Signal::new(true)))
                .item(MenuItem::submenu(lit!("Submenu"), || {
                    Box::new(MenuList::new().item(MenuItem::new(lit!("Inner"))))
                }));
            let (mut tree, root) =
                in_a_row(&theme, density, FixedSize::new().width(200.0).child(menu));
            let rows = nodes_named(&tree, root, "MenuItem");
            assert_eq!(rows.len(), 4);
            for row in &rows {
                assert_eq!(tree.bounds(*row).height, item, "{density:?}");
            }
            let sep = nodes_named(&tree, root, "MenuSeparator");
            assert!(
                (tree.bounds(sep[0]).height - separator).abs() < 1e-4,
                "{density:?}"
            );
            let frame = nodes_named(&tree, root, "BreezePopupFrame");
            let (f, first) = (tree.bounds(frame[0]), tree.bounds(rows[0]));
            assert_eq!((first.x - f.x, first.y - f.y), (4.0, 4.0), "{density:?}");
            // The first row's label.
            let rendered = tree.render();
            let label_x = rendered
                .glyphs
                .iter()
                .filter(|g| g.screen[1] >= first.y && g.screen[1] < first.bottom())
                .map(|g| g.screen[0])
                .fold(f32::INFINITY, f32::min);
            let expected = (12.0 + 3.0) * spacing + 16.0;
            assert!(
                (label_x - first.x - expected).abs() < 1e-3,
                "{density:?}: the label at {}, want {expected}",
                label_x - first.x
            );
            assert_eq!(
                target_audit(&tree, density)
                    .into_iter()
                    .filter(|v| v.rule.is_conformance_failure())
                    .count(),
                0,
                "{density:?}: a conformance failure"
            );
        }
    }
}

// ── List and tree rows ─────────────────────────────────────────────────

/// The `size_hint_for_row` Qt reported for the Breeze item view `id` at
/// Plasma's default font.
fn qt_row(id: &str) -> f32 {
    let all: Value = serde_json::from_str(SIZES_JSON).expect("sizes.json parses");
    all["widgets"]
        .as_array()
        .expect("the measured widgets")
        .iter()
        .find(|w| w["id"] == id && w["font"] == "Noto Sans 10")
        .unwrap_or_else(|| panic!("sizes.json has no {id}"))["size_hint_for_row"]
        .as_f64()
        .expect("a row height") as f32
}

/// The laid-out height of every `name` row under `root`.
fn heights(tree: &WidgetTree, root: WidgetId, name: &str) -> Vec<f32> {
    nodes_named(tree, root, name)
        .into_iter()
        .map(|id| tree.bounds(id).height)
        .collect()
}

/// A column of list rows at `density`: text only, with an icon, with a check
/// box, and with a subtitle; then the same as tree rows. Laid out with no
/// height proposed, so each row takes its own.
fn rows_at(theme: &Theme, density: TargetDensity) -> (Vec<f32>, Vec<f32>, usize) {
    use teksilo_widgets::{StandardListItem, StandardTreeItem};
    let icon = || IconWidget::checkmark(16.0);
    let column = teksilo_widgets::VStack::new()
        .child(StandardListItem::new(lit!("Text")))
        .child(StandardListItem::new(lit!("Icon")).leading_slot(icon()))
        .child(StandardListItem::new(lit!("Check")).checkbox(Signal::new(true)))
        .child(StandardListItem::new(lit!("Two")).subtitle(lit!("Lines")))
        .child(StandardTreeItem::new(lit!("Text")).depth(1))
        .child(
            StandardTreeItem::new(lit!("Icon"))
                .depth(1)
                .leading_slot(icon()),
        )
        .child(
            StandardTreeItem::new(lit!("Check"))
                .depth(1)
                .checkbox(Signal::new(true)),
        )
        .child(
            StandardTreeItem::new(lit!("Two"))
                .depth(1)
                .subtitle(lit!("Lines")),
        );
    let mut tree = WidgetTree::new()
        .with_theme(theme.with_density(density))
        .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
    let root = in_tappable_row(&mut tree, FixedSize::new().width(400.0).child(column));
    tree.layout(SizeProposal {
        width: Some(600.0),
        height: None,
    });
    let failures = target_audit(&tree, density)
        .into_iter()
        .filter(|v| v.rule.is_conformance_failure())
        .count();
    (
        heights(&tree, root, "StandardListItem"),
        heights(&tree, root, "StandardTreeItem"),
        failures,
    )
}

/// A row is Qt's at Compact: 24 px of text, 26 with a 16 px icon or a check
/// box, measured on the laid-out rows; a row with a subtitle is 42. Above
/// Compact every row rises to the density's target size, and a check box
/// row is its check box's node, itself at the target size, with the 1 px
/// item margin above and below it: 34 and 46. A tree row lays out through
/// the list row it wraps.
#[test]
fn a_row_measures_breezes_at_every_density() {
    assert_eq!(qt_row("list_view"), 24.0);
    assert_eq!(qt_row("list_view.icon"), 26.0);
    assert_eq!(qt_row("list_view.check"), 26.0);
    assert_eq!(qt_row("tree_view"), 24.0);
    assert_eq!(qt_row("tree_view.icon"), 26.0);
    let g = BreezeGeometry::breeze_default();
    assert_eq!(g.list_row_height, qt_row("list_view"));
    let expected = [
        (TargetDensity::Compact, [24.0, 26.0, 26.0, 42.0]),
        (TargetDensity::Comfortable, [32.0, 32.0, 34.0, 42.0]),
        (TargetDensity::Touch, [44.0, 44.0, 46.0, 44.0]),
    ];
    assert_eq!(expected.len(), ALL_DENSITIES.len(), "every density");
    for (density, rows) in expected {
        for theme in themes() {
            let (list, tree, failures) = rows_at(&theme, density);
            assert_eq!(list, rows.to_vec(), "{density:?}: the list rows");
            assert_eq!(tree, rows.to_vec(), "{density:?}: the tree rows");
            assert_eq!(failures, 0, "{density:?}: a conformance failure");
        }
    }
}

/// The installed row style reports what the rows lay out.
#[test]
fn the_installed_row_style_reports_the_breeze_rows() {
    for (density, floors) in [
        (TargetDensity::Compact, (24.0, 26.0, 42.0)),
        (TargetDensity::Comfortable, (32.0, 32.0, 42.0)),
        (TargetDensity::Touch, (44.0, 44.0, 44.0)),
    ] {
        let theme = kde::light().with_density(density);
        let slot = theme
            .style_slots
            .standard_item
            .clone()
            .expect("the preset installs a row style");
        let t = &theme.input;
        assert_eq!(
            (
                slot.min_height_single_line(t),
                slot.min_height_decorated(t),
                slot.min_height_two_line(t)
            ),
            floors,
            "{density:?}"
        );
    }
}

// ── Tables ─────────────────────────────────────────────────────────────

/// The laid-out `(header height, row height)` of a `TableView` and of a
/// `TreeTableView` at `density`, each measured on its hosted header and its
/// first two realized rows, the first starting under the header.
fn table_bands(theme: &Theme, density: TargetDensity) -> [(f32, f32); 2] {
    use teksilo_data::{ListModel, TreeModel};
    use teksilo_widgets::table_view::{Column, ColumnWidth};
    use teksilo_widgets::{TableView, TreeTableView};

    fn bands(tree: &WidgetTree, root: WidgetId) -> (f32, f32) {
        let header = nodes_named(tree, root, "TableHeader");
        assert_eq!(header.len(), 1, "one hosted header");
        let mut rows = nodes_named(tree, root, "BodyRow");
        rows.sort_by(|a, b| tree.bounds(*a).y.total_cmp(&tree.bounds(*b).y));
        assert!(rows.len() >= 2, "at least two realized rows");
        let h = tree.bounds(header[0]);
        let first = tree.bounds(rows[0]);
        assert_eq!(
            first.y,
            h.y + h.height,
            "the first row starts under the header"
        );
        assert_eq!(tree.bounds(rows[1]).y - first.y, first.height, "the pitch");
        (h.height, first.height)
    }
    fn column<T: 'static>(label: fn(&T) -> String) -> Column<T> {
        Column::new("name", lit!("Name"), move |row: &T, _| {
            Box::new(teksilo_widgets::primitives::TextWidget::new(lit!(label(
                row
            ))))
        })
        .width(ColumnWidth::Fixed(200.0))
    }
    let mount = |w: Box<dyn Widget>| {
        let mut tree = WidgetTree::new()
            .with_theme(theme.with_density(density))
            .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
        let root = tree.add_boxed(w);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        bands(&tree, root)
    };
    let model = ListModel::from_vec(vec!["alpha".to_string(), "beta".into(), "gamma".into()]);
    let flat = mount(Box::new(
        TableView::new(model).add_column(column(|s: &String| s.clone())),
    ));
    let model: TreeModel<String> = TreeModel::new();
    for (i, name) in ["alpha", "beta", "gamma"].into_iter().enumerate() {
        model.insert_root(i, name.to_string());
    }
    let nested = mount(Box::new(
        TreeTableView::new(model).add_column(column(|s: &String| s.clone())),
    ));
    [flat, nested]
}

/// D7. Qt's Breeze table row is 23 px with its grid line, under the 24 px
/// target size; the laid-out row is 24, the header Qt's 30, at Compact, and
/// both rise to the density's target size above it.
#[test]
fn d7_a_table_row_is_raised_to_the_target_size() {
    assert_eq!(qt_row("table_view"), 23.0, "Qt's row, with its grid line");
    assert_eq!(qt_size("header.horizontal", None).1, 30.0);
    let g = BreezeGeometry::breeze_default();
    assert_eq!(g.table_row_height, qt_row("table_view"));
    assert_eq!(g.header_height, 30.0);
    for (density, bands) in [
        (TargetDensity::Compact, (30.0, 24.0)),
        (TargetDensity::Comfortable, (32.0, 32.0)),
        (TargetDensity::Touch, (44.0, 44.0)),
    ] {
        for theme in themes() {
            assert_eq!(table_bands(&theme, density), [bands; 2], "{density:?}");
        }
        let theme = kde::light().with_density(density);
        let slot = theme.style_slots.table.clone().expect("a table style");
        assert_eq!(
            (
                slot.header_height(&theme.input),
                slot.row_height(&theme.input)
            ),
            bands,
            "{density:?}: the installed slot"
        );
    }
}

// ── Tabs ───────────────────────────────────────────────────────────────

/// A tab is Qt's 30 px at Compact, its title 8 px inside its edges, rising
/// to the density's target size above it and its padding by the spacing.
#[test]
fn a_tab_measures_breezes_at_every_density() {
    use teksilo_data::ListModel;
    use teksilo_widgets::tab_widget::{TabBar, TabDelegate, TabId, TabSizing};
    assert_eq!(qt_size("tab_bar", None).1, 30.0);
    for (density, height, padding) in [
        (TargetDensity::Compact, 30.0, 8.0),
        (TargetDensity::Comfortable, 32.0, 9.2),
        (TargetDensity::Touch, 44.0, 10.4),
    ] {
        for theme in themes() {
            let model =
                ListModel::from_vec(vec![(TabId::fresh(), "Tab"), (TabId::fresh(), "Third")]);
            let delegate = TabDelegate::new(|_, t: &(TabId, &'static str)| lit!(t.1));
            let mut tree = WidgetTree::new()
                .with_theme(theme.with_density(density))
                .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
            let bar = tree.add(
                TabBar::horizontal(model, delegate, Signal::new(None), |_, t| t.0)
                    .tab_sizing(TabSizing::Independent)
                    .show_scroll_arrows(false)
                    .show_overflow_dropdown(false),
            );
            tree.layout(SizeProposal {
                width: Some(800.0),
                height: None,
            });
            let headers = nodes_named(&tree, bar, "TabHeader");
            assert_eq!(headers.len(), 2);
            for header in headers {
                assert_eq!(tree.bounds(header).height, height, "{density:?}");
                let pad = nodes_named(&tree, header, "Padding");
                let outer = tree.bounds(pad[0]);
                let row = tree.bounds(tree.children(pad[0])[0]);
                assert!(
                    (row.x - outer.x - padding).abs() < 1e-3,
                    "{density:?}: padding {}",
                    row.x - outer.x
                );
            }
            assert_eq!(tree.bounds(bar).height, height, "{density:?}: the strip");
        }
    }
}

// ── Scroll bars and progress bars ──────────────────────────────────────

/// A scroll bar's handle is never shorter than Breeze's 20 px, at every
/// density: measured on the thumb the mounted bar reports, the rectangle its
/// hit test and its drag use, over a document fifty viewports long.
#[test]
fn a_scroll_bar_floors_its_handle_at_twenty_pixels_at_every_density() {
    use teksilo_widgets::scroll_bar::{SCROLLBAR_PART_THUMB, ScrollBar, ScrollBarOrientation};
    for density in ALL_DENSITIES.iter().copied() {
        for theme in themes() {
            let mut tree = WidgetTree::new()
                .with_theme(theme.with_density(density))
                .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
            let bar = tree.add(
                ScrollBar::new(
                    ScrollBarOrientation::Vertical,
                    Signal::new(0.0),
                    Signal::new(500.0),
                    Signal::new(0.02),
                )
                .thickness(21.0),
            );
            tree.layout(SizeProposal::exact(21.0, 400.0));
            let thumb = tree
                .widget_target_regions(bar)
                .into_iter()
                .find(|r| r.part == SCROLLBAR_PART_THUMB)
                .expect("the bar reports its thumb");
            assert_eq!(thumb.rect.height, 20.0, "{density:?}");
        }
    }
}

/// A progress bar is Qt's 26 px at every density, its 6 px groove 10 px
/// down, Qt's groove; a thickness the app sets wins.
#[test]
fn a_progress_bar_is_qts_hint_around_breezes_groove() {
    use teksilo_widgets::ProgressBar;
    assert_eq!(qt_size("progress_bar.no_text", None).1, 26.0);
    assert_eq!(qt_size("progress_bar.vertical", None).0, 26.0);
    for density in ALL_DENSITIES.iter().copied() {
        for theme in themes() {
            let ((_, h), _) = measured(&theme, density, "ProgressBar", ProgressBar::new(0.3));
            assert_eq!(h, 26.0, "{density:?}");
            let ((_, h), _) = measured(
                &theme,
                density,
                "ProgressBar",
                ProgressBar::new(0.3).thickness(6.0),
            );
            assert_eq!(h, 6.0, "{density:?}: the app's thickness");
        }
    }
}
