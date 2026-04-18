//! RadioButton — mutually exclusive selection within a group.
//!
//! Non-generic: uses `usize` for values. Multiple RadioButtons share a
//! `Signal<usize>` — selecting one automatically deselects others.
//! V2 attached handlers — no event() override.

use std::cell::RefCell;
use std::rc::Rc;

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::{
    BorderRole, CornerRadius, SurfaceRole, TextRole, TextStyleRole, VAlignment,
};

use crate::button::InteractionState;
use crate::primitives::{FixedSize, HStack, MinSize, RectWidget, TextWidget, VStack, ZStack};

/// A radio button that sets a shared `Signal<usize>` to its value when selected.
pub struct RadioButton {
    label: Option<String>,
    caption: Option<String>,
    value: usize,
    selected: Signal<usize>,
    enabled: bool,
    tooltip_text: Option<String>,
    interaction: Option<Signal<InteractionState>>,
    root_child_id: Option<WidgetId>,
    /// Shared radio-group sibling id buffer populated by an enclosing
    /// `RadioGroup`. When set, `accessibility()` emits
    /// `push_to_radio_group(sibling_id)` for every id in the buffer
    /// so screen readers can announce "2 of 3" positional info.
    /// Loose radios not wrapped in a RadioGroup leave this `None`
    /// and drop the group membership metadata.
    group_ids: Option<Rc<RefCell<Vec<WidgetId>>>>,
}

impl RadioButton {
    pub fn new(value: usize, selected: Signal<usize>) -> Self {
        Self {
            label: None,
            caption: None,
            value,
            selected,
            enabled: true,
            tooltip_text: None,
            interaction: None,
            root_child_id: None,
            group_ids: None,
        }
    }

    /// Called by `RadioGroup` at build time to install the shared
    /// sibling-id buffer. Not part of the public fluent API —
    /// users wrap radios in `RadioGroup::new().radio(...)` rather
    /// than threading the buffer manually.
    pub(crate) fn set_group_ids(&mut self, ids: Rc<RefCell<Vec<WidgetId>>>) {
        self.group_ids = Some(ids);
    }

    pub fn label(mut self, label: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        self.label = Some(ls.resolve_now());
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `label(...)` accepting a raw string.
    #[doc(hidden)]
    pub fn label_literal(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Secondary explanatory text rendered below the label, left-aligned
    /// with the label (not the radio circle). Uses the `small` /
    /// `text_secondary` style. Has no effect unless `label(...)` is also set.
    pub fn caption(mut self, text: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = text.into();
        self.caption = Some(ls.resolve_now());
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `caption(...)` accepting a raw string.
    #[doc(hidden)]
    pub fn caption_literal(mut self, text: impl Into<String>) -> Self {
        self.caption = Some(text.into());
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn tooltip(mut self, text: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = text.into();
        self.tooltip_text = Some(ls.resolve_now());
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `tooltip(...)` accepting a raw string.
    #[doc(hidden)]
    pub fn tooltip_literal(mut self, text: impl Into<String>) -> Self {
        self.tooltip_text = Some(text.into());
        self
    }

    fn is_selected(&self) -> bool {
        self.selected.get() == self.value
    }
}

impl std::fmt::Debug for RadioButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RadioButton")
            .field("label", &self.label)
            .field("caption", &self.caption)
            .field("value", &self.value)
            .finish()
    }
}

fn resolve_circle_border_role(state: InteractionState, selected: bool) -> BorderRole {
    // Focus wins: a keyboard-focused radio always draws the
    // accent ring, even when selected or disabled — it's the only
    // focus indicator (no external ring).
    match state {
        InteractionState::Focused => BorderRole::Focused,
        InteractionState::Disabled => BorderRole::AccentDisabled,
        _ if selected => BorderRole::Accent,
        InteractionState::Hovered => BorderRole::Strong,
        _ => BorderRole::Default,
    }
}

fn resolve_dot_role(state: InteractionState) -> SurfaceRole {
    // Jewel renders the radio (and its disabled state) as a pre-baked
    // SVG icon, so there is no canonical token for "disabled dot" to
    // mirror. We pick `AccentDisabled` because the outer ring already
    // uses it in the Disabled state, making the whole widget read as
    // one desaturated-accent block. The previous mapping (`text_disabled`)
    // left the dot a neutral gray against an accent-disabled ring — a
    // two-color disabled state that was inconsistent with the rest of
    // the widget chrome.
    if state == InteractionState::Disabled {
        SurfaceRole::AccentDisabled
    } else {
        SurfaceRole::Accent
    }
}

impl Widget for RadioButton {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme();
        let radio_style = theme.components.radio;
        let radius_pill = theme.shape.radius_pill;
        let focus_ring_width = theme.shape.focus_ring_width;
        let border_width = theme.shape.border_width;
        let selected = self.selected.clone();
        let value = self.value;
        let enabled = self.enabled;

        let interaction = ctx.signal(if enabled {
            InteractionState::Idle
        } else {
            InteractionState::Disabled
        });
        self.interaction = Some(interaction.clone());

        // Border role depends on `interaction` and group selection. `zip`
        // registers both upstream roots; the paint layer resolves the
        // role against the current theme, so runtime theme switches
        // refresh colors for free without a third zip.
        let is_selected = selected.map(move |s| *s == value);
        let border_role = interaction
            .zip(&is_selected)
            .map(|(s, sel)| resolve_circle_border_role(*s, *sel));
        // Int UI focus convention: thicken the existing border to
        // `focus_ring_width` on focus, instead of wrapping the circle
        // in a separate ring.
        let border_width_signal = interaction.map(move |s| match *s {
            InteractionState::Focused => focus_ring_width,
            _ => border_width,
        });
        let outer = RectWidget::new()
            .bind_border_color(border_role)
            .bind_border_width(border_width_signal)
            .corner_radius(CornerRadius::uniform(radius_pill));
        let outer_id = ctx.add(outer);
        let outer_sized = ctx.add(
            FixedSize::new()
                .bind_width(radio_style.visual_size)
                .bind_height(radio_style.visual_size)
                .child_id(outer_id),
        );

        let dot_role = interaction.map(|s| resolve_dot_role(*s));
        let dot = RectWidget::new()
            .bind_background(dot_role)
            .corner_radius(CornerRadius::uniform(radius_pill));
        let dot_id = ctx.add(dot);
        let dot_sized = ctx.add(
            FixedSize::new()
                .bind_width(radio_style.inner_dot_size)
                .bind_height(radio_style.inner_dot_size)
                .child_id(dot_id),
        );

        ctx.visible_when(dot_sized, selected.map(move |s| *s == value));

        // Compose the visual circle with the inner dot. No
        // external focus ring — the circle's own border is the
        // focus indicator (thickened + accent-colored) per the
        // Int UI convention applied uniformly across widgets.
        let radio = ctx.add(ZStack::new().add_child(outer_sized).add_child(dot_sized));

        let mut row = HStack::new().spacing(radio_style.label_gap).add_child(radio);
        if let Some(ref label) = self.label {
            let label_widget = TextWidget::new_literal(label)
                .style(TextStyleRole::Body)
                .color(TextRole::Primary)
                .single_line()
                .a11y_hidden();
            let label_id = ctx.add(label_widget);

            let label_column_id = if let Some(ref caption) = self.caption {
                let caption_widget = TextWidget::new_literal(caption)
                    .style(TextStyleRole::Small)
                    .color(TextRole::Secondary)
                    .a11y_hidden();
                let caption_id = ctx.add(caption_widget);
                ctx.add(
                    VStack::new()
                        .spacing(2.0)
                        .add_child(label_id)
                        .add_child(caption_id),
                )
            } else {
                label_id
            };
            row = row.add_child(label_column_id);
        }
        // Top-align so the radio circle sits next to the label's first line
        // instead of the vertical center of the label+caption column.
        if self.caption.is_some() && self.label.is_some() {
            row = row.alignment(VAlignment::Top);
        }

        let row_id = ctx.add(row);
        let root_id =
            ctx.add(MinSize::new(radio_style.hit_area, radio_style.hit_area).child_id(row_id));

        if let Some(ref tooltip_text) = self.tooltip_text {
            let tw = crate::tooltip::TooltipWidget::new_literal(tooltip_text);
            let tid = ctx.add(tw);
            ctx.attach_tooltip(root_id, tid, std::time::Duration::from_millis(500));
        }

        self.root_child_id = Some(root_id);

        // --- V2 attached handlers ---
        let sel_tap = self.selected.clone();
        let sel_key = self.selected.clone();
        let sel_access = self.selected.clone();
        let int_tap = interaction.clone();
        let int_hover = interaction.clone();
        let int_key = interaction.clone();
        let int_focus = interaction.clone();

        let handler_set = HandlerSet::new()
            .on_tap({
                move |_pos, _ctx: &mut EventContext| {
                    if !enabled {
                        return;
                    }
                    sel_tap.set(value);
                    int_tap.set(InteractionState::Hovered);
                }
            })
            .on_hover({
                move |entered: bool, _ctx: &mut EventContext| {
                    if !enabled {
                        return;
                    }
                    if entered {
                        int_hover.set(InteractionState::Hovered);
                    } else {
                        int_hover.set(InteractionState::Idle);
                    }
                }
            })
            .on_key({
                move |event: &WidgetEvent, _ctx: &mut EventContext| -> EventResponse {
                    if !enabled {
                        return EventResponse::Ignored;
                    }
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::Space, ..
                        } => {
                            int_key.set(InteractionState::Pressed);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyUp {
                            key: Key::Space, ..
                        } => {
                            sel_key.set(value);
                            int_key.set(InteractionState::Focused);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            })
            .on_focus({
                move |gained: bool, _ctx: &mut EventContext| {
                    if gained {
                        if int_focus.get() == InteractionState::Idle {
                            int_focus.set(InteractionState::Focused);
                        }
                    } else {
                        int_focus.set(InteractionState::Idle);
                    }
                }
            })
            .on_access_action({
                move |action: fern_core::accesskit::Action,
                      _ctx: &mut EventContext|
                      -> EventResponse {
                    if action == fern_core::accesskit::Action::Click && enabled {
                        sel_access.set(value);
                        EventResponse::Handled
                    } else {
                        EventResponse::Ignored
                    }
                }
            })
            .focusable(enabled)
            .cursor(CursorIcon::Pointer);

        ctx.apply_self_handlers(handler_set);

        vec![root_id]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        if let Some(root) = self.root_child_id
            && let Some(size) = ctx.child_size(root, proposal)
        {
            return size;
        }
        proposal.resolve(0.0, 0.0)
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = fern_canvas::Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::RadioButton);
        if let Some(ref label) = self.label {
            builder.set_name(label);
        }
        if let Some(ref caption) = self.caption {
            builder.set_description(caption);
        }
        // AccessKit / ARIA models radio selection via `selected`,
        // not `toggled` — `toggled` is for checkbox/switch on-off.
        builder.set_selected(self.is_selected());
        // Publish radio-group membership if this button was wrapped
        // in a `RadioGroup`. Each button declares every sibling
        // (including itself) so AT can announce "2 of 3".
        if let Some(group_ids) = &self.group_ids {
            for &id in group_ids.borrow().iter() {
                builder.push_to_radio_group(
                    fern_core::accessibility::widget_id_to_node_id(id),
                );
            }
        }
        if !self.enabled {
            builder.set_disabled();
        }
        builder.add_action(fern_core::accesskit::Action::Click);
        builder.add_action(fern_core::accesskit::Action::Focus);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::event::Modifiers;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;

    #[test]
    fn selecting_one_deselects_others() {
        use crate::primitives::VStack;
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let r0 = tree.add(RadioButton::new(0, selected.clone()).label_literal("A"));
        let r1 = tree.add(RadioButton::new(1, selected.clone()).label_literal("B"));
        let r2 = tree.add(RadioButton::new(2, selected.clone()).label_literal("C"));
        let _root = tree.add(VStack::new().add_child(r0).add_child(r1).add_child(r2));
        tree.layout(SizeProposal::exact(200.0, 300.0));

        assert_eq!(selected.get(), 0);
        tree.click(r1);
        assert_eq!(selected.get(), 1);
        tree.click(r2);
        assert_eq!(selected.get(), 2);
        tree.click(r0);
        assert_eq!(selected.get(), 0);
    }

    #[test]
    fn space_selects() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let _r0 = tree.add(RadioButton::new(0, selected.clone()).label_literal("A"));
        let r1 = tree.add(RadioButton::new(1, selected.clone()).label_literal("B"));
        tree.layout(SizeProposal::exact(200.0, 200.0));

        tree.focus(r1);
        tree.press_key(Key::Space, Modifiers::NONE);
        assert_eq!(selected.get(), 1);
    }

    #[test]
    fn accessibility() {
        let selected = Signal::new(1_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let r0 = tree.add(RadioButton::new(0, selected.clone()).label_literal("A"));
        let r1 = tree.add(RadioButton::new(1, selected.clone()).label_literal("B"));
        tree.layout(SizeProposal::exact(200.0, 200.0));

        let info0 = tree.accessibility_node(r0);
        assert_eq!(info0.role(), fern_core::accesskit::Role::RadioButton);
        assert!(!info0.is_selected());

        let info1 = tree.accessibility_node(r1);
        assert!(info1.is_selected());
    }

    #[test]
    fn accessibility_has_actions() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let r0 = tree.add(RadioButton::new(0, selected).label_literal("A"));
        tree.layout(SizeProposal::exact(200.0, 200.0));
        let info = tree.accessibility_node(r0);
        assert!(
            info.actions()
                .contains(&fern_core::accesskit::Action::Click)
        );
    }
}
