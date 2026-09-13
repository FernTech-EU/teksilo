// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Every [`Widget`] hook, reached through a builder method.
//!
//! A same-node wrapper — [`WidgetWithHandlers`](crate::widget_builder::WidgetWithHandlers)
//! and the `TeksiBranch{,3,4}` sum types — takes over the wrapped widget's arena
//! node, so the tree only ever asks the wrapper. A hook the wrapper does not
//! forward therefore answers the trait's *default*, and the widget loses the
//! behaviour behind it with no error, no warning and no failing assertion
//! anywhere: the only symptom is that something stops happening, far from the
//! `.on_tap(..)` that stopped it.
//!
//! Which is why exercising a hook on a bare widget proves nothing about it. Two
//! probes here answer every hook distinctively, are then wrapped by a builder
//! method, and are asked again through the wrapper. The pure queries are asked
//! directly; the hooks that only a framework pass can call are driven by a real
//! layout / paint / accessibility pass and observed through a counter.
//!
//! The tests report *every* hook that failed to survive rather than the first,
//! so a wrapper that has drifted reads as a list of what it dropped.
//!
//! These tests catch a forward that is missing or wrong **today**. They cannot
//! catch a method added to `Widget` tomorrow — nothing here enumerates the
//! trait. That is `missing_trait_methods`' job, denied on each wrapper impl.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Canvas, EdgeInsets, Point, Rect, Size, SizeProposal};
use teksilo_tokens::{InputTokens, PointerKind, TargetDensity, TargetRole};

use crate::accessibility::AccessNodeBuilder;
use crate::build_context::BuildContext;
use crate::partition::TargetRegion;
use crate::pointer::hit_slop::HitSlop;
use crate::shortcut::{KeyStroke, Shortcut};
use crate::widget::{
    LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement, WidgetTreeView,
};
use crate::widget_builder::WidgetBuilder;
use crate::widget_builder_branching::{TeksiBranch, TeksiBranch3, TeksiBranch4};
use crate::widget_id::WidgetId;
use crate::widget_tree::WidgetTree;

// ---------------------------------------------------------------------------
// The answers
// ---------------------------------------------------------------------------

const TYPE_NAME: &str = "probe::distinctive-type-name";
const TITLE_HINT: &str = "probe-title-hint";
const RUN_PROBE_A11Y_NAME: &str = "probe-a11y-name";
const SHORTCUT_ID: &str = "__probe.declared.shortcut";
const REDIRECT_NODE: accesskit::NodeId = accesskit::NodeId(0xF00D_BEEF);
const QUERY_SIZE: Size = Size {
    width: 77.0,
    height: 33.0,
};
const REVEAL_RECT: Rect = Rect {
    x: 3.0,
    y: 5.0,
    width: 7.0,
    height: 11.0,
};
const OUTSET: EdgeInsets = EdgeInsets {
    top: 1.0,
    trailing: 2.0,
    bottom: 3.0,
    leading: 4.0,
};
const SLOP: HitSlop = HitSlop {
    radius: 9.0,
    up_to: 41.0,
};
const DISTANCE: f32 = 12.5;
const REGION_PART: u16 = 4242;

fn tokens() -> InputTokens {
    InputTokens::for_density(TargetDensity::Compact)
}

// ---------------------------------------------------------------------------
// QueryProbe — every hook whose answer is a value
// ---------------------------------------------------------------------------

/// Answers every pure-query hook with a value nothing else in the framework
/// returns, so a default answering in its place is unmistakable.
///
/// Never inserted into a tree: several of its answers (a null `WidgetId` among
/// its children, a redirect to a node nobody emitted) are deliberately
/// impossible, which is what makes them recognisable.
#[derive(Debug)]
struct QueryProbe;

impl Widget for QueryProbe {
    fn type_name(&self) -> &'static str {
        TYPE_NAME
    }

    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        QUERY_SIZE.into()
    }

    fn cacheable_layout(&self) -> bool {
        false
    }

    fn wants_after_paint(&self) -> bool {
        true
    }

    fn wants_post_paint(&self) -> bool {
        true
    }

    fn wants_descendant_redirects(&self) -> bool {
        true
    }

    fn a11y_redirect_descendant(
        &self,
        _self_id: WidgetId,
        _descendant: WidgetId,
    ) -> Option<accesskit::NodeId> {
        Some(REDIRECT_NODE)
    }

    fn accessible_title_hint(&self) -> Option<String> {
        Some(TITLE_HINT.to_string())
    }

    fn accessible_title_node(&self) -> Option<WidgetId> {
        Some(WidgetId::default())
    }

    fn initial_focus_hint(&self) -> Option<WidgetId> {
        Some(WidgetId::default())
    }

    fn context_menu_key_target(&self) -> Option<WidgetId> {
        Some(WidgetId::default())
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![WidgetId::default()]
    }

    fn accessibility_children(&self) -> Option<Vec<WidgetId>> {
        Some(vec![WidgetId::default()])
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn focus_reveal_rect(&self, _bounds: Rect) -> Option<Rect> {
        Some(REVEAL_RECT)
    }

    fn hit_shape(&self, _local_point: Point, _bounds: Rect) -> bool {
        false
    }

    fn hit_outset(&self, _kind: PointerKind, _tokens: &InputTokens) -> EdgeInsets {
        OUTSET
    }

    fn hit_slop(&self, _kind: PointerKind, _tokens: &InputTokens) -> Option<HitSlop> {
        Some(SLOP)
    }

    fn hit_distance(&self, _local_point: Point, _bounds: Rect) -> Option<f32> {
        Some(DISTANCE)
    }

    fn target_regions(&self, bounds: Rect) -> Vec<TargetRegion> {
        vec![TargetRegion::target(bounds, REGION_PART)]
    }

    fn preserves_children_on_rebuild(&self) -> bool {
        true
    }

    fn tooltip_has_content(&self) -> bool {
        false
    }

    fn declare_shortcuts(&self) -> Vec<Shortcut> {
        vec![
            Shortcut::new(SHORTCUT_ID)
                .name("Probe")
                .primary(KeyStroke::ctrl(crate::event::Key::L))
                .build(),
        ]
    }
}

/// Ask every pure-query hook through `widget` and name each one that did not
/// answer [`QueryProbe`]'s value.
///
/// Takes `&mut dyn Widget` so the calls go through the vtable the arena stores
/// — the same dispatch the framework performs, not an inherent method the
/// wrapper happens to expose.
fn query_hooks_lost(widget: &mut dyn Widget) -> Vec<&'static str> {
    let theme = crate::presets::intui::light();
    let ctx = LayoutContext::for_testing(&theme);
    let tk = tokens();
    let bounds = Rect::new(0.0, 0.0, 100.0, 100.0);
    let mut lost = Vec::new();

    // `as_any_mut` first: it is the only `&mut self` query, and taking it
    // before the shared borrows keeps the rest of the checks on `&*widget`.
    if widget
        .as_any_mut()
        .and_then(|a| a.downcast_mut::<QueryProbe>())
        .is_none()
    {
        lost.push("as_any_mut");
    }

    let widget: &dyn Widget = widget;

    if widget.type_name() != TYPE_NAME {
        lost.push("type_name");
    }
    let size = widget
        .layout_response(SizeProposal::exact(10.0, 10.0), &ctx)
        .size;
    if (size.width - QUERY_SIZE.width).abs() > 0.01 {
        lost.push("layout_response");
    }
    if widget.cacheable_layout() {
        lost.push("cacheable_layout");
    }
    if !widget.wants_after_paint() {
        lost.push("wants_after_paint");
    }
    if !widget.wants_post_paint() {
        lost.push("wants_post_paint");
    }
    if !widget.wants_descendant_redirects() {
        lost.push("wants_descendant_redirects");
    }
    if widget.a11y_redirect_descendant(WidgetId::default(), WidgetId::default())
        != Some(REDIRECT_NODE)
    {
        lost.push("a11y_redirect_descendant");
    }
    if widget.accessible_title_hint().as_deref() != Some(TITLE_HINT) {
        lost.push("accessible_title_hint");
    }
    if widget.accessible_title_node().is_none() {
        lost.push("accessible_title_node");
    }
    if widget.initial_focus_hint().is_none() {
        lost.push("initial_focus_hint");
    }
    if widget.context_menu_key_target().is_none() {
        lost.push("context_menu_key_target");
    }
    if widget.children().len() != 1 {
        lost.push("children");
    }
    if widget.accessibility_children().is_none() {
        lost.push("accessibility_children");
    }
    if widget
        .as_any()
        .and_then(|a| a.downcast_ref::<QueryProbe>())
        .is_none()
    {
        lost.push("as_any");
    }
    if !widget.clips_children() {
        lost.push("clips_children");
    }
    if widget.focus_reveal_rect(bounds) != Some(REVEAL_RECT) {
        lost.push("focus_reveal_rect");
    }
    if widget.hit_shape(Point::new(1.0, 1.0), bounds) {
        lost.push("hit_shape");
    }
    if widget.hit_outset(PointerKind::Touch, &tk) != OUTSET {
        lost.push("hit_outset");
    }
    if Widget::hit_slop(widget, PointerKind::Touch, &tk) != Some(SLOP) {
        lost.push("hit_slop");
    }
    if widget.hit_distance(Point::new(1.0, 1.0), bounds) != Some(DISTANCE) {
        lost.push("hit_distance");
    }
    let regions = widget.target_regions(bounds);
    if regions.len() != 1 || regions[0].part != REGION_PART || regions[0].role != TargetRole::Target
    {
        lost.push("target_regions");
    }
    if !widget.preserves_children_on_rebuild() {
        lost.push("preserves_children_on_rebuild");
    }
    if widget.tooltip_has_content() {
        lost.push("tooltip_has_content");
    }
    let declared = widget.declare_shortcuts();
    if declared.len() != 1 || declared[0].id != SHORTCUT_ID {
        lost.push("declare_shortcuts");
    }

    lost
}

// ---------------------------------------------------------------------------
// RunProbe — the hooks only a framework pass can call
// ---------------------------------------------------------------------------

/// How many times each pass-driven hook was entered.
#[derive(Default)]
struct RunLog {
    build: Cell<u32>,
    place_children: Cell<u32>,
    paint: Cell<u32>,
    after_paint: Cell<u32>,
    post_paint: Cell<u32>,
    accessibility: Cell<u32>,
}

/// Counts the hooks whose return type is `()`, which cannot be asked a question
/// and have to be observed instead. Lives in a real tree.
struct RunProbe(Rc<RunLog>);

impl std::fmt::Debug for RunProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunProbe").finish()
    }
}

impl Widget for RunProbe {
    fn build(&mut self, _ctx: &mut BuildContext) -> Vec<WidgetId> {
        self.0.build.set(self.0.build.get() + 1);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(60.0, 40.0).into()
    }

    fn place_children(
        &self,
        _bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        self.0.place_children.set(self.0.place_children.get() + 1);
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        self.0.paint.set(self.0.paint.get() + 1);
        canvas.fill_rect(bounds, teksilo_tokens::Color::RED);
    }

    fn wants_after_paint(&self) -> bool {
        true
    }

    fn after_paint(&self, _view: &WidgetTreeView<'_>, _ctx: &PaintContext) {
        self.0.after_paint.set(self.0.after_paint.get() + 1);
    }

    fn wants_post_paint(&self) -> bool {
        true
    }

    fn post_paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        self.0.post_paint.set(self.0.post_paint.get() + 1);
        canvas.fill_rect(bounds, teksilo_tokens::Color::BLUE);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        self.0.accessibility.set(self.0.accessibility.get() + 1);
        builder.set_role(accesskit::Role::Button);
        builder.set_name(RUN_PROBE_A11Y_NAME);
    }
}

/// Drive a real layout / paint / accessibility pass over `widget` and name every
/// pass-driven hook the framework never reached.
fn run_hooks_lost(log: &RunLog, widget: impl Widget + 'static) -> Vec<&'static str> {
    let mut tree = WidgetTree::new().with_theme(crate::presets::intui::light());
    tree.add(widget);
    tree.layout(SizeProposal::exact(200.0, 200.0));
    let _ = tree.render();
    let _ = tree.accessibility_tree_snapshot();

    let mut lost = Vec::new();
    if log.build.get() == 0 {
        lost.push("build");
    }
    if log.place_children.get() == 0 {
        lost.push("place_children");
    }
    if log.paint.get() == 0 {
        lost.push("paint");
    }
    if log.after_paint.get() == 0 {
        // Also the only check that `wants_after_paint` is forwarded on the
        // walker's own path: an unforwarded gate answers `false` and the hook
        // is never consulted.
        lost.push("after_paint (or its wants_after_paint gate)");
    }
    if log.post_paint.get() == 0 {
        lost.push("post_paint (or its wants_post_paint gate)");
    }
    if log.accessibility.get() == 0 {
        lost.push("accessibility");
    }
    lost
}

// ---------------------------------------------------------------------------
// WidgetWithHandlers
// ---------------------------------------------------------------------------

#[test]
fn a_builder_method_keeps_every_query_hook() {
    let mut wrapped = QueryProbe.on_tap(|_, _| {});
    let lost = query_hooks_lost(&mut wrapped);
    assert!(
        lost.is_empty(),
        "`WidgetWithHandlers` answered the trait default for {lost:?} — each is a \
         hook the wrapped widget loses the moment any builder method touches it"
    );
}

#[test]
fn a_builder_method_keeps_every_pass_driven_hook() {
    let log = Rc::new(RunLog::default());
    let lost = run_hooks_lost(&log, RunProbe(log.clone()).on_tap(|_, _| {}));
    assert!(
        lost.is_empty(),
        "the framework never reached {lost:?} on a widget a builder method wrapped"
    );
}

#[test]
fn a_builder_method_keeps_the_widgets_accessible_name() {
    // The a11y forward, seen from where it matters: the emitted AT tree, not
    // the hook's own return.
    let log = Rc::new(RunLog::default());
    let mut tree = WidgetTree::new().with_theme(crate::presets::intui::light());
    tree.add(RunProbe(log.clone()).focusable(true));
    tree.layout(SizeProposal::exact(200.0, 200.0));
    let snapshot = tree.accessibility_tree_snapshot();
    assert!(
        snapshot
            .nodes
            .iter()
            .any(|(_, node)| node.label() == Some(RUN_PROBE_A11Y_NAME)),
        "a wrapped widget's `accessibility` must still reach the AT tree"
    );
}

#[test]
fn the_wrapper_still_yields_its_own_handler_set() {
    // `take_handler_set` is the one method the wrapper answers for itself
    // rather than forwarding — the handlers a builder method just attached are
    // what the arena has to receive.
    let mut wrapped = QueryProbe.focusable(true);
    let taken = Widget::take_handler_set(&mut wrapped);
    assert!(
        taken.is_some_and(|hs| hs.focusable == Some(true)),
        "the wrapper must hand the arena the handler set its builder methods built"
    );
}

// ---------------------------------------------------------------------------
// TeksiBranch{,3,4}
// ---------------------------------------------------------------------------

#[test]
fn a_branch_keeps_every_query_hook_of_its_active_arm() {
    // A different arm per width, so the delegation is checked on a variant that
    // is not simply the first.
    let mut two = TeksiBranch::<QueryProbe, QueryProbe>::R(QueryProbe);
    let lost = query_hooks_lost(&mut two);
    assert!(lost.is_empty(), "TeksiBranch dropped {lost:?}");

    let mut three = TeksiBranch3::<QueryProbe, QueryProbe, QueryProbe>::B(QueryProbe);
    let lost = query_hooks_lost(&mut three);
    assert!(lost.is_empty(), "TeksiBranch3 dropped {lost:?}");

    let mut four = TeksiBranch4::<QueryProbe, QueryProbe, QueryProbe, QueryProbe>::D(QueryProbe);
    let lost = query_hooks_lost(&mut four);
    assert!(lost.is_empty(), "TeksiBranch4 dropped {lost:?}");
}

#[test]
fn a_branch_keeps_every_pass_driven_hook_of_its_active_arm() {
    let log = Rc::new(RunLog::default());
    let lost = run_hooks_lost(
        &log,
        TeksiBranch::<RunProbe, RunProbe>::R(RunProbe(log.clone())),
    );
    assert!(
        lost.is_empty(),
        "the framework never reached {lost:?} on a widget inside a TeksiBranch"
    );
}
