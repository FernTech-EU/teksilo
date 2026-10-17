// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `TabBar`. See `docs/styling-system.md`.
//!
//! `TabBar` has two themable surfaces, so the trait carries two
//! methods: [`TabStyle::make_body`] wraps a single tab header (accent
//! indicator, focus ring, …) and [`TabStyle::make_bar`] wraps the
//! whole strip (backdrop fill, content-pane separator, drag-reorder
//! drop indicator). A custom `impl TabStyle` provides both.
//!
//! Two defaulted metrics accessors, [`TabStyle::editor_height`] and
//! [`TabStyle::padding_horizontal`], say how tall a tab header is and how far
//! its content sits from its leading and trailing edges. The bar and its
//! headers measure themselves from them, so a preset's tab geometry is the one
//! laid out.

use std::rc::Rc;

use serde::{Deserialize, Serialize};
use teksilo_tokens::{InputTokens, TargetRole};

use crate::build_context::BuildContext;
use crate::color_prop::ColorProp;
use crate::signal::Signal;
use crate::styles::density::{dp, spacing};
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

    /// The outer height of one tab header, in logical pixels, at the density
    /// `tokens` describe: the strip height of a horizontal bar, and the pill
    /// height of a vertical one. The focus-ring envelope is reserved inside it,
    /// never added on top.
    ///
    /// A metrics accessor because the bar sizes its scroll viewport and its
    /// header row from one number, and each header measures itself in
    /// `layout_response`, where the chrome `make_body` built is out of reach.
    /// Without it a preset's tab height is written and never rendered. A bar's
    /// own `tab_bar_height(..)` wins over it.
    ///
    /// **Defaulted**, so a `TabStyle` implemented outside this workspace keeps
    /// compiling. The default is
    /// `teksilo_widgets::styles::recipe_tab_style::TAB_EDITOR_HEIGHT` put
    /// through [`dp`] as a target, which is the shipped recipe's own answer at
    /// every density, restated as a literal because `teksilo-core` cannot name
    /// a `teksilo-widgets` constant, and pinned equal to it by
    /// `the_tab_trait_defaults_restate_the_module_constants`.
    fn editor_height(&self, tokens: &InputTokens) -> f32 {
        dp(50.0, TargetRole::Target, tokens)
    }

    /// The gap between a tab header's edges and its content (icon, label,
    /// slots, close button) along the bar's main axis, in logical pixels, at
    /// the density `tokens` describe. It counts twice in a header's natural
    /// width. A pinned tab, which centres its icon in a fixed width, ignores
    /// it.
    ///
    /// Defaulted on the same terms as [`editor_height`](Self::editor_height),
    /// from `teksilo_widgets::styles::recipe_tab_style::TAB_PADDING_HORIZONTAL`
    /// put through [`spacing`].
    fn padding_horizontal(&self, tokens: &InputTokens) -> f32 {
        spacing(12.0, tokens)
    }
}

pub type SharedTabStyle = Rc<dyn TabStyle>;
