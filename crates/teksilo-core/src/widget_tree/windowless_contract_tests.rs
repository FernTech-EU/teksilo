// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a tree that belongs to no window can rely on.
//!
//! `teksilo-app` hosts application code that must run with no window open
//! (a tray's menu, a single-instance handler) in a tree of its own, which is
//! never painted and has no `WindowState`. That tree is an ordinary
//! `WidgetTree`; these tests pin the parts of its contract the app layer
//! builds on, so a change here that would break it fails here first:
//!
//! - an intent sent from an application callback reaches an action the root
//!   registered globally, and the action reaches the window operations;
//! - a root rebuilt by a `Rebuild` binding during `layout_with_ops`, the tick
//!   such a tree gets, registers its action again, once;
//! - a mount action runs with the window operations;
//! - a tree with no root drops the context's intents, which is why that tree
//!   always has a root;
//! - a tree that was laid out but never painted still reports
//!   `needs_render`, which is why its tick must not be gated on it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use teksilo_canvas::{Size, SizeProposal};

use crate::binding::BindingLevel;
use crate::signal::Signal;
use crate::window::{TeksiloWindowId, WindowConfig, WindowOps, WindowState};
use crate::{Action, BuildContext, Intent, LayoutContext, LayoutResponse, Widget, WidgetId};

use super::WidgetTree;

/// Records the window operations a tree asks for.
#[derive(Default)]
struct RecordingOps {
    opened: Vec<Option<String>>,
}

impl WindowOps for RecordingOps {
    fn open_window(&mut self, config: WindowConfig) -> TeksiloWindowId {
        self.opened.push(config.string_id.clone());
        TeksiloWindowId::new(self.opened.len() as u64)
    }

    fn find_window(&self, _string_id: &str) -> Option<TeksiloWindowId> {
        None
    }

    fn window_state(&self, _id: TeksiloWindowId) -> Option<WindowState> {
        None
    }

    fn windows(&self) -> Vec<WindowState> {
        Vec::new()
    }

    fn focus_window(&mut self, _id: TeksiloWindowId) {}

    fn close_window_by_id(&mut self, _id: TeksiloWindowId) {}
}

/// A zero-size root that registers one global action, `app.open`, which
/// opens a window and counts its invocations under the build generation
/// that registered it. Rebuilt whenever `generation` changes.
#[derive(Debug)]
struct AppRoot {
    generation: Signal<u32>,
    invoked_by: Rc<RefCell<Vec<u32>>>,
    mounted: Rc<Cell<bool>>,
}

impl Widget for AppRoot {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        self.generation
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);
        let generation = self.generation.get();
        let invoked_by = self.invoked_by.clone();
        ctx.register_action_global(Action::new("app.open").on_invoke(move |_, ctx| {
            invoked_by.borrow_mut().push(generation);
            ctx.open_window(WindowConfig::new().id("opened"));
        }));
        let mounted = self.mounted.clone();
        ctx.run_after_mount(move |ctx| {
            mounted.set(true);
            ctx.open_window(WindowConfig::new().id("from-mount"));
        });
        Vec::new()
    }

    fn layout_response(&self, _: SizeProposal, _: &LayoutContext) -> LayoutResponse {
        Size::new(0.0, 0.0).into()
    }
}

fn app_tree() -> (
    WidgetTree,
    Signal<u32>,
    Rc<RefCell<Vec<u32>>>,
    Rc<Cell<bool>>,
) {
    let generation = Signal::new(0);
    let invoked_by = Rc::new(RefCell::new(Vec::new()));
    let mounted = Rc::new(Cell::new(false));
    let mut tree = WidgetTree::new();
    tree.add(AppRoot {
        generation: generation.clone(),
        invoked_by: invoked_by.clone(),
        mounted: mounted.clone(),
    });
    (tree, generation, invoked_by, mounted)
}

/// The proposal a windowless tree is laid out at: it has no window size.
fn no_window() -> SizeProposal {
    SizeProposal::exact(0.0, 0.0)
}

#[test]
fn an_application_callback_reaches_a_root_action_and_the_window_operations() {
    let (mut tree, _, invoked_by, _) = app_tree();
    let mut ops = RecordingOps::default();
    tree.run_with_event_context(&mut ops, |ctx| {
        assert!(ctx.window().is_none(), "the tree belongs to no window");
        ctx.send_intent(Intent::new("app.open"));
    });
    assert_eq!(*invoked_by.borrow(), [0]);
    assert_eq!(ops.opened, [Some("opened".to_string())]);
}

#[test]
fn a_rebuilt_root_registers_its_action_again_once() {
    let (mut tree, generation, invoked_by, _) = app_tree();
    let mut ops = RecordingOps::default();
    tree.layout_with_ops(no_window(), &mut ops);
    generation.set(1);
    tree.layout_with_ops(no_window(), &mut ops);
    tree.run_with_event_context(&mut ops, |ctx| ctx.send_intent(Intent::new("app.open")));
    assert_eq!(
        *invoked_by.borrow(),
        [1],
        "the action registered by the rebuild answers, and the old one is gone"
    );
}

#[test]
fn a_mount_action_runs_with_the_window_operations() {
    let (mut tree, _, _, mounted) = app_tree();
    let mut ops = RecordingOps::default();
    assert!(tree.has_pending_mount_actions());
    tree.run_mount_actions(&mut ops);
    assert!(mounted.get());
    assert_eq!(ops.opened, [Some("from-mount".to_string())]);
    assert!(!tree.has_pending_mount_actions());
}

#[test]
fn a_tree_with_no_root_drops_the_contexts_intents() {
    let mut tree = WidgetTree::new();
    let mut ops = RecordingOps::default();
    let ran = Rc::new(Cell::new(false));
    let flag = ran.clone();
    tree.run_with_event_context(&mut ops, move |ctx| {
        flag.set(true);
        ctx.send_intent(Intent::new("app.open"));
    });
    assert!(ran.get(), "the callback itself runs");
    assert!(
        tree.pending_intents.is_empty(),
        "but its intent has nowhere to start and is gone"
    );
    let invoked_by = Rc::new(RefCell::new(Vec::new()));
    tree.add(AppRoot {
        generation: Signal::new(0),
        invoked_by: invoked_by.clone(),
        mounted: Rc::new(Cell::new(false)),
    });
    tree.run_with_event_context(&mut ops, |_| {});
    assert!(
        invoked_by.borrow().is_empty(),
        "a root added afterwards never sees the dropped intent"
    );
}

#[test]
fn a_tree_laid_out_but_never_painted_still_needs_render() {
    let (mut tree, _, _, _) = app_tree();
    let mut ops = RecordingOps::default();
    tree.layout_with_ops(no_window(), &mut ops);
    tree.layout_with_ops(no_window(), &mut ops);
    assert!(
        tree.needs_render(),
        "paint never clears it here, so a tick gated on it would run on every turn"
    );
}
