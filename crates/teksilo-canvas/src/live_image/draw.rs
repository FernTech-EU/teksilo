// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Drawing a live picture: what a widget asks for, and what a frame carries.

use super::{ImageOrientation, LiveImageConsumer, ScalingFilter};
use crate::geometry::Rect;
use crate::image_geometry::oriented_crop;

/// Where and how to draw a live picture, in the window-space logical pixels
/// `paint()` receives. Public so a widget of one's own can draw a source
/// without `LiveImage`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct LiveImageDraw {
    /// Where the whole displayed picture lies; it may exceed `clip` where
    /// the fit crops.
    pub content: Rect,
    /// The part that shows, usually the widget's bounds. The picture is
    /// cropped through its texture coordinates, not a scissor.
    pub clip: Rect,
    pub filter: ScalingFilter,
    pub orientation: ImageOrientation,
    /// Keep the texture the window holds and upload nothing, unless the
    /// window holds none of the painted size. `LiveImage` sets it from
    /// `pause_when_inactive`; a custom widget may set it from any policy.
    pub paused: bool,
}

impl LiveImageDraw {
    /// `content`, shown where it meets `clip`: bilinear, upright, live.
    pub fn new(content: Rect, clip: Rect) -> Self {
        Self {
            content,
            clip,
            filter: ScalingFilter::default(),
            orientation: ImageOrientation::default(),
            paused: false,
        }
    }

    pub fn filter(mut self, filter: ScalingFilter) -> Self {
        self.filter = filter;
        self
    }

    pub fn orientation(mut self, orientation: ImageOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    pub fn paused(mut self, paused: bool) -> Self {
        self.paused = paused;
        self
    }
}

/// One live draw inside a frame. It carries no pixels: `Clone` is an `Arc`
/// clone, so a cached frame that replays it copies nothing.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct LiveImageQuad {
    pub consumer: LiveImageConsumer,
    /// The visible destination, `[x, y, width, height]` in window-space
    /// logical pixels. Empty when nothing of the picture shows.
    pub screen: [f32; 4],
    /// The texture coordinates of `screen`'s corners: top-leading,
    /// top-trailing, bottom-trailing, bottom-leading. The orientation and
    /// the crop live here.
    pub uv: [[f32; 2]; 4],
    pub filter: ScalingFilter,
    /// The source size the window's last layout pre-pass recorded while the
    /// source was `Live`; `None` when there is nothing to show. The renderer
    /// draws only a texture of exactly this size.
    pub painted: Option<(u32, u32)>,
    /// Copied from [`LiveImageDraw::paused`]. A cached frame replays it, and
    /// a window becoming active or inactive repaints every widget, so a
    /// replayed value is never stale.
    pub paused: bool,
}

impl LiveImageQuad {
    /// The quad `consumer`'s widget draws for `draw`: cropped to the clip,
    /// stamped with the size the consumer laid out with.
    pub(crate) fn new(consumer: &LiveImageConsumer, draw: &LiveImageDraw) -> Self {
        let meta = consumer.layout_meta();
        let live = meta.status == super::LiveImageStatus::Live;
        let (screen, uv, painted) = match oriented_crop(draw.content, draw.clip, draw.orientation) {
            Some((rect, uv)) => (rect.to_array(), uv, if live { meta.size } else { None }),
            // Nothing shows: the quad still reaches the renderer, which takes
            // the widget's pixel flag only for quads in the frame, and draws
            // nothing for it.
            None => ([draw.clip.x, draw.clip.y, 0.0, 0.0], [[0.0; 2]; 4], None),
        };
        Self {
            consumer: consumer.clone(),
            screen,
            uv,
            filter: draw.filter,
            painted,
            paused: draw.paused,
        }
    }

    /// Shift the destination by `(dx, dy)`, as a frame embedded at an offset
    /// is.
    pub(crate) fn offset(&mut self, dx: f32, dy: f32) {
        self.screen[0] += dx;
        self.screen[1] += dy;
    }
}
