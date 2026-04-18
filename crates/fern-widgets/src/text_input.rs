//! `TextInput` — styled single-line text field composite.
//!
//! Wraps the [`TextInputField`](crate::primitives::TextInputField)
//! editing primitive in a bordered, padded frame with placeholder
//! overlay, validation, optional clear button, and leading/trailing
//! slots. All actual text editing is delegated to the field: every
//! configuration method here has a direct counterpart on the
//! primitive.
//!
//! Most applications want `TextInput`. Choose
//! [`TextInputField`](crate::primitives::TextInputField) directly
//! when you're building a composite of your own that already
//! supplies its frame — `SpinBox` is the canonical in-tree example.
//!
//! # Example
//!
//! ```ignore
//! let search = ctx.signal(String::new());
//! TextInput::new(search.clone())
//!     .placeholder("Search...")
//!     .show_clear_button(true)
//!     .leading_slot(IconWidget::from_svg(SEARCH_ICON))
//!     .on_submit_fn(|ctx| ctx.send_intent(AppIntent::Search))
//! ```

#[cfg(test)]
mod tests;

use fern_canvas::{Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::signal::Signal;
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_builder::WidgetBuilder;
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, ColorTokens, CornerRadius};

use crate::button::InteractionState;
use crate::primitives::text_input_field::TextInputField;
use crate::primitives::{
    Expand, HStack, MinSize, Padding, RectWidget, TextWidget, ZStack,
};
use crate::tooltip::{self, RichTooltipSource};

/// Validation state for the text input field.
#[derive(Debug, Clone, Default)]
pub enum ValidationState {
    #[default]
    None,
    Error(String),
    Warning(String),
}

/// Styled single-line text input composite.
///
/// See the [module-level documentation](self) for usage examples.
pub struct TextInput {
    // ── Configuration forwarded to the inner TextInputField ─────────
    text: Signal<String>,
    placeholder: String,
    enabled: bool,
    read_only: bool,
    max_length: Option<usize>,
    on_submit: Option<Box<dyn Fn(&mut EventContext)>>,
    on_blur: Option<Box<dyn Fn(&mut EventContext)>>,
    char_filter: Option<std::rc::Rc<dyn Fn(char) -> bool>>,
    suffix: String,

    // ── Configuration owned by this composite only ──────────────────
    label: Option<String>,
    show_clear_button: bool,
    leading_slot: Option<Box<dyn Widget>>,
    trailing_slot: Option<Box<dyn Widget>>,
    validation: Signal<ValidationState>,
    tooltip_text: Option<String>,
    rich_tooltip_source: Option<RichTooltipSource>,

    // ── Internal (set during build) ─────────────────────────────────
    interaction: Signal<InteractionState>,
    root_child_id: Option<WidgetId>,
}

impl std::fmt::Debug for TextInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextInput")
            .field("placeholder", &self.placeholder)
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

impl TextInput {
    /// Construct a new text input bound to `text`.
    pub fn new(text: Signal<String>) -> Self {
        Self {
            text,
            placeholder: String::new(),
            enabled: true,
            read_only: false,
            max_length: None,
            on_submit: None,
            on_blur: None,
            char_filter: None,
            suffix: String::new(),
            label: None,
            show_clear_button: false,
            leading_slot: None,
            trailing_slot: None,
            validation: Signal::new(ValidationState::None),
            tooltip_text: None,
            rich_tooltip_source: None,
            interaction: Signal::new(InteractionState::Idle),
            root_child_id: None,
        }
    }

    // ── Builder methods ─────────────────────────────────────────────
    //
    // Every method below that has a direct analogue on
    // `TextInputField` forwards to it 1:1 at build time — the
    // `TextInput` composite just owns the framing around the field.

    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }

    /// Accessible name for the composite. Propagated to the outer
    /// container's a11y node; the inner `TextInputField` still
    /// carries `Role::TextInput` with the document's value.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    pub fn max_length(mut self, max_length: usize) -> Self {
        self.max_length = Some(max_length);
        self
    }

    pub fn show_clear_button(mut self, show: bool) -> Self {
        self.show_clear_button = show;
        self
    }

    /// Set an arbitrary widget in the leading slot (before the text area).
    /// Typically a `BuiltInButton` or `IconWidget`.
    pub fn leading_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.leading_slot = Some(Box::new(widget));
        self
    }

    /// Set an arbitrary widget in the trailing slot (after the text area).
    /// Typically a `BuiltInButton` or `IconWidget`.
    pub fn trailing_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.trailing_slot = Some(Box::new(widget));
        self
    }

    /// Closure invoked on Enter. Forwarded to `TextInputField`.
    pub fn on_submit_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.on_submit = Some(Box::new(f));
        self
    }

    /// Closure invoked on focus loss. Forwarded to `TextInputField`.
    pub fn on_blur_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.on_blur = Some(Box::new(f));
        self
    }

    /// Per-character input-filter predicate. Forwarded to
    /// `TextInputField`.
    pub fn char_filter(mut self, f: impl Fn(char) -> bool + 'static) -> Self {
        self.char_filter = Some(std::rc::Rc::new(f));
        self
    }

    /// Non-editable trailing string (Qt's `QSpinBox::suffix`).
    /// Forwarded to `TextInputField`.
    pub fn suffix(mut self, text: impl Into<String>) -> Self {
        self.suffix = text.into();
        self
    }

    pub fn validation(mut self, validation: Signal<ValidationState>) -> Self {
        self.validation = validation;
        self
    }

    pub fn tooltip_literal(mut self, text: impl Into<String>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self
    }

    pub fn rich_tooltip_key(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self
    }

    pub fn rich_tooltip(mut self, content: tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self
    }

    // ── Signal accessors (call before add to tree) ──────────────────

    /// The reactive text content signal.
    pub fn text(&self) -> Signal<String> {
        self.text.clone()
    }
}

impl Widget for TextInput {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // TextInput is a heavy composite. We snapshot the theme once for
        // static layout params (padding, border width, field height) and
        // migrate leaf color reads to `theme_signal.map(...)` so the
        // placeholder, clear-icon tint, and border color track runtime
        // theme switches. Layout params stay frozen to the theme at the
        // last rebuild — acceptable because they rarely differ between
        // themes and the composite rebuilds on data changes (text,
        // validation) that typically coincide with any theme-driven
        // layout variation a user would notice.
        let theme_signal = ctx.theme_signal();
        let theme = theme_signal.get();
        let colors = theme.colors.clone();
        let field_style = theme.components.text_field;
        let interaction = self.interaction.clone();
        let validation = self.validation.clone();

        // ── Build the inner editing primitive ──────────────────────
        //
        // The inner field owns the bound text signal, the document,
        // engine, caret, clipboard, context menu — everything
        // interactive. The composite just styles it.
        let inner_height = (field_style.height - 2.0 * field_style.border_width).max(0.0);
        let text_area_height = (inner_height - 2.0 * field_style.padding_vertical).max(0.0);

        let mut field = TextInputField::new(self.text.clone())
            .enabled(self.enabled)
            .read_only(self.read_only)
            .placeholder(self.placeholder.clone())
            .text_height(text_area_height)
            .interaction_signal(interaction.clone());
        if let Some(max) = self.max_length {
            field = field.max_length(max);
        }
        if let Some(f) = self.char_filter.take() {
            // Re-wrap the Rc'd closure into a plain closure for the
            // primitive's builder surface, which owns its own Rc.
            field = field.char_filter(move |c| (f)(c));
        }
        if let Some(cb) = self.on_submit.take() {
            field = field.on_submit_fn(move |ctx| (cb)(ctx));
        }
        if let Some(cb) = self.on_blur.take() {
            field = field.on_blur_fn(move |ctx| (cb)(ctx));
        }
        if !self.suffix.is_empty() {
            field = field.suffix(std::mem::take(&mut self.suffix));
        }

        // Expose the field's text signal for downstream reactivity
        // (placeholder visibility, clear-button visibility) before
        // the field is consumed by `ctx.add`.
        let text_signal_for_vis = field.text();

        // Text editing area, wrapped in vertical padding so slots
        // (BuiltInButton etc.) sit flush against top/bottom of the
        // inner border area and are vertically centered by the HStack.
        let padded_field = Padding::new(
            field_style.padding_vertical, 0.0, field_style.padding_vertical, 0.0,
        ).child(field);

        // The placeholder lives in a local ZStack with the text field so
        // it shares the same column in the HStack — no overlap with
        // leading/trailing slots. The text field is the last ZStack child
        // so it wins hit-testing (ZStack tests children in reverse order).
        let text_column_id = if !self.placeholder.is_empty() {
            let ph = TextWidget::new(self.placeholder.clone())
                .bind_color(theme_signal.map(|t| t.colors.text_secondary))
                .a11y_hidden();
            let ph_id = ctx.add(
                Expand::new().fills_stack().child(
                    Padding::new(
                        field_style.padding_vertical, 0.0,
                        field_style.padding_vertical, 0.0,
                    ).child(ph),
                ),
            );
            let visible = text_signal_for_vis.map(|t| t.is_empty());
            ctx.visible_when(ph_id, visible);

            ctx.add(
                Expand::horizontal().child(
                    ZStack::new()
                        .add_child(ph_id)       // below (placeholder)
                        .child(padded_field),    // on top (text field, gets hits)
                ),
            )
        } else {
            ctx.add(Expand::horizontal().child(padded_field))
        };

        // HStack: [leading] [text_column] [clear] [trailing]
        let mut row = HStack::new().spacing(4.0);

        if let Some(leading) = self.leading_slot.take() {
            let leading_id = ctx.add_boxed(leading);
            row = row.add_child(leading_id);
        }

        row = row.add_child(text_column_id);

        // Clear button (opt-in). The clear affordance clears the
        // bound text signal — the field's ext→internal effect
        // picks this up and wipes the document.
        if self.show_clear_button {
            let icon = (crate::built_in_button::BuiltInIcons::global().clear)()
                .icon_size(12.0)
                .bind_color(theme_signal.map(|t| t.colors.text_secondary));
            let text_for_clear = self.text.clone();
            let clear_id = ctx.add(
                MinSize::new(16.0, 16.0)
                    .child(crate::primitives::Center::new().child(icon))
                    .on_tap(move |_pos, ctx| {
                        text_for_clear.set(String::new());
                        ctx.request_frame();
                    })
                    .cursor(CursorIcon::Pointer),
            );
            let visible = text_signal_for_vis.map(|t| !t.is_empty());
            ctx.visible_when(clear_id, visible);
            let reserve_id = ctx.add(
                crate::primitives::FixedSize::new()
                    .bind_width(16.0_f32)
                    .bind_height(16.0_f32)
                    .child_id(clear_id),
            );
            row = row.add_child(reserve_id);
        }

        if let Some(trailing) = self.trailing_slot.take() {
            let trailing_id = ctx.add_boxed(trailing);
            row = row.add_child(trailing_id);
        }

        let row_id = ctx.add(row);

        // Horizontal-only padding around the row.
        let padded_id = ctx.add(
            Padding::new(0.0, field_style.padding_horizontal, 0.0, field_style.padding_horizontal)
                .child_id(row_id),
        );

        // Border color + width depend on interaction state AND validation
        // state. `zip` produces a derived signal that registers both upstream
        // roots with the binding registry, so the border refreshes whenever
        // either source changes.
        //
        // This is the Int UI text-field convention (Section 7 of the v2
        // reference): emphasis lives in the field's own border — thicker
        // and accent-colored when focused — rather than in a separate ring
        // wrapping the control. A validation `Error` / `Warning` state
        // overrides the focus color so a user can't miss a broken field.
        let combined = interaction.zip(&validation);
        let _ = colors; // still kept as a snapshot; reactive derives use theme_signal below
        let border_color = derive_border_color(combined.clone(), theme_signal.clone());
        let border_width = combined.zip(&theme_signal).map(|((state, _val), t)| {
            if *state == InteractionState::Focused {
                t.shape.focus_ring_width
            } else {
                t.components.text_field.border_width
            }
        });

        let bg = RectWidget::new()
            .background(fern_tokens::SurfaceRole::Content)
            .border_color(border_color)
            .border_width(border_width)
            .corner_radius(CornerRadius::uniform(field_style.corner_radius));
        let bg_id = ctx.add(bg);

        let zstack = ZStack::new().add_child(bg_id).add_child(padded_id);
        let zstack_id = ctx.add(zstack);

        let root_id = ctx.add(
            MinSize::new(65.0, field_style.height).child_id(zstack_id),
        );

        // Tooltip.
        if let Some(source) = self.rich_tooltip_source.take() {
            tooltip::attach_rich_tooltip_source(
                ctx,
                root_id,
                source,
                tooltip::DEFAULT_RICH_TOOLTIP_DELAY,
            );
        } else if let Some(ref text) = self.tooltip_text {
            let tw = crate::tooltip::TooltipWidget::new_literal(text);
            let tooltip_id = ctx.add(tw);
            let delay = std::time::Duration::from_millis(500);
            ctx.attach_tooltip(root_id, tooltip_id, delay);
        }

        if !self.enabled {
            self.interaction.set(InteractionState::Disabled);
        }

        self.root_child_id = Some(root_id);
        vec![root_id]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        if let Some(p) = children.first_mut() {
            p.origin = Point::new(bounds.x, bounds.y);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // The inner TextInputField handles Role::TextInput.
        // The outer composite is transparent to a11y except for a
        // pass-through label.
        builder.set_role(fern_core::accesskit::Role::GenericContainer);
        if let Some(ref label) = self.label {
            builder.set_name(label);
        }
        if !self.enabled {
            builder.set_disabled();
        }
    }
}

/// Derive the border color from interaction state, validation state, and
/// the live theme signal. Combining with `theme_signal` means runtime theme
/// switches refresh the border alongside focus/error transitions — no
/// rebuild required.
fn derive_border_color(
    combined: Signal<(InteractionState, ValidationState)>,
    theme_signal: Signal<fern_tokens::Theme>,
) -> Signal<Color> {
    combined
        .zip(&theme_signal)
        .map(|((state, val), t)| match val {
            ValidationState::Error(_) => t.colors.border_error,
            ValidationState::Warning(_) => t.colors.border_warning,
            _ => match *state {
                InteractionState::Focused => t.colors.focus_ring,
                _ => t.colors.border,
            },
        })
}
