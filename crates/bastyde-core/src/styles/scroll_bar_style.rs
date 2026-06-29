// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `ScrollBar`. See `docs/styling-system.md`.

use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::build_context::BuildContext;
use crate::signal::Signal;
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
    /// Minimum thumb length in logical pixels. Sourced from
    /// `bastyde_widgets::styles::recipe_scroll_bar_style` constants by
    /// default; apps override per-instance via `ScrollBar::min_thumb_length(...)`.
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
    fn make_body(&self, cfg: &ScrollBarStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedScrollBarStyle = Rc<dyn ScrollBarStyle>;
