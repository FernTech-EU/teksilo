use std::cell::Cell;
use std::rc::Rc;

use fern_canvas::{Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::event::{EventResponse, PointerButton, WidgetEvent};
use fern_core::gesture::DragPhase;
use fern_core::signal::Signal;
use fern_core::widget::{LayoutContext, PaintContext, Widget};
use fern_core::widget_builder::HandlerSet;
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
    hovered: Rc<Cell<bool>>,
    /// Whether the thumb is being dragged.
    dragging: Rc<Cell<bool>>,
    /// Pointer position at drag start (in scroll bar local coords).
    drag_start_pointer: Rc<Cell<f32>>,
    /// Scroll position at drag start.
    drag_start_scroll: Rc<Cell<f32>>,
    /// Current bounds, cached from last layout for event handling.
    cached_bounds: Rc<Cell<Rect>>,

    // --- visual tuning ---
    /// Thickness of the scroll bar (width for vertical, height for horizontal).
    thickness: f32,
    /// Minimum thumb length in pixels.
    min_thumb_length: f32,
    /// Pixels to scroll per keyboard step.
    step_size: f32,
    /// Whether to paint the track background (false for overlay-style scrollbars).
    show_track: bool,
    /// Ubuntu-style overlay mode: thin indicator at rest, full-size on hover.
    overlay_mode: bool,
    /// Thickness of the thin resting indicator in overlay mode.
    resting_thickness: f32,
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
        // Defaults sourced from `ScrollBarStyle` (Int UI: 8 dp on hover,
        // 4 dp at idle, 24 dp minimum thumb length).
        Self {
            orientation,
            scroll_position,
            max_scroll,
            viewport_ratio,
            hovered: Rc::new(Cell::new(false)),
            dragging: Rc::new(Cell::new(false)),
            drag_start_pointer: Rc::new(Cell::new(0.0)),
            drag_start_scroll: Rc::new(Cell::new(0.0)),
            cached_bounds: Rc::new(Cell::new(Rect::ZERO)),
            thickness: 8.0,
            min_thumb_length: 24.0,
            step_size: 40.0,
            show_track: true,
            overlay_mode: false,
            resting_thickness: 4.0,
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

    /// Enable Ubuntu-style overlay mode: thin indicator at rest, full-size
    /// scrollbar with track on hover.
    pub fn overlay_mode(mut self, enabled: bool) -> Self {
        self.overlay_mode = enabled;
        if enabled {
            self.show_track = false; // track only shown on hover
        }
        self
    }

    /// Set the resting indicator thickness for overlay mode (default 3px).
    pub fn resting_thickness(mut self, thickness: f32) -> Self {
        self.resting_thickness = thickness;
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
            fern_core::binding::BindingLevel::RepaintOnly,
        );
        self.max_scroll.bind_to(
            self_id,
            registry,
            fern_core::binding::BindingLevel::RepaintOnly,
        );
        self.viewport_ratio.bind_to(
            self_id,
            registry,
            fern_core::binding::BindingLevel::RepaintOnly,
        );

        let scroll_position = self.scroll_position.clone();
        let max_scroll = self.max_scroll.clone();
        let viewport_ratio = self.viewport_ratio.clone();
        let orientation = self.orientation;
        let hovered = self.hovered.clone();
        let dragging = self.dragging.clone();
        let drag_start_pointer = self.drag_start_pointer.clone();
        let drag_start_scroll = self.drag_start_scroll.clone();
        let cached_bounds = self.cached_bounds.clone();
        let step_size = self.step_size;
        let min_thumb_length = self.min_thumb_length;

        let axis_value = move |point: Point| -> f32 {
            match orientation {
                ScrollBarOrientation::Vertical => point.y,
                ScrollBarOrientation::Horizontal => point.x,
            }
        };

        let set_scroll = {
            let scroll_position = scroll_position.clone();
            let max_scroll = max_scroll.clone();
            move |value: f32| {
                let max = max_scroll.get();
                scroll_position.set(value.clamp(0.0, max));
            }
        };

        let track_length = {
            let cached_bounds = cached_bounds.clone();
            move || -> f32 {
                let bounds = cached_bounds.get();
                match orientation {
                    ScrollBarOrientation::Vertical => bounds.height,
                    ScrollBarOrientation::Horizontal => bounds.width,
                }
            }
        };

        let thumb_length = {
            let viewport_ratio = viewport_ratio.clone();
            let track_length = track_length.clone();
            move || -> f32 {
                let ratio = viewport_ratio.get().clamp(0.0, 1.0);
                let track = track_length();
                (track * ratio).max(min_thumb_length).min(track)
            }
        };

        let thumb_rect = {
            let cached_bounds = cached_bounds.clone();
            let scroll_position = scroll_position.clone();
            let max_scroll = max_scroll.clone();
            let track_length = track_length.clone();
            let thumb_length = thumb_length.clone();
            move || -> Rect {
                let bounds = cached_bounds.get();
                let max = max_scroll.get();
                let offset = if max <= 0.0 {
                    0.0
                } else {
                    let pos = scroll_position.get();
                    let ratio = (pos / max).clamp(0.0, 1.0);
                    let available = track_length() - thumb_length();
                    ratio * available
                };
                let tl = thumb_length();
                match orientation {
                    ScrollBarOrientation::Vertical => {
                        Rect::new(bounds.x, bounds.y + offset, bounds.width, tl)
                    }
                    ScrollBarOrientation::Horizontal => {
                        Rect::new(bounds.x + offset, bounds.y, tl, bounds.height)
                    }
                }
            }
        };

        let mut handlers = HandlerSet::new().focusable(true);

        // Thumb drag — routed through the typed gesture API. The
        // framework auto-captures the pointer on `DragPhase::Started`
        // and releases it on `DragPhase::Ended`, so thumb drags that
        // leave the widget bounds keep firing.
        //
        // A drag that began off the thumb (e.g. on the track) is
        // deliberately ignored: the `dragging` signal only flips true
        // when the initial press was on the thumb, and track clicks
        // are handled by `on_tap` below.
        {
            let dragging = dragging.clone();
            let drag_start_pointer = drag_start_pointer.clone();
            let drag_start_scroll = drag_start_scroll.clone();
            let scroll_position = scroll_position.clone();
            let max_scroll = max_scroll.clone();
            let set_scroll = set_scroll.clone();
            let thumb_rect = thumb_rect.clone();
            let track_length = track_length.clone();
            let thumb_length = thumb_length.clone();
            handlers = handlers.on_drag(move |phase, _ctx| {
                let max = max_scroll.get();
                if max <= 0.0 {
                    return;
                }
                match phase {
                    DragPhase::Started {
                        position,
                        button: PointerButton::Primary,
                    } => {
                        if thumb_rect().contains(position) {
                            dragging.set(true);
                            drag_start_pointer.set(axis_value(position));
                            drag_start_scroll.set(scroll_position.get());
                        }
                    }
                    DragPhase::Moved { position, .. } if dragging.get() => {
                        let current = axis_value(position);
                        let delta_pixels = current - drag_start_pointer.get();
                        let available = track_length() - thumb_length();
                        if available > 0.0 {
                            let scroll_delta = delta_pixels * max / available;
                            set_scroll(drag_start_scroll.get() + scroll_delta);
                        }
                    }
                    DragPhase::Ended { .. } => {
                        dragging.set(false);
                    }
                    _ => {}
                }
            });
        }

        // Track click — page-scroll toward the click position.
        // The tap recognizer only fires on press+release without
        // movement past the 5 px threshold, so a thumb grab that
        // starts as a click but becomes a drag is handled by the
        // `on_drag` arm above and never reaches here.
        {
            let scroll_position = scroll_position.clone();
            let max_scroll = max_scroll.clone();
            let viewport_ratio = viewport_ratio.clone();
            let set_scroll = set_scroll.clone();
            let thumb_rect = thumb_rect.clone();
            handlers = handlers.on_tap(move |position, _ctx| {
                let max = max_scroll.get();
                if max <= 0.0 {
                    return;
                }
                let tr = thumb_rect();
                if tr.contains(position) {
                    return;
                }
                let click_axis = axis_value(position);
                let thumb_center = match orientation {
                    ScrollBarOrientation::Vertical => tr.y + tr.height / 2.0,
                    ScrollBarOrientation::Horizontal => tr.x + tr.width / 2.0,
                };
                let ratio = viewport_ratio.get().clamp(0.001, 0.999);
                let viewport_scroll = max * ratio / (1.0 - ratio);
                let current = scroll_position.get();
                if click_axis < thumb_center {
                    set_scroll(current - viewport_scroll);
                } else {
                    set_scroll(current + viewport_scroll);
                }
            });
        }

        // Hover handler
        {
            let hovered = hovered.clone();
            handlers = handlers.on_hover(move |entered, _ctx| {
                hovered.set(entered);
            });
        }

        // Key handler
        {
            let scroll_position = scroll_position.clone();
            let max_scroll = max_scroll.clone();
            let set_scroll = set_scroll.clone();
            handlers = handlers.on_key(move |event, _ctx| {
                let max = max_scroll.get();
                if max <= 0.0 {
                    return EventResponse::Ignored;
                }
                match event {
                    WidgetEvent::KeyDown { key, .. } => {
                        use fern_core::event::Key;
                        let step = step_size;
                        match (orientation, key) {
                            (ScrollBarOrientation::Vertical, Key::ArrowUp) => {
                                set_scroll(scroll_position.get() - step);
                                EventResponse::Handled
                            }
                            (ScrollBarOrientation::Vertical, Key::ArrowDown) => {
                                set_scroll(scroll_position.get() + step);
                                EventResponse::Handled
                            }
                            (ScrollBarOrientation::Horizontal, Key::ArrowLeft) => {
                                set_scroll(scroll_position.get() - step);
                                EventResponse::Handled
                            }
                            (ScrollBarOrientation::Horizontal, Key::ArrowRight) => {
                                set_scroll(scroll_position.get() + step);
                                EventResponse::Handled
                            }
                            (_, Key::Home) => {
                                set_scroll(0.0);
                                EventResponse::Handled
                            }
                            (_, Key::End) => {
                                set_scroll(max);
                                EventResponse::Handled
                            }
                            _ => EventResponse::Ignored,
                        }
                    }
                    _ => EventResponse::Ignored,
                }
            });
        }

        // Access action handler
        {
            handlers = handlers.on_access_action(move |action, _ctx| {
                if action == fern_core::accesskit::Action::SetValue {
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            });
        }

        ctx.apply_self_handlers(handlers);

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
            return;
        }

        let dragging = self.dragging.get();
        let hovered = self.hovered.get();
        let active = hovered || dragging;

        if self.overlay_mode && !active {
            // --- Resting state: thin indicator aligned to trailing edge ---
            let thin = self.resting_thickness;
            let thin_bounds = match self.orientation {
                ScrollBarOrientation::Vertical => {
                    Rect::new(bounds.right() - thin, bounds.y, thin, bounds.height)
                }
                ScrollBarOrientation::Horizontal => {
                    Rect::new(bounds.x, bounds.bottom() - thin, bounds.width, thin)
                }
            };
            let radius = CornerRadius::uniform(thin / 2.0);
            // Thin thumb
            let offset = self.thumb_offset();
            let thumb_len = self.thumb_length();
            let thumb_rect = match self.orientation {
                ScrollBarOrientation::Vertical => {
                    Rect::new(thin_bounds.x, thin_bounds.y + offset, thin, thumb_len)
                }
                ScrollBarOrientation::Horizontal => {
                    Rect::new(thin_bounds.x + offset, thin_bounds.y, thumb_len, thin)
                }
            };
            // Thin resting indicator uses the idle thumb color.
            let thumb_color = ctx.theme.colors.scrollbar_thumb;
            canvas.fill_rounded_rect(thumb_rect, radius, thumb_color);
        } else {
            // --- Full-size state (Permanent, or Overlay when hovered/dragging) ---
            let radius = CornerRadius::uniform(self.thickness / 2.0);
            let colors = &ctx.theme.colors;

            // Track background — Int UI scrollbar track is transparent at idle
            // and a faint tint on hover. `show_track: true` (Permanent mode)
            // forces it visible.
            if self.show_track {
                canvas.fill_rounded_rect(bounds, radius, colors.scrollbar_track_hover);
            } else if self.overlay_mode && active {
                canvas.fill_rounded_rect(bounds, radius, colors.scrollbar_track_hover);
            }

            // Thumb — switch color by pressed/hover/idle state.
            let thumb = self.thumb_rect();
            let thumb_color = if dragging {
                colors.scrollbar_thumb_pressed
            } else if hovered {
                colors.scrollbar_thumb_hover
            } else {
                colors.scrollbar_thumb
            };
            canvas.fill_rounded_rect(thumb, radius, thumb_color);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::ScrollBar);

        let pos = self.scroll_position.get();
        let max = self.max_scroll.get();

        builder.set_numeric_value(pos as f64);
        builder.set_min_numeric_value(0.0);
        builder.set_max_numeric_value(max as f64);

        let orientation = match self.orientation {
            ScrollBarOrientation::Vertical => fern_core::accesskit::Orientation::Vertical,
            ScrollBarOrientation::Horizontal => fern_core::accesskit::Orientation::Horizontal,
        };
        builder.set_orientation(orientation);

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
        // Vertical: width = thickness (8), height = proposed (400)
        assert!((bounds.width - 8.0).abs() < 0.01);
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
        // Horizontal: width = proposed (400), height = thickness (8)
        assert!((bounds.width - 400.0).abs() < 0.01);
        assert!((bounds.height - 8.0).abs() < 0.01);
    }

    #[test]
    fn scrollbar_thumb_drag_updates_position() {
        let (bar, position, _max, _ratio) = make_scrollbar();
        let mut tree = WidgetTree::new();
        let _id = tree.add(bar);
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
            modifiers: fern_core::event::Modifiers::NONE,
        });

        // Drag 100px down: track is 400px, thumb is 200px (50% ratio),
        // so available travel = 200px, 100px drag = 50% of travel = 250 scroll.
        // DragRecognizer needs one move to cross the 5px threshold and emit
        // DragStarted (which carries the *down* position, so the thumb-vs-track
        // check latches on), then subsequent moves emit DragMoved with a delta
        // from the initial press.
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(6.0, 20.0),
        });
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
                modifiers: fern_core::event::Modifiers::NONE,
            });
            // Release so next click isn't a drag
            tree.dispatch_event(WidgetEvent::PointerUp {
                position: Point::new(6.0, 390.0),
                button: PointerButton::Primary,
                modifiers: fern_core::event::Modifiers::NONE,
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
        let (bar, position, _max_scroll, _ratio) = make_scrollbar();
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
        let _id = tree.add(bar);
        tree.layout(SizeProposal::exact(12.0, 400.0));
        tree.render();

        // Click on the track below the thumb (thumb starts at top, ~200px tall).
        // Track clicks are routed through `on_tap`, which requires a full
        // press+release sequence without the pointer crossing the drag
        // threshold.
        tree.pointer_move(Point::new(6.0, 350.0));
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(6.0, 350.0),
            button: PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(6.0, 350.0),
            button: PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });

        let pos = position.get();
        assert!(
            pos > 0.0,
            "Expected positive scroll after track click, got {}",
            pos
        );
    }

    #[test]
    fn scrollbar_drag_inside_scroll_area_updates_position() {
        // Regression: reproduces the real-app case where the ScrollBar
        // is a child of a ScrollArea (overlay mode), which wraps a tall
        // content widget. Before the V2 migration this worked through
        // `on_pointer_event`; the drag must keep working through the
        // typed `on_drag` + auto-capture path.
        use crate::primitives::MinSize;
        use crate::scroll_area::{ScrollArea, ScrollBarMode};
        use fern_canvas::Point;
        use fern_core::event::{Modifiers, PointerButton};

        let mut tree = WidgetTree::new();
        // Content is twice as tall as the ScrollArea viewport → v scrollbar
        // is needed with viewport_ratio = 0.5.
        let content = MinSize::new(400.0, 800.0);
        let root = tree.add(
            ScrollArea::new(content).scroll_bar_style(ScrollBarMode::Permanent),
        );
        tree.layout(SizeProposal::exact(400.0, 400.0));
        tree.render();

        // Find the vertical scrollbar child (second child of ScrollArea:
        // content is first, v-scrollbar second).
        let sb_id = tree.children(root)[1];
        let sb_bounds = tree.bounds(sb_id);
        assert!(sb_bounds.width > 0.0, "scrollbar should have non-zero width");
        assert!(
            sb_bounds.height > 0.0,
            "scrollbar should have non-zero height"
        );

        // Press in the middle of the thumb (thumb spans y=sb_bounds.y..+half).
        let thumb_cx = sb_bounds.x + sb_bounds.width / 2.0;
        let thumb_cy = sb_bounds.y + sb_bounds.height / 4.0;
        tree.pointer_move(Point::new(thumb_cx, thumb_cy));
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(thumb_cx, thumb_cy),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });

        // Cross the drag threshold…
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(thumb_cx, thumb_cy + 10.0),
        });
        // …and then actually drag down.
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(thumb_cx, thumb_cy + 100.0),
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(thumb_cx, thumb_cy + 100.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });

        // Apply the scroll-triggered relayout so the content's cached
        // bounds reflect the new scroll offset (the real event loop does
        // this automatically every frame).
        tree.layout(SizeProposal::exact(400.0, 400.0));

        // The scroll position should have advanced by a substantial amount
        // (a 100-px drag on a 400-px track with 50 % viewport ratio moves
        // the content ~200 px).
        let final_scroll = tree.hit_test(Point::new(1.0, 1.0)); // dummy, just keep borrow checker quiet
        let _ = final_scroll;
        // We can't read scroll_y directly from the public API; assert the
        // *bounds* of the content child moved in the ScrollArea's layout
        // rect — after layout the content's origin.y is `-scroll_y`.
        let content_bounds = tree.bounds(tree.children(root)[0]);
        assert!(
            content_bounds.y < -1.0,
            "content should have scrolled up (y < 0); got y={}",
            content_bounds.y
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
        let _id = tree.add(bar);
        tree.layout(SizeProposal::exact(12.0, 400.0));
        tree.render();

        // Start drag on the thumb
        tree.pointer_move(Point::new(6.0, 10.0));
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(6.0, 10.0),
            button: PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
        });

        // Move far outside the scrollbar bounds
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(200.0, 300.0),
        });

        // Release outside
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(200.0, 300.0),
            button: PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::NONE,
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
