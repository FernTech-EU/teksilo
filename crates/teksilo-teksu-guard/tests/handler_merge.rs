// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The two silent-content-loss regressions, pinned.
//!
//! Both are reachable from a plain builder chain with no macro anywhere, which
//! is why they live beside the DSL guard rather than inside it: the DSL only
//! made them easier to hit.
//!
//! 1. A `WidgetBuilder` method with no inherent twin wraps an already-wrapped
//!    widget. Before `Widget::take_handler_set` merged recursively, the arena
//!    lifted only the outer `HandlerSet` and every handler attached before that
//!    call was dropped without a word.
//! 2. `dim_when_inactive` was the one `WidgetBuilder` method returning a real
//!    wrapper *widget* rather than `WidgetWithHandlers<Self>`. Chaining past it
//!    silently retargeted the rest of the chain at the wrapper, so a container
//!    and all but its last child vanished. It is no longer on the trait.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::SizeProposal;
use teksilo_core::widget::LayoutResponse;
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::{BuildContext, LayoutContext, Widget, WidgetId, WidgetTree};
use teksilo_tokens::PointerKind;

#[derive(Debug, Default)]
struct Leaf;

impl Leaf {
    fn new() -> Self {
        Self
    }
}

impl Widget for Leaf {
    fn layout_response(&self, p: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        p.resolve(20.0, 20.0).into()
    }
}

#[derive(Debug)]
struct Root(Option<Box<dyn Widget>>, Option<WidgetId>);

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.add_boxed(self.0.take().expect("root built once"));
        self.1 = Some(id);
        vec![id]
    }

    fn layout_response(&self, p: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        p.resolve(100.0, 100.0).into()
    }

    fn children(&self) -> Vec<WidgetId> {
        self.1.into_iter().collect()
    }
}

fn mount(w: impl Widget + 'static) -> (WidgetTree, WidgetId) {
    let mut tree = WidgetTree::new();
    let root = tree.add(Root(Some(Box::new(w)), None));
    tree.layout(SizeProposal::exact(200.0, 200.0));
    let subject = *tree
        .children(root)
        .first()
        .expect("the root mounted its child");
    (tree, subject)
}

/// A handler attached before a wrapping method must survive that method.
///
/// `clips_children_on` is the trait method whose name differs from its
/// inherent twin, so before the twin existed it re-wrapped. With the recursive
/// merge in place the handler survives either way.
#[test]
fn a_handler_survives_a_later_wrapping_method() {
    let hits = Rc::new(Cell::new(0u32));
    let sink = hits.clone();

    let (mut tree, subject) = mount(
        Leaf::new()
            .on_tap(move |_, _| sink.set(sink.get() + 1))
            .clips_children_on(true),
    );

    let at = tree.bounds(subject).center();
    tree.tap_with(PointerKind::Mouse, at);
    assert_eq!(
        hits.get(),
        1,
        "the tap handler attached before `clips_children_on` never reached the node"
    );
}

/// The same, with the wrapping method first: order must not matter.
#[test]
fn a_handler_survives_an_earlier_wrapping_method() {
    let hits = Rc::new(Cell::new(0u32));
    let sink = hits.clone();

    let (mut tree, subject) = mount(
        Leaf::new()
            .clips_children_on(true)
            .on_tap(move |_, _| sink.set(sink.get() + 1)),
    );

    let at = tree.bounds(subject).center();
    tree.tap_with(PointerKind::Mouse, at);
    assert_eq!(hits.get(), 1, "the tap handler did not reach the node");
}

/// Two handlers split across a wrap must both arrive.
#[test]
fn handlers_on_both_sides_of_a_wrap_both_arrive() {
    let taps = Rc::new(Cell::new(0u32));
    let hovers = Rc::new(Cell::new(0u32));
    let tap_sink = taps.clone();
    let hover_sink = hovers.clone();

    let (mut tree, subject) = mount(
        Leaf::new()
            .on_tap(move |_, _| tap_sink.set(tap_sink.get() + 1))
            .clips_children_on(true)
            .on_hover(move |entered, _| {
                if entered {
                    hover_sink.set(hover_sink.get() + 1)
                }
            }),
    );

    let centre = tree.bounds(subject).center();
    tree.pointer_move(centre);
    tree.tap_with(PointerKind::Mouse, centre);

    assert_eq!(taps.get(), 1, "the handler below the wrap was dropped");
    assert_eq!(hovers.get(), 1, "the handler above the wrap was dropped");
}

/// `HandlerSet::merge_under` is the public door on the value itself: the later
/// declaration wins on a conflict, the earlier one fills the gaps. Observed
/// through behaviour, because the gap-filling is what the bug destroyed.
#[test]
fn merge_under_keeps_the_earlier_handler_and_the_later_value() {
    #[derive(Debug)]
    struct Merged {
        taps: Rc<Cell<u32>>,
        hovers: Rc<Cell<u32>>,
    }

    impl Widget for Merged {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            use teksilo_core::widget_builder::HandlerSet;
            let tap_sink = self.taps.clone();
            let hover_sink = self.hovers.clone();

            // `base` is the earlier declaration, `later` the newer one. The
            // tap only survives if `merge_under` fills from `base`.
            let base = HandlerSet::new().on_tap(move |_, _| tap_sink.set(tap_sink.get() + 1));
            let mut later = HandlerSet::new().on_hover(move |entered, _| {
                if entered {
                    hover_sink.set(hover_sink.get() + 1)
                }
            });
            later.merge_under(base);

            ctx.apply_self_handlers(later);
            vec![]
        }

        fn layout_response(&self, p: SizeProposal, _: &LayoutContext) -> LayoutResponse {
            p.resolve(40.0, 40.0).into()
        }
    }

    let taps = Rc::new(Cell::new(0u32));
    let hovers = Rc::new(Cell::new(0u32));
    let (mut tree, subject) = mount(Merged {
        taps: taps.clone(),
        hovers: hovers.clone(),
    });

    let centre = tree.bounds(subject).center();
    tree.pointer_move(centre);
    tree.tap_with(PointerKind::Mouse, centre);

    assert_eq!(taps.get(), 1, "`merge_under` dropped the earlier handler");
    assert_eq!(hovers.get(), 1, "`merge_under` dropped the later handler");
}
