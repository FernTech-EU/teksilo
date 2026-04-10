use std::cell::RefCell;
use std::rc::Rc;

use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::state::BindingLevel;
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::CornerRadius;

use crate::primitives::{Divider, Expand, HStack, Padding, Switcher, VStack};
use crate::scroll_area::{ScrollArea, ScrollBarPolicy, ScrollBarStyle};

const HEADER_PADDING_H: f32 = 14.0;
const HEADER_PADDING_V: f32 = 10.0;
const FALLBACK_CHAR_WIDTH: f32 = 8.0;
const FALLBACK_LINE_HEIGHT: f32 = 16.0;
const HEADER_MIN_WIDTH: f32 = 72.0;
const HEADER_MIN_HEIGHT: f32 = 40.0;
const CONTENT_TOP_INSET: f32 = 8.0;

struct TabEntry {
    label: String,
    content: Box<dyn Widget>,
    enabled: bool,
}

/// A single tab definition used by `TabWidget`.
pub struct TabItem {
    label: String,
    content: Box<dyn Widget>,
    enabled: bool,
}

impl std::fmt::Debug for TabItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabItem")
            .field("label", &self.label)
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl TabItem {
    pub fn new(label: impl Into<String>, content: impl Widget + 'static) -> Self {
        Self {
            label: label.into(),
            content: Box::new(content),
            enabled: true,
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TabHeaderInteraction {
    Idle,
    Hovered,
    Focused,
}

#[derive(Debug)]
struct TabPane {
    label: String,
    child_id: Option<WidgetId>,
    pending_child: Option<Box<dyn Widget>>,
}

impl TabPane {
    fn new(label: String, child: Box<dyn Widget>) -> Self {
        Self {
            label,
            child_id: None,
            pending_child: Some(child),
        }
    }
}

impl Widget for TabPane {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(child) = self.pending_child.take() {
            self.child_id = Some(ctx.add_boxed(child));
        }
        self.child_id.into_iter().collect()
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        self.child_id
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
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::TabPanel);
        builder.set_name(&self.label);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}

#[derive(Debug)]
struct TabHeader {
    label: String,
    index: usize,
    enabled: bool,
    selected: Signal<usize>,
    header_ids: Rc<RefCell<Vec<WidgetId>>>,
    enabled_tabs: Rc<Vec<bool>>,
    interaction: Signal<TabHeaderInteraction>,
}

impl TabHeader {
    fn new(
        label: String,
        index: usize,
        enabled: bool,
        selected: Signal<usize>,
        header_ids: Rc<RefCell<Vec<WidgetId>>>,
        enabled_tabs: Rc<Vec<bool>>,
    ) -> Self {
        Self {
            label,
            index,
            enabled,
            selected,
            header_ids,
            enabled_tabs,
            interaction: Signal::new(TabHeaderInteraction::Idle),
        }
    }

    fn estimate_width(&self, ctx: &LayoutContext) -> f32 {
        let text_width = if let Some(backend) = ctx.text_backend {
            backend
                .borrow_mut()
                .layout_single_line(&self.label, &ctx.theme.typography.label, None)
                .width
        } else {
            self.label.len() as f32 * FALLBACK_CHAR_WIDTH
        };
        (text_width + HEADER_PADDING_H * 2.0).max(HEADER_MIN_WIDTH)
    }
}

impl Widget for TabHeader {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        let interaction = ctx.signal(TabHeaderInteraction::Idle);
        let registry = ctx.binding_registry();

        self.selected
            .bind_to(self_id, registry, BindingLevel::RepaintOnly);
        interaction.bind_to(self_id, registry, BindingLevel::RepaintOnly);
        self.interaction = interaction.clone();

        let index = self.index;
        let enabled = self.enabled;
        let selected = self.selected.clone();
        let header_ids = self.header_ids.clone();
        let enabled_tabs = self.enabled_tabs.clone();

        let handler_set = HandlerSet::new()
            .on_tap(move |_ctx: &mut EventContext| {
                if enabled {
                    selected.set(index);
                }
            })
            .on_hover({
                let interaction = interaction.clone();
                move |entered: bool, _ctx: &mut EventContext| {
                    if !enabled {
                        interaction.set(TabHeaderInteraction::Idle);
                        return;
                    }
                    if interaction.get() == TabHeaderInteraction::Focused {
                        return;
                    }
                    interaction.set(if entered {
                        TabHeaderInteraction::Hovered
                    } else {
                        TabHeaderInteraction::Idle
                    });
                }
            })
            .on_focus({
                let interaction = interaction.clone();
                move |gained: bool, _ctx: &mut EventContext| {
                    if !enabled {
                        interaction.set(TabHeaderInteraction::Idle);
                        return;
                    }
                    if gained {
                        interaction.set(TabHeaderInteraction::Focused);
                    } else {
                        interaction.set(TabHeaderInteraction::Idle);
                    }
                }
            })
            .on_key({
                let selected = self.selected.clone();
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    if !enabled {
                        return EventResponse::Ignored;
                    }
                    let headers = header_ids.borrow();
                    if headers.is_empty() {
                        return EventResponse::Ignored;
                    }
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::ArrowRight,
                            ..
                        } => {
                            let next = next_enabled_index(&enabled_tabs, index, 1);
                            selected.set(next);
                            ctx.request_focus(headers[next]);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::ArrowLeft,
                            ..
                        } => {
                            let next = next_enabled_index(&enabled_tabs, index, -1);
                            selected.set(next);
                            ctx.request_focus(headers[next]);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::Enter | Key::Space,
                            ..
                        } => {
                            selected.set(index);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            })
            .on_access_action({
                let selected = self.selected.clone();
                move |action, _ctx: &mut EventContext| {
                    if enabled && action == fern_core::accesskit::Action::Click {
                        selected.set(index);
                        EventResponse::Handled
                    } else {
                        EventResponse::Ignored
                    }
                }
            })
            .focusable(enabled)
            .cursor(if enabled {
                CursorIcon::Pointer
            } else {
                CursorIcon::Default
            });

        ctx.apply_self_handlers(handler_set);

        Vec::new()
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        let width = proposal.width.unwrap_or_else(|| self.estimate_width(ctx));
        let height = if let Some(backend) = ctx.text_backend {
            let text_height = backend
                .borrow_mut()
                .layout_single_line(&self.label, &ctx.theme.typography.label, None)
                .height;
            (text_height + HEADER_PADDING_V * 2.0).max(HEADER_MIN_HEIGHT)
        } else {
            (FALLBACK_LINE_HEIGHT + HEADER_PADDING_V * 2.0).max(HEADER_MIN_HEIGHT)
        };
        Size::new(width, proposal.height.unwrap_or(height))
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let selected = self.selected.get() == self.index;
        let interaction = self.interaction.get();
        let colors = &ctx.theme.colors;
        let radius = ctx.theme.shape.radius_sm;
        let corner_radius = CornerRadius {
            top_left: radius,
            top_right: radius,
            bottom_left: 0.0,
            bottom_right: 0.0,
        };

        let background = if !self.enabled {
            colors.disabled_fill
        } else if selected {
            colors.surface
        } else if interaction == TabHeaderInteraction::Hovered
            || interaction == TabHeaderInteraction::Focused
        {
            colors.surface
        } else {
            colors.surface_secondary
        };

        canvas.fill_rounded_rect(bounds, corner_radius, background);

        let border = if !self.enabled {
            colors.border
        } else if interaction == TabHeaderInteraction::Focused {
            colors.focus_ring
        } else if selected {
            colors.primary.with_alpha(0.45)
        } else if interaction == TabHeaderInteraction::Hovered {
            colors.border_strong
        } else {
            colors.border
        };
        canvas.stroke_rounded_rect(
            Rect::new(bounds.x, bounds.y, bounds.width, bounds.height - 1.0),
            corner_radius,
            border,
            1.0,
        );

        if selected && self.enabled {
            let indicator = Rect::new(bounds.x, bounds.bottom() - 3.0, bounds.width, 3.0);
            canvas.fill_rect(indicator, colors.primary);
        }

        let text_color = if !self.enabled {
            colors.disabled_text
        } else if selected {
            colors.primary
        } else if interaction == TabHeaderInteraction::Hovered {
            colors.on_surface
        } else {
            colors.on_surface_secondary
        };
        let text_bounds = Rect::new(
            bounds.x + HEADER_PADDING_H,
            bounds.y + HEADER_PADDING_V,
            (bounds.width - HEADER_PADDING_H * 2.0).max(0.0),
            (bounds.height - HEADER_PADDING_V * 2.0).max(0.0),
        );
        canvas.draw_text(
            &self.label,
            text_bounds,
            &ctx.theme.typography.label,
            text_color,
        );

        if interaction == TabHeaderInteraction::Focused {
            canvas.stroke_rounded_rect(
                Rect::new(bounds.x, bounds.y, bounds.width, bounds.height - 3.0),
                corner_radius,
                colors.focus_ring,
                2.0,
            );
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Tab);
        builder.set_name(&self.label);
        if !self.enabled {
            builder.set_disabled();
        } else {
            builder.add_action(fern_core::accesskit::Action::Click);
        }
        builder.add_action(fern_core::accesskit::Action::Focus);
        builder.inner_mut().set_selected(self.selected.get() == self.index);
    }
}

fn next_enabled_index(enabled_tabs: &[bool], current: usize, direction: isize) -> usize {
    if enabled_tabs.is_empty() {
        return current;
    }

    let len = enabled_tabs.len() as isize;
    let mut offset = 1_isize;
    while offset <= len {
        let candidate = (current as isize + direction * offset).rem_euclid(len) as usize;
        if enabled_tabs[candidate] {
            return candidate;
        }
        offset += 1;
    }
    current
}

#[derive(Debug)]
struct TabBar {
    header_ids: Vec<WidgetId>,
    trailing_child_id: Option<WidgetId>,
    root_child_id: Option<WidgetId>,
}

impl TabBar {
    fn new(header_ids: Vec<WidgetId>, trailing_child_id: Option<WidgetId>) -> Self {
        Self {
            header_ids,
            trailing_child_id,
            root_child_id: None,
        }
    }
}

impl Widget for TabBar {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let mut headers = HStack::new().spacing(4.0);
        for &header_id in &self.header_ids {
            headers = headers.add_child(header_id);
        }

        let headers_scroll_id = ctx.add(
            ScrollArea::new(headers)
                .scroll_bar_style(ScrollBarStyle::Overlay)
                .vertical_scroll_bar_policy(ScrollBarPolicy::AlwaysOff)
                .horizontal_scroll_bar_policy(ScrollBarPolicy::AsNeeded)
                .widget_resizable(true),
        );

        let mut row = HStack::new()
            .spacing(8.0)
            .child(Expand::horizontal().fills_stack().set_child(headers_scroll_id));

        if let Some(trailing_child_id) = self.trailing_child_id {
            row = row.add_child(trailing_child_id);
        }

        let root_id = ctx.add(row);
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
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let radius = CornerRadius::uniform(ctx.theme.shape.radius_sm);
        canvas.fill_rounded_rect(bounds, radius, ctx.theme.colors.surface_tertiary);
        canvas.stroke_rounded_rect(
            bounds,
            radius,
            ctx.theme.colors.border,
            ctx.theme.shape.border_width,
        );
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::TabList);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

/// A tabbed container with keyboard navigation, a trailing action slot,
/// and dormant content panes backed by `Switcher`.
pub struct TabWidget {
    selected: Signal<usize>,
    entries: Vec<TabEntry>,
    trailing_slot: Option<Box<dyn Widget>>,
    root_child_id: Option<WidgetId>,
}

impl TabWidget {
    pub fn new(selected: Signal<usize>) -> Self {
        Self {
            selected,
            entries: Vec::new(),
            trailing_slot: None,
            root_child_id: None,
        }
    }

    pub fn tab(mut self, label: impl Into<String>, content: impl Widget + 'static) -> Self {
        self.entries.push(TabEntry {
            label: label.into(),
            content: Box::new(content),
            enabled: true,
        });
        self
    }

    pub fn tab_item(mut self, item: TabItem) -> Self {
        self.entries.push(TabEntry {
            label: item.label,
            content: item.content,
            enabled: item.enabled,
        });
        self
    }

    pub fn trailing_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.trailing_slot = Some(Box::new(widget));
        self
    }
}

impl std::fmt::Debug for TabWidget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabWidget")
            .field("tab_count", &self.entries.len())
            .finish()
    }
}

impl Widget for TabWidget {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let entries = std::mem::take(&mut self.entries);
        let enabled_tabs = Rc::new(entries.iter().map(|entry| entry.enabled).collect::<Vec<_>>());
        let mut header_ids = Vec::with_capacity(entries.len());
        let shared_header_ids = Rc::new(RefCell::new(Vec::with_capacity(entries.len())));
        let mut switcher = Switcher::new(self.selected.clone());

        for (index, entry) in entries.into_iter().enumerate() {
            let header_id = ctx.add(TabHeader::new(
                entry.label.clone(),
                index,
                entry.enabled,
                self.selected.clone(),
                shared_header_ids.clone(),
                enabled_tabs.clone(),
            ));
            header_ids.push(header_id);
            shared_header_ids.borrow_mut().push(header_id);

            switcher = switcher.child_boxed(Box::new(TabPane::new(entry.label, entry.content)));
        }

        let trailing_child_id = self.trailing_slot.take().map(|widget| ctx.add_boxed(widget));
        let tab_bar_id = ctx.add(TabBar::new(header_ids, trailing_child_id));
        let divider_id = ctx.add(Divider::new());
        let switcher_id = ctx.add(switcher);
        let padded_content_id = ctx.add(Padding::new(CONTENT_TOP_INSET, 0.0, 0.0, 0.0).set_child(switcher_id));
        let content_id = ctx.add(Expand::vertical().fills_stack().set_child(padded_content_id));

        let root_id = ctx.add(
            VStack::new()
                .add_child(tab_bar_id)
                .add_child(divider_id)
                .add_child(content_id),
        );

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
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::event::Modifiers;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;
    use std::cell::Cell;
    use std::rc::Rc;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);

    impl Widget for FixedLeaf {
        fn size_that_fits(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
            Size::new(self.0, self.1)
        }
    }

    #[derive(Debug)]
    struct BuildCountingLeaf {
        build_count: Rc<Cell<usize>>,
    }

    impl Widget for BuildCountingLeaf {
        fn build(&mut self, _ctx: &mut BuildContext) -> Vec<WidgetId> {
            self.build_count.set(self.build_count.get() + 1);
            Vec::new()
        }

        fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
            proposal.resolve(120.0, 48.0)
        }
    }

    fn header_id(tree: &WidgetTree, tabs_id: WidgetId, index: usize) -> WidgetId {
        let root = tree.child_widget(tabs_id, 0);
        let tab_bar = tree.child_widget(root, 0);
        let row = tree.child_widget(tab_bar, 0);
        let expand = tree.child_widget(row, 0);
        let scroll = tree.child_widget(expand, 0);
        let headers = tree.child_widget(scroll, 0);
        tree.child_widget(headers, index)
    }

    fn switcher_id(tree: &WidgetTree, tabs_id: WidgetId) -> WidgetId {
        let root = tree.child_widget(tabs_id, 0);
        let expand = tree.child_widget(root, 2);
        let padded_content = tree.child_widget(expand, 0);
        tree.child_widget(padded_content, 0)
    }

    #[test]
    fn access_click_updates_selected_index() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let tabs = tree.add(
            TabWidget::new(selected.clone())
                .tab("Overview", FixedLeaf(120.0, 48.0))
                .tab("Details", FixedLeaf(140.0, 52.0)),
        );

        tree.layout(SizeProposal::exact(480.0, 240.0));
        let second_header = header_id(&tree, tabs, 1);
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: fern_core::accesskit::Action::Click,
            target: Some(second_header),
        });

        assert_eq!(selected.get(), 1);
    }

    #[test]
    fn keyboard_navigation_updates_selection_and_focus() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let tabs = tree.add(
            TabWidget::new(selected.clone())
                .tab("Overview", FixedLeaf(120.0, 48.0))
                .tab("Details", FixedLeaf(140.0, 52.0))
                .tab("Activity", FixedLeaf(160.0, 56.0)),
        );

        tree.layout(SizeProposal::exact(640.0, 320.0));

        tree.press_key(Key::Tab, Modifiers::NONE);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(selected.get(), 1);

        let second_header = header_id(&tree, tabs, 1);
        assert_eq!(tree.focused(), Some(second_header));

        tree.press_key(Key::ArrowLeft, Modifiers::NONE);
        assert_eq!(selected.get(), 0);
    }

    #[test]
    fn inactive_panes_are_dormant() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let tabs = tree.add(
            TabWidget::new(selected.clone())
                .tab("Overview", FixedLeaf(120.0, 48.0))
                .tab("Details", FixedLeaf(140.0, 52.0)),
        );

        tree.layout(SizeProposal::exact(480.0, 240.0));

        let switcher = switcher_id(&tree, tabs);
        let zstack = tree.child_widget(switcher, 0);
        let panes = tree.children(zstack);
        assert_eq!(panes.len(), 2);
        assert!(tree.is_visible(panes[0]));
        assert!(!tree.is_visible(panes[1]));

        selected.set(1);
        tree.layout(SizeProposal::exact(480.0, 240.0));

        assert!(!tree.is_visible(panes[0]));
        assert!(tree.is_visible(panes[1]));
    }

    #[test]
    fn panes_preserve_state_across_switches() {
        let selected = Signal::new(0_usize);
        let first_builds = Rc::new(Cell::new(0));
        let second_builds = Rc::new(Cell::new(0));
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());

        tree.add(
            TabWidget::new(selected.clone())
                .tab(
                    "Overview",
                    BuildCountingLeaf {
                        build_count: first_builds.clone(),
                    },
                )
                .tab(
                    "Details",
                    BuildCountingLeaf {
                        build_count: second_builds.clone(),
                    },
                ),
        );

        tree.layout(SizeProposal::exact(480.0, 240.0));
        assert_eq!(first_builds.get(), 1);
        assert_eq!(second_builds.get(), 1);

        selected.set(1);
        tree.layout(SizeProposal::exact(480.0, 240.0));
        selected.set(0);
        tree.layout(SizeProposal::exact(480.0, 240.0));

        assert_eq!(first_builds.get(), 1);
        assert_eq!(second_builds.get(), 1);
    }

    #[test]
    fn accessibility_roles_are_exposed() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(
            TabWidget::new(selected)
                .tab("Overview", FixedLeaf(120.0, 48.0))
                .tab("Details", FixedLeaf(140.0, 52.0)),
        );

        tree.layout(SizeProposal::exact(480.0, 240.0));

        let tab_list = tree.find_by_role(fern_core::accesskit::Role::TabList);
        let tab = tree.find_by_role(fern_core::accesskit::Role::Tab);
        let tab_panel = tree.find_by_role(fern_core::accesskit::Role::TabPanel);

        assert!(tab_list.is_some());
        assert!(tab.is_some());
        assert!(tab_panel.is_some());

        let info = tree.accessibility_node(tab.unwrap());
        assert_eq!(info.role(), fern_core::accesskit::Role::Tab);
        assert!(info.actions().contains(&fern_core::accesskit::Action::Click));
    }

    #[test]
    fn disabled_tabs_do_not_activate_and_are_skipped_by_keyboard() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let tabs = tree.add(
            TabWidget::new(selected.clone())
                .tab("Overview", FixedLeaf(120.0, 48.0))
                .tab_item(TabItem::new("Locked", FixedLeaf(120.0, 48.0)).enabled(false))
                .tab("Activity", FixedLeaf(120.0, 48.0)),
        );

        tree.layout(SizeProposal::exact(640.0, 320.0));

        let disabled_header = header_id(&tree, tabs, 1);

        tree.click(disabled_header);
        assert_eq!(selected.get(), 0);

        tree.press_key(Key::Tab, Modifiers::NONE);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(selected.get(), 2);

        let info = tree.accessibility_node(disabled_header);
        assert_eq!(info.role(), fern_core::accesskit::Role::Tab);
        assert!(!info.actions().contains(&fern_core::accesskit::Action::Click));
    }

    #[test]
    fn content_is_positioned_below_tab_strip() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let tabs = tree.add(
            TabWidget::new(selected)
                .tab("Overview", FixedLeaf(120.0, 48.0))
                .tab("Details", FixedLeaf(140.0, 52.0)),
        );

        tree.layout(SizeProposal::exact(480.0, 240.0));

        let root = tree.child_widget(tabs, 0);
        let tab_bar = tree.child_widget(root, 0);
        let divider = tree.child_widget(root, 1);
        let content_expand = tree.child_widget(root, 2);
        let padded_content = tree.child_widget(content_expand, 0);
        let switcher = tree.child_widget(padded_content, 0);

        let tab_bar_bounds = tree.bounds(tab_bar);
        let divider_bounds = tree.bounds(divider);
        let switcher_bounds = tree.bounds(switcher);

        assert!(divider_bounds.y >= tab_bar_bounds.bottom() - 0.01);
        assert!(switcher_bounds.y >= divider_bounds.bottom() + CONTENT_TOP_INSET - 0.01);
    }

    #[test]
    fn tab_bar_wraps_headers_in_horizontal_scroll_area() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let tabs = tree.add(
            TabWidget::new(selected)
                .tab("One", FixedLeaf(120.0, 48.0))
                .tab("Two", FixedLeaf(120.0, 48.0))
                .tab("Three", FixedLeaf(120.0, 48.0))
                .tab("Four", FixedLeaf(120.0, 48.0))
                .tab("Five", FixedLeaf(120.0, 48.0)),
        );

        tree.layout(SizeProposal::exact(220.0, 240.0));

        let root = tree.child_widget(tabs, 0);
        let tab_bar = tree.child_widget(root, 0);
        let row = tree.child_widget(tab_bar, 0);
        let expand = tree.child_widget(row, 0);
        let scroll = tree.child_widget(expand, 0);
        let info = tree.accessibility_node(scroll);

        assert_eq!(info.role(), fern_core::accesskit::Role::ScrollView);
    }
}