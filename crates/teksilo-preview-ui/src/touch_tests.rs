// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The previewer under a finger: the density switch, the knob form inside its
//! scroller, and the canvas zoom.
//!
//! None of these registers a catalog entry. That is deliberate: the widget
//! catalog is populated by `inventory` only when some crate in the build graph
//! turns on `teksilo-widgets/preview`, which `cargo test -p teksilo-preview-ui`
//! does not and `cargo test --workspace` does — so a test that read the registry
//! would assert different things depending on how it was invoked. Each fixture
//! here builds the real UI piece it is about.

#![cfg(test)]

use std::cell::Cell;

use teksilo_canvas::{Point, Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_preview::{KnobSpec, KnobValues};
use teksilo_tokens::TargetDensity;

use crate::app_state::{AppState, PreviewerRoot};

const W: f32 = 900.0;
const H: f32 = 600.0;

fn tree() -> WidgetTree {
    WidgetTree::new().with_theme(teksilo_core::presets::intui::light())
}

fn layout(tree: &mut WidgetTree) {
    tree.layout(SizeProposal::exact(W, H));
}

// ---------------------------------------------------------------------------
// the density switch
// ---------------------------------------------------------------------------

/// The previewer's root binds its density at `BindingLevel::Rebuild`, because a
/// density decides dimensions inside `build()` and a relayout cannot re-bake
/// them. Proof: flipping the signal replaces the root's child subtree.
///
/// And it is a rebuild, not a transition: nothing animates across a density
/// switch, at either end of it.
#[test]
fn the_density_switch_rebuilds_the_previewer_and_animates_nothing() {
    let mut t = tree();
    let root = t.add(PreviewerRoot::new(None, None));
    layout(&mut t);

    let state = previewer_state(&t, root);
    assert_eq!(
        state.density.get(),
        TargetDensity::Compact,
        "the previewer starts Compact"
    );
    let before = t.children(root);
    assert_eq!(t.active_animation_count(), 0, "nothing is animating yet");

    // What the toolbar's handler does, in the order it does it: the app layer
    // applies the re-projected theme, then the rebuild runs against it.
    let projected = t.theme().with_density(TargetDensity::Touch);
    t.set_theme(projected);
    state.density.set(TargetDensity::Touch);
    layout(&mut t);

    let after = t.children(root);
    assert_ne!(
        before, after,
        "a density switch must rebuild, not merely relayout: the root's children \
         are the same ids ({before:?})"
    );
    assert_eq!(
        t.active_animation_count(),
        0,
        "a density switch is instant — nothing may tween across it"
    );
    assert_eq!(
        t.theme().input.target_size,
        44.0,
        "and the tokens the rebuild reads are the Touch ones"
    );
}

/// The state, read through the `as_any` hook once the root is mounted.
fn previewer_state(t: &WidgetTree, root: WidgetId) -> AppState {
    t.widget_as_any(root)
        .and_then(|any| any.downcast_ref::<PreviewerRoot>())
        .and_then(|r| r.state().cloned())
        .expect("PreviewerRoot exposes its state through as_any once built")
}

// ---------------------------------------------------------------------------
// the knob form inside its scroller
// ---------------------------------------------------------------------------

/// A host that builds the real knob form inside the real scroller the inspector
/// pane wraps it in.
struct FormHost {
    spec: KnobSpec,
    values: KnobValues,
    root: Option<WidgetId>,
}

impl std::fmt::Debug for FormHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FormHost").finish()
    }
}

impl Widget for FormHost {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let form = crate::knob_form::build_knob_form(ctx, &self.spec, &self.values);
        let padded =
            ctx.add(teksilo_widgets::primitives::Padding::symmetric(8.0, 8.0).child_id(form));
        let scroller = ctx.add(teksilo_widgets::ScrollArea::from_id(padded));
        self.root = Some(scroller);
        vec![scroller]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root.into_iter().collect()
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}

fn find_by_type(t: &WidgetTree, id: WidgetId, suffix: &str) -> Option<WidgetId> {
    if t.widget_type_name(id).is_some_and(|n| n.ends_with(suffix)) {
        return Some(id);
    }
    t.children(id)
        .into_iter()
        .find_map(|child| find_by_type(t, child, suffix))
}

fn form_fixture() -> (WidgetTree, WidgetId, teksilo_core::signal::Signal<f32>) {
    let spec = KnobSpec::new().f32_("v", "Value", 0.5, 0.0, 1.0);
    let values = KnobValues::from_spec(&spec, None);
    let knob = values.f32_("v");
    let mut t = tree();
    let host = t.add(FormHost {
        spec,
        values,
        root: None,
    });
    layout(&mut t);
    (t, host, knob)
}

/// A `Slider` in a scrolling form is a continuous manipulator inside a pan
/// claimant, and the finger belongs to the slider. Nothing is declared here to
/// make that true: `Slider` already declares `touch_action(NONE)` and takes the
/// press's capture (`teksilo-widgets/src/slider.rs`), and this is the test that
/// the *composition* holds — a second declaration at the call site would be one
/// mechanism tested and one hoped for.
#[test]
fn a_finger_drags_a_knob_slider_instead_of_scrolling_the_form() {
    let (mut t, host, knob) = form_fixture();
    let slider = find_by_type(&t, host, "Slider").expect("the form built a Slider");
    let scroller = find_by_type(&t, host, "ScrollArea").expect("the form is in a ScrollArea");
    let bounds = t.bounds(slider);
    assert!(
        bounds.width > 40.0,
        "the fixture needs a slider wide enough to drag along: {bounds:?}"
    );
    let before = knob.get();

    // Horizontally across the slider — the axis its value lives on.
    let from = Point::new(bounds.x + bounds.width * 0.2, bounds.center().y);
    let to = Point::new(bounds.x + bounds.width * 0.8, bounds.center().y);
    let finger = t.new_contact();
    t.touch_down(finger, from);
    t.touch_move(finger, Point::new(from.x + 20.0, from.y));
    t.touch_move(finger, to);
    t.touch_up(finger, to);

    assert!(
        knob.get() > before,
        "the finger must move the knob's value: {before} → {}",
        knob.get()
    );
    let _ = scroller;
    t.assert_no_leaked_pointer_state();
}

/// The declaration the composition rests on, asserted where a reader will look
/// for it: the effective touch action over a knob slider forbids every default,
/// so the form's own pan claim cannot take the gesture.
#[test]
fn a_knob_slider_leaves_no_default_touch_behaviour_over_itself() {
    let (t, host, _knob) = form_fixture();
    let slider = find_by_type(&t, host, "Slider").expect("the form built a Slider");
    assert_eq!(
        t.touch_action_for(slider),
        teksilo_core::pointer::touch_action::TouchAction::NONE,
        "a slider inside a scroller must forbid the scroller's defaults"
    );
}

/// The switcher, end to end: pressing **Touch** in the toolbar re-projects the
/// theme, rebuilds, and the chrome comes back at the Touch ladder — with
/// nothing animating across it.
///
/// The host installs the one binding `PreviewerRoot` installs (density at
/// `BindingLevel::Rebuild`, pinned by the test above) and nothing else, so what
/// is exercised here is the toolbar's own handler and the order it does its two
/// halves in: the re-projected theme first — the app layer applies a parked
/// theme before the next layout pass — then the rebuild, which reads it.
#[test]
fn pressing_touch_in_the_toolbar_switches_the_density_and_regrows_the_chrome() {
    struct ToolbarHost {
        root: Option<WidgetId>,
        state: AppState,
    }
    impl std::fmt::Debug for ToolbarHost {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("ToolbarHost").finish()
        }
    }
    impl Widget for ToolbarHost {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            self.state.density.bind_to(
                ctx.self_id(),
                ctx.binding_registry(),
                teksilo_core::binding::BindingLevel::Rebuild,
            );
            let id = crate::toolbar::build_toolbar(ctx, &self.state);
            self.root = Some(id);
            vec![id]
        }
        fn layout_response(&self, p: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
            self.root
                .and_then(|id| ctx.child_size(id, p))
                .unwrap_or_else(|| p.resolve(0.0, 0.0))
                .into()
        }
        fn place_children(
            &self,
            b: Rect,
            _p: SizeProposal,
            kids: &mut [WidgetPlacement],
            _c: &LayoutContext,
        ) {
            for k in kids.iter_mut() {
                k.origin = b.origin();
                k.size = b.size();
            }
        }
        fn children(&self) -> Vec<WidgetId> {
            self.root.into_iter().collect()
        }
        fn accessibility(&self, _b: &mut AccessNodeBuilder) {}
    }

    /// The tallest `Button` in the toolbar. `RecipeButtonStyle` sizes its
    /// `MinSize` through `density_min_size`, so this is a real density
    /// dimension rather than a number this test invented.
    fn tallest_button(t: &WidgetTree, id: WidgetId) -> f32 {
        let mine = if t
            .widget_type_name(id)
            .is_some_and(|n| n.ends_with("Button"))
        {
            t.bounds(id).height
        } else {
            0.0
        };
        t.children(id)
            .into_iter()
            .map(|c| tallest_button(t, c))
            .fold(mine, f32::max)
    }

    let mut t = tree();
    let state = AppState::new();
    let host = t.add(ToolbarHost {
        root: None,
        state: state.clone(),
    });
    layout(&mut t);
    let compact = tallest_button(&t, host);
    assert!(
        (24.0..44.0).contains(&compact),
        "a Compact toolbar button is 24 dp tall, got {compact}"
    );
    assert_eq!(t.active_animation_count(), 0, "nothing is animating yet");

    let touch_button = t
        .find_by_label("Touch")
        .expect("the toolbar offers a Touch density button");
    t.click(touch_button);

    assert_eq!(
        state.density.get(),
        TargetDensity::Touch,
        "the handler must record the new density"
    );
    let parked = t
        .take_pending_theme_request()
        .expect("the handler must hand the app a re-projected theme");
    assert_eq!(
        parked.input.target_size, 44.0,
        "…projected onto the Touch ladder"
    );
    // What `WindowManager::set_theme` does with it.
    t.set_theme(parked);
    layout(&mut t);

    let grown = tallest_button(&t, host);
    assert!(
        grown >= 44.0,
        "the rebuilt chrome must come back at the Touch ladder: {compact} → {grown}"
    );
    assert_eq!(
        t.active_animation_count(),
        0,
        "a density switch is instant — nothing may tween across it"
    );
}

// ---------------------------------------------------------------------------
// the canvas zoom
// ---------------------------------------------------------------------------

/// A host that puts the real `ZoomStage` around a fixed-size child, so a pinch
/// can be aimed at it.
struct ZoomHost {
    zoom: teksilo_core::signal::Signal<f32>,
    root: Option<WidgetId>,
    child: Cell<Option<WidgetId>>,
}

impl std::fmt::Debug for ZoomHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZoomHost").finish()
    }
}

impl Widget for ZoomHost {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let inner = ctx.add(teksilo_widgets::primitives::MinSize::new(120.0, 40.0));
        self.child.set(Some(inner));
        let stage = ctx.add(crate::canvas::ZoomStage::new(self.zoom.clone(), inner));
        self.root = Some(stage);
        vec![stage]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root.into_iter().collect()
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}

/// Two fingers spreading over the canvas magnify it, and the previewed widget's
/// **layout** is untouched — which is what keeps the footer's size readout
/// honest at any zoom.
#[test]
fn a_pinch_on_the_canvas_zooms_it_and_moves_no_layout() {
    let zoom = teksilo_core::signal::Signal::new(100.0_f32);
    let mut t = tree();
    let host = t.add(ZoomHost {
        zoom: zoom.clone(),
        root: None,
        child: Cell::new(None),
    });
    layout(&mut t);
    let inner = find_by_type(&t, host, "MinSize").expect("the stage has a child");
    let before = t.bounds(inner);

    // Spread: both contacts start close together and end far apart.
    t.pinch(
        Point::new(430.0, 300.0),
        Point::new(470.0, 300.0),
        Point::new(330.0, 300.0),
        Point::new(570.0, 300.0),
        6,
    );

    assert!(zoom.get() > 100.0, "a spread must zoom in: {}%", zoom.get());
    layout(&mut t);
    assert_eq!(
        t.bounds(inner),
        before,
        "the zoom is visual — the previewed widget's layout must not move"
    );

    // …and it really is applied: the render walker emits the stage's transform
    // scope, magnified by the same factor. Asserting the signal alone would pass
    // with the transform never installed.
    let frame = t.render();
    let scales: Vec<f32> = frame
        .draw_order
        .iter()
        .filter_map(|c| match c {
            teksilo_canvas::DrawCommand::PushTransform(m) => Some(m.m[0]),
            _ => None,
        })
        .collect();
    let want = zoom.get() / 100.0;
    assert!(
        scales.iter().any(|s| (s - want).abs() < 1e-3),
        "the frame must carry a {want}× transform, got {scales:?}"
    );
    // …inside a clip, so a magnified preview stays in the canvas pane instead of
    // spilling over the navigator and the knob form either side of it.
    assert!(
        frame
            .draw_order
            .iter()
            .any(|c| matches!(c, teksilo_canvas::DrawCommand::SetClip(_))),
        "the zoom stage must clip what it magnifies"
    );
    t.assert_no_leaked_pointer_state();
}

/// The zoom is clamped at both ends: a pinch that keeps going stops rather than
/// collapsing the preview to nothing or blowing it up past use.
#[test]
fn the_canvas_zoom_is_clamped_at_both_ends() {
    let zoom = teksilo_core::signal::Signal::new(100.0_f32);
    let mut t = tree();
    t.add(ZoomHost {
        zoom: zoom.clone(),
        root: None,
        child: Cell::new(None),
    });
    layout(&mut t);

    for _ in 0..12 {
        t.pinch(
            Point::new(440.0, 300.0),
            Point::new(460.0, 300.0),
            Point::new(140.0, 300.0),
            Point::new(760.0, 300.0),
            4,
        );
    }
    assert!(
        zoom.get() <= 400.0 + f32::EPSILON,
        "zoom must clamp at 400 %, got {}",
        zoom.get()
    );

    for _ in 0..20 {
        t.pinch(
            Point::new(140.0, 300.0),
            Point::new(760.0, 300.0),
            Point::new(440.0, 300.0),
            Point::new(460.0, 300.0),
            4,
        );
    }
    assert!(
        zoom.get() >= 25.0 - f32::EPSILON,
        "and at 25 %, got {}",
        zoom.get()
    );
}

/// Ctrl-wheel is the mouse's way to the same control, and a *bare* wheel is
/// left alone so the pane around the stage still scrolls.
#[test]
fn ctrl_wheel_zooms_and_a_bare_wheel_does_not() {
    use teksilo_core::event::{Modifiers, ScrollDelta, WidgetEvent};

    let zoom = teksilo_core::signal::Signal::new(100.0_f32);
    let mut t = tree();
    t.add(ZoomHost {
        zoom: zoom.clone(),
        root: None,
        child: Cell::new(None),
    });
    layout(&mut t);

    let at = Point::new(W * 0.5, H * 0.5);
    t.pointer_move(at);
    t.dispatch_event(WidgetEvent::Scroll {
        delta: ScrollDelta::Lines { x: 0.0, y: 1.0 },
        modifiers: Modifiers::NONE,
        window_position: Some(at),
        phase: teksilo_core::pointer::ScrollPhase::Discrete,
        pointer: teksilo_core::pointer::PointerInfo::mouse(teksilo_core::pointer::EventTime::ZERO),
    });
    assert_eq!(
        zoom.get(),
        100.0,
        "a bare wheel belongs to whatever scrolls, not to the zoom"
    );

    t.dispatch_event(WidgetEvent::Scroll {
        delta: ScrollDelta::Lines { x: 0.0, y: 1.0 },
        modifiers: Modifiers::COMMAND,
        window_position: Some(at),
        phase: teksilo_core::pointer::ScrollPhase::Discrete,
        pointer: teksilo_core::pointer::PointerInfo::mouse(teksilo_core::pointer::EventTime::ZERO),
    });
    assert!(
        zoom.get() > 100.0,
        "the accelerator plus a wheel notch zooms in: {}%",
        zoom.get()
    );
}
