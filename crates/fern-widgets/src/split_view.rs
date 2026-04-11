use std::cell::Cell;
use std::rc::Rc;

use fern_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, PointerButton, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::state::BindingLevel;
use fern_core::widget::{
    CursorIcon, EventContext, LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement,
};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::{CornerRadius, Orientation};

const DEFAULT_DIVIDER_THICKNESS: f32 = 12.0;
const DEFAULT_MIN_PANE_SIZE: f32 = 96.0;
const KEYBOARD_STEP_PX: f32 = 24.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SplitHandleState {
    Idle,
    Hovered,
    Focused,
    Dragging,
}

struct SplitHandle {
    split: Signal<f32>,
    orientation: Orientation,
    min_first_size: f32,
    min_second_size: f32,
    divider_thickness: f32,
    enabled: bool,
    container_bounds: Rc<Cell<Rect>>,
    interaction: Signal<SplitHandleState>,
}

impl SplitHandle {
    fn new(
        split: Signal<f32>,
        orientation: Orientation,
        min_first_size: f32,
        min_second_size: f32,
        divider_thickness: f32,
        enabled: bool,
        container_bounds: Rc<Cell<Rect>>,
    ) -> Self {
        Self {
            split,
            orientation,
            min_first_size,
            min_second_size,
            divider_thickness,
            enabled,
            container_bounds,
            interaction: Signal::new(SplitHandleState::Idle),
        }
    }
}

impl std::fmt::Debug for SplitHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SplitHandle")
            .field("orientation", &self.orientation)
            .field("enabled", &self.enabled)
            .field("split", &self.split.get())
            .finish()
    }
}

impl Widget for SplitHandle {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        let interaction = ctx.signal(SplitHandleState::Idle);
        let registry = ctx.binding_registry();
        interaction.bind_to(self_id, registry, BindingLevel::RepaintOnly);
        self.interaction = interaction.clone();

        let enabled = self.enabled;
        let orientation = self.orientation;
        let divider_thickness = self.divider_thickness;
        let split = self.split.clone();
        let container_bounds = self.container_bounds.clone();
        let min_first_size = self.min_first_size;
        let min_second_size = self.min_second_size;
        let resize_cursor = match orientation {
            Orientation::Horizontal => CursorIcon::ColResize,
            Orientation::Vertical => CursorIcon::RowResize,
        };

        let set_from_position = move |position: Point| {
            let bounds = container_bounds.get();
            let available = match orientation {
                Orientation::Horizontal => bounds.width,
                Orientation::Vertical => bounds.height,
            } - divider_thickness;
            if available <= 0.0 {
                return;
            }

            let start = match orientation {
                Orientation::Horizontal => bounds.x,
                Orientation::Vertical => bounds.y,
            };
            let coordinate = match orientation {
                Orientation::Horizontal => position.x,
                Orientation::Vertical => position.y,
            };
            let min = (min_first_size / available).clamp(0.0, 1.0);
            let max = 1.0 - (min_second_size / available).clamp(0.0, 1.0);
            let (min, max) = if min <= max { (min, max) } else { (0.5, 0.5) };
            let fraction =
                ((coordinate - start - divider_thickness / 2.0) / available).clamp(min, max);
            split.set(fraction);
        };

        let handler_set = HandlerSet::new()
            .on_pointer_event({
                let interaction = interaction.clone();
                move |event, ctx: &mut EventContext| {
                    if !enabled {
                        return EventResponse::Ignored;
                    }

                    match event {
                        WidgetEvent::PointerDown {
                            position, button, ..
                        } => {
                            if *button != PointerButton::Primary {
                                return EventResponse::Ignored;
                            }
                            interaction.set(SplitHandleState::Dragging);
                            set_from_position(*position);
                            ctx.capture_pointer();
                            ctx.request_focus(self_id);
                            EventResponse::Handled
                        }
                        WidgetEvent::PointerMove { position } => {
                            if interaction.get() == SplitHandleState::Dragging {
                                set_from_position(*position);
                                EventResponse::Handled
                            } else {
                                EventResponse::Ignored
                            }
                        }
                        WidgetEvent::PointerUp { .. } => {
                            if interaction.get() == SplitHandleState::Dragging {
                                interaction.set(SplitHandleState::Focused);
                                ctx.release_pointer();
                                EventResponse::Handled
                            } else {
                                EventResponse::Ignored
                            }
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            })
            .on_hover({
                let interaction = interaction.clone();
                move |entered, _ctx| {
                    if !enabled {
                        interaction.set(SplitHandleState::Idle);
                        return;
                    }
                    if interaction.get() == SplitHandleState::Dragging {
                        return;
                    }
                    interaction.set(if entered {
                        SplitHandleState::Hovered
                    } else {
                        SplitHandleState::Idle
                    });
                }
            })
            .on_focus({
                let interaction = interaction.clone();
                move |gained, _ctx| {
                    if !enabled {
                        interaction.set(SplitHandleState::Idle);
                        return;
                    }
                    if interaction.get() == SplitHandleState::Dragging {
                        return;
                    }
                    interaction.set(if gained {
                        SplitHandleState::Focused
                    } else {
                        SplitHandleState::Idle
                    });
                }
            })
            .on_key({
                let handle = self.split.clone();
                let container_bounds = self.container_bounds.clone();
                let interaction = interaction.clone();
                move |event, _ctx| {
                    if !enabled {
                        return EventResponse::Ignored;
                    }

                    let bounds = container_bounds.get();
                    let available = match orientation {
                        Orientation::Horizontal => bounds.width,
                        Orientation::Vertical => bounds.height,
                    } - divider_thickness;
                    let step = (KEYBOARD_STEP_PX / available.max(1.0)).clamp(0.01, 0.2);
                    let min = (min_first_size / available.max(1.0)).clamp(0.0, 1.0);
                    let max = 1.0 - (min_second_size / available.max(1.0)).clamp(0.0, 1.0);
                    let (min, max) = if min <= max { (min, max) } else { (0.5, 0.5) };

                    match event {
                        WidgetEvent::KeyDown { key, .. } => {
                            let mut next = handle.get();
                            let handled = match (orientation, key) {
                                (Orientation::Horizontal, Key::ArrowLeft) => {
                                    next -= step;
                                    true
                                }
                                (Orientation::Horizontal, Key::ArrowRight) => {
                                    next += step;
                                    true
                                }
                                (Orientation::Vertical, Key::ArrowUp) => {
                                    next -= step;
                                    true
                                }
                                (Orientation::Vertical, Key::ArrowDown) => {
                                    next += step;
                                    true
                                }
                                (_, Key::Home) => {
                                    next = min;
                                    true
                                }
                                (_, Key::End) => {
                                    next = max;
                                    true
                                }
                                _ => false,
                            };

                            if handled {
                                handle.set(next.clamp(min, max));
                                interaction.set(SplitHandleState::Focused);
                                EventResponse::Handled
                            } else {
                                EventResponse::Ignored
                            }
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            })
            .on_access_action({
                let split = self.split.clone();
                let container_bounds = self.container_bounds.clone();
                let interaction = interaction.clone();
                move |action, _ctx| {
                    if !enabled {
                        return EventResponse::Ignored;
                    }

                    let bounds = container_bounds.get();
                    let available = match orientation {
                        Orientation::Horizontal => bounds.width,
                        Orientation::Vertical => bounds.height,
                    } - divider_thickness;
                    let step = (KEYBOARD_STEP_PX / available.max(1.0)).clamp(0.01, 0.2);
                    let min = (min_first_size / available.max(1.0)).clamp(0.0, 1.0);
                    let max = 1.0 - (min_second_size / available.max(1.0)).clamp(0.0, 1.0);
                    let (min, max) = if min <= max { (min, max) } else { (0.5, 0.5) };

                    let delta = match action {
                        fern_core::accesskit::Action::Increment => Some(step),
                        fern_core::accesskit::Action::Decrement => Some(-step),
                        _ => None,
                    };

                    if let Some(delta) = delta {
                        split.set((split.get() + delta).clamp(min, max));
                        interaction.set(SplitHandleState::Focused);
                        EventResponse::Handled
                    } else {
                        EventResponse::Ignored
                    }
                }
            })
            .focusable(enabled)
            .cursor(if enabled {
                resize_cursor
            } else {
                CursorIcon::Default
            });

        ctx.apply_self_handlers(handler_set);
        Vec::new()
    }

    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        match self.orientation {
            Orientation::Horizontal => Size::new(
                self.divider_thickness,
                proposal.height.unwrap_or(self.divider_thickness),
            ),
            Orientation::Vertical => Size::new(
                proposal.width.unwrap_or(self.divider_thickness),
                self.divider_thickness,
            ),
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let colors = &ctx.theme.colors;
        let interaction = self.interaction.get();

        let background = if !self.enabled {
            colors.surface_secondary
        } else if interaction == SplitHandleState::Dragging {
            colors.primary.with_alpha(0.14)
        } else if interaction == SplitHandleState::Focused {
            colors.primary.with_alpha(0.10)
        } else if interaction == SplitHandleState::Hovered {
            colors.surface
        } else {
            colors.surface_secondary
        };
        canvas.fill_rounded_rect(
            bounds,
            CornerRadius::uniform(ctx.theme.shape.radius_sm),
            background,
        );

        let grip_color = if !self.enabled {
            colors.disabled_text
        } else if interaction == SplitHandleState::Dragging
            || interaction == SplitHandleState::Focused
        {
            colors.primary
        } else if interaction == SplitHandleState::Hovered {
            colors.on_surface
        } else {
            colors.on_surface_secondary
        };

        let center_x = bounds.x + bounds.width / 2.0;
        let center_y = bounds.y + bounds.height / 2.0;
        match self.orientation {
            Orientation::Horizontal => {
                canvas.fill_rect(
                    Rect::new(center_x - 1.0, bounds.y + 2.0, 2.0, bounds.height - 4.0),
                    grip_color.with_alpha(0.35),
                );
                for offset in [-8.0_f32, 0.0, 8.0] {
                    canvas.fill_circle(Point::new(center_x, center_y + offset), 1.6, grip_color);
                }
            }
            Orientation::Vertical => {
                canvas.fill_rect(
                    Rect::new(bounds.x + 2.0, center_y - 1.0, bounds.width - 4.0, 2.0),
                    grip_color.with_alpha(0.35),
                );
                for offset in [-8.0_f32, 0.0, 8.0] {
                    canvas.fill_circle(Point::new(center_x + offset, center_y), 1.6, grip_color);
                }
            }
        }

        if interaction == SplitHandleState::Focused {
            canvas.stroke_rounded_rect(
                bounds,
                CornerRadius::uniform(ctx.theme.shape.radius_sm),
                colors.focus_ring,
                2.0,
            );
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Splitter);
        builder.set_name("Split view divider");
        builder.set_numeric_value((self.split.get() * 100.0) as f64);
        builder.set_min_numeric_value(0.0);
        builder.set_max_numeric_value(100.0);
        builder.set_value(format!("{:.0}%", self.split.get() * 100.0));
        if !self.enabled {
            builder.set_disabled();
        } else {
            builder.add_action(fern_core::accesskit::Action::Focus);
            builder.add_action(fern_core::accesskit::Action::Increment);
            builder.add_action(fern_core::accesskit::Action::Decrement);
        }
    }
}

pub struct SplitView {
    split: Signal<f32>,
    orientation: Orientation,
    min_first_size: f32,
    min_second_size: f32,
    divider_thickness: f32,
    enabled: bool,
    first_pending: Option<PendingChild>,
    second_pending: Option<PendingChild>,
    first_id: Option<WidgetId>,
    handle_id: Option<WidgetId>,
    second_id: Option<WidgetId>,
    container_bounds: Rc<Cell<Rect>>,
}

impl SplitView {
    pub fn new(split: Signal<f32>) -> Self {
        Self {
            split,
            orientation: Orientation::Horizontal,
            min_first_size: DEFAULT_MIN_PANE_SIZE,
            min_second_size: DEFAULT_MIN_PANE_SIZE,
            divider_thickness: DEFAULT_DIVIDER_THICKNESS,
            enabled: true,
            first_pending: None,
            second_pending: None,
            first_id: None,
            handle_id: None,
            second_id: None,
            container_bounds: Rc::new(Cell::new(Rect::ZERO)),
        }
    }

    pub fn orientation(mut self, orientation: Orientation) -> Self {
        self.orientation = orientation;
        self
    }

    pub fn min_first_size(mut self, size: f32) -> Self {
        self.min_first_size = size.max(0.0);
        self
    }

    pub fn min_second_size(mut self, size: f32) -> Self {
        self.min_second_size = size.max(0.0);
        self
    }

    pub fn divider_thickness(mut self, thickness: f32) -> Self {
        self.divider_thickness = thickness.max(1.0);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn first(mut self, widget: impl Widget + 'static) -> Self {
        self.first_pending = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    pub fn first_id(mut self, id: WidgetId) -> Self {
        self.first_pending = Some(PendingChild::Id(id));
        self
    }

    pub fn second(mut self, widget: impl Widget + 'static) -> Self {
        self.second_pending = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    pub fn second_id(mut self, id: WidgetId) -> Self {
        self.second_pending = Some(PendingChild::Id(id));
        self
    }

    fn clamp_fraction(&self, bounds: Rect) -> f32 {
        let available = match self.orientation {
            Orientation::Horizontal => bounds.width,
            Orientation::Vertical => bounds.height,
        } - self.divider_thickness;

        if available <= 0.0 {
            return 0.5;
        }

        let min = (self.min_first_size / available).clamp(0.0, 1.0);
        let max = 1.0 - (self.min_second_size / available).clamp(0.0, 1.0);
        let (min, max) = if min <= max { (min, max) } else { (0.5, 0.5) };
        self.split.get().clamp(min, max)
    }
}

impl std::fmt::Debug for SplitView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SplitView")
            .field("orientation", &self.orientation)
            .field("enabled", &self.enabled)
            .field("split", &self.split.get())
            .finish()
    }
}

impl Widget for SplitView {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.split
            .bind_to(self_id, registry, BindingLevel::Relayout);

        if let Some(pending) = self.first_pending.take() {
            self.first_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(widget) => ctx.add_boxed(widget),
            });
        }

        if let Some(pending) = self.second_pending.take() {
            self.second_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(widget) => ctx.add_boxed(widget),
            });
        }

        self.handle_id = Some(ctx.add(SplitHandle::new(
            self.split.clone(),
            self.orientation,
            self.min_first_size,
            self.min_second_size,
            self.divider_thickness,
            self.enabled,
            self.container_bounds.clone(),
        )));

        self.children()
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        let first_size = self
            .first_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or(Size::ZERO);
        let second_size = self
            .second_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or(Size::ZERO);

        match self.orientation {
            Orientation::Horizontal => Size::new(
                first_size.width + self.divider_thickness + second_size.width,
                first_size.height.max(second_size.height),
            ),
            Orientation::Vertical => Size::new(
                first_size.width.max(second_size.width),
                first_size.height + self.divider_thickness + second_size.height,
            ),
        }
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        self.container_bounds.set(bounds);
        if children.len() != 3 {
            return;
        }

        let split = self.clamp_fraction(bounds);
        let available = match self.orientation {
            Orientation::Horizontal => (bounds.width - self.divider_thickness).max(0.0),
            Orientation::Vertical => (bounds.height - self.divider_thickness).max(0.0),
        };
        let first_main = available * split;
        let second_main = available - first_main;

        match self.orientation {
            Orientation::Horizontal => {
                children[0].origin = Point::new(bounds.x, bounds.y);
                children[0].size = Size::new(first_main, bounds.height);

                children[1].origin = Point::new(bounds.x + first_main, bounds.y);
                children[1].size = Size::new(self.divider_thickness, bounds.height);

                children[2].origin =
                    Point::new(bounds.x + first_main + self.divider_thickness, bounds.y);
                children[2].size = Size::new(second_main, bounds.height);
            }
            Orientation::Vertical => {
                children[0].origin = Point::new(bounds.x, bounds.y);
                children[0].size = Size::new(bounds.width, first_main);

                children[1].origin = Point::new(bounds.x, bounds.y + first_main);
                children[1].size = Size::new(bounds.width, self.divider_thickness);

                children[2].origin =
                    Point::new(bounds.x, bounds.y + first_main + self.divider_thickness);
                children[2].size = Size::new(bounds.width, second_main);
            }
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        [self.first_id, self.handle_id, self.second_id]
            .into_iter()
            .flatten()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::event::Modifiers;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);

    impl Widget for FixedLeaf {
        fn size_that_fits(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
            Size::new(self.0, self.1)
        }
    }

    #[test]
    fn horizontal_split_places_panes_and_divider() {
        let split = Signal::new(0.25_f32);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let root = tree.add(
            SplitView::new(split)
                .first(FixedLeaf(100.0, 40.0))
                .second(FixedLeaf(100.0, 40.0)),
        );

        tree.layout(SizeProposal::exact(400.0, 200.0));

        let first = tree.child_widget(root, 0);
        let handle = tree.child_widget(root, 1);
        let second = tree.child_widget(root, 2);

        assert!((tree.bounds(first).width - 97.0).abs() < 0.01);
        assert!((tree.bounds(handle).width - DEFAULT_DIVIDER_THICKNESS).abs() < 0.01);
        assert!((tree.bounds(second).width - 291.0).abs() < 0.01);
    }

    #[test]
    fn drag_updates_split_fraction() {
        let split = Signal::new(0.5_f32);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let root = tree.add(
            SplitView::new(split.clone())
                .first(FixedLeaf(100.0, 40.0))
                .second(FixedLeaf(100.0, 40.0)),
        );

        tree.layout(SizeProposal::exact(400.0, 200.0));

        let handle = tree.child_widget(root, 1);
        let start = tree.bounds(handle).center();
        let end = Point::new(start.x + 80.0, start.y);
        tree.drag(start, end);

        assert!(
            split.get() > 0.65,
            "split should move right, got {}",
            split.get()
        );
    }

    #[test]
    fn keyboard_resizes_focused_splitter() {
        let split = Signal::new(0.5_f32);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let root = tree.add(
            SplitView::new(split.clone())
                .first(FixedLeaf(100.0, 40.0))
                .second(FixedLeaf(100.0, 40.0)),
        );

        tree.layout(SizeProposal::exact(400.0, 200.0));

        let handle = tree.child_widget(root, 1);
        tree.focus(handle);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);

        assert!(split.get() > 0.5);
        assert_eq!(tree.focused(), Some(handle));
    }

    #[test]
    fn minimum_sizes_clamp_fraction() {
        let split = Signal::new(0.05_f32);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let root = tree.add(
            SplitView::new(split)
                .min_first_size(120.0)
                .min_second_size(120.0)
                .first(FixedLeaf(100.0, 40.0))
                .second(FixedLeaf(100.0, 40.0)),
        );

        tree.layout(SizeProposal::exact(300.0, 160.0));

        let first = tree.child_widget(root, 0);
        let second = tree.child_widget(root, 2);
        assert!(tree.bounds(first).width >= 119.99);
        assert!(tree.bounds(second).width >= 119.99);
    }

    #[test]
    fn splitter_exposes_accessibility_role() {
        let split = Signal::new(0.5_f32);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(
            SplitView::new(split)
                .first(FixedLeaf(100.0, 40.0))
                .second(FixedLeaf(100.0, 40.0)),
        );

        tree.layout(SizeProposal::exact(400.0, 200.0));

        let handle = tree
            .find_by_role(fern_core::accesskit::Role::Splitter)
            .unwrap();
        let info = tree.accessibility_node(handle);
        assert_eq!(info.role(), fern_core::accesskit::Role::Splitter);
        assert!(
            info.actions()
                .contains(&fern_core::accesskit::Action::Increment)
        );
    }
}
