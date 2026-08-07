// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`TextScaleControl`] — the settings control that grows all text in the app.
//!
//! Drop this into a preferences/settings window to let low-vision users scale
//! every piece of text uniformly (the framework multiplies the active theme's
//! typography by the chosen factor — see
//! [`WidgetTree::set_user_text_scale`](teksilo_core::widget_tree::WidgetTree::set_user_text_scale)).
//! It is a thin specialization of [`SpinBox`] that displays a percent
//! (80 %–200 %, step 10 %) and, on each edit, both **persists** the value and
//! **applies it app-wide** — so the developer only has to place the widget.
//!
//! Bind it to the persisted factor signal, typically the settings-backed
//! `teksilo_settings::TEXT_SCALE_KEY`:
//!
//! ```ignore
//! use teksilo::prelude::*;
//! use teksilo::widgets::TextScaleControl;
//!
//! // inside build():
//! let scale = ctx.settings().signal_for(&teksilo_settings::TEXT_SCALE_KEY);
//! ctx.add(TextScaleControl::new(scale).label(tr!(text_size())));
//! ```
//!
//! Writing the bound signal triggers the `SettingsStore`'s debounced auto-save
//! (persistence), and the widget's `on_value_changed` calls
//! [`EventContext::set_text_scale`](teksilo_core::widget::EventContext::set_text_scale)
//! (immediate app-wide application). At startup `teksilo-app` reads the saved
//! key and seeds every window, so the chosen size is restored automatically.

use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::LocalizedString;

use crate::primitives::{HStack, TextWidget};
use crate::spin_box::SpinBox;

/// Lowest user-selectable scale, as a percent. The control is grow-oriented but
/// allows a slight shrink for users who prefer a denser UI.
const MIN_PERCENT: i32 = 80;
/// Highest user-selectable scale, as a percent (2× the base size).
const MAX_PERCENT: i32 = 200;
/// Single-step increment, as a percent.
const STEP_PERCENT: i32 = 10;
/// Page-step increment (PageUp/PageDown), as a percent.
const PAGE_PERCENT: i32 = 50;

/// Convert a scale factor (`1.0` = 100 %) to a rounded integer percent.
fn factor_to_percent(factor: f32) -> i32 {
    (factor * 100.0).round() as i32
}

/// A specialized [`SpinBox`] for the global user text-scale setting.
///
/// See the [module docs](self) for the persistence + app-wide application
/// contract. Construct with [`TextScaleControl::new`], optionally attach a
/// visible [`label`](TextScaleControl::label), and place it in a settings view.
#[derive(Debug)]
pub struct TextScaleControl {
    /// The bound scale factor (`1.0` = 100 %). Usually the settings-backed
    /// signal so edits persist; writes also flow out via `set_text_scale`.
    factor_signal: Signal<f32>,
    /// Internal percent view bridged to `factor_signal`, driving the inner
    /// `SpinBox<i32>`.
    percent_signal: Signal<i32>,
    /// Optional visible label rendered to the leading side of the spinbox.
    label: Option<LocalizedString>,
    root_child_id: Option<WidgetId>,
    /// Optional plain tooltip text shown after a hover delay.
    /// Mutually exclusive with `rich_tooltip_source` and
    /// `composite_tooltip_content` — every tooltip setter clears the other two.
    tooltip_text: Option<LocalizedString>,
    /// Optional rich tooltip source (registry key or inline content).
    /// Mutually exclusive with `tooltip_text` and `composite_tooltip_content`
    /// — every tooltip setter clears the other two so last-call wins.
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    /// Optional composite tooltip body. Hosts an arbitrary widget inside the
    /// tooltip overlay. Mutually exclusive with `tooltip_text` and
    /// `rich_tooltip_source` per the last-call-wins contract.
    composite_tooltip_content: Option<Box<dyn Widget>>,
}

impl TextScaleControl {
    /// Construct bound to `factor_signal` (a scale factor where `1.0` = 100 %).
    ///
    /// Pass `ctx.settings().signal_for(&teksilo_settings::TEXT_SCALE_KEY)` to get
    /// automatic persistence; any `Signal<f32>` works for ad-hoc / preview use.
    pub fn new(factor_signal: Signal<f32>) -> Self {
        let percent = factor_to_percent(factor_signal.get());
        Self {
            factor_signal,
            percent_signal: Signal::new(percent),
            label: None,
            root_child_id: None,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
        }
    }

    /// Attach a visible label placed to the leading side of the spinbox
    /// (e.g. `tr!(text_size())`). Also used as the control's accessible name.
    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Attach a plain tooltip that appears after a hover delay.
    ///
    /// Clears any previously set rich or composite tooltip (last-call wins).
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip resolved from the app-wide tooltip registry.
    ///
    /// `key` is looked up in the
    /// [`TooltipRegistry`](crate::tooltip::TooltipRegistry) at build time.
    /// Clears any previously set plain or composite tooltip (last-call wins).
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip driven by inline
    /// [`TooltipContent`](crate::tooltip::TooltipContent).
    ///
    /// Clears any previously set plain or composite tooltip (last-call wins).
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a composite tooltip that hosts an arbitrary widget body.
    ///
    /// The `content` widget is rendered inside the tooltip overlay after the
    /// heavy hover delay. Clears any previously set plain or rich tooltip
    /// (last-call wins).
    pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
        self
    }
}

impl Widget for TextScaleControl {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Reflect external factor changes (settings load, another window's edit
        // fanned in) into the percent view. Guarded so the round-trip from an
        // in-widget edit (percent → factor → here) does not re-enter.
        ctx.effect(&self.factor_signal, {
            let percent = self.percent_signal.clone();
            move |factor| {
                let pct = factor_to_percent(*factor);
                if percent.get() != pct {
                    percent.set(pct);
                }
            }
        });

        let at_name = self
            .label
            .clone()
            .unwrap_or_else(|| LocalizedString::literal("Text scale"));

        let spin = SpinBox::new(self.percent_signal.clone(), MIN_PERCENT, MAX_PERCENT)
            .single_step(STEP_PERCENT)
            .page_step(PAGE_PERCENT)
            // Plain unit string — `suffix` is not localized; acceptable for a
            // settings unit. The percent value itself is what the user reads.
            .suffix(" %")
            .label(at_name)
            .on_value_changed({
                let factor = self.factor_signal.clone();
                move |pct, ectx| {
                    let f = pct as f32 / 100.0;
                    // Persist (settings-backed signals auto-save on set)…
                    factor.set(f);
                    // …and apply app-wide immediately (every window re-scales).
                    ectx.set_text_scale(f);
                }
            });
        let spin_id = ctx.add(spin);

        let root = if let Some(label) = &self.label {
            let label_id = ctx.add(TextWidget::new(label.clone()));
            ctx.add(
                HStack::new()
                    .spacing(8.0)
                    .add_child(label_id)
                    .add_child(spin_id),
            )
        } else {
            spin_id
        };

        self.root_child_id = Some(root);

        // Attach whichever tooltip variant was set, anchored on this widget's
        // own root (not forwarded to the inner SpinBox).
        if let Some(content) = self.composite_tooltip_content.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed(ctx, root, content, delay);
        } else if let Some(source) = self.rich_tooltip_source.clone() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_rich_tooltip_source(ctx, root, source, delay);
        } else if let Some(text) = self.tooltip_text.clone() {
            let tooltip_widget = crate::tooltip::TooltipWidget::new(text);
            let tooltip_id = ctx.add(tooltip_widget);
            let delay = ctx.theme().motion.tooltip_delay;
            ctx.attach_tooltip(root, tooltip_id, delay);
        }

        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_child_id
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
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut teksilo_core::accessibility::AccessNodeBuilder) {
        // A labelled group wrapping the inner SpinButton.
        builder.set_role(teksilo_core::accesskit::Role::Group);
        if let Some(label) = &self.label {
            builder.set_name(label.resolve_now());
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_i18n::lit;

    #[test]
    fn factor_percent_roundtrip() {
        assert_eq!(factor_to_percent(1.0), 100);
        assert_eq!(factor_to_percent(1.5), 150);
        assert_eq!(factor_to_percent(0.8), 80);
        // Round to nearest, no truncation surprises.
        assert_eq!(factor_to_percent(1.234), 123);
    }

    #[test]
    fn percent_signal_seeded_from_factor() {
        let control = TextScaleControl::new(Signal::new(1.3));
        assert_eq!(control.percent_signal.get(), 130);
    }

    #[test]
    fn builds_and_lays_out() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let factor = Signal::new(1.0_f32);
        let id =
            tree.add(TextScaleControl::new(factor).label(LocalizedString::literal("Text size")));
        tree.layout(SizeProposal::exact(400.0, 60.0));
        let b = tree.bounds(id);
        assert!(b.width > 0.0 && b.height > 0.0);
    }

    #[test]
    fn external_factor_change_reflects_into_percent() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let factor = Signal::new(1.0_f32);
        let control = TextScaleControl::new(factor.clone());
        let percent = control.percent_signal.clone();
        tree.add(control);
        tree.layout(SizeProposal::exact(400.0, 60.0));
        // Simulate a settings load / cross-window fan-in.
        factor.set(1.6);
        assert_eq!(percent.get(), 160);
    }

    #[test]
    fn tooltip_appears_on_hover() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(TextScaleControl::new(Signal::new(1.0_f32)).tooltip(lit!("Tip")));
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
