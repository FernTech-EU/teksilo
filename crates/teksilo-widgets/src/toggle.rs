// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Toggle — an animated on/off switch bound to a [`Signal<bool>`](teksilo_core::signal::Signal).
//!
//! Renders as a sliding-knob switch (IntUI default) or one of the alternate
//! [`ToggleVariant`] shapes. All visual chrome is delegated to a [`ToggleStyle`]
//! impl; the widget itself owns only event handling (tap, Space, AccessKit
//! `Click`). The IntUI recipe
//! ([`crate::styles::RecipeToggleStyle`]) ships out of the box; apps install a
//! custom look per-call with `.style(impl ToggleStyle)` or theme-wide via
//! `theme.style_slots.toggle = Some(Rc::new(…))`.
//!
//! ## Accessibility
//!
//! Emits `Role::Switch` with `toggled` reflecting the signal value. Always pair
//! with `.label(…)` — the debug build asserts that a label is present, and
//! screen readers will announce "switch" with no context if it is absent.
//!
//! ## Example
//!
//! ```rust
//! # use teksilo_widgets::Toggle;
//! # use teksilo_core::signal::Signal;
//! # use teksilo_i18n::lit;
//! let dark_mode = Signal::new(false);
//! let _w = Toggle::new(dark_mode)
//!     .label(lit!("Dark mode"));
//! ```

use std::rc::Rc;

use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::focus::FocusOrigin;
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::styles::{SharedToggleStyle, ToggleStyle, ToggleStyleConfig};
use teksilo_core::widget::{CursorIcon, LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;

// Re-export the variant enum at module top so callers can write
// `Toggle::new(...).variant(ToggleVariant::Pill)` without a deeper
// import path. Same pattern as `Button` re-exporting `ButtonVariant`.
pub use teksilo_core::styles::ToggleVariant;
use teksilo_i18n::LocalizedString;

/// An animated toggle switch bound to a `Signal<bool>`.
pub struct Toggle {
    on: Signal<bool>,
    label: Option<LocalizedString>,
    /// Enabled state, static or reactive; forwarded to the arena at build
    /// time.
    enabled: Prop<bool>,
    variant: ToggleVariant,
    style: Option<SharedToggleStyle>,
    hovered: Signal<bool>,
    focused: Signal<bool>,
    pressed: Signal<bool>,
    focus_origin: Signal<Option<FocusOrigin>>,
    body_id: Option<WidgetId>,
    /// Optional plain tooltip text shown after a hover delay. Mutually exclusive
    /// with the rich / composite slots — every setter clears the other two so
    /// the last call wins.
    tooltip_text: Option<LocalizedString>,
    /// Optional rich tooltip source (registry key or inline content).
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    /// Optional composite tooltip body (arbitrary widget tree).
    composite_tooltip_content: Option<Box<dyn Widget>>,
    /// The accessible name comes from a sibling label wired after mount — see
    /// [`Toggle::labelled_externally`].
    labelled_externally: bool,
}

impl Toggle {
    /// Create a toggle bound to `on`. The signal is both read (to paint the
    /// current state) and written (flipped on each activation).
    pub fn new(on: Signal<bool>) -> Self {
        Self {
            on,
            label: None,
            enabled: Prop::Static(true),
            variant: ToggleVariant::default(),
            style: None,
            hovered: Signal::new(false),
            focused: Signal::new(false),
            pressed: Signal::new(false),
            focus_origin: Signal::new(None),
            body_id: None,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
            labelled_externally: false,
        }
    }

    /// Accessible label announced by AT and optionally displayed beside the switch.
    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
        self.label = Some(ls);
        self
    }

    /// Declare that this toggle's accessible name comes from a **sibling label
    /// widget**, wired by a container after mount (`FormLayout::line` does this
    /// via `access_labelled_by`).
    ///
    /// Without it the debug assertion below fires even though the toggle *is*
    /// properly labelled: the `labelled_by` relation is pushed post-mount, so
    /// `accessibility()` cannot see it and every form-hosted toggle looks
    /// nameless. Setting `.label(..)` instead would satisfy the assert but
    /// render the text a second time, beside a label column that already has it.
    pub fn labelled_externally(mut self) -> Self {
        self.labelled_externally = true;
        self
    }

    /// Set the enabled state, statically or reactively. Forwarded to the
    /// arena via `ctx.enabled_when(self_id, self.enabled.clone())` at
    /// build time.
    pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self {
        self.enabled = enabled.into();
        self
    }

    /// Pick a Tier-1 design-language variant
    /// ([`ToggleVariant::Switch`] / `Pill` / `Square` / `Inset`). The
    /// active [`ToggleStyle`] decides what to do with the hint —
    /// IntUI's default impl honours all four; a custom impl might
    /// ignore the variant entirely.
    pub fn variant(mut self, variant: ToggleVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Override the active [`ToggleStyle`] for this widget instance
    /// only. Useful for one-off custom-painted toggles in a single
    /// view.
    pub fn style(mut self, style: impl ToggleStyle) -> Self {
        self.style = Some(Rc::new(style));
        self
    }

    /// Attach a plain single-line tooltip shown after a hover delay.
    ///
    /// Mutually exclusive with [`rich_tooltip`](Self::rich_tooltip),
    /// [`rich_tooltip_content`](Self::rich_tooltip_content), and
    /// [`composite_tooltip`](Self::composite_tooltip) — the last setter
    /// called wins and clears the others.
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip looked up by registry `key`.
    ///
    /// Mutually exclusive with the other tooltip setters — the last
    /// setter called wins and clears the others.
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip from an inline [`crate::tooltip::TooltipContent`]
    /// value rather than a registry key.
    ///
    /// Mutually exclusive with the other tooltip setters — the last
    /// setter called wins and clears the others.
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a composite tooltip whose body is an arbitrary widget tree.
    ///
    /// Mutually exclusive with the other tooltip setters — the last
    /// setter called wins and clears the others.
    pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
        self
    }
}

impl std::fmt::Debug for Toggle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Toggle")
            .field("enabled", &self.enabled.get())
            .field("variant", &self.variant)
            .finish()
    }
}

impl Widget for Toggle {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        // Forward the enabled state into the arena; see IconButton.
        ctx.enabled_when(self_id, self.enabled.clone());
        let effective_enabled = ctx.effective_enabled_signal(self_id);
        // Resolve the active style: per-call override > theme slot >
        // built-in `RecipeToggleStyle` default.
        let style: SharedToggleStyle = self
            .style
            .clone()
            .or_else(|| ctx.theme().style_slots.toggle.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeToggleStyle::default()));

        // Build the visual body via the active style. The body is a
        // child subtree we'll lay out to the bounds we get.
        let cfg = ToggleStyleConfig {
            is_on: self.on.clone(),
            is_hovered: self.hovered.clone(),
            is_pressed: self.pressed.clone(),
            is_focused: self.focused.clone(),
            // `:focus-visible` — input modality, so the recipe shows the
            // focus ring only during keyboard navigation, not on a click.
            is_focus_visible: ctx.focus_visible(),
            // is_disabled tracks the arena's effective enabled-state
            // reactively (see `BuildContext::effective_enabled_signal`).
            is_disabled: effective_enabled.map(|on| !*on),
            variant: self.variant,
        };
        let body_id = style.make_body(&cfg, ctx);

        // Wrap body + optional label in an HStack so the label paints
        // alongside the body without this widget needing a `paint()`
        // method. label_gap is small (6 dp default in IntUI); a fixed
        // `HStack::spacing` is plenty without a per-theme token here.
        let root = if let Some(ref label) = self.label {
            use crate::primitives::{HStack, TextWidget};
            use teksilo_tokens::TextStyleRole;
            let label_widget = TextWidget::new(label.clone()).style(TextStyleRole::Body);
            let label_id = ctx.add(label_widget);
            ctx.add(
                HStack::new()
                    .spacing(6.0)
                    .add_child(body_id)
                    .add_child(label_id),
            )
        } else {
            body_id
        };
        self.body_id = Some(root);

        // Attach tooltip (at most one tier fires; each setter cleared the others).
        if let Some(content) = self.composite_tooltip_content.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed(ctx, root, content, delay);
        } else if let Some(source) = self.rich_tooltip_source.clone() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_rich_tooltip_source(ctx, root, source, delay);
        } else if let Some(text) = self.tooltip_text.clone() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_plain_tooltip(ctx, root, text, delay);
        }

        // Wire up the toggle's interactive behaviour. The body owns
        // paint; the wrapper owns input handling.
        let on = self.on.clone();
        let hovered = self.hovered.clone();
        let focused = self.focused.clone();
        let pressed = self.pressed.clone();
        let focus_origin = self.focus_origin.clone();

        let toggle = {
            let on = on.clone();
            move || {
                on.set(!on.get());
            }
        };

        // Framework gates events on `arena.is_enabled(self_id)`; the
        // focus walker skips disabled subtrees. No need to AND with
        // a per-handler `enabled` snapshot anymore.
        let mut handlers = HandlerSet::new()
            .focusable(true)
            .cursor(CursorIcon::Pointer);

        {
            let toggle = toggle.clone();
            handlers = handlers.on_tap(move |_pos, _ctx| {
                toggle();
            });
        }
        {
            let hovered = hovered.clone();
            handlers = handlers.on_hover(move |entered, _ctx| {
                hovered.set(entered);
            });
        }
        {
            // Pointer-pressed signal (PointerDown→true, Up/Leave→false).
            // IntUI ignores it; design languages with press feedback
            // (the Material 3 switch's thumb-grow) read `is_pressed`.
            // Returns `Ignored` so the tap gesture still recognises.
            let pressed = pressed.clone();
            handlers = handlers.on_pointer_event(move |event, _ctx| {
                use teksilo_core::event::{PointerButton, WidgetEvent};
                match event {
                    WidgetEvent::PointerDown {
                        button: PointerButton::Primary,
                        ..
                    } => pressed.set(true),
                    WidgetEvent::PointerUp { .. } | WidgetEvent::PointerLeave => pressed.set(false),
                    _ => {}
                }
                teksilo_core::event::EventResponse::Ignored
            });
        }
        {
            let toggle = toggle.clone();
            // Lone-KeyUp guard: track whether we saw the matching KeyDown so
            // a stray KeyUp (e.g. a shortcut consumed the KeyDown and focus
            // returned here) does NOT toggle.
            let key_pressed = std::cell::Cell::new(false);
            handlers = handlers.on_key(move |event, _ctx| match event {
                WidgetEvent::KeyDown {
                    key: Key::Space, ..
                } => {
                    key_pressed.set(true);
                    EventResponse::Handled
                }
                WidgetEvent::KeyUp {
                    key: Key::Space, ..
                } => {
                    if !key_pressed.replace(false) {
                        return EventResponse::Ignored;
                    }
                    toggle();
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            });
        }
        {
            let focused = focused.clone();
            let focus_origin = focus_origin.clone();
            let hovered_for_focus = hovered.clone();
            handlers = handlers.on_focus(move |gained, _ctx| {
                focused.set(gained);
                if gained {
                    focus_origin.set(Some(if hovered_for_focus.get() {
                        FocusOrigin::Pointer
                    } else {
                        FocusOrigin::Keyboard
                    }));
                } else {
                    focus_origin.set(None);
                }
            });
        }
        {
            let toggle = toggle.clone();
            handlers = handlers.on_access_action(move |action, _ctx| {
                if action == teksilo_core::accesskit::Action::Click {
                    toggle();
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            });
        }

        ctx.apply_self_handlers(handlers);

        // Return `root` (the HStack wrapper when a label is set,
        // else the bare body). Returning `body_id` here would leave
        // the HStack as a parent-less arena root: its child list
        // would still list `body_id`, and the AccessKit walker would
        // see `body_id` claimed by both Toggle and the orphan HStack
        // — a "duplicate accessibility child" log on every refresh.
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.body_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        if let Some(child) = children.first_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.body_id.into_iter().collect()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        debug_assert!(
            self.label.is_some() || self.labelled_externally,
            "Toggle is missing an accessible label — \
             screen readers will announce \"switch\" with no context. \
             Call .label(...) when constructing the widget."
        );
        builder.set_role(teksilo_core::accesskit::Role::Switch);
        if let Some(ref label) = self.label {
            builder.set_name(label.resolve_now());
        }
        builder.set_toggled(self.on.get());
        // Framework a11y walker sets `set_disabled` from arena state.
        builder.add_action(teksilo_core::accesskit::Action::Click);
        builder.add_action(teksilo_core::accesskit::Action::Focus);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    use teksilo_core::event::Modifiers;
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_i18n::lit;

    #[test]
    fn focus_ring_only_under_focus_visible() {
        // `:focus-visible`: the keyboard-only focus ring. Programmatic focus
        // leaves `focus_visible` false → no ring; a key press reveals it.
        let theme = teksilo_core::presets::intui::light();
        let ring = theme.colors.focus_ring.to_array();
        let mut tree = WidgetTree::new().with_theme(theme);
        let t = tree.add(Toggle::new(Signal::new(false)));
        tree.layout(SizeProposal::exact(120.0, 60.0));

        tree.focus(t);
        assert!(
            !frame_has_ring(&tree.render(), ring),
            "no focus ring while focus-visible is false (pointer modality)",
        );

        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        assert!(
            frame_has_ring(&tree.render(), ring),
            "focus ring shows under keyboard modality",
        );
    }

    #[test]
    fn is_pressed_tracks_pointer_down_and_up() {
        use std::cell::RefCell;
        use std::rc::Rc;
        use teksilo_core::event::PointerButton;
        use teksilo_core::styles::{ToggleStyle, ToggleStyleConfig};

        // A style that captures the cfg's is_pressed signal so the test can
        // observe it (IntUI ignores is_pressed, so it isn't visible in paint).
        struct CaptureStyle(Rc<RefCell<Option<Signal<bool>>>>);
        impl ToggleStyle for CaptureStyle {
            fn make_body(&self, cfg: &ToggleStyleConfig, ctx: &mut BuildContext) -> WidgetId {
                *self.0.borrow_mut() = Some(cfg.is_pressed.clone());
                ctx.add(crate::primitives::RectWidget::new())
            }
        }

        let captured: Rc<RefCell<Option<Signal<bool>>>> = Rc::new(RefCell::new(None));
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let t = tree.add(Toggle::new(Signal::new(false)).style(CaptureStyle(captured.clone())));
        tree.layout(SizeProposal::exact(120.0, 60.0));

        let pressed = captured.borrow().clone().expect("is_pressed captured");
        assert!(!pressed.get(), "not pressed initially");

        let b = tree.bounds(t);
        let center = teksilo_canvas::Point::new(b.x + b.width / 2.0, b.y + b.height / 2.0);
        tree.pointer_down_button(center, PointerButton::Primary);
        assert!(pressed.get(), "pressed after PointerDown");
        tree.pointer_up_button(center, PointerButton::Primary);
        assert!(!pressed.get(), "released after PointerUp");
    }

    /// Whether the focus-ring *stroke* (ring color + non-zero stroke width) is
    /// present. A plain color match is ambiguous: in IntUI `focus_ring` shares
    /// the `accent` RGBA, and the toggle paints accent *fills* — the ring is
    /// the only *stroked* shape in that color.
    fn frame_has_ring(frame: &teksilo_canvas::RenderFrame, color: [f32; 4]) -> bool {
        frame
            .shapes
            .iter()
            .any(|s| s.color == color && s.stroke_width > 0.0)
            || frame.cosmetic_lines.iter().any(|l| l.color == color)
    }

    #[test]
    fn click_toggles_state() {
        let on = Signal::new(false);
        let mut tree = WidgetTree::new();
        let t = tree.add(Toggle::new(on.clone()));
        tree.layout(SizeProposal::exact(100.0, 60.0));

        tree.click(t);
        assert!(on.get());
        tree.click(t);
        assert!(!on.get());
    }

    #[test]
    fn space_toggles_state() {
        let on = Signal::new(false);
        let mut tree = WidgetTree::new();
        let t = tree.add(Toggle::new(on.clone()));
        tree.layout(SizeProposal::exact(100.0, 60.0));

        tree.focus(t);
        tree.press_key(Key::Space, Modifiers::NONE);
        assert!(on.get());
    }

    #[test]
    fn lone_keyup_does_not_toggle() {
        // Lone-KeyUp guard: a KeyUp with no matching KeyDown must NOT toggle.
        let on = Signal::new(false);
        let mut tree = WidgetTree::new();
        let t = tree.add(Toggle::new(on.clone()));
        tree.layout(SizeProposal::exact(100.0, 60.0));

        tree.focus(t);
        tree.dispatch_event(WidgetEvent::KeyUp {
            key: Key::Space,
            modifiers: Modifiers::NONE,
        });
        assert!(!on.get(), "a lone KeyUp must not toggle the switch");

        tree.press_key(Key::Space, Modifiers::NONE);
        assert!(on.get());
    }

    #[test]
    fn animation_runs_after_toggle() {
        let on = Signal::new(false);
        let mut tree = WidgetTree::new();
        let t = tree.add(Toggle::new(on.clone()));
        tree.layout(SizeProposal::exact(100.0, 60.0));

        tree.click(t); // toggles on, body's effect tweens knob
        assert!(on.get());

        // Mid-flight: animation should still be running.
        tree.tick_animations(Duration::from_millis(75));
        assert!(tree.has_active_animations());

        // After the full duration, animation completes.
        tree.tick_animations(Duration::from_millis(200));
        assert!(!tree.has_active_animations());
    }

    #[test]
    fn accessibility() {
        let on = Signal::new(true);
        let mut tree = WidgetTree::new();
        let t = tree.add(Toggle::new(on).label(lit!("Dark mode")));
        tree.layout(SizeProposal::exact(100.0, 60.0));
        let info = tree.accessibility_node(t);
        assert_eq!(info.role(), teksilo_core::accesskit::Role::Switch);
        assert!(info.is_toggled());
    }

    /// Regression: a labeled Toggle wraps its body + label in an
    /// HStack inside `build`. The earlier code returned the inner
    /// body id instead of the HStack id, which left the HStack
    /// parent-less in the arena. The AccessKit walker then saw the
    /// body id claimed by both Toggle and the orphan HStack —
    /// "Teksilo bug: duplicate accessibility child …" on every AT
    /// refresh.
    #[test]
    fn labeled_toggle_does_not_orphan_hstack_wrapper() {
        let on = Signal::new(false);
        let mut tree = WidgetTree::new();
        let _t = tree.add(Toggle::new(on).label(lit!("Dark mode")));
        tree.layout(SizeProposal::exact(200.0, 60.0));
        let update = tree.sync_accessibility();
        let mut seen = std::collections::HashMap::new();
        for (parent_id, node) in &update.nodes {
            for &child_id in node.children() {
                let prev = seen.insert(child_id, *parent_id);
                assert!(
                    prev.is_none(),
                    "duplicate AT child {child_id:?}: claimed by both {prev:?} and {parent_id:?}"
                );
            }
        }
    }

    #[test]
    fn accessibility_has_actions() {
        let on = Signal::new(false);
        let mut tree = WidgetTree::new();
        let t = tree.add(Toggle::new(on).label(lit!("Dark mode")));
        tree.layout(SizeProposal::exact(100.0, 60.0));
        let info = tree.accessibility_node(t);
        assert!(
            info.actions()
                .contains(&teksilo_core::accesskit::Action::Click)
        );
    }

    /// The **rich** tier, on a Toggle, opens the same way the plain one does.
    ///
    /// Its own case beside `tooltip_appears_on_hover` because the two tiers take
    /// different attach paths out of `build` — `attach_rich_tooltip_source`
    /// against `ctx.attach_tooltip` — and only the plain one was pinned. A
    /// settings page that moved its explanations onto its switches is the first
    /// caller to depend on the rich one here.
    #[test]
    fn a_rich_tooltip_appears_on_hover_too() {
        let mut tree = WidgetTree::new();
        let id = tree.add(
            Toggle::new(Signal::new(false))
                .label(lit!("Wi-Fi"))
                .rich_tooltip_content(crate::tooltip::TooltipContent::new(
                    "toggle.rich",
                    lit!("What this switch does"),
                )),
        );
        tree.layout(SizeProposal::exact(300.0, 200.0));
        tree.pointer_move(tree.bounds(id).center());
        tree.advance_time(std::time::Duration::from_secs(1));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "a rich tooltip on a Toggle must open under the pointer"
        );
    }

    #[test]
    fn tooltip_appears_on_hover() {
        let mut tree = WidgetTree::new();
        let id = tree.add(
            Toggle::new(Signal::new(false))
                .label(lit!("Wi-Fi"))
                .tooltip(lit!("Tip")),
        );
        tree.layout(SizeProposal::exact(300.0, 200.0));
        tree.pointer_move(tree.bounds(id).center());
        tree.advance_time(std::time::Duration::from_secs(1));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "tooltip should appear on hover"
        );
        assert!(tree.find_by_label("Tip").is_some());
    }
}
