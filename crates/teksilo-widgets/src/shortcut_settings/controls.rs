// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The pieces a shortcut row is made of: the control every focusable thing
//! in the list is built on, the key caps a chord is drawn with, the dot that
//! marks a changed row and the heading of a group.

use std::rc::Rc;

use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::shortcut::KeyStroke;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{ButtonStyleConfig, ButtonVariant, SharedButtonStyle};
use teksilo_core::widget::{
    EventContext, LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement,
};
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::{LocalizedString, lit};
use teksilo_tokens::{BorderRole, SurfaceRole, TextRole, TextStyleRole};

use super::state::{FocusKey, Shared};
use crate::button::InteractionState;
use crate::keystroke_format::keystroke_parts;
use crate::panel::Panel;
use crate::primitives::{HStack, Padding, TextWidget};

/// A button whose content is any widget: a chord's key caps, an icon, a
/// line of text. Its chrome comes from the theme's `ButtonStyle`, so the
/// list looks like the rest of the application under every preset.
///
/// It exists, rather than `Button`, for two reasons. `Button`'s content is
/// a label, and a chord is a row of key caps. And the list must know which
/// of its controls holds focus, to give focus back after it rebuilds; the
/// handler set has one focus slot, which `Button` already fills.
pub(super) struct RowButton {
    content: Option<Box<dyn Widget>>,
    variant: ButtonVariant,
    /// Draw the focus ring whether or not the control has focus: the
    /// recording field's "listening" state.
    ring: bool,
    name: String,
    description: Option<String>,
    key: FocusKey,
    shared: Rc<Shared>,
    on_activate: Rc<dyn Fn(&mut EventContext)>,
    tooltip: Option<LocalizedString>,
    root: Option<WidgetId>,
}

impl std::fmt::Debug for RowButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RowButton")
            .field("name", &self.name)
            .field("key", &self.key)
            .finish()
    }
}

impl RowButton {
    pub(super) fn new(
        key: FocusKey,
        shared: Rc<Shared>,
        content: impl Widget + 'static,
        name: impl Into<String>,
        on_activate: impl Fn(&mut EventContext) + 'static,
    ) -> Self {
        Self {
            content: Some(Box::new(content)),
            variant: ButtonVariant::Ghost,
            ring: false,
            name: name.into(),
            description: None,
            key,
            shared,
            on_activate: Rc::new(on_activate),
            tooltip: None,
            root: None,
        }
    }

    pub(super) fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub(super) fn ring(mut self, on: bool) -> Self {
        self.ring = on;
        self
    }

    pub(super) fn description(mut self, description: Option<String>) -> Self {
        self.description = description;
        self
    }

    pub(super) fn tooltip(mut self, tooltip: LocalizedString) -> Self {
        self.tooltip = Some(tooltip);
        self
    }
}

impl Widget for RowButton {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let style: SharedButtonStyle =
            ctx.theme().style_slots.button.clone().unwrap_or_else(|| {
                Rc::new(crate::styles::RecipeButtonStyle::for_tokens(
                    &ctx.theme().input,
                ))
            });
        let interaction = Signal::new(InteractionState::Idle);
        let content = self
            .content
            .take()
            .unwrap_or_else(|| Box::new(HStack::new()));
        let content_id = ctx.add_boxed(content);

        let self_id = ctx.self_id();
        let is_pressed = interaction.map(|s| matches!(s, InteractionState::Pressed));
        let is_hovered = interaction.map(|s| matches!(s, InteractionState::Hovered));
        let is_focused = if self.ring {
            Signal::new(true)
        } else {
            interaction
                .map(|s| matches!(s, InteractionState::Focused))
                .and(&ctx.focus_visible())
        };
        let is_disabled = ctx.effective_enabled_signal(self_id).map(|on| !*on);
        let cfg = ButtonStyleConfig {
            label: content_id,
            is_pressed,
            is_hovered,
            is_focused,
            is_disabled,
            variant: self.variant,
        };
        let root = style.make_body(&cfg, ctx);
        if let Some(tooltip) = self.tooltip.clone() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_plain_tooltip(ctx, root, tooltip, delay);
        }

        let key = self.key;
        let shared = self.shared.clone();
        let observer: Rc<dyn Fn(bool, &mut EventContext)> =
            Rc::new(move |gained: bool, _ctx: &mut EventContext| {
                if gained {
                    shared.focus.set(Some(key));
                    if matches!(key, FocusKey::Slot(_) | FocusKey::Reset(_))
                        && shared.roving.get() != Some(key)
                    {
                        shared.roving.set(Some(key));
                    }
                    return;
                }
                if shared.focus.get() == Some(key) {
                    shared.focus.set(None);
                }
                // A recording ends when its control loses focus, so a stray
                // click elsewhere cannot leave the next keystroke captured.
                if let FocusKey::Slot(target) = key
                    && shared.capturing.get() == Some(target)
                {
                    shared.cancel_capture();
                }
            });
        let handlers = crate::button::build_interaction_handlers_with_focus(
            ctx,
            interaction,
            self.on_activate.clone(),
            true,
            Some(observer),
        );
        ctx.apply_self_handlers(handlers);

        self.root = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root
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

    fn children(&self) -> Vec<WidgetId> {
        self.root.into_iter().collect()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::Button);
        builder.set_name(self.name.as_str());
        if let Some(description) = &self.description {
            builder.set_description(description.as_str());
        }
        builder.add_action(teksilo_core::accesskit::Action::Click);
        builder.add_action(teksilo_core::accesskit::Action::Focus);
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

/// A chord drawn as one key cap per key: `[Ctrl] [Shift] [S]`.
///
/// Hidden from assistive technology: the control it sits in names the
/// chord in words.
pub(super) fn key_caps(ks: KeyStroke) -> HStack {
    let mut row = HStack::new().spacing(3.0);
    for part in keystroke_parts(ks) {
        row = row.child(
            Panel::new()
                .a11y_presentational()
                .background(SurfaceRole::Raised)
                .border_color(BorderRole::Strong)
                .border_width(1.0)
                .corner_radius(4.0)
                .padding(0.0)
                .child(
                    Padding::symmetric(1.0, 5.0).child(
                        TextWidget::new(lit!(part))
                            .style(TextStyleRole::Small)
                            .color(TextRole::Primary)
                            .single_line()
                            .no_shrink()
                            .a11y_hidden(),
                    ),
                ),
        );
    }
    row
}

/// One line of muted or coloured text inside a control or a detail line,
/// hidden from assistive technology (the control or the line names it).
pub(super) fn quiet_text(text: impl Into<String>, role: TextRole) -> TextWidget {
    TextWidget::new(lit!(text.into()))
        .style(TextStyleRole::Small)
        .color(role)
        .single_line()
        .a11y_hidden()
}

/// The dot in front of a row whose shortcut the user changed. Always takes
/// its space, painted or not, so marking a row never moves its text.
#[derive(Debug)]
pub(super) struct ModifiedDot {
    modified: bool,
}

/// The dot's diameter.
pub(super) const DOT: f32 = 6.0;

impl ModifiedDot {
    pub(super) fn new(modified: bool) -> Self {
        Self { modified }
    }
}

impl Widget for ModifiedDot {
    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::new(DOT, DOT).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        if !self.modified {
            return;
        }
        let center = Point::new(
            bounds.x + bounds.width / 2.0,
            bounds.y + bounds.height / 2.0,
        );
        canvas.fill_circle(center, DOT / 2.0, ctx.theme.colors.accent);
    }
}

/// A group's title, published as a `Role::Heading` so a screen reader can
/// jump from group to group. The visible label is hidden beneath it: a
/// heading is not a text-range node, so the runs would go unheard there
/// anyway, and the heading's name already says it.
#[derive(Debug)]
pub(super) struct GroupHeading {
    title: String,
    child: Option<WidgetId>,
}

impl GroupHeading {
    pub(super) fn new(title: String) -> Self {
        Self { title, child: None }
    }
}

impl Widget for GroupHeading {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.add(
            TextWidget::new(lit!(self.title.clone()))
                .style(TextStyleRole::BodyBold)
                .color(TextRole::Primary)
                .single_line()
                .a11y_hidden(),
        );
        self.child = Some(id);
        vec![id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.child
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

    fn children(&self) -> Vec<WidgetId> {
        self.child.into_iter().collect()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::Heading);
        builder.set_name(self.title.as_str());
        builder.set_level(2);
    }
}

/// Takes the whole width it is offered and keeps its child at the leading
/// edge, vertically centred.
///
/// A button's chrome centres a label narrower than its minimum width, so a
/// `+`, an `F3` and a `Ctrl Y` placed in the same column would each start
/// somewhere else. Inside this, they all start at the column's edge.
/// Measured with no width offered, it is exactly as wide as its child, so
/// the column is still sized by the widest chord.
#[derive(Debug)]
pub(super) struct LeadingFill {
    child: Option<Box<dyn Widget>>,
    child_id: Option<WidgetId>,
}

impl LeadingFill {
    pub(super) fn new(child: impl Widget + 'static) -> Self {
        Self {
            child: Some(Box::new(child)),
            child_id: None,
        }
    }
}

impl Widget for LeadingFill {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(child) = self.child.take() {
            self.child_id = Some(ctx.add_boxed(child));
        }
        self.child_id.into_iter().collect()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let child = self
            .child_id
            .and_then(|id| ctx.child_size(id, SizeProposal::unspecified()))
            .unwrap_or(Size::ZERO);
        Size::new(
            proposal.width.unwrap_or(child.width).max(child.width),
            child.height,
        )
        .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            let size = ctx
                .child_size(child.id, SizeProposal::unspecified())
                .unwrap_or(Size::ZERO);
            let x = if ctx.is_rtl() {
                bounds.x + bounds.width - size.width
            } else {
                bounds.x
            };
            child.origin = Point::new(x, bounds.y + (bounds.height - size.height) / 2.0);
            child.size = size;
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}
