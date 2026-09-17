// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech
//! Prototype check: a lowercase helper CALL as a bare child.
use teksilo_canvas::SizeProposal;
use teksilo_core::widget::LayoutResponse;
use teksilo_core::{BuildContext, LayoutContext, Widget, WidgetId, WidgetTree};
use teksilo_macros::teksu;

#[derive(Debug, Default)]
struct Leaf;
impl Leaf {
    fn new() -> Self {
        Self
    }
}
impl Widget for Leaf {
    fn layout_response(&self, p: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        p.resolve(1.0, 1.0).into()
    }
}
#[derive(Debug, Default)]
struct Probe {
    pending: Vec<Box<dyn Widget>>,
    spacing: f32,
    mounted: Vec<WidgetId>,
}
impl Probe {
    fn new() -> Self {
        Self::default()
    }
    fn child(mut self, w: impl Widget + 'static) -> Self {
        self.pending.push(Box::new(w));
        self
    }
    fn spacing(mut self, s: f32) -> Self {
        self.spacing = s;
        self
    }
    fn fills(mut self) -> Self {
        self.spacing = 1.0;
        self
    }
}
impl Widget for Probe {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        self.mounted = self.pending.drain(..).map(|w| ctx.add_boxed(w)).collect();
        self.mounted.clone()
    }
    fn layout_response(&self, p: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        p.resolve(1.0, 1.0).into()
    }
    fn children(&self) -> Vec<WidgetId> {
        self.mounted.clone()
    }
}
#[derive(Debug)]
struct Root(fn(&mut BuildContext) -> WidgetId);
impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        vec![(self.0)(ctx)]
    }
    fn layout_response(&self, p: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        p.resolve(1.0, 1.0).into()
    }
}

// The component pattern the whole corpus is made of.
fn section(_title: &str) -> impl Widget {
    Leaf::new()
}

fn row(_n: u32) -> Probe {
    Probe::new()
}

fn count(f: fn(&mut BuildContext) -> WidgetId) -> usize {
    let mut tree = WidgetTree::new();
    let root = tree.add(Root(f));
    tree.layout(SizeProposal::exact(50.0, 50.0));
    let top = *tree.children(root).first().unwrap();
    tree.children(top).len()
}

#[test]
fn helper_call_is_a_bare_child() {
    assert_eq!(
        count(|ctx| teksu!(ctx => Probe { section("a")  section("b") })),
        2
    );
}

#[test]
fn method_chain_is_a_bare_child() {
    assert_eq!(
        count(|ctx| teksu!(ctx => Probe { row(1).spacing(4.0)  Leaf })),
        2
    );
}

#[test]
fn argument_free_property_still_works() {
    // `fills` alone must stay a property, not become an expression child.
    assert_eq!(count(|ctx| teksu!(ctx => Probe { fills  Leaf  Leaf })), 2);
}

#[test]
fn mixed_properties_children_and_helpers() {
    assert_eq!(
        count(|ctx| teksu!(ctx => Probe {
            spacing: 6.0
            section("head")
            Leaf
            row(2).spacing(1.0)
            fills
        })),
        3
    );
}
#[test]
fn keyword_path_call_is_a_bare_child() {
    assert_eq!(
        count(|ctx| teksu!(ctx => Probe { self::section("a")  Leaf })),
        2
    );
}

/// A parenthesised item is deliberately NOT accepted at body position: body
/// items are whitespace-separated and Rust reads `(a) (b)` as a call, so it
/// would swallow its neighbour. The delimited property form is the way in.
#[test]
fn a_keyword_rooted_head_is_a_bare_child() {
    assert_eq!(
        count(|ctx| teksu!(ctx => Probe { self::section("a")  crate::section("b")  Leaf })),
        3
    );
}

#[test]
fn a_parenthesised_expression_goes_through_the_property_form() {
    assert_eq!(
        count(|ctx| teksu!(ctx => Probe { child: (section("p"))  Leaf })),
        2
    );
}
