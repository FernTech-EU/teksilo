// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! LiveImage — shows a picture another thread rewrites many times a second.
//!
//! A virtual machine's screen, a video frame, a camera preview: the pixels
//! never pass through the widget. A producer writes a
//! [`LiveImageSource`] from any thread through a [`LiveImageWriter`]; the
//! window's renderer uploads what changed since its last frame and draws it
//! where this widget's last paint put it. A commit that changes only pixels
//! runs no `paint()`, marks no widget and repaints nothing else: the window
//! replays its cached frame. Only a change of the source's size or status
//! (resized, `Waiting`, `Live`, `Disconnected`) relayouts and repaints this
//! widget. A producer that hands over whole frames, changed or not, writes
//! through a [`LiveImageDiffWriter`] instead, which commits only what changed
//! and nothing for an identical frame.
//!
//! # Sizing
//!
//! [`LiveImageSizing`] picks the box: `Aspect` (the default) is the largest
//! box with the picture's aspect ratio inside the proposal, rounded to whole
//! device pixels; `Fill` takes the whole proposal and letterboxes; `Natural`
//! is one source pixel per logical pixel ([`device_pixels`](LiveImage::device_pixels)
//! makes it one per device pixel). [`width`](LiveImage::width),
//! [`height`](LiveImage::height) and [`size`](LiveImage::size) pin the box
//! and win over the mode. Inside the box the picture is placed by an
//! [`ImageFit`] and an [`Alignment`], turned by an [`ImageOrientation`], and
//! its edges snap to the device-pixel grid.
//!
//! ```rust
//! use teksilo_widgets::primitives::live_image::{LiveImage, LiveImageSizing};
//! use teksilo_widgets::primitives::live_image::{LiveImageSource, LivePixelFormat};
//! use teksilo_widgets::primitives::ImageFit;
//!
//! let screen = LiveImageSource::new(LivePixelFormat::Bgrx8);
//! let writer = screen.writer();
//! std::thread::spawn(move || {
//!     let frame = vec![0u8; 720 * 1280 * 4];
//!     writer.write_frame(720, 1280, &frame, 720 * 4).unwrap();
//! });
//! let _view = LiveImage::new(screen)
//!     .sizing(LiveImageSizing::Aspect)
//!     .fit(ImageFit::Contain)
//!     .alt("Virtual machine screen");
//! ```
//!
//! # Input
//!
//! `LiveImage` handles no input. An app that forwards pointer input to what
//! the picture shows attaches its handlers through `WidgetBuilder` and maps
//! the positions they receive with the widget's [`LiveImageHandle`], taken
//! before a `WidgetBuilder` method wraps the widget (or injected with
//! [`with_handle`](LiveImage::with_handle)):
//! [`map_to_source`](LiveImageHandle::map_to_source) gives the source pixel
//! a point shows, from the same placement paint drew. A position handed in
//! window space (`Scroll::window_position`) goes through
//! `EventContext::to_local` first.
//!
//! # Accessibility
//!
//! One `Role::Image` node named by [`alt`](LiveImage::alt); a decorative
//! picture calls [`a11y_hidden`](LiveImage::a11y_hidden) instead. Pixels are
//! not accessible content, and a commit never changes the node: only a
//! status change does, when the [`placeholder`](LiveImage::placeholder)
//! becomes or stops being its description.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use teksilo_canvas::live_image::LiveImageDraw;
use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal, Transform2D};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::color_prop::ColorProp;
use teksilo_core::environment::LayoutDirection;
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::{LiveImageAttachment, LiveImageSignals, WidgetId};
use teksilo_tokens::{Alignment, Color, TextRole, TextStyleRole};

pub use teksilo_canvas::image_geometry::{ImageFit, ImageGeometry, ImageOrientation, PixelRect};
pub use teksilo_canvas::live_image::{
    LiveImageDiffWriter, LiveImageSource, LiveImageStats, LiveImageStatus, LiveImageWriter,
    LivePixelFormat, ScalingFilter,
};

/// How a [`LiveImage`] sizes its box. [`LiveImage::width`],
/// [`LiveImage::height`] and [`LiveImage::size`] pin it and win over the
/// mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum LiveImageSizing {
    /// The largest box with the picture's aspect ratio inside the proposal,
    /// each side rounded to whole device pixels. It scales up as well as
    /// down; an axis the proposal leaves open follows the other one. With no
    /// frame size yet, the proposal on its bounded axes, else zero.
    #[default]
    Aspect,
    /// The whole proposal, growing into a stack's slack; the
    /// [`fit`](LiveImage::fit) places the picture and the rest is
    /// letterbox. An axis the proposal leaves open follows the picture's
    /// aspect ratio.
    Fill,
    /// The picture's own size, rigid: one source pixel per logical pixel,
    /// or per device pixel with [`device_pixels`](LiveImage::device_pixels).
    /// Zero with no frame size yet.
    Natural,
}

/// Shows a [`LiveImageSource`]. See the [module documentation](self).
pub struct LiveImage {
    source: Prop<LiveImageSource>,
    fit: ImageFit,
    alignment: Alignment,
    sizing: LiveImageSizing,
    width: Option<f32>,
    height: Option<f32>,
    device_pixels: bool,
    scaling: ScalingFilter,
    orientation: ImageOrientation,
    pixel_snap: bool,
    background: ColorProp,
    placeholder: Prop<String>,
    alt: Option<Prop<String>>,
    a11y_hidden: bool,
    pause_when_inactive: Prop<bool>,
    dim_when_disabled: bool,
    shared: Rc<Shared>,
    /// What the mounted widget shares with its handles. The widget owns it,
    /// so a handle that outlives the widget sees nothing.
    mounted: Option<Rc<Mounted>>,
}

/// What a `LiveImage` and its handles share from construction on.
struct Shared {
    /// The size and status the window's last layout saw. Kept across a
    /// switch to another source.
    signals: LiveImageSignals,
    /// The source the widget shows; `None` until a widget takes the handle.
    source: RefCell<Option<LiveImageSource>>,
    mounted: RefCell<Weak<Mounted>>,
}

/// One mount of the widget: its attachment, and the placement its last
/// layout computed.
struct Mounted {
    attachment: LiveImageAttachment,
    placement: Cell<Option<Placement>>,
}

/// Where the picture lies, as computed for one window-space box.
#[derive(Debug, Clone, Copy)]
struct Placement {
    /// The fit's own placement, widget-local, unsnapped.
    fitted: ImageGeometry,
    /// What paint draws and input maps through: `fitted`, its edges on the
    /// device grid when snapping applies.
    shown: ImageGeometry,
    /// The window-space box it was computed for.
    window: Rect,
    /// Window space to device pixels, the box's own origin aside: the
    /// ancestors' transforms, then the device scale. `None` when the
    /// picture is not snapped.
    to_device: Option<Transform2D>,
}

impl Placement {
    /// The placement of `fitted` in the window-space box `window`.
    fn new(fitted: ImageGeometry, window: Rect, to_device: Option<Transform2D>) -> Self {
        let shown = match to_device {
            Some(device) => {
                fitted.snapped(Transform2D::translate(window.x, window.y).then(&device))
            }
            None => fitted,
        };
        Self {
            fitted,
            shown,
            window,
            to_device,
        }
    }
}

impl LiveImage {
    /// Show `source`. A `Signal<LiveImageSource>` switches sources: the
    /// widget rebuilds and attaches the new one, keeping its handle's
    /// Signals, and the window drops the old one's texture at its next frame.
    pub fn new(source: impl Into<Prop<LiveImageSource>>) -> Self {
        let source = source.into();
        let current = source.get();
        let shared = Rc::new(Shared {
            signals: LiveImageSignals::new(&current),
            source: RefCell::new(Some(current)),
            mounted: RefCell::new(Weak::new()),
        });
        Self {
            source,
            fit: ImageFit::Contain,
            alignment: Alignment::CENTER,
            sizing: LiveImageSizing::Aspect,
            width: None,
            height: None,
            device_pixels: false,
            scaling: ScalingFilter::Linear,
            orientation: ImageOrientation::Normal,
            pixel_snap: true,
            background: ColorProp::Static(Color::TRANSPARENT),
            placeholder: Prop::Static(String::new()),
            alt: None,
            a11y_hidden: false,
            pause_when_inactive: Prop::Static(false),
            dim_when_disabled: false,
            shared,
            mounted: None,
        }
    }

    /// How the picture fills its box: the CSS `object-fit` set. Default
    /// [`ImageFit::Contain`]. `Cover`, and `None` on a picture larger than
    /// the box, crop it to the box.
    pub fn fit(mut self, fit: ImageFit) -> Self {
        self.fit = fit;
        self
    }

    /// Where the picture sits in its box when the fit leaves room or crops.
    /// Leading and Trailing follow the layout direction. Default
    /// [`Alignment::CENTER`].
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// How the box is sized. Default [`LiveImageSizing::Aspect`].
    pub fn sizing(mut self, sizing: LiveImageSizing) -> Self {
        self.sizing = sizing;
        self
    }

    /// Pin the box's width, in logical pixels; its height follows the
    /// picture's aspect ratio (zero before the first frame).
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Pin the box's height, in logical pixels; its width follows the
    /// picture's aspect ratio (zero before the first frame).
    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    /// Pin the box to `width` × `height` logical pixels.
    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.width = Some(width);
        self.height = Some(height);
        self
    }

    /// Measure the picture's own size in device pixels: one source pixel
    /// per device pixel instead of per logical pixel. The fit starts from
    /// that size too: `ImageFit::None` draws the whole picture at it, and
    /// `ImageFit::ScaleDown` never grows it past it. With
    /// `sizing(Natural)`, `fit(ImageFit::None)` and
    /// `scaling(ScalingFilter::Nearest)`, every texel lands on one device
    /// pixel, at any device scale. Default false.
    pub fn device_pixels(mut self, on: bool) -> Self {
        self.device_pixels = on;
        self
    }

    /// How the picture is sampled when drawn at another size. Default
    /// [`ScalingFilter::Linear`]; `Nearest` keeps an integer upscale crisp,
    /// and `Trilinear` keeps a thumbnail drawn below half size from
    /// aliasing.
    pub fn scaling(mut self, scaling: ScalingFilter) -> Self {
        self.scaling = scaling;
        self
    }

    /// How the picture is turned or mirrored for display. A quarter turn
    /// swaps the width and height the box is sized from. Default
    /// [`ImageOrientation::Normal`].
    pub fn orientation(mut self, orientation: ImageOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Snap the picture's edges to the device-pixel grid, which keeps a 1:1
    /// picture sharp. An edge moves by less than one device pixel. Turn it
    /// off only for a picture whose box is animated, where that step would
    /// show. Snapping applies under translations and scales; a rotated or
    /// skewed ancestor turns it off. Default true.
    pub fn pixel_snap(mut self, on: bool) -> Self {
        self.pixel_snap = on;
        self
    }

    /// The fill of the letterbox, and of the whole box while the source is
    /// not `Live`. A role or a `Signal` follows the theme and the window.
    /// Default transparent.
    pub fn background(mut self, color: impl Into<ColorProp>) -> Self {
        self.background = color.into();
        self
    }

    /// Text shown centred in the box while the source is not `Live`
    /// (secondary text, body style, truncated to the box), and the node's
    /// description then. Painted, so it takes no input. Default empty.
    pub fn placeholder(mut self, text: impl Into<Prop<String>>) -> Self {
        self.placeholder = text.into();
        self
    }

    /// The accessible name of the picture. A `tr!` string follows the
    /// locale. Give it, or call [`a11y_hidden`](Self::a11y_hidden): a debug
    /// build asserts one of them.
    pub fn alt(mut self, text: impl Into<Prop<String>>) -> Self {
        self.alt = Some(text.into());
        self
    }

    /// Hide a decorative picture from assistive technology: what it shows
    /// is said by text beside it.
    pub fn a11y_hidden(mut self) -> Self {
        self.a11y_hidden = true;
        self
    }

    /// Stop uploading while the window is inactive: not focused, or covered
    /// where the platform reports it. The picture keeps the last frame it
    /// uploaded and a commit no longer wakes the window; once the window is
    /// active again, the next frame shows the latest commit, in one upload.
    /// A change of size or status still applies, a picture with nothing of
    /// its size uploaded yet still gets its first frame, and a screenshot
    /// shows the latest commit all the same. The pause belongs to the
    /// window's texture: while another widget of the window shows the same
    /// source unpaused, both stay live. A `Signal<bool>` turns it on and off
    /// as the user decides. Default false.
    pub fn pause_when_inactive(mut self, pause: impl Into<Prop<bool>>) -> Self {
        self.pause_when_inactive = pause.into();
        self
    }

    /// Dim the picture in a disabled subtree, by the theme's
    /// [`disabled_content_opacity`](teksilo_tokens::ColorTokens::disabled_content_opacity)
    /// over the widget's background, which then fills the whole box. By
    /// default a live picture keeps its full strength when an ancestor is
    /// disabled: a VM's screen is content, not a control. A commit still
    /// repaints nothing while dimmed. Default false.
    pub fn dim_when_disabled(mut self, on: bool) -> Self {
        self.dim_when_disabled = on;
        self
    }

    /// Drive this widget through `handle`, made beforehand with
    /// [`LiveImageHandle::new`]: the form a `teksu!` tree can use, where
    /// [`handle`](Self::handle) cannot be called. A handle follows one
    /// widget; handing it to a second moves it there.
    pub fn with_handle(mut self, handle: &LiveImageHandle) -> Self {
        let current = self.source.get();
        handle.shared.signals.frame_size.set(current.size());
        handle.shared.signals.status.set(current.status());
        *handle.shared.source.borrow_mut() = Some(current);
        self.shared = handle.shared.clone();
        self
    }

    /// A handle to this widget, for the UI thread: its source's size and
    /// status, its placement, and the mapping from widget-local points to
    /// source pixels. Take it before a `WidgetBuilder` method wraps the
    /// widget.
    pub fn handle(&self) -> LiveImageHandle {
        LiveImageHandle {
            shared: self.shared.clone(),
        }
    }

    /// The picture's own size in logical pixels, turned for display.
    fn natural(&self, frame: (u32, u32), scale: f32) -> Size {
        let (w, h) = self.orientation.displayed_size(frame.0, frame.1);
        let per_pixel = if self.device_pixels { scale } else { 1.0 };
        Size::new(w as f32 / per_pixel, h as f32 / per_pixel)
    }

    /// The box this widget asks for under `proposal`.
    fn box_size(&self, proposal: SizeProposal, scale: f32) -> LayoutResponse {
        let bounded = |p: Option<f32>| p.filter(|v| v.is_finite());
        let (pw, ph) = (bounded(proposal.width), bounded(proposal.height));
        let natural = self
            .shared
            .signals
            .frame_size
            .get()
            .map(|frame| self.natural(frame, scale));
        match (self.width, self.height, natural) {
            (Some(w), Some(h), _) => Size::new(w, h).into(),
            (Some(w), None, Some(n)) => Size::new(w, w * n.height / n.width).into(),
            (None, Some(h), Some(n)) => Size::new(h * n.width / n.height, h).into(),
            (Some(w), None, None) => Size::new(w, 0.0).into(),
            (None, Some(h), None) => Size::new(0.0, h).into(),
            (None, None, natural) => match (self.sizing, natural) {
                (LiveImageSizing::Natural, Some(n)) => n.into(),
                (LiveImageSizing::Natural, None) => Size::new(0.0, 0.0).into(),
                (LiveImageSizing::Aspect, Some(n)) => aspect_box(n, pw, ph, scale).into(),
                (LiveImageSizing::Fill, Some(n)) => {
                    let fitted = aspect_box(n, pw, ph, scale);
                    LayoutResponse::flexible(
                        Size::new(pw.unwrap_or(fitted.width), ph.unwrap_or(fitted.height)),
                        1.0,
                    )
                }
                (LiveImageSizing::Fill, None) => {
                    LayoutResponse::flexible(Size::new(pw.unwrap_or(0.0), ph.unwrap_or(0.0)), 1.0)
                }
                (_, None) => Size::new(pw.unwrap_or(0.0), ph.unwrap_or(0.0)).into(),
            },
        }
    }

    /// The placement for the window-space box `window` at device scale
    /// `scale`, or `None` with no frame size.
    ///
    /// The picture is fitted from its natural size, the one its box is
    /// measured from: with `device_pixels`, one source pixel per device
    /// pixel. `Fill`, `Contain` and `Cover` do not depend on it; `None`
    /// draws at it and `ScaleDown` never grows past it, so a picture
    /// measured in device pixels is neither cropped nor drawn larger.
    fn place(
        &self,
        window: Rect,
        rtl: bool,
        scale: f32,
        to_device: Option<Transform2D>,
    ) -> Option<Placement> {
        let frame = self.shared.signals.frame_size.get()?;
        let bounds = Rect::new(0.0, 0.0, window.width, window.height);
        let content = self
            .fit
            .fitted_rect(self.natural(frame, scale), bounds, self.alignment, rtl);
        let fitted = ImageGeometry::new(frame, self.orientation, content, bounds);
        Some(Placement::new(fitted, window, to_device))
    }

    /// The placement paint draws in `bounds`: the one the last layout
    /// computed, or, when a parent moved the widget without laying it out
    /// again, the same one recomputed there and kept.
    fn placement_for(
        &self,
        mounted: &Mounted,
        bounds: Rect,
        rtl: bool,
        scale: f32,
    ) -> Option<Placement> {
        let stored = mounted.placement.get();
        match stored {
            Some(p) if p.window == bounds => Some(p),
            Some(p) if p.window.width == bounds.width && p.window.height == bounds.height => {
                let moved = Placement::new(p.fitted, bounds, p.to_device);
                mounted.placement.set(Some(moved));
                Some(moved)
            }
            _ => {
                let to_device = stored.and_then(|p| p.to_device);
                let fresh = self.place(bounds, rtl, scale, to_device.filter(|_| self.pixel_snap));
                mounted.placement.set(fresh);
                fresh
            }
        }
    }

    /// Paint the placeholder centred in `bounds`, truncated to its width.
    fn paint_placeholder(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let text = self.placeholder.get();
        if text.is_empty() {
            return;
        }
        let Some(backend) = canvas.text_backend().cloned() else {
            return;
        };
        let style = TextStyleRole::Body.resolve(&ctx.theme.typography);
        let color =
            ColorProp::TextRole(TextRole::Secondary).resolve(ctx.theme, ctx.effective_enabled);
        // The same small epsilon `Canvas::draw_text` adds, so a label that
        // fits exactly is not truncated.
        let layout =
            backend
                .borrow_mut()
                .layout_single_line(&text, &style, Some(bounds.width + 0.5));
        let origin = Point::new(
            bounds.x + ((bounds.width - layout.width) / 2.0).max(0.0),
            bounds.y + ((bounds.height - layout.height) / 2.0).max(0.0),
        );
        if !canvas.draw_text_layout(&layout, origin, color) {
            // The layout's glyphs were evicted: shape it again.
            canvas.draw_text(
                &text,
                Rect::new(origin.x, origin.y, bounds.width, layout.height),
                &style,
                color,
            );
        }
    }
}

/// `scale` when it is a device scale, else 1.
fn usable_scale(scale: f32) -> f32 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

/// The `Aspect` box: `natural` scaled to the largest size that fits the
/// bounded axes, each side rounded to whole device pixels and kept inside
/// the proposal's device grid.
fn aspect_box(natural: Size, pw: Option<f32>, ph: Option<f32>, scale: f32) -> Size {
    let factor = match (pw, ph) {
        (Some(pw), Some(ph)) => (pw / natural.width).min(ph / natural.height),
        (Some(pw), None) => pw / natural.width,
        (None, Some(ph)) => ph / natural.height,
        (None, None) => 1.0,
    };
    let round = |side: f32| (side * scale).round() / scale;
    let within = |side: f32, p: Option<f32>| match p {
        Some(p) => side.min((p * scale).floor() / scale),
        None => side,
    };
    Size::new(
        within(round(natural.width * factor), pw).max(0.0),
        within(round(natural.height * factor), ph).max(0.0),
    )
}

/// The parts of `bounds` outside `picture`: up to four rects, above, below,
/// leading and trailing.
fn letterbox(bounds: Rect, picture: Rect) -> impl Iterator<Item = Rect> {
    let top = picture.y - bounds.y;
    let bottom = bounds.y + bounds.height - (picture.y + picture.height);
    let left = picture.x - bounds.x;
    let right = bounds.x + bounds.width - (picture.x + picture.width);
    [
        Rect::new(bounds.x, bounds.y, bounds.width, top),
        Rect::new(bounds.x, picture.y + picture.height, bounds.width, bottom),
        Rect::new(bounds.x, picture.y, left, picture.height),
        Rect::new(picture.x + picture.width, picture.y, right, picture.height),
    ]
    .into_iter()
    .filter(|r| r.width > 0.0 && r.height > 0.0)
}

/// `rect`, widget-local, in the window space of a box at `origin`.
fn at(rect: Rect, origin: Rect) -> Rect {
    Rect::new(
        rect.x + origin.x,
        rect.y + origin.y,
        rect.width,
        rect.height,
    )
}

impl Widget for LiveImage {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let source = self.source.get();
        *self.shared.source.borrow_mut() = Some(source.clone());
        let attachment = ctx.attach_live_image(&source, &self.shared.signals);
        let registry = ctx.binding_registry();
        self.source
            .register_if_bound(id, registry, BindingLevel::Rebuild);
        let signals = &self.shared.signals;
        signals
            .frame_size
            .bind_to(id, registry, BindingLevel::Relayout);
        signals
            .status
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        signals
            .status
            .bind_to(id, registry, BindingLevel::AccessibilityOnly);
        self.placeholder
            .register_if_bound(id, registry, BindingLevel::RepaintOnly);
        self.placeholder
            .register_if_bound(id, registry, BindingLevel::AccessibilityOnly);
        if let Some(alt) = &self.alt {
            alt.register_if_bound(id, registry, BindingLevel::AccessibilityOnly);
        }
        self.background
            .register_if_bound(id, registry, BindingLevel::RepaintOnly);
        // A window turning active or inactive repaints every widget, so only
        // the user's own switch needs a binding.
        self.pause_when_inactive
            .register_if_bound(id, registry, BindingLevel::RepaintOnly);
        // The box rounds to device pixels and the picture snaps to them, so
        // a move to a display with another scale lays it out again, even
        // when the window's logical size stays the same.
        ctx.device_scale_signal()
            .bind_to(id, registry, BindingLevel::Relayout);
        let mounted = Rc::new(Mounted {
            attachment,
            placement: Cell::new(None),
        });
        *self.shared.mounted.borrow_mut() = Rc::downgrade(&mounted);
        self.mounted = Some(mounted);
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.box_size(proposal, usable_scale(ctx.scale_factor))
    }

    /// The placement paint will draw and input maps through, computed once
    /// per layout: the edges snap in device space, through the widget's
    /// window origin, its ancestors' transforms and the device scale.
    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let Some(mounted) = &self.mounted else {
            return;
        };
        let to_device = self.pixel_snap.then(|| {
            let ancestors = ctx
                .arena()
                .map(|arena| arena.effective_transform(mounted.attachment.widget_id()))
                .unwrap_or(Transform2D::IDENTITY);
            ancestors.then(&Transform2D::scale(ctx.scale_factor, ctx.scale_factor))
        });
        let placement = self.place(
            bounds,
            ctx.is_rtl(),
            usable_scale(ctx.scale_factor),
            to_device,
        );
        mounted.placement.set(placement);
        // A source keeps its size once it has one (a cleared buffer's size
        // stays as its hint), so a placement is never taken back.
        if let Some(p) = placement {
            mounted.attachment.set_geometry(p.shown);
        }
    }

    /// The background, then the picture's one quad (emitted whatever the
    /// status, so the window takes the source's commits), inside an opacity
    /// scope while dimmed, then the placeholder while the source is not
    /// `Live`.
    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let Some(mounted) = &self.mounted else {
            return;
        };
        let rtl = matches!(ctx.layout_direction, LayoutDirection::RightToLeft);
        let shown = self
            .placement_for(mounted, bounds, rtl, usable_scale(ctx.scale_factor))
            .map(|p| p.shown);
        let live = self.shared.signals.status.get() == LiveImageStatus::Live;
        // The enabled state needs no binding: an ancestor's change repaints
        // the whole subtree.
        let dimmed = self.dim_when_disabled && !ctx.effective_enabled;

        let background = self.background.resolve(ctx.theme, ctx.effective_enabled);
        if background.a() > 0.0 {
            // A dimmed picture blends over the background, so it fills the
            // whole box then.
            match shown.and_then(|g| g.visible()).filter(|_| live && !dimmed) {
                Some(picture) => {
                    for rect in letterbox(bounds, at(picture, bounds)) {
                        canvas.fill_rect(rect, background);
                    }
                }
                None => canvas.fill_rect(bounds, background),
            }
        }

        let content = shown
            .map(|g| at(g.content, bounds))
            .unwrap_or(Rect::new(bounds.x, bounds.y, 0.0, 0.0));
        if dimmed {
            canvas.set_opacity(ctx.theme.colors.disabled_content_opacity());
        }
        canvas.draw_live_image(
            mounted.attachment.consumer(),
            &LiveImageDraw::new(content, bounds)
                .filter(self.scaling)
                .orientation(self.orientation)
                .paused(self.pause_when_inactive.get() && !ctx.window_active),
        );
        if dimmed {
            canvas.restore_opacity();
        }

        if !live {
            self.paint_placeholder(bounds, canvas, ctx);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        if self.a11y_hidden {
            builder.set_hidden();
            return;
        }
        debug_assert!(
            self.alt.is_some(),
            "LiveImage has no alt text — call .alt(\"…\") for a meaningful picture or \
             .a11y_hidden() for a decorative one"
        );
        builder.set_role(teksilo_core::accesskit::Role::Image);
        if let Some(alt) = &self.alt {
            builder.set_name(alt.get());
        }
        if self.shared.signals.status.get() != LiveImageStatus::Live {
            let placeholder = self.placeholder.get();
            if !placeholder.is_empty() {
                builder.set_description(placeholder);
            }
        }
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

impl std::fmt::Debug for LiveImage {
    /// What the source and the window did, from atomics and the widget's
    /// own state, never under the source's lock:
    /// `LiveImage { source: #3 "vm-screen" Bgrx8 720x1280 Live, gen: 5021,
    /// window_gen: 5019, sizing: Aspect, fit: Contain, orientation: Normal,
    /// scaling: Linear, content: (0, 0, 424, 754), paints: 4, paused: false }`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        struct Source<'a>(&'a LiveImageSource);
        impl std::fmt::Debug for Source<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                let source = self.0;
                write!(f, "{}", source.id())?;
                if let Some(label) = source.label() {
                    write!(f, " {label:?}")?;
                }
                write!(f, " {:?}", source.format())?;
                if let Some((w, h)) = source.size() {
                    write!(f, " {w}x{h}")?;
                }
                write!(f, " {:?}", source.status())
            }
        }
        struct Content(Option<Rect>);
        impl std::fmt::Debug for Content {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self.0 {
                    Some(r) => write!(f, "({}, {}, {}, {})", r.x, r.y, r.width, r.height),
                    None => f.write_str("none"),
                }
            }
        }
        let source = self.shared.source.borrow().clone();
        let attachment = self
            .mounted
            .as_ref()
            .map(|m| m.attachment.consumer().stats().attachment)
            .unwrap_or_default();
        let content = self
            .mounted
            .as_ref()
            .and_then(|m| m.placement.get())
            .map(|p| p.shown.content);
        let mut s = f.debug_struct("LiveImage");
        match &source {
            Some(source) => s
                .field("source", &Source(source))
                .field("gen", &source.generation()),
            None => s.field("source", &"none"),
        };
        s.field("window_gen", &attachment.window_generation)
            .field("sizing", &self.sizing)
            .field("fit", &self.fit)
            .field("orientation", &self.orientation)
            .field("scaling", &self.scaling)
            .field("content", &Content(content))
            .field("paints", &attachment.paints)
            .field("paused", &attachment.paused)
            .finish()
    }
}

/// A handle to a [`LiveImage`], for the UI thread: the size and status of
/// what it shows, where it shows it, and the mapping between its points and
/// the source's pixels. `Clone` is an `Rc` clone; `!Send`.
///
/// It owns the widget's Signals from construction, so it works before the
/// widget mounts, and they stay the same across a switch of source. It does
/// not keep the widget alive: once the widget is gone, it has no placement.
#[derive(Clone)]
pub struct LiveImageHandle {
    shared: Rc<Shared>,
}

impl LiveImageHandle {
    /// A handle for a widget not built yet, which takes it with
    /// [`LiveImage::with_handle`].
    pub fn new() -> Self {
        Self {
            shared: Rc::new(Shared {
                signals: LiveImageSignals::default(),
                source: RefCell::new(None),
                mounted: RefCell::new(Weak::new()),
            }),
        }
    }

    fn mounted(&self) -> Option<Rc<Mounted>> {
        self.shared.mounted.borrow().upgrade()
    }

    /// The widget's id while it is mounted.
    pub fn widget_id(&self) -> Option<WidgetId> {
        self.mounted().map(|m| m.attachment.widget_id())
    }

    /// The source the widget shows; `None` until a widget took the handle.
    pub fn source(&self) -> Option<LiveImageSource> {
        self.shared.source.borrow().clone()
    }

    /// The source's status as the window's last layout saw it: the same
    /// Signal across switches of source.
    pub fn status(&self) -> Signal<LiveImageStatus> {
        self.shared.signals.status.clone()
    }

    /// The frame size layout uses: the source's buffer, else its size hint,
    /// as the window's last layout saw it. The same Signal across switches
    /// of source.
    pub fn frame_size(&self) -> Signal<Option<(u32, u32)>> {
        self.shared.signals.frame_size.clone()
    }

    /// The placement of the last layout, widget-local: the one paint draws
    /// and the mappings below use. `None` before the first layout, with no
    /// frame size, or once the widget is gone.
    pub fn geometry(&self) -> Option<ImageGeometry> {
        self.mounted()?.placement.get().map(|p| p.shown)
    }

    /// The source pixel whose displayed square contains the widget-local
    /// point `local`; `None` on the letterbox, outside the widget, or
    /// without a placement. A pointer handler's positions are widget-local.
    pub fn map_to_source(&self, local: Point) -> Option<(u32, u32)> {
        self.geometry()?.map_to_source(local)
    }

    /// The source pixel nearest `local`: the point is clamped into the
    /// picture first, for a drag that leaves it. `None` only without a
    /// placement.
    pub fn map_to_source_clamped(&self, local: Point) -> Option<(u32, u32)> {
        self.geometry()?.map_to_source_clamped(local)
    }

    /// Continuous source coordinates of `local`, unclamped: pixel `k` spans
    /// `[k, k + 1)`.
    pub fn map_to_source_f32(&self, local: Point) -> Option<(f32, f32)> {
        self.geometry()?.map_to_source_f32(local)
    }

    /// Where the source pixels of `rect` are displayed, widget-local: to
    /// place an overlay on what the picture shows.
    pub fn map_from_source(&self, rect: PixelRect) -> Option<Rect> {
        self.geometry()?.map_from_source(rect)
    }

    /// The source's counters and, while the widget is mounted, its
    /// attachment's; zero attachment counters otherwise.
    pub fn stats(&self) -> LiveImageStats {
        if let Some(mounted) = self.mounted() {
            return mounted.attachment.consumer().stats();
        }
        let mut stats = LiveImageStats::default();
        if let Some(source) = self.source() {
            stats.source = source.stats();
        }
        stats
    }
}

impl Default for LiveImageHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for LiveImageHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveImageHandle")
            .field("widget", &self.widget_id())
            .field("signals", &self.shared.signals)
            .finish()
    }
}

#[cfg(test)]
mod dim_tests;
#[cfg(test)]
mod pause_tests;
#[cfg(test)]
mod tests;
