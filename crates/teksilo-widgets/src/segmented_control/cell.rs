// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! One segment's composed content: a centred icon + label with a
//! reactive tint, owning that segment's click / hover / tooltip / a11y.
//!
//! Paints no chrome — the control's style leaf paints the frame,
//! selection and hover backgrounds behind these cells.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::EventResponse;
use teksilo_core::focus::FocusOrigin;
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::widget::{
    CursorIcon, EventContext, LayoutContext, LayoutResponse, Widget, WidgetPlacement,
};
use teksilo_core::widget_builder::{HandlerSet, WidgetBuilder};
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::LocalizedString;
use teksilo_tokens::{TextRole, TextStyleRole};

use super::{IconFactory, SEGMENT_ICON_LABEL_SPACING, SegmentDisplay};
use crate::primitives::{HStack, TextWidget};
use crate::styles::recipe_segmented_control_style::SEGMENTED_CONTROL_PADDING_HORIZONTAL;

pub(crate) struct SegmentCell {
    pub(crate) label: LocalizedString,
    pub(crate) icon: Option<IconFactory>,
    pub(crate) tooltip: Option<LocalizedString>,
    pub(crate) rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    pub(crate) composite_tooltip_factory: Option<Rc<dyn Fn() -> Box<dyn Widget>>>,
    pub(crate) label_style: Option<teksilo_core::color_prop::TextStyleProp>,
    pub(crate) display: SegmentDisplay,
    /// Reactive per-segment disabled flag. Forwarded to the arena
    /// unconditionally so a bound signal flips the cell live.
    pub(crate) disabled: Prop<bool>,
    /// Position in the control's *live* segment list (segments whose
    /// `visible` prop is currently true), which is also what the private
    /// index mirror holds.
    pub(crate) index: usize,
    /// Number of live segments, for `size_of_set`.
    pub(crate) live_count: usize,
    /// The control's private index mirror — the single write target for
    /// every internal interactive path.
    pub(crate) selected: Signal<usize>,
    pub(crate) hovered_segment: Signal<Option<usize>>,
    pub(crate) focus_origin: Signal<Option<FocusOrigin>>,
    /// Sibling cell ids, shared with the control, for
    /// `push_to_radio_group`. Only *active* cells are listed — a dormant
    /// (overflowed) cell emits no AccessKit node, so referencing it would
    /// dangle.
    pub(crate) group_ids: Rc<RefCell<Vec<WidgetId>>>,
    /// The control's single selection funnel: writes the index mirror and
    /// fires `on_change` with the live `EventContext`.
    pub(crate) select: Rc<dyn Fn(usize, &mut EventContext)>,
    pub(crate) content_id: Option<WidgetId>,
}

impl std::fmt::Debug for SegmentCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SegmentCell")
            .field("index", &self.index)
            .field("disabled", &self.disabled.get())
            .finish()
    }
}

impl SegmentCell {
    /// Whether this cell paints its icon, given the control's display
    /// mode. `Text` suppresses it; every other mode shows it when one
    /// was declared.
    fn shows_icon(&self) -> bool {
        self.icon.is_some() && self.display != SegmentDisplay::Text
    }

    /// Whether this cell paints its label. `Icon` suppresses it — but
    /// only for a segment that actually *has* an icon, so the mode is
    /// never a silent no-op that renders an empty box.
    fn shows_label(&self) -> bool {
        self.display != SegmentDisplay::Icon || self.icon.is_none()
    }
}

impl Widget for SegmentCell {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        // Forward the (possibly reactive) per-segment disabled flag to the
        // arena: clicks and hovers are gated and the AT node is announced
        // disabled automatically by the framework walker. Unconditional —
        // a `Prop::Bound(false)` must still be registered so a later flip
        // to `true` disables the cell without a rebuild.
        ctx.enabled_when(self_id, super::overflow::not(&self.disabled));
        let enabled = ctx.effective_enabled_signal(self_id);

        // Label / icon tint follows (selected, focus, enabled):
        //   !enabled            -> Disabled
        //   selected + focused  -> OnAccent (the chrome fills it accent)
        //   otherwise           -> Primary
        // `Signal<TextRole>` resolves against the live theme at paint, so
        // this is theme-reactive too.
        let idx = self.index;
        let color = self
            .selected
            .zip3(&self.focus_origin, &enabled)
            .map(move |(sel, foc, en)| {
                if !*en {
                    TextRole::Disabled
                } else if *sel == idx && foc.is_some() {
                    TextRole::OnAccent
                } else {
                    TextRole::Primary
                }
            });

        // Borrow (don't consume) the icon factory / tooltip so the cell
        // stays rebuild-safe.
        let mut row = HStack::new().spacing(SEGMENT_ICON_LABEL_SPACING);
        if self.shows_icon() {
            let icon_factory = self.icon.as_ref().expect("shows_icon checked is_some");
            row = row.child(icon_factory().color(color.clone()));
        }
        if self.shows_label() {
            let label_widget = match &self.label_style {
                Some(style) => TextWidget::new(self.label.clone()).style(style.clone()),
                None => TextWidget::new(self.label.clone()).style(TextStyleRole::Small),
            };
            row = row.child(label_widget.color(color).single_line());
        }

        // The cell node owns the AT RadioButton + name; exclude the inner
        // content subtree so a screen reader doesn't double-announce the
        // label (an `access_hidden` flag alone would not prune the
        // descendant `TextWidget`/icon nodes).
        //
        // The row is added *directly* (not wrapped in a `Center`): this cell's
        // `place_children` measures it at the cell's bounded width and centres
        // the result, so a `single_line` label truncates with an ellipsis to
        // fit a narrow cell. A `Center` here would measure the row with an
        // *unbounded* width — the label would then never see a `max_width`, so
        // it could not ellipsize and would spill a few px past the (correctly
        // sized) control whenever the segments are compressed below their
        // label width.
        let content_id = ctx.add(row.access_exclude_subtree());
        self.content_id = Some(content_id);

        // Optional hover tooltip. Composite > rich > plain, mutually
        // exclusive — last setter wins. In `Icon` display mode the label
        // is not painted, so it becomes the tooltip unless the segment
        // declared one of its own (the `TabDisplayMode` convention).
        if let Some(factory) = &self.composite_tooltip_factory {
            let content = factory();
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed(ctx, self_id, content, delay);
        } else if let Some(source) = self.rich_tooltip_source.clone() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_rich_tooltip_source(ctx, self_id, source, delay);
        } else {
            let implicit = (!self.shows_label()).then(|| self.label.clone());
            if let Some(text) = self.tooltip.clone().or(implicit) {
                let tip = ctx.add(crate::tooltip::TooltipWidget::new(text));
                let delay = ctx.theme().motion.tooltip_delay;
                ctx.attach_tooltip(self_id, tip, delay);
            }
        }

        // A cell that overflows into the menu goes dormant mid-hover if
        // the pointer happened to be over it while the control narrowed.
        // Dormancy fires no `PointerLeave`, so clear the shared hover
        // slot here or the chrome keeps tinting a segment that is no
        // longer on the strip.
        {
            let hovered = self.hovered_segment.clone();
            let activation = ctx.activation_signal(self_id);
            ctx.effect(&activation, move |active| {
                if !*active && hovered.get() == Some(idx) {
                    hovered.set(None);
                }
            });
        }

        // Click selects (arena gates disabled cells, so no per-cell guard
        // needed); hover drives the chrome's hover tint. Focus stays on
        // the parent SegmentedControl (the segments are not tab stops).
        //
        // Every internal writer goes through the control's `select`
        // funnel, which targets the private index mirror and never the
        // public id signal — see the write-discipline note on
        // `SegmentedControl`.
        let select = self.select.clone();
        let hovered = self.hovered_segment.clone();
        let handlers = HandlerSet::new()
            .cursor(CursorIcon::Pointer)
            .focusable(false)
            .on_tap({
                let select = select.clone();
                move |_pos, ctx: &mut EventContext| {
                    select(idx, ctx);
                }
            })
            // The cell advertises `Action::Click` in `accessibility`; the
            // framework routes an AT / automation click here rather than
            // synthesizing a pointer tap, so the selection must be driven
            // explicitly (same shape as Button / Checkbox / RadioButton).
            .on_access_action(move |action, ctx: &mut EventContext| {
                if action == teksilo_core::accesskit::Action::Click {
                    select(idx, ctx);
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            })
            .on_hover(move |entered, _ctx| {
                if entered {
                    hovered.set(Some(idx));
                } else if hovered.get() == Some(idx) {
                    hovered.set(None);
                }
            });
        ctx.apply_self_handlers(handlers);

        vec![content_id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        // The parent assigns exact bounds in `place_children`, so for a
        // *bounded* proposal echoing it is both correct and cheapest.
        //
        // For an **unbounded** proposal this must report the cell's real
        // intrinsic size: that is the path `LayoutContext::measure_intrinsic`
        // takes, and it is how the control learns each segment's natural
        // width — including for cells that are currently dormant because
        // they overflowed into the menu. Echoing `0.0` here (as this did
        // before overflow existed) would make every natural width zero,
        // "everything fits" trivially true, and the whole overflow feature
        // a silent no-op.
        let pad = SEGMENTED_CONTROL_PADDING_HORIZONTAL * 2.0;
        let content = self.content_id.and_then(|id| {
            let inner = match proposal.width {
                Some(w) => SizeProposal::with_width((w - pad).max(0.0)),
                None => SizeProposal::unspecified(),
            };
            ctx.child_size(id, inner)
        });
        let content = content.unwrap_or(Size::new(0.0, 0.0));
        Size::new(
            proposal.width.unwrap_or(content.width + pad),
            proposal.height.unwrap_or(content.height),
        )
        .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        if let Some(c) = children.first_mut() {
            // Measure the label row at the cell's *bounded* width so a
            // `single_line` label ellipsizes to fit the cell instead of
            // overflowing it, then centre the hugged result in both axes.
            // (A `Center` wrapper would measure the row unbounded and let a
            // too-long label spill past the control — see `build`.)
            let inner = ctx
                .child_size(c.id, SizeProposal::with_width(bounds.width))
                .unwrap_or_else(|| bounds.size());
            let w = inner.width.min(bounds.width);
            let h = inner.height.min(bounds.height);
            c.origin = Point::new(
                bounds.x + ((bounds.width - w) / 2.0).max(0.0),
                bounds.y + ((bounds.height - h) / 2.0).max(0.0),
            );
            c.size = Size::new(w, h);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::RadioButton);
        builder.set_name(self.label.resolve_now());
        builder.set_selected(self.selected.get() == self.index);
        // "N of M" over the live segment list. Hidden segments
        // (`Segment::visible(false)`) are not part of the set at all;
        // *overflowed* ones are — they are still reachable, from the
        // chevron menu — so the count deliberately exceeds the number of
        // rendered radios while the control is narrow.
        builder.set_position_in_set(self.index + 1);
        builder.set_size_of_set(self.live_count);
        // Sibling relations, for AT that announces group membership.
        // Only currently-active cells are in the buffer.
        for id in self.group_ids.borrow().iter() {
            builder.push_to_radio_group(teksilo_core::accessibility::widget_id_to_node_id(*id));
        }
        // Framework a11y walker sets `set_disabled` from arena state.
        builder.add_action(teksilo_core::accesskit::Action::Click);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.content_id.into_iter().collect()
    }
}
