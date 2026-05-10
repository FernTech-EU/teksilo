//! Toggle — an animated on/off switch.
//!
//! The widget itself is a thin event-handler wrapper that delegates
//! all painting and chrome composition to a [`ToggleStyle`] impl. The
//! IntUI default ([`crate::styles::RecipeToggleStyle`]) ships out of
//! the box; apps install a different look per-call via
//! `Toggle::style(...)` or theme-wide via the `ComponentStyles.toggle`
//! slot (step 8 of the styling refactor).
//!
//! No `paint()` method on this widget — the only canvas work happens
//! inside the active `ToggleStyle::make_body` subtree.

use std::rc::Rc;

use fern_canvas::{Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::focus::FocusOrigin;
use fern_core::signal::Signal;
use fern_core::styles::{SharedToggleStyle, ToggleStyle, ToggleStyleConfig};
use fern_core::widget::{CursorIcon, LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;

// Re-export the variant enum at module top so callers can write
// `Toggle::new(...).variant(ToggleVariant::Pill)` without a deeper
// import path. Same pattern as `Button` re-exporting `ButtonVariant`.
pub use fern_core::styles::ToggleVariant;

/// An animated toggle switch bound to a `Signal<bool>`.
pub struct Toggle {
    on: Signal<bool>,
    label: Option<String>,
    enabled: bool,
    variant: ToggleVariant,
    style: Option<SharedToggleStyle>,
    hovered: Signal<bool>,
    focused: Signal<bool>,
    focus_origin: Signal<Option<FocusOrigin>>,
    body_id: Option<WidgetId>,
}

impl Toggle {
    pub fn new(on: Signal<bool>) -> Self {
        Self {
            on,
            label: None,
            enabled: true,
            variant: ToggleVariant::default(),
            style: None,
            hovered: Signal::new(false),
            focused: Signal::new(false),
            focus_origin: Signal::new(None),
            body_id: None,
        }
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

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
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
}

impl std::fmt::Debug for Toggle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Toggle")
            .field("enabled", &self.enabled)
            .field("variant", &self.variant)
            .finish()
    }
}

impl Widget for Toggle {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Resolve the active style: per-call override > theme slot >
        // built-in `RecipeToggleStyle` default. Theme-slot lookup
        // ships in step 8; for now per-call override falls through
        // to the recipe default.
        let style: SharedToggleStyle = self
            .style
            .clone()
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeToggleStyle));

        // Build the visual body via the active style. The body is a
        // child subtree we'll lay out to the bounds we get.
        let cfg = ToggleStyleConfig {
            is_on: self.on.clone(),
            is_hovered: self.hovered.clone(),
            is_focused: self.focused.clone(),
            is_disabled: Signal::new(!self.enabled),
            variant: self.variant,
        };
        let body_id = style.make_body(&cfg, ctx);

        // Wrap body + optional label in an HStack so the label paints
        // alongside the body without this widget needing a `paint()`
        // method. label_gap is small (6 dp default in IntUI); a fixed
        // `HStack::spacing` is plenty without a per-theme token here.
        let root = if let Some(ref label) = self.label {
            use crate::primitives::{HStack, TextWidget};
            use fern_tokens::TextStyleRole;
            let label_widget = TextWidget::new_literal(label.clone()).style(TextStyleRole::Body);
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

        // Wire up the toggle's interactive behaviour. The body owns
        // paint; the wrapper owns input handling.
        let on = self.on.clone();
        let hovered = self.hovered.clone();
        let focused = self.focused.clone();
        let focus_origin = self.focus_origin.clone();
        let enabled = self.enabled;

        let toggle = {
            let on = on.clone();
            move || {
                on.set(!on.get());
            }
        };

        let mut handlers = HandlerSet::new()
            .focusable(enabled)
            .cursor(CursorIcon::Pointer);

        {
            let toggle = toggle.clone();
            handlers = handlers.on_tap(move |_pos, _ctx| {
                if enabled {
                    toggle();
                }
            });
        }
        {
            let hovered = hovered.clone();
            handlers = handlers.on_hover(move |entered, _ctx| {
                hovered.set(entered);
            });
        }
        {
            let toggle = toggle.clone();
            handlers = handlers.on_key(move |event, _ctx| {
                if !enabled {
                    return EventResponse::Ignored;
                }
                match event {
                    WidgetEvent::KeyDown {
                        key: Key::Space, ..
                    } => EventResponse::Handled,
                    WidgetEvent::KeyUp {
                        key: Key::Space, ..
                    } => {
                        toggle();
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }
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
                if action == fern_core::accesskit::Action::Click && enabled {
                    toggle();
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            });
        }

        ctx.apply_self_handlers(handlers);

        vec![body_id]
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
            self.label.is_some(),
            "Toggle is missing an accessible label — \
             screen readers will announce \"switch\" with no context. \
             Call .label(...) when constructing the widget."
        );
        builder.set_role(fern_core::accesskit::Role::Switch);
        if let Some(ref label) = self.label {
            builder.set_name(label);
        }
        builder.set_toggled(self.on.get());
        if !self.enabled {
            builder.set_disabled();
        }
        builder.add_action(fern_core::accesskit::Action::Click);
        builder.add_action(fern_core::accesskit::Action::Focus);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use fern_core::event::Modifiers;
    use fern_core::widget_tree::WidgetTree;

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
        let t = tree.add(Toggle::new(on).label_literal("Dark mode"));
        tree.layout(SizeProposal::exact(100.0, 60.0));
        let info = tree.accessibility_node(t);
        assert_eq!(info.role(), fern_core::accesskit::Role::Switch);
        assert!(info.is_toggled());
    }

    #[test]
    fn accessibility_has_actions() {
        let on = Signal::new(false);
        let mut tree = WidgetTree::new();
        let t = tree.add(Toggle::new(on).label_literal("Dark mode"));
        tree.layout(SizeProposal::exact(100.0, 60.0));
        let info = tree.accessibility_node(t);
        assert!(
            info.actions()
                .contains(&fern_core::accesskit::Action::Click)
        );
    }
}
