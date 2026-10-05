// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The device scale as a signal: a scale change relayouts the widgets bound to
//! it, and nothing else; the child of a relaid-out widget, placed at the same
//! bounds, keeps its paint.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Canvas, Rect, SizeProposal};
use teksilo_tokens::Color;

use super::WidgetTree;
use crate::WidgetId;
use crate::binding::BindingLevel;
use crate::build_context::BuildContext;
use crate::widget::{LayoutContext, PaintContext, Widget};

/// The root: binds the device scale at `Relayout`, records the scale each
/// layout of it saw, and places its one child over its whole bounds.
#[derive(Debug)]
struct ScaleProbe {
    bind: bool,
    seen: Rc<Cell<f32>>,
    layouts: Rc<Cell<u32>>,
    child: WidgetId,
}

impl Widget for ScaleProbe {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if self.bind {
            ctx.device_scale_signal().bind_to(
                ctx.self_id(),
                ctx.binding_registry(),
                BindingLevel::Relayout,
            );
        }
        vec![]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> crate::widget::LayoutResponse {
        self.seen.set(ctx.scale_factor);
        self.layouts.set(self.layouts.get() + 1);
        proposal.resolve(40.0, 40.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [crate::widget::WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
    }
}

/// Counts its paints.
#[derive(Debug)]
struct Painter {
    paints: Rc<Cell<u32>>,
}

impl Widget for Painter {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> crate::widget::LayoutResponse {
        proposal.resolve(40.0, 40.0).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        self.paints.set(self.paints.get() + 1);
        canvas.fill_rect(bounds, Color::from_rgb(0.1, 0.2, 0.3));
    }
}

struct Scene {
    tree: WidgetTree,
    seen: Rc<Cell<f32>>,
    layouts: Rc<Cell<u32>>,
    paints: Rc<Cell<u32>>,
}

fn scene(bind: bool) -> Scene {
    let mut tree = WidgetTree::new();
    let (seen, layouts, paints) = (Rc::default(), Rc::default(), Rc::default());
    let painter = tree.add(Painter {
        paints: Rc::clone(&paints),
    });
    tree.add(ScaleProbe {
        bind,
        seen: Rc::clone(&seen),
        layouts: Rc::clone(&layouts),
        child: painter,
    });
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let _ = tree.render();
    Scene {
        tree,
        seen,
        layouts,
        paints,
    }
}

#[test]
fn device_scale_signal_follows_set_device_scale_factor() {
    let mut tree = WidgetTree::new();
    let signal = tree.device_scale_signal();
    assert_eq!(signal.get(), 1.0);
    tree.set_device_scale_factor(1.5);
    assert_eq!(signal.get(), 1.5);
    let notified = Rc::new(Cell::new(0));
    let _observer = {
        let notified = Rc::clone(&notified);
        signal.observe(move |_| notified.set(notified.get() + 1))
    };
    tree.set_device_scale_factor(1.5);
    assert_eq!(notified.get(), 0, "an unchanged scale notifies nobody");
    tree.set_device_scale_factor(2.0);
    assert_eq!(notified.get(), 1);
}

#[test]
fn a_scale_change_relayouts_a_widget_bound_to_it() {
    let mut s = scene(true);
    assert_eq!(s.seen.get(), 1.0);
    let (layouts, paints) = (s.layouts.get(), s.paints.get());
    s.tree.set_device_scale_factor(2.0);
    s.tree.layout(SizeProposal::exact(400.0, 300.0));
    let _ = s.tree.render();
    assert!(s.layouts.get() > layouts, "the bound widget laid out again");
    assert_eq!(s.seen.get(), 2.0, "at the new scale");
    assert_eq!(s.paints.get(), paints, "its unbound child did not repaint");
}

#[test]
fn a_scale_change_relayouts_nothing_unbound() {
    let mut s = scene(false);
    let layouts = s.layouts.get();
    s.tree.set_device_scale_factor(2.0);
    assert!(
        !s.tree.needs_render(),
        "no global relayout: a widget that reads the scale binds the signal"
    );
    s.tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(s.layouts.get(), layouts);
}
