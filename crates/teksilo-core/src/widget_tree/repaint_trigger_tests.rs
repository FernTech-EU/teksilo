// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `RepaintTrigger` in a tree: what a request wakes, what the next frame
//! repaints, relayouts or takes in, and what releases an attachment.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use teksilo_canvas::wake::{CountingWaker, RedrawWaker, WakeKind};
use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use teksilo_tokens::Color;

use super::WidgetTree;
use crate::build_context::BuildContext;
use crate::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use crate::{PullOutcome, RepaintTrigger, WidgetId};

/// Paints what a producer stored, and lays out at the width it stored.
#[derive(Debug)]
struct Probe {
    trigger: RepaintTrigger,
    content: Arc<AtomicU64>,
    width: Arc<AtomicU64>,
    painted: Rc<Cell<Option<u64>>>,
    paints: Rc<Cell<u32>>,
    attach_twice: bool,
}

impl Widget for Probe {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        ctx.attach_repaint_trigger(&self.trigger);
        if self.attach_twice {
            ctx.attach_repaint_trigger(&self.trigger);
        }
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal
            .resolve(self.width.load(Ordering::Relaxed) as f32, 40.0)
            .into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        self.paints.set(self.paints.get() + 1);
        self.painted.set(Some(self.content.load(Ordering::Relaxed)));
        canvas.fill_rect(bounds, Color::from_rgb(0.2, 0.4, 0.6));
    }
}

/// Counts its paints.
#[derive(Debug)]
struct Painter {
    paints: Rc<Cell<u32>>,
}

impl Widget for Painter {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(40.0, 40.0).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        self.paints.set(self.paints.get() + 1);
        canvas.fill_rect(bounds, Color::from_rgb(0.5, 0.5, 0.5));
    }
}

/// Lays its children in a row, each at the size it asks for, and reports the
/// row's size. With `visible_width`, it clips to that width.
#[derive(Debug)]
struct Row {
    children: Vec<WidgetId>,
    visible_width: Option<Rc<Cell<f32>>>,
    culls: Option<Rc<Cell<bool>>>,
}

impl Row {
    fn new(children: Vec<WidgetId>) -> Self {
        Self {
            children,
            visible_width: None,
            culls: None,
        }
    }
}

impl Widget for Row {
    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        if let Some(width) = &self.visible_width {
            return Size::new(width.get(), 40.0).into();
        }
        let width: f32 = self
            .children
            .iter()
            .filter_map(|&id| ctx.child_size(id, SizeProposal::unspecified()))
            .map(|size| size.width)
            .sum();
        proposal.resolve(width, 40.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let mut x = bounds.x;
        for child in children.iter_mut() {
            if let Some(awake) = &self.culls {
                child.dormant = !awake.get();
            }
            let size = ctx
                .child_size(child.id, SizeProposal::unspecified())
                .unwrap_or(Size::new(40.0, 40.0));
            // A clipping row puts its children past a gap, so a narrow clip
            // leaves them out.
            if self.visible_width.is_some() {
                x = bounds.x + 100.0;
            }
            child.origin = Point::new(x, bounds.y);
            child.size = size;
            x += size.width;
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.children.clone()
    }

    fn clips_children(&self) -> bool {
        self.visible_width.is_some()
    }

    fn culls_children(&self) -> bool {
        self.culls.is_some()
    }
}

struct Fixture {
    tree: WidgetTree,
    trigger: RepaintTrigger,
    waker: Arc<CountingWaker>,
    probe: WidgetId,
    root: WidgetId,
    content: Arc<AtomicU64>,
    width: Arc<AtomicU64>,
    painted: Rc<Cell<Option<u64>>>,
    paints: Rc<Cell<u32>>,
    sibling_paints: Rc<Cell<u32>>,
}

impl Fixture {
    fn frame(&mut self) {
        self.tree.layout(SizeProposal::exact(400.0, 300.0));
        let _ = self.tree.render();
    }
}

fn probe(
    trigger: &RepaintTrigger,
) -> (
    Probe,
    Arc<AtomicU64>,
    Arc<AtomicU64>,
    Rc<Cell<Option<u64>>>,
    Rc<Cell<u32>>,
) {
    let content = Arc::new(AtomicU64::new(0));
    let width = Arc::new(AtomicU64::new(40));
    let painted = Rc::new(Cell::new(None));
    let paints = Rc::new(Cell::new(0));
    (
        Probe {
            trigger: trigger.clone(),
            content: content.clone(),
            width: width.clone(),
            painted: painted.clone(),
            paints: paints.clone(),
            attach_twice: false,
        },
        content,
        width,
        painted,
        paints,
    )
}

/// A row of the probe and a painting sibling, with a counting waker
/// installed before anything is built, laid out and rendered once.
fn fixture_with(row: impl FnOnce(Vec<WidgetId>) -> Row) -> Fixture {
    let mut tree = WidgetTree::new();
    let waker = Arc::new(CountingWaker::new());
    tree.set_redraw_waker(Some(waker.clone() as Arc<dyn RedrawWaker>));
    let trigger = RepaintTrigger::new();
    let (probe_widget, content, width, painted, paints) = probe(&trigger);
    let probe = tree.add(probe_widget);
    let sibling_paints = Rc::new(Cell::new(0));
    let sibling = tree.add(Painter {
        paints: sibling_paints.clone(),
    });
    // Under a plain root: a tree's root takes the window's size whatever it
    // reports, and the row's own size is what these tests watch.
    let root = tree.add(row(vec![probe, sibling]));
    tree.add(Row::new(vec![root]));
    let mut f = Fixture {
        tree,
        trigger,
        waker,
        probe,
        root,
        content,
        width,
        painted,
        paints,
        sibling_paints,
    };
    f.frame();
    f
}

fn fixture() -> Fixture {
    fixture_with(Row::new)
}

/// B.16: attaching registers once per widget, a rebuild re-attaches, and the
/// widget's destruction releases it.
#[test]
fn b16_attach_is_idempotent_and_released_with_the_widget() {
    let mut f = fixture();
    assert_eq!(f.tree.repaint_trigger_count(), 1);
    assert!(f.trigger.is_attached());
    assert_eq!(f.trigger.stats().attachments, 1);

    f.tree.arena.get_mut(f.probe).unwrap().dirty.needs_rebuild = true;
    f.frame();
    assert_eq!(f.tree.repaint_trigger_count(), 1, "a rebuild re-attaches");
    assert_eq!(f.trigger.stats().attachments, 1);

    f.tree.destroy_subtree_for_testing(f.probe);
    assert_eq!(f.tree.repaint_trigger_count(), 0);
    assert_eq!(f.trigger.stats().attachments, 0);
}

#[test]
fn b16_a_double_attach_in_one_build_counts_once() {
    let mut tree = WidgetTree::new();
    let trigger = RepaintTrigger::new();
    let (mut widget, ..) = probe(&trigger);
    widget.attach_twice = true;
    tree.add(widget);
    tree.layout(SizeProposal::exact(100.0, 100.0));
    assert_eq!(trigger.stats().attachments, 1);
    assert_eq!(trigger.node_list_len(), 1);
}

/// B.17: a burst wakes the window once, and the next frame repaints the
/// probe alone, with the latest state.
#[test]
fn b17_a_burst_repaints_one_widget_once() {
    let mut f = fixture();
    let (paints, sibling) = (f.paints.get(), f.sibling_paints.get());
    for value in 1..=100 {
        f.content.store(value, Ordering::Relaxed);
        f.trigger.request_repaint();
    }
    assert_eq!(f.waker.count_of(WakeKind::Draw), 1);
    f.frame();
    assert_eq!(f.paints.get(), paints + 1);
    assert_eq!(f.painted.get(), Some(100), "the latest state");
    assert_eq!(f.sibling_paints.get(), sibling, "nothing else repaints");
    let s = f.trigger.stats();
    assert_eq!(
        (s.requests, s.wakes, s.wakes_coalesced, s.repaints),
        (100, 1, 99, 1)
    );
    f.trigger.request_repaint();
    assert_eq!(f.waker.count(), 2, "painted, the next request wakes again");
}

/// B.18: a relayout wakes with `Layout`, and the new size reaches the
/// parent's layout.
#[test]
fn b18_a_relayout_reaches_the_parent() {
    let mut f = fixture();
    let sibling = f.sibling_paints.get();
    let before = f.tree.bounds(f.root).width;
    f.width.store(120, Ordering::Relaxed);
    f.trigger.request_relayout();
    assert_eq!(f.waker.count_of(WakeKind::Layout), 1);
    // The pre-pass marks the widget and its ancestors, as a `Relayout`
    // binding does.
    f.tree.poll_off_thread();
    assert!(f.tree.arena.get(f.probe).unwrap().dirty.needs_layout);
    assert!(f.tree.arena.get(f.root).unwrap().dirty.needs_layout);
    f.frame();
    assert_eq!(f.tree.bounds(f.probe).width, 120.0);
    assert_eq!(f.tree.bounds(f.root).width, before + 80.0, "the row grew");
    assert_eq!(f.trigger.stats().relayouts, 1);
    let _ = sibling;
}

/// B.19: a widget clipped out keeps its request for when it is painted: one
/// wake per burst, its stale paint dropped once, and no frame after that.
#[test]
fn b19_a_clipped_out_widget_absorbs_a_burst_in_one_wake() {
    let visible = Rc::new(Cell::new(50.0));
    let mut f = fixture_with(|children| Row {
        children,
        visible_width: Some(visible.clone()),
        culls: None,
    });
    let paints = f.paints.get();
    f.content.store(7, Ordering::Relaxed);
    for _ in 0..10 {
        f.trigger.request_repaint();
    }
    assert_eq!(f.waker.count(), 1);
    f.frame();
    assert_eq!(f.paints.get(), paints, "clipped out: not painted");
    for _ in 0..10 {
        f.trigger.request_repaint();
    }
    assert_eq!(f.waker.count(), 1, "the request is still pending: no wake");
    f.tree.layout(SizeProposal::exact(400.0, 300.0));
    assert!(!f.tree.needs_render(), "and no frame");

    visible.set(200.0);
    f.tree.arena.mark_needs_layout(f.root);
    f.frame();
    assert_eq!(f.paints.get(), paints + 1, "painted once in view");
    assert_eq!(f.painted.get(), Some(7));
    f.trigger.request_repaint();
    assert_eq!(f.waker.count(), 2);
}

/// B.20: a dormant widget absorbs a burst in one wake and repaints on
/// activation with the latest state.
#[test]
fn b20_a_dormant_widget_repaints_on_activation() {
    let mut f = fixture();
    f.tree.set_dormant(f.probe);
    f.frame();
    let paints = f.paints.get();
    f.content.store(3, Ordering::Relaxed);
    f.trigger.request_repaint();
    f.trigger.request_repaint();
    assert_eq!(f.waker.count(), 1);
    f.frame();
    f.trigger.request_repaint();
    assert_eq!(f.waker.count(), 1);
    assert!(!f.tree.needs_render());
    assert_eq!(f.trigger.stats().repaints, 0);

    f.tree.activate(f.probe);
    f.frame();
    assert_eq!(f.paints.get(), paints + 1);
    assert_eq!(f.painted.get(), Some(3));
    f.trigger.request_repaint();
    assert_eq!(f.waker.count(), 2);
}

/// A relayout requested while dormant is taken by the activation itself, not
/// by a later pass: a request made right after it wakes the window again.
#[test]
fn a_relayout_requested_while_dormant_is_taken_by_activation() {
    let mut f = fixture();
    f.tree.set_dormant(f.probe);
    f.frame();
    f.width.store(90, Ordering::Relaxed);
    f.trigger.request_relayout();
    assert_eq!(f.waker.count(), 1);
    f.frame();
    assert_eq!(f.trigger.stats().relayouts, 0, "dormant: still pending");
    f.tree.activate(f.probe);
    assert_eq!(f.trigger.stats().relayouts, 1, "the activation took it");
    f.trigger.request_relayout();
    assert_eq!(f.waker.count(), 2, "so the next request wakes again");
    f.frame();
    assert_eq!(f.tree.bounds(f.probe).width, 90.0);
}

/// B.20b: a widget a culling parent wakes inside the walk, with a relayout
/// pending, is laid out again on the next pass.
#[test]
fn b20b_a_widget_woken_mid_walk_with_a_relayout_is_laid_out_again() {
    let awake = Rc::new(Cell::new(false));
    let mut f = fixture_with(|children| Row {
        children,
        visible_width: None,
        culls: Some(awake.clone()),
    });
    f.width.store(70, Ordering::Relaxed);
    f.trigger.request_relayout();
    assert_eq!(f.waker.count(), 1);
    awake.set(true);
    f.tree.arena.mark_needs_layout(f.root);
    f.tree.layout(SizeProposal::exact(400.0, 300.0));
    assert!(
        f.tree.arena.get(f.probe).unwrap().dirty.needs_layout,
        "woken mid-walk with a relayout pending: laid out again next pass"
    );
    assert_eq!(f.trigger.stats().relayouts, 1, "the activation took it");
    f.frame();
    assert_eq!(f.tree.bounds(f.probe).width, 70.0);
    f.trigger.request_relayout();
    assert_eq!(f.waker.count(), 2, "taken, the next request wakes again");
}

/// B.21: two triggers on one widget wake once; one trigger in two trees
/// wakes each tree's window once.
#[test]
fn b21_one_wake_per_window() {
    let mut tree = WidgetTree::new();
    let waker = Arc::new(CountingWaker::new());
    tree.set_redraw_waker(Some(waker.clone() as Arc<dyn RedrawWaker>));
    let (first, second) = (RepaintTrigger::new(), RepaintTrigger::new());

    #[derive(Debug)]
    struct Two(RepaintTrigger, RepaintTrigger);
    impl Widget for Two {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            ctx.attach_repaint_trigger(&self.0);
            ctx.attach_repaint_trigger(&self.1);
            vec![]
        }
        fn layout_response(&self, p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
            p.resolve(10.0, 10.0).into()
        }
    }
    tree.add(Two(first.clone(), second.clone()));
    tree.layout(SizeProposal::exact(100.0, 100.0));
    first.request_repaint();
    second.request_repaint();
    assert_eq!(waker.count(), 1, "one widget state, one wake");

    let shared = RepaintTrigger::new();
    let (a, b) = (
        Arc::new(CountingWaker::new()),
        Arc::new(CountingWaker::new()),
    );
    let mut trees = Vec::new();
    for waker in [&a, &b] {
        let mut tree = WidgetTree::new();
        tree.set_redraw_waker(Some(waker.clone() as Arc<dyn RedrawWaker>));
        let (widget, ..) = probe(&shared);
        tree.add(widget);
        tree.layout(SizeProposal::exact(100.0, 100.0));
        trees.push(tree);
    }
    shared.request_repaint();
    assert_eq!((a.count(), b.count()), (1, 1));
}

/// B.22: dropping the tree releases every attachment.
#[test]
fn b22_dropping_the_tree_releases_the_attachments() {
    let f = fixture();
    let (trigger, waker) = (f.trigger.clone(), f.waker.clone());
    drop(f);
    assert_eq!(trigger.stats().attachments, 0);
    trigger.request_repaint();
    assert_eq!(waker.count(), 0);
    assert_eq!(trigger.node_list_len(), 0, "pruned");
}

/// F.1 (strong): a widget that attaches in its very first build, before any
/// frame, wakes the window on its first request.
#[test]
fn f1_a_widget_attached_in_its_first_build_wakes_its_window() {
    let mut tree = WidgetTree::new();
    let waker = Arc::new(CountingWaker::new());
    tree.set_redraw_waker(Some(waker.clone() as Arc<dyn RedrawWaker>));
    let trigger = RepaintTrigger::new();
    let (widget, content, _, painted, _) = probe(&trigger);
    tree.add(widget);
    tree.layout(SizeProposal::exact(100.0, 100.0));
    let remote = trigger.clone();
    std::thread::spawn(move || {
        content.store(9, Ordering::Relaxed);
        remote.request_repaint();
    })
    .join()
    .unwrap();
    assert_eq!(waker.count(), 1);
    let _ = tree.render();
    assert_eq!(painted.get(), Some(9));
}

/// A widget destroyed behind the tree's back is found orphaned and released
/// by the next pre-pass.
#[test]
fn an_orphaned_attachment_is_released_by_the_next_pass() {
    let mut f = fixture();
    f.tree.arena.destroy(f.probe);
    f.tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(f.tree.off_thread.trigger_node_count(), 0);
    assert_eq!(f.trigger.stats().attachments, 0);
}

/// A widget rebuilt a thousand times without a request keeps one entry.
#[test]
fn rebuilds_without_requests_do_not_grow_the_trigger() {
    let mut f = fixture();
    for _ in 0..1000 {
        f.tree.arena.get_mut(f.probe).unwrap().dirty.needs_rebuild = true;
        f.tree.layout(SizeProposal::exact(400.0, 300.0));
    }
    assert!(f.trigger.node_list_len() <= 1);
}

/// A request made before the tree has a waker wakes the window when one is
/// installed.
#[test]
fn a_waker_installed_late_is_woken_for_what_was_requested() {
    let mut tree = WidgetTree::new();
    let trigger = RepaintTrigger::new();
    let (widget, ..) = probe(&trigger);
    tree.add(widget);
    tree.layout(SizeProposal::exact(100.0, 100.0));
    let _ = tree.render();
    trigger.request_repaint();
    let waker = Arc::new(CountingWaker::new());
    tree.set_redraw_waker(Some(waker.clone() as Arc<dyn RedrawWaker>));
    assert_eq!(waker.count(), 1);
}

/// A widget that takes its content in through a pull hook.
#[derive(Debug)]
struct Consumer {
    trigger: RepaintTrigger,
    incoming: Arc<Mutex<Vec<u64>>>,
    taken: Rc<RefCell<Vec<u64>>>,
    hook_runs: Rc<Cell<u32>>,
    painted_len: Rc<Cell<usize>>,
    paints: Rc<Cell<u32>>,
}

impl Widget for Consumer {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        ctx.attach_repaint_trigger(&self.trigger);
        let (incoming, taken, runs) = (
            self.incoming.clone(),
            self.taken.clone(),
            self.hook_runs.clone(),
        );
        ctx.on_trigger_pull(move || {
            runs.set(runs.get() + 1);
            let new: Vec<u64> = std::mem::take(&mut *incoming.lock().unwrap());
            if new.is_empty() {
                return PullOutcome::Unchanged;
            }
            taken.borrow_mut().extend(new);
            PullOutcome::Repaint
        });
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(40.0, 40.0).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        self.paints.set(self.paints.get() + 1);
        self.painted_len.set(self.taken.borrow().len());
        canvas.fill_rect(bounds, Color::from_rgb(0.1, 0.1, 0.1));
    }
}

struct PullFixture {
    tree: WidgetTree,
    trigger: RepaintTrigger,
    waker: Arc<CountingWaker>,
    id: WidgetId,
    incoming: Arc<Mutex<Vec<u64>>>,
    taken: Rc<RefCell<Vec<u64>>>,
    hook_runs: Rc<Cell<u32>>,
    painted_len: Rc<Cell<usize>>,
    paints: Rc<Cell<u32>>,
}

impl PullFixture {
    fn new() -> Self {
        let mut tree = WidgetTree::new();
        let waker = Arc::new(CountingWaker::new());
        tree.set_redraw_waker(Some(waker.clone() as Arc<dyn RedrawWaker>));
        let trigger = RepaintTrigger::new();
        let (incoming, taken, hook_runs, painted_len, paints) = (
            Arc::new(Mutex::new(Vec::new())),
            Rc::new(RefCell::new(Vec::new())),
            Rc::new(Cell::new(0)),
            Rc::new(Cell::new(0)),
            Rc::new(Cell::new(0)),
        );
        let id = tree.add(Consumer {
            trigger: trigger.clone(),
            incoming: incoming.clone(),
            taken: taken.clone(),
            hook_runs: hook_runs.clone(),
            painted_len: painted_len.clone(),
            paints: paints.clone(),
        });
        tree.add(Row::new(vec![id]));
        let mut f = Self {
            tree,
            trigger,
            waker,
            id,
            incoming,
            taken,
            hook_runs,
            painted_len,
            paints,
        };
        f.frame();
        f
    }

    fn frame(&mut self) {
        self.tree.layout(SizeProposal::exact(400.0, 300.0));
        let _ = self.tree.render();
    }

    fn produce(&self, value: u64) {
        self.incoming.lock().unwrap().push(value);
        self.trigger.request_pull();
    }
}

/// A pull for a widget that is shown wakes its window once, and the window
/// is told to draw a frame for it; the hook takes the data in and the widget
/// repaints with it.
#[test]
fn a_pull_for_a_shown_widget_is_taken_in_and_painted() {
    let mut f = PullFixture::new();
    let paints = f.paints.get();
    f.produce(1);
    f.produce(2);
    assert_eq!(f.waker.count_of(WakeKind::Layout), 1, "one state wake");
    assert!(
        f.tree.off_thread_needs_frame(),
        "shown: draw a frame for it"
    );
    f.frame();
    assert!(!f.tree.off_thread_needs_frame(), "taken in");
    assert_eq!(f.hook_runs.get(), 1);
    assert_eq!(*f.taken.borrow(), vec![1, 2]);
    assert_eq!(f.paints.get(), paints + 1);
    assert_eq!(f.painted_len.get(), 2);
    assert_eq!(f.trigger.stats().pulls, 1);
}

/// A pull for a dormant widget wakes its window with a state wake; the
/// next pass takes the data in without drawing anything, and the widget
/// shows it when it is shown again.
#[test]
fn a_pull_for_a_widget_not_shown_is_taken_in_without_drawing() {
    let mut f = PullFixture::new();
    f.tree.set_dormant(f.id);
    f.frame();
    let paints = f.paints.get();
    f.produce(5);
    assert_eq!(f.waker.count_of(WakeKind::Layout), 1, "a state wake");
    assert!(
        !f.tree.off_thread_needs_frame(),
        "not shown: no frame drawn for it"
    );
    f.tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(*f.taken.borrow(), vec![5], "taken in while dormant");
    assert!(!f.tree.needs_render(), "and nothing to draw for it");
    f.produce(6);
    assert_eq!(f.waker.count(), 2, "taken, the next pull wakes again");
    f.tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(*f.taken.borrow(), vec![5, 6]);

    f.tree.activate(f.id);
    f.frame();
    assert_eq!(f.paints.get(), paints + 1, "shown again, it repaints");
    assert_eq!(f.painted_len.get(), 2, "with everything taken in");
}

/// A pull for a widget clipped out of view is taken in, and its window
/// draws no frame for it: its stale paint is dropped instead, for when it
/// comes into view.
#[test]
fn a_pull_for_a_clipped_out_widget_draws_nothing() {
    let mut tree = WidgetTree::new();
    let waker = Arc::new(CountingWaker::new());
    tree.set_redraw_waker(Some(waker.clone() as Arc<dyn RedrawWaker>));
    let trigger = RepaintTrigger::new();
    let incoming = Arc::new(Mutex::new(Vec::new()));
    let taken = Rc::new(RefCell::new(Vec::new()));
    let paints = Rc::new(Cell::new(0));
    let painted_len = Rc::new(Cell::new(0));
    let id = tree.add(Consumer {
        trigger: trigger.clone(),
        incoming: incoming.clone(),
        taken: taken.clone(),
        hook_runs: Rc::new(Cell::new(0)),
        painted_len: painted_len.clone(),
        paints: paints.clone(),
    });
    let visible = Rc::new(Cell::new(50.0));
    let clip = tree.add(Row {
        children: vec![id],
        visible_width: Some(visible.clone()),
        culls: None,
    });
    tree.add(Row::new(vec![clip]));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let _ = tree.render();
    assert_eq!(paints.get(), 0, "clipped out");

    incoming.lock().unwrap().push(1);
    trigger.request_pull();
    assert_eq!(waker.count_of(WakeKind::Layout), 1, "a state wake");
    assert!(
        !tree.off_thread_needs_frame(),
        "clipped out: no frame for it"
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(*taken.borrow(), vec![1]);
    assert!(!tree.needs_render(), "no frame for a widget nobody sees");

    visible.set(200.0);
    tree.arena.mark_needs_layout(clip);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let _ = tree.render();
    assert_eq!(paints.get(), 1, "in view, it paints");
    assert_eq!(painted_len.get(), 1, "what it took in");
}

/// A hook that reports a relayout lays the widget and its ancestors out
/// again when active, and leaves it for the activation when dormant.
#[test]
fn a_pull_that_relayouts_while_dormant_waits_for_the_activation() {
    #[derive(Debug)]
    struct Grows {
        trigger: RepaintTrigger,
        width: Rc<Cell<f32>>,
        pending: Arc<AtomicU64>,
    }
    impl Widget for Grows {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            ctx.attach_repaint_trigger(&self.trigger);
            let (width, pending) = (self.width.clone(), self.pending.clone());
            ctx.on_trigger_pull(move || {
                width.set(pending.load(Ordering::Relaxed) as f32);
                PullOutcome::Relayout
            });
            vec![]
        }
        fn layout_response(&self, p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
            p.resolve(self.width.get(), 40.0).into()
        }
    }
    let mut tree = WidgetTree::new();
    let trigger = RepaintTrigger::new();
    let width = Rc::new(Cell::new(40.0));
    let pending = Arc::new(AtomicU64::new(40));
    let id = tree.add(Grows {
        trigger: trigger.clone(),
        width: width.clone(),
        pending: pending.clone(),
    });
    let root = tree.add(Row::new(vec![id]));
    tree.add(Row::new(vec![root]));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    pending.store(60, Ordering::Relaxed);
    trigger.request_pull();
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(tree.bounds(id).width, 60.0, "active: laid out at once");
    assert_eq!(tree.bounds(root).width, 60.0);

    tree.set_dormant(id);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    pending.store(80, Ordering::Relaxed);
    trigger.request_pull();
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(width.get(), 80.0, "taken in while dormant");
    tree.activate(id);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(tree.bounds(id).width, 80.0);
    assert_eq!(
        trigger.stats().relayouts,
        1,
        "the activation took the relayout"
    );
}

/// B.23: a widget wrapped by a handler builder attaches like any other.
#[test]
fn b23_a_widget_with_handlers_attaches_its_trigger() {
    use crate::widget_builder::WidgetBuilder;
    let mut tree = WidgetTree::new();
    let waker = Arc::new(CountingWaker::new());
    tree.set_redraw_waker(Some(waker.clone() as Arc<dyn RedrawWaker>));
    let trigger = RepaintTrigger::new();
    let (widget, content, _, painted, _) = probe(&trigger);
    tree.add(widget.on_tap(|_, _| {}));
    tree.layout(SizeProposal::exact(100.0, 100.0));
    let _ = tree.render();
    content.store(4, Ordering::Relaxed);
    trigger.request_repaint();
    assert_eq!(waker.count(), 1);
    tree.layout(SizeProposal::exact(100.0, 100.0));
    let _ = tree.render();
    assert_eq!(painted.get(), Some(4));
    assert_eq!(trigger.stats().repaints, 1);
}

impl PullFixture {
    fn rebuild(&mut self) {
        self.tree
            .arena
            .get_mut(self.id)
            .unwrap()
            .dirty
            .needs_rebuild = true;
        self.tree.layout(SizeProposal::exact(400.0, 300.0));
    }
}

/// A pull requested before the widget is rebuilt is still pending after
/// it, and runs the hook the new build set.
#[test]
fn a_pending_pull_survives_a_rebuild() {
    let mut f = PullFixture::new();
    f.produce(1);
    f.rebuild();
    assert_eq!(f.hook_runs.get(), 1, "the pass that rebuilt it took it in");
    f.produce(2);
    let state = f
        .tree
        .arena
        .get(f.id)
        .unwrap()
        .repaint_wake
        .clone()
        .unwrap();
    // A rebuild before the next pass: the request is still there after it.
    f.tree.rebuild_single_widget(f.id);
    assert!(state.state_pending(), "still pending after the rebuild");
    assert!(
        Arc::ptr_eq(
            f.tree
                .arena
                .get(f.id)
                .unwrap()
                .repaint_wake
                .as_ref()
                .unwrap(),
            &state
        ),
        "the same wake state"
    );
    f.frame();
    assert_eq!(*f.taken.borrow(), vec![1, 2]);
    assert_eq!(f.hook_runs.get(), 2);
    assert_eq!(f.trigger.stats().attachments, 1);
}

/// A request made while the widget is being rebuilt, before its new build
/// has attached the trigger again, is not lost.
#[test]
fn a_request_made_during_a_rebuild_is_kept() {
    #[derive(Debug)]
    struct RequestsInBuild {
        trigger: RepaintTrigger,
        armed: Rc<Cell<bool>>,
        runs: Rc<Cell<u32>>,
    }
    impl Widget for RequestsInBuild {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            // What a producer thread can do at this moment: request while the
            // rebuild is between releasing and re-attaching.
            if self.armed.replace(false) {
                self.trigger.request_pull();
            }
            ctx.attach_repaint_trigger(&self.trigger);
            let runs = self.runs.clone();
            ctx.on_trigger_pull(move || {
                runs.set(runs.get() + 1);
                PullOutcome::Unchanged
            });
            vec![]
        }
        fn layout_response(&self, p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
            p.resolve(10.0, 10.0).into()
        }
    }
    let mut tree = WidgetTree::new();
    let waker = Arc::new(CountingWaker::new());
    tree.set_redraw_waker(Some(waker.clone() as Arc<dyn RedrawWaker>));
    let trigger = RepaintTrigger::new();
    let (armed, runs) = (Rc::new(Cell::new(false)), Rc::new(Cell::new(0)));
    let id = tree.add(RequestsInBuild {
        trigger: trigger.clone(),
        armed: armed.clone(),
        runs: runs.clone(),
    });
    tree.layout(SizeProposal::exact(100.0, 100.0));
    assert_eq!(runs.get(), 0);
    armed.set(true);
    tree.rebuild_single_widget(id);
    assert_eq!(waker.count(), 1, "the request woke the window");
    tree.layout(SizeProposal::exact(100.0, 100.0));
    assert_eq!(runs.get(), 1, "and the next pass took it in");
}

/// A rebuild whose new build no longer attaches a trigger releases that one
/// alone; the widget's other trigger stays attached.
#[test]
fn a_rebuild_releases_only_the_triggers_it_no_longer_attaches() {
    #[derive(Debug)]
    struct Picks {
        triggers: [RepaintTrigger; 2],
        both: Rc<Cell<bool>>,
    }
    impl Widget for Picks {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            ctx.attach_repaint_trigger(&self.triggers[0]);
            if self.both.get() {
                ctx.attach_repaint_trigger(&self.triggers[1]);
            }
            vec![]
        }
        fn layout_response(&self, p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
            p.resolve(10.0, 10.0).into()
        }
    }
    let mut tree = WidgetTree::new();
    let waker = Arc::new(CountingWaker::new());
    tree.set_redraw_waker(Some(waker.clone() as Arc<dyn RedrawWaker>));
    let (kept, dropped) = (RepaintTrigger::new(), RepaintTrigger::new());
    let both = Rc::new(Cell::new(true));
    let id = tree.add(Picks {
        triggers: [kept.clone(), dropped.clone()],
        both: both.clone(),
    });
    tree.layout(SizeProposal::exact(100.0, 100.0));
    assert_eq!(
        (kept.stats().attachments, dropped.stats().attachments),
        (1, 1)
    );
    both.set(false);
    tree.rebuild_single_widget(id);
    assert_eq!(kept.stats().attachments, 1);
    assert_eq!(dropped.stats().attachments, 0, "released");
    assert_eq!(dropped.node_list_len(), 0);
    dropped.request_repaint();
    assert_eq!(waker.count(), 0, "it wakes nothing now");
    kept.request_repaint();
    assert_eq!(waker.count(), 1);

    // A rebuild that attaches nothing and sets no hook releases the widget's
    // wake state too.
    #[derive(Debug)]
    struct Nothing;
    impl Widget for Nothing {
        fn layout_response(&self, p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
            p.resolve(10.0, 10.0).into()
        }
    }
    let _ = tree.arena.take_widget(id);
    tree.arena.restore_widget(id, Box::new(Nothing));
    tree.rebuild_single_widget(id);
    assert_eq!(kept.stats().attachments, 0);
    assert_eq!(tree.repaint_trigger_count(), 0);
    assert!(tree.arena.get(id).unwrap().repaint_wake.is_none());
}

/// A widget that painted, is then clipped out without moving, and pulls
/// content in meanwhile repaints when it comes back into view: what it painted
/// before is dropped, never replayed.
#[test]
fn a_pull_while_clipped_out_drops_the_stale_paint() {
    let mut tree = WidgetTree::new();
    let waker = Arc::new(CountingWaker::new());
    tree.set_redraw_waker(Some(waker.clone() as Arc<dyn RedrawWaker>));
    let trigger = RepaintTrigger::new();
    let incoming = Arc::new(Mutex::new(vec![1]));
    let taken = Rc::new(RefCell::new(Vec::new()));
    let paints = Rc::new(Cell::new(0));
    let painted_len = Rc::new(Cell::new(0));
    let id = tree.add(Consumer {
        trigger: trigger.clone(),
        incoming: incoming.clone(),
        taken: taken.clone(),
        hook_runs: Rc::new(Cell::new(0)),
        painted_len: painted_len.clone(),
        paints: paints.clone(),
    });
    let visible = Rc::new(Cell::new(200.0));
    let clip = tree.add(Row {
        children: vec![id],
        visible_width: Some(visible.clone()),
        culls: None,
    });
    tree.add(Row::new(vec![clip]));
    trigger.request_pull();
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let _ = tree.render();
    assert_eq!((paints.get(), painted_len.get()), (1, 1), "in view");
    let bounds = tree.bounds(id);

    visible.set(50.0);
    tree.arena.mark_needs_layout(clip);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let _ = tree.render();
    assert_eq!(tree.bounds(id), bounds, "clipped out where it was");
    assert_eq!(paints.get(), 1);

    incoming.lock().unwrap().push(2);
    trigger.request_pull();
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(*taken.borrow(), vec![1, 2]);
    assert!(
        tree.arena.get(id).unwrap().cached_paint.is_none(),
        "dropped"
    );

    visible.set(200.0);
    tree.arena.mark_needs_layout(clip);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let _ = tree.render();
    assert_eq!(paints.get(), 2, "back in view, it paints again");
    assert_eq!(painted_len.get(), 2, "what it took in while out of view");
}

/// A widget made invisible by an opacity of its own is not shown: a pull
/// draws no frame for it.
#[test]
fn a_transparent_widget_is_not_shown() {
    let mut f = PullFixture::new();
    f.tree.arena.get_mut(f.id).unwrap().opacity_prop = Some(crate::signal::Prop::Static(0.0));
    f.tree.arena.mark_needs_paint(f.id);
    f.frame();
    f.produce(1);
    assert!(!f.tree.off_thread_needs_frame());
    f.tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(*f.taken.borrow(), vec![1]);
    assert!(!f.tree.needs_render(), "and nothing marked for it");
}
