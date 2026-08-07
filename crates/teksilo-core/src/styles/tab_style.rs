// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `TabBar`. See `docs/styling-system.md`.
//!
//! `TabBar` has two themable surfaces, so the trait carries two
//! methods: [`TabStyle::make_body`] wraps a single tab header (accent
//! indicator, focus ring, …) and [`TabStyle::make_bar`] wraps the
//! whole strip (backdrop fill, content-pane separator, drag-reorder
//! drop indicator). A custom `impl TabStyle` provides both.

use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::build_context::BuildContext;
use crate::color_prop::ColorProp;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub enum TabBarOrientation {
    #[default]
    Horizontal,
    Vertical,
}

/// Which edge of a tab the active-tab highlight indicator hugs.
///
/// The position is expressed relative to the content pane, so it stays
/// meaningful in both orientations and under RTL:
///
/// - [`OuterEdge`](TabIndicatorPosition::OuterEdge) (default) — the edge
///   pointing *away* from the content: **top** for a horizontal bar,
///   **leading** for a vertical bar. The IntUI / browser-tab look.
/// - [`InnerEdge`](TabIndicatorPosition::InnerEdge) — the edge pointing
///   *toward* the content: **bottom** for a horizontal bar (the indicator
///   sits below the label), **trailing** for a vertical bar.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub enum TabIndicatorPosition {
    #[default]
    OuterEdge,
    InnerEdge,
}

#[derive(Clone, Debug)]
pub struct TabStyleConfig {
    pub label: WidgetId,
    pub leading: Option<WidgetId>,
    pub trailing: Option<WidgetId>,
    pub is_active: Signal<bool>,
    pub is_hovered: Signal<bool>,
    pub is_focused: Signal<bool>,
    pub is_disabled: Signal<bool>,
    pub orientation: TabBarOrientation,
    /// Which edge the active-tab highlight indicator hugs. See
    /// [`TabIndicatorPosition`].
    pub indicator_position: TabIndicatorPosition,
}

/// Inputs for the bar-level chrome — the surface the headers row,
/// pinned strip, scroll arrows, and slots all sit on.
#[derive(Clone, Debug)]
pub struct TabBarChromeConfig {
    /// The composed bar content (leading slot → pinned strip → scroll
    /// arrows → headers row → overflow dropdown → trailing slot). The
    /// style wraps this and returns the bar's root.
    pub content: WidgetId,
    pub orientation: TabBarOrientation,
    /// Whether to draw the 1 px separator along the content-pane edge
    /// (bottom for horizontal bars, trailing for vertical bars).
    pub show_separator: bool,
    /// Optional app-set backdrop fill spanning the whole bar. `None`
    /// leaves the bar transparent.
    pub surface_role: Option<ColorProp>,
    /// Drag-reorder drop-indicator position — layout-axis offset in
    /// bar-local coords. `None` when no reorder drag is in progress
    /// over the bar.
    pub drop_indicator: Signal<Option<f32>>,
}

pub trait TabStyle: 'static {
    /// Chrome for a single tab header.
    fn make_body(&self, cfg: &TabStyleConfig, ctx: &mut BuildContext) -> WidgetId;
    /// Chrome for the whole bar strip — wraps [`TabBarChromeConfig::content`].
    fn make_bar(&self, cfg: &TabBarChromeConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedTabStyle = Rc<dyn TabStyle>;
