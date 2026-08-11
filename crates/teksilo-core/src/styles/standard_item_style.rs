// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `StandardListItem` / `StandardTreeItem`.
//! See `docs/styling-system.md`.

use std::rc::Rc;

use teksilo_tokens::TextRole;

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

#[derive(Clone, Debug)]
pub struct StandardItemStyleConfig {
    /// Pre-built row content — typically an HStack of `[checkbox?]
    /// [leading?] [center?] [label / VStack { label, subtitle row }]
    /// [Spacer] [trailing?]` composed by the host
    /// `StandardListItem` / `StandardTreeItem`. The style is
    /// responsible for the chrome (selection background, corner
    /// radius, padding) but not for row-internal layout — the
    /// per-slot composition is StandardItem-specific (subtitle has
    /// its own sub-row, the checkbox carries `labels_hidden` AT
    /// metadata, etc.) and would force every custom style to
    /// reimplement it.
    pub content: WidgetId,
    pub is_selected: Signal<bool>,
    pub is_hovered: Signal<bool>,
    pub is_pressed: Signal<bool>,
    pub is_focused: Signal<bool>,
    /// Input-modality "focus-visible": `true` after keyboard input. A focus ring
    /// should render only when this and `is_focused` are both true, so a mouse
    /// click selects without a ring while keyboard navigation reveals one.
    pub is_focus_visible: Signal<bool>,
    pub is_disabled: Signal<bool>,
    /// Whether the host window is currently active (`focused AND not occluded`).
    /// Composed with `is_focused` so a selected row shows the vivid `Selected`
    /// surface only while the view holds keyboard focus **and** the window is
    /// active; otherwise it falls back to the muted `SelectedInactive` — the
    /// same desaturation a view-focus loss produces (macOS "unemphasized"
    /// selection serves both states). Populated from
    /// `BuildContext::window_active_signal`.
    pub is_window_active: Signal<bool>,
}

pub trait StandardItemStyle: 'static {
    fn make_body(&self, cfg: &StandardItemStyleConfig, ctx: &mut BuildContext) -> WidgetId;

    /// The text role a row's label takes while the row is **emphasised**
    /// — selected, with its view focused and its window active. `None`
    /// (the default) keeps the row's own mapping, `TextRole::Primary`.
    ///
    /// The row builds its label before a style ever sees it, so a style
    /// that fills the selection with a saturated colour cannot recolour
    /// the text on top of it. Design languages whose selected row is a
    /// solid accent fill with a light label — macOS's
    /// `alternateSelectedControlTextColor` is the canonical case — return
    /// [`TextRole::OnAccent`] here; ones whose selection is a pale wash
    /// (IntUI, Fluent) leave it `None` and the label keeps its contrast
    /// against the wash.
    ///
    /// Same shape as
    /// [`ButtonStyle::label_text_role`](crate::styles::ButtonStyle::label_text_role),
    /// and defaulted for the same reason: an existing style needs no
    /// change.
    fn selected_label_role(&self) -> Option<TextRole> {
        None
    }
}

pub type SharedStandardItemStyle = Rc<dyn StandardItemStyle>;
