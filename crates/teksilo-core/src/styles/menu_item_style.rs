// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `MenuItem`. See `docs/styling-system.md`.

use std::rc::Rc;

use teksilo_tokens::TextRole;

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

#[derive(Clone, Debug)]
pub struct MenuItemStyleConfig {
    pub label: WidgetId,
    /// Optional leading slot (icon, checkmark, radio dot).
    pub leading: Option<WidgetId>,
    /// Optional trailing slot (shortcut chip, submenu chevron).
    pub trailing: Option<WidgetId>,
    pub is_hovered: Signal<bool>,
    pub is_pressed: Signal<bool>,
    pub is_focused: Signal<bool>,
    pub is_disabled: Signal<bool>,
    /// Bound to keyboard-arrow navigation within the parent menu.
    pub is_highlighted: Signal<bool>,
}

pub trait MenuItemStyle: 'static {
    fn make_body(&self, cfg: &MenuItemStyleConfig, ctx: &mut BuildContext) -> WidgetId;

    /// The text role a menu row's label and shortcut take while the row is
    /// **highlighted** — hovered, or reached by keyboard navigation.
    /// `None` (the default) keeps the row's own mapping.
    ///
    /// The row builds its label before a style ever sees it, so a style
    /// that fills the highlight with a saturated colour cannot recolour
    /// the text on top of it. macOS fills a highlighted menu row with the
    /// accent and flips its label to `selectedMenuItemTextColor` (white),
    /// so its style returns [`TextRole::OnAccent`]; IntUI and Fluent both
    /// use a neutral wash and leave this `None`.
    ///
    /// Same shape as
    /// [`ButtonStyle::label_text_role`](crate::styles::ButtonStyle::label_text_role),
    /// and defaulted for the same reason: an existing style needs no
    /// change.
    fn highlighted_label_role(&self) -> Option<TextRole> {
        None
    }
}

pub type SharedMenuItemStyle = Rc<dyn MenuItemStyle>;
