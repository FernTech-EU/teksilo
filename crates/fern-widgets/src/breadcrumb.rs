use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::app_command::AppCommand;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::state::BindingLevel;
use fern_core::widget::{
    CursorIcon, EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement,
};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, CornerRadius};

use crate::primitives::{HStack, IconWidget, Spacer};

const SEGMENT_PADDING_H: f32 = 10.0;
const SEGMENT_PADDING_V: f32 = 6.0;
const SEGMENT_MIN_HEIGHT: f32 = 32.0;
const FALLBACK_CHAR_WIDTH: f32 = 8.0;
const FALLBACK_LINE_HEIGHT: f32 = 16.0;

type CommandFactory = Box<dyn Fn(&mut EventContext)>;

struct BreadcrumbEntry {
    label: String,
    action: Option<CommandFactory>,
    current: bool,
}

impl std::fmt::Debug for BreadcrumbEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreadcrumbEntry")
            .field("label", &self.label)
            .field("current", &self.current)
            .finish()
    }
}

/// A single breadcrumb segment definition.
pub struct BreadcrumbItem {
    label: String,
    action: Option<CommandFactory>,
    current: bool,
}

impl std::fmt::Debug for BreadcrumbItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreadcrumbItem")
            .field("label", &self.label)
            .field("current", &self.current)
            .finish()
    }
}

impl BreadcrumbItem {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            action: None,
            current: false,
        }
    }

    pub fn current(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            action: None,
            current: true,
        }
    }

    pub fn on_click<C: AppCommand>(mut self, command: C) -> Self {
        self.action = Some(Box::new(move |ctx: &mut EventContext| {
            ctx.emit(command.clone());
        }));
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SegmentInteraction {
    Idle,
    Hovered,
    Focused,
}

struct BreadcrumbSegment {
    label: String,
    action: Option<CommandFactory>,
    current: bool,
    interaction: Signal<SegmentInteraction>,
}

impl std::fmt::Debug for BreadcrumbSegment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreadcrumbSegment")
            .field("label", &self.label)
            .field("current", &self.current)
            .field("interaction", &self.interaction.get())
            .finish()
    }
}

impl BreadcrumbSegment {
    fn new(label: String, action: Option<CommandFactory>, current: bool) -> Self {
        Self {
            label,
            action,
            current,
            interaction: Signal::new(SegmentInteraction::Idle),
        }
    }

    fn is_interactive(&self) -> bool {
        !self.current && self.action.is_some()
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
        text_width + SEGMENT_PADDING_H * 2.0
    }
}

impl Widget for BreadcrumbSegment {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        let interaction = ctx.signal(SegmentInteraction::Idle);
        let registry = ctx.binding_registry();
        interaction.bind_to(self_id, registry, BindingLevel::RepaintOnly);
        self.interaction = interaction.clone();

        let interactive = self.is_interactive();
        let action = self.action.take();
        let action_rc = std::rc::Rc::new(action);
        let action_for_tap = action_rc.clone();
        let action_for_key = action_rc.clone();
        let action_for_access = action_rc.clone();

        let handler_set = HandlerSet::new()
            .on_tap({
                let interaction = interaction.clone();
                move |ctx: &mut EventContext| {
                    if !interactive {
                        return;
                    }
                    if let Some(ref action) = *action_for_tap {
                        action(ctx);
                    }
                    interaction.set(SegmentInteraction::Hovered);
                }
            })
            .on_hover({
                let interaction = interaction.clone();
                move |entered: bool, _ctx: &mut EventContext| {
                    if !interactive {
                        interaction.set(SegmentInteraction::Idle);
                        return;
                    }
                    if interaction.get() == SegmentInteraction::Focused {
                        return;
                    }
                    interaction.set(if entered {
                        SegmentInteraction::Hovered
                    } else {
                        SegmentInteraction::Idle
                    });
                }
            })
            .on_focus({
                let interaction = interaction.clone();
                move |gained: bool, _ctx: &mut EventContext| {
                    if !interactive {
                        interaction.set(SegmentInteraction::Idle);
                        return;
                    }
                    interaction.set(if gained {
                        SegmentInteraction::Focused
                    } else {
                        SegmentInteraction::Idle
                    });
                }
            })
            .on_key({
                let interaction = interaction.clone();
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    if !interactive {
                        return EventResponse::Ignored;
                    }
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::Enter | Key::Space,
                            ..
                        } => {
                            if let Some(ref action) = *action_for_key {
                                action(ctx);
                            }
                            interaction.set(SegmentInteraction::Focused);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            })
            .on_access_action(move |action, ctx: &mut EventContext| {
                if interactive && action == fern_core::accesskit::Action::Click {
                    if let Some(ref action) = *action_for_access {
                        action(ctx);
                    }
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            })
            .focusable(interactive)
            .cursor(if interactive {
                CursorIcon::Pointer
            } else {
                CursorIcon::Default
            });

        ctx.apply_self_handlers(handler_set);
        Vec::new()
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        let width = proposal.width.unwrap_or_else(|| self.estimate_width(ctx));
        let text_height = if let Some(backend) = ctx.text_backend {
            backend
                .borrow_mut()
                .layout_single_line(&self.label, &ctx.theme.typography.label, None)
                .height
        } else {
            FALLBACK_LINE_HEIGHT
        };
        Size::new(
            width,
            (text_height + SEGMENT_PADDING_V * 2.0).max(SEGMENT_MIN_HEIGHT),
        )
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let colors = &ctx.theme.colors;
        let interaction = self.interaction.get();
        let interactive = self.is_interactive();

        if interactive {
            let background = if interaction == SegmentInteraction::Hovered {
                colors.primary.with_alpha(0.08)
            } else if interaction == SegmentInteraction::Focused {
                colors.primary.with_alpha(0.12)
            } else {
                Color::TRANSPARENT
            };
            if background.a() > 0.0 {
                canvas.fill_rounded_rect(
                    bounds,
                    CornerRadius::uniform(ctx.theme.shape.radius_sm),
                    background,
                );
            }
            if interaction == SegmentInteraction::Focused {
                canvas.stroke_rounded_rect(
                    bounds,
                    CornerRadius::uniform(ctx.theme.shape.radius_sm),
                    colors.focus_ring,
                    2.0,
                );
            }
        }

        let text_color = if self.current {
            colors.on_surface
        } else if interactive && interaction == SegmentInteraction::Hovered {
            colors.primary_hover
        } else if interactive {
            colors.primary
        } else {
            colors.on_surface_secondary
        };

        let text_bounds = Rect::new(
            bounds.x + SEGMENT_PADDING_H,
            bounds.y + SEGMENT_PADDING_V,
            (bounds.width - SEGMENT_PADDING_H * 2.0).max(0.0),
            (bounds.height - SEGMENT_PADDING_V * 2.0).max(0.0),
        );
        canvas.draw_text(
            &self.label,
            text_bounds,
            &ctx.theme.typography.label,
            text_color,
        );
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(if self.current {
            fern_core::accesskit::Role::Label
        } else {
            fern_core::accesskit::Role::Link
        });
        builder.set_name(&self.label);
        if self.current {
            builder.inner_mut().set_value("current page".to_string());
        } else if self.is_interactive() {
            builder.add_action(fern_core::accesskit::Action::Click);
            builder.add_action(fern_core::accesskit::Action::Focus);
        }
    }
}

#[derive(Debug)]
struct BreadcrumbSeparator;

impl Widget for BreadcrumbSeparator {
    fn size_that_fits(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        Size::new(12.0, SEGMENT_MIN_HEIGHT)
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let size = 10.0;
        let icon_bounds = Rect::new(
            bounds.x + (bounds.width - size) / 2.0,
            bounds.y + (bounds.height - size) / 2.0,
            size,
            size,
        );
        let icon = IconWidget::chevron_right(size).color(ctx.theme.colors.on_surface_secondary);
        icon.paint(icon_bounds, canvas, ctx);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::GenericContainer);
    }
}

/// A breadcrumb navigation row.
pub struct Breadcrumb {
    entries: Vec<BreadcrumbEntry>,
    trailing_slot: Option<Box<dyn Widget>>,
    root_child_id: Option<WidgetId>,
}

impl Breadcrumb {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            trailing_slot: None,
            root_child_id: None,
        }
    }

    pub fn item(mut self, item: BreadcrumbItem) -> Self {
        self.entries.push(BreadcrumbEntry {
            label: item.label,
            action: item.action,
            current: item.current,
        });
        self
    }

    pub fn trailing_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.trailing_slot = Some(Box::new(widget));
        self
    }
}

impl Default for Breadcrumb {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Breadcrumb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Breadcrumb")
            .field("item_count", &self.entries.len())
            .finish()
    }
}

impl Widget for Breadcrumb {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let entries = std::mem::take(&mut self.entries);
        let entry_count = entries.len();
        let mut row = HStack::new().spacing(4.0);

        for (index, entry) in entries.into_iter().enumerate() {
            row = row.child(BreadcrumbSegment::new(
                entry.label,
                entry.action,
                entry.current,
            ));
            if index + 1 < entry_count {
                row = row.child(BreadcrumbSeparator);
            }
        }

        if let Some(trailing) = self.trailing_slot.take() {
            let trailing_id = ctx.add_boxed(trailing);
            row = row.child(Spacer::new()).add_child(trailing_id);
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

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Navigation);
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
    use std::cell::Cell;
    use std::rc::Rc;

    #[derive(Debug, Clone, PartialEq)]
    enum TestCmd {
        GoLibrary,
        GoProject,
    }

    impl AppCommand for TestCmd {}

    #[test]
    fn clicking_interactive_segment_emits_command() {
        let called = Rc::new(Cell::new(false));
        let flag = called.clone();
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.on_command(move |cmd: &TestCmd| {
            if *cmd == TestCmd::GoProject {
                flag.set(true);
            }
        });

        let breadcrumb = tree.add(
            Breadcrumb::new()
                .item(BreadcrumbItem::new("Library").on_click(TestCmd::GoLibrary))
                .item(BreadcrumbItem::new("Project").on_click(TestCmd::GoProject))
                .item(BreadcrumbItem::current("Current")),
        );
        tree.layout(SizeProposal::exact(500.0, 48.0));

        let root = tree.child_widget(breadcrumb, 0);
        let project_segment = tree.child_widget(root, 2);
        tree.click(project_segment);

        assert!(called.get());
    }

    #[test]
    fn current_segment_is_not_clickable() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let breadcrumb = tree.add(
            Breadcrumb::new()
                .item(BreadcrumbItem::new("Library").on_click(TestCmd::GoLibrary))
                .item(BreadcrumbItem::current("Current")),
        );
        tree.layout(SizeProposal::exact(400.0, 48.0));

        let root = tree.child_widget(breadcrumb, 0);
        let current_segment = tree.child_widget(root, 2);
        let info = tree.accessibility_node(current_segment);

        assert_eq!(info.role(), fern_core::accesskit::Role::Label);
        assert!(
            !info
                .actions()
                .contains(&fern_core::accesskit::Action::Click)
        );
    }

    #[test]
    fn breadcrumb_exposes_navigation_role() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let breadcrumb = tree.add(
            Breadcrumb::new()
                .item(BreadcrumbItem::new("Library").on_click(TestCmd::GoLibrary))
                .item(BreadcrumbItem::current("Current")),
        );
        tree.layout(SizeProposal::exact(400.0, 48.0));

        let info = tree.accessibility_node(breadcrumb);
        assert_eq!(info.role(), fern_core::accesskit::Role::Navigation);
    }
}
