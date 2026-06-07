//! RadioButton — mutually exclusive selection within a group.
//!
//! Non-generic: uses `usize` for values. Multiple RadioButtons share a
//! `Signal<usize>` — selecting one automatically deselects others.
//! V2 attached handlers — no event() override.

use std::cell::RefCell;
use std::rc::Rc;

use bastyde_canvas::{Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::signal::Signal;
use bastyde_core::styles::{RadioStyleConfig, RadioVariant, SharedRadioStyle};
use bastyde_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{TextRole, TextStyleRole, VAlignment};

use crate::primitives::{HStack, MinSize, TextWidget, VStack};
use bastyde_i18n::LocalizedString;

/// A radio button that sets a shared `Signal<usize>` to its value when selected.
pub struct RadioButton {
    label: Option<LocalizedString>,
    caption: Option<LocalizedString>,
    value: usize,
    selected: Signal<usize>,
    /// Initial enabled-state; forwarded to the arena at build time.
    initial_enabled: bool,
    tooltip_text: Option<LocalizedString>,
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    composite_tooltip_content: Option<Box<dyn bastyde_core::widget::Widget>>,
    variant: RadioVariant,
    style_override: Option<SharedRadioStyle>,
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
            initial_enabled: true,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
            variant: RadioVariant::default(),
            style_override: None,
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

    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
        self.label = Some(ls);
        self
    }

    /// Secondary explanatory text rendered below the label, left-aligned
    /// with the label (not the radio circle). Uses the `small` /
    /// `text_secondary` style. Has no effect unless `label(...)` is also set.
    pub fn caption(mut self, text: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = text.into();
        self.caption = Some(ls);
        self
    }

    /// Set the initial enabled state. Forwarded to the arena via
    /// `ctx.enabled_when(self_id, false)` at build time. Reactive
    /// enable/disable is supported via `ctx.enabled_when(id, signal)`.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.initial_enabled = enabled;
        self
    }

    /// Pick the design-language variant. Default `Circle`. The active
    /// `RadioStyle` impl decides what the variant means visually.
    pub fn variant(mut self, variant: RadioVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Per-call style override. Replaces the theme-wide default
    /// `RadioStyle` for just this RadioButton instance.
    pub fn style(mut self, style: impl bastyde_core::styles::RadioStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip resolved from the app-wide tooltip
    /// registry. See [`Button::rich_tooltip`](crate::button::Button::rich_tooltip).
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip driven by inline `TooltipContent`.
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a composite tooltip — third tier, hosting an arbitrary
    /// widget tree. See [`Button::composite_tooltip`](crate::button::Button::composite_tooltip).
    pub fn composite_tooltip(
        mut self,
        content: impl bastyde_core::widget::Widget + 'static,
    ) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
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

/// Internal interaction state — local to this widget's handlers; the
/// active `RadioStyle` only sees the four derived boolean signals
/// (is_hovered, is_pressed, is_focused, is_disabled) plus is_selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InteractionState {
    Idle,
    Hovered,
    Pressed,
    Focused,
    Disabled,
}

impl Widget for RadioButton {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        use crate::styles::recipe_radio_style as radio_dims;
        let selected = self.selected.clone();
        let value = self.value;
        let variant = self.variant;
        let self_id = ctx.self_id();

        // Forward initial-enabled into the arena; see IconButton.
        if !self.initial_enabled {
            ctx.enabled_when(self_id, false);
        }
        let effective_enabled = ctx.effective_enabled_signal(self_id);

        let interaction = ctx.signal(InteractionState::Idle);

        let is_selected = selected.map(move |s| *s == value);
        let is_hovered = interaction.map(|s| matches!(s, InteractionState::Hovered));
        let is_pressed = interaction.map(|s| matches!(s, InteractionState::Pressed));
        let is_focused = interaction.map(|s| matches!(s, InteractionState::Focused));
        // is_disabled derives from the arena.
        let is_disabled = effective_enabled.map(|on| !*on);

        let style: SharedRadioStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.radio.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeRadioStyle));
        let cfg = RadioStyleConfig {
            is_selected,
            is_hovered,
            is_pressed,
            is_focused,
            is_disabled,
            variant,
        };
        let body_id = style.make_body(&cfg, ctx);

        let mut row = HStack::new()
            .spacing(radio_dims::RADIO_LABEL_GAP)
            .add_child(body_id);
        if let Some(ref label) = self.label {
            let label_widget = TextWidget::new(label.clone())
                .style(TextStyleRole::Body)
                .color(TextRole::Primary)
                .single_line()
                .a11y_hidden();
            let label_id = ctx.add(label_widget);

            let label_column_id = if let Some(ref caption) = self.caption {
                let caption_widget = TextWidget::new(caption.clone())
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
        let root_id = ctx.add(
            MinSize::new(radio_dims::RADIO_HIT_AREA, radio_dims::RADIO_HIT_AREA).child_id(row_id),
        );

        if let Some(content) = self.composite_tooltip_content.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed(ctx, root_id, content, delay);
        } else if let Some(source) = self.rich_tooltip_source.take() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_rich_tooltip_source(ctx, root_id, source, delay);
        } else if let Some(tooltip_text) = self.tooltip_text.clone() {
            let tw = crate::tooltip::TooltipWidget::new(tooltip_text);
            let tid = ctx.add(tw);
            let delay = ctx.theme().motion.tooltip_delay;
            ctx.attach_tooltip(root_id, tid, delay);
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

        // Framework gates events on arena.is_enabled; no per-handler
        // snapshot guards anymore.
        let handler_set = HandlerSet::new()
            .on_tap({
                move |_pos, _ctx: &mut EventContext| {
                    sel_tap.set(value);
                    int_tap.set(InteractionState::Hovered);
                }
            })
            .on_hover({
                move |entered: bool, _ctx: &mut EventContext| {
                    if entered {
                        int_hover.set(InteractionState::Hovered);
                    } else {
                        int_hover.set(InteractionState::Idle);
                    }
                }
            })
            .on_key({
                move |event: &WidgetEvent, _ctx: &mut EventContext| -> EventResponse {
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
                            // Lone-KeyUp guard: only select if we saw the
                            // matching KeyDown (state is Pressed). A stray KeyUp
                            // — e.g. a shortcut consumed the KeyDown and focus
                            // returned here — must NOT select.
                            if int_key.get() != InteractionState::Pressed {
                                return EventResponse::Ignored;
                            }
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
                move |action: bastyde_core::accesskit::Action,
                      _ctx: &mut EventContext|
                      -> EventResponse {
                    if action == bastyde_core::accesskit::Action::Click {
                        sel_access.set(value);
                        EventResponse::Handled
                    } else {
                        EventResponse::Ignored
                    }
                }
            })
            .focusable(true)
            .cursor(CursorIcon::Pointer);

        ctx.apply_self_handlers(handler_set);

        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        if let Some(root) = self.root_child_id
            && let Some(size) = ctx.child_size(root, proposal)
        {
            return (size).into();
        }
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bastyde_canvas::Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::RadioButton);
        if let Some(ref label) = self.label {
            builder.set_name(label.resolve_now());
        }
        if let Some(ref caption) = self.caption {
            builder.set_description(caption.resolve_now());
        }
        // ARIA role="radio" uses aria-checked (→ AccessKit `toggled`),
        // not aria-selected. `selected` is for options, tabs, and grid cells.
        builder.set_toggled(self.is_selected());
        // Publish radio-group membership if this button was wrapped
        // in a `RadioGroup`. Each button declares every sibling
        // (including itself) so AT can announce "2 of 3".
        if let Some(group_ids) = &self.group_ids {
            for &id in group_ids.borrow().iter() {
                builder.push_to_radio_group(bastyde_core::accessibility::widget_id_to_node_id(id));
            }
        }
        // Framework a11y walker sets `set_disabled` from arena state.
        builder.add_action(bastyde_core::accesskit::Action::Click);
        builder.add_action(bastyde_core::accesskit::Action::Focus);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::event::Modifiers;
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_i18n::lit;

    #[test]
    fn selecting_one_deselects_others() {
        use crate::primitives::VStack;
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let r0 = tree.add(RadioButton::new(0, selected.clone()).label(lit!("A")));
        let r1 = tree.add(RadioButton::new(1, selected.clone()).label(lit!("B")));
        let r2 = tree.add(RadioButton::new(2, selected.clone()).label(lit!("C")));
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
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let _r0 = tree.add(RadioButton::new(0, selected.clone()).label(lit!("A")));
        let r1 = tree.add(RadioButton::new(1, selected.clone()).label(lit!("B")));
        tree.layout(SizeProposal::exact(200.0, 200.0));

        tree.focus(r1);
        tree.press_key(Key::Space, Modifiers::NONE);
        assert_eq!(selected.get(), 1);
    }

    #[test]
    fn lone_keyup_does_not_select() {
        // Lone-KeyUp guard: a KeyUp with no matching KeyDown must NOT select.
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let _r0 = tree.add(RadioButton::new(0, selected.clone()).label(lit!("A")));
        let r1 = tree.add(RadioButton::new(1, selected.clone()).label(lit!("B")));
        tree.layout(SizeProposal::exact(200.0, 200.0));

        tree.focus(r1);
        tree.dispatch_event(WidgetEvent::KeyUp {
            key: Key::Space,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(selected.get(), 0, "a lone KeyUp must not select the radio");

        tree.press_key(Key::Space, Modifiers::NONE);
        assert_eq!(selected.get(), 1);
    }

    #[test]
    fn accessibility() {
        let selected = Signal::new(1_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let r0 = tree.add(RadioButton::new(0, selected.clone()).label(lit!("A")));
        let r1 = tree.add(RadioButton::new(1, selected.clone()).label(lit!("B")));
        tree.layout(SizeProposal::exact(200.0, 200.0));

        let info0 = tree.accessibility_node(r0);
        assert_eq!(info0.role(), bastyde_core::accesskit::Role::RadioButton);
        assert!(!info0.is_toggled());

        let info1 = tree.accessibility_node(r1);
        assert!(info1.is_toggled());
    }

    #[test]
    fn accessibility_has_actions() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let r0 = tree.add(RadioButton::new(0, selected).label(lit!("A")));
        tree.layout(SizeProposal::exact(200.0, 200.0));
        let info = tree.accessibility_node(r0);
        assert!(
            info.actions()
                .contains(&bastyde_core::accesskit::Action::Click)
        );
    }
}
