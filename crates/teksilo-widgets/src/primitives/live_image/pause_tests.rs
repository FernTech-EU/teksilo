// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `pause_when_inactive`: a paused picture parks its flag and keeps its
//! texture (spec B.14), a capture shows through the pause (B.15), the
//! default does not pause (C.18), a pause belongs to the window's texture
//! (C.19), a paused picture is never blank (C.20), the switch is reactive
//! (C.21), and a picture mounted into an inactive window still gets its
//! first frame (C.22). On the renderer's live pass with textures in memory.

use std::sync::Arc;

use teksilo_canvas::live_image::testing::{CountingWaker, LiveImageMirror, MirrorReport};
use teksilo_canvas::wake::RedrawWaker;
use teksilo_canvas::{MockTextBackend, TextBackend};
use teksilo_core::signal::Signal;
use teksilo_core::widget_tree::WidgetTree;

use super::*;
use crate::primitives::HStack;

const W: u32 = 8;
const H: u32 = 6;

fn frame(seed: u8, w: u32, h: u32) -> Vec<u8> {
    [seed, 0, 0, 255].repeat((w * h) as usize)
}

fn commit(writer: &LiveImageWriter, seed: u8) {
    writer
        .write_frame(W, H, &frame(seed, W, H), (W * 4) as usize)
        .unwrap();
}

/// A tree with a counting waker, showing `images` in a row, laid out.
struct Window {
    tree: WidgetTree,
    waker: Arc<CountingWaker>,
    mirror: LiveImageMirror,
}

impl Window {
    fn new(images: Vec<LiveImage>, active: bool) -> Self {
        let waker = Arc::new(CountingWaker::new());
        let backend: Rc<RefCell<dyn TextBackend>> = Rc::new(RefCell::new(MockTextBackend::new()));
        let mut tree = WidgetTree::new()
            .with_theme(teksilo_core::presets::intui::light())
            .with_text_backend(backend);
        tree.set_redraw_waker(Some(waker.clone() as Arc<dyn RedrawWaker>));
        tree.set_window_active(active);
        let mut row = HStack::new();
        for image in images {
            row = row.child(image);
        }
        tree.add(row);
        Self {
            tree,
            waker,
            mirror: LiveImageMirror::new(),
        }
    }

    fn render(&mut self) -> MirrorReport {
        self.tree.layout(SizeProposal::exact(200.0, 100.0));
        let frame = self.tree.render();
        self.mirror.consume(&frame)
    }

    fn capture(&mut self) -> MirrorReport {
        self.tree.layout(SizeProposal::exact(200.0, 100.0));
        let frame = self.tree.render();
        self.mirror.capture(&frame)
    }

    fn shown(&self, source: &LiveImageSource) -> u8 {
        let (_, _, pixels) = self.mirror.pixels(source.id()).expect("a texture");
        pixels[0]
    }
}

fn uploads(report: &MirrorReport) -> u32 {
    report.full_uploads + report.partial_uploads
}

fn paused(source: &LiveImageSource) -> LiveImage {
    LiveImage::new(source.clone())
        .size(W as f32, H as f32)
        .pause_when_inactive(true)
        .alt("x")
}

fn live() -> (LiveImageSource, LiveImageWriter) {
    let source = LiveImageSource::new(LivePixelFormat::Rgba8);
    let writer = source.writer();
    commit(&writer, 1);
    (source, writer)
}

#[test]
fn b14_a_paused_picture_parks_its_flag_and_catches_up_once() {
    let (source, writer) = live();
    let image = paused(&source);
    let handle = image.handle();
    let mut window = Window::new(vec![image], true);
    assert_eq!(uploads(&window.render()), 1);

    window.tree.set_window_active(false);
    let report = window.render();
    assert_eq!(uploads(&report), 0);
    assert!(
        handle.stats().attachment.paused,
        "the render drew it paused"
    );
    let wakes = window.waker.count();
    for seed in 2..=101 {
        commit(&writer, seed);
    }
    assert_eq!(window.waker.count(), wakes, "no commit woke the window");
    assert!(!window.tree.needs_render());

    window.tree.set_window_active(true);
    let report = window.render();
    assert_eq!(uploads(&report), 1, "one catch-up upload");
    assert_eq!(window.shown(&source), 101, "of the latest commit");
    assert!(!handle.stats().attachment.paused);
}

#[test]
fn b15_a_capture_shows_through_the_pause_and_leaves_it_paused() {
    let (source, writer) = live();
    let image = paused(&source);
    let handle = image.handle();
    let mut window = Window::new(vec![image], false);
    window.render();
    commit(&writer, 2);
    let report = window.render();
    assert_eq!(uploads(&report), 0, "a presented frame keeps the old one");
    assert_eq!(window.shown(&source), 1);
    let before = handle.stats().attachment;

    let report = window.capture();
    assert_eq!(uploads(&report), 1);
    assert_eq!(
        window.shown(&source),
        2,
        "the capture mirrors the latest commit"
    );
    let after = handle.stats().attachment;
    assert_eq!(after.frames_drawn, before.frames_drawn);
    assert_eq!(after.captures, before.captures + 1);
    assert!(after.paused, "and the attachment is still paused");
}

#[test]
fn c18_without_the_pause_an_inactive_window_keeps_uploading() {
    let (source, writer) = live();
    let image = LiveImage::new(source.clone())
        .size(W as f32, H as f32)
        .alt("x");
    let mut window = Window::new(vec![image], false);
    window.render();
    let wakes = window.waker.count();
    for seed in 2..=101 {
        commit(&writer, seed);
    }
    assert_eq!(window.waker.count(), wakes + 1);
    assert_eq!(uploads(&window.render()), 1);
    assert_eq!(window.shown(&source), 101);
}

#[test]
fn c19_one_unpaused_picture_keeps_the_source_live_in_its_window() {
    let (source, writer) = live();
    let pausing = paused(&source);
    let staying = LiveImage::new(source.clone())
        .size(W as f32, H as f32)
        .alt("x");
    let (a, b) = (pausing.handle(), staying.handle());
    let mut window = Window::new(vec![pausing, staying], false);
    window.render();
    for seed in 2..=4 {
        commit(&writer, seed);
        assert_eq!(uploads(&window.render()), 1, "every render uploads");
        assert_eq!(window.shown(&source), seed, "both quads draw the latest");
    }
    for handle in [&a, &b] {
        let stats = handle.stats().attachment;
        assert_eq!(stats.paused_frames, 0);
        assert!(!stats.paused);
    }
    assert_eq!(source.stats().paused_attachments, 0);
}

#[test]
fn c20_a_paused_picture_is_never_blank() {
    let (source, writer) = live();
    let mut window = Window::new(
        vec![
            LiveImage::new(source.clone())
                .pause_when_inactive(true)
                .placeholder("Stopped")
                .alt("x"),
        ],
        false,
    );
    window.render();
    // A new size has nothing to keep: one whole-frame upload at that size.
    writer
        .write_frame(16, 12, &frame(7, 16, 12), 16 * 4)
        .unwrap();
    let report = window.render();
    assert_eq!((report.full_uploads, report.partial_uploads), (1, 0));
    assert_eq!(window.mirror.pixels(source.id()).unwrap().0, 16);
    writer
        .write_frame(16, 12, &frame(8, 16, 12), 16 * 4)
        .unwrap();
    assert_eq!(uploads(&window.render()), 0, "then it holds");

    // Cleared: nothing to show, the texture goes and the placeholder shows.
    writer.clear().unwrap();
    window.tree.layout(SizeProposal::exact(200.0, 100.0));
    let frame = window.tree.render();
    assert!(!frame.glyphs.is_empty(), "the placeholder");
    window.mirror.consume(&frame);
    assert_eq!(window.mirror.texture_count(), 0);
}

#[test]
fn c21_the_switch_is_reactive() {
    let (source, writer) = live();
    let pause = Signal::new(true);
    let image = LiveImage::new(source.clone())
        .size(W as f32, H as f32)
        .pause_when_inactive(pause.clone())
        .alt("x");
    let handle = image.handle();
    let mut window = Window::new(vec![image], false);
    window.render();
    commit(&writer, 2);
    assert_eq!(uploads(&window.render()), 0);
    let paints = handle.stats().attachment.paints;

    pause.set(false);
    let report = window.render();
    assert_eq!(handle.stats().attachment.paints, paints + 1, "it repainted");
    assert_eq!(uploads(&report), 1, "and uploaded");
    assert_eq!(window.shown(&source), 2);
}

#[test]
fn c22_a_picture_mounted_into_an_inactive_window_shows_its_first_frame() {
    let (source, writer) = live();
    let image = paused(&source);
    let handle = image.handle();
    let mut window = Window::new(vec![image], false);
    let report = window.render();
    assert_eq!((report.full_uploads, report.partial_uploads), (1, 0));
    assert_eq!(window.shown(&source), 1);
    commit(&writer, 2);
    assert_eq!(uploads(&window.render()), 0, "then it freezes");
    assert_eq!(source.stats().paused_attachments, 1);
    assert!(!source.is_displayed());
    assert!(handle.stats().attachment.paused);
}

#[test]
fn the_debug_repr_says_when_the_window_kept_the_picture_paused() {
    let (source, _writer) = live();
    let image = paused(&source);
    let mut window = Window::new(vec![image], false);
    window.render();
    let id = window.tree.roots()[0];
    let row = window.tree.children(id);
    let repr = window.tree.widget_debug_string(row[0]).expect("a repr");
    assert!(repr.contains("paused: true"), "{repr}");
}
