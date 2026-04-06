//! ComboBox — dropdown selection widget.
//!
//! Non-generic, index-based selection using `Signal<Option<usize>>`.
//! Opens a dropdown overlay with selectable items.
//! The dropdown panel is pre-created during build() and kept dormant until opened.

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
use fern_core::signal::Signal;
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, CornerRadius};

use crate::primitives::{
    HStack, IconWidget, Padding, RectWidget, Spacer, TextWidget, VStack, ZStack,
};

/// Interaction state for the trigger button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComboBoxState {
    Idle,
    Hovered,
    Focused,
    Open,
    Disabled,
}

/// A dropdown selection widget.
///
/// ```ignore
/// let selected = ctx.signal(None::<usize>);
/// ComboBox::new(vec!["Apple", "Banana", "Cherry"], selected.clone())
///     .placeholder("Select a fruit...")
/// ```
pub struct ComboBox {
    items: Vec<String>,
    selected: Signal<Option<usize>>,
    placeholder: String,
    enabled: bool,
    // Build state
    interaction: Signal<ComboBoxState>,
    root_child_id: Option<WidgetId>,
    dropdown_content_id: Option<WidgetId>,
}

impl ComboBox {
    pub fn new(
        items: impl IntoIterator<Item = impl Into<String>>,
        selected: Signal<Option<usize>>,
    ) -> Self {
        Self {
            items: items.into_iter().map(|s| s.into()).collect(),
            selected,
            placeholder: String::new(),
            enabled: true,
            interaction: Signal::new(ComboBoxState::Idle),
            root_child_id: None,
            dropdown_content_id: None,
        }
    }

    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

impl std::fmt::Debug for ComboBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComboBox")
            .field("items", &self.items.len())
            .field("enabled", &self.enabled)
            .finish()
    }
}

fn resolve_bg(state: ComboBoxState, colors: &fern_tokens::ColorTokens) -> Color {
    match state {
        ComboBoxState::Idle | ComboBoxState::Focused => colors.surface,
        ComboBoxState::Hovered => colors.on_surface.with_alpha(0.04),
        ComboBoxState::Open => colors.on_surface.with_alpha(0.04),
        ComboBoxState::Disabled => colors.disabled_fill,
    }
}

fn resolve_border(state: ComboBoxState, colors: &fern_tokens::ColorTokens) -> Color {
    match state {
        ComboBoxState::Focused => colors.focus_ring,
        ComboBoxState::Disabled => colors.disabled_fill,
        _ => colors.border,
    }
}

fn resolve_text(state: ComboBoxState, colors: &fern_tokens::ColorTokens) -> Color {
    match state {
        ComboBoxState::Disabled => colors.disabled_text,
        _ => colors.on_surface,
    }
}

/// A single item row in the dropdown (internal widget).
#[derive(Debug)]
struct DropdownItem {
    label: String,
    index: usize,
    selected_signal: Signal<Option<usize>>,
    hovered: Signal<bool>,
    root_child_id: Option<WidgetId>,
}

impl Widget for DropdownItem {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let hovered = self.hovered.clone();
        let selected_signal = self.selected_signal.clone();
        let index = self.index;

        let bg_color = hovered.map({
            let on_surface = theme.colors.on_surface;
            move |h| {
                if *h {
                    on_surface.with_alpha(0.08)
                } else {
                    Color::TRANSPARENT
                }
            }
        });

        let text = TextWidget::new(&self.label)
            .style(theme.typography.body.clone())
            .color(theme.colors.on_surface);
        let text_id = ctx.add(text);

        let padding = Padding::symmetric(6.0, 12.0).set_child(text_id);
        let padding_id = ctx.add(padding);

        let bg = RectWidget::new().bind_background(bg_color);
        let bg_id = ctx.add(bg);

        let zstack = ZStack::new().add_child(bg_id).add_child(padding_id);
        let root_id = ctx.add(zstack);
        self.root_child_id = Some(root_id);

        let hovered_enter = hovered.clone();
        let hovered_leave = hovered.clone();

        let handler_set = HandlerSet::new()
            .on_tap(move |ctx: &mut EventContext| {
                selected_signal.set(Some(index));
                ctx.dismiss_all_overlays();
            })
            .on_hover(move |entered: bool, _ctx: &mut EventContext| {
                if entered {
                    hovered_enter.set(true);
                } else {
                    hovered_leave.set(false);
                }
            })
            .cursor(CursorIcon::Pointer);

        ctx.apply_self_handlers(handler_set);

        vec![root_id]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        match self.root_child_id {
            Some(id) => {
                let s = ctx
                    .child_size(id, proposal)
                    .unwrap_or_else(|| proposal.resolve(0.0, 0.0));
                Size::new(s.width, s.height.max(32.0))
            }
            None => proposal.resolve(120.0, 32.0),
        }
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

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::ListBoxOption);
        builder.set_name(&self.label);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

/// Dropdown panel content (internal widget — shown as overlay).
#[derive(Debug)]
struct DropdownPanel {
    items: Vec<String>,
    selected: Signal<Option<usize>>,
    root_child_id: Option<WidgetId>,
}

impl Widget for DropdownPanel {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();

        let mut vstack = VStack::new();
        for (i, label) in self.items.iter().enumerate() {
            let item = DropdownItem {
                label: label.clone(),
                index: i,
                selected_signal: self.selected.clone(),
                hovered: Signal::new(false),
                root_child_id: None,
            };
            vstack = vstack.child(item);
        }
        let vstack_id = ctx.add(vstack);

        let padding = Padding::uniform(4.0).set_child(vstack_id);
        let padding_id = ctx.add(padding);

        let bg = RectWidget::new()
            .background(theme.colors.surface)
            .border_color(theme.colors.border.with_alpha(0.3))
            .border_width(1.0)
            .corner_radius(CornerRadius::uniform(theme.shape.radius_sm));
        let bg_id = ctx.add(bg);

        let zstack = ZStack::new().add_child(bg_id).add_child(padding_id);
        let root_id = ctx.add(zstack);
        self.root_child_id = Some(root_id);

        let handler_set = HandlerSet::new().focusable(true);
        ctx.apply_self_handlers(handler_set);

        vec![root_id]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        match self.root_child_id {
            Some(id) => ctx
                .child_size(id, proposal)
                .unwrap_or_else(|| proposal.resolve(120.0, 0.0)),
            None => proposal.resolve(120.0, 0.0),
        }
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

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::ListBox);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

impl Widget for ComboBox {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let enabled = self.enabled;

        let interaction = ctx.signal(if enabled {
            ComboBoxState::Idle
        } else {
            ComboBoxState::Disabled
        });
        self.interaction = interaction.clone();

        // Derive label text from selected signal
        let items_for_label = self.items.clone();
        let placeholder = self.placeholder.clone();
        let label_text = self.selected.map(move |sel| match sel {
            Some(idx) if *idx < items_for_label.len() => items_for_label[*idx].clone(),
            _ => placeholder.clone(),
        });

        let bg_color = {
            let colors = theme.colors.clone();
            interaction.map(move |s| resolve_bg(*s, &colors))
        };
        let border_color = {
            let colors = theme.colors.clone();
            interaction.map(move |s| resolve_border(*s, &colors))
        };
        let text_color = {
            let colors = theme.colors.clone();
            interaction.map(move |s| resolve_text(*s, &colors))
        };

        // Build trigger: [label | Spacer | chevron]
        let label = TextWidget::new("")
            .style(theme.typography.body.clone())
            .bind_text(label_text)
            .bind_color(text_color);
        let label_id = ctx.add(label);

        let chevron = IconWidget::chevron_down(12.0).color(theme.colors.on_surface.with_alpha(0.5));
        let chevron_id = ctx.add(chevron);

        let row = HStack::new()
            .spacing(8.0)
            .add_child(label_id)
            .child(Spacer::new())
            .add_child(chevron_id);
        let row_id = ctx.add(row);

        let padding = Padding::symmetric(
            theme.spacing.widget_padding,
            theme.spacing.widget_padding * 1.5,
        )
        .set_child(row_id);
        let padding_id = ctx.add(padding);

        let bg = RectWidget::new()
            .bind_background(bg_color)
            .bind_border_color(border_color)
            .border_width(theme.shape.border_width)
            .corner_radius(CornerRadius::uniform(theme.shape.radius_sm));
        let bg_id = ctx.add(bg);

        let zstack = ZStack::new().add_child(bg_id).add_child(padding_id);
        let root_id = ctx.add(zstack);
        self.root_child_id = Some(root_id);

        // Pre-create the dropdown panel (dormant until opened)
        let dropdown_panel = DropdownPanel {
            items: self.items.clone(),
            selected: self.selected.clone(),
            root_child_id: None,
        };
        let dropdown_id = ctx.add(dropdown_panel);
        self.dropdown_content_id = Some(dropdown_id);
        ctx.set_dormant(dropdown_id);

        // --- Handlers ---
        let self_id = ctx.self_id();
        let int_hover = interaction.clone();
        let int_focus = interaction.clone();

        let handler_set = HandlerSet::new()
            .on_tap({
                let interaction = interaction.clone();
                let dropdown_id = dropdown_id;
                move |ctx: &mut EventContext| {
                    if !enabled {
                        return;
                    }
                    interaction.set(ComboBoxState::Open);
                    ctx.activate(dropdown_id);
                    ctx.show_overlay(OverlayRequest {
                        content_id: dropdown_id,
                        anchor: self_id,
                        placement: OverlayPlacement::Below,
                        dismiss: DismissBehavior::ClickOutside,
                        layer: OverlayLayer::InTree,
                        parent_overlay: None,
                    });
                }
            })
            .on_hover(move |entered: bool, _ctx: &mut EventContext| {
                if !enabled {
                    return;
                }
                let current = int_hover.get();
                if current == ComboBoxState::Open {
                    return;
                }
                if entered {
                    int_hover.set(ComboBoxState::Hovered);
                } else {
                    int_hover.set(ComboBoxState::Idle);
                }
            })
            .on_key({
                let interaction = interaction.clone();
                let items_len = self.items.len();
                let selected = self.selected.clone();
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    if !enabled {
                        return EventResponse::Ignored;
                    }
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::Enter | Key::Space,
                            ..
                        } => {
                            interaction.set(ComboBoxState::Open);
                            ctx.activate(dropdown_id);
                            ctx.show_overlay(OverlayRequest {
                                content_id: dropdown_id,
                                anchor: self_id,
                                placement: OverlayPlacement::Below,
                                dismiss: DismissBehavior::ClickOutside,
                                layer: OverlayLayer::InTree,
                                parent_overlay: None,
                            });
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::ArrowDown,
                            ..
                        } => {
                            let current = selected.get().unwrap_or(0);
                            let next = if current + 1 >= items_len {
                                0
                            } else {
                                current + 1
                            };
                            selected.set(Some(next));
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::ArrowUp, ..
                        } => {
                            let current = selected.get().unwrap_or(0);
                            let next = if current == 0 {
                                items_len.saturating_sub(1)
                            } else {
                                current - 1
                            };
                            selected.set(Some(next));
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            })
            .on_focus(move |gained: bool, _ctx: &mut EventContext| {
                if gained {
                    let current = int_focus.get();
                    if current == ComboBoxState::Idle {
                        int_focus.set(ComboBoxState::Focused);
                    }
                } else {
                    int_focus.set(ComboBoxState::Idle);
                }
            })
            .focusable(enabled)
            .cursor(CursorIcon::Pointer);

        ctx.apply_self_handlers(handler_set);

        vec![root_id]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        match self.root_child_id {
            Some(id) => {
                let child_size = ctx
                    .child_size(id, proposal)
                    .unwrap_or_else(|| proposal.resolve(0.0, 0.0));
                Size::new(child_size.width.max(120.0), child_size.height.max(36.0))
            }
            None => proposal.resolve(120.0, 36.0),
        }
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

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::ComboBox);
        if !self.enabled {
            builder.set_disabled();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;

    #[test]
    fn combo_box_builds_and_lays_out() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let selected = Signal::new(None);
        let cb = tree.add(
            ComboBox::new(vec!["Apple", "Banana", "Cherry"], selected.clone())
                .placeholder("Select..."),
        );
        tree.layout(SizeProposal::exact(300.0, 50.0));
        let bounds = tree.bounds(cb);
        assert!(bounds.width >= 120.0);
        assert!(bounds.height >= 36.0);
    }

    #[test]
    fn combo_box_accessibility_role() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let selected = Signal::new(None);
        let cb = tree.add(ComboBox::new(vec!["A", "B"], selected.clone()));
        tree.layout(SizeProposal::exact(200.0, 50.0));
        let info = tree.accessibility_node(cb);
        assert_eq!(info.role(), fern_core::accesskit::Role::ComboBox);
    }

    #[test]
    fn arrow_keys_cycle_selection() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let selected = Signal::new(None::<usize>);
        let cb = tree.add(ComboBox::new(
            vec!["Apple", "Banana", "Cherry"],
            selected.clone(),
        ));
        tree.layout(SizeProposal::exact(300.0, 50.0));
        tree.focus(cb);

        // ArrowDown: None → unwrap_or(0) → next = 1
        tree.press_key(Key::ArrowDown, fern_core::event::Modifiers::NONE);
        assert_eq!(selected.get(), Some(1));

        tree.press_key(Key::ArrowDown, fern_core::event::Modifiers::NONE);
        assert_eq!(selected.get(), Some(2));

        // Wraps around
        tree.press_key(Key::ArrowDown, fern_core::event::Modifiers::NONE);
        assert_eq!(selected.get(), Some(0));
    }

    #[test]
    fn selected_updates_label() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let selected = Signal::new(Some(1_usize));
        let cb = tree.add(ComboBox::new(
            vec!["Apple", "Banana", "Cherry"],
            selected.clone(),
        ));
        tree.layout(SizeProposal::exact(300.0, 50.0));
        assert!(tree.bounds(cb).width > 0.0);
    }

    #[test]
    fn click_opens_overlay() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let selected = Signal::new(None::<usize>);
        let cb = tree.add(ComboBox::new(
            vec!["Apple", "Banana", "Cherry"],
            selected.clone(),
        ));
        tree.layout(SizeProposal::exact(300.0, 200.0));

        // No overlays initially
        assert!(tree.active_overlays().is_empty());

        // Click the combo box
        tree.click(cb);
        tree.layout(SizeProposal::exact(300.0, 200.0));

        // Overlay should be open
        assert_eq!(tree.active_overlays().len(), 1);
    }
}
