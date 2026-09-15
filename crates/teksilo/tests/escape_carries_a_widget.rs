// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `#{ expr }` carries a widget value as well as a `WidgetId`.
//!
//! The escape used to lower to `.child(expr)`, so it took an id and nothing
//! else. That left the one shape the element form claims by mistake, a Rust
//! struct literal at body position, with no escape to escape into: `HStack {
//! Card { title: t } }` lowers to `Card::new().title(t)`, and the documented
//! way out was to parenthesise it AND demote it to a `child:` property, which
//! is two mechanisms and neither is guessable.
//!
//! Every container's `child` now takes `impl IntoTeksiChild`, so the escape
//! lowers to `.child(expr)` and one form carries both.

use teksilo::canvas::SizeProposal;
use teksilo::core::widget::LayoutResponse;
use teksilo::core::{LayoutContext, Widget, WidgetId, WidgetTree};
use teksilo::prelude::*;
use teksilo::widgets::{Panel, TextWidget, VStack};

/// A custom widget built by struct literal, the shape the corpus has 181 of.
#[derive(Debug, Default)]
struct Card {
    title: &'static str,
    root: Option<WidgetId>,
}

impl Widget for Card {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.add(TextWidget::new(lit!(self.title)));
        self.root = Some(id);
        vec![id]
    }

    fn layout_response(&self, p: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        p.resolve(10.0, 10.0).into()
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root.into_iter().collect()
    }
}

#[derive(Debug)]
struct Root(fn(&mut BuildContext) -> WidgetId, Option<WidgetId>);

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = (self.0)(ctx);
        self.1 = Some(id);
        vec![id]
    }

    fn layout_response(&self, p: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        p.resolve(200.0, 200.0).into()
    }

    fn children(&self) -> Vec<WidgetId> {
        self.1.into_iter().collect()
    }
}

/// Count widgets of type `name` anywhere below `id`.
///
/// A container's internals make a direct child count meaningless: a childless
/// `Panel` still emits a zero-size placeholder so its style has a `content:
/// WidgetId` to wrap.
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

fn mount(f: fn(&mut BuildContext) -> WidgetId) -> (WidgetTree, WidgetId) {
    let mut tree = WidgetTree::new();
    let root = tree.add(Root(f, None));
    tree.layout(SizeProposal::exact(200.0, 200.0));
    let subject = *tree
        .children(root)
        .first()
        .expect("the block built a widget");
    (tree, subject)
}

/// The case that had no escape: a struct literal, with no parentheses and no
/// `child:` property.
#[test]
fn the_escape_carries_a_struct_literal() {
    let (tree, stack) = mount(|ctx| {
        teksu!(ctx => VStack {
                #{ Card { title: "one", root: None } }
                #{ Card { title: "two", root: None } }
            }
        )
    });
    assert_eq!(count_kind(&tree, stack, "Card"), 2);
}

/// And it still carries a `WidgetId`, which is all it could carry before.
#[test]
fn the_escape_still_carries_a_widget_id() {
    let (tree, stack) = mount(|ctx| {
        let pre = ctx.add(TextWidget::new(lit!("pre")));
        teksu!(ctx => VStack {
            #{ pre }
            TextWidget::new(lit!("inline"))
        })
    });
    assert_eq!(count_kind(&tree, stack, "TextWidget"), 2);
}

/// Both arms reach a single-child wrapper too, not only the flow primitives.
#[test]
fn a_single_child_wrapper_takes_both_arms() {
    let (tree, panel) = mount(|ctx| {
        teksu!(ctx => Panel {
            #{ Card { title: "boxed", root: None } }
        })
    });
    assert_eq!(count_kind(&tree, panel, "Card"), 1);

    let (tree, panel) = mount(|ctx| {
        let pre = ctx.add(TextWidget::new(lit!("pre")));
        teksu!(ctx => Panel {
            #{ pre }
        })
    });
    assert_eq!(count_kind(&tree, panel, "TextWidget"), 1);
}

/// `.child` itself now takes an id, so the plain builder API gained the same
/// unification the escape did.
#[test]
fn the_builder_child_takes_an_id() {
    let (tree, stack) = mount(|ctx| {
        let pre = ctx.add(TextWidget::new(lit!("pre")));
        ctx.add(
            VStack::new()
                .child(pre)
                .child(TextWidget::new(lit!("inline"))),
        )
    });
    assert_eq!(count_kind(&tree, stack, "TextWidget"), 2);
}
