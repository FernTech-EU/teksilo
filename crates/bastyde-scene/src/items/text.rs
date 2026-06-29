// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`TextItem`] — unstyled text in a local-coord rectangle.
//!
//! `TextItem` renders text that wraps within a caller-specified rectangle in
//! local item coordinates. Text can be a static localized string (constructed
//! via `TextItem::new`) or a live `Signal<String>` (constructed via
//! `TextItem::with_signal_text`). Signal-bound and locale-reactive text both
//! register bindings at `RepaintOnly` so changes dirty the `SceneView`'s
//! paint pass without triggering a full rebuild.
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
//! use bastyde_scene::{SceneModel, TextItem};
//! use bastyde_canvas::{Point, Rect};
//! use bastyde_tokens::Color;
//! use bastyde_i18n::lit;
//!
//! let model = SceneModel::new();
//!
//! let item = TextItem::new(lit!("Scene node"), Rect::new(0.0, 0.0, 120.0, 30.0))
//!     .color(Color::new(0.1, 0.1, 0.1, 1.0));
//!
//! model.add_item(item, Point::new(40.0, 40.0));
//! ```

use accesskit::Role;
use bastyde_canvas::{Canvas, Rect};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::Color;

use crate::flags::ItemFlags;
use crate::item::{SceneItem, SceneItemA11yContext, SceneItemPaintContext};
use crate::items::{AccessSubtreeMode, ItemA11yOverrides};
use bastyde_i18n::LocalizedString;

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

/// Unstyled text in a local-coord rectangle.
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
    color: Color,
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
            color: Color::BLACK,
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
            color: Color::BLACK,
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

    /// Override the foreground color.
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
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

    crate::items::item_a11y_builders!();
}

impl SceneItem for TextItem {
    fn local_bounds(&self) -> Rect {
        self.local_bounds
    }

    fn set_local_bounds(&mut self, bounds: Rect) {
        self.local_bounds = bounds;
    }

    fn paint(&self, canvas: &mut Canvas, ctx: &SceneItemPaintContext) {
        let text = self.text.current();
        let mut style = bastyde_tokens::TextStyle::default();
        if self.follow_text_scale {
            style.size *= ctx.text_scale;
        }
        if canvas.text_backend().is_some() {
            canvas.draw_paragraph(&text, self.local_bounds, &style, self.color, None);
        } else {
            canvas.draw_text(&text, self.local_bounds, &style, self.color);
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
        if let TextSource::Bound(signal) = &self.text {
            signal.bind_to(view_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
        }
        if matches!(self.text, TextSource::Localized(_)) {
            ctx.locale_signal()
                .bind_to(view_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_i18n::lit;

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
}
