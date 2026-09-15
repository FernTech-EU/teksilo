// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Phase 1: the two framework gaps that stranded conditional and boxed
//! children, pinned against real catalog widgets.
//!
//! Both were reachable with no macro. `teksu!`'s bare `if` lowers to
//! `.child_opt(..)`, which existed on 7 of the 38 containers that have
//! `.child(..)`, and its over-four-arms advice recommends `Box<dyn Widget>`,
//! which did not implement `Widget` at all.

use teksilo::canvas::SizeProposal;
use teksilo::core::{Widget, WidgetId, WidgetTree};
use teksilo::prelude::*;
use teksilo::widgets::{Panel, TextWidget, VStack};

#[derive(Debug)]
struct Root(Option<Box<dyn Widget>>, Option<WidgetId>);

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.add_boxed(self.0.take().expect("built once"));
        self.1 = Some(id);
        vec![id]
    }

    fn layout_response(
        &self,
        p: SizeProposal,
        _: &teksilo::core::LayoutContext,
    ) -> teksilo::core::widget::LayoutResponse {
        p.resolve(400.0, 400.0).into()
    }

    fn children(&self) -> Vec<WidgetId> {
        self.1.into_iter().collect()
    }
}

/// Count widgets of type `name` anywhere below `id`.
///
/// Container internals make a direct `children(..).len()` meaningless: a
/// childless `Panel` still emits a zero-size placeholder so its style has a
/// `content: WidgetId` to wrap. What matters is whether the caller's widget
/// reached the tree.
fn count_kind(tree: &WidgetTree, id: WidgetId, name: &str) -> usize {
    let here = usize::from(
        tree.widget_type_name(id)
            .is_some_and(|t| t.rsplit("::").next() == Some(name)),
    );
    here + tree
        .children(id)
        .into_iter()
        .map(|c| count_kind(tree, c, name))
        .sum::<usize>()
}

fn mount(w: impl Widget + 'static) -> (WidgetTree, WidgetId) {
    let mut tree = WidgetTree::new();
    let root = tree.add(Root(Some(Box::new(w)), None));
    tree.layout(SizeProposal::exact(400.0, 400.0));
    let subject = *tree.children(root).first().expect("mounted");
    (tree, subject)
}

/// `child_opt` now exists on every container that has `child`, so a bare
/// `teksu!` `if` works inside a single-child wrapper, not only inside a stack.
#[test]
fn child_opt_on_a_single_child_wrapper() {
    let (tree, panel) = mount(Panel::new().child_opt(Some(TextWidget::new(lit!("shown")))));
    assert_eq!(count_kind(&tree, panel, "TextWidget"), 1);

    let (tree, panel) = mount(Panel::new().child_opt(None::<TextWidget>));
    assert_eq!(
        count_kind(&tree, panel, "TextWidget"),
        0,
        "`None` must attach nothing"
    );
}

/// The same through the DSL: a bare `if` inside a `Panel` used to fail to
/// compile, because the lowering targets `child_opt`.
#[test]
fn teksu_bare_if_inside_a_panel() {
    fn block(ctx: &mut BuildContext, show: bool) -> WidgetId {
        teksu!(ctx => Panel {
                if show {
                    TextWidget::new(lit!("conditional"))
                }
            }
        )
    }

    // Exercise both arms through a real build.
    for (show, expected) in [(true, 1usize), (false, 0usize)] {
        #[derive(Debug)]
        struct Host(bool, Option<WidgetId>);
        impl Widget for Host {
            fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
                let id = block(ctx, self.0);
                self.1 = Some(id);
                vec![id]
            }
            fn layout_response(
                &self,
                p: SizeProposal,
                _: &teksilo::core::LayoutContext,
            ) -> teksilo::core::widget::LayoutResponse {
                p.resolve(400.0, 400.0).into()
            }
            fn children(&self) -> Vec<WidgetId> {
                self.1.into_iter().collect()
            }
        }
        let mut tree = WidgetTree::new();
        let host = tree.add(Host(show, None));
        tree.layout(SizeProposal::exact(400.0, 400.0));
        let panel = *tree.children(host).first().expect("panel mounted");
        assert_eq!(
            count_kind(&tree, panel, "TextWidget"),
            expected,
            "bare `if` with show={show}"
        );
    }
}

/// `Box<dyn Widget>` is a widget, so it goes anywhere a widget goes and adds
/// no node of its own.
#[test]
fn a_boxed_widget_is_a_child() {
    let boxed: Box<dyn Widget> = Box::new(TextWidget::new(lit!("boxed")));
    let (tree, stack) = mount(VStack::new().child(boxed));

    let kids = tree.children(stack);
    assert_eq!(kids.len(), 1, "the box must not add a node of its own");
    assert_eq!(
        tree.widget_type_name(kids[0])
            .and_then(|t| t.rsplit("::").next()),
        Some("TextWidget"),
        "the box must be transparent: the arena sees the widget that was boxed"
    );
}

/// The shape the macro's own over-four-arms advice recommends: heterogeneous
/// arms unified by boxing. This did not compile before.
#[test]
fn boxed_match_arms_unify() {
    fn pick(n: u32) -> Box<dyn Widget> {
        match n {
            0 => Box::new(TextWidget::new(lit!("zero"))),
            1 => Box::new(Panel::new()),
            2 => Box::new(VStack::new()),
            3 => Box::new(TextWidget::new(lit!("three"))),
            _ => Box::new(Panel::new().child(TextWidget::new(lit!("many")))),
        }
    }

    let (tree, stack) = mount(VStack::new().children((0..5).map(pick)));
    assert_eq!(tree.children(stack).len(), 5);
}

/// And boxed values flow through `child_opt` too.
#[test]
fn a_boxed_widget_flows_through_child_opt() {
    let some: Option<Box<dyn Widget>> = Some(Box::new(TextWidget::new(lit!("x"))));
    let (tree, panel) = mount(Panel::new().child_opt(some));
    assert_eq!(count_kind(&tree, panel, "TextWidget"), 1);
}
