//! Canvas pane — the centre of the previewer.
//!
//! `PreviewCanvas` is a custom rebuilding widget. Switcher (which would
//! be the natural fit for "swap visible child by index") fixes its
//! child set at `build()` time and can't host arbitrary children
//! swapped at runtime, so the canvas owns the rebuild logic itself:
//!
//! * Reads `selected_widget`, `selected_variant`, and `canvas_rebuild_tick`
//!   at `BindingLevel::Rebuild`. Any change to those signals re-runs `build()`.
//! * Reads `canvas_theme` and `background_mode` at `RepaintOnly` (theme
//!   change does its own propagation through scoped theme; background
//!   role swap is a property change).
//! * Inside `build()`, looks up the registry entry for the current
//!   `(widget_id, variant_name)`, fetches the cached `KnobValues` from
//!   `AppState::knobs_for`, and calls `entry.build(...)` to produce a
//!   fresh widget instance.
//! * Wraps the widget in a background rect and a footer strip showing
//!   bounds + last frame time.

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{SurfaceRole, TextRole, TextStyleRole};
use bastyde_widgets::primitives::ZStack;
use bastyde_widgets::{
    Center, Divider, Expand, HStack, MaxSize, RectWidget, Spacer, TextWidget, VStack,
};

use crate::app_state::{AppState, BackgroundMode};

/// The custom rebuilding widget that hosts whichever previewed widget
/// is currently selected.
pub struct PreviewCanvas {
    state: AppState,
    root_id: Option<WidgetId>,
    /// Live size readout — bound to a label in the footer strip.
    size_readout: Signal<String>,
}

impl PreviewCanvas {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            root_id: None,
            size_readout: Signal::new(String::new()),
        }
    }

    fn build_inner_widget(
        &self,
        registry: &bastyde_core::binding::BindingRegistry,
        self_id: bastyde_core::widget_id::WidgetId,
    ) -> Box<dyn Widget> {
        let (widget_id, variant_name) = match (
            self.state.selected_widget.get(),
            self.state.selected_variant.get(),
        ) {
            (Some(w), Some(v)) => (w, v),
            _ => return placeholder_message("Select a widget on the left."),
        };
        let entry = match bastyde_preview::find_by_id(widget_id) {
            Some(e) => e,
            None => return placeholder_message("Unknown widget id."),
        };
        let variants = entry.variants();
        let resolved_name = variants
            .iter()
            .find(|v| v.name() == variant_name)
            .map(|v| v.name())
            .or_else(|| variants.first().map(|v| v.name()));
        let variant_name = match resolved_name {
            Some(n) => n,
            None => return placeholder_message("This widget has no variants."),
        };
        let knobs = self.state.knobs_for(widget_id, variant_name);
        // Bind every knob signal at Rebuild level so a knob change
        // re-runs `entry.build(...)` with the fresh values. Most
        // widgets read knob values once at construction (e.g.,
        // `Button::new_literal(label)` consumes the string by-value
        // rather than `Prop::Bound`), so the only way to reflect the
        // edit is to reconstruct.
        knobs.bind_all(self_id, registry, bastyde_core::binding::BindingLevel::Rebuild);
        entry.build(variant_name, &knobs)
    }
}

impl std::fmt::Debug for PreviewCanvas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreviewCanvas").finish()
    }
}

impl Widget for PreviewCanvas {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Rebuild on widget/variant/reset-tick change. Knob value
        // mutations don't trigger this — they propagate through the
        // child's `Prop::Bound` bindings at RepaintOnly / Relayout.
        let registry = ctx.binding_registry().clone();
        let self_id = ctx.self_id();
        self.state
            .selected_widget
            .bind_to(self_id, &registry, BindingLevel::Rebuild);
        self.state
            .selected_variant
            .bind_to(self_id, &registry, BindingLevel::Rebuild);
        self.state
            .canvas_rebuild_tick
            .bind_to(self_id, &registry, BindingLevel::Rebuild);

        let inner = self.build_inner_widget(&registry, self_id);
        let inner_id = ctx.add_boxed(inner);

        // Background under the previewed widget.
        let bg = self.build_background(ctx);
        let bg_id = ctx.add(bg);

        // Center the previewed widget. Use Center (which fills the
        // available proposal) over the inner widget so the inner
        // widget appears in the middle regardless of its natural size.
        let preview_centered = ctx.add(Center::new().child_id(inner_id));

        // Stage = ZStack(bg, centered preview).
        let stage = ZStack::new().add_child(bg_id).add_child(preview_centered);
        let stage_id = ctx.add(stage);

        // Wrap stage in an Expand that claims remaining VStack space
        // *and* ignores its child's intrinsic size when computing
        // its own — important because ZStack with all-fill children
        // reports 0×0 intrinsic, which would propagate up and starve
        // the layout otherwise.
        let stage_expanded = ctx.add(Expand::new().child_id(stage_id));

        // Footer strip: thin divider + size readout.
        let divider = ctx.add(MaxSize::new(f32::INFINITY, 1.0).child(Divider::horizontal()));
        let footer = self.build_footer(ctx);
        let footer_id = ctx.add(footer);

        let column = VStack::new()
            .add_child(stage_expanded)
            .add_child(divider)
            .add_child(footer_id);
        let root_id = ctx.add(column);
        self.root_id = Some(root_id);
        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        let size = match self.root_id {
            Some(id) => ctx
                .child_size(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0)),
            None => proposal.resolve(0.0, 0.0),
        };
        // Update the readout signal — the footer label re-renders.
        let formatted = format!("{:.0} × {:.0} px", size.width, size.height);
        if self.size_readout.get() != formatted {
            self.size_readout.set(formatted);
        }
        size.into()
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

    fn children(&self) -> Vec<WidgetId> {
        match self.root_id {
            Some(id) => vec![id],
            None => Vec::new(),
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
        builder.set_name("Preview canvas");
    }
}

impl PreviewCanvas {
    fn build_background(&self, ctx: &mut BuildContext) -> impl Widget + 'static {
        // Background colour reacts to the user's background-mode
        // selection. Bound at the RectWidget's background prop, so
        // changes repaint without rebuild.
        //
        // The default ("Themed") deliberately uses `SurfaceRole::Sunken`
        // rather than `Main`: most controls (Button, Card, Panel, …)
        // sit on `Main`, and using the same colour for the canvas
        // background would make them invisible. Sunken provides a
        // subtle but distinguishable backdrop.
        let mode = self.state.background_mode.clone();
        let role = mode.map(|m| match m {
            BackgroundMode::Themed => SurfaceRole::Sunken,
            BackgroundMode::ContentSurface => SurfaceRole::Content,
            BackgroundMode::Sunken => SurfaceRole::Sunken,
            // Approximate "checkered" with a sunken neutral surface
            // — a real checker pattern needs a custom paint, deferred.
            BackgroundMode::Checkered => SurfaceRole::Sunken,
        });
        let _ = ctx; // kept for symmetry with other helpers
        RectWidget::new().background(role)
    }

    fn build_footer(&self, ctx: &mut BuildContext) -> impl Widget + 'static {
        let size_readout = self.size_readout.clone();
        let widget_id_sig = self.state.selected_widget.clone();
        let variant_sig = self.state.selected_variant.clone();
        let label = widget_id_sig.zip(&variant_sig).map(|t| match *t {
            (Some(w), Some(v)) => format!("{} · {}", w, v),
            _ => "—".to_string(),
        });
        let label_widget = TextWidget::new_literal("")
            .style(TextStyleRole::Tiny)
            .color(TextRole::Secondary)
            .single_line()
            .bind_text(label);
        let size_widget = TextWidget::new_literal("")
            .style(TextStyleRole::Tiny)
            .color(TextRole::Secondary)
            .single_line()
            .bind_text(size_readout);
        let _ = ctx;
        bastyde_widgets::primitives::Padding::symmetric(4.0, 12.0).child(
            HStack::new()
                .spacing(12.0)
                .child(label_widget)
                .child(Spacer::new())
                .child(size_widget),
        )
    }
}

fn placeholder_message(text: &str) -> Box<dyn Widget> {
    Box::new(
        Center::new().child(
            TextWidget::new_literal(text)
                .style(TextStyleRole::Body)
                .color(TextRole::Secondary),
        ),
    )
}
