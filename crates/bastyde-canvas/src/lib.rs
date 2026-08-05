// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

pub mod animated;
pub mod canvas;
pub mod ellipsis;
pub mod exif;
pub mod geometry;
pub mod paint;
pub mod path;
pub mod raster;
pub mod render_frame;
pub mod resample;
pub mod svg;
pub mod text_backend;
mod xml;

pub use animated::AnimatedIcon;
pub use canvas::Canvas;
pub use exif::Orientation;
pub use geometry::{EdgeInsets, Point, Rect, Size, SizeProposal, Transform2D, Vec2};
pub use paint::{
    FillRule, GradientStop, ImageHandle, LineCap, LineJoin, Paint, StrokeSpace, StrokeStyle,
};
pub use path::{Path, PathCommand};
pub use raster::{ImageDecodeError, ImageFormat, RasterIcon};
pub use render_frame::{
    AnimParams, AnimatedQuadClass, AnimatedQuadDraw, BlendMode, CosmeticLine, DecorationKind,
    DecorationRect, DrawCommand, GlyphQuad, ImageQuad, PaintData, PathEntry, PendingImage,
    RasterizedQuad, RenderFrame, ShadowQuad, ShapeKind, ShapeQuad,
};
pub use svg::{
    ResolvedGradient, SvgDrawOp, SvgFill, SvgIcon, SvgOp, SvgPaint, SvgParseError, SvgStop,
    SvgStroke,
};
pub use text_backend::{
    AtlasInfo, EllipsisMode, GlyphValidation, HitTarget, MockTextBackend, TextBackend, TextLayout,
    TextLayoutSpan, TextOverflow, TextSpanKind, quantize_raster_scale,
};
