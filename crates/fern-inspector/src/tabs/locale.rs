//! Locale tab — switch the active locale on the fly.
//!
//! Lists every locale declared in `I18nConfig::supported_locales`
//! (read via `fern_i18n::current_supported_locales()`), one per row.
//! Tapping a row calls `EventContext::set_locale(...)`. Without a
//! configured `I18nManager` the tab shows a hint message and does
//! nothing.

use std::cell::RefCell;
use std::rc::Rc;

use fern_canvas::{Canvas, Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, CornerRadius, TextRole};

use crate::state::InspectorState;
use crate::tabs::{ROW_HEIGHT, ROW_PADDING_X};

pub(crate) struct LocaleTab {
    #[allow(dead_code)]
    state: InspectorState,
    /// Snapshot of supported locale tags (e.g. "en-US", "fr-FR"),
    /// shared with the on_tap handler so a click resolves to the
    /// correct row index. Refreshed on every layout pass.
    locales: Rc<RefCell<Vec<String>>>,
    /// Active locale tag (string form). Used by `paint` to highlight
    /// the active row.
    active: RefCell<Option<String>>,
}

impl LocaleTab {
    pub fn new(state: InspectorState) -> Self {
        Self {
            state,
            locales: Rc::new(RefCell::new(Vec::new())),
            active: RefCell::new(None),
        }
    }
}

impl std::fmt::Debug for LocaleTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocaleTab").finish()
    }
}

impl Widget for LocaleTab {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let snapshot_handle = self.locales.clone();
        let handlers = HandlerSet::new()
            .focusable(true)
            .on_tap(move |event, event_ctx| {
                let idx = (event.position.y / ROW_HEIGHT).floor() as usize;
                let tag = snapshot_handle.borrow().get(idx).cloned();
                if let Some(tag) = tag {
                    event_ctx.set_locale(tag);
                }
            });
        ctx.apply_self_handlers(handlers);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        let supported = fern_i18n::current_supported_locales().unwrap_or_default();
        let active = fern_i18n::current_locale().map(|s| s.get().to_string());
        let tags: Vec<String> = supported.iter().map(|l| l.to_string()).collect();
        let height = if tags.is_empty() {
            ROW_HEIGHT
        } else {
            tags.len() as f32 * ROW_HEIGHT
        };
        *self.locales.borrow_mut() = tags;
        *self.active.borrow_mut() = active;
        proposal.resolve(0.0, height).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let theme = ctx.theme;
        let style = &theme.typography.body;
        let primary = TextRole::Primary.resolve(&theme.colors);
        let secondary = TextRole::Secondary.resolve(&theme.colors);

        let tags = self.locales.borrow();
        let active = self.active.borrow();

        if tags.is_empty() {
            let r = Rect::new(
                bounds.x + ROW_PADDING_X,
                bounds.y + 2.0,
                bounds.width,
                ROW_HEIGHT,
            );
            canvas.draw_text("(no I18nManager configured)", r, style, secondary);
            return;
        }

        for (i, tag) in tags.iter().enumerate() {
            let y = bounds.y + (i as f32) * ROW_HEIGHT;
            let row_rect = Rect::new(bounds.x, y, bounds.width, ROW_HEIGHT);
            let is_active = active.as_deref() == Some(tag.as_str());
            if is_active {
                let bg = Color::from_rgba(0.13, 0.55, 1.0, 0.15);
                canvas.fill_rounded_rect(row_rect, CornerRadius::ZERO, bg);
            }
            let text_rect = Rect::new(
                bounds.x + ROW_PADDING_X,
                y + 2.0,
                bounds.width,
                ROW_HEIGHT,
            );
            let color = if is_active { primary } else { secondary };
            let label = if is_active {
                format!("{}  (active)", tag)
            } else {
                tag.clone()
            };
            canvas.draw_text(&label, text_rect, style, color);
        }
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}
