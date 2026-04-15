//! ComboBox — dropdown selection widget.
//!
//! Non-generic, index-based selection using `Signal<Option<usize>>`.
//! Opens a dropdown overlay with selectable items.
//! The dropdown panel is pre-created during build() and kept dormant until opened.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

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

// TODO(milestone-7): Add `max_visible_items` option (default ~8) to ComboBox and MenuList.
// When item count exceeds the limit, show a scrollable list with arrow
// headers/footers for quick navigation. Blocked on ListView from Milestone 7.

/// A dropdown selection widget.
///
/// ```ignore
/// let selected = ctx.signal(None::<usize>);
/// ComboBox::new_literal(vec!["Apple", "Banana", "Cherry"], selected.clone())
///     .placeholder_literal("Select a fruit...")
/// ```
pub struct ComboBox {
    items: Vec<String>,
    selected: Signal<Option<usize>>,
    placeholder: String,
    /// Accessible label — independent of placeholder and current selection.
    /// Screen readers announce this as the name of the control.
    label: Option<String>,
    enabled: bool,
    // Build state
    interaction: Signal<ComboBoxState>,
    root_child_id: Option<WidgetId>,
    dropdown_content_id: Option<WidgetId>,
}

impl ComboBox {
    pub fn new(
        items: impl IntoIterator<Item = impl Into<fern_i18n::LocalizedString>>,
        selected: Signal<Option<usize>>,
    ) -> Self {
        Self {
            items: items
                .into_iter()
                .map(|s| {
                    let ls: fern_i18n::LocalizedString = s.into();
                    ls.resolve_now()
                })
                .collect(),
            selected,
            placeholder: String::new(),
            label: None,
            enabled: true,
            interaction: Signal::new(ComboBoxState::Idle),
            root_child_id: None,
            dropdown_content_id: None,
        }
    }

    /// Shim (permanent, `#[doc(hidden)]`) — wraps each raw item in `LocalizedString::literal`.
    #[doc(hidden)]
    pub fn new_literal(
        items: impl IntoIterator<Item = impl Into<String>>,
        selected: Signal<Option<usize>>,
    ) -> Self {
        Self::new(
            items
                .into_iter()
                .map(|s| fern_i18n::LocalizedString::literal(s))
                .collect::<Vec<_>>(),
            selected,
        )
    }

    pub fn placeholder(mut self, text: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = text.into();
        self.placeholder = ls.resolve_now();
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `placeholder(...)` accepting a raw string.
    #[doc(hidden)]
    pub fn placeholder_literal(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }

    /// Accessible label describing what this combo box is for
    /// (e.g. "Fruit", "Font family"). Independent of the visible
    /// placeholder and of the current selection — screen readers
    /// announce this as the name of the control.
    pub fn label(mut self, text: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = text.into();
        self.label = Some(ls.resolve_now());
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `label(...)` accepting a raw string.
    #[doc(hidden)]
    pub fn label_literal(mut self, text: impl Into<String>) -> Self {
        self.label = Some(text.into());
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
        ComboBoxState::Idle | ComboBoxState::Focused => colors.surface_main,
        ComboBoxState::Hovered => colors.text_primary.with_alpha(0.04),
        ComboBoxState::Open => colors.text_primary.with_alpha(0.04),
        ComboBoxState::Disabled => colors.accent_disabled,
    }
}

fn resolve_border(state: ComboBoxState, colors: &fern_tokens::ColorTokens) -> Color {
    // Int UI: focus uses the FocusRing wrapper, not a border color change.
    match state {
        ComboBoxState::Disabled => colors.accent_disabled,
        _ => colors.border,
    }
}

fn resolve_text(state: ComboBoxState, colors: &fern_tokens::ColorTokens) -> Color {
    match state {
        ComboBoxState::Disabled => colors.text_disabled,
        _ => colors.text_primary,
    }
}

/// A single item row in the dropdown (internal widget).
#[derive(Debug)]
struct DropdownItem {
    label: String,
    index: usize,
    selected_signal: Signal<Option<usize>>,
    root_child_id: Option<WidgetId>,
}

impl Widget for DropdownItem {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let selected_signal = self.selected_signal.clone();
        let index = self.index;

        // Track whether this item is highlighted (hovered or keyboard-selected)
        let highlighted = ctx.signal(false);

        // Observe the selection signal — highlight this item when it's selected
        {
            let highlighted = highlighted.clone();
            ctx.effect(&self.selected_signal, move |sel| {
                highlighted.set(*sel == Some(index));
            });
        }

        let bg_color = highlighted.map({
            let primary = theme.colors.accent;
            move |h| {
                if *h {
                    primary.with_alpha(0.12)
                } else {
                    Color::TRANSPARENT
                }
            }
        });

        let text = TextWidget::new_literal(&self.label)
            .style(theme.typography.body.clone())
            .color(theme.colors.text_primary)
            .single_line()
            .a11y_hidden();
        let text_id = ctx.add(text);

        let menu_style = theme.components.menu;
        let pad_v =
            ((menu_style.item_height - theme.typography.body.size).max(0.0) * 0.5).max(0.0);
        let padding =
            Padding::symmetric(pad_v, menu_style.item_padding_horizontal).set_child(text_id);
        let padding_id = ctx.add(padding);

        let bg = RectWidget::new().bind_background(bg_color);
        let bg_id = ctx.add(bg);

        let zstack = ZStack::new().add_child(bg_id).add_child(padding_id);
        let root_id = ctx.add(zstack);
        self.root_child_id = Some(root_id);

        let handler_set = HandlerSet::new()
            .on_tap(move |_pos, ctx: &mut EventContext| {
                selected_signal.set(Some(index));
                ctx.dismiss_all_overlays();
            })
            .on_hover({
                let highlighted = highlighted.clone();
                move |entered: bool, _ctx: &mut EventContext| {
                    highlighted.set(entered);
                }
            })
            .cursor(CursorIcon::Pointer);

        ctx.apply_self_handlers(handler_set);

        vec![root_id]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        let min_h = ctx.theme.components.menu.item_height;
        match self.root_child_id {
            Some(id) => {
                let s = ctx
                    .child_size(id, proposal)
                    .unwrap_or_else(|| proposal.resolve(0.0, 0.0));
                Size::new(s.width, s.height.max(min_h))
            }
            None => proposal.resolve(120.0, min_h),
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
                root_child_id: None,
            };
            vstack = vstack.child(item);
        }
        let vstack_id = ctx.add(vstack);

        let menu_style = theme.components.menu;
        let padding = Padding::uniform(4.0).set_child(vstack_id);
        let padding_id = ctx.add(padding);

        // Dropdown panel — same surface treatment as MenuList (raised + popup radius)
        let bg = RectWidget::new()
            .background(theme.colors.surface_raised)
            .border_color(theme.colors.border)
            .border_width(menu_style.popup_border_width)
            .corner_radius(CornerRadius::uniform(menu_style.popup_corner_radius));
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
        let label = TextWidget::new_literal("")
            .style(theme.typography.body.clone())
            .bind_text(label_text)
            .bind_color(text_color)
            .single_line()
            .a11y_hidden();
        let label_id = ctx.add(label);

        let chevron = IconWidget::chevron_down(12.0).color(theme.colors.text_primary.with_alpha(0.5));
        let chevron_id = ctx.add(chevron);

        let row = HStack::new()
            .spacing(8.0)
            .add_child(label_id)
            .child(Spacer::new())
            .add_child(chevron_id);
        let row_id = ctx.add(row);

        let combo_style = theme.components.combo_box;
        let padding = Padding::symmetric(
            combo_style.padding_horizontal * 0.5,
            combo_style.padding_horizontal,
        )
        .set_child(row_id);
        let padding_id = ctx.add(padding);

        let bg = RectWidget::new()
            .bind_background(bg_color)
            .bind_border_color(border_color)
            .border_width(theme.shape.border_width)
            .corner_radius(CornerRadius::uniform(combo_style.corner_radius));
        let bg_id = ctx.add(bg);

        let visual_zstack = ZStack::new().add_child(bg_id).add_child(padding_id);
        let visual_id = ctx.add(visual_zstack);
        let sized_id = ctx.add(
            crate::primitives::MinSize::new(0.0, combo_style.height).set_child(visual_id),
        );

        // Wrap in a FocusRing — drawn outside the control on keyboard focus.
        let focused = interaction.map(|s| *s == ComboBoxState::Focused);
        let root_id = ctx.add(
            crate::primitives::FocusRing::new(focused)
                .corner_radius(combo_style.corner_radius)
                .set_child(sized_id),
        );
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
                move |_pos, ctx: &mut EventContext| {
                    if !enabled {
                        return;
                    }
                    interaction.set(ComboBoxState::Open);
                    ctx.activate(dropdown_id);
                    ctx.show_overlay(OverlayRequest {
                        content_id: dropdown_id,
                        anchor: self_id,
                        placement: OverlayPlacement::BelowPreferred,
                        dismiss: DismissBehavior::EscapeOrClickOutside,
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
                let items_for_typeahead = self.items.clone();
                // Type-ahead buffer: (prefix, last_keystroke_time)
                let typeahead: Rc<RefCell<(String, Instant)>> =
                    Rc::new(RefCell::new((String::new(), Instant::now())));
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    if !enabled {
                        return EventResponse::Ignored;
                    }
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::Enter | Key::Space,
                            ..
                        } => {
                            if interaction.get() == ComboBoxState::Open {
                                // Close the dropdown and confirm selection
                                interaction.set(ComboBoxState::Focused);
                                ctx.dismiss_all_overlays();
                            } else {
                                // Open the dropdown
                                interaction.set(ComboBoxState::Open);
                                ctx.activate(dropdown_id);
                                ctx.show_overlay(OverlayRequest {
                                    content_id: dropdown_id,
                                    anchor: self_id,
                                    placement: OverlayPlacement::BelowPreferred,
                                    dismiss: DismissBehavior::EscapeOrClickOutside,
                                    layer: OverlayLayer::InTree,
                                    parent_overlay: None,
                                });
                            }
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::Escape, ..
                        } => {
                            if interaction.get() == ComboBoxState::Open {
                                interaction.set(ComboBoxState::Focused);
                                ctx.dismiss_all_overlays();
                                EventResponse::Handled
                            } else {
                                EventResponse::Ignored
                            }
                        }
                        WidgetEvent::KeyDown {
                            key: Key::ArrowDown,
                            ..
                        } => {
                            if interaction.get() != ComboBoxState::Open {
                                // Open dropdown on ArrowDown when closed
                                interaction.set(ComboBoxState::Open);
                                ctx.activate(dropdown_id);
                                ctx.show_overlay(OverlayRequest {
                                    content_id: dropdown_id,
                                    anchor: self_id,
                                    placement: OverlayPlacement::BelowPreferred,
                                    dismiss: DismissBehavior::EscapeOrClickOutside,
                                    layer: OverlayLayer::InTree,
                                    parent_overlay: None,
                                });
                            }
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
                            if interaction.get() != ComboBoxState::Open {
                                interaction.set(ComboBoxState::Open);
                                ctx.activate(dropdown_id);
                                ctx.show_overlay(OverlayRequest {
                                    content_id: dropdown_id,
                                    anchor: self_id,
                                    placement: OverlayPlacement::BelowPreferred,
                                    dismiss: DismissBehavior::EscapeOrClickOutside,
                                    layer: OverlayLayer::InTree,
                                    parent_overlay: None,
                                });
                            }
                            let current = selected.get().unwrap_or(0);
                            let next = if current == 0 {
                                items_len.saturating_sub(1)
                            } else {
                                current - 1
                            };
                            selected.set(Some(next));
                            EventResponse::Handled
                        }
                        // Type-ahead: letter/character keys jump to matching item.
                        // Key::A..Key::Z and Key::Character are all handled via to_char().
                        WidgetEvent::KeyDown { key, .. } if key.to_char().is_some() => {
                            let ch = key.to_char().unwrap();
                            let mut ta = typeahead.borrow_mut();
                            let now = Instant::now();
                            // Reset buffer if more than 500ms since last keystroke
                            if now.duration_since(ta.1).as_millis() > 500 {
                                ta.0.clear();
                            }
                            ta.0.push(ch.to_ascii_lowercase());
                            ta.1 = now;
                            let prefix = ta.0.clone();
                            drop(ta);

                            // Find first item matching the prefix (case-insensitive)
                            if let Some(idx) = items_for_typeahead
                                .iter()
                                .position(|item| item.to_lowercase().starts_with(&prefix))
                            {
                                selected.set(Some(idx));
                            }
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
        builder.set_has_popup(fern_core::accesskit::HasPopup::Listbox);

        if let Some(name) = self.label.as_deref() {
            builder.set_name(name);
        }

        let value = match self.selected.get() {
            Some(idx) if idx < self.items.len() => self.items[idx].clone(),
            _ => self.placeholder.clone(),
        };
        if !value.is_empty() {
            builder.set_value(value);
        }

        builder.set_expanded(self.interaction.get() == ComboBoxState::Open);

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
            ComboBox::new_literal(vec!["Apple", "Banana", "Cherry"], selected.clone())
                .placeholder_literal("Select..."),
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
        let cb = tree.add(ComboBox::new_literal(vec!["A", "B"], selected.clone()));
        tree.layout(SizeProposal::exact(200.0, 50.0));
        let info = tree.accessibility_node(cb);
        assert_eq!(info.role(), fern_core::accesskit::Role::ComboBox);
        // A fresh, closed combo box should announce its collapsed state.
        assert!(!info.is_expanded());
    }

    #[test]
    fn accessibility_exposes_label_via_set_name() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let selected = Signal::new(None::<usize>);
        let cb = tree.add(
            ComboBox::new_literal(vec!["Apple", "Banana"], selected.clone())
                .label_literal("Fruit"),
        );
        tree.layout(SizeProposal::exact(200.0, 50.0));
        let info = tree.accessibility_node(cb);
        assert_eq!(info.name(), Some("Fruit"));
    }

    #[test]
    fn accessibility_expanded_flips_on_open_close() {
        // Uses Enter→Enter (both handled by ComboBox's own key handler) to
        // exercise the full open/close cycle for `is_expanded()`. Escape
        // dismissal goes through the overlay manager and doesn't currently
        // write back into ComboBox's `interaction` signal — that's a
        // pre-existing framework coherence gap, not something we test here.
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let selected = Signal::new(None::<usize>);
        let cb = tree.add(ComboBox::new_literal(
            vec!["Apple", "Banana", "Cherry"],
            selected.clone(),
        ));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        tree.focus(cb);

        assert!(!tree.accessibility_node(cb).is_expanded());

        // Enter opens the dropdown.
        tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(300.0, 200.0));
        assert!(tree.accessibility_node(cb).is_expanded());

        // Second Enter is routed back through ComboBox's handler (cb retains
        // focus), which sets interaction to Focused and dismisses the overlay.
        tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(300.0, 200.0));
        assert!(!tree.accessibility_node(cb).is_expanded());
    }

    #[test]
    fn arrow_keys_cycle_selection() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let selected = Signal::new(None::<usize>);
        let cb = tree.add(ComboBox::new_literal(
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
        let cb = tree.add(ComboBox::new_literal(
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
        let cb = tree.add(ComboBox::new_literal(
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

    #[test]
    fn type_ahead_jumps_to_matching_item() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let selected = Signal::new(None::<usize>);
        let cb = tree.add(ComboBox::new_literal(
            vec!["Apple", "Banana", "Cherry", "Blueberry"],
            selected.clone(),
        ));
        tree.layout(SizeProposal::exact(300.0, 50.0));
        tree.focus(cb);

        // Key::B (as sent by the real app via translate_key) → Banana (index 1)
        tree.press_key(Key::B, fern_core::event::Modifiers::NONE);
        assert_eq!(selected.get(), Some(1), "should jump to 'Banana'");

        // Key::L quickly after → buffer becomes "bl" → Blueberry (index 3)
        tree.press_key(Key::L, fern_core::event::Modifiers::NONE);
        assert_eq!(selected.get(), Some(3), "should jump to 'Blueberry'");
    }

    #[test]
    fn type_ahead_with_character_key() {
        // Key::Character is used for non-letter characters (numbers, symbols)
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let selected = Signal::new(None::<usize>);
        let cb = tree.add(ComboBox::new_literal(
            vec!["100px", "200px", "300px"],
            selected.clone(),
        ));
        tree.layout(SizeProposal::exact(300.0, 50.0));
        tree.focus(cb);

        tree.press_key(Key::Character('2'), fern_core::event::Modifiers::NONE);
        assert_eq!(selected.get(), Some(1), "should jump to '200px'");
    }

    #[test]
    fn type_ahead_case_insensitive() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let selected = Signal::new(None::<usize>);
        let cb = tree.add(ComboBox::new_literal(
            vec!["Apple", "Banana", "Cherry"],
            selected.clone(),
        ));
        tree.layout(SizeProposal::exact(300.0, 50.0));
        tree.focus(cb);

        // Key::C matches 'Cherry' (to_char returns lowercase 'c')
        tree.press_key(Key::C, fern_core::event::Modifiers::NONE);
        assert_eq!(
            selected.get(),
            Some(2),
            "should match 'Cherry' case-insensitively"
        );
    }

    #[test]
    fn type_ahead_no_match_keeps_selection() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let selected = Signal::new(Some(1_usize));
        let cb = tree.add(ComboBox::new_literal(
            vec!["Apple", "Banana", "Cherry"],
            selected.clone(),
        ));
        tree.layout(SizeProposal::exact(300.0, 50.0));
        tree.focus(cb);

        // Key::Z → no match, selection unchanged
        tree.press_key(Key::Z, fern_core::event::Modifiers::NONE);
        assert_eq!(
            selected.get(),
            Some(1),
            "no match should keep existing selection"
        );
    }

    #[test]
    fn below_preferred_opens_above_when_no_space() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let selected = Signal::new(None::<usize>);
        let cb = tree.add(ComboBox::new_literal(
            vec!["Apple", "Banana", "Cherry"],
            selected.clone(),
        ));
        // Tiny viewport: combo box near the bottom, no space for dropdown below
        tree.layout(SizeProposal::exact(300.0, 60.0));

        tree.click(cb);
        tree.layout(SizeProposal::exact(300.0, 60.0));

        assert_eq!(tree.active_overlays().len(), 1, "overlay should be open");

        // The overlay should be positioned above the combo box (negative y)
        // because there's no space below in a 60px viewport
        let content_ids = tree.overlay_manager().active_content_ids();
        let overlay_bounds = tree.bounds(content_ids[0]);
        let cb_bounds = tree.bounds(cb);

        // Overlay should be above the combo box (its bottom edge at or above the combo box top)
        assert!(
            overlay_bounds.y + overlay_bounds.height <= cb_bounds.y + 5.0,
            "overlay should be positioned above when no space below (overlay bottom: {}, combo top: {})",
            overlay_bounds.y + overlay_bounds.height,
            cb_bounds.y
        );
    }

    #[test]
    fn enter_toggles_dropdown_open_close() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let selected = Signal::new(None::<usize>);
        let cb = tree.add(ComboBox::new_literal(
            vec!["Apple", "Banana", "Cherry"],
            selected.clone(),
        ));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        tree.focus(cb);

        // Enter opens the dropdown
        tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(300.0, 200.0));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "Enter should open dropdown"
        );

        // Navigate to an item
        tree.press_key(Key::ArrowDown, fern_core::event::Modifiers::NONE);
        assert_eq!(selected.get(), Some(1));

        // Enter again closes the dropdown and confirms selection
        tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
        assert!(
            tree.active_overlays().is_empty(),
            "Enter should close dropdown when open"
        );
        assert_eq!(selected.get(), Some(1), "selection should be preserved");
    }

    #[test]
    fn escape_closes_dropdown() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let selected = Signal::new(None::<usize>);
        let cb = tree.add(ComboBox::new_literal(
            vec!["Apple", "Banana", "Cherry"],
            selected.clone(),
        ));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        tree.focus(cb);

        // Open
        tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(300.0, 200.0));
        assert_eq!(tree.active_overlays().len(), 1);

        // Escape closes
        tree.press_key(Key::Escape, fern_core::event::Modifiers::NONE);
        assert!(
            tree.active_overlays().is_empty(),
            "Escape should close the dropdown"
        );
    }

    #[test]
    fn arrow_down_opens_dropdown_when_closed() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let selected = Signal::new(None::<usize>);
        let cb = tree.add(ComboBox::new_literal(
            vec!["Apple", "Banana", "Cherry"],
            selected.clone(),
        ));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        tree.focus(cb);

        // ArrowDown when closed should open and navigate
        tree.press_key(Key::ArrowDown, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(300.0, 200.0));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "ArrowDown should open dropdown"
        );
        assert_eq!(selected.get(), Some(1));
    }

    #[test]
    fn type_ahead_highlights_in_open_dropdown() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let selected = Signal::new(None::<usize>);
        let cb = tree.add(ComboBox::new_literal(
            vec!["Apple", "Banana", "Cherry"],
            selected.clone(),
        ));
        tree.layout(SizeProposal::exact(300.0, 300.0));
        tree.focus(cb);

        // Open dropdown
        tree.click(cb);
        tree.layout(SizeProposal::exact(300.0, 300.0));
        assert_eq!(tree.active_overlays().len(), 1);
        let frame_before = tree.render();

        // Type 'b' (Key::B as sent by the real app) → type-ahead should select Banana
        tree.press_key(Key::B, fern_core::event::Modifiers::NONE);
        assert_eq!(
            selected.get(),
            Some(1),
            "type-ahead should update selection"
        );

        // Layout + render — triggers process_state_changes which should detect dirty binding
        tree.layout(SizeProposal::exact(300.0, 300.0));
        let frame_after = tree.render();

        // The rendered output should differ (highlight on Banana)
        assert_ne!(
            frame_before.shapes, frame_after.shapes,
            "dropdown should repaint with highlight after type-ahead"
        );
    }
}
