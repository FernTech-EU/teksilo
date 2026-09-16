// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! RadioButton — mutually exclusive selection control.
//!
//! Multiple `RadioButton`s share a `Signal<usize>`; selecting one writes its
//! `value` to the signal, which automatically deselects every sibling that
//! observes the same signal. The widget is non-generic: values are `usize`
//! indices into the caller's choice list. Wrap related buttons in a
//! [`RadioGroup`](crate::radio_group::RadioGroup) to provide the AT "2 of 3"
//! positional announcement required by ARIA.
//!
//! ## Touch and pen
//!
//! Same shape as [`Checkbox`](crate::Checkbox): the pressed state is the
//! framework's (`docs/touch-and-pen.md` §7.1), selection lands on the release,
//! and the 24 dp `MinSize` around the 19 dp dot already clears the conformance
//! floor at Compact.
//!
//! ## Accessibility
//!
//! Reports `Role::RadioButton` with `set_toggled` mirroring the selected
//! state. Responds to `Action::Click` from assistive technology. The focus
//! ring is keyboard-only (`:focus-visible` gated by the input-modality
//! signal). When wrapped in `RadioGroup`, each button emits
//! `push_to_radio_group([sibling_ids])` so screen readers can announce
//! positional membership.
//!
//! ```rust
//! # use teksilo_widgets::RadioButton;
//! # use teksilo_core::signal::Signal;
//! # use teksilo_i18n::lit;
//! let selected = Signal::new(0_usize);
//! let _r0 = RadioButton::new(0, selected.clone()).label(lit!("Light"));
//! let _r1 = RadioButton::new(1, selected.clone()).label(lit!("Dark"));
//! let _r2 = RadioButton::new(2, selected.clone()).label(lit!("System"));
//! ```

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use teksilo_canvas::{Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::styles::{RadioStyleConfig, RadioVariant, SharedRadioStyle};
use teksilo_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{TextRole, TextStyleRole, VAlignment};

use crate::button::InteractionState;
use crate::primitives::{HStack, MinSize, TextWidget, VStack};
use teksilo_i18n::LocalizedString;

/// A single radio button option that writes `value` into a shared `Signal<usize>` on selection.
pub struct RadioButton {
    label: Option<LocalizedString>,
    caption: Option<LocalizedString>,
    value: usize,
    selected: Signal<usize>,
    /// Enabled state, static or reactive; forwarded to the arena at
    /// build time.
    enabled: Prop<bool>,
    tooltip_text: Option<LocalizedString>,
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    composite_tooltip_content: Option<Box<dyn teksilo_core::widget::Widget>>,
    variant: RadioVariant,
    style_override: Option<SharedRadioStyle>,
    on_change: Option<Rc<dyn Fn(usize, &mut EventContext)>>,
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
    /// Create a radio button with the given `value` and shared selection signal.
    /// Run `f` when the **user** selects this button and it was not already
    /// selected, with this button's value and an `EventContext`, so it can do
    /// what a bare `Signal` write cannot (`ctx.send_intent(...)`,
    /// `ctx.set_locale(...)`, opening a window). Fires for the pointer, for
    /// `Space`, and for an assistive-technology `Click`.
    ///
    /// Re-activating the selected button writes the signal, as every path
    /// does, but reports nothing: that is not a change. Programmatic writes to
    /// the bound signal report nothing either — there is no event in flight to
    /// carry. Observe the signal for those.
    pub fn on_change(mut self, f: impl Fn(usize, &mut EventContext) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }

    pub fn new(value: usize, selected: Signal<usize>) -> Self {
        Self {
            label: None,
            caption: None,
            value,
            selected,
            enabled: Prop::Static(true),
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
            variant: RadioVariant::default(),
            style_override: None,
            on_change: None,
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

    /// Set the visible label text displayed to the right of the radio circle.
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

    /// Set the enabled state, statically or reactively. Forwarded to
    /// the arena via `ctx.enabled_when(self_id, self.enabled.clone())`
    /// at build time.
    pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self {
        self.enabled = enabled.into();
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
    pub fn style(mut self, style: impl teksilo_core::styles::RadioStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Attach a plain single-line tooltip shown on hover.
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
        content: impl teksilo_core::widget::Widget + 'static,
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
impl Widget for RadioButton {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        use crate::styles::recipe_radio_style as radio_dims;
        let selected = self.selected.clone();
        let value = self.value;
        let variant = self.variant;
        let self_id = ctx.self_id();

        // Forward the enabled state into the arena; see IconButton.
        ctx.enabled_when(self_id, self.enabled.clone());
        let effective_enabled = ctx.effective_enabled_signal(self_id);

        let interaction = ctx.signal(InteractionState::Idle);

        let is_selected = selected.map(move |s| *s == value);
        let is_hovered = interaction.map(|s| matches!(s, InteractionState::Hovered));
        let is_pressed = interaction.map(|s| matches!(s, InteractionState::Pressed));
        // `:focus-visible`: reveal the focus ring during keyboard navigation
        // only, not on a mouse click. Gate raw focus on the input-modality
        // signal (true after a key event, false after pointer-down).
        let is_focused = interaction
            .map(|s| matches!(s, InteractionState::Focused))
            .and(&ctx.focus_visible());
        // is_disabled derives from the arena.
        let is_disabled = effective_enabled.map(|on| !*on);

        let style: SharedRadioStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.radio.clone())
            .unwrap_or_else(|| {
                Rc::new(crate::styles::RecipeRadioStyle::for_tokens(
                    &ctx.theme().input,
                ))
            });
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
            .child(body_id);
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
                ctx.add(VStack::new().spacing(2.0).child(label_id).child(caption_id))
            } else {
                label_id
            };
            row = row.child(label_column_id);
        }
        // Top-align so the radio circle sits next to the label's first line
        // instead of the vertical center of the label+caption column.
        if self.caption.is_some() && self.label.is_some() {
            row = row.alignment(VAlignment::Top);
        }

        // The hit box comes from the same recipe the chrome was built from, so
        // a density switch moves both together.
        let hit_area = crate::styles::RadioRecipe::for_tokens(&ctx.theme().input).hit_area;
        let row_id = ctx.add(row);
        let root_id = ctx.add(MinSize::new(hit_area, hit_area).child(row_id));

        if let Some(content) = self.composite_tooltip_content.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed(ctx, root_id, content, delay);
        } else if let Some(source) = self.rich_tooltip_source.take() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_rich_tooltip_source(ctx, root_id, source, delay);
        } else if let Some(tooltip_text) = self.tooltip_text.clone() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_plain_tooltip(ctx, root_id, tooltip_text, delay);
        }

        self.root_child_id = Some(root_id);

        // --- V2 attached handlers ---
        let select = {
            let selected = self.selected.clone();
            let on_change = self.on_change.clone();
            let value = self.value;
            move |ctx: &mut EventContext| {
                // Re-activating the button that is already selected is not a
                // change, and `on_change` says change. The write still happens
                // so every path stays idempotent.
                let was = selected.get();
                selected.set(value);
                if was != value
                    && let Some(ref f) = on_change
                {
                    f(value, ctx);
                }
            }
        };
        let select_tap = select.clone();
        let select_key = select.clone();
        let select_access = select;
        let int_tap = interaction.clone();
        let int_hover = interaction.clone();
        let int_key = interaction.clone();
        let int_focus = interaction.clone();

        // The pointer press is the framework's, not this control's own: the
        // router knows about a press that slid off its target, one that slid
        // back on, and one a pan claimant took away with no release to reset
        // from — none of which a `PointerDown` / `PointerUp` pair here can
        // see. `docs/touch-and-pen.md` §7.1. `pointer_over` carries the hover
        // truth across the press, so a press that ends without an activation
        // rests on the right state.
        let pointer_over = Rc::new(Cell::new(false));
        crate::button::bind_press_interaction(ctx, interaction.clone(), pointer_over.clone());

        // Framework gates events on arena.is_enabled; no per-handler
        // snapshot guards anymore.
        let handler_set = HandlerSet::new()
            .on_tap({
                let hovering = pointer_over.clone();
                move |_pos, ctx: &mut EventContext| {
                    select_tap(ctx);
                    int_tap.set(if ctx.pointer_kind().hovers() {
                        hovering.set(true);
                        InteractionState::Hovered
                    } else {
                        InteractionState::Idle
                    });
                }
            })
            .on_hover({
                let hovering = pointer_over.clone();
                move |entered: bool, _ctx: &mut EventContext| {
                    hovering.set(entered);
                    if entered {
                        int_hover.set(InteractionState::Hovered);
                    } else {
                        int_hover.set(InteractionState::Idle);
                    }
                }
            })
            .on_key({
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
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
                            select_key(ctx);
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
                move |action: teksilo_core::accesskit::Action,
                      ctx: &mut EventContext|
                      -> EventResponse {
                    if action == teksilo_core::accesskit::Action::Click {
                        select_access(ctx);
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
    ) -> teksilo_core::widget::LayoutResponse {
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
            child.origin = teksilo_canvas::Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    /// The first implementer of the hit-targeting **shape** hook: a labelled
    /// radio is its whole row, but a bare one is a disc inside a square box.
    ///
    /// A `RadioButton` with a label is tappable across the label too — the row
    /// *is* the target — so the default rectangular distance is exactly right
    /// and this returns it unchanged. A **bare** radio (a cell in a table, a
    /// tight option grid) is a 19 dp disc centred in a 24 dp box, and measuring
    /// a near miss to the box would offer the same reach diagonally past its
    /// corner as straight out from its edge — where the corner is 3 dp further
    /// from the thing the user aimed at. Measuring to the disc makes the slop
    /// follow the silhouette, so a miss past the corner loses to a neighbour
    /// that is genuinely nearer.
    ///
    /// This is consulted **only** by the miss-only slop pass, never by the
    /// exact one, so a click inside the box's corner still selects the radio
    /// exactly as it always has — `hit_shape` is deliberately left alone.
    fn hit_distance(&self, local_point: teksilo_canvas::Point, bounds: Rect) -> Option<f32> {
        if self.label.is_some() {
            return Some(teksilo_core::pointer::hit_slop::rect_distance(
                bounds,
                local_point,
            ));
        }
        let diameter = crate::styles::recipe_radio_style::RADIO_VISUAL_SIZE
            .min(bounds.width)
            .min(bounds.height);
        Some(teksilo_core::pointer::hit_slop::circle_distance(
            bounds.center(),
            diameter / 2.0,
            local_point,
        ))
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::RadioButton);
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
                builder.push_to_radio_group(teksilo_core::accessibility::widget_id_to_node_id(id));
            }
        }
        // Framework a11y walker sets `set_disabled` from arena state.
        builder.add_action(teksilo_core::accesskit::Action::Click);
        builder.add_action(teksilo_core::accesskit::Action::Focus);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_core::event::Modifiers;
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_i18n::lit;

    #[test]
    fn selecting_one_deselects_others() {
        use crate::primitives::VStack;
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let r0 = tree.add(RadioButton::new(0, selected.clone()).label(lit!("A")));
        let r1 = tree.add(RadioButton::new(1, selected.clone()).label(lit!("B")));
        let r2 = tree.add(RadioButton::new(2, selected.clone()).label(lit!("C")));
        let _root = tree.add(VStack::new().child(r0).child(r1).child(r2));
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
    fn on_change_reports_a_real_change_only() {
        use crate::primitives::VStack;
        use std::cell::RefCell;
        use std::rc::Rc;

        let selected = Signal::new(0_usize);
        let seen: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
        let mk = |v: usize, seen: &Rc<RefCell<Vec<usize>>>, sel: &Signal<usize>| {
            let sink = seen.clone();
            RadioButton::new(v, sel.clone())
                .label(lit!("Option"))
                .on_change(move |now, _ctx| sink.borrow_mut().push(now))
        };
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let r0 = tree.add(mk(0, &seen, &selected));
        let r1 = tree.add(mk(1, &seen, &selected));
        let _root = tree.add(VStack::new().child(r0).child(r1));
        tree.layout(SizeProposal::exact(200.0, 200.0));

        tree.click(r1);
        // Already selected: the write still lands, but nothing changed, so
        // nothing is reported.
        tree.click(r1);
        tree.focus(r0);
        tree.press_key(teksilo_core::event::Key::Space, Modifiers::NONE);
        tree.dispatch_access_action(
            teksilo_core::accessibility::widget_id_to_node_id(r1),
            teksilo_core::accesskit::Action::Click,
            None,
            &mut teksilo_core::NoopWindowOps,
        );

        assert_eq!(*seen.borrow(), vec![1, 0, 1]);
        assert_eq!(selected.get(), 1);
    }

    #[test]
    fn on_change_is_silent_for_a_programmatic_write() {
        use std::cell::Cell;
        use std::rc::Rc;

        let selected = Signal::new(0_usize);
        let fired = Rc::new(Cell::new(false));
        let sink = fired.clone();
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let _r = tree.add(
            RadioButton::new(1, selected.clone())
                .label(lit!("B"))
                .on_change(move |_now, _ctx| sink.set(true)),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));

        selected.set(1);
        assert!(!fired.get());
    }

    #[test]
    fn space_selects() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let r0 = tree.add(RadioButton::new(0, selected.clone()).label(lit!("A")));
        let r1 = tree.add(RadioButton::new(1, selected.clone()).label(lit!("B")));
        tree.layout(SizeProposal::exact(200.0, 200.0));

        let info0 = tree.accessibility_node(r0);
        assert_eq!(info0.role(), teksilo_core::accesskit::Role::RadioButton);
        assert!(!info0.is_toggled());

        let info1 = tree.accessibility_node(r1);
        assert!(info1.is_toggled());
    }

    #[test]
    fn accessibility_has_actions() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let r0 = tree.add(RadioButton::new(0, selected).label(lit!("A")));
        tree.layout(SizeProposal::exact(200.0, 200.0));
        let info = tree.accessibility_node(r0);
        assert!(
            info.actions()
                .contains(&teksilo_core::accesskit::Action::Click)
        );
    }
    // -----------------------------------------------------------------
    // The framework press (docs/touch-and-pen.md §7.1)
    // -----------------------------------------------------------------

    struct PressProbe(std::rc::Rc<std::cell::RefCell<Option<(Signal<bool>, Signal<bool>)>>>);

    impl teksilo_core::styles::RadioStyle for PressProbe {
        fn make_body(
            &self,
            cfg: &teksilo_core::styles::RadioStyleConfig,
            ctx: &mut BuildContext,
        ) -> WidgetId {
            *self.0.borrow_mut() = Some((cfg.is_pressed.clone(), cfg.is_hovered.clone()));
            ctx.add(crate::primitives::FixedSize::new().width(19.0).height(19.0))
        }
    }

    #[allow(clippy::type_complexity)]
    fn probed_radio_with_hover() -> (
        WidgetTree,
        WidgetId,
        Signal<bool>,
        Signal<bool>,
        Signal<usize>,
    ) {
        let probe: std::rc::Rc<std::cell::RefCell<Option<(Signal<bool>, Signal<bool>)>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));
        let selected = Signal::new(9_usize);
        let mut theme = teksilo_core::presets::intui::light();
        theme.style_slots.radio = Some(std::rc::Rc::new(PressProbe(probe.clone())));
        let mut tree = WidgetTree::new().with_theme(theme);
        let rb = tree.add(RadioButton::new(0, selected.clone()).label(lit!("One")));
        tree.layout(SizeProposal::exact(200.0, 60.0));
        let (pressed, hovered) = probe.borrow().clone().expect("style ran");
        (tree, rb, pressed, hovered, selected)
    }

    fn probed_radio() -> (WidgetTree, WidgetId, Signal<bool>, Signal<usize>) {
        let (tree, rb, pressed, _hovered, selected) = probed_radio_with_hover();
        (tree, rb, pressed, selected)
    }

    /// Where the radio comes to rest after a selection — its own copy of the
    /// button family's `on_tap` resting-state rule, which is duplicated per
    /// control rather than shared, so it needs its own probe.
    #[test]
    fn a_mouse_selection_rests_hovered_and_a_finger_selection_rests_idle() {
        use crate::button::press_test_support::touch_tap;

        let (mut tree, rb, pressed, hovered, selected) = probed_radio_with_hover();
        let at = tree.bounds(rb).center();
        tree.pointer_move(at);
        assert!(hovered.get(), "the pointer arrived over the dot");
        tree.pointer_down_button(at, teksilo_core::event::PointerButton::Primary);
        tree.pointer_up_button(at, teksilo_core::event::PointerButton::Primary);
        assert_eq!(selected.get(), 0, "the release selected");
        assert!(!pressed.get());
        assert!(
            hovered.get(),
            "a mouse that clicked the dot is still on it, so it rests hovered",
        );

        let (mut tree, rb, pressed, hovered, selected) = probed_radio_with_hover();
        let at = tree.bounds(rb).center();
        touch_tap(&mut tree, at);
        assert_eq!(selected.get(), 0, "the contact selected on its release");
        assert!(!pressed.get());
        assert!(
            !hovered.get(),
            "a finger leaves nothing behind, so the dot must rest idle",
        );
    }

    /// The mouse path: press lights the state the style has always been given,
    /// release selects.
    #[test]
    fn a_mouse_press_lights_the_pressed_state_and_the_release_selects() {
        let (mut tree, rb, pressed, selected) = probed_radio();
        let at = tree.bounds(rb).center();
        tree.pointer_move(at);
        tree.pointer_down_button(at, teksilo_core::event::PointerButton::Primary);
        assert!(pressed.get());
        assert_eq!(selected.get(), 9, "the press selects nothing");
        tree.pointer_up_button(at, teksilo_core::event::PointerButton::Primary);
        assert!(!pressed.get());
        assert_eq!(selected.get(), 0);
    }

    /// And a finger, with the slide-off abort in the middle.
    #[test]
    fn a_touch_tap_selects_on_release_and_a_slide_off_abandons_it() {
        use crate::button::press_test_support::{finger, touch};
        use teksilo_core::pointer::PointerPhase;

        let (mut tree, rb, pressed, selected) = probed_radio();
        let bounds = tree.bounds(rb);
        let at = bounds.center();
        let away = teksilo_canvas::Point::new(at.x, bounds.y + bounds.height + 80.0);

        let id = finger();
        tree.dispatch_pointer(touch(id, PointerPhase::Down, at, 0));
        assert!(pressed.get());
        tree.dispatch_pointer(touch(id, PointerPhase::Move, away, 20));
        assert!(!pressed.get());
        tree.dispatch_pointer(touch(id, PointerPhase::Up, away, 40));
        assert_eq!(
            selected.get(),
            9,
            "a release off the control selects nothing"
        );

        let id = finger();
        tree.dispatch_pointer(touch(id, PointerPhase::Down, at, 100));
        tree.dispatch_pointer(touch(id, PointerPhase::Up, at, 130));
        assert_eq!(selected.get(), 0);
        assert!(!pressed.get());
    }

    /// A 24 dp hit box around a 19 dp dot: at the floor already, so no widening
    /// mechanism is involved.
    #[test]
    fn the_radio_hit_box_clears_the_conformance_floor_at_compact() {
        let theme = teksilo_core::presets::intui::light();
        let floor = theme.input.min_target_conformance;
        let mut tree = WidgetTree::new().with_theme(theme);
        let rb = tree.add(RadioButton::new(0, Signal::new(0_usize)));
        tree.layout(SizeProposal::exact(200.0, 60.0));
        let b = tree.bounds(rb);
        assert!(b.width >= floor && b.height >= floor, "measured {b:?}");
    }
}

#[cfg(test)]
mod hit_distance_tests {
    use super::*;
    use teksilo_canvas::Point;
    use teksilo_core::pointer::{EventTime, PointerId, PointerInfo};
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_i18n::lit;

    fn finger() -> PointerInfo {
        PointerInfo::touch(PointerId::MOUSE, EventTime::ZERO)
    }

    /// A **bare** radio measures a near miss to its disc, so a press past its
    /// box's corner is further away than one past its edge.
    ///
    /// Measured on the widget directly, the way its labelled twin below is.
    /// The tree-level version this replaced placed its probe points *outside*
    /// the radio's box but within the disc's reach, and that arrangement only
    /// existed while the box was smaller than the density's `target_size`:
    /// P20's density projection makes a radio's box exactly `target_size` at
    /// every density, so the miss-only slop pass — whose per-node top-up is
    /// `(target_size - min(w, h)) / 2` — now has nothing left to add for this
    /// control, and no point outside the box is in reach of a disc that stayed
    /// 19 dp. What the test is actually about is the *shape* of the measure,
    /// which this asserts without depending on the reach at all.
    #[test]
    fn a_bare_radio_measures_a_near_miss_to_its_disc() {
        use teksilo_canvas::Rect;
        use teksilo_core::widget::Widget;
        let radio = RadioButton::new(0, Signal::new(0_usize));
        let bounds = Rect::new(0.0, 0.0, 24.0, 24.0);

        // Straight out from the edge, and diagonally past the corner at the
        // same axis distance. A rectangle would call these equally far; a disc
        // does not.
        let side = Point::new(bounds.right() + 4.0, bounds.center().y);
        let corner = Point::new(bounds.right() + 4.0, bounds.bottom() + 4.0);

        let d_side = radio
            .hit_distance(side, bounds)
            .expect("a bare radio measures");
        let d_corner = radio
            .hit_distance(corner, bounds)
            .expect("a bare radio measures");
        assert!(
            d_corner > d_side,
            "the corner ({d_corner}) must be further from the disc than the edge ({d_side})"
        );
        // And it really is the disc, not the box: a point on the box's own edge
        // is already a positive distance from the disc inside it.
        let on_edge = Point::new(bounds.right(), bounds.center().y);
        assert!(
            radio.hit_distance(on_edge, bounds).expect("measures") > 0.0,
            "a rectangular measure would call the box's edge zero"
        );
    }

    /// A **labelled** radio is its whole row, so it keeps the rectangular
    /// measure — the corner of a row is not further from the target than its
    /// edge, because the row IS the target.
    #[test]
    fn a_labelled_radio_keeps_the_rectangular_measure() {
        use teksilo_canvas::Rect;
        use teksilo_core::widget::Widget;
        let radio = RadioButton::new(0, Signal::new(0_usize)).label(lit!("Option"));
        let bounds = Rect::new(0.0, 0.0, 200.0, 24.0);
        let corner = Point::new(203.0, 28.0);
        assert_eq!(
            radio.hit_distance(corner, bounds),
            Some(teksilo_core::pointer::hit_slop::rect_distance(
                bounds, corner
            ))
        );
    }

    /// The exact pass is untouched: a click inside the box's corner still
    /// selects the radio, exactly as it always has.
    #[test]
    fn the_exact_pass_still_accepts_the_corner_of_the_box() {
        use crate::primitives::Center;
        let selected = Signal::new(1_usize);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let radio = tree.add(RadioButton::new(0, selected.clone()));
        tree.add(Center::new().child(radio));
        tree.layout(SizeProposal::exact(200.0, 200.0));
        let b = tree.bounds(radio);
        let corner = Point::new(b.x + 1.0, b.y + 1.0);
        // The exact pass resolves to the deepest node, which is inside the
        // radio's own subtree — what matters is that the corner is still hit at
        // all, and that clicking it still selects.
        let hit = tree.hit_test(corner);
        assert!(
            hit.is_some_and(
                |id| std::iter::successors(Some(id), |id| tree.parent(*id)).any(|id| id == radio)
            ),
            "the corner of a bare radio's box resolved to {hit:?}"
        );
        tree.click(radio);
        assert_eq!(selected.get(), 0);
    }
}
