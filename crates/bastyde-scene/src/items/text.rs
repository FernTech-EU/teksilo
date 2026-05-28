//! [`TextItem`] — unstyled text in a local-coord rectangle.
//!
//! Text content can be a static string or a live `Signal<String>`;
//! signal-bound text dirties the SceneView's paint via
//! `register_bindings`.

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

/// Text source for [`TextItem`]: either a static string or a live
/// `Signal<String>`. Signal-bound text refreshes on each paint and
/// dirties the SceneView via `register_bindings`.
#[derive(Debug)]
enum TextSource {
    Bound(Signal<String>),
    /// Localized text; resolved against the active locale on each paint.
    /// `register_bindings` ties the locale signal to the SceneView so a
    /// locale switch repaints and re-resolves.
    Localized(bastyde_i18n::LocalizedString),
}

impl TextSource {
    fn current(&self) -> String {
        match self {
            TextSource::Bound(signal) => signal.get(),
            TextSource::Localized(ls) => ls.resolve_now(),
        }
    }
}

/// Unstyled text in a local-coord rectangle. Text wraps within the
/// rect; size is the caller's responsibility.
#[derive(Debug)]
pub struct TextItem {
    text: TextSource,
    local_bounds: Rect,
    color: Color,
    label: Option<bastyde_i18n::LocalizedString>,
    flags: ItemFlags,
    a11y: ItemA11yOverrides,
}

impl TextItem {
    /// A static-text item in local coordinates. The `text` is
    /// resolved eagerly via `LocalizedString::resolve_now` at
    /// construction; locale changes rebuild the composite parent,
    /// which re-creates this `TextItem` with a fresh translation.
    pub fn new(text: impl Into<bastyde_i18n::LocalizedString>, local_bounds: Rect) -> Self {
        let ls: bastyde_i18n::LocalizedString = text.into();
        Self {
            text: TextSource::Localized(ls),
            local_bounds,
            color: Color::BLACK,
            label: None,
            flags: ItemFlags::default(),
            a11y: ItemA11yOverrides::default(),
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

    /// Override the AT label (defaults to the current text content).
    pub fn label(mut self, label: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        let ls: bastyde_i18n::LocalizedString = label.into();
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

    fn paint(&self, canvas: &mut Canvas, _ctx: &SceneItemPaintContext) {
        let text = self.text.current();
        let style = bastyde_tokens::TextStyle::default();
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
}
