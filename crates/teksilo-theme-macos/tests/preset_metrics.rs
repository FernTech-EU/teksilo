// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A dimension this preset writes must be the dimension the widget renders.
//!
//! The unit tests beside each style check that the *recipe* carries Apple's
//! number. That is only half the claim: a recipe field nothing reads is a
//! number written and never rendered, and several of them were exactly that.
//! These tests mount the real widget under the real preset and measure the
//! laid-out geometry, so they fail when the widget stops asking the style —
//! which no assertion about a recipe can see.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{MockTextBackend, SizeProposal};
use teksilo_core::accessibility::target_audit::{
    PIN_TOLERANCE, gate::ALL_DENSITIES, measure_targets, target_audit,
};
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::signal::Signal;
use teksilo_core::styles::Theme;
use teksilo_core::widget::Widget;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_tokens::TargetDensity;
use teksilo_widgets::primitives::IconWidget;
use teksilo_widgets::{Calendar, IconButton, IconButtonSize, SearchField, Toggle};

/// Mount `w` under `theme` in a tree with a real text backend and lay it out.
fn mounted(theme: Theme, w: impl Widget + 'static, size: (f32, f32)) -> (WidgetTree, WidgetId) {
    let mut tree = WidgetTree::new()
        .with_theme(theme)
        .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
    let id = tree.add(w);
    tree.layout(SizeProposal::exact(size.0, size.1));
    (tree, id)
}

/// Every node under `root` whose `Widget::type_name` ends in `name`, in tree
/// order. Named by type rather than by index so an extra wrapper somewhere in
/// the chrome does not silently move the measurement onto another node.
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

// ── Calendar nav arrows ────────────────────────────────────────────────

/// The square footprint of each of the calendar header's four nav arrows —
/// the **painted chrome**, not the node.
///
/// The two differ whenever a preset paints under the 24 dp conformance floor:
/// the node is grown to the floor and the chrome is centred inside it, so the
/// node's own bounds would report the floor and say nothing about what the
/// preset asked for. The chrome is the `ZStack` carrying the focus ring and the
/// chevron.
fn nav_arrow_extents(theme: Theme) -> Vec<(f32, f32)> {
    let (tree, root) = mounted(theme, Calendar::single(Signal::new(None)), (400.0, 460.0));
    let arrows = nodes_named(&tree, root, "NavArrow");
    assert_eq!(arrows.len(), 4, "prev-year / prev / next / next-year");
    arrows
        .into_iter()
        .map(|a| {
            let chrome = nodes_named(&tree, a, "ZStack");
            assert_eq!(chrome.len(), 1, "one arrow, one chrome stack");
            let b = tree.bounds(chrome[0]);
            (b.width, b.height)
        })
        .collect()
}

/// `NSDatePicker`'s graphical stepper is 20 dp, which is what the macOS
/// recipe writes. The arrow used to apply `dp(24, Target, ..)` to the shipped
/// constant itself and render 24 dp whatever the theme had decided.
///
/// The IntUI arm is the control: 24 dp at Compact, which is both the shipped
/// constant and the conformance floor, and must not move.
///
/// What the arrow's *target* does with a chrome under the floor is
/// [`a_calendars_nav_arrows_clear_the_conformance_floor_at_every_density`]'s
/// subject; this one is about the chrome alone.
#[test]
fn a_calendar_sizes_its_nav_arrows_to_the_macos_stepper() {
    assert_eq!(
        nav_arrow_extents(teksilo_core::presets::intui::light()),
        vec![(24.0, 24.0); 4],
        "IntUI at Compact must be unchanged",
    );
    assert_eq!(
        nav_arrow_extents(teksilo_theme_macos::light()),
        vec![(20.0, 20.0); 4],
        "the macOS recipe's `nav_arrow_size` must reach the arrow",
    );
    assert_eq!(
        nav_arrow_extents(teksilo_theme_macos::dark()),
        vec![(20.0, 20.0); 4],
        "both appearances install the same metrics",
    );
}

// ── SearchField suggestion rows ────────────────────────────────────────

/// The `(horizontal, vertical)` gutter inside a suggestion row, measured on an
/// open suggestions popover.
///
/// Measured as the difference between the row's padding box and the label
/// inside it, rather than as an absolute coordinate, because the popover is a
/// separately-positioned overlay and the row's own x/y say nothing about its
/// inset.
fn suggestion_row_gutters(theme: Theme) -> (f32, f32) {
    let mut tree = WidgetTree::new()
        .with_theme(theme)
        .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
    let query = Signal::new(String::new());
    tree.add(
        SearchField::new(query.clone())
            .with_suggestions(|_| vec!["alpha".to_string(), "apricot".to_string()]),
    );
    let proposal = SizeProposal::exact(400.0, 300.0);
    tree.layout(proposal);

    // The provider runs off the query signal, so the list is empty until the
    // text changes — and ArrowDown declines to open an empty popover.
    query.set("a".to_string());
    tree.layout(proposal);

    // Focus the editable surface itself: the field's opener runs in the
    // *preview* pass, which reaches strict ancestors of the focused node only,
    // so focusing the `SearchField` would skip its own handler.
    let mut fields = Vec::new();
    for root in tree.roots() {
        fields.extend(nodes_named(&tree, root, "TextInputField"));
    }
    tree.focus(*fields.first().expect("the field has an editable surface"));
    tree.press_key(Key::ArrowDown, Modifiers::default());
    tree.layout(proposal);
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "ArrowDown must raise the suggestions popover",
    );

    let mut rows = Vec::new();
    for root in tree.roots() {
        rows.extend(nodes_named(&tree, root, "SuggestionRow"));
    }
    assert_eq!(rows.len(), 2, "two suggestions, two rows");
    let paddings = nodes_named(&tree, rows[0], "Padding");
    assert_eq!(paddings.len(), 1, "a row wraps its label in one Padding");
    let pad = tree.bounds(paddings[0]);
    let label = tree.children(paddings[0]);
    assert_eq!(label.len(), 1, "a Padding has one child");
    let label = tree.bounds(label[0]);
    (
        (pad.width - label.width) / 2.0,
        (pad.height - label.height) / 2.0,
    )
}

/// `NSSearchField`'s results row is tighter than IntUI's — 8 dp across and
/// 3 dp down — which is what the macOS recipe writes. The panel used to read
/// the shipped 10 / 4 constants, so the preset's numbers never rendered.
///
/// The IntUI arm is the control and must stay 10 / 4 at Compact.
#[test]
fn a_search_field_pads_its_suggestion_rows_by_the_macos_gutter() {
    assert_eq!(
        suggestion_row_gutters(teksilo_core::presets::intui::light()),
        (10.0, 4.0),
        "IntUI at Compact must be unchanged",
    );
    assert_eq!(
        suggestion_row_gutters(teksilo_theme_macos::light()),
        (8.0, 3.0),
        "the macOS recipe's `row_padding_*` must reach the row",
    );
}

/// The accessor half of the same mechanism, checked directly on the installed
/// slots: what a widget asking the theme is told.
#[test]
fn the_installed_slots_report_the_macos_metrics() {
    let theme = teksilo_theme_macos::light();

    let calendar = theme
        .style_slots
        .calendar
        .clone()
        .expect("the preset installs a calendar slot");
    assert_eq!(calendar.nav_arrow_size(&theme.input), 20.0);

    let search = theme
        .style_slots
        .search_field
        .clone()
        .expect("the preset installs a search-field slot");
    assert_eq!(search.row_padding_horizontal(&theme.input), 8.0);
    assert_eq!(search.row_padding_vertical(&theme.input), 3.0);
}

// ── The nav arrow's target, now that its chrome is under the floor ─────

/// A calendar at `density`, and every `NavArrow` in it measured against that
/// density's ladder: `(painted width, reachable width, reachable height,
/// whether an outset was confirmed)`.
fn nav_arrow_targets(theme: Theme, density: TargetDensity) -> (usize, Vec<(f32, f32, f32, bool)>) {
    // Built *at* the density, never switched into it: a control's painted size
    // is baked in `build()`, so judging a Compact tree against the Touch ladder
    // would measure a mixture of the two.
    let mut tree = WidgetTree::new()
        .with_theme(theme.with_density(density))
        .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
    tree.add(Calendar::single(Signal::new(None)));
    tree.layout(SizeProposal::exact(600.0, 700.0));

    let failures = target_audit(&tree, density)
        .into_iter()
        .filter(|v| v.rule.is_conformance_failure())
        .count();
    let arrows = measure_targets(&tree, density)
        .into_iter()
        .filter(|m| m.widget.ends_with("NavArrow"))
        .map(|m| {
            (
                m.size.width,
                m.expanded.width,
                m.expanded.height,
                m.sources.outset,
            )
        })
        .collect();
    (failures, arrows)
}

/// Honouring Apple's 20 dp stepper must not cost the calendar its WCAG 2.2
/// SC 2.5.8 conformance.
///
/// The density rule forbids answering a sub-24 dp preset value by raising the
/// paint, so the arrow keeps its 20 dp chrome and its **box** reaches the 24 dp
/// floor with the chrome centred inside — the bargain `Checkbox` already
/// strikes between `box_visual_size` and `box_hit_area`.
///
/// Measured at all three densities because `min_target_conformance` is 24 dp at
/// every one of them and never scales: the floor this clears is the same number
/// each time, and it is the box rather than any hit mechanism that clears it.
#[test]
fn a_calendars_nav_arrows_clear_the_conformance_floor_at_every_density() {
    for density in [
        TargetDensity::Compact,
        TargetDensity::Comfortable,
        TargetDensity::Touch,
    ] {
        let (failures, arrows) = nav_arrow_targets(teksilo_theme_macos::light(), density);
        assert_eq!(
            arrows.len(),
            4,
            "{density:?}: prev-year / prev / next / next-year",
        );
        for (painted, _, _, _) in &arrows {
            assert!(
                *painted >= 24.0,
                "{density:?}: the arrow's node is {painted} dp — the 20 dp                  chrome must sit inside a box that reaches the conformance floor",
            );
        }
        assert_eq!(
            failures, 0,
            "{density:?}: the macOS calendar reports {failures} WCAG 2.2              SC 2.5.8 conformance failures; arrows measured {arrows:?}",
        );
    }
}

/// Above the floor, the coarse-pointer reach is the outset's job.
///
/// The box stops at 24 dp — `min_target_conformance` does not scale — so at
/// Comfortable and Touch a finger's reach past it comes from
/// `NavArrow::hit_outset` and from nothing else. This is the density at which
/// that mechanism is discriminated: at Compact the box already equals
/// `target_size`, so the outset contributes exactly zero and a test there would
/// pass its own deletion.
///
/// The reach is asserted as a *band*, not a number: an outset cannot escape its
/// parent and the outermost arrows sit flush against the header row's edge, so
/// they grow inward only, while two neighbouring arrows split the gap between
/// them. What the assertion pins is that the reach exceeds the box at all and is
/// credited to the outset.
#[test]
fn a_coarse_pointer_reaches_past_the_nav_arrows_box_above_the_floor() {
    for (density, target_size) in [
        (TargetDensity::Comfortable, 32.0_f32),
        (TargetDensity::Touch, 44.0),
    ] {
        let (_, arrows) = nav_arrow_targets(teksilo_theme_macos::light(), density);
        for (painted, width, height, outset) in &arrows {
            assert_eq!(*painted, 24.0, "{density:?}: the box is the 24 dp floor");
            assert!(
                *outset,
                "{density:?}: the reach was not credited to an outset —                  {arrows:?}",
            );
            assert_eq!(
                *height, target_size,
                "{density:?}: the vertical axis has room, so it reaches the                  density's target in full",
            );
            assert!(
                *width > 24.0,
                "{density:?}: the horizontal reach is {width} dp, no more than                  the box — the outset bought nothing",
            );
            assert!(
                *width <= target_size,
                "{density:?}: the reach {width} dp exceeds the target it was                  growing towards",
            );
        }
    }
}

/// Int UI is untouched by either mechanism, at every density.
///
/// Its arrow is `dp(24, Target, ..)` — the density's target size exactly — so
/// the conformance box is not built at all (the shared helper's identity
/// early-return adds no nodes) and the outset has nothing to add. The
/// control arm for both tests above, and the programme's Compact-is-unchanged
/// invariant for this widget.
#[test]
fn the_int_ui_calendar_needs_neither_mechanism() {
    for (density, target_size) in [
        (TargetDensity::Compact, 24.0_f32),
        (TargetDensity::Comfortable, 32.0),
        (TargetDensity::Touch, 44.0),
    ] {
        let (failures, arrows) = nav_arrow_targets(teksilo_core::presets::intui::light(), density);
        assert_eq!(failures, 0, "{density:?}");
        for (painted, _, _, outset) in &arrows {
            assert_eq!(
                *painted, target_size,
                "{density:?}: Int UI paints the arrow at the density's target",
            );
            assert!(
                !outset,
                "{density:?}: a control already at `target_size` earns no                  top-up — {arrows:?}",
            );
        }
    }
}

// ── The two constants that sit under the conformance floor ─────────────

// The tappable row around each subject — what makes these tests discriminate
// (a control in a bare stack is topped up by the miss-only slop pass and
// passes its own deletion) — is `teksilo_target_conformance::in_tappable_row`,
// imported rather than copied: that helper is public precisely because "a
// second copy of this geometry is a second thing to get wrong".
use teksilo_target_conformance::in_tappable_row;

/// Apple's 22 dp control height must not cost an icon button its WCAG 2.2
/// SC 2.5.8 conformance.
///
/// `min_target_conformance` is 24 dp at every density and never scales, so a
/// recipe that pins a size below it is below it at Touch as much as at Compact
/// — which is why this sweeps all three rather than only the densest. Neither
/// hit mechanism can close the gap: an outset never escapes its parent, and the
/// slop pass gives nothing to a control whose bubble path carries an eligible
/// handler, which a toolbar row is.
#[test]
fn a_macos_icon_button_reaches_the_conformance_floor_at_every_density() {
    // Both sub-floor rungs, because the two regress independently: the default
    // 22 dp control and the 18 dp `IconButtonSize::Compact` — the rung the §0
    // density-inventory row says gains 6 dp, which no gate fixture builds.
    for size in [IconButtonSize::Default, IconButtonSize::Compact] {
        for &density in ALL_DENSITIES {
            let mut tree = WidgetTree::new()
                .with_theme(teksilo_theme_macos::light().with_density(density))
                .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
            in_tappable_row(
                &mut tree,
                IconButton::new(IconWidget::checkmark(12.0)).size(size),
            );
            tree.layout(SizeProposal::exact(400.0, 200.0));

            let button = measure_targets(&tree, density)
                .into_iter()
                .find(|m| m.widget.ends_with("IconButton") && m.part.is_none())
                .unwrap_or_else(|| panic!("{density:?}: the row holds an icon button"));
            // The floor the walker judged this row against, off the row itself
            // — never `InputTokens::for_density`, which answers for the generic
            // ladder rather than for the theme under measurement.
            let floor = button.conformance_floor;
            assert!(
                button.expanded.width + PIN_TOLERANCE >= floor
                    && button.expanded.height + PIN_TOLERANCE >= floor,
                "{density:?}: a macOS {size:?} icon button reaches {:?} against a \
                 {floor} dp floor",
                button.expanded,
            );
            assert_eq!(
                target_audit(&tree, density)
                    .into_iter()
                    .filter(|v| v.rule.is_conformance_failure())
                    .count(),
                0,
                "{density:?}: the row reports a conformance failure at {size:?}",
            );
        }
    }
}

/// And the chrome is still Apple's 22 dp, so "clears the floor" has not been
/// implemented by falsifying the preset.
///
/// Measured on the laid-out tree rather than on the recipe: a recipe field
/// nothing renders is the failure this whole file exists to catch. The two
/// `FixedSize` nodes in an icon button's chrome are the conformance box and the
/// painted square inside it.
#[test]
fn a_macos_icon_buttons_chrome_is_still_apples_twenty_two_dp() {
    // (rung, painted square) — the compact rung keeps its 18 dp square inside
    // the same 24 dp box, so "clears the floor" has not resized either chrome.
    for (size, painted) in [
        (IconButtonSize::Default, 22.0),
        (IconButtonSize::Compact, 18.0),
    ] {
        let mut tree = WidgetTree::new()
            .with_theme(teksilo_theme_macos::light())
            .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
        let root = in_tappable_row(
            &mut tree,
            IconButton::new(IconWidget::checkmark(12.0)).size(size),
        );
        tree.layout(SizeProposal::exact(400.0, 200.0));

        let mut squares: Vec<(f32, f32)> = nodes_named(&tree, root, "FixedSize")
            .into_iter()
            .map(|id| {
                let b = tree.bounds(id);
                (b.width, b.height)
            })
            .collect();
        squares.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(
            squares,
            vec![(painted, painted), (24.0, 24.0)],
            "the painted square stays at Apple's number for {size:?} and sits \
             centred in a box that reaches the floor",
        );
    }
}

/// The same bargain for `NSSwitch`: the track keeps Apple's 22 dp and the node
/// clears the floor.
#[test]
fn a_macos_toggle_reaches_the_conformance_floor_at_every_density() {
    for &density in ALL_DENSITIES {
        let mut tree = WidgetTree::new()
            .with_theme(teksilo_theme_macos::light().with_density(density))
            .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
        in_tappable_row(&mut tree, Toggle::new(Signal::new(true)));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        let toggle = measure_targets(&tree, density)
            .into_iter()
            .find(|m| m.widget.ends_with("Toggle") && m.part.is_none())
            .unwrap_or_else(|| panic!("{density:?}: the row holds a toggle"));
        // The floor the walker judged this row against, off the row itself —
        // never `InputTokens::for_density`, which answers for the generic
        // ladder rather than for the theme under measurement.
        let floor = toggle.conformance_floor;
        assert!(
            toggle.expanded.width + PIN_TOLERANCE >= floor
                && toggle.expanded.height + PIN_TOLERANCE >= floor,
            "{density:?}: a macOS switch reaches {:?} against a {floor} dp floor",
            toggle.expanded,
        );
        // Both axes, and the gate's own verdict as well — the height alone is
        // satisfied by a switch that still fails on the other axis, or by a row
        // whose other targets do. Its icon-button sibling asks the same pair.
        assert_eq!(
            target_audit(&tree, density)
                .into_iter()
                .filter(|v| v.rule.is_conformance_failure())
                .count(),
            0,
            "{density:?}: the row reports a conformance failure",
        );
    }
}

/// And the track is still 38 x 22, so the switch has not been resized to reach
/// the floor.
#[test]
fn a_macos_switch_track_is_still_thirty_eight_by_twenty_two() {
    let mut tree = WidgetTree::new()
        .with_theme(teksilo_theme_macos::light())
        .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
    tree.add(Toggle::new(Signal::new(true)));
    tree.layout(SizeProposal::exact(400.0, 200.0));

    let frame = tree.render();
    let tracks: Vec<[f32; 4]> = frame
        .shapes
        .iter()
        .map(|s| s.screen)
        .filter(|s| (s[2] - 38.0).abs() < 0.01 && (s[3] - 22.0).abs() < 0.01)
        .collect();
    assert!(
        !tracks.is_empty(),
        "the 38 x 22 track is painted; shapes were {:?}",
        frame.shapes.iter().map(|s| s.screen).collect::<Vec<_>>(),
    );
}
