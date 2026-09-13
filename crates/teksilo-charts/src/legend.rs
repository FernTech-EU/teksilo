// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `ChartLegend` — series swatch + label list, model-backed.
//!
//! Use the standalone widget when you want the legend in a different
//! container than the chart (or with custom layout). Charts also embed
//! this internally when constructed with `.legend(true)`, sharing the
//! same [`teksilo_data::ChartModel`] and palette.
//!
//! `.interactive(true)` turns each row into a real focusable/clickable
//! `LegendRow` child (`Role::CheckBox`, click or Space toggles that
//! series' visibility via `ChartModel::set_series_visible`) — the
//! standard "click a legend entry to hide/show that series" affordance.
//! Non-interactive legends (the default) self-paint their rows directly,
//! same as before this widget grew model-backed interactivity.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::accesskit::{Action, Role};
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::signal::Prop;
use teksilo_core::widget::{
    CursorIcon, EventContext, LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement,
};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_data::{ChartModel, SeriesId};
use teksilo_tokens::{InputTokens, TargetDensity, TextRole, TextStyleRole};

use crate::palette::ChartPalette;
use crate::pattern::{self, LegendSwatch, PatternPolicy};
use crate::text::{measure_text_width, measure_text_width_via};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegendOrientation {
    Horizontal,
    Vertical,
}

/// One interactive legend row: a swatch + the series' name, clickable
/// (and Space-toggleable when focused) to flip that series' visibility.
/// Private — only reachable through `ChartLegend::interactive(true)`.
struct LegendRow<T: Clone + 'static> {
    model: ChartModel<T>,
    series_id: SeriesId,
    palette_index: usize,
    palette: Prop<ChartPalette>,
    /// How this row samples its series — must match what the plot draws, or
    /// the legend hands the reader a second mapping to learn.
    swatch: LegendSwatch,
    /// Whether the non-colour channel is drawn at all. Decided once by the
    /// owning `ChartLegend` from its policy and the visible-series count, so
    /// every row agrees.
    patterned: bool,
}

impl<T: Clone + 'static> std::fmt::Debug for LegendRow<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LegendRow")
            .field("palette_index", &self.palette_index)
            .finish()
    }
}

impl<T: Clone + 'static> Widget for LegendRow<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        {
            let registry = ctx.binding_registry();
            self.model
                .style_version()
                .bind_to(id, registry, BindingLevel::RepaintOnly);
            self.model
                .structure_version()
                .bind_to(id, registry, BindingLevel::RepaintOnly);
            self.model
                .structure_version()
                .bind_to(id, registry, BindingLevel::AccessibilityOnly);
            self.palette
                .register_if_bound(id, registry, BindingLevel::RepaintOnly);
        }

        let model_tap = self.model.clone();
        let series_id_tap = self.series_id;
        let model_key = self.model.clone();
        let series_id_key = self.series_id;
        // KeyDown-arm / KeyUp-fire dance (mirrors Checkbox): a lone KeyUp
        // with no matching KeyDown (e.g. a shortcut consumed the KeyDown)
        // must not toggle.
        let armed = Rc::new(Cell::new(false));
        let armed_key = armed.clone();

        let handlers = HandlerSet::new()
            .on_tap(move |_event, _ctx: &mut EventContext| {
                let current = model_tap
                    .with_series(series_id_tap, |_, _, visible| visible)
                    .unwrap_or(true);
                model_tap.set_series_visible(series_id_tap, !current);
            })
            .on_key(
                move |event: &WidgetEvent, _ctx: &mut EventContext| -> EventResponse {
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::Space, ..
                        } => {
                            armed_key.set(true);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyUp {
                            key: Key::Space, ..
                        } => {
                            if !armed_key.get() {
                                return EventResponse::Ignored;
                            }
                            armed_key.set(false);
                            let current = model_key
                                .with_series(series_id_key, |_, _, visible| visible)
                                .unwrap_or(true);
                            model_key.set_series_visible(series_id_key, !current);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                },
            )
            .focusable(true)
            .cursor(CursorIcon::Pointer);
        ctx.apply_self_handlers(handlers);

        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        use crate::style as cs;
        let label_style = TextStyleRole::Tiny.resolve(&ctx.theme.typography);
        let name = self
            .model
            .with_series(self.series_id, |name, _, _| name.to_string())
            .unwrap_or_default();
        let label_w = measure_text_width_via(ctx.text_backend, &name, &label_style);
        let w = cs::LEGEND_SWATCH_SIZE + 4.0 + label_w;
        // A `LegendRow` only exists inside an interactive legend, so it
        // always asks for the interactive extent.
        let h = legend_row_extent(&label_style, true, &ctx.theme.input);
        Size::new(proposal.width.unwrap_or(w), proposal.height.unwrap_or(h)).into()
    }

    fn place_children(
        &self,
        _bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        use crate::style as cs;
        let theme = ctx.theme;
        let enabled = ctx.effective_enabled;
        let Some((name, color_prop, series_pattern, visible)) =
            self.model.with_series_view(self.series_id, |v| {
                (v.name.to_string(), v.color.cloned(), v.pattern, v.visible)
            })
        else {
            return;
        };
        let palette = self.palette.get();
        let color = color_prop
            .as_ref()
            .map(|c| c.resolve(theme, enabled))
            .unwrap_or_else(|| palette.color_for(self.palette_index, theme));
        let final_color = if visible {
            color
        } else {
            color.with_alpha(0.35)
        };
        let label_style = TextStyleRole::Tiny.resolve(&theme.typography);
        let label_color = TextRole::Primary.resolve(&theme.colors);
        let line_height = legend_painted_line_height(&label_style);
        // Centred in the room the row was GIVEN, not in the room its
        // content needs: at a density that raises the row to a 44 dp
        // target, content centred on the painted line height would hug the
        // top of a target twice its size.
        let center_y = bounds.y + bounds.height.max(line_height) * 0.5;

        let swatch = Rect::new(
            bounds.x,
            center_y - cs::LEGEND_SWATCH_SIZE * 0.5,
            cs::LEGEND_SWATCH_SIZE,
            cs::LEGEND_SWATCH_SIZE,
        );
        pattern::draw_legend_swatch(
            canvas,
            swatch,
            self.swatch,
            pattern::resolve(series_pattern, self.palette_index),
            final_color,
            self.patterned,
        );

        let label_w = measure_text_width(canvas, &name, &label_style);
        let text_color = if visible {
            label_color
        } else {
            label_color.with_alpha(0.5)
        };
        canvas.draw_text(
            &name,
            Rect::new(
                bounds.x + cs::LEGEND_SWATCH_SIZE + 4.0,
                center_y - label_style.size * 0.6,
                label_w,
                label_style.size * 1.2,
            ),
            &label_style,
            text_color,
        );
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(Role::CheckBox);
        if let Some((name, visible)) = self.model.with_series(self.series_id, |name, _, visible| {
            (name.to_string(), visible)
        }) {
            builder.set_name(name);
            builder.set_toggled(visible);
        }
        builder.add_action(Action::Focus);
    }
}

/// The extent one legend row occupies along the axis its rows stack on —
/// the height of a horizontal legend's strip, the pitch of a vertical
/// one's rows.
///
/// A row's **painted** content is a 10 dp swatch beside an 11 pt label, so
/// it draws about 13 dp tall, and it draws that at every density: this
/// function never changes what a row looks like, only how much room it is
/// given.
///
/// An **interactive** row is a real target — `Role::CheckBox`, focusable,
/// tap-toggles its series — and 13 dp is under the 24 dp WCAG 2.2 SC 2.5.8
/// floor. Neither of the framework's two hit-widening mechanisms can lift
/// it where it sits:
///
/// * `Widget::hit_outset` never escapes its parent's rectangle, and the
///   parent here is a legend band reserved at exactly one row's extent, so
///   vertically there is no space for an outset to claim. In a vertical
///   legend the rows are contiguous besides, so an outset could only take
///   space from the row above or below it.
/// * The miss-only slop pass is short-circuited before it runs: it yields
///   only when the exact hit's bubble path carries no eligible handler or
///   the candidate is strictly closer than the bubble owner, and a chart
///   with a readout — the default — installs a pointer handler, so a press
///   anywhere inside it already has an owner at distance zero. (A chart
///   built with `hover_tooltip(false)` and no selection installs none, so
///   the pass could serve its legend rows; a legend whose conformance
///   depended on the enclosing chart being inert would be a worse contract
///   than one that gives the row room.)
///
/// So the only mechanism left is giving the row and the band it sits in
/// more room together, and that is a **layout** change. At
/// [`TargetDensity::Compact`] the programme's invariant is that layout is
/// byte-identical, so Compact keeps the 13 dp row and its shortfall is
/// recorded in `docs/charts.md` rather than silently fixed; Comfortable
/// and Touch raise the row to the density's `target_size` (32 / 44 dp).
/// A non-interactive legend has no target at all and is never raised, at
/// any density.
///
/// Three sites must agree on this number or the legend breaks visibly:
/// [`ChartLegend::layout_response`] (what the legend asks for),
/// [`ChartLegend::place_children`] (what each row is given), and
/// [`legend_main_axis_size`] (what the embedding chart reserves). They all
/// call here.
pub(crate) fn legend_row_extent(
    label_style: &teksilo_tokens::TextStyle,
    interactive: bool,
    tokens: &InputTokens,
) -> f32 {
    let painted = legend_painted_line_height(label_style);
    if !interactive || tokens.density == TargetDensity::Compact {
        painted
    } else {
        painted.max(tokens.target_size)
    }
}

/// What a legend row paints, independent of the room it is given.
pub(crate) fn legend_painted_line_height(label_style: &teksilo_tokens::TextStyle) -> f32 {
    crate::style::LEGEND_SWATCH_SIZE.max(label_style.size * 1.2)
}

/// Series swatch + label list. Bound to a [`ChartModel`] shared with the
/// chart that embeds it (or a standalone one for a detached legend).
pub struct ChartLegend<T: Clone + 'static> {
    model: ChartModel<T>,
    palette: Prop<ChartPalette>,
    orientation: LegendOrientation,
    interactive: bool,
    /// How each row samples its series. Set by the owning chart to match what
    /// its plot draws — a hatched chip for bars, a dashed line + marker for
    /// lines. A swatch that does not match the plot is a second mapping for
    /// the reader to learn, which defeats the point.
    swatch: LegendSwatch,
    /// Whether rows draw the non-colour channel. See [`PatternPolicy`].
    pattern_policy: PatternPolicy,
    row_ids: Vec<WidgetId>,
}

impl<T: Clone + 'static> ChartLegend<T> {
    pub fn new(model: ChartModel<T>) -> Self {
        Self {
            model,
            palette: Prop::Static(ChartPalette::FromTheme),
            orientation: LegendOrientation::Horizontal,
            interactive: false,
            swatch: LegendSwatch::Block,
            pattern_policy: PatternPolicy::default(),
            row_ids: Vec::new(),
        }
    }

    /// How each row samples its series — a filled chip carrying the hatch
    /// ([`LegendSwatch::Block`], the default, matching bars and pie slices) or
    /// a line sample carrying the dash and marker
    /// ([`LegendSwatch::Line`], matching a line series).
    pub fn swatch(mut self, swatch: LegendSwatch) -> Self {
        self.swatch = swatch;
        self
    }

    /// Whether rows carry the series' non-colour channel. Mirror the owning
    /// chart's own [`PatternPolicy`] so plot and legend agree; a legend that
    /// hatches what the plot leaves plain is worse than one that does neither.
    pub fn pattern_policy(mut self, policy: PatternPolicy) -> Self {
        self.pattern_policy = policy;
        self
    }

    /// Whether the non-colour channel applies given how many series are
    /// currently visible.
    fn patterned(&self) -> bool {
        let visible = self
            .model
            .with_all_series(|views| views.iter().filter(|v| v.visible).count());
        self.pattern_policy.applies(visible)
    }

    pub fn palette(mut self, p: impl Into<Prop<ChartPalette>>) -> Self {
        self.palette = p.into();
        self
    }

    pub fn orientation(mut self, o: LegendOrientation) -> Self {
        self.orientation = o;
        self
    }

    /// When `true`, each row becomes a real focusable/clickable
    /// `LegendRow` (`Role::CheckBox`) that toggles its series'
    /// visibility on click or Space. Default `false` (rows self-paint,
    /// no interaction).
    pub fn interactive(mut self, on: bool) -> Self {
        self.interactive = on;
        self
    }
}

impl<T: Clone + 'static> std::fmt::Debug for ChartLegend<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChartLegend")
            .field("orientation", &self.orientation)
            .field("interactive", &self.interactive)
            .finish()
    }
}

impl<T: Clone + 'static> ChartLegend<T> {
    fn item_widths(
        &self,
        backend: Option<&std::rc::Rc<std::cell::RefCell<dyn teksilo_canvas::TextBackend>>>,
        label_style: &teksilo_tokens::TextStyle,
    ) -> Vec<f32> {
        use crate::style as cs;
        self.model.with_all_series(|views| {
            views
                .iter()
                .map(|v| {
                    cs::LEGEND_SWATCH_SIZE
                        + 4.0
                        + measure_text_width_via(backend, v.name, label_style)
                })
                .collect()
        })
    }
}

impl<T: Clone + 'static> Widget for ChartLegend<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        {
            let registry = ctx.binding_registry();
            self.model
                .structure_version()
                .bind_to(id, registry, BindingLevel::Relayout);
            self.model
                .style_version()
                .bind_to(id, registry, BindingLevel::RepaintOnly);
            self.palette
                .register_if_bound(id, registry, BindingLevel::RepaintOnly);
        }

        if self.interactive {
            let patterned = self.patterned();
            let series_ids = self.model.series_ids();
            let mut row_ids = Vec::with_capacity(series_ids.len());
            for (i, sid) in series_ids.into_iter().enumerate() {
                let row = LegendRow {
                    model: self.model.clone(),
                    series_id: sid,
                    palette_index: i,
                    palette: self.palette.clone(),
                    swatch: self.swatch,
                    patterned,
                };
                row_ids.push(ctx.add(row));
            }
            self.row_ids = row_ids.clone();
            row_ids
        } else {
            self.row_ids.clear();
            Vec::new()
        }
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        use crate::style as cs;
        if self.model.series_count() == 0 {
            return Size::ZERO.into();
        }
        let label_style = TextStyleRole::Tiny.resolve(&ctx.theme.typography);
        let item_widths = self.item_widths(ctx.text_backend, &label_style);
        let line_height = legend_row_extent(&label_style, self.interactive, &ctx.theme.input);

        match self.orientation {
            LegendOrientation::Horizontal => {
                let total_w: f32 = item_widths.iter().sum::<f32>()
                    + cs::LEGEND_ITEM_GAP * (item_widths.len() as f32 - 1.0).max(0.0);
                Size::new(
                    proposal.width.unwrap_or(total_w).min(total_w.max(0.0)),
                    proposal.height.unwrap_or(line_height),
                )
            }
            LegendOrientation::Vertical => {
                let max_w = item_widths.iter().copied().fold(0.0_f32, f32::max);
                let total_h = line_height * item_widths.len() as f32;
                Size::new(
                    proposal.width.unwrap_or(max_w),
                    proposal.height.unwrap_or(total_h),
                )
            }
        }
        .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        if children.is_empty() {
            return;
        }
        use crate::style as cs;
        let label_style = TextStyleRole::Tiny.resolve(&ctx.theme.typography);
        let item_widths = self.item_widths(ctx.text_backend, &label_style);
        let line_height = legend_row_extent(&label_style, self.interactive, &ctx.theme.input);

        match self.orientation {
            LegendOrientation::Horizontal => {
                let total_w: f32 = item_widths.iter().sum::<f32>()
                    + cs::LEGEND_ITEM_GAP * (item_widths.len() as f32 - 1.0).max(0.0);
                let mut x = bounds.x + (bounds.width - total_w) * 0.5;
                for (child, &w) in children.iter_mut().zip(item_widths.iter()) {
                    child.origin = Point::new(x, bounds.y);
                    child.size = Size::new(w, line_height.max(bounds.height));
                    x += w + cs::LEGEND_ITEM_GAP;
                }
            }
            LegendOrientation::Vertical => {
                for (i, (child, &w)) in children.iter_mut().zip(item_widths.iter()).enumerate() {
                    let row_y = bounds.y + i as f32 * line_height;
                    child.origin = Point::new(bounds.x, row_y);
                    child.size = Size::new(w.max(bounds.width), line_height);
                }
            }
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        if self.interactive {
            // Rows self-paint (they're real children).
            return;
        }
        use crate::style as cs;
        let theme = ctx.theme;
        let enabled = ctx.effective_enabled;
        let palette = self.palette.get();
        let label_style = TextStyleRole::Tiny.resolve(&theme.typography);
        let label_color = TextRole::Primary.resolve(&theme.colors);
        let line_height = cs::LEGEND_SWATCH_SIZE.max(label_style.size * 1.2);
        let orientation = self.orientation;
        let patterned = self.patterned();
        let swatch_style = self.swatch;

        self.model.with_all_series(|views| {
            if views.is_empty() {
                return;
            }
            match orientation {
                LegendOrientation::Horizontal => {
                    let label_widths: Vec<f32> = views
                        .iter()
                        .map(|v| measure_text_width(canvas, v.name, &label_style))
                        .collect();
                    let item_widths: Vec<f32> = label_widths
                        .iter()
                        .map(|w| cs::LEGEND_SWATCH_SIZE + 4.0 + w)
                        .collect();
                    let total_w: f32 = item_widths.iter().sum::<f32>()
                        + cs::LEGEND_ITEM_GAP * (item_widths.len() as f32 - 1.0).max(0.0);
                    let mut x = bounds.x + (bounds.width - total_w) * 0.5;
                    let center_y = bounds.y + line_height * 0.5;
                    for (i, view) in views.iter().enumerate() {
                        let color = view
                            .color
                            .map(|c| c.resolve(theme, enabled))
                            .unwrap_or_else(|| palette.color_for(i, theme));
                        let final_color = if view.visible {
                            color
                        } else {
                            color.with_alpha(0.35)
                        };
                        let swatch = Rect::new(
                            x,
                            center_y - cs::LEGEND_SWATCH_SIZE * 0.5,
                            cs::LEGEND_SWATCH_SIZE,
                            cs::LEGEND_SWATCH_SIZE,
                        );
                        pattern::draw_legend_swatch(
                            canvas,
                            swatch,
                            swatch_style,
                            pattern::resolve(view.pattern, i),
                            final_color,
                            patterned,
                        );
                        x += cs::LEGEND_SWATCH_SIZE + 4.0;
                        let label_w = label_widths[i];
                        let text_color = if view.visible {
                            label_color
                        } else {
                            label_color.with_alpha(0.5)
                        };
                        canvas.draw_text(
                            view.name,
                            Rect::new(
                                x,
                                center_y - label_style.size * 0.6,
                                label_w,
                                label_style.size * 1.2,
                            ),
                            &label_style,
                            text_color,
                        );
                        x += label_w + cs::LEGEND_ITEM_GAP;
                    }
                }
                LegendOrientation::Vertical => {
                    for (i, view) in views.iter().enumerate() {
                        let color = view
                            .color
                            .map(|c| c.resolve(theme, enabled))
                            .unwrap_or_else(|| palette.color_for(i, theme));
                        let final_color = if view.visible {
                            color
                        } else {
                            color.with_alpha(0.35)
                        };
                        let row_y = bounds.y + i as f32 * line_height;
                        let center_y = row_y + line_height * 0.5;
                        let swatch = Rect::new(
                            bounds.x,
                            center_y - cs::LEGEND_SWATCH_SIZE * 0.5,
                            cs::LEGEND_SWATCH_SIZE,
                            cs::LEGEND_SWATCH_SIZE,
                        );
                        pattern::draw_legend_swatch(
                            canvas,
                            swatch,
                            swatch_style,
                            pattern::resolve(view.pattern, i),
                            final_color,
                            patterned,
                        );
                        let label_w = measure_text_width(canvas, view.name, &label_style);
                        let text_color = if view.visible {
                            label_color
                        } else {
                            label_color.with_alpha(0.5)
                        };
                        canvas.draw_text(
                            view.name,
                            Rect::new(
                                bounds.x + cs::LEGEND_SWATCH_SIZE + 4.0,
                                center_y - label_style.size * 0.6,
                                label_w,
                                label_style.size * 1.2,
                            ),
                            &label_style,
                            text_color,
                        );
                    }
                }
            }
        });
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(Role::List);
        builder.set_name("Chart legend");
    }

    fn children(&self) -> Vec<WidgetId> {
        self.row_ids.clone()
    }
}

/// Compute the size the legend would occupy along its main axis (used by
/// charts to reserve space for an embedded legend). For Horizontal returns
/// height; for Vertical returns width.
///
/// `backend` is the live text backend if one is wired (canvas at paint
/// time, layout context at layout time) — `None` falls back to the same
/// `chars * size * 0.7 + 4` heuristic used by `measure_text_width` so
/// reservation matches what the painter will later request.
///
/// `interactive` and `tokens` are what [`legend_row_extent`] needs: an
/// interactive legend's rows are targets and may be given more room than
/// they paint, and the band has to be reserved at the same extent the rows
/// will be placed at or they are clipped by their own container.
pub(crate) fn legend_main_axis_size<T: Clone + 'static>(
    backend: Option<&std::rc::Rc<std::cell::RefCell<dyn teksilo_canvas::TextBackend>>>,
    model: &ChartModel<T>,
    label_style: &teksilo_tokens::TextStyle,
    orientation: LegendOrientation,
    interactive: bool,
    tokens: &InputTokens,
) -> f32 {
    use crate::style as cs;
    let line_height = legend_row_extent(label_style, interactive, tokens);
    match orientation {
        LegendOrientation::Horizontal => line_height,
        LegendOrientation::Vertical => {
            let max_w = model.with_all_series(|views| {
                views
                    .iter()
                    .map(|v| measure_text_width_via(backend, v.name, label_style))
                    .fold(0.0_f32, f32::max)
            });
            cs::LEGEND_SWATCH_SIZE + 4.0 + max_w
        }
    }
}

/// Pick the appropriate orientation for an embedded legend given its
/// position around the plot.
pub(crate) fn orientation_for_position(pos: crate::layout::LegendPosition) -> LegendOrientation {
    use crate::layout::LegendPosition;
    match pos {
        LegendPosition::Top | LegendPosition::Bottom => LegendOrientation::Horizontal,
        LegendPosition::Leading | LegendPosition::Trailing => LegendOrientation::Vertical,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_data::ChartSeries;

    fn three_series() -> ChartModel<String> {
        ChartModel::from_series_vec(vec![
            ChartSeries::<String>::new("A"),
            ChartSeries::<String>::new("B"),
            ChartSeries::<String>::new("C"),
        ])
    }

    // ── Interactive rows as targets ───────────────────────────────────────
    //
    // An interactive row is a real target — `Role::CheckBox`, focusable,
    // tap-toggles its series — and paints about 13 dp tall, under the 24 dp
    // WCAG 2.2 SC 2.5.8 floor. See `legend_row_extent` for why neither of the
    // framework's hit-widening mechanisms can reach it and why Compact keeps
    // the shortfall.

    fn row_height(interactive: bool, density: TargetDensity) -> f32 {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.set_input_density(density);
        let id = tree.add(ChartLegend::new(three_series()).interactive(interactive));
        // Unspecified, so the legend reports its own extent rather than
        // filling a band a test invented for it.
        tree.layout(SizeProposal::unspecified());
        if interactive {
            let row = tree.children(id)[0];
            tree.bounds(row).height
        } else {
            tree.bounds(id).height
        }
    }

    #[test]
    fn a_compact_interactive_row_is_exactly_the_size_it_paints() {
        let h = row_height(true, TargetDensity::Compact);
        let painted = legend_painted_line_height(
            &TextStyleRole::Tiny.resolve(&teksilo_core::presets::intui::light().typography),
        );
        assert!(
            (h - painted).abs() < 0.01,
            "Compact layout is byte-identical by invariant: {h} vs {painted}"
        );
        assert!(
            h < 24.0,
            "and that is a recorded 2.5.8 shortfall, not a passing target"
        );
    }

    #[test]
    fn an_interactive_row_reaches_the_target_floor_where_the_density_asks() {
        for density in [TargetDensity::Comfortable, TargetDensity::Touch] {
            let want = InputTokens::for_density(density).target_size;
            let h = row_height(true, density);
            assert!(
                h >= want,
                "{density:?}: an interactive legend row must reach {want} dp, got {h}"
            );
        }
    }

    #[test]
    fn a_non_interactive_row_never_grows_at_any_density() {
        let painted = legend_painted_line_height(
            &TextStyleRole::Tiny.resolve(&teksilo_core::presets::intui::light().typography),
        );
        for density in [
            TargetDensity::Compact,
            TargetDensity::Comfortable,
            TargetDensity::Touch,
        ] {
            let h = row_height(false, density);
            assert!(
                (h - painted).abs() < 0.01,
                "{density:?}: a legend with no targets has nothing to grow, got {h}"
            );
        }
    }

    #[test]
    fn the_legends_own_band_grows_with_the_rows_it_places() {
        // Three sites have to agree or the rows are clipped by their own
        // container: what the legend asks for, what it gives each row, and
        // what an embedding chart reserves. This pins the first two.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.set_input_density(TargetDensity::Touch);
        let id = tree.add(ChartLegend::new(three_series()).interactive(true));
        tree.layout(SizeProposal::unspecified());
        let band = tree.bounds(id).height;
        let row = tree.bounds(tree.children(id)[0]).height;
        assert!(
            band >= row,
            "a band shorter than its rows clips them: band {band}, row {row}"
        );
        assert!(row >= InputTokens::for_density(TargetDensity::Touch).target_size);
    }

    #[test]
    fn a_vertical_interactive_legend_stacks_at_the_grown_pitch() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.set_input_density(TargetDensity::Touch);
        let id = tree.add(
            ChartLegend::new(three_series())
                .orientation(LegendOrientation::Vertical)
                .interactive(true),
        );
        tree.layout(SizeProposal::exact(200.0, 400.0));
        let kids = tree.children(id);
        let first = tree.bounds(kids[0]);
        let second = tree.bounds(kids[1]);
        assert!(
            second.y - first.y >= InputTokens::for_density(TargetDensity::Touch).target_size,
            "contiguous rows must not overlap once they are targets: \
             {first:?} then {second:?}"
        );
    }

    #[test]
    fn one_swatch_per_series() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.add(ChartLegend::new(three_series()));
        tree.layout(SizeProposal::exact(300.0, 30.0));
        let frame = tree.render();
        // 3 swatches as rounded rects (Tier 2 shapes).
        assert_eq!(frame.shapes.len(), 3, "expected 3 swatch shapes");
    }

    #[test]
    fn empty_legend_has_zero_size() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(ChartLegend::new(ChartModel::<String>::new()));
        tree.layout(SizeProposal::unspecified());
        let b = tree.bounds(id);
        assert_eq!(b.width, 0.0);
        assert_eq!(b.height, 0.0);
    }

    #[test]
    fn vertical_orientation_changes_height() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id =
            tree.add(ChartLegend::new(three_series()).orientation(LegendOrientation::Vertical));
        tree.layout(SizeProposal::unspecified());
        let b = tree.bounds(id);
        // Vertical has 3 rows of swatches → height ≥ 3 * swatch size.
        assert!(
            b.height >= 30.0,
            "vertical should stack rows: got {}",
            b.height
        );
    }

    #[test]
    fn interactive_row_tap_toggles_visibility() {
        let model = three_series();
        let sid = model.series_id_at(0).unwrap();
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(ChartLegend::new(model.clone()).interactive(true));
        tree.layout(SizeProposal::exact(300.0, 30.0));

        assert!(model.with_series(sid, |_, _, v| v).unwrap());
        let row_id = tree.children(id)[0];
        tree.click(row_id);
        assert!(!model.with_series(sid, |_, _, v| v).unwrap());
    }

    #[test]
    fn interactive_row_space_toggles_visibility() {
        let model = three_series();
        let sid = model.series_id_at(0).unwrap();
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(ChartLegend::new(model.clone()).interactive(true));
        tree.layout(SizeProposal::exact(300.0, 30.0));

        let row_id = tree.children(id)[0];
        tree.focus(row_id);
        tree.press_key(
            teksilo_core::event::Key::Space,
            teksilo_core::event::Modifiers::NONE,
        );
        assert!(!model.with_series(sid, |_, _, v| v).unwrap());
    }

    #[test]
    fn interactive_row_accessibility_is_checkbox() {
        let model = three_series();
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(ChartLegend::new(model).interactive(true));
        tree.layout(SizeProposal::exact(300.0, 30.0));
        let row_id = tree.children(id)[0];
        let info = tree.accessibility_node(row_id);
        assert_eq!(info.role(), Role::CheckBox);
        assert!(info.is_toggled());
    }

    #[test]
    fn non_interactive_legend_has_no_children() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(ChartLegend::new(three_series()));
        tree.layout(SizeProposal::exact(300.0, 30.0));
        assert!(tree.children(id).is_empty());
    }
}
