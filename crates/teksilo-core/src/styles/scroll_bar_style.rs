// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `ScrollBar`. See `docs/styling-system.md`.
//!
//! One defaulted metrics accessor, [`ScrollBarStyle::min_thumb_length`], says
//! how short the thumb may get. The bar sizes its thumb, and decides whether a
//! press lands on it, from that one number, so a preset's floor is the one laid
//! out.

use std::rc::Rc;

use serde::{Deserialize, Serialize};
use teksilo_tokens::{InputTokens, TargetRole};

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::styles::density::dp;
use crate::widget_id::WidgetId;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub enum ScrollBarOrientation {
    #[default]
    Vertical,
    Horizontal,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default, Serialize, Deserialize)]
pub enum ScrollBarVariant {
    /// Full track + thumb, always visible. The classic always-on scroll
    /// bar; reserves layout space when used inside a parent that honours
    /// its full thickness. The widget's historical default.
    #[default]
    Permanent,
    /// Thin resting indicator at idle, cross-fades to the full track +
    /// thumb on hover or drag. macOS / Ubuntu / IntUI overlay style.
    Overlay,
    /// Thin resting indicator only — never reveals the full bar.
    /// A passive scroll-position display for minimal UIs; interaction
    /// (drag, track click, keyboard) still works against the full slot
    /// bounds even though only the thin strip is painted.
    Thin,
}

#[derive(Clone, Debug)]
pub struct ScrollBarStyleConfig {
    /// Normalized `0.0..=1.0` scroll position: `0.0` at the start of the
    /// content, `1.0` at the end. Re-renders the body on every scroll.
    pub scroll_ratio: Signal<f32>,
    /// Visible viewport as a fraction of total content
    /// (`viewport_size / content_size`). Drives the thumb size.
    pub viewport_ratio: Signal<f32>,
    /// `true` whenever the pointer is over the scroll bar's slot.
    pub is_hovered: Signal<bool>,
    /// `true` while the user is drag-pressing the thumb.
    pub is_dragging: Signal<bool>,
    /// `true` whenever the content is non-scrollable (`max_scroll == 0`).
    /// Default IntUI paints nothing in this case; custom impls may choose
    /// to keep a faint placeholder.
    pub is_idle: Signal<bool>,
    pub orientation: ScrollBarOrientation,
    pub variant: ScrollBarVariant,
    /// Minimum thumb length in logical pixels: the active style's
    /// [`ScrollBarStyle::min_thumb_length`] at the tree's density, unless the
    /// app named one per instance via `ScrollBar::min_thumb_length(...)`. The
    /// widget resolves it before `make_body` runs and sizes its own thumb
    /// geometry (hit test, drag, reported target) from the same value.
    pub min_thumb_length: f32,
    /// Optional thumb tint override (`ScrollBar::thumb_color` /
    /// `ScrollArea::scroll_bar_thumb_color`). `None` (the default) means the
    /// style paints from the theme's `scrollbar_thumb*` tokens. When set, the
    /// style tints the thumb from this `ColorProp` instead — resolved against
    /// the live theme at paint, so a role (e.g. `TextRole::TooltipText`) or a
    /// `Signal` stays reactive. Lets chrome on a non-standard surface — a
    /// tooltip's inverse chip, a branded panel — give the thumb a contrasting
    /// colour the surface-relative tokens can't. Mirrors `Button::text_role`.
    pub thumb_color: Option<crate::color_prop::ColorProp>,
}

pub trait ScrollBarStyle: 'static {
    /// Build the bar's chrome — track and thumb.
    ///
    /// A horizontal bar's `scroll_ratio == 0.0` is the **start** of the
    /// content, which sits at the right-hand edge in a right-to-left window:
    /// `ScrollArea` places its content that way, and the host widget mirrors
    /// its thumb hit-test and its drag there. A style that offsets the thumb
    /// from `bounds.x` unconditionally therefore paints it at the far end of
    /// the track while the content shows its beginning, and grabbing it jumps.
    /// Read `PaintContext::layout_direction` at paint time — not at build
    /// time, because a locale change repaints without rebuilding — and mirror
    /// the thumb's offset. The vertical orientation has no leading/trailing to
    /// mirror.
    ///
    /// The widget cannot enforce this, and getting it wrong is only visible in
    /// an RTL locale — the same obligation
    /// [`SliderStyle::make_body`](super::SliderStyle::make_body) carries.
    fn make_body(&self, cfg: &ScrollBarStyleConfig, ctx: &mut BuildContext) -> WidgetId;

    /// The shortest the thumb may get along the scroll axis, in logical
    /// pixels, at the density `tokens` describe. A long document's thumb is
    /// held at this length rather than shrinking with the viewport ratio, so it
    /// stays something a pointer can land on.
    ///
    /// A metrics accessor because the bar, not the chrome, owns the thumb's
    /// geometry: its hit test, its drag and the target it reports all need the
    /// floor outside `make_body`. The widget resolves it once per build and
    /// hands the answer to `make_body` as
    /// [`ScrollBarStyleConfig::min_thumb_length`]. A bar's own
    /// `min_thumb_length(..)` wins over it.
    ///
    /// **Defaulted**, so a `ScrollBarStyle` implemented outside this workspace
    /// keeps compiling. The default is
    /// `teksilo_widgets::styles::recipe_scroll_bar_style::SCROLLBAR_MIN_THUMB_LENGTH`
    /// put through [`dp`] as a target (24 at Compact, 32 at Comfortable, 44 at
    /// Touch), which is the shipped recipe's own answer at every density,
    /// restated as a literal because `teksilo-core` cannot name a
    /// `teksilo-widgets` constant, and pinned equal to it by
    /// `the_scroll_bar_trait_default_restates_the_module_constant`.
    fn min_thumb_length(&self, tokens: &InputTokens) -> f32 {
        dp(24.0, TargetRole::Target, tokens)
    }
}

pub type SharedScrollBarStyle = Rc<dyn ScrollBarStyle>;
