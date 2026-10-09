// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `dim_when_disabled` (spec C.23): in a disabled subtree the picture is
//! drawn inside an opacity scope of the theme's disabled-content opacity,
//! over a background that then fills the whole box; commits still repaint
//! nothing; enabling again repaints once and drops the scope; and without
//! the switch nothing dims.

use teksilo_canvas::live_image::testing::LiveImageMirror;
use teksilo_canvas::{DrawCommand, RenderFrame};
use teksilo_core::signal::Signal;
use teksilo_core::widget_tree::WidgetTree;

use super::*;
use crate::primitives::HStack;

const W: u32 = 20;
const H: u32 = 10;

fn commit(writer: &LiveImageWriter, seed: u8) {
    writer
        .write_frame(
            W,
            H,
            &[seed, 0, 0, 255].repeat((W * H) as usize),
            (W * 4) as usize,
        )
        .unwrap();
}

/// A 20 x 10 picture in a 20 x 20 box, letterboxed top and bottom, in a row
/// whose enabled state is `enabled`.
fn tree_with(dim: bool, enabled: &Signal<bool>) -> (WidgetTree, LiveImageHandle, LiveImageWriter) {
    let source = LiveImageSource::new(LivePixelFormat::Rgba8);
    let writer = source.writer();
    commit(&writer, 1);
    let image = LiveImage::new(source)
        .size(W as f32, 20.0)
        .background(Color::from_rgb(0.0, 0.0, 1.0))
        .dim_when_disabled(dim)
        .alt("x");
    let handle = image.handle();
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let row = tree.add(HStack::new().child(image));
    tree.enabled_when(row, enabled.clone());
    tree.layout(SizeProposal::exact(100.0, 40.0));
    (tree, handle, writer)
}

/// The opacity scopes around the frame's live draw: `Some(f)` when it sits
/// inside `SetOpacity(f)`, `None` when it sits in none.
fn scope_of_live_draw(frame: &RenderFrame) -> Option<f32> {
    let mut stack: Vec<f32> = Vec::new();
    for command in &frame.draw_order {
        match command {
            DrawCommand::SetOpacity(f) => stack.push(*f),
            DrawCommand::RestoreOpacity => {
                stack.pop();
            }
            DrawCommand::LiveImage(_) => return stack.last().copied(),
            _ => {}
        }
    }
    panic!("the frame draws no live picture");
}

fn background_fills(frame: &RenderFrame) -> Vec<[f32; 4]> {
    let blue = Color::from_rgb(0.0, 0.0, 1.0).to_array();
    frame
        .decorations
        .iter()
        .filter(|d| d.color == blue)
        .map(|d| d.rect)
        .collect()
}

#[test]
fn c23_a_disabled_picture_draws_inside_the_themes_opacity() {
    let enabled = Signal::new(false);
    let (mut tree, handle, writer) = tree_with(true, &enabled);
    let frame = tree.render();
    let f = tree.theme().colors.disabled_content_opacity();
    assert_eq!(scope_of_live_draw(&frame), Some(f));
    let bounds = tree.bounds(handle.widget_id().unwrap());
    assert_eq!(
        background_fills(&frame),
        vec![[bounds.x, bounds.y, bounds.width, bounds.height]],
        "the background fills the whole box under the dimmed picture"
    );
    drop(frame);

    // Commits still repaint nothing, and the replayed frame keeps the scope.
    let mut mirror = LiveImageMirror::new();
    mirror.consume(&tree.render());
    let paints = handle.stats().attachment.paints;
    for seed in 2..=101 {
        commit(&writer, seed);
    }
    tree.layout(SizeProposal::exact(100.0, 40.0));
    let frame = tree.render();
    assert_eq!(handle.stats().attachment.paints, paints);
    assert_eq!(scope_of_live_draw(&frame), Some(f));
    assert_eq!(mirror.consume(&frame).full_uploads, 1);
    drop(frame);

    // Enabled again: one repaint, and the scope is gone.
    enabled.set(true);
    tree.layout(SizeProposal::exact(100.0, 40.0));
    let frame = tree.render();
    assert_eq!(handle.stats().attachment.paints, paints + 1);
    assert_eq!(scope_of_live_draw(&frame), None);
    assert_eq!(background_fills(&frame).len(), 2, "the letterbox only");
}

#[test]
fn c23_without_the_switch_a_disabled_picture_keeps_its_strength() {
    let enabled = Signal::new(false);
    let (mut tree, _handle, _writer) = tree_with(false, &enabled);
    let frame = tree.render();
    assert_eq!(scope_of_live_draw(&frame), None);
    assert!(
        !frame
            .draw_order
            .iter()
            .any(|c| matches!(c, DrawCommand::SetOpacity(_))),
        "no opacity scope at all"
    );
}

#[test]
fn c23_an_enabled_picture_never_dims() {
    let enabled = Signal::new(true);
    let (mut tree, _handle, _writer) = tree_with(true, &enabled);
    assert_eq!(scope_of_live_draw(&tree.render()), None);
}
