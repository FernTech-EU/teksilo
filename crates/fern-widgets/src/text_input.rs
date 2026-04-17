//! Single-line text input widget.
//!
//! `TextInput` is a one-line stripped-down `RichTextEditor`: it reuses the
//! same `TextDocument` + `TextCursor` + `RichTextEngine` stack but
//! restricts input to a single line of plain text. Newlines are stripped,
//! Enter fires `on_submit`, and Up/Down/PageUp/PageDown are ignored.
//!
//! The public `TextInput` is a composite widget that builds a visual
//! frame (border, focus ring, placeholder, clear button, leading/trailing
//! slots). The actual text editing is handled by an internal
//! `TextInputField` leaf widget.
//!
//! # Example
//!
//! ```ignore
//! let search = ctx.signal(String::new());
//! TextInput::new(search.clone())
//!     .placeholder("Search...")
//!     .show_clear_button(true)
//!     .leading_slot(IconWidget::from_svg(SEARCH_ICON))
//!     .on_submit(AppCmd::Search)
//! ```

mod field;
pub(crate) mod keyboard;
mod mouse;
pub(crate) mod state;

#[cfg(test)]
mod tests;

use std::rc::Rc;

use fern_canvas::{Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::app_command::AppCommand;
use fern_core::build_context::BuildContext;
use fern_core::signal::Signal;
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_builder::WidgetBuilder;
use fern_core::widget_id::WidgetId;
use fern_text::text_document::SelectionType;
use fern_tokens::{Color, ColorTokens, CornerRadius};

use crate::button::InteractionState;
use crate::primitives::{
    Expand, HStack, MinSize, Padding, RectWidget, TextWidget, ZStack,
};
use crate::tooltip::{self, RichTooltipSource};

use self::field::TextInputField;
use self::state::{CommandFactory, SharedState, TextInputState};

/// Validation state for the text input field.
#[derive(Debug, Clone, Default)]
pub enum ValidationState {
    #[default]
    None,
    Error(String),
    Warning(String),
}

/// A single-line text input widget.
///
/// See the [module-level documentation](self) for usage examples.
pub struct TextInput {
    // ── Configuration (builder methods, consumed in build) ───────────
    text: Signal<String>,
    placeholder: String,
    label: Option<String>,
    enabled: bool,
    read_only: bool,
    max_length: Option<usize>,
    show_clear_button: bool,
    leading_slot: Option<Box<dyn Widget>>,
    trailing_slot: Option<Box<dyn Widget>>,
    on_submit: Option<CommandFactory>,
    validation: Signal<ValidationState>,
    tooltip_text: Option<String>,
    rich_tooltip_source: Option<RichTooltipSource>,

    // ── Internal (set during build) ─────────────────────────────────
    state: Option<SharedState>,
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
            label: None,
            enabled: true,
            read_only: false,
            max_length: None,
            show_clear_button: false,
            leading_slot: None,
            trailing_slot: None,
            on_submit: None,
            validation: Signal::new(ValidationState::None),
            tooltip_text: None,
            rich_tooltip_source: None,
            state: None,
            interaction: Signal::new(InteractionState::Idle),
            root_child_id: None,
        }
    }

    // ── Builder methods ─────────────────────────────────────────────

    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }

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

    pub fn on_submit<C: AppCommand>(mut self, command: C) -> Self {
        self.on_submit = Some(Box::new(move |ctx: &mut EventContext| {
            ctx.emit(command.clone());
        }));
        self
    }

    pub fn on_submit_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.on_submit = Some(Box::new(f));
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
        let theme = ctx.theme().clone();
        let colors = theme.colors.clone();
        let field_style = theme.components.text_field;
        let interaction = self.interaction.clone();
        let validation = self.validation.clone();

        let initial_text = self.text.get();
        let on_submit = self.on_submit.take().map(Rc::new);

        // Create the shared state.
        let shared_state = TextInputState::new(
            &initial_text,
            self.max_length,
            self.read_only || !self.enabled,
            on_submit,
            self.placeholder.clone(),
        );
        self.state = Some(shared_state.clone());

        let text_signal = shared_state.borrow().text_signal.clone();

        // Sync external text signal → internal state. If the external
        // signal changes (e.g. programmatic set), update the document.
        {
            let ext = self.text.clone();
            let state_for_sync = shared_state.clone();
            ctx.effect(&ext, move |new_text| {
                let st = state_for_sync.borrow();
                let current = st.document.to_plain_text().unwrap_or_default();
                if current != *new_text {
                    st.cursor.select(SelectionType::Document);
                    let _ = st.cursor.insert_text(new_text);
                }
                drop(st);
            });
        }

        // Sync internal text signal → external text signal.
        {
            let ext = self.text.clone();
            ctx.effect(&text_signal, move |new_text| {
                if ext.get() != *new_text {
                    ext.set(new_text.clone());
                }
            });
        }

        // ── Build the child subtree ────────────────────────────────
        //
        // Layout structure (inside-out):
        //   FocusRing > MinSize(65, 28) > ZStack(bg_rect, content_row, placeholder)
        //
        // The content row uses horizontal-only padding. The text field
        // gets its own vertical padding so that slots (BuiltInButton etc.)
        // sit flush against top/bottom of the inner border area and are
        // vertically centered by the HStack — keeping the total height
        // at exactly field_style.height regardless of slot content.

        let inner_height = (field_style.height - 2.0 * field_style.border_width).max(0.0);
        let text_area_height = (inner_height - 2.0 * field_style.padding_vertical).max(0.0);

        // Text editing area, wrapped in vertical padding.
        let field = TextInputField {
            state: shared_state.clone(),
            text_height: text_area_height,
            interaction: interaction.clone(),
        };
        let padded_field = Padding::new(
            field_style.padding_vertical, 0.0, field_style.padding_vertical, 0.0,
        ).child(field);

        // The placeholder lives in a local ZStack with the text field so
        // it shares the same column in the HStack — no overlap with
        // leading/trailing slots. The text field is the last ZStack child
        // so it wins hit-testing (ZStack tests children in reverse order).
        let text_column_id = if !self.placeholder.is_empty() {
            let ph = TextWidget::new(self.placeholder.clone())
                .color(colors.text_secondary)
                .a11y_hidden();
            let ph_id = ctx.add(
                Expand::new().fills_stack().child(
                    Padding::new(
                        field_style.padding_vertical, 0.0,
                        field_style.padding_vertical, 0.0,
                    ).child(ph),
                ),
            );
            let visible = text_signal.map(|t| t.is_empty());
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

        // Leading slot.
        if let Some(leading) = self.leading_slot.take() {
            let leading_id = ctx.add_boxed(leading);
            row = row.add_child(leading_id);
        }

        row = row.add_child(text_column_id);

        // Clear button (opt-in). Uses the framework's built-in clear
        // icon (SVG) rather than a hand-drawn path: the previous
        // in-file `clear_icon` produced an `×` with two open line
        // subpaths and fed them to `Canvas::fill_path`, which fills
        // the enclosed area only — unclosed lines fill nothing, so
        // the button sat visible but empty on screen.
        //
        // The interactive affordance (`clear_id`) is wrapped in a
        // fixed-size reservation (`reserve_id`): `visible_when` sets
        // `clear_id` dormant when the field is empty, and dormant
        // widgets collapse to zero size. Without the outer wrapper
        // the text row width would oscillate as the user typed (empty
        // → narrow; first character → suddenly 16 px wider). The
        // `FixedSize` always reports 16×16, so row width stays
        // stable regardless of whether the inner affordance is alive.
        if self.show_clear_button {
            let icon = (crate::built_in_button::BuiltInIcons::global().clear)()
                .icon_size(12.0)
                .color(colors.text_secondary);
            let state_for_clear = shared_state.clone();
            let clear_id = ctx.add(
                MinSize::new(16.0, 16.0)
                    .child(crate::primitives::Center::new().child(icon))
                    .on_tap(move |_pos, ctx| {
                        let st = state_for_clear.borrow();
                        st.cursor.select(SelectionType::Document);
                        let _ = st.cursor.remove_selected_text();
                        drop(st);
                        state::sync_cursor_signals(&state_for_clear);
                        ctx.request_frame();
                    })
                    .cursor(CursorIcon::Pointer),
            );
            let visible = text_signal.map(|t| !t.is_empty());
            ctx.visible_when(clear_id, visible);
            let reserve_id = ctx.add(
                crate::primitives::FixedSize::new()
                    .bind_width(16.0_f32)
                    .bind_height(16.0_f32)
                    .child_id(clear_id),
            );
            row = row.add_child(reserve_id);
        }

        // Trailing slot.
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

        // Combined signal for border color: merges interaction + validation.
        // We can't use Signal::map2 (doesn't exist), so we use a combined
        // Signal<(InteractionState, ValidationState)> updated by observers
        // on both source signals.
        let combined = Signal::new((interaction.get(), validation.get()));
        {
            let combined_for_int = combined.clone();
            let val_for_int = validation.clone();
            ctx.effect(&interaction, move |state| {
                combined_for_int.set((*state, val_for_int.get()));
            });
        }
        {
            let combined_for_val = combined.clone();
            let int_for_val = interaction.clone();
            ctx.effect(&validation, move |val| {
                combined_for_val.set((int_for_val.get(), val.clone()));
            });
        }
        let border_color = derive_border_color(combined.clone(), &colors);

        // Border width: 2px when focused (accent ring covering the border),
        // 1px otherwise. Same combined signal drives both color and width.
        let normal_bw = field_style.border_width;
        let focus_ring_width = theme.shape.focus_ring_width;
        let border_width = combined.map(move |(state, _val)| match state {
            InteractionState::Focused => focus_ring_width,
            _ => normal_bw,
        });

        let bg = RectWidget::new()
            .background(colors.surface_content)
            .bind_border_color(border_color)
            .bind_border_width(border_width)
            .corner_radius(CornerRadius::uniform(field_style.corner_radius));
        let bg_id = ctx.add(bg);

        // ZStack: background + content row.
        // The placeholder is inside the text column (local ZStack in HStack),
        // not a sibling here.
        let zstack = ZStack::new().add_child(bg_id).add_child(padded_id);
        let zstack_id = ctx.add(zstack);

        // MinSize — no FocusRing wrapper. The border itself becomes the
        // focus indicator (thicker + accent colored), matching Int UI
        // text field convention.
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

        // No handlers on root_id — the inner TextInputField drives the
        // interaction signal directly from its own on_focus handler.

        if !self.enabled {
            interaction.set(InteractionState::Disabled);
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
        // The outer composite is transparent to a11y.
        builder.set_role(fern_core::accesskit::Role::GenericContainer);
        if let Some(ref label) = self.label {
            builder.set_name(label);
        }
        if !self.enabled {
            builder.set_disabled();
        }
    }
}

/// Derive the border color from a combined (interaction, validation) signal.
fn derive_border_color(
    combined: Signal<(InteractionState, ValidationState)>,
    colors: &ColorTokens,
) -> Signal<Color> {
    let border = colors.border;
    let focus_ring = colors.focus_ring;
    let border_error = colors.border_error;
    let border_warning = colors.border_warning;

    combined.map(move |(state, val)| match val {
        ValidationState::Error(_) => border_error,
        ValidationState::Warning(_) => border_warning,
        _ => match state {
            // Focused: accent ring covering the border.
            InteractionState::Focused => focus_ring,
            _ => border,
        },
    })
}

