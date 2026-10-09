// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The bottom of Teksilo's stack, below teksilo-core and teksilo-render,
//! with no GPU, window or widget in it. It holds three things.
//!
//! - **The paint model.** A widget's `paint()` draws into a [`Canvas`],
//!   which records a [`RenderFrame`] of [`DrawCommand`]s for a renderer to
//!   replay. Around it: [`geometry`], [`Path`], [`Paint`], raster and SVG
//!   icons ([`RasterIcon`], [`SvgIcon`]), and the [`TextBackend`] trait a
//!   text engine implements.
//! - **Live pictures.** [`live_image`]: a raster another thread rewrites
//!   (a VM screen, a video, a camera), the writers a producer commits
//!   through, and what a frame carries of it. [`image_geometry`] places a
//!   picture in a widget and maps points to its pixels.
//! - **The wake layer.** [`wake`]: how any thread wakes one window, shared
//!   by live pictures and teksilo-core's `RepaintTrigger`.
//!
//! The last two are here, not in a crate of their own, because of who needs
//! them. teksilo-render consumes live pictures and depends on this crate, not
//! on teksilo-core; a [`RenderFrame`] carries them; and a producer uses them
//! without the GUI. A crate of their own would sit below this one, and this
//! one would depend on it.

pub mod animated;
pub mod canvas;
pub mod ellipsis;
pub mod exif;
pub mod geometry;
pub mod image_geometry;
pub mod live_image;
pub mod paint;
pub mod path;
pub mod raster;
pub mod render_frame;
pub mod resample;
pub mod svg;
#[doc(hidden)]
pub mod sync;
pub mod text_backend;
pub mod wake;
mod xml;

pub use animated::AnimatedIcon;
pub use canvas::Canvas;
#[allow(deprecated)]
pub use exif::Orientation;
pub use geometry::{EdgeInsets, Point, Rect, Size, SizeProposal, Transform2D, Vec2};
pub use image_geometry::{ImageFit, ImageGeometry, ImageOrientation, PixelRect};
pub use paint::{
    FillRule, GradientStop, ImageHandle, LineCap, LineJoin, Paint, StrokeSpace, StrokeStyle,
};
pub use path::{
    ArcCubic, Path, PathCommand, Subpath, arc_to_cubics, arc_transform_is_exact,
    subpaths_contain_point,
};
pub use raster::{ImageDecodeError, ImageFormat, RasterIcon};
pub use render_frame::{
    AnimParams, AnimatedQuadClass, AnimatedQuadDraw, BlendMode, CosmeticLine, DecorationKind,
    DecorationRect, DrawCommand, GlyphQuad, ImagePixels, ImageQuad, PaintData, PathEntry,
    PendingImage, RasterizedQuad, RenderFrame, ShadowQuad, ShapeKind, ShapeQuad,
};
pub use svg::{
    ResolvedGradient, SvgDrawOp, SvgFill, SvgIcon, SvgOp, SvgPaint, SvgParseError, SvgStop,
    SvgStroke,
};
pub use text_backend::{
    AtlasInfo, CharGeom, EllipsisMode, GlyphValidation, HitTarget, LineEnd, LineTruncation,
    MockTextBackend, TextBackend, TextDirection, TextGeometry, TextLayout, TextLayoutSpan,
    TextLine, TextLineSegment, TextLink, TextOverflow, TextSpanKind, quantize_raster_scale,
};
