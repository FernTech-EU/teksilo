// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

pub mod animated;
pub mod canvas;
pub mod ellipsis;
pub mod geometry;
pub mod paint;
pub mod path;
pub mod raster;
pub mod render_frame;
pub mod svg;
pub mod text_backend;
mod xml;

pub use animated::AnimatedIcon;
pub use canvas::Canvas;
pub use geometry::{EdgeInsets, Point, Rect, Size, SizeProposal, Transform2D, Vec2};
pub use paint::{GradientStop, ImageHandle, LineCap, LineJoin, Paint, StrokeSpace, StrokeStyle};
pub use path::{Path, PathCommand};
pub use raster::{ImageDecodeError, RasterIcon};
pub use render_frame::{
    AnimParams, AnimatedQuadClass, AnimatedQuadDraw, BlendMode, CosmeticLine, DecorationKind,
    DecorationRect, DrawCommand, GlyphQuad, ImageQuad, PaintData, PathEntry, PendingImage,
    RasterizedQuad, RenderFrame, ShadowQuad, ShapeKind, ShapeQuad,
};
pub use svg::{SvgIcon, SvgParseError, SvgStroke};
pub use text_backend::{
    AtlasInfo, EllipsisMode, GlyphValidation, HitTarget, MockTextBackend, TextBackend, TextLayout,
    TextLayoutSpan, TextOverflow, TextSpanKind, quantize_raster_scale,
};
