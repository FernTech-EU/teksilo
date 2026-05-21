//! Inspector pane (right).
//!
//! Three sections, top-down:
//!
//! 1. **Variant** — radio-button group bound to `selected_variant`.
//! 2. **Knobs** — auto-generated form via `knob_form::build_knob_form`.
//!    Reset button at the section header.
//! 3. **Export** — PNG snapshot button.
//!
//! The inspector itself is a custom widget so its `build()` re-runs
//! when the selected widget changes (different `KnobSpec`, different
//! variant list).

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::widget::{LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_i18n::lit;
use bastyde_tokens::{BorderRole, SurfaceRole, TextRole, TextStyleRole};
use bastyde_widgets::primitives::{Padding, ZStack};
use bastyde_widgets::{
    Button, ButtonVariant, Divider, HStack, MaxSize, RadioButton, RectWidget, ScrollArea, Spacer,
    TextWidget, VStack,
};

use crate::app_state::AppState;
use crate::knob_form::build_knob_form;

/// Build the inspector pane and return its root widget id. Wraps an
/// [`InspectorBody`] in a sunken background panel.
pub fn build_inspector(ctx: &mut BuildContext, state: &AppState) -> WidgetId {
    let body_id = ctx.add(InspectorBody::new(state.clone()));
    let bg = RectWidget::new()
        .background(SurfaceRole::Sunken)
        .border_color(BorderRole::Default)
        .border_width(1.0);
    let bg_id = ctx.add(bg);
    let stack = ZStack::new().add_child(bg_id).add_child(body_id);
    ctx.add(stack)
}

/// Custom widget — rebuilds when `selected_widget` or
/// `selected_variant` changes so a different widget's variants/knobs
/// surface their own controls.
pub struct InspectorBody {
    state: AppState,
    root_id: Option<WidgetId>,
}

impl InspectorBody {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            root_id: None,
        }
    }
}

impl std::fmt::Debug for InspectorBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InspectorBody").finish()
    }
}

impl Widget for InspectorBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let registry = ctx.binding_registry().clone();
        let self_id = ctx.self_id();
        self.state
            .selected_widget
            .bind_to(self_id, &registry, BindingLevel::Rebuild);
        self.state
            .selected_variant
            .bind_to(self_id, &registry, BindingLevel::Rebuild);

        let widget_id = self.state.selected_widget.get();
        let variant_name = self.state.selected_variant.get();

        let entry = widget_id.and_then(bastyde_preview::find_by_id);

        let mut column = VStack::new().spacing(12.0);

        // Variant section
        let variant_section = match entry {
            Some(entry) => self.build_variant_section(ctx, entry),
            None => placeholder_section(ctx, "No widget selected."),
        };
        column = column.add_child(variant_section);

        // Knobs section
        if let (Some(entry), Some(variant_name)) = (entry, variant_name) {
            let knobs_section = self.build_knobs_section(ctx, entry, variant_name);
            column = column.add_child(knobs_section);
        }

        // Export section
        column = column.add_child(self.build_export_section(ctx));

        // Wrap in scroll area + padding.
        let column_id = ctx.add(column);
        let padding = ctx.add(Padding::symmetric(12.0, 12.0).child_id(column_id));
        let scroll_id = ctx.add(ScrollArea::from_id(padding));
        self.root_id = Some(scroll_id);
        vec![scroll_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        match self.root_id {
            Some(id) => ctx
                .child_size(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0)),
            None => proposal.resolve(0.0, 0.0),
        }
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

    fn children(&self) -> Vec<WidgetId> {
        match self.root_id {
            Some(id) => vec![id],
            None => Vec::new(),
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
        builder.set_name("Inspector");
    }
}

impl InspectorBody {
    fn build_variant_section(
        &self,
        ctx: &mut BuildContext,
        entry: &'static dyn bastyde_preview::CatalogEntry,
    ) -> WidgetId {
        let variants = entry.variants();
        let header = section_header(ctx, "Variant");

        // RadioButton::new requires `(value: usize, selected: Signal<usize>)`.
        // Bridge `Signal<Option<&'static str>>` through an index signal.
        let names: Vec<&'static str> = variants.iter().map(|v| v.name()).collect();
        let selected_name = self.state.selected_variant.clone();
        let initial_idx = names
            .iter()
            .position(|n| Some(*n) == selected_name.get())
            .unwrap_or(0);
        let idx_sig = ctx.signal(initial_idx);

        // Forward: idx_sig change → selected_name update. The
        // equality check is load-bearing — without it, this and the
        // reverse observer below ping-pong (idx → name → idx → …)
        // and recurse until the stack overflows. (The nested
        // `selected_name.set` is reentrancy-safe and no longer panics
        // — cf. signal.rs `try_set`, which snapshots and releases its
        // borrow before notifying — so this guard's sole job is to
        // terminate that cycle.) The chain only recurses when two
        // widgets share a variant *name*, which is common for
        // "default" / "primary" / "disabled" labels — so this guard
        // is mandatory.
        {
            let names_c = names.clone();
            let selected_name = selected_name.clone();
            let h = idx_sig.observe(move |i| {
                if let Some(name) = names_c.get(*i) {
                    let new_val = Some(*name);
                    if selected_name.get() != new_val {
                        selected_name.set(new_val);
                    }
                }
            });
            ctx.own_handle(h);
        }
        // Reverse: external selected_name change → idx_sig update.
        {
            let names_c = names.clone();
            let idx_sig = idx_sig.clone();
            let h = selected_name.observe(move |opt| {
                if let Some(target) = opt
                    .as_ref()
                    .and_then(|n| names_c.iter().position(|m| m == n))
                    && idx_sig.get() != target
                {
                    idx_sig.set(target);
                }
            });
            ctx.own_handle(h);
        }

        let mut column = VStack::new().spacing(4.0);
        for (i, name) in names.iter().enumerate() {
            let radio = RadioButton::new(i, idx_sig.clone());
            let label = TextWidget::new(lit!(*name))
                .style(TextStyleRole::Body)
                .color(TextRole::Primary)
                .single_line();
            let row = HStack::new().spacing(8.0).child(radio).child(label);
            column = column.child(row);
        }

        ctx.add(VStack::new().spacing(6.0).add_child(header).child(column))
    }

    fn build_knobs_section(
        &self,
        ctx: &mut BuildContext,
        entry: &'static dyn bastyde_preview::CatalogEntry,
        variant_name: &'static str,
    ) -> WidgetId {
        let spec = entry.knobs();
        if spec.declarations().is_empty() {
            let header = section_header(ctx, "Knobs");
            let placeholder = TextWidget::new(lit!("No knobs declared for this widget."))
                .style(TextStyleRole::Small)
                .color(TextRole::Secondary);
            return ctx.add(
                VStack::new()
                    .spacing(6.0)
                    .add_child(header)
                    .child(placeholder),
            );
        }
        let values = self.state.knobs_for(entry.id(), variant_name);
        let form_id = build_knob_form(ctx, &spec, &values);

        // Header with Reset button on the right.
        let title = TextWidget::new(lit!("Knobs"))
            .style(TextStyleRole::SmallBold)
            .color(TextRole::Primary);
        let widget_id = entry.id();
        let st = self.state.clone();
        let reset_btn = Button::new(lit!("Reset"))
            .variant(ButtonVariant::Ghost)
            .on_activate_fn(move |_ctx| {
                st.reset_knobs(widget_id, variant_name);
            });
        let header_row = HStack::new()
            .spacing(8.0)
            .child(title)
            .child(Spacer::new())
            .child(reset_btn);

        let divider = MaxSize::new(f32::INFINITY, 1.0).child(Divider::horizontal());
        let header_block = VStack::new().spacing(4.0).child(header_row).child(divider);

        ctx.add(
            VStack::new()
                .spacing(6.0)
                .child(header_block)
                .add_child(form_id),
        )
    }

    fn build_export_section(&self, ctx: &mut BuildContext) -> WidgetId {
        let header = section_header(ctx, "Export");
        let st = self.state.clone();
        let save_btn = Button::new(lit!("Save PNG…"))
            .variant(ButtonVariant::Plain)
            .on_activate_fn(move |_ctx| {
                if let Err(e) = crate::png_export::export_current(&st) {
                    eprintln!("PNG export failed: {}", e);
                }
            });
        ctx.add(VStack::new().spacing(6.0).add_child(header).child(save_btn))
    }
}

fn section_header(ctx: &mut BuildContext, title: &str) -> WidgetId {
    let title_widget = TextWidget::new(lit!(title))
        .style(TextStyleRole::SmallBold)
        .color(TextRole::Primary);
    let divider = MaxSize::new(f32::INFINITY, 1.0).child(Divider::horizontal());
    ctx.add(
        VStack::new()
            .spacing(4.0)
            .child(title_widget)
            .child(divider),
    )
}

fn placeholder_section(ctx: &mut BuildContext, msg: &str) -> WidgetId {
    let title = TextWidget::new(lit!(msg))
        .style(TextStyleRole::Body)
        .color(TextRole::Secondary);
    ctx.add(title)
}
