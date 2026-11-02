// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Context-menu contributions (`HandlerSet::context_menu_contribution`) and the
//! transitive keyboard target (`Widget::context_menu_key_target`): the closest
//! claimer wins, the sections above the owner join its menu, and the keyboard
//! and assistive routes start where a right click would.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::SizeProposal;

use crate::event::{Key, Modifiers, PointerButton, WidgetEvent};
use crate::test_widgets::FillWidget;
use crate::widget_builder::WidgetBuilder;
use crate::{BuildContext, LayoutContext, LayoutResponse, Widget, WidgetId};

use super::WidgetTree;

type Log = Rc<RefCell<Vec<String>>>;

/// A menu recording the sections handed to it and its own mounting.
#[derive(Debug)]
struct ProbeMenu {
    name: &'static str,
    log: Log,
    takes_sections: bool,
}

impl Widget for ProbeMenu {
    fn append_context_menu_section(
        &mut self,
        section: Box<dyn Widget>,
    ) -> Result<(), Box<dyn Widget>> {
        if !self.takes_sections {
            return Err(section);
        }
        let name = section
            .as_any()
            .and_then(|any| any.downcast_ref::<ProbeMenu>())
            .map_or("?", |probe| probe.name);
        self.log
            .borrow_mut()
            .push(format!("{} takes {name}", self.name));
        Ok(())
    }

    fn build(&mut self, _ctx: &mut BuildContext) -> Vec<WidgetId> {
        self.log.borrow_mut().push(format!("mounted {}", self.name));
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        proposal.resolve(50.0, 20.0).into()
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

fn probe(name: &'static str, log: &Log) -> Box<dyn Widget> {
    Box::new(ProbeMenu {
        name,
        log: log.clone(),
        takes_sections: true,
    })
}

/// A node offering a menu named `name`.
fn menu(
    name: &'static str,
    log: &Log,
) -> impl Fn(teksilo_canvas::Point, &mut crate::widget::EventContext) -> Option<Box<dyn Widget>> + 'static
{
    let log = log.clone();
    move |_, _| Some(probe(name, &log))
}

/// A node adding a section named `name`.
fn section(
    name: &'static str,
    log: &Log,
) -> impl Fn(&mut crate::widget::EventContext) -> Option<Box<dyn Widget>> + 'static {
    let log = log.clone();
    move |_| Some(probe(name, &log))
}

/// A container whose children are given ids, and whose keyboard target is
/// `key_target`.
#[derive(Debug)]
struct Holder {
    children: Vec<WidgetId>,
    key_target: Option<WidgetId>,
}

impl Widget for Holder {
    fn children(&self) -> Vec<WidgetId> {
        self.children.clone()
    }

    fn context_menu_key_target(&self) -> Option<WidgetId> {
        self.key_target
    }

    fn layout_response(&self, proposal: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        proposal.resolve(200.0, 100.0).into()
    }

    fn place_children(
        &self,
        bounds: teksilo_canvas::Rect,
        _: SizeProposal,
        children: &mut [crate::widget::WidgetPlacement],
        _: &LayoutContext,
    ) {
        for child in children {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }
}

fn holder(children: Vec<WidgetId>, key_target: Option<WidgetId>) -> Holder {
    Holder {
        children,
        key_target,
    }
}

fn right_click(tree: &mut WidgetTree, id: WidgetId) {
    let at = tree.bounds(id).center();
    tree.dispatch_event(WidgetEvent::pointer_down(
        at,
        PointerButton::Secondary,
        Modifiers::NONE,
    ));
    tree.dispatch_event(WidgetEvent::pointer_up(
        at,
        PointerButton::Secondary,
        Modifiers::NONE,
    ));
}

fn show_menu_action(tree: &mut WidgetTree, id: WidgetId) {
    tree.dispatch_event(WidgetEvent::AccessAction {
        action: accesskit::Action::ShowContextMenu,
        target: Some(id),
        target_node: crate::accessibility::widget_id_to_node_id(id),
        data: None,
    });
}

fn close(tree: &mut WidgetTree, log: &Log) {
    tree.press_key(Key::Escape, Modifiers::NONE);
    log.borrow_mut().clear();
}

/// A focusable view whose keyboard target is a row, whose keyboard target is
/// the delegate carrying the application's menu: the menu key, Shift+F10 and
/// the assistive action on either node open the delegate's menu, which
/// takes the row's section.
#[test]
fn the_keyboard_and_the_assistive_action_start_where_a_right_click_would() {
    let log = Log::default();
    let mut tree = WidgetTree::new();
    let delegate = tree.add(FillWidget::new().context_menu(menu("app", &log)));
    let row = tree.add(
        holder(vec![delegate], Some(delegate)).context_menu_contribution(section("move", &log)),
    );
    let view = tree.add(holder(vec![row], Some(row)).focusable(true));
    tree.layout(SizeProposal::exact(200.0, 100.0));
    tree.focus(view);
    let expected = ["app takes move", "mounted app"];
    for (what, open) in [
        (
            "menu key",
            Box::new(|t: &mut WidgetTree| t.press_key(Key::ContextMenu, Modifiers::NONE))
                as Box<dyn Fn(&mut WidgetTree)>,
        ),
        (
            "shift f10",
            Box::new(|t: &mut WidgetTree| t.press_key(Key::F10, Modifiers::SHIFT)),
        ),
        (
            "action on the view",
            Box::new(move |t: &mut WidgetTree| show_menu_action(t, view)),
        ),
        (
            "action on the row",
            Box::new(move |t: &mut WidgetTree| show_menu_action(t, row)),
        ),
        (
            "right click",
            Box::new(move |t: &mut WidgetTree| right_click(t, delegate)),
        ),
    ] {
        open(&mut tree);
        assert_eq!(*log.borrow(), expected, "{what}");
        close(&mut tree, &log);
        tree.focus(view);
    }
}

/// A node whose keyboard target is set after it is built.
#[derive(Debug)]
struct Pointing(Rc<std::cell::Cell<Option<WidgetId>>>);

impl Widget for Pointing {
    fn context_menu_key_target(&self) -> Option<WidgetId> {
        self.0.get()
    }

    fn layout_response(&self, proposal: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        proposal.resolve(10.0, 10.0).into()
    }
}

/// A cycle is a widget bug: a debug build reports it, a release build stops
/// on the last good node.
#[test]
#[cfg_attr(
    debug_assertions,
    should_panic(expected = "context_menu_key_target cycle")
)]
fn a_key_target_cycle_stops() {
    let mut tree = WidgetTree::new();
    let a_points = Rc::new(std::cell::Cell::new(None));
    let b_points = Rc::new(std::cell::Cell::new(None));
    let a = tree.add(Pointing(a_points.clone()));
    let b = tree.add(Pointing(b_points.clone()));
    a_points.set(Some(b));
    b_points.set(Some(a));
    tree.layout(SizeProposal::exact(200.0, 100.0));
    assert_eq!(
        tree.resolve_context_menu_target(a),
        b,
        "one hop, then a again"
    );
}

/// Sections above the owner join its menu, innermost first; the owner's own
/// section once.
#[test]
fn sections_above_the_owner_join_its_menu_innermost_first() {
    let log = Log::default();
    let mut tree = WidgetTree::new();
    let leaf = tree.add(
        FillWidget::new()
            .context_menu(menu("app", &log))
            .context_menu_contribution(section("own", &log)),
    );
    let inner =
        tree.add(holder(vec![leaf], None).context_menu_contribution(section("inner", &log)));
    tree.add(holder(vec![inner], None).context_menu_contribution(section("outer", &log)));
    tree.layout(SizeProposal::exact(200.0, 100.0));
    right_click(&mut tree, leaf);
    assert_eq!(
        *log.borrow(),
        [
            "app takes own",
            "app takes inner",
            "app takes outer",
            "mounted app"
        ]
    );
}

/// The closest claimer wins: a row's section under a menu on the view opens
/// alone, and the view's menu is not asked.
#[test]
fn a_section_closer_than_any_menu_opens_alone() {
    let log = Log::default();
    let mut tree = WidgetTree::new();
    let delegate = tree.add(FillWidget::new());
    let row =
        tree.add(holder(vec![delegate], None).context_menu_contribution(section("move", &log)));
    tree.add(
        holder(vec![row], None)
            .focusable(true)
            .context_menu(menu("view", &log)),
    );
    tree.layout(SizeProposal::exact(200.0, 100.0));
    right_click(&mut tree, delegate);
    assert_eq!(*log.borrow(), ["mounted move"]);
}

/// Contributions are collected up to the first focusable node above the
/// owner: a view nested in another view's row adds its own section only.
#[test]
fn a_nested_view_adds_only_its_own_section() {
    let log = Log::default();
    let mut tree = WidgetTree::new();
    let delegate = tree.add(FillWidget::new().context_menu(menu("app", &log)));
    let inner_row = tree
        .add(holder(vec![delegate], None).context_menu_contribution(section("inner move", &log)));
    let inner_view = tree.add(holder(vec![inner_row], None).focusable(true));
    let outer_row = tree
        .add(holder(vec![inner_view], None).context_menu_contribution(section("outer move", &log)));
    tree.add(holder(vec![outer_row], None).focusable(true));
    tree.layout(SizeProposal::exact(200.0, 100.0));
    right_click(&mut tree, delegate);
    assert_eq!(*log.borrow(), ["app takes inner move", "mounted app"]);
}

#[test]
fn with_nothing_offered_no_menu_opens() {
    let mut tree = WidgetTree::new();
    let leaf = tree.add(FillWidget::new().focusable());
    tree.layout(SizeProposal::exact(200.0, 100.0));
    tree.focus(leaf);
    let overlays = tree.overlay_manager().stack.len();
    tree.press_key(Key::ContextMenu, Modifiers::NONE);
    assert_eq!(tree.overlay_manager().stack.len(), overlays);
}

/// A menu that does not take sections opens without them.
#[test]
fn a_menu_without_the_hook_opens_without_the_section() {
    let log = Log::default();
    let mut tree = WidgetTree::new();
    let factory_log = log.clone();
    let leaf = tree.add(FillWidget::new().context_menu(move |_, _| {
        Some(Box::new(ProbeMenu {
            name: "plain",
            log: factory_log.clone(),
            takes_sections: false,
        }) as Box<dyn Widget>)
    }));
    tree.add(holder(vec![leaf], None).context_menu_contribution(section("move", &log)));
    tree.layout(SizeProposal::exact(200.0, 100.0));
    right_click(&mut tree, leaf);
    assert_eq!(*log.borrow(), ["mounted plain"]);
}

/// A focusable control's own menu is about that control: no section.
#[test]
fn a_focusable_controls_menu_takes_no_section() {
    let log = Log::default();
    let mut tree = WidgetTree::new();
    let field = tree.add(
        FillWidget::new()
            .focusable()
            .context_menu(menu("field", &log)),
    );
    tree.add(holder(vec![field], None).context_menu_contribution(section("move", &log)));
    tree.layout(SizeProposal::exact(200.0, 100.0));
    right_click(&mut tree, field);
    assert_eq!(*log.borrow(), ["mounted field"]);
}

/// A composite whose root is not focusable but stands for a focusable field
/// through its accessibility proxy (a `SpinBox`, a `DateEdit`): its menu is
/// about that control and takes no section either.
#[derive(Debug)]
struct Composite {
    field: Option<WidgetId>,
}

impl Widget for Composite {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let field = ctx.add(FillWidget::new().focusable());
        self.field = Some(field);
        vec![field]
    }

    fn layout_response(&self, proposal: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn children(&self) -> Vec<WidgetId> {
        self.field.into_iter().collect()
    }

    fn accessibility_proxy(&self) -> Option<WidgetId> {
        self.field
    }
}

#[test]
fn a_composite_standing_for_a_focusable_field_takes_no_section() {
    let log = Log::default();
    let mut tree = WidgetTree::new();
    let composite = tree.add(Composite { field: None }.context_menu(menu("spin", &log)));
    tree.add(holder(vec![composite], None).context_menu_contribution(section("move", &log)));
    tree.layout(SizeProposal::exact(200.0, 100.0));
    right_click(&mut tree, composite);
    assert_eq!(*log.borrow(), ["mounted spin"]);
}

/// A node with a section alone offers `ShowContextMenu`, and so does a view
/// whose keyboard target reaches one.
#[test]
fn a_section_or_a_key_target_reaching_one_advertises_the_action() {
    let log = Log::default();
    let mut tree = WidgetTree::new();
    let delegate = tree.add(FillWidget::new());
    let row = tree.add(
        holder(vec![delegate], Some(delegate)).context_menu_contribution(section("move", &log)),
    );
    let view = tree.add(holder(vec![row], Some(row)).focusable(true));
    let plain = tree.add(holder(vec![], None).focusable(true));
    tree.layout(SizeProposal::exact(200.0, 100.0));
    let update = tree.sync_accessibility();
    let offers = |id: WidgetId| {
        let nid = crate::accessibility::widget_id_to_node_id(id);
        update
            .nodes
            .iter()
            .find(|(n, _)| *n == nid)
            .is_some_and(|(_, node)| node.supports_action(accesskit::Action::ShowContextMenu))
    };
    assert!(offers(row), "the row's section");
    assert!(offers(view), "the view, through its row");
    assert!(!offers(plain));
}

/// A finger's hold on a node with a section alone opens it.
#[test]
fn a_hold_opens_a_section_alone() {
    let log = Log::default();
    let mut tree = WidgetTree::new();
    let delegate = tree.add(FillWidget::new());
    tree.add(holder(vec![delegate], None).context_menu_contribution(section("move", &log)));
    tree.layout(SizeProposal::exact(200.0, 100.0));
    let at = tree.bounds(delegate).center();
    tree.long_press_at(teksilo_tokens::PointerKind::Touch, at);
    assert_eq!(*log.borrow(), ["mounted move"]);
    tree.assert_no_leaked_pointer_state();
}
