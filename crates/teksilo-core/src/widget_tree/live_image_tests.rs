// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Live images in a tree: attachments, the layout pre-pass, what a commit
//! wakes and what the next frame does, and the lookups through the tree
//! (spec B.1, B.2, B.5-B.8, B.10-B.13, AC1-AC5), on the renderer's own live
//! pass with textures in memory.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use teksilo_canvas::live_image::testing::{LiveImageMirror, MirrorReport};
use teksilo_canvas::live_image::{
    LiveImageDraw, LiveImageSource, LiveImageStatus, LiveImageWriter, LivePixelFormat, PixelRect,
};
use teksilo_canvas::wake::{CountingWaker, RedrawWaker, WakeKind};
use teksilo_canvas::{Canvas, ImageGeometry, Point, Rect, Size, SizeProposal};

use super::WidgetTree;
use crate::binding::BindingLevel;
use crate::build_context::BuildContext;
use crate::event_source::{AppEventPoster, SubscriptionId, TreeAppContext};
use crate::signal::Signal;
use crate::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use crate::widget_builder::WidgetBuilder;
use crate::{LiveImageAttachment, LiveImageSignals, WidgetId};

/// Shows a source at its natural size, as `LiveImage` would at its
/// simplest: attaches in `build()`, binds the size at `Relayout` and the
/// status at `RepaintOnly`, draws one quad per paint.
#[derive(Debug)]
struct Live {
    source: LiveImageSource,
    signals: LiveImageSignals,
    attachment: Rc<RefCell<Option<LiveImageAttachment>>>,
    paints: Rc<Cell<u32>>,
}

impl Live {
    fn new(source: &LiveImageSource) -> Self {
        Self {
            source: source.clone(),
            signals: LiveImageSignals::new(source),
            attachment: Rc::new(RefCell::new(None)),
            paints: Rc::new(Cell::new(0)),
        }
    }
}

impl Widget for Live {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let attachment = ctx.attach_live_image(&self.source, &self.signals);
        let id = ctx.self_id();
        self.signals
            .frame_size
            .bind_to(id, ctx.binding_registry(), BindingLevel::Relayout);
        self.signals
            .status
            .bind_to(id, ctx.binding_registry(), BindingLevel::RepaintOnly);
        *self.attachment.borrow_mut() = Some(attachment);
        vec![]
    }

    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        let (w, h) = self.signals.frame_size.get().unwrap_or((0, 0));
        Size::new(w as f32, h as f32).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        if let (Some(a), Some(size)) = (
            self.attachment.borrow().as_ref(),
            self.signals.frame_size.get(),
        ) {
            a.set_geometry(ImageGeometry::new(
                size,
                Default::default(),
                Rect::new(0.0, 0.0, bounds.width, bounds.height),
                Rect::new(0.0, 0.0, bounds.width, bounds.height),
            ));
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        self.paints.set(self.paints.get() + 1);
        if let Some(a) = self.attachment.borrow().as_ref() {
            canvas.draw_live_image(a.consumer(), &LiveImageDraw::new(bounds, bounds));
        }
    }
}

/// Counts its paints.
#[derive(Debug)]
struct Painter(Rc<Cell<u32>>);

impl Widget for Painter {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(30.0, 30.0).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        self.0.set(self.0.get() + 1);
        canvas.fill_rect(bounds, teksilo_tokens::Color::from_rgb(0.1, 0.2, 0.3));
    }
}

/// Lays children in a row at the sizes they ask for. With `visible_width`
/// it clips there and places its children past it; with `awake`, a child is
/// dormant while it is false.
#[derive(Debug)]
struct Row {
    children: Vec<WidgetId>,
    visible_width: Option<f32>,
    awake: Option<Rc<Cell<bool>>>,
    layouts: Rc<Cell<u32>>,
}

impl Row {
    fn new(children: Vec<WidgetId>) -> Self {
        Self {
            children,
            visible_width: None,
            awake: None,
            layouts: Rc::new(Cell::new(0)),
        }
    }
}

impl Widget for Row {
    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.layouts.set(self.layouts.get() + 1);
        if let Some(width) = self.visible_width {
            return Size::new(width, 40.0).into();
        }
        let (mut w, mut h) = (0.0f32, 0.0f32);
        for &id in &self.children {
            // Measured whether or not the child is dormant, as a culling
            // widget knows where its parked items go.
            let size = ctx
                .measure_intrinsic(id, SizeProposal::unspecified())
                .unwrap_or_default();
            w += size.width;
            h = h.max(size.height);
        }
        proposal.resolve(w, h).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let mut x = bounds.x
            + if self.visible_width.is_some() {
                100.0
            } else {
                0.0
            };
        for child in children.iter_mut() {
            if let Some(awake) = &self.awake {
                child.dormant = !awake.get();
            }
            let size = ctx
                .measure_intrinsic(child.id, SizeProposal::unspecified())
                .unwrap_or_default();
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
        self.awake.is_some()
    }
}

/// Counts what is posted to the event loop: a live image posts nothing.
#[derive(Default)]
struct RecordingPoster(Mutex<usize>);

impl AppEventPoster for RecordingPoster {
    fn post_subscription_event(&self, _: SubscriptionId, _: Box<dyn std::any::Any + Send>) {
        *self.0.lock().unwrap() += 1;
    }
    fn post_external(&self, _: Box<dyn std::any::Any + Send>) {
        *self.0.lock().unwrap() += 1;
    }
}

fn frame_of(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            px.extend_from_slice(&[x as u8, y as u8, seed, 255]);
        }
    }
    px
}

struct Fixture {
    tree: WidgetTree,
    waker: Arc<CountingWaker>,
    poster: Arc<RecordingPoster>,
    source: LiveImageSource,
    writer: LiveImageWriter,
    live: WidgetId,
    row: WidgetId,
    attachment: Rc<RefCell<Option<LiveImageAttachment>>>,
    paints: Rc<Cell<u32>>,
    sibling_paints: Rc<Cell<u32>>,
    row_layouts: Rc<Cell<u32>>,
    mirror: LiveImageMirror,
}

impl Fixture {
    fn with(row: impl FnOnce(Row) -> Row, source: LiveImageSource) -> Self {
        let mut tree = WidgetTree::new();
        let waker = Arc::new(CountingWaker::new());
        tree.set_redraw_waker(Some(waker.clone() as Arc<dyn RedrawWaker>));
        let poster = Arc::new(RecordingPoster::default());
        tree.set_app_context(Rc::new(
            TreeAppContext::empty().with_poster(poster.clone() as Arc<dyn AppEventPoster>),
        ));
        let writer = source.writer();
        let widget = Live::new(&source);
        let (attachment, paints) = (widget.attachment.clone(), widget.paints.clone());
        let live = tree.add(widget);
        let sibling_paints = Rc::new(Cell::new(0));
        let sibling = tree.add(Painter(sibling_paints.clone()));
        let row = row(Row::new(vec![live, sibling]));
        let row_layouts = row.layouts.clone();
        let row = tree.add(row);
        tree.add(Row::new(vec![row]));
        let mut f = Self {
            tree,
            waker,
            poster,
            source,
            writer,
            live,
            row,
            attachment,
            paints,
            sibling_paints,
            row_layouts,
            mirror: LiveImageMirror::new(),
        };
        f.frame();
        f
    }

    fn new() -> Self {
        let f = Self::with(|r| r, LiveImageSource::new(LivePixelFormat::Rgba8));
        f.writer.write_frame(8, 6, &frame_of(8, 6, 0), 32).unwrap();
        let mut f = f;
        f.frame();
        f
    }

    /// One frame: layout, render, the live pass.
    fn frame(&mut self) -> MirrorReport {
        self.tree.layout(SizeProposal::exact(400.0, 300.0));
        let frame = self.tree.render();
        self.mirror.consume(&frame)
    }

    fn commit_pixel(&self, x: u32, seed: u8) {
        self.writer
            .write_rect(PixelRect::new(x, 0, 1, 1), &[seed; 4], 4)
            .unwrap();
    }

    fn shown(&self) -> Vec<u8> {
        self.mirror
            .pixels(self.source.id())
            .map(|(_, _, p)| p.to_vec())
            .expect("a texture")
    }
}

// ── B.1 ──

#[test]
fn b01_attaching_registers_and_a_rebuild_or_a_destroy_detaches() {
    let mut f = Fixture::new();
    assert_eq!(f.tree.live_image_attachment_count(), 1);
    assert_eq!(f.source.stats().attachments, 1);
    let first = f.attachment.borrow().clone().unwrap();
    f.tree.arena.get_mut(f.live).unwrap().dirty.needs_rebuild = true;
    let report = f.frame();
    assert!(
        first.is_detached(),
        "the rebuild detached the old attachment"
    );
    assert_eq!(f.tree.live_image_attachment_count(), 1, "and attached anew");
    assert_eq!(f.source.stats().attachments, 1);
    assert_eq!(
        report.full_uploads + report.partial_uploads,
        0,
        "the texture, keyed by source, survives the rebuild"
    );
    f.tree.destroy_subtree_for_testing(f.live);
    assert_eq!(f.tree.live_image_attachment_count(), 0);
    assert_eq!(f.source.stats().attachments, 0);
}

/// `Live`, attaching a `RepaintTrigger` too while `with_trigger` holds.
#[derive(Debug)]
struct LiveWithTrigger {
    live: Live,
    trigger: crate::RepaintTrigger,
    with_trigger: Rc<Cell<bool>>,
}

impl Widget for LiveWithTrigger {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if self.with_trigger.get() {
            ctx.attach_repaint_trigger(&self.trigger);
        }
        self.live.build(ctx)
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.live.layout_response(proposal, ctx)
    }

    fn place_children(
        &self,
        bounds: Rect,
        proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        self.live.place_children(bounds, proposal, children, ctx);
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        self.live.paint(bounds, canvas, ctx);
    }
}

/// A rebuild that stops attaching the widget's last trigger releases the
/// trigger's wake state, and only that: the live image the same `build()`
/// attached keeps waking the window and updating its layout signals.
#[test]
fn a_rebuild_that_drops_its_last_trigger_keeps_its_live_image() {
    let source = LiveImageSource::new(LivePixelFormat::Rgba8);
    let writer = source.writer();
    writer.write_frame(4, 4, &frame_of(4, 4, 0), 16).unwrap();
    let mut tree = WidgetTree::new();
    let waker = Arc::new(CountingWaker::new());
    tree.set_redraw_waker(Some(waker.clone() as Arc<dyn RedrawWaker>));
    let with_trigger = Rc::new(Cell::new(true));
    let widget = LiveWithTrigger {
        live: Live::new(&source),
        trigger: crate::RepaintTrigger::new(),
        with_trigger: with_trigger.clone(),
    };
    let (signals, attachment) = (widget.live.signals.clone(), widget.live.attachment.clone());
    let id = tree.add(widget);
    let mut mirror = LiveImageMirror::new();
    let mut frame = |tree: &mut WidgetTree| {
        tree.layout(SizeProposal::exact(400.0, 300.0));
        mirror.consume(&tree.render())
    };
    frame(&mut tree);
    assert_eq!(tree.repaint_trigger_count(), 1);
    assert_eq!(tree.live_image_attachment_count(), 1);

    with_trigger.set(false);
    tree.arena.get_mut(id).unwrap().dirty.needs_rebuild = true;
    frame(&mut tree);
    assert_eq!(tree.repaint_trigger_count(), 0, "the trigger is released");
    assert_eq!(
        tree.live_image_attachment_count(),
        1,
        "the live image the new build attached is kept"
    );
    assert_eq!(source.stats().attachments, 1);
    let kept = attachment.borrow().clone().unwrap();
    assert!(!kept.is_detached());

    let before = waker.count();
    writer
        .write_rect(PixelRect::new(0, 0, 1, 1), &[9; 4], 4)
        .unwrap();
    assert_eq!(waker.count(), before + 1, "a commit still wakes the window");
    assert!(
        frame(&mut tree).partial_uploads == 1,
        "and the next frame uploads it"
    );
    writer.write_frame(6, 4, &frame_of(6, 4, 1), 24).unwrap();
    frame(&mut tree);
    assert_eq!(
        signals.frame_size.get(),
        Some((6, 4)),
        "and a resize still reaches its layout signals"
    );
}

/// Places a live image as a snapping widget does: records the transform it
/// snapped under (or none, when `snaps` is false) and counts its
/// placements.
#[derive(Debug)]
struct SnapProbe {
    source: LiveImageSource,
    signals: LiveImageSignals,
    attachment: Option<LiveImageAttachment>,
    snaps: bool,
    placed: Rc<Cell<u32>>,
}

impl Widget for SnapProbe {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        self.attachment = Some(ctx.attach_live_image(&self.source, &self.signals));
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(20.0, 20.0).into()
    }

    fn place_children(
        &self,
        _bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        self.placed.set(self.placed.get() + 1);
        let attachment = self.attachment.as_ref().unwrap();
        let under = self.snaps.then(|| {
            ctx.arena()
                .map(|arena| arena.effective_transform(attachment.widget_id()))
                .unwrap()
        });
        attachment.set_snapped_under(under);
    }
}

/// A transform scope lays nothing out, so a picture snapped under an
/// ancestor's transform is laid out again when that transform changes, and
/// only then: not on a frame where it did not change, not between two
/// rotations (neither snaps, so the placement is the same), and never for a
/// picture that does not snap.
#[test]
fn a_picture_is_laid_out_again_only_when_the_transform_it_snapped_under_moves_the_snap() {
    use teksilo_canvas::Transform2D;
    for snaps in [true, false] {
        let source = LiveImageSource::new(LivePixelFormat::Rgba8);
        source
            .writer()
            .write_frame(4, 4, &frame_of(4, 4, 0), 16)
            .unwrap();
        let placed = Rc::new(Cell::new(0));
        let mut tree = WidgetTree::new();
        let probe = tree.add(SnapProbe {
            signals: LiveImageSignals::new(&source),
            source,
            attachment: None,
            snaps,
            placed: placed.clone(),
        });
        let parent = tree.add(Row::new(vec![probe]));
        let transform = Signal::new(Transform2D::IDENTITY);
        tree.set_transform(parent, transform.clone());
        let placements = |tree: &mut WidgetTree, t: Option<Transform2D>| {
            if let Some(t) = t {
                transform.set(t);
            }
            let before = placed.get();
            tree.layout(SizeProposal::exact(200.0, 100.0));
            let _ = tree.render();
            placed.get() - before
        };
        placements(&mut tree, None);

        let moved = placements(&mut tree, Some(Transform2D::translate(0.5, 0.0)));
        assert_eq!(moved, u32::from(snaps), "snaps {snaps}: a translation");
        assert_eq!(placements(&mut tree, None), 0, "snaps {snaps}: no change");
        let turned = placements(&mut tree, Some(Transform2D::rotate(0.3)));
        assert_eq!(turned, u32::from(snaps), "snaps {snaps}: to a rotation");
        assert_eq!(
            placements(&mut tree, Some(Transform2D::rotate(0.6))),
            0,
            "snaps {snaps}: between two rotations neither snaps"
        );
        let back = placements(&mut tree, Some(Transform2D::IDENTITY));
        assert_eq!(
            back,
            u32::from(snaps),
            "snaps {snaps}: back from a rotation"
        );
    }
}

// ── AC1-AC4: a pixel commit, and idle ──

#[test]
fn a_pixel_commit_wakes_once_paints_nothing_and_uploads_its_rect() {
    let mut f = Fixture::new();
    let (paints, sibling) = (f.paints.get(), f.sibling_paints.get());
    for i in 0..100 {
        f.commit_pixel(i % 8, i as u8);
    }
    assert_eq!(
        f.waker.count_of(WakeKind::Draw),
        1,
        "AC4: one wake for a hundred"
    );
    assert!(
        !f.tree.needs_render(),
        "AC3: a pixel commit marks no widget"
    );
    let report = f.frame();
    assert_eq!(
        f.paints.get(),
        paints,
        "AC3: no paint, the live image's included"
    );
    assert_eq!(f.sibling_paints.get(), sibling);
    assert_eq!(
        (report.full_uploads, report.partial_uploads),
        (1, 0),
        "one upload, whole: a hundred commits are more than the log holds"
    );
    assert_eq!(f.live_stats_paints(), paints as u64);
    let stats = f.tree.live_image_stats(f.live).unwrap();
    assert_eq!(stats.attachment.window_generation, stats.source.generation);
    assert_eq!(
        *f.poster.0.lock().unwrap(),
        0,
        "AC2: nothing posted to the event loop"
    );
}

impl Fixture {
    fn live_stats_paints(&self) -> u64 {
        self.tree
            .live_image_stats(self.live)
            .unwrap()
            .attachment
            .paints
    }
}

#[test]
fn idle_frames_render_nothing_and_wake_nobody() {
    let mut f = Fixture::new();
    f.commit_pixel(0, 1);
    f.frame();
    let wakes = f.waker.count();
    let first = f.tree.render();
    let ptr = Rc::as_ptr(&first);
    drop(first);
    for _ in 0..5 {
        f.tree.layout(SizeProposal::exact(400.0, 300.0));
        assert!(!f.tree.needs_render(), "AC1");
        let again = f.tree.render();
        assert_eq!(Rc::as_ptr(&again), ptr, "the cached frame, replayed");
        let report = f.mirror.consume(&again);
        assert_eq!(report, MirrorReport::default());
    }
    assert_eq!(f.waker.count(), wakes);
}

// ── B.2: a size change ──

#[test]
fn b02_a_resize_relayouts_the_widget_and_its_ancestors_in_a_clean_tree() {
    let mut f = Fixture::new();
    let layouts = f.row_layouts.get();
    let wakes = f.waker.count_of(WakeKind::Layout);
    f.writer
        .write_frame(20, 6, &frame_of(20, 6, 1), 80)
        .unwrap();
    assert_eq!(
        f.waker.count_of(WakeKind::Layout),
        wakes + 1,
        "a size change is layout's"
    );
    assert!(
        f.tree.off_thread_needs_frame(),
        "and the widget showed: draw a frame"
    );
    let report = f.frame();
    assert!(f.row_layouts.get() > layouts, "the row relaid out");
    assert_eq!(f.tree.bounds(f.live).width, 20.0);
    assert_eq!(
        f.attachment
            .borrow()
            .as_ref()
            .unwrap()
            .signals()
            .frame_size
            .get(),
        Some((20, 6))
    );
    assert_eq!(
        report.full_uploads, 1,
        "a texture of the new size, filled whole"
    );
    assert_eq!(f.shown(), frame_of(20, 6, 1));
    assert!(!f.tree.off_thread_needs_frame());
}

#[test]
fn status_changes_reach_the_signals_and_a_cleared_source_frees_its_texture() {
    let mut f = Fixture::new();
    let status = f
        .attachment
        .borrow()
        .as_ref()
        .unwrap()
        .signals()
        .status
        .clone();
    assert_eq!(status.get(), LiveImageStatus::Live);
    f.writer.clear().unwrap();
    let report = f.frame();
    assert_eq!(status.get(), LiveImageStatus::Waiting);
    assert_eq!(report.pruned, 1);
    assert_eq!(f.mirror.texture_count(), 0);
    let s = f.mirror.stats();
    assert_eq!((s.textures, s.bytes), (0, 0));
}

// ── B.5: a dormant widget ──

#[test]
fn b05_a_dormant_widget_is_woken_once_and_still_learns_its_size() {
    let awake = Rc::new(Cell::new(true));
    let mut f = Fixture::with(
        |mut r| {
            r.awake = Some(awake.clone());
            r
        },
        LiveImageSource::new(LivePixelFormat::Rgba8),
    );
    f.writer.write_frame(8, 6, &frame_of(8, 6, 0), 32).unwrap();
    f.frame();
    // The row parks the widget, and repaints, as a culling widget does
    // when what it shows changes.
    awake.set(false);
    f.tree.arena.mark_needs_layout(f.row);
    f.tree.arena.mark_needs_paint(f.row);
    f.frame();
    let wakes = f.waker.count();
    for i in 0..10 {
        f.commit_pixel(i % 8, i as u8);
    }
    assert_eq!(f.waker.count(), wakes + 1, "one wake for the burst");
    f.frame();
    for i in 0..10 {
        f.commit_pixel(i % 8, 50 + i as u8);
    }
    assert_eq!(
        f.waker.count(),
        wakes + 1,
        "not drawn: the flag stays raised"
    );
    f.writer.write_frame(9, 6, &frame_of(9, 6, 2), 36).unwrap();
    assert!(
        !f.tree.off_thread_needs_frame(),
        "dormant: a frame that draws nothing will do"
    );
    f.tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(
        f.attachment
            .borrow()
            .as_ref()
            .unwrap()
            .signals()
            .frame_size
            .get(),
        Some((9, 6)),
        "the pre-pass reached the dormant widget"
    );
    awake.set(true);
    f.tree.arena.mark_needs_layout(f.row);
    f.tree.arena.mark_needs_paint(f.row);
    let report = f.frame();
    assert_eq!(report.full_uploads, 1);
    assert_eq!(f.shown(), frame_of(9, 6, 2));
}

#[test]
fn a_clipped_out_widget_absorbs_a_burst_in_one_wake() {
    let mut f = Fixture::with(
        |mut r| {
            r.visible_width = Some(50.0);
            r
        },
        LiveImageSource::new(LivePixelFormat::Rgba8),
    );
    // The first frame's commit wakes once; the widget is clipped out of
    // every frame, so nothing takes its pixel flag again.
    f.writer.write_frame(8, 6, &frame_of(8, 6, 0), 32).unwrap();
    let wakes = f.waker.count();
    f.frame();
    for i in 0..10 {
        f.commit_pixel(i % 8, i as u8);
    }
    f.frame();
    for i in 0..10 {
        f.commit_pixel(i % 8, i as u8);
    }
    assert_eq!(
        f.waker.count(),
        wakes,
        "culled: never drawn, its bursts wake nobody"
    );
    f.tree.layout(SizeProposal::exact(400.0, 300.0));
    assert!(!f.tree.needs_render());
    assert_eq!(f.mirror.texture_count(), 0, "not in the frame: no texture");
}

// ── B.6, B.7, B.8: the lost-wake and freeze regressions ──

#[test]
fn b06_a_commit_between_paint_and_the_live_pass_is_not_lost() {
    let mut f = Fixture::with(|r| r, LiveImageSource::new(LivePixelFormat::Rgba8));
    // Waiting, laid out and painted: the quad has nothing to show.
    f.writer.resize(8, 6).unwrap();
    f.tree.layout(SizeProposal::exact(400.0, 300.0));
    let frame = f.tree.render();
    assert_eq!(frame.live_images[0].painted, None);
    // The first pixels land after paint and before the live pass.
    let (layout, draw) = (
        f.waker.count_of(WakeKind::Layout),
        f.waker.count_of(WakeKind::Draw),
    );
    f.writer.write_frame(8, 6, &frame_of(8, 6, 7), 32).unwrap();
    f.mirror.consume(&frame);
    assert_eq!(
        (
            f.waker.count_of(WakeKind::Layout),
            f.waker.count_of(WakeKind::Draw)
        ),
        (layout + 1, draw),
        "exactly one further wake, for layout: Waiting became Live"
    );
    assert!(f.tree.off_thread_needs_frame());
    let report = f.frame();
    assert_eq!(report.full_uploads, 1);
    assert!(f.mirror.decisions()[0].draw);
    assert_eq!(f.shown(), frame_of(8, 6, 7), "Live, and drawn");
}

#[test]
fn b07_the_picture_keeps_updating_through_resizes_clears_and_new_writers() {
    let mut f = Fixture::new();
    // A to B to A between two frames.
    f.writer.write_frame(9, 6, &frame_of(9, 6, 1), 36).unwrap();
    f.writer.write_frame(8, 6, &frame_of(8, 6, 2), 32).unwrap();
    f.frame();
    assert_eq!(f.shown(), frame_of(8, 6, 2));
    // clear() then a same-size commit.
    f.writer.clear().unwrap();
    f.writer.write_frame(8, 6, &frame_of(8, 6, 3), 32).unwrap();
    f.frame();
    assert_eq!(f.shown(), frame_of(8, 6, 3));
    // The writer drops, a new one takes over and resizes.
    let writer = std::mem::replace(&mut f.writer, f.source.writer());
    drop(writer);
    f.writer.write_frame(8, 6, &frame_of(8, 6, 4), 32).unwrap();
    f.frame();
    assert_eq!(f.shown(), frame_of(8, 6, 4));
}

#[test]
fn b08_a_widget_that_starts_at_zero_size_gets_its_first_frame() {
    let mut f = Fixture::with(|r| r, LiveImageSource::new(LivePixelFormat::Rgba8));
    assert_eq!(
        f.tree.bounds(f.live).width,
        0.0,
        "no hint, no frame: zero size"
    );
    let wakes = f.waker.count();
    f.writer
        .write_frame(1920, 1080, &frame_of(1920, 1080, 0), 1920 * 4)
        .unwrap();
    assert_eq!(f.waker.count(), wakes + 1, "exactly one wake");
    f.frame();
    assert_eq!(f.tree.bounds(f.live).size(), Size::new(1920.0, 1080.0));
}

// ── B.10, B.11: tree lifetime and windows ──

#[test]
fn b10_dropping_the_tree_detaches_every_consumer() {
    let f = Fixture::new();
    let consumer = f.tree.live_image_consumer(f.live).unwrap();
    let (writer, waker) = (f.writer.clone(), f.waker.clone());
    let source = f.source.clone();
    drop(f);
    assert!(consumer.is_detached());
    assert_eq!(source.stats().attachments, 0);
    let wakes = waker.count();
    writer
        .write_rect(PixelRect::new(0, 0, 1, 1), &[1; 4], 4)
        .unwrap();
    assert_eq!(waker.count(), wakes, "a detached consumer wakes nobody");
    drop(consumer);
}

#[test]
fn b11_a_source_shown_in_one_window_never_wakes_another() {
    let mut a = Fixture::new();
    let mut b_tree = WidgetTree::new();
    let b_waker = Arc::new(CountingWaker::new());
    b_tree.set_redraw_waker(Some(b_waker.clone() as Arc<dyn RedrawWaker>));
    b_tree.add(Painter(Rc::new(Cell::new(0))));
    b_tree.layout(SizeProposal::exact(100.0, 100.0));
    for i in 0..20 {
        a.commit_pixel(i % 8, i as u8);
        a.frame();
    }
    assert_eq!(b_waker.count(), 0, "AC5");
    assert!(a.waker.count() > 0);
}

// ── B.12, lookups ──

#[test]
fn b12_lookups_see_through_a_builder_wrapper() {
    let source = LiveImageSource::new(LivePixelFormat::Rgba8);
    let writer = source.writer();
    writer.write_frame(4, 4, &frame_of(4, 4, 0), 16).unwrap();
    let mut tree = WidgetTree::new();
    let wrapped = tree.add(Live::new(&source).focusable(true));
    tree.layout(SizeProposal::exact(100.0, 100.0));
    let frame = tree.render();
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&frame);
    let stats = tree
        .live_image_stats(wrapped)
        .expect("found through the wrapper");
    assert_eq!(stats.attachment.uploads, 1);
    assert_eq!(stats.source.generation, 1);
    assert!(tree.live_image_consumer(wrapped).is_some());
    let geometry = tree
        .live_image_geometry(wrapped)
        .expect("recorded in place_children");
    assert_eq!(geometry.source, (4, 4));
    let all: Vec<_> = tree.live_image_attachments().collect();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].0, wrapped);
    assert!(all[0].1.ptr_eq(&tree.live_image_consumer(wrapped).unwrap()));
    assert!(tree.live_image_stats(WidgetId::default()).is_none());
}

// ── B.13: attached after the pre-pass ──

/// Builds a [`Live`] child while `show` holds, rebuilt when it flips.
#[derive(Debug)]
struct Shows {
    show: Signal<bool>,
    source: LiveImageSource,
    child: Rc<RefCell<Option<LiveImageSignals>>>,
}

impl Widget for Shows {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        self.show
            .bind_to(id, ctx.binding_registry(), BindingLevel::Rebuild);
        if !self.show.get() {
            *self.child.borrow_mut() = None;
            return vec![];
        }
        let live = Live::new(&self.source);
        *self.child.borrow_mut() = Some(live.signals.clone());
        vec![ctx.add(live)]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(200.0, 200.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = ctx
                .child_size(child.id, SizeProposal::unspecified())
                .unwrap_or_default();
        }
    }
}

#[test]
fn b13_a_widget_built_after_the_pre_pass_draws_a_live_source_in_its_first_frame() {
    let source = LiveImageSource::new(LivePixelFormat::Rgba8);
    let writer = source.writer();
    writer.write_frame(6, 5, &frame_of(6, 5, 3), 24).unwrap();
    let show = Signal::new(false);
    let child = Rc::new(RefCell::new(None));
    let mut tree = WidgetTree::new();
    tree.add(Shows {
        show: show.clone(),
        source: source.clone(),
        child: child.clone(),
    });
    tree.layout(SizeProposal::exact(300.0, 300.0));
    let _ = tree.render();
    // The Rebuild binding builds the child during this layout, after the
    // pre-pass has run: nothing will commit again to wake it.
    show.set(true);
    tree.layout(SizeProposal::exact(300.0, 300.0));
    let signals = child.borrow().clone().expect("built");
    assert_eq!(signals.frame_size.get(), Some((6, 5)), "written at attach");
    let frame = tree.render();
    assert_eq!(frame.live_images.len(), 1);
    assert_eq!(
        frame.live_images[0].painted,
        Some((6, 5)),
        "stamped from the attach's record"
    );
    let mut mirror = LiveImageMirror::new();
    let report = mirror.consume(&frame);
    assert_eq!(report.full_uploads, 1, "uploaded in that render");
    assert_eq!(
        mirror.pixels(source.id()).unwrap().2,
        frame_of(6, 5, 3).as_slice()
    );
}

// ── the waker, orphans ──

#[test]
fn a_waker_installed_after_attaching_reaches_the_consumer() {
    let source = LiveImageSource::new(LivePixelFormat::Rgba8);
    let writer = source.writer();
    writer.write_frame(4, 4, &frame_of(4, 4, 0), 16).unwrap();
    let mut tree = WidgetTree::new();
    tree.add(Live::new(&source));
    tree.layout(SizeProposal::exact(100.0, 100.0));
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&tree.render());
    let waker = Arc::new(CountingWaker::new());
    tree.set_redraw_waker(Some(waker.clone() as Arc<dyn RedrawWaker>));
    writer
        .write_rect(PixelRect::new(0, 0, 1, 1), &[1; 4], 4)
        .unwrap();
    assert_eq!(waker.count(), 1);
}

#[test]
fn an_attachment_whose_widget_vanished_is_detached_by_the_pre_pass() {
    let mut f = Fixture::new();
    let consumer = f.tree.live_image_consumer(f.live).unwrap();
    // Remove the node without the destroy path that detaches.
    f.tree.arena.remove_node(f.live);
    f.tree.layout(SizeProposal::exact(400.0, 300.0));
    assert!(consumer.is_detached());
    assert_eq!(f.tree.live_image_attachment_count(), 0);
}

/// A widget constructed before its source had a frame, then mounted after:
/// the attach writes the current size and status, before any layout.
#[test]
fn attaching_writes_the_sources_current_size_and_status_into_the_signals() {
    let source = LiveImageSource::new(LivePixelFormat::Rgba8);
    let widget = Live::new(&source);
    let signals = widget.signals.clone();
    assert_eq!(
        (signals.frame_size.get(), signals.status.get()),
        (None, LiveImageStatus::Disconnected)
    );
    let writer = source.writer();
    writer.write_frame(3, 2, &frame_of(3, 2, 0), 12).unwrap();
    let mut tree = WidgetTree::new();
    tree.add(widget);
    assert_eq!(
        (signals.frame_size.get(), signals.status.get()),
        (Some((3, 2)), LiveImageStatus::Live),
        "written at attach, not left for a pre-pass"
    );
}
