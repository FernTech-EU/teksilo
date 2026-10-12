// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A custom overlay trigger under a pointer: which clicks open it, which close
//! it, and that one click is one open.
//!
//! The shape that motivated these: a composite whose focus stop is an inner
//! node with its own `on_tap`, `on_key` and `focusable(true)`, wired through
//! `apply_self_handlers`. Enter, Space and the reader's `Click` reached the
//! opener on that node; a pointer click did not, because the opener sat on
//! the composite's root and the inner node's own tap recognizer took the
//! click first.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{ButtonMask, EventResponse, Key, Modifiers, PointerButton, WidgetEvent};
use teksilo_core::overlay::DismissReason;
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
use teksilo_core::widget_builder::{HandlerSet, WidgetBuilder};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;

use crate::button::Button;
use crate::overlay_trigger::OverlayTrigger;
use crate::popover_widget::PopoverWidget;
use crate::primitives::{MinSize, Padding, RectWidget, VStack};

fn light_tree() -> WidgetTree {
    WidgetTree::new().with_theme(teksilo_core::presets::intui::light())
}

fn laid_out(tree: &mut WidgetTree) {
    tree.layout(SizeProposal::exact(800.0, 600.0));
}

/// The inner focus stop of [`Composite`]: a fixed-size leaf with its own tap,
/// key handling and focusability, all on its own bucket. It counts its own
/// taps so a test can see that the composite still works as itself.
#[derive(Debug)]
struct Stop {
    taps: Rc<Cell<u32>>,
}

impl Widget for Stop {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let taps = self.taps.clone();
        ctx.apply_self_handlers(
            HandlerSet::new()
                .on_tap(move |_event, _ctx| taps.set(taps.get() + 1))
                .on_key(|event, _ctx| match event {
                    WidgetEvent::KeyDown {
                        key: Key::ArrowUp, ..
                    } => EventResponse::Handled,
                    _ => EventResponse::Ignored,
                })
                .focusable(true),
        );
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(80.0, 24.0).into()
    }
}

/// A composite whose focus stop is an inner node, with a padded margin around
/// it that belongs to the composite's root and to nothing that takes focus.
#[derive(Debug)]
struct Composite {
    taps: Rc<Cell<u32>>,
    root: Option<WidgetId>,
}

impl Composite {
    fn new(taps: Rc<Cell<u32>>) -> Self {
        Self { taps, root: None }
    }
}

impl Widget for Composite {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let stop = ctx.add(Stop {
            taps: self.taps.clone(),
        });
        let root = ctx.add(Padding::uniform(12.0).child(stop));
        self.root = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root
            .and_then(|id| ctx.child_layout_response(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0).into())
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root.into_iter().collect()
    }
}

/// How many times the popover opened and closed, and the reasons it gave.
#[derive(Default, Clone)]
struct Counts {
    opens: Rc<Cell<u32>>,
    closes: Rc<Cell<u32>>,
    reasons: Rc<std::cell::RefCell<Vec<DismissReason>>>,
}

impl Counts {
    fn opens(&self) -> u32 {
        self.opens.get()
    }
    fn closes(&self) -> u32 {
        self.closes.get()
    }
}

fn counted<T: crate::popover_widget::PopoverTrigger>(
    popover: PopoverWidget<T>,
    counts: &Counts,
) -> PopoverWidget<T> {
    let opens = counts.opens.clone();
    let closes = counts.closes.clone();
    let reasons = counts.reasons.clone();
    popover
        .on_open(move || opens.set(opens.get() + 1))
        .on_close_with(move |reason| {
            closes.set(closes.get() + 1);
            reasons.borrow_mut().push(reason);
        })
}

fn content() -> impl Widget {
    MinSize::new(40.0, 40.0).child(RectWidget::new())
}

/// A popover around [`Composite`], mounted in a column with a spare control
/// after it. Returns the composite's focus stop.
fn composite_popover(tree: &mut WidgetTree, taps: &Rc<Cell<u32>>, counts: &Counts) -> WidgetId {
    let popover = counted(
        PopoverWidget::new(OverlayTrigger::around(Composite::new(taps.clone()))),
        counts,
    )
    .content(content());
    let id = tree.add(popover);
    tree.add(VStack::new().child(id).child(Button::new(lit!("After"))));
    laid_out(tree);
    tree.first_focusable_descendant(id)
        .expect("the composite's focus stop")
}

fn click_at(tree: &mut WidgetTree, at: Point, button: PointerButton) {
    tree.pointer_down_button(at, button);
    tree.pointer_up_button(at, button);
    laid_out(tree);
}

#[test]
fn a_click_on_a_composites_inner_control_opens_the_popover_once() {
    let mut tree = light_tree();
    let taps = Rc::new(Cell::new(0));
    let counts = Counts::default();
    let stop = composite_popover(&mut tree, &taps, &counts);

    let centre = tree.bounds(stop).center();
    click_at(&mut tree, centre, PointerButton::Primary);
    assert_eq!(taps.get(), 1, "the control still sees its own click");
    assert_eq!(
        (counts.opens(), counts.closes()),
        (1, 0),
        "one click on the control is one open, and nothing closes it again"
    );
    assert_eq!(tree.active_overlays().len(), 1);

    click_at(&mut tree, centre, PointerButton::Primary);
    assert_eq!(
        (counts.opens(), counts.closes()),
        (1, 1),
        "a second click on the control closes it, and does not reopen it"
    );
    assert!(tree.active_overlays().is_empty());
}

#[test]
fn a_click_on_a_composites_margin_still_opens_the_popover() {
    let mut tree = light_tree();
    let taps = Rc::new(Cell::new(0));
    let counts = Counts::default();
    let stop = composite_popover(&mut tree, &taps, &counts);

    // Inside the composite's padding, outside its control.
    let control = tree.bounds(stop);
    let margin = Point::new(control.x - 6.0, control.y - 6.0);
    click_at(&mut tree, margin, PointerButton::Primary);
    assert_eq!(taps.get(), 0, "precondition: the click missed the control");
    assert_eq!((counts.opens(), counts.closes()), (1, 0));
}

#[test]
fn enter_on_a_composites_inner_control_opens_the_popover() {
    let mut tree = light_tree();
    let taps = Rc::new(Cell::new(0));
    let counts = Counts::default();
    let stop = composite_popover(&mut tree, &taps, &counts);

    tree.focus(stop);
    tree.press_key(Key::Enter, Modifiers::NONE);
    laid_out(&mut tree);
    assert_eq!((counts.opens(), counts.closes()), (1, 0));
    assert_eq!(tree.active_overlays().len(), 1);
}

#[test]
fn a_click_on_a_plain_button_trigger_opens_the_popover_once() {
    let mut tree = light_tree();
    let counts = Counts::default();
    let popover = counted(
        PopoverWidget::new(OverlayTrigger::around(Button::new(lit!("Filter")))),
        &counts,
    )
    .content(content());
    let id = tree.add(popover);
    laid_out(&mut tree);
    let button = tree.first_focusable_descendant(id).expect("the button");

    let centre = tree.bounds(button).center();
    click_at(&mut tree, centre, PointerButton::Primary);
    assert_eq!((counts.opens(), counts.closes()), (1, 0));
    click_at(&mut tree, centre, PointerButton::Primary);
    assert_eq!((counts.opens(), counts.closes()), (1, 1));
}

/// A face that takes no focus and opens on the secondary button only.
fn right_click_face() -> impl Widget {
    MinSize::new(80.0, 24.0)
        .child(RectWidget::new())
        .accept_tap_buttons(ButtonMask::SECONDARY)
}

fn right_click_popover(tree: &mut WidgetTree, counts: &Counts) -> Point {
    let popover = counted(
        PopoverWidget::new(OverlayTrigger::around(right_click_face()).named("Options")),
        counts,
    )
    .content(content());
    let id = tree.add(popover);
    laid_out(tree);
    tree.bounds(id).center()
}

#[test]
fn a_right_click_on_an_open_right_click_trigger_closes_it() {
    let mut tree = light_tree();
    let counts = Counts::default();
    let centre = right_click_popover(&mut tree, &counts);

    click_at(&mut tree, centre, PointerButton::Secondary);
    assert_eq!(
        (counts.opens(), counts.closes()),
        (1, 0),
        "a right click opens"
    );

    click_at(&mut tree, centre, PointerButton::Secondary);
    assert_eq!(
        (counts.opens(), counts.closes()),
        (1, 1),
        "a second right click on the trigger closes it, and does not reopen it"
    );
    assert!(tree.active_overlays().is_empty());
}

#[test]
fn a_left_click_on_a_right_click_trigger_does_not_open_it() {
    let mut tree = light_tree();
    let counts = Counts::default();
    let centre = right_click_popover(&mut tree, &counts);

    click_at(&mut tree, centre, PointerButton::Primary);
    assert_eq!((counts.opens(), counts.closes()), (0, 0));
}

fn left_click_popover(tree: &mut WidgetTree, counts: &Counts) -> Point {
    let popover = counted(
        PopoverWidget::new(OverlayTrigger::around(Button::new(lit!("Filter")))),
        counts,
    )
    .content(content());
    let id = tree.add(popover);
    laid_out(tree);
    tree.bounds(id).center()
}

#[test]
fn a_right_click_on_an_open_left_click_trigger_dismisses_it_and_falls_through() {
    let mut tree = light_tree();
    let counts = Counts::default();
    let centre = left_click_popover(&mut tree, &counts);

    click_at(&mut tree, centre, PointerButton::Primary);
    assert_eq!((counts.opens(), counts.closes()), (1, 0));

    click_at(&mut tree, centre, PointerButton::Secondary);
    assert_eq!(
        (counts.opens(), counts.closes()),
        (1, 1),
        "a press the trigger does not answer to dismisses, as any outside press does"
    );
    assert_eq!(
        counts.reasons.borrow().as_slice(),
        &[DismissReason::OutsidePress]
    );
    assert!(tree.active_overlays().is_empty());
}

#[test]
fn the_dismiss_reason_reaches_on_close_with() {
    let mut tree = light_tree();
    let counts = Counts::default();
    let centre = left_click_popover(&mut tree, &counts);

    // A press outside both the trigger and the panel.
    click_at(&mut tree, centre, PointerButton::Primary);
    click_at(&mut tree, Point::new(790.0, 590.0), PointerButton::Primary);
    // Escape, from inside the panel.
    click_at(&mut tree, centre, PointerButton::Primary);
    tree.press_key(Key::Escape, Modifiers::NONE);
    laid_out(&mut tree);
    // The trigger clicked again while open: the press lands outside the panel.
    click_at(&mut tree, centre, PointerButton::Primary);
    click_at(&mut tree, centre, PointerButton::Primary);
    // The close chord, which the popover answers by closing itself.
    click_at(&mut tree, centre, PointerButton::Primary);
    tree.press_key(Key::ArrowUp, Modifiers::ALT);
    laid_out(&mut tree);

    assert_eq!(counts.opens(), 4);
    assert_eq!(
        counts.reasons.borrow().as_slice(),
        &[
            DismissReason::OutsidePress,
            DismissReason::Escape,
            DismissReason::OutsidePress,
            DismissReason::Programmatic,
        ]
    );
}

#[test]
fn on_close_still_fires_without_a_reason() {
    let mut tree = light_tree();
    let closes = Rc::new(Cell::new(0));
    let popover = PopoverWidget::new(OverlayTrigger::around(Button::new(lit!("Filter"))))
        .on_close({
            let closes = closes.clone();
            move || closes.set(closes.get() + 1)
        })
        .content(content());
    let id = tree.add(popover);
    laid_out(&mut tree);
    let centre = tree.bounds(id).center();
    click_at(&mut tree, centre, PointerButton::Primary);
    tree.press_key(Key::Escape, Modifiers::NONE);
    assert_eq!(closes.get(), 1);
}

/// A composite given a right-click mask on its root opens on a right click
/// anywhere on it, its inner control included, and a left click on that
/// control is the control's own.
#[test]
fn a_composite_with_a_right_click_mask_opens_on_a_right_click_only() {
    let mut tree = light_tree();
    let taps = Rc::new(Cell::new(0));
    let counts = Counts::default();
    let popover = counted(
        PopoverWidget::new(OverlayTrigger::around(
            Composite::new(taps.clone()).accept_tap_buttons(ButtonMask::SECONDARY),
        )),
        &counts,
    )
    .content(content());
    let id = tree.add(popover);
    laid_out(&mut tree);
    let stop = tree.first_focusable_descendant(id).expect("the focus stop");
    let centre = tree.bounds(stop).center();

    click_at(&mut tree, centre, PointerButton::Primary);
    assert_eq!(taps.get(), 1, "the control's own click");
    assert_eq!(
        (counts.opens(), counts.closes()),
        (0, 0),
        "a left click does not open a right-click trigger"
    );

    click_at(&mut tree, centre, PointerButton::Secondary);
    assert_eq!(
        (counts.opens(), counts.closes()),
        (1, 0),
        "a right click opens"
    );
    click_at(&mut tree, centre, PointerButton::Secondary);
    assert_eq!(
        (counts.opens(), counts.closes()),
        (1, 1),
        "and a second one closes"
    );
}
