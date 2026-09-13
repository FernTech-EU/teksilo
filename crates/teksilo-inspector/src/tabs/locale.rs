// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Locale tab — switch the active locale on the fly.
//!
//! Lists every locale declared in `I18nConfig::supported_locales`
//! (read via `teksilo_i18n::current_supported_locales()`), one per row.
//! Tapping a row calls `EventContext::set_locale(...)`. Without a
//! configured `I18nManager` the tab shows a hint message and does
//! nothing.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use teksilo_canvas::{Canvas, Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{Color, CornerRadius, TextRole};

use crate::state::InspectorState;
use crate::tabs::{ROW_HEIGHT, ROW_PADDING_X, row_height};

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
    /// The row height the last layout pass painted at, published for the tap
    /// handler: rows are pressed here, so their height follows the density —
    /// and an `EventContext` has no theme to read it from.
    row_height: Rc<Cell<f32>>,
}

impl LocaleTab {
    pub fn new(state: InspectorState) -> Self {
        Self {
            state,
            locales: Rc::new(RefCell::new(Vec::new())),
            active: RefCell::new(None),
            row_height: Rc::new(Cell::new(ROW_HEIGHT)),
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
        let rh_handle = self.row_height.clone();
        let handlers = HandlerSet::new()
            .focusable(true)
            .on_tap(move |event, event_ctx| {
                let idx = (event.position.y / rh_handle.get()).floor() as usize;
                let tag = snapshot_handle.borrow().get(idx).cloned();
                if let Some(tag) = tag {
                    event_ctx.set_locale(tag);
                }
            });
        ctx.apply_self_handlers(handlers);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let rh = row_height(&ctx.theme.input);
        self.row_height.set(rh);
        let supported = teksilo_i18n::current_supported_locales().unwrap_or_default();
        let active = teksilo_i18n::current_locale().map(|s| s.get().to_string());
        let tags: Vec<String> = supported.iter().map(|l| l.to_string()).collect();
        let height = if tags.is_empty() {
            rh
        } else {
            tags.len() as f32 * rh
        };
        *self.locales.borrow_mut() = tags;
        *self.active.borrow_mut() = active;
        proposal.resolve(0.0, height).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let rh = row_height(&ctx.theme.input);
        let theme = ctx.theme;
        let style = &theme.typography.body;
        let primary = TextRole::Primary.resolve(&theme.colors);
        let secondary = TextRole::Secondary.resolve(&theme.colors);

        let tags = self.locales.borrow();
        let active = self.active.borrow();

        if tags.is_empty() {
            let r = Rect::new(bounds.x + ROW_PADDING_X, bounds.y + 2.0, bounds.width, rh);
            canvas.draw_text("(no I18nManager configured)", r, style, secondary);
            return;
        }

        for (i, tag) in tags.iter().enumerate() {
            let y = bounds.y + (i as f32) * rh;
            let row_rect = Rect::new(bounds.x, y, bounds.width, rh);
            let is_active = active.as_deref() == Some(tag.as_str());
            if is_active {
                let bg = Color::from_rgba(0.13, 0.55, 1.0, 0.15);
                canvas.fill_rounded_rect(row_rect, CornerRadius::ZERO, bg);
            }
            let text_rect = Rect::new(bounds.x + ROW_PADDING_X, y + 2.0, bounds.width, rh);
            let color = if is_active { primary } else { secondary };
            let label = if is_active {
                format!("{}  (active)", tag)
            } else {
                tag.clone()
            };
            canvas.draw_text(&label, text_rect, style, color);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // The supported locale tags, one per row.
        // The house convention for painted text (`TextWidget` does exactly
        // this): one `Role::Label` whose name is what is on the screen.
        // Without it the tab is a blank rectangle to a screen reader.
        builder.set_role(teksilo_core::accesskit::Role::Label);
        builder.set_name(self.locales.borrow().join("\n"));
    }
}
