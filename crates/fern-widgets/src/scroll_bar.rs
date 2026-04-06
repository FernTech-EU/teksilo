use std::cell::Cell;

use fern_canvas::{Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::event::{EventResponse, PointerButton, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::widget::{EventContext, LayoutContext, PaintContext, Widget};
use fern_tokens::CornerRadius;

/// Orientation of the scroll bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollBarOrientation {
    Vertical,
    Horizontal,
}

/// A standalone Level 2 scroll bar widget.
///
/// The ScrollBar reads and writes a shared `Signal<f32>` for the scroll
/// position, and reads a `Signal<f32>` for the content-to-viewport ratio
/// (viewport_size / content_size, clamped to 0.0..1.0).
///
/// Supports:
/// - Thumb drag to scroll
/// - Track click to page-scroll toward the click position
/// - Keyboard: Up/Down (vertical) or Left/Right (horizontal) to step
/// - Accessibility: `Role::ScrollBar` with `set_numeric_value`
pub struct ScrollBar {
    orientation: ScrollBarOrientation,
    /// Scroll position: 0.0 = start, max_scroll = end.
    /// Shared with ScrollArea — both read and write.
    scroll_position: Signal<f32>,
    /// Maximum scroll value (content_size - viewport_size).
    /// Written by the ScrollArea, read by the ScrollBar.
    max_scroll: Signal<f32>,
    /// Viewport / content ratio (0.0..1.0). Determines thumb size.
    /// Written by the ScrollArea, read by the ScrollBar.
    viewport_ratio: Signal<f32>,

    // --- interaction state ---
    /// Whether the pointer is over the scroll bar.
    hovered: Cell<bool>,
    /// Whether the thumb is being dragged.
    dragging: Cell<bool>,
    /// Pointer position at drag start (in scroll bar local coords).
    drag_start_pointer: Cell<f32>,
    /// Scroll position at drag start.
    drag_start_scroll: Cell<f32>,
    /// Current bounds, cached from last layout for event handling.
    cached_bounds: Cell<Rect>,

    // --- visual tuning ---
    /// Thickness of the scroll bar (width for vertical, height for horizontal).
    thickness: f32,
    /// Minimum thumb length in pixels.
    min_thumb_length: f32,
    /// Pixels to scroll per keyboard step.
    step_size: f32,
    /// Whether to paint the track background (false for overlay-style scrollbars).
    show_track: bool,
}

impl std::fmt::Debug for ScrollBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollBar")
            .field("orientation", &self.orientation)
            .field("hovered", &self.hovered.get())
            .field("dragging", &self.dragging.get())
            .finish()
    }
}

impl ScrollBar {
    /// Create a new ScrollBar with shared state.
    ///
    /// - `scroll_position`: shared Signal<f32> for current scroll offset
    /// - `max_scroll`: shared Signal<f32> for maximum scroll offset
    /// - `viewport_ratio`: shared Signal<f32> for viewport/content ratio (0.0..1.0)
    pub fn new(
        orientation: ScrollBarOrientation,
        scroll_position: Signal<f32>,
        max_scroll: Signal<f32>,
        viewport_ratio: Signal<f32>,
    ) -> Self {
        Self {
            orientation,
            scroll_position,
            max_scroll,
            viewport_ratio,
            hovered: Cell::new(false),
            dragging: Cell::new(false),
            drag_start_pointer: Cell::new(0.0),
            drag_start_scroll: Cell::new(0.0),
            cached_bounds: Cell::new(Rect::ZERO),
            thickness: 12.0,
            min_thumb_length: 24.0,
            step_size: 40.0,
            show_track: true,
        }
    }

    /// Set the bar thickness (width for vertical, height for horizontal).
    pub fn thickness(mut self, thickness: f32) -> Self {
        self.thickness = thickness;
        self
    }

    /// Set the minimum thumb length in pixels.
    pub fn min_thumb_length(mut self, len: f32) -> Self {
        self.min_thumb_length = len;
        self
    }

    /// Set the scroll step for keyboard navigation.
    pub fn step_size(mut self, step: f32) -> Self {
        self.step_size = step;
        self
    }

    /// Whether to paint the track background. Set to `false` for overlay-style
    /// scrollbars that should only show the thumb.
    pub fn show_track(mut self, show: bool) -> Self {
        self.show_track = show;
        self
    }

    // --- geometry helpers ---

    /// The total length of the track (along the scroll axis).
    fn track_length(&self) -> f32 {
        let bounds = self.cached_bounds.get();
        match self.orientation {
            ScrollBarOrientation::Vertical => bounds.height,
            ScrollBarOrientation::Horizontal => bounds.width,
        }
    }

    /// Computed thumb length based on viewport ratio.
    fn thumb_length(&self) -> f32 {
        let ratio = self.viewport_ratio.get().clamp(0.0, 1.0);
        let track = self.track_length();
        (track * ratio).max(self.min_thumb_length).min(track)
    }

    /// Thumb offset from the start of the track.
    fn thumb_offset(&self) -> f32 {
        let max = self.max_scroll.get();
        if max <= 0.0 {
            return 0.0;
        }
        let pos = self.scroll_position.get();
        let ratio = (pos / max).clamp(0.0, 1.0);
        let available = self.track_length() - self.thumb_length();
        ratio * available
    }

    /// The thumb rect in absolute coordinates.
    fn thumb_rect(&self) -> Rect {
        let bounds = self.cached_bounds.get();
        let offset = self.thumb_offset();
        let thumb_len = self.thumb_length();
        match self.orientation {
            ScrollBarOrientation::Vertical => {
                Rect::new(bounds.x, bounds.y + offset, bounds.width, thumb_len)
            }
            ScrollBarOrientation::Horizontal => {
                Rect::new(bounds.x + offset, bounds.y, thumb_len, bounds.height)
            }
        }
    }

    /// Extract the coordinate along the scroll axis from a point.
    fn axis_value(&self, point: Point) -> f32 {
        match self.orientation {
            ScrollBarOrientation::Vertical => point.y,
            ScrollBarOrientation::Horizontal => point.x,
        }
    }

    /// Whether a point is inside the thumb.
    fn point_in_thumb(&self, point: Point) -> bool {
        self.thumb_rect().contains(point)
    }

    /// Set scroll position, clamped to valid range.
    fn set_scroll(&self, value: f32) {
        let max = self.max_scroll.get();
        self.scroll_position.set(value.clamp(0.0, max));
    }

    /// Scroll one page toward the given point (track click behavior).
    fn page_scroll_toward(&self, point: Point) {
        let click_axis = self.axis_value(point);
        let thumb = self.thumb_rect();
        let thumb_center = match self.orientation {
            ScrollBarOrientation::Vertical => thumb.y + thumb.height / 2.0,
            ScrollBarOrientation::Horizontal => thumb.x + thumb.width / 2.0,
        };

        let max = self.max_scroll.get();
        if max <= 0.0 {
            return;
        }

        // Page scroll: move by one viewport's worth in the direction of the click
        // viewport_ratio = viewport / content, max = content - viewport
        // viewport = max * ratio / (1 - ratio)
        let ratio = self.viewport_ratio.get().clamp(0.001, 0.999);
        let viewport_scroll = max * ratio / (1.0 - ratio);
        let current = self.scroll_position.get();
        if click_axis < thumb_center {
            self.set_scroll(current - viewport_scroll);
        } else {
            self.set_scroll(current + viewport_scroll);
        }
    }
}

impl Widget for ScrollBar {
    fn build(
        &mut self,
        ctx: &mut fern_core::build_context::BuildContext,
    ) -> Vec<fern_core::widget_id::WidgetId> {
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.scroll_position.bind_to(
            self_id,
            registry,
            fern_core::state::BindingLevel::RepaintOnly,
        );
        self.max_scroll.bind_to(
            self_id,
            registry,
            fern_core::state::BindingLevel::RepaintOnly,
        );
        self.viewport_ratio.bind_to(
            self_id,
            registry,
            fern_core::state::BindingLevel::RepaintOnly,
        );
        Vec::new()
    }

    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        match self.orientation {
            ScrollBarOrientation::Vertical => {
                Size::new(self.thickness, proposal.height.unwrap_or(100.0))
            }
            ScrollBarOrientation::Horizontal => {
                Size::new(proposal.width.unwrap_or(100.0), self.thickness)
            }
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut fern_canvas::Canvas, ctx: &PaintContext) {
        // Cache bounds for event handling
        self.cached_bounds.set(bounds);

        let max = self.max_scroll.get();
        if max <= 0.0 {
            // Nothing to scroll — don't paint
            return;
        }

        let radius = CornerRadius::uniform(self.thickness / 2.0);
        let dragging = self.dragging.get();
        let hovered = self.hovered.get();

        // Track background (skipped for overlay-style scrollbars)
        if self.show_track {
            let track_color = ctx
                .theme
                .colors
                .on_surface
                .with_alpha(if hovered || dragging { 0.08 } else { 0.04 });
            canvas.fill_rounded_rect(bounds, radius, track_color);
        }

        // Thumb
        let thumb = self.thumb_rect();
        let thumb_color = ctx.theme.colors.on_surface.with_alpha(if dragging {
            0.6
        } else if hovered {
            0.4
        } else {
            0.25
        });
        canvas.fill_rounded_rect(thumb, radius, thumb_color);
    }

    fn event(&mut self, event: &WidgetEvent, ctx: &mut EventContext) -> EventResponse {
        let max = self.max_scroll.get();
        if max <= 0.0 {
            return EventResponse::Ignored;
        }

        match event {
            WidgetEvent::PointerEnter => {
                self.hovered.set(true);
                EventResponse::Handled
            }
            WidgetEvent::PointerLeave => {
                self.hovered.set(false);
                EventResponse::Handled
            }
            WidgetEvent::PointerDown {
                position,
                button: PointerButton::Primary,
            } => {
                if self.point_in_thumb(*position) {
                    // Start thumb drag
                    self.dragging.set(true);
                    self.drag_start_pointer.set(self.axis_value(*position));
                    self.drag_start_scroll.set(self.scroll_position.get());
                    ctx.capture_pointer();
                } else {
                    // Track click — page scroll toward click position
                    self.page_scroll_toward(*position);
                }
                EventResponse::Handled
            }
            WidgetEvent::PointerMove { position } => {
                if self.dragging.get() {
                    let current = self.axis_value(*position);
                    let delta_pixels = current - self.drag_start_pointer.get();

                    // Convert pixel delta to scroll delta
                    let available = self.track_length() - self.thumb_length();
                    if available > 0.0 {
                        let scroll_delta = delta_pixels * max / available;
                        self.set_scroll(self.drag_start_scroll.get() + scroll_delta);
                    }
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::PointerUp {
                button: PointerButton::Primary,
                ..
            } => {
                if self.dragging.get() {
                    self.dragging.set(false);
                    ctx.release_pointer();
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::KeyDown { key, .. } => {
                use fern_core::event::Key;
                let step = self.step_size;
                match (self.orientation, key) {
                    (ScrollBarOrientation::Vertical, Key::ArrowUp) => {
                        self.set_scroll(self.scroll_position.get() - step);
                        EventResponse::Handled
                    }
                    (ScrollBarOrientation::Vertical, Key::ArrowDown) => {
                        self.set_scroll(self.scroll_position.get() + step);
                        EventResponse::Handled
                    }
                    (ScrollBarOrientation::Horizontal, Key::ArrowLeft) => {
                        self.set_scroll(self.scroll_position.get() - step);
                        EventResponse::Handled
                    }
                    (ScrollBarOrientation::Horizontal, Key::ArrowRight) => {
                        self.set_scroll(self.scroll_position.get() + step);
                        EventResponse::Handled
                    }
                    (_, Key::Home) => {
                        self.set_scroll(0.0);
                        EventResponse::Handled
                    }
                    (_, Key::End) => {
                        self.set_scroll(max);
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }
            }
            WidgetEvent::AccessAction { action, .. } => {
                if *action == fern_core::accesskit::Action::SetValue {
                    // AccessKit SetValue — the actual value would come from
                    // the action data in a real implementation. For now, no-op.
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            }
            _ => EventResponse::Ignored,
        }
    }

    fn is_focusable(&self) -> bool {
        true
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::ScrollBar);

        let pos = self.scroll_position.get();
        let max = self.max_scroll.get();

        builder.inner_mut().set_numeric_value(pos as f64);
        builder.inner_mut().set_min_numeric_value(0.0);
        builder.inner_mut().set_max_numeric_value(max as f64);

        match self.orientation {
            ScrollBarOrientation::Vertical => {
                builder
                    .inner_mut()
                    .set_orientation(fern_core::accesskit::Orientation::Vertical);
            }
            ScrollBarOrientation::Horizontal => {
                builder
                    .inner_mut()
                    .set_orientation(fern_core::accesskit::Orientation::Horizontal);
            }
        }

        builder.add_action(fern_core::accesskit::Action::SetValue);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_canvas::SizeProposal;
    use fern_core::widget_tree::WidgetTree;

    fn make_scrollbar() -> (ScrollBar, Signal<f32>, Signal<f32>, Signal<f32>) {
        let position = Signal::new(0.0_f32);
        let max_scroll = Signal::new(500.0_f32);
        let viewport_ratio = Signal::new(0.5_f32); // viewport is half of content

        let bar = ScrollBar::new(
            ScrollBarOrientation::Vertical,
            position.clone(),
            max_scroll.clone(),
            viewport_ratio.clone(),
        );
        (bar, position, max_scroll, viewport_ratio)
    }

    #[test]
    fn vertical_scrollbar_size() {
        let (bar, ..) = make_scrollbar();
        let mut tree = WidgetTree::new();
        let id = tree.add(bar);
        tree.layout(SizeProposal {
            width: None,
            height: Some(400.0),
        });

        let bounds = tree.bounds(id);
        // Vertical: width = thickness (12), height = proposed (400)
        assert!((bounds.width - 12.0).abs() < 0.01);
        assert!((bounds.height - 400.0).abs() < 0.01);
    }

    #[test]
    fn horizontal_scrollbar_size() {
        let position = Signal::new(0.0_f32);
        let max_scroll = Signal::new(500.0_f32);
        let viewport_ratio = Signal::new(0.5_f32);

        let bar = ScrollBar::new(
            ScrollBarOrientation::Horizontal,
            position,
            max_scroll,
            viewport_ratio,
        );
        let mut tree = WidgetTree::new();
        let id = tree.add(bar);
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: None,
        });

        let bounds = tree.bounds(id);
        // Horizontal: width = proposed (400), height = thickness (12)
        assert!((bounds.width - 400.0).abs() < 0.01);
        assert!((bounds.height - 12.0).abs() < 0.01);
    }

    #[test]
    fn scrollbar_thumb_drag_updates_position() {
        let (bar, position, _max, _ratio) = make_scrollbar();
        let mut tree = WidgetTree::new();
        let id = tree.add(bar);
        tree.layout(SizeProposal::exact(12.0, 400.0));

        // Render once to cache bounds
        tree.render();

        // Initial position is 0
        assert!((position.get() - 0.0).abs() < 0.01);

        // Pointer down on the thumb (which starts at top)
        tree.pointer_move(Point::new(6.0, 10.0));
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(6.0, 10.0),
            button: PointerButton::Primary,
        });

        // Drag 100px down: track is 400px, thumb is 200px (50% ratio),
        // so available travel = 200px, 100px drag = 50% of travel = 250 scroll
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(6.0, 110.0),
        });

        let pos = position.get();
        assert!(pos > 200.0, "Expected scroll > 200, got {}", pos);
        assert!(pos < 300.0, "Expected scroll < 300, got {}", pos);
    }

    #[test]
    fn scrollbar_clamps_to_range() {
        let (bar, position, max_scroll, ..) = make_scrollbar();
        let mut tree = WidgetTree::new();
        tree.add(bar);
        tree.layout(SizeProposal::exact(12.0, 400.0));
        tree.render();

        // Repeatedly click far below the thumb to page-scroll forward
        // until we hit the maximum (500). Each page scroll adds 250,
        // so after 3 clicks the position should be clamped at 500.
        for _ in 0..5 {
            tree.pointer_move(Point::new(6.0, 390.0));
            tree.dispatch_event(WidgetEvent::PointerDown {
                position: Point::new(6.0, 390.0),
                button: PointerButton::Primary,
            });
            // Release so next click isn't a drag
            tree.dispatch_event(WidgetEvent::PointerUp {
                position: Point::new(6.0, 390.0),
                button: PointerButton::Primary,
            });
        }

        let pos = position.get();
        let max = max_scroll.get();
        assert!(
            (pos - max).abs() < 0.01,
            "Expected pos to be clamped at max={}, got {}",
            max,
            pos,
        );
    }

    #[test]
    fn scrollbar_nothing_to_scroll() {
        let position = Signal::new(0.0_f32);
        let max_scroll = Signal::new(0.0_f32); // content fits in viewport
        let viewport_ratio = Signal::new(1.0_f32);

        let bar = ScrollBar::new(
            ScrollBarOrientation::Vertical,
            position,
            max_scroll,
            viewport_ratio,
        );
        let mut tree = WidgetTree::new();
        tree.add(bar);
        tree.layout(SizeProposal::exact(12.0, 400.0));

        let frame = tree.render();
        // When max_scroll is 0, paint returns early — no shapes
        assert!(
            frame.shapes.is_empty(),
            "Expected no rendering when nothing to scroll"
        );
    }

    #[test]
    fn scrollbar_accessibility() {
        let (bar, position, max_scroll, _ratio) = make_scrollbar();
        position.set(100.0);

        let mut tree = WidgetTree::new();
        let id = tree.add(bar);
        tree.layout(SizeProposal::exact(12.0, 400.0));

        let info = tree.accessibility_node(id);
        assert_eq!(info.role(), fern_core::accesskit::Role::ScrollBar);
    }

    #[test]
    fn track_click_pages_forward() {
        let (bar, position, ..) = make_scrollbar();
        let mut tree = WidgetTree::new();
        let id = tree.add(bar);
        tree.layout(SizeProposal::exact(12.0, 400.0));
        tree.render();

        // Click on the track below the thumb (thumb starts at top, ~200px tall)
        tree.pointer_move(Point::new(6.0, 350.0));
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(6.0, 350.0),
            button: PointerButton::Primary,
        });

        let pos = position.get();
        assert!(
            pos > 0.0,
            "Expected positive scroll after track click, got {}",
            pos
        );
    }

    #[test]
    fn drag_release_outside_does_not_stick() {
        // Regression test: dragging the thumb and releasing outside the
        // scrollbar must not leave `dragging` stuck to true. This requires
        // pointer capture so that PointerUp reaches the scrollbar even when
        // the pointer is outside its bounds.
        let (bar, position, ..) = make_scrollbar();
        let mut tree = WidgetTree::new();
        let id = tree.add(bar);
        tree.layout(SizeProposal::exact(12.0, 400.0));
        tree.render();

        // Start drag on the thumb
        tree.pointer_move(Point::new(6.0, 10.0));
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(6.0, 10.0),
            button: PointerButton::Primary,
        });

        // Move far outside the scrollbar bounds
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(200.0, 300.0),
        });

        // Release outside
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(200.0, 300.0),
            button: PointerButton::Primary,
        });

        // Now hover the scrollbar again — should NOT continue dragging
        let pos_before = position.get();
        tree.pointer_move(Point::new(6.0, 50.0));
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(6.0, 50.0),
        });

        let pos_after = position.get();
        assert!(
            (pos_after - pos_before).abs() < 0.01,
            "Hovering after release should not move scroll: before={}, after={}",
            pos_before,
            pos_after,
        );
    }
}
