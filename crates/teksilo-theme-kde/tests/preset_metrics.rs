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
use teksilo_core::styles::Theme;
use teksilo_core::widget::Widget;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;
use teksilo_target_conformance::in_tappable_row;
use teksilo_theme_kde::geometry::BreezeGeometry;
use teksilo_theme_kde::{self as kde};
use teksilo_tokens::TargetDensity;
use teksilo_widgets::primitives::IconWidget;
use teksilo_widgets::{Button, ButtonVariant, IconButton, IconButtonSize, MenuItem, SplitButton};

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
