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

/// The row dimensions a menu style owns that `MenuItem` and `MenuList` must
/// know *before* `make_body` runs — or that belong to a sibling widget
/// entirely.
///
/// A style decides the row's layout, but it does not build every part of it.
/// `MenuItem` builds the leading icon/check column and the trailing chevron
/// column itself (they are slot *contents*, handed to the style), and the
/// separator between two rows is a `MenuSeparator` widget the style never
/// sees at all. Those three sizes therefore cannot come out of `make_body`,
/// and until this existed they were read from the IntUI module constants —
/// so a preset could set them on its recipe and nothing moved. macOS asked
/// for a 14 dp check column and got 16; Fluent asked for a 3 dp separator
/// and macOS for 11, and both got 9.
///
/// The defaults are the IntUI numbers those call sites used to hardcode, so
/// a style that does not override this behaves exactly as before.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MenuItemMetrics {
    /// Width (and height) of the leading icon / checkmark / radio column.
    /// Reserved even on a row with no glyph, so labels line up.
    pub icon_column_width: f32,
    /// Width of the trailing chevron column, always reserved so a submenu
    /// row and a plain row share a trailing edge. Doubles as the gap
    /// between an accelerator and a trailing hint.
    pub trailing_column_width: f32,
    /// Full height of a `MenuSeparator`'s slot — the hairline plus its air.
    pub separator_height: f32,
    /// One row's height, used to size a `max_visible_items` scroll viewport.
    pub item_height: f32,
}

impl Default for MenuItemMetrics {
    fn default() -> Self {
        Self {
            icon_column_width: 16.0,
            trailing_column_width: 12.0,
            separator_height: 9.0,
            item_height: 24.0,
        }
    }
}

pub trait MenuItemStyle: 'static {
    fn make_body(&self, cfg: &MenuItemStyleConfig, ctx: &mut BuildContext) -> WidgetId;

    /// Row dimensions `MenuItem` / `MenuList` need outside `make_body`.
    ///
    /// Defaulted to the IntUI numbers, so an existing style needs no change.
    /// A style that delegates its row to another one should delegate this
    /// too, or the slots it is handed will not match the row it builds.
    fn metrics(&self) -> MenuItemMetrics {
        MenuItemMetrics::default()
    }

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
