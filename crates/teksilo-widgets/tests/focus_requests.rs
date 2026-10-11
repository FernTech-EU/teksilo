// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Focus requests into a page the same handler shows, and where a click on
//! space with nothing focusable in it leaves focus.
//!
//! The first half is the `Switcher` shape of the deferred focus request: a
//! handler selects a page and asks for focus on a control on it, while the
//! page is still dormant. Focus used to land on the dormant control, be
//! dropped by the next layout's revalidation, and then be "restored" by the
//! `Switcher`'s rebuild to the page's *first* control, so a request for the
//! second of two buttons ended on the first.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::WidgetId;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;
use teksilo_tokens::PointerKind;
use teksilo_widgets::button::Button;
use teksilo_widgets::primitives::{FixedSize, RectWidget, Switcher, TextWidget, VStack};
use teksilo_widgets::text_input::TextInput;

fn size() -> SizeProposal {
    SizeProposal::exact(400.0, 300.0)
}

fn tree() -> WidgetTree {
    WidgetTree::new().with_theme(teksilo_core::presets::intui::light())
}

/// A `go` button whose activation selects page 1 and requests focus on
/// whatever `target` holds, above a `Switcher` whose page 1 is `page`.
struct Pages {
    tree: WidgetTree,
    go: WidgetId,
    selected: Signal<usize>,
    target: Rc<Cell<Option<WidgetId>>>,
}

fn pages(tree: WidgetTree, page: WidgetId) -> Pages {
    pages_with(tree, |s| s.child(page))
}

fn pages_with(mut tree: WidgetTree, page_one: impl FnOnce(Switcher) -> Switcher) -> Pages {
    let selected = Signal::new(0_usize);
    let target: Rc<Cell<Option<WidgetId>>> = Rc::new(Cell::new(None));
    let go = {
        let (selected, target) = (selected.clone(), target.clone());
        tree.add(Button::new(lit!("Edit")).on_activate_fn(move |ctx| {
            selected.set(1);
            if let Some(id) = target.get() {
                ctx.request_focus(id);
            }
        }))
    };
    let switcher = tree.add(page_one(
        Switcher::new(selected.clone()).child(TextWidget::new(lit!("Page 0"))),
    ));
    tree.add(VStack::new().child(go).child(switcher));
    tree.layout(size());
    Pages {
        tree,
        go,
        selected,
        target,
    }
}

impl Pages {
    /// Show page 1 once and go back to page 0, so page 1 is mounted, and
    /// dormant.
    fn visit_page_one(&mut self) {
        self.selected.set(1);
        self.tree.layout(size());
        self.selected.set(0);
        self.tree.layout(size());
    }

    /// Activate `go` with `target` as the request, and lay out.
    fn request(&mut self, target: WidgetId) {
        self.target.set(Some(target));
        self.tree.click(self.go);
        self.tree.layout(size());
    }
}

/// The measured bug: page 1 holds two buttons and the request names the
/// second. It must end on the second, not on the page's first control.
#[test]
fn a_request_for_the_second_button_on_a_hidden_page_lands_on_it() {
    let mut tree = tree();
    let a = tree.add(Button::new(lit!("A")));
    let b = tree.add(Button::new(lit!("B")));
    let page = tree.add(VStack::new().child(a).child(b));
    let mut p = pages(tree, page);
    p.visit_page_one();
    assert!(!p.tree.is_active(b), "page 1 is dormant");

    p.request(b);
    assert!(p.tree.is_active(b));
    assert_eq!(p.tree.focused(), Some(b));
}

/// A request naming a composite (`TextInput`, whose focus lives on an inner
/// leaf) re-resolves its descendant once the page has woken.
#[test]
fn a_request_for_a_composite_on_a_hidden_page_lands_inside_it() {
    let mut tree = tree();
    let first = tree.add(TextInput::new(Signal::new(String::new())));
    let second = tree.add(TextInput::new(Signal::new(String::new())));
    let page = tree.add(VStack::new().child(first).child(second));
    let mut p = pages(tree, page);
    p.visit_page_one();
    p.selected.set(1);
    p.tree.layout(size());
    let inner = p
        .tree
        .first_focusable_descendant(second)
        .expect("an inner field");
    p.selected.set(0);
    p.tree.layout(size());

    p.request(second);
    assert_eq!(p.tree.focused(), Some(inner));
}

/// A page mounted eagerly but never shown is dormant from its first layout:
/// the request waits for it just the same.
#[test]
fn a_request_into_a_page_never_shown_lands_on_it() {
    let mut tree = tree();
    let a = tree.add(Button::new(lit!("A")));
    let b = tree.add(Button::new(lit!("B")));
    let page = tree.add(VStack::new().child(a).child(b));
    let mut p = pages(tree, page);
    assert!(!p.tree.is_active(b), "page 1 has never been shown");

    p.request(b);
    assert_eq!(p.tree.focused(), Some(b));
}

/// A page the `Switcher` builds lazily, on its first selection, and that puts
/// focus on its second control from its own `build`. Nothing about that path
/// is deferred, and it must keep working.
#[test]
fn a_lazily_mounted_page_that_focuses_in_its_build_keeps_its_focus() {
    #[derive(Debug)]
    struct FocusSecond {
        root: Option<WidgetId>,
        second: Rc<Cell<Option<WidgetId>>>,
    }
    impl Widget for FocusSecond {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            let a = ctx.add(Button::new(lit!("A")));
            let b = ctx.add(Button::new(lit!("B")));
            let root = ctx.add(VStack::new().child(a).child(b));
            ctx.focus(b);
            self.second.set(Some(b));
            self.root = Some(root);
            vec![root]
        }
        fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
            self.root
                .and_then(|id| ctx.child_size(id, proposal))
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
                .into()
        }
        fn children(&self) -> Vec<WidgetId> {
            self.root.into_iter().collect()
        }
    }

    let second = Rc::new(Cell::new(None));
    let page = FocusSecond {
        root: None,
        second: second.clone(),
    };
    let mut p = pages_with(tree(), move |s| s.child(page));
    assert_eq!(
        second.get(),
        None,
        "page 1 is not built until it is selected"
    );

    // The handler selects the page and asks for nothing: the page does.
    p.tree.click(p.go);
    p.tree.layout(size());
    let b = second.get().expect("page 1 was built by its selection");
    assert_eq!(p.tree.focused(), Some(b));
}

/// A request that waits, and a page that never comes: selecting the page is
/// undone before the layout, so the request is dropped and focus stays on the
/// button that made it.
#[test]
fn a_request_for_a_page_that_stays_hidden_is_dropped() {
    let mut tree = tree();
    let a = tree.add(Button::new(lit!("A")));
    let b = tree.add(Button::new(lit!("B")));
    let page = tree.add(VStack::new().child(a).child(b));
    let mut p = pages(tree, page);
    p.visit_page_one();
    p.target.set(Some(b));
    p.tree.focus(p.go);
    p.tree.click(p.go);
    p.selected.set(0);
    p.tree.layout(size());
    assert_eq!(p.tree.focused(), Some(p.go));
    // Showing the page later does not resurrect the request.
    p.selected.set(1);
    p.tree.layout(size());
    assert_eq!(p.tree.focused(), Some(p.go));
}

/// The same request into a plain `visible_when` gate, with no `Switcher`
/// rebuild in the way. Focus used to end at `None`.
#[test]
fn a_request_into_a_visible_when_gate_lands_after_the_layout() {
    let mut tree = tree();
    let shown = Signal::new(false);
    let field = tree.add(TextInput::new(Signal::new(String::new())));
    let target: Rc<Cell<Option<WidgetId>>> = Rc::new(Cell::new(None));
    let go = {
        let (shown, target) = (shown.clone(), target.clone());
        tree.add(Button::new(lit!("Edit")).on_activate_fn(move |ctx| {
            shown.set(true);
            if let Some(id) = target.get() {
                ctx.request_focus(id);
            }
        }))
    };
    tree.visible_when(field, shown.clone());
    tree.add(VStack::new().child(go).child(field));
    tree.layout(size());
    shown.set(true);
    tree.layout(size());
    let inner = tree
        .first_focusable_descendant(field)
        .expect("an inner field");
    shown.set(false);
    tree.layout(size());
    assert!(!tree.is_active(field));

    target.set(Some(inner));
    tree.click(go);
    tree.layout(size());
    assert_eq!(tree.focused(), Some(inner));
}

/// A click on space that holds nothing focusable keeps focus where it was, for
/// a mouse and for a finger alike. Deliberate (Qt and AppKit behave the same;
/// browsers blur), and `EventContext::clear_focus` is the opt-in for the
/// browser behaviour. Pinned so the convention is a decision, not an accident.
#[test]
fn a_click_on_space_with_nothing_focusable_keeps_focus() {
    for kind in [PointerKind::Mouse, PointerKind::Touch] {
        let mut tree = tree();
        let field = tree.add(TextInput::new(Signal::new(String::new())));
        let blank = tree.add(
            FixedSize::new()
                .width(300.0)
                .height(100.0)
                .child(RectWidget::new()),
        );
        tree.add(VStack::new().child(field).child(blank));
        tree.layout(size());
        let inner = tree
            .first_focusable_descendant(field)
            .expect("an inner field");
        tree.tap_with(kind, tree.bounds(inner).center());
        assert_eq!(
            tree.focused(),
            Some(inner),
            "{kind:?}: the tap focused the field"
        );

        let at: Point = tree.bounds(blank).center();
        tree.tap_with(kind, at);
        tree.layout(size());
        assert_eq!(
            tree.focused(),
            Some(inner),
            "{kind:?}: a tap on empty space must leave focus alone"
        );
    }
}

/// The opt-in: a background with `on_tap(|_, ctx| ctx.clear_focus())` makes a
/// click on empty space drop focus.
#[test]
fn a_background_that_clears_focus_drops_it_on_a_click() {
    for kind in [PointerKind::Mouse, PointerKind::Touch] {
        let mut tree = tree();
        let field = tree.add(TextInput::new(Signal::new(String::new())));
        let blank = tree.add(
            FixedSize::new()
                .width(300.0)
                .height(100.0)
                .child(RectWidget::new())
                .on_tap(|_, ctx| ctx.clear_focus()),
        );
        tree.add(VStack::new().child(field).child(blank));
        tree.layout(size());
        let inner = tree
            .first_focusable_descendant(field)
            .expect("an inner field");
        tree.tap_with(kind, tree.bounds(inner).center());
        assert_eq!(tree.focused(), Some(inner));

        tree.tap_with(kind, tree.bounds(blank).center());
        tree.layout(size());
        assert_eq!(tree.focused(), None, "{kind:?}");
    }
}
