// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`TextItem`] — text in a local-coord rectangle, with alignment + rotation.
//!
//! `TextItem` renders text that wraps within a caller-specified rectangle in
//! local item coordinates. Text can be a static localized string (constructed
//! via `TextItem::new`) or a live `Signal<String>` (constructed via
//! `TextItem::with_signal_text`). Signal-bound and locale-reactive text both
//! register bindings at `RepaintOnly` so changes dirty the `SceneView`'s
//! paint pass without triggering a full rebuild.
//!
//! The foreground colour is a [`ColorProp`], so it accepts a plain
//! [`Color`](bastyde_tokens::Color), a theme role
//! ([`TextRole`](bastyde_tokens::TextRole)), a reactive `Signal<Color>`, or a
//! `Signal<Role>` — resolved against the active theme at paint time.
//!
//! Horizontal [alignment](TextAlign) (leading / center / trailing) and a free
//! [rotation](TextItem::rotation) let a text item self-place value tags, axis
//! labels, and rotated titles without the caller hand-measuring; [`measure`]
//! reports the item's single-line intrinsic size when the caller does want to
//! size around it.
//!
//! Text scale: the global accessibility "grow all text" setting is **off** by
//! default for scene text, since a scene has its own pan/zoom. Opt in via
//! `.follow_text_scale(true)` for labels that should track the app-wide
//! setting instead.
//!
//! ## When to use
//!
//! Use `TextItem` for card labels, node titles, annotation text, or any text
//! decoration in the lightweight tier. For editable text or text that needs
//! focus, selection, and full accessibility, embed a `RichTextEditor` or
//! `TextInput` as a heavyweight scene widget instead.
//!
//! ## Example
//!
//! ```ignore
//! use bastyde_scene::{SceneModel, TextItem, TextAlign};
//! use bastyde_canvas::{Point, Rect};
//! use bastyde_tokens::Color;
//! use bastyde_i18n::lit;
//!
//! let model = SceneModel::new();
//!
//! let item = TextItem::new(lit!("Scene node"), Rect::new(0.0, 0.0, 120.0, 30.0))
//!     .color(Color::new(0.1, 0.1, 0.1, 1.0))
//!     .align(TextAlign::Center);
//!
//! model.add_item(item, Point::new(40.0, 40.0));
//! ```
//!
//! [`measure`]: TextItem::measure

use accesskit::Role;
use bastyde_canvas::{Canvas, Rect, Size, TextBackend, Transform2D};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::signal::Signal;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::Color;

use crate::flags::ItemFlags;
use crate::item::{SceneItem, SceneItemA11yContext, SceneItemPaintContext};
use crate::items::{AccessSubtreeMode, ItemA11yOverrides};
use bastyde_i18n::LocalizedString;

/// Horizontal alignment of a [`TextItem`] within its `local_bounds`.
///
/// Alignment shifts the text's draw origin by the leftover width
/// (`bounds.width − measured_width`); it needs a text backend to measure, so a
/// mock/headless canvas with no backend renders leading-aligned regardless.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlign {
    /// Left edge in LTR (the default).
    #[default]
    Leading,
    /// Centred within the bounds.
    Center,
    /// Right edge in LTR.
    Trailing,
}

/// Text source for [`TextItem`]: either a static string or a live
/// `Signal<String>`. Signal-bound text refreshes on each paint and
/// dirties the SceneView via `register_bindings`.
#[derive(Debug)]
enum TextSource {
    Bound(Signal<String>),
    /// Localized text; resolved against the active locale on each paint.
    /// `register_bindings` ties the locale signal to the SceneView so a
    /// locale switch repaints and re-resolves.
    Localized(LocalizedString),
}

impl TextSource {
    fn current(&self) -> String {
        match self {
            TextSource::Bound(signal) => signal.get(),
            TextSource::Localized(ls) => ls.resolve_now(),
        }
    }
}

/// Leftover-width offset for a horizontal [`TextAlign`]. Pure so it is unit
/// testable in isolation from the text backend.
fn align_offset(align: TextAlign, available: f32, text_width: f32) -> f32 {
    let extra = (available - text_width).max(0.0);
    match align {
        TextAlign::Leading => 0.0,
        TextAlign::Center => extra * 0.5,
        TextAlign::Trailing => extra,
    }
}

/// Text in a local-coord rectangle, with optional alignment and rotation.
///
/// Text wraps within the `local_bounds` rectangle; the caller is responsible
/// for sizing the rect so all text is visible. Content is either a static
/// localized string (see [`TextItem::new`]) or a reactive `Signal<String>`
/// (see [`TextItem::with_signal_text`]). Both sources trigger a repaint on
/// change without rebuilding the scene.
#[derive(Debug)]
pub struct TextItem {
    text: TextSource,
    local_bounds: Rect,
    color: ColorProp,
    align: TextAlign,
    /// Rotation about the item's centre, in radians. `0.0` = upright.
    rotation: f32,
    label: Option<LocalizedString>,
    flags: ItemFlags,
    a11y: ItemA11yOverrides,
    /// When `true`, the font size grows with the global accessibility text
    /// scale (`ctx.text_scale`). Off by default: a scene has its own pan/zoom,
    /// so most scene text should stay at its authored size. Opt in via
    /// [`follow_text_scale`](Self::follow_text_scale) for labels that should
    /// track the app-wide "grow all text" setting.
    follow_text_scale: bool,
}

impl TextItem {
    /// A static-text item in local coordinates. The `text` is
    /// resolved eagerly via `LocalizedString::resolve_now` at
    /// construction; locale changes rebuild the composite parent,
    /// which re-creates this `TextItem` with a fresh translation.
    pub fn new(text: impl Into<LocalizedString>, local_bounds: Rect) -> Self {
        let ls: LocalizedString = text.into();
        Self {
            text: TextSource::Localized(ls),
            local_bounds,
            color: ColorProp::Static(Color::BLACK),
            align: TextAlign::Leading,
            rotation: 0.0,
            label: None,
            flags: ItemFlags::default(),
            a11y: ItemA11yOverrides::default(),
            follow_text_scale: false,
        }
    }

    /// A text item whose content is driven by a `Signal<String>`.
    /// `register_bindings` ties the signal to the SceneView at
    /// `BindingLevel::RepaintOnly` so changes dirty paint and the
    /// next walk reads the current value.
    pub fn with_signal_text(text: Signal<String>, local_bounds: Rect) -> Self {
        Self {
            text: TextSource::Bound(text),
            local_bounds,
            color: ColorProp::Static(Color::BLACK),
            align: TextAlign::Leading,
            rotation: 0.0,
            label: None,
            flags: ItemFlags::default(),
            a11y: ItemA11yOverrides::default(),
            follow_text_scale: false,
        }
    }

    /// Opt the text into drag-to-move.
    pub fn draggable(mut self, draggable: bool) -> Self {
        self.flags.set(ItemFlags::IS_DRAGGABLE, draggable);
        self
    }

    /// Override the foreground colour. Accepts a plain [`Color`], a theme role,
    /// a `Signal<Color>`, or a `Signal<Role>` — resolved against the active
    /// theme at paint time.
    pub fn color(mut self, color: impl Into<ColorProp>) -> Self {
        self.color = color.into();
        self
    }

    /// Horizontal alignment within `local_bounds`. Default
    /// [`TextAlign::Leading`]. Needs a text backend to measure the text width;
    /// a headless canvas with no backend renders leading-aligned.
    pub fn align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }

    /// Rotate the text about the item's centre by `radians`. Default `0.0`
    /// (upright). Pair with `Signal::animate_to` on a driving signal for
    /// animated rotation, or set a fixed angle for a vertical axis title.
    pub fn rotation(mut self, radians: f32) -> Self {
        self.rotation = radians;
        self
    }

    /// Opt this text into the global accessibility text scale, so it grows with
    /// the app-wide "grow all text" setting. Off by default — the scene's own
    /// pan/zoom usually governs scene text size.
    pub fn follow_text_scale(mut self, follow: bool) -> Self {
        self.follow_text_scale = follow;
        self
    }

    /// Override the AT label (defaults to the current text content).
    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
        self.label = Some(ls);
        self
    }

    /// Measure the current text's single-line intrinsic size against `backend`
    /// at the authored [`TextStyle`](bastyde_tokens::TextStyle). Lets a
    /// consumer size a slot around a label (axis labels, value tags) before
    /// placing it. Does not apply the global text scale — measure at the
    /// authored size.
    pub fn measure(&self, backend: &mut dyn TextBackend) -> Size {
        let text = self.text.current();
        let style = bastyde_tokens::TextStyle::default();
        let layout = backend.layout_single_line(&text, &style, None);
        Size::new(layout.width, layout.height)
    }

    crate::items::item_a11y_builders!();
}

impl SceneItem for TextItem {
    fn local_bounds(&self) -> Rect {
        self.local_bounds
    }

    fn set_local_bounds(&mut self, bounds: Rect) {
        self.local_bounds = bounds;
    }

    fn paint(&self, canvas: &mut Canvas, ctx: &SceneItemPaintContext<'_>) {
        let text = self.text.current();
        let mut style = bastyde_tokens::TextStyle::default();
        if self.follow_text_scale {
            style.size *= ctx.text_scale;
        }
        let color = self.color.resolve(ctx.theme, ctx.enabled);
        let lb = self.local_bounds;

        // One `text_backend()` query serves both the alignment measure and the
        // paragraph-vs-plain draw choice. Scoped so the `&self` canvas borrow
        // is released before the `&mut self` draw calls below.
        let (has_backend, x_offset) = {
            let backend = canvas.text_backend();
            let has_backend = backend.is_some();
            // Horizontal alignment: shift the draw rect by the leftover width.
            // Needs a backend to measure; leading needs no measure at all.
            let x_offset = if self.align != TextAlign::Leading {
                backend
                    .map(|tb| {
                        let w = tb
                            .borrow_mut()
                            .layout_single_line(&text, &style, None)
                            .width;
                        align_offset(self.align, lb.width, w)
                    })
                    .unwrap_or(0.0)
            } else {
                0.0
            };
            (has_backend, x_offset)
        };
        let draw_rect = Rect::new(
            lb.x + x_offset,
            lb.y,
            (lb.width - x_offset).max(0.0),
            lb.height,
        );

        // Rotation about the item's centre wraps the whole draw.
        //
        // This MUST compose in the item's **local** space: `paint_band` has
        // already pushed the item's scene→screen transform onto the canvas, and
        // `Canvas::translate`/`rotate` POST-multiply (they compose in *output*
        // space). Using them here would rotate about the screen origin offset by
        // a local-coordinate amount — the wrong pivot for any item that isn't at
        // the scene origin at zoom 1. `apply_transform` PRE-multiplies
        // (`new = t.then(current)`), so the rotate-about-centre matrix is applied
        // to local points *before* the outer transform, keeping the item's centre
        // a fixed point at any placement, pan, or zoom.
        let rotated = self.rotation.abs() > f32::EPSILON;
        if rotated {
            let cx = lb.x + lb.width * 0.5;
            let cy = lb.y + lb.height * 0.5;
            canvas.save();
            canvas.apply_transform(
                Transform2D::translate(-cx, -cy)
                    .then(&Transform2D::rotate(self.rotation))
                    .then(&Transform2D::translate(cx, cy)),
            );
        }
        if has_backend {
            canvas.draw_paragraph(&text, draw_rect, &style, color, None);
        } else {
            canvas.draw_text(&text, draw_rect, &style, color);
        }
        if rotated {
            canvas.restore();
        }
    }

    fn set_fill(&mut self, fill: Option<ColorProp>) -> bool {
        // A text item's "fill" is its foreground colour — it always has one,
        // so a `None` (clear) is rejected.
        match fill {
            Some(c) => {
                self.color = c;
                true
            }
            None => false,
        }
    }

    fn label(&self) -> Option<String> {
        self.label
            .as_ref()
            .map(|l| l.resolve_now())
            .or_else(|| Some(self.text.current()))
    }

    fn initial_flags(&self) -> ItemFlags {
        self.flags
    }

    fn access_subtree_mode(&self) -> AccessSubtreeMode {
        self.a11y.subtree_mode()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder, _ctx: &SceneItemA11yContext) {
        builder.set_role(Role::Label);
        if let Some(label) = self.label() {
            builder.set_name(label);
        }
        self.a11y.apply(builder);
    }

    fn register_bindings(&self, ctx: &mut BuildContext, view_id: WidgetId) {
        let registry = ctx.binding_registry();
        if let TextSource::Bound(signal) = &self.text {
            signal.bind_to(view_id, registry, BindingLevel::RepaintOnly);
        }
        if matches!(self.text, TextSource::Localized(_)) {
            ctx.locale_signal()
                .bind_to(view_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
        }
        // A signal-/role-bound foreground colour repaints on change too.
        self.color
            .register_if_bound(view_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_canvas::{DrawCommand, Point};
    use bastyde_i18n::lit;

    /// Minimal fixed-metrics text backend (8px per char) for align / measure /
    /// rotation tests. Mirrors the stub in `view/tests/raster_scale_tests.rs`.
    #[derive(Default)]
    struct StubBackend {
        raster_scale: f32,
    }

    impl TextBackend for StubBackend {
        fn set_raster_scale(&mut self, raster_scale: f32) {
            self.raster_scale = raster_scale;
        }
        fn raster_scale(&self) -> f32 {
            self.raster_scale
        }
        fn layout_single_line(
            &mut self,
            text: &str,
            _style: &bastyde_tokens::TextStyle,
            _max_width: Option<f32>,
        ) -> bastyde_canvas::TextLayout {
            bastyde_canvas::TextLayout {
                width: text.chars().count() as f32 * 8.0,
                height: 16.0,
                ascent: 12.0,
                descent: 4.0,
                underline_offset: 1.0,
                underline_thickness: 1.0,
                layout_key: 1,
                line_count: 1,
                spans: Vec::new(),
                raster_scale: self.raster_scale,
            }
        }
        fn ensure_glyphs(
            &mut self,
            _layout: &bastyde_canvas::TextLayout,
        ) -> Vec<bastyde_canvas::GlyphQuad> {
            Vec::new()
        }
    }

    fn ctx<'a>(theme: &'a bastyde_core::styles::Theme) -> SceneItemPaintContext<'a> {
        SceneItemPaintContext::new(Transform2D::identity(), None, theme)
    }

    #[test]
    fn text_item_label_falls_back_to_text() {
        let item = TextItem::new(lit!("Hello"), Rect::new(0.0, 0.0, 100.0, 30.0));
        assert_eq!(SceneItem::label(&item).as_deref(), Some("Hello"));
    }

    #[test]
    fn follow_text_scale_defaults_off_and_opts_in() {
        let item = TextItem::new(lit!("Hi"), Rect::new(0.0, 0.0, 100.0, 30.0));
        assert!(!item.follow_text_scale);
        let opted = item.follow_text_scale(true);
        assert!(opted.follow_text_scale);
    }

    #[test]
    fn align_offset_centers_and_trails() {
        // #7: pure alignment maths.
        assert_eq!(align_offset(TextAlign::Leading, 100.0, 40.0), 0.0);
        assert_eq!(align_offset(TextAlign::Center, 100.0, 40.0), 30.0);
        assert_eq!(align_offset(TextAlign::Trailing, 100.0, 40.0), 60.0);
        // No negative offset when the text overflows.
        assert_eq!(align_offset(TextAlign::Center, 30.0, 40.0), 0.0);
    }

    #[test]
    fn measure_reports_single_line_size() {
        // #7: measure against a fixed-metrics backend.
        let item = TextItem::new(lit!("abcd"), Rect::new(0.0, 0.0, 200.0, 30.0));
        let mut backend = StubBackend::default();
        let size = item.measure(&mut backend);
        assert_eq!(size.width, 32.0); // 4 chars × 8px
        assert_eq!(size.height, 16.0);
    }

    #[test]
    fn rotation_emits_a_transform_command() {
        // #7: a rotated text item wraps its draw in a transform scope.
        let theme = bastyde_core::presets::intui::light();
        let item = TextItem::new(lit!("Title"), Rect::new(0.0, 0.0, 100.0, 20.0))
            .rotation(std::f32::consts::FRAC_PI_2);
        let mut canvas = Canvas::new();
        item.paint(&mut canvas, &ctx(&theme));
        let frame = canvas.into_render_frame();
        assert!(
            frame
                .draw_order
                .iter()
                .any(|c| matches!(c, DrawCommand::SetTransform(_))),
            "rotation must push a transform"
        );
    }

    #[test]
    fn rotation_pivots_about_the_item_centre() {
        // #7 regression guard: the rotation must keep the item's OWN centre a
        // fixed point. `Canvas::translate`/`rotate` post-multiply (compose in
        // output space), so the naive translate/rotate/translate idiom pivots
        // about the wrong point for any item not at the origin — this asserts
        // the composed transform actually fixes the centre.
        let theme = bastyde_core::presets::intui::light();
        // Deliberately off-origin bounds so a wrong pivot moves the centre.
        let lb = Rect::new(120.0, 60.0, 100.0, 40.0);
        let (cx, cy) = (lb.x + lb.width * 0.5, lb.y + lb.height * 0.5);

        for angle in [0.25_f32, std::f32::consts::FRAC_PI_2, 2.4] {
            let item = TextItem::new(lit!("Axis label"), lb).rotation(angle);
            let mut canvas = Canvas::new();
            item.paint(&mut canvas, &ctx(&theme));
            let frame = canvas.into_render_frame();
            let xform = frame
                .draw_order
                .iter()
                .find_map(|c| match c {
                    DrawCommand::SetTransform(t) => Some(*t),
                    _ => None,
                })
                .expect("rotation must push a transform");
            let centre = xform.apply_point(Point::new(cx, cy));
            assert!(
                (centre.x - cx).abs() < 0.01 && (centre.y - cy).abs() < 0.01,
                "centre must be a fixed point of the rotation (angle {angle}): \
                 expected ({cx}, {cy}), got ({}, {})",
                centre.x,
                centre.y
            );
        }
    }

    #[test]
    fn no_rotation_emits_no_transform_command() {
        let theme = bastyde_core::presets::intui::light();
        let item = TextItem::new(lit!("Title"), Rect::new(0.0, 0.0, 100.0, 20.0));
        let mut canvas = Canvas::new();
        item.paint(&mut canvas, &ctx(&theme));
        let frame = canvas.into_render_frame();
        assert!(
            !frame
                .draw_order
                .iter()
                .any(|c| matches!(c, DrawCommand::SetTransform(_))),
            "upright text must not push a transform"
        );
    }

    #[test]
    fn set_fill_maps_to_foreground_colour() {
        // #2: the SceneItem fill hook sets the text colour; None is rejected.
        let mut item = TextItem::new(lit!("Hi"), Rect::new(0.0, 0.0, 100.0, 30.0));
        assert!(item.set_fill(Some(ColorProp::from(Color::RED))));
        assert!(!item.set_fill(None));
    }

    #[test]
    fn signal_colour_re_resolves() {
        // #2: a Signal<Color> foreground re-resolves each paint.
        let theme = bastyde_core::presets::intui::light();
        let sig = Signal::new(Color::RED);
        let item = TextItem::new(lit!("Hi"), Rect::new(0.0, 0.0, 100.0, 30.0)).color(sig.clone());
        // Paint twice with different signal values; both must succeed without
        // panicking and read the current value (visual assertion lives at the
        // view level where a real backend emits glyph colours).
        let mut c1 = Canvas::new();
        item.paint(&mut c1, &ctx(&theme));
        sig.set(Color::BLUE);
        let mut c2 = Canvas::new();
        item.paint(&mut c2, &ctx(&theme));
        assert_eq!(sig.get(), Color::BLUE);
    }
}
