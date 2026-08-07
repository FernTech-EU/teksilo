// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `RadioTile`. See `docs/styling-system.md`.
//!
//! A `RadioTile` is a "selectable card" — a bordered, rounded surface that
//! behaves as a single radio option (`Role::RadioButton`, `set_toggled`)
//! while rendering a leading icon, a bold title, an inline radio indicator,
//! and a muted description. The host widget composes the tile's *content*
//! (icon / title / indicator / description) and hands it to the style as a
//! single `content` id; the style owns only the card chrome — background,
//! border, corner radius, padding, and any elevation shadow — driven by the
//! selection / hover / press / focus / disabled cascade.
//!
//! This mirrors [`StandardItemStyleConfig`](crate::styles::StandardItemStyleConfig)
//! (content-plus-state) rather than [`RadioStyleConfig`](crate::styles::RadioStyleConfig)
//! (a bare glyph), because a tile is a container whose chrome wraps arbitrary
//! content, not a self-drawn mark.

use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

/// Design-language variant for a `RadioTile` (in `teksilo-widgets`).
/// The active `RadioTileStyle` decides what each variant means visually.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub enum RadioTileVariant {
    /// IntUI default: a subtle surface with a 1 dp border; the selected
    /// tile gains an accent border. No shadow. Matches the flat
    /// "selectable card" look.
    #[default]
    Outlined,
    /// A raised card with a drop shadow (the theme's `shadow_md`). Use
    /// when tiles float above the surrounding surface.
    Elevated,
    /// A solid filled surface (no border in the resting state). The
    /// selection cue is the fill tint and the radio indicator.
    Filled,
}

/// Everything a [`RadioTileStyle`] needs to render one tile's chrome.
///
/// All state is delivered as reactive `Signal`s so the chrome repaints on
/// interaction without a rebuild. The window-active / focus split matches
/// [`StandardItemStyleConfig`](crate::styles::StandardItemStyleConfig): a
/// selected tile shows the vivid selection surface only while its group
/// holds keyboard focus **and** the window is active, and the keyboard focus
/// ring appears only under `is_focused && is_focus_visible`.
#[derive(Clone, Debug)]
pub struct RadioTileStyleConfig {
    /// Pre-composed tile content — typically a `VStack` of
    /// `[HStack { icon?, title, Spacer, indicator? }, description|body?]`
    /// built by the host `RadioTile`. The style wraps it in the card chrome
    /// but does not lay it out internally.
    pub content: WidgetId,
    pub is_selected: Signal<bool>,
    pub is_hovered: Signal<bool>,
    pub is_pressed: Signal<bool>,
    /// Whether the tile's group (or the tile itself, when standalone) holds
    /// keyboard focus. Combined with `is_window_active` to pick the vivid vs
    /// muted selection surface.
    pub is_focused: Signal<bool>,
    /// Input-modality "focus-visible": `true` after keyboard input. The focus
    /// ring renders only when this and `is_focused` are both true, so a mouse
    /// click selects without a ring while keyboard navigation reveals one.
    pub is_focus_visible: Signal<bool>,
    pub is_disabled: Signal<bool>,
    /// Whether the host window is active (`focused AND not occluded`).
    /// Populated from [`BuildContext::window_active_signal`].
    pub is_window_active: Signal<bool>,
    pub variant: RadioTileVariant,
    /// The tile is in the compact single-line arrangement (a
    /// `TileLayout::Vertical` row): the style should center its content in the
    /// fixed row height rather than pad-and-top-anchor it, so a short fixed
    /// height never over-constrains the content.
    pub is_compact: bool,
}

/// Tier-3 style protocol for `RadioTile`. Implement this to fully replace the
/// card chrome (per-call `RadioTile::style(...)` or theme-wide
/// `theme.style_slots.radio_tile = Some(Rc::new(MyTile))`).
pub trait RadioTileStyle: 'static {
    fn make_body(&self, cfg: &RadioTileStyleConfig, ctx: &mut BuildContext) -> WidgetId;

    /// Fixed row height, in logical pixels, for a
    /// `RadioTileGroup` in `TileLayout::Vertical` (the compact settings-list
    /// arrangement). A theme-driven value — override it in a custom style to
    /// change how tall the compact rows are. The default recipe reads it from
    /// its `RadioTileRecipe`.
    fn vertical_row_height(&self) -> f32 {
        44.0
    }
}

pub type SharedRadioTileStyle = Rc<dyn RadioTileStyle>;
