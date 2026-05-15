//! Two-pane split container with a draggable divider.
//!
//! `SplitView` arranges a `first` and `second` child side-by-side
//! (or stacked, per [`Orientation`]) with a grabbable gutter between
//! them. The split position is driven by an external `Signal<f32>` in
//! the `[0.0, 1.0]` range, so callers can persist, animate, or bind it
//! to other UI. Minimum pane sizes and gutter thickness default to
//! `Theme.components.split_view` but can be overridden per-instance.
//!
//! The divider is exposed as a standalone internal widget
//! (`SplitHandle`) so it owns its own interaction state, keyboard
//! shortcuts, and accessibility node (`Role::Splitter`).

use std::cell::Cell;
use std::rc::Rc;

use fern_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::binding::BindingLevel;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, PointerButton, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::widget::{
    CursorIcon, EventContext, LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement,
};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::Orientation;

#[derive(Debug, Clone, Copy)]
struct SplitBounds {
    start: f32,
    available: f32,
    min: f32,
    max: f32,
    keyboard_step_px: f32,
}

impl SplitBounds {
    fn compute(
        bounds: Rect,
        orientation: Orientation,
        divider_thickness: f32,
        min_first_size: f32,
        min_second_size: f32,
        keyboard_step_px: f32,
    ) -> Option<Self> {
        let (start, total) = match orientation {
            Orientation::Horizontal => (bounds.x, bounds.width),
            Orientation::Vertical => (bounds.y, bounds.height),
        };
        let available = total - divider_thickness;
        if available <= 0.0 {
            return None;
        }
        let min = (min_first_size / available).clamp(0.0, 1.0);
        let max = 1.0 - (min_second_size / available).clamp(0.0, 1.0);
        let (min, max) = if min <= max { (min, max) } else { (0.5, 0.5) };
        Some(Self {
            start,
            available,
            min,
            max,
            keyboard_step_px,
        })
    }

    fn clamp(&self, fraction: f32) -> f32 {
        fraction.clamp(self.min, self.max)
    }

    fn keyboard_step(&self) -> f32 {
        (self.keyboard_step_px / self.available.max(1.0)).clamp(0.01, 0.2)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SplitHandleState {
    Idle,
    Hovered,
    Focused,
    Dragging,
}

/// Configuration for a `SplitHandle`. Lets `SplitView` pass theme-
/// resolved values (thickness, min pane sizes, keyboard step) and the
/// shared `container_bounds` cell in one grouped argument.
struct SplitHandleConfig {
    split: Signal<f32>,
    orientation: Orientation,
    min_first_size: f32,
    min_second_size: f32,
    divider_thickness: f32,
    keyboard_step_px: f32,
    enabled: bool,
    container_bounds: Rc<Cell<Rect>>,
}

struct SplitHandle {
    split: Signal<f32>,
    orientation: Orientation,
    min_first_size: f32,
    min_second_size: f32,
    divider_thickness: f32,
    keyboard_step_px: f32,
    enabled: bool,
    container_bounds: Rc<Cell<Rect>>,
    interaction: Signal<SplitHandleState>,
    /// Hover-dwell progress driving the focus-ring fade-in. The signal
    /// linearly animates 0→1 over `HOVER_DWELL_TOTAL` on hover-enter and
    /// is mapped in paint to "hold at 0 for `HOVER_DWELL_DELAY`, then
    /// fade in" via the (raw - delay_frac) / fade_frac formula. On
    /// hover-leave the same signal animates back to 0 over a short
    /// `HOVER_FADE_OUT` so the ring fades out cleanly.
    hover_progress: Signal<f32>,
}

/// Total animation duration for hover-enter. Split into a hold phase
/// (delay) and a visible fade-in phase. 300ms hold + 100ms fade-in
/// keeps the divider unobtrusive during incidental cursor crossings
/// while still feeling responsive once the user dwells.
const HOVER_DWELL_TOTAL: std::time::Duration = std::time::Duration::from_millis(400);
/// Portion of `HOVER_DWELL_TOTAL` spent at zero opacity before the
/// fade-in starts. Drives the (1.0 - HOVER_DWELL_DELAY/HOVER_DWELL_TOTAL)
/// fraction used in paint to map the linear signal to a delayed ramp.
const HOVER_DWELL_DELAY: std::time::Duration = std::time::Duration::from_millis(300);
/// Fade-out duration on hover-leave. Quick enough to feel snappy but
/// long enough to avoid a hard pop.
const HOVER_FADE_OUT: std::time::Duration = std::time::Duration::from_millis(120);

impl SplitHandle {
    fn new(config: SplitHandleConfig) -> Self {
        Self {
            split: config.split,
            orientation: config.orientation,
            min_first_size: config.min_first_size,
            min_second_size: config.min_second_size,
            divider_thickness: config.divider_thickness,
            keyboard_step_px: config.keyboard_step_px,
            enabled: config.enabled,
            container_bounds: config.container_bounds,
            interaction: Signal::new(SplitHandleState::Idle),
            hover_progress: Signal::new_animated(0.0),
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
        let registry = ctx.binding_registry();
        self.interaction
            .bind_to(self_id, registry, BindingLevel::RepaintOnly);
        self.hover_progress
            .bind_to(self_id, registry, BindingLevel::RepaintOnly);
        // hover_progress was created by SplitHandle::new() (outside the
        // tree), so register it with the animation scheduler now —
        // otherwise animate_to() silently no-ops.
        ctx.register_animated_signal(&self.hover_progress);
        let interaction = self.interaction.clone();
        let hover_progress = self.hover_progress.clone();

        let enabled = self.enabled;
        let orientation = self.orientation;
        let divider_thickness = self.divider_thickness;
        let min_first_size = self.min_first_size;
        let min_second_size = self.min_second_size;
        let keyboard_step_px = self.keyboard_step_px;
        let resize_cursor = match orientation {
            Orientation::Horizontal => CursorIcon::ColResize,
            Orientation::Vertical => CursorIcon::RowResize,
        };

        let drag_offset = Rc::new(Cell::new(0.0_f32));

        let set_from_position = {
            let split = self.split.clone();
            let container_bounds = self.container_bounds.clone();
            let drag_offset = drag_offset.clone();
            move |position: Point| {
                let Some(sb) = SplitBounds::compute(
                    container_bounds.get(),
                    orientation,
                    divider_thickness,
                    min_first_size,
                    min_second_size,
                    keyboard_step_px,
                ) else {
                    return;
                };
                let coordinate = match orientation {
                    Orientation::Horizontal => position.x,
                    Orientation::Vertical => position.y,
                };
                let divider_center = coordinate - drag_offset.get();
                let fraction = (divider_center - sb.start - divider_thickness / 2.0) / sb.available;
                split.set(sb.clamp(fraction));
            }
        };

        let handler_set = HandlerSet::new()
            .on_pointer_event({
                let interaction = interaction.clone();
                let split = self.split.clone();
                let container_bounds = self.container_bounds.clone();
                let drag_offset = drag_offset.clone();
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
                            // Record pointer offset relative to the divider's center so the
                            // splitter doesn't jump when the user grabs it near an edge.
                            if let Some(sb) = SplitBounds::compute(
                                container_bounds.get(),
                                orientation,
                                divider_thickness,
                                min_first_size,
                                min_second_size,
                                keyboard_step_px,
                            ) {
                                let coordinate = match orientation {
                                    Orientation::Horizontal => position.x,
                                    Orientation::Vertical => position.y,
                                };
                                let divider_center = sb.start
                                    + sb.available * sb.clamp(split.get())
                                    + divider_thickness / 2.0;
                                drag_offset.set(coordinate - divider_center);
                            } else {
                                drag_offset.set(0.0);
                            }
                            interaction.set(SplitHandleState::Dragging);
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
                                // Drop back to Hovered — focus ring is reserved for
                                // keyboard-initiated focus, not pointer interaction.
                                interaction.set(SplitHandleState::Hovered);
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
                let hover_progress = hover_progress.clone();
                move |entered, _ctx| {
                    if !enabled {
                        interaction.set(SplitHandleState::Idle);
                        hover_progress.animate_to(0.0, HOVER_FADE_OUT, fern_tokens::Easing::Linear);
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
                    if entered {
                        hover_progress.animate_to(
                            1.0,
                            HOVER_DWELL_TOTAL,
                            fern_tokens::Easing::Linear,
                        );
                    } else {
                        hover_progress.animate_to(0.0, HOVER_FADE_OUT, fern_tokens::Easing::Linear);
                    }
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
                let split = self.split.clone();
                let container_bounds = self.container_bounds.clone();
                let interaction = interaction.clone();
                move |event, _ctx| {
                    if !enabled {
                        return EventResponse::Ignored;
                    }

                    let Some(sb) = SplitBounds::compute(
                        container_bounds.get(),
                        orientation,
                        divider_thickness,
                        min_first_size,
                        min_second_size,
                        keyboard_step_px,
                    ) else {
                        return EventResponse::Ignored;
                    };
                    let step = sb.keyboard_step();

                    match event {
                        WidgetEvent::KeyDown { key, .. } => {
                            let mut next = split.get();
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
                                    next = sb.min;
                                    true
                                }
                                (_, Key::End) => {
                                    next = sb.max;
                                    true
                                }
                                _ => false,
                            };

                            if handled {
                                split.set(sb.clamp(next));
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

                    let Some(sb) = SplitBounds::compute(
                        container_bounds.get(),
                        orientation,
                        divider_thickness,
                        min_first_size,
                        min_second_size,
                        keyboard_step_px,
                    ) else {
                        return EventResponse::Ignored;
                    };
                    let step = sb.keyboard_step();

                    let delta = match action {
                        fern_core::accesskit::Action::Increment => Some(step),
                        fern_core::accesskit::Action::Decrement => Some(-step),
                        _ => None,
                    };

                    if let Some(delta) = delta {
                        split.set(sb.clamp(split.get() + delta));
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

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
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
        .into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let colors = &ctx.theme.colors;
        let interaction = self.interaction.get();

        // Int UI convention: thin static line at the gutter's center, no
        // visible handle, no filled background. The hit area is wider
        // than the line for comfortable pointer targeting; the cursor
        // change on hover is what tells the user it's grabbable.
        let line_thickness = SPLIT_VIEW_DIVIDER_LINE_THICKNESS.max(1.0);
        // Focus indicator is the same divider line drawn thicker with
        // the focus color — not a bounding stroke. Cross-fades over the
        // resting line via alpha during the hover-dwell phase, fully
        // replaces it on keyboard focus / drag.
        let focus_thickness = (line_thickness * 3.0).max(line_thickness + 2.0);

        let line_rect = |thickness: f32| match self.orientation {
            Orientation::Horizontal => Rect::new(
                bounds.x + (bounds.width - thickness) / 2.0,
                bounds.y,
                thickness,
                bounds.height,
            ),
            Orientation::Vertical => Rect::new(
                bounds.x,
                bounds.y + (bounds.height - thickness) / 2.0,
                bounds.width,
                thickness,
            ),
        };

        // Resting line — always present so the divider never disappears.
        canvas.fill_rect(line_rect(line_thickness), colors.border);

        // Focus indicator alpha: instant on keyboard focus / drag,
        // hover-dwell fade-in otherwise. The hover_progress signal
        // animates 0→1 linearly over HOVER_DWELL_TOTAL; this maps the
        // first 75% of progress to alpha=0 (the dwell delay) and the
        // last 25% to alpha 0..1 (the visible fade). Same signal
        // animates back to 0 on hover-leave so the indicator fades out
        // via the same formula.
        let focus_alpha = if !self.enabled {
            0.0
        } else if interaction == SplitHandleState::Focused
            || interaction == SplitHandleState::Dragging
        {
            1.0
        } else {
            let p = self.hover_progress.get();
            let delay_frac = HOVER_DWELL_DELAY.as_secs_f32() / HOVER_DWELL_TOTAL.as_secs_f32();
            ((p - delay_frac) / (1.0 - delay_frac)).clamp(0.0, 1.0)
        };

        if focus_alpha > 0.0 {
            canvas.fill_rect(
                line_rect(focus_thickness),
                colors.focus_ring.with_alpha(focus_alpha),
            );
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Splitter);
        builder.set_name(fern_i18n::tr_widget!(a11y_split_view_divider_name()).resolve_now());
        builder.set_numeric_value((self.split.get() * 100.0) as f64);
        builder.set_min_numeric_value(0.0);
        builder.set_max_numeric_value(100.0);
        builder.set_value(format!("{:.0}%", self.split.get() * 100.0));
        // ARIA `aria-orientation` on a separator describes the BAR's
        // own axis — not the split axis. A horizontal SplitView (panes
        // side-by-side) has a vertical handle bar, and vice versa.
        let handle_orientation = match self.orientation {
            Orientation::Horizontal => fern_core::accesskit::Orientation::Vertical,
            Orientation::Vertical => fern_core::accesskit::Orientation::Horizontal,
        };
        builder.set_orientation(handle_orientation);
        if !self.enabled {
            builder.set_disabled();
        } else {
            builder.add_action(fern_core::accesskit::Action::Focus);
            builder.add_action(fern_core::accesskit::Action::Increment);
            builder.add_action(fern_core::accesskit::Action::Decrement);
        }
    }
}

/// SplitView design tokens — relocated from
/// `theme.components.split_view` in Stage G of the styling migration.
pub const SPLIT_VIEW_GUTTER_THICKNESS: f32 = 6.0;
pub const SPLIT_VIEW_DIVIDER_LINE_THICKNESS: f32 = 1.0;
pub const SPLIT_VIEW_MIN_PANE_SIZE: f32 = 96.0;
pub const SPLIT_VIEW_KEYBOARD_STEP: f32 = 24.0;

pub struct SplitView {
    split: Signal<f32>,
    orientation: Orientation,
    min_first_size: Option<f32>,
    min_second_size: Option<f32>,
    divider_thickness: Option<f32>,
    enabled: bool,
    first_pending: Option<PendingChild>,
    second_pending: Option<PendingChild>,
    first_id: Option<WidgetId>,
    handle_id: Option<WidgetId>,
    second_id: Option<WidgetId>,
    /// Shared with the `SplitHandle` child so its pointer and keyboard
    /// handlers can map coordinates back to a split fraction. Written
    /// by this widget's `place_children`, read by the handle's
    /// handler closures at event time. An `Rc<Cell<Rect>>` is the
    /// simplest channel: event handlers run outside the arena and
    /// can't query layout through `LayoutContext`, and the handle
    /// needs the *container* bounds (not its own gutter bounds).
    container_bounds: Rc<Cell<Rect>>,
}

impl SplitView {
    pub fn new(split: Signal<f32>) -> Self {
        Self {
            split,
            orientation: Orientation::Horizontal,
            min_first_size: None,
            min_second_size: None,
            divider_thickness: None,
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
        self.min_first_size = Some(size.max(0.0));
        self
    }

    pub fn min_second_size(mut self, size: f32) -> Self {
        self.min_second_size = Some(size.max(0.0));
        self
    }

    pub fn divider_thickness(mut self, thickness: f32) -> Self {
        self.divider_thickness = Some(thickness.max(1.0));
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

    /// Resolve theme-driven style values, applying per-instance overrides.
    /// `keyboard_step_px` is theme-only (not user-overridable). Panes
    /// and handle all share this resolution path so they stay in sync
    /// when the theme changes.
    fn resolved_style(&self, _theme: &fern_core::Theme) -> ResolvedStyle {
        ResolvedStyle {
            min_first_size: self.min_first_size.unwrap_or(SPLIT_VIEW_MIN_PANE_SIZE),
            min_second_size: self.min_second_size.unwrap_or(SPLIT_VIEW_MIN_PANE_SIZE),
            divider_thickness: self.divider_thickness.unwrap_or(SPLIT_VIEW_GUTTER_THICKNESS),
            keyboard_step_px: SPLIT_VIEW_KEYBOARD_STEP,
        }
    }

    fn clamp_fraction(&self, bounds: Rect, style: &ResolvedStyle) -> f32 {
        SplitBounds::compute(
            bounds,
            self.orientation,
            style.divider_thickness,
            style.min_first_size,
            style.min_second_size,
            style.keyboard_step_px,
        )
        .map(|sb| sb.clamp(self.split.get()))
        .unwrap_or(0.5)
    }
}

#[derive(Debug, Clone, Copy)]
struct ResolvedStyle {
    min_first_size: f32,
    min_second_size: f32,
    divider_thickness: f32,
    keyboard_step_px: f32,
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
        // SplitView style drives layout math (min sizes, divider thickness,
        // keyboard step). These rarely change between themes, so a one-time
        // snapshot is adequate — paint-time lookups in SplitHandle keep
        // colors reactive.
        let style = self.resolved_style(&ctx.theme_signal().get());

        let registry = ctx.binding_registry();
        self.split
            .bind_to(self_id, registry, BindingLevel::Relayout);

        // Wrap each user-supplied pane in a `ClipPane` so its content
        // is clipped to the pane's bounds. Without this, an overflowing
        // descendant (e.g. a `MinSize` larger than the current split
        // fraction allows, or a focus ring) would paint over the
        // gutter and the sibling pane.
        if let Some(pending) = self.first_pending.take() {
            let inner = match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(widget) => ctx.add_boxed(widget),
            };
            self.first_id = Some(ctx.add(ClipPane { child_id: inner }));
        }

        if let Some(pending) = self.second_pending.take() {
            let inner = match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(widget) => ctx.add_boxed(widget),
            };
            self.second_id = Some(ctx.add(ClipPane { child_id: inner }));
        }

        self.handle_id = Some(ctx.add(SplitHandle::new(SplitHandleConfig {
            split: self.split.clone(),
            orientation: self.orientation,
            min_first_size: style.min_first_size,
            min_second_size: style.min_second_size,
            divider_thickness: style.divider_thickness,
            keyboard_step_px: style.keyboard_step_px,
            enabled: self.enabled,
            container_bounds: self.container_bounds.clone(),
        })));

        self.children()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        let style = self.resolved_style(ctx.theme);
        // Query children with an unbounded primary axis to get their intrinsic
        // size — used only as a fallback when the parent doesn't constrain us.
        let child_proposal = match self.orientation {
            Orientation::Horizontal => SizeProposal {
                width: None,
                height: proposal.height,
            },
            Orientation::Vertical => SizeProposal {
                width: proposal.width,
                height: None,
            },
        };
        let first_size = self
            .first_id
            .and_then(|id| ctx.child_size(id, child_proposal))
            .unwrap_or(Size::ZERO);
        let second_size = self
            .second_id
            .and_then(|id| ctx.child_size(id, child_proposal))
            .unwrap_or(Size::ZERO);

        match self.orientation {
            Orientation::Horizontal => {
                let intrinsic_width =
                    first_size.width + style.divider_thickness + second_size.width;
                let min_width =
                    style.min_first_size + style.divider_thickness + style.min_second_size;
                Size::new(
                    proposal.width.unwrap_or(intrinsic_width).max(min_width),
                    proposal
                        .height
                        .unwrap_or_else(|| first_size.height.max(second_size.height)),
                )
            }
            Orientation::Vertical => {
                let intrinsic_height =
                    first_size.height + style.divider_thickness + second_size.height;
                let min_height =
                    style.min_first_size + style.divider_thickness + style.min_second_size;
                Size::new(
                    proposal
                        .width
                        .unwrap_or_else(|| first_size.width.max(second_size.width)),
                    proposal.height.unwrap_or(intrinsic_height).max(min_height),
                )
            }
        }
        .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        self.container_bounds.set(bounds);
        // Children are laid out in the order returned by `children()`:
        //   [0] first pane (ClipPane wrapping user's first widget)
        //   [1] gutter (SplitHandle)
        //   [2] second pane (ClipPane wrapping user's second widget)
        // If any is missing (build hasn't set ids), bail out rather
        // than misplace.
        if children.len() != 3 {
            return;
        }

        let style = self.resolved_style(ctx.theme);
        let split = self.clamp_fraction(bounds, &style);
        let available = match self.orientation {
            Orientation::Horizontal => (bounds.width - style.divider_thickness).max(0.0),
            Orientation::Vertical => (bounds.height - style.divider_thickness).max(0.0),
        };
        let first_main = available * split;
        let second_main = available - first_main;

        match self.orientation {
            Orientation::Horizontal => {
                children[0].origin = Point::new(bounds.x, bounds.y);
                children[0].size = Size::new(first_main, bounds.height);

                children[1].origin = Point::new(bounds.x + first_main, bounds.y);
                children[1].size = Size::new(style.divider_thickness, bounds.height);

                children[2].origin =
                    Point::new(bounds.x + first_main + style.divider_thickness, bounds.y);
                children[2].size = Size::new(second_main, bounds.height);
            }
            Orientation::Vertical => {
                children[0].origin = Point::new(bounds.x, bounds.y);
                children[0].size = Size::new(bounds.width, first_main);

                children[1].origin = Point::new(bounds.x, bounds.y + first_main);
                children[1].size = Size::new(bounds.width, style.divider_thickness);

                children[2].origin =
                    Point::new(bounds.x, bounds.y + first_main + style.divider_thickness);
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

/// Single-child wrapper used internally by `SplitView` to clip each
/// pane's content to its placement. Fills whatever bounds the parent
/// assigns; delegates intrinsic sizing to the wrapped widget.
#[derive(Debug)]
struct ClipPane {
    child_id: WidgetId,
}

impl Widget for ClipPane {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        ctx.child_size(self.child_id, proposal)
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

    fn clips_children(&self) -> bool {
        true
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_hidden();
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child_id]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::event::Modifiers;
    use fern_core::widget_tree::WidgetTree;
    use fern_core::Theme;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);

    impl Widget for FixedLeaf {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> fern_core::widget::LayoutResponse {
            Size::new(self.0, self.1).into()
        }
    }

    #[test]
    fn horizontal_split_places_panes_and_divider() {
        let split = Signal::new(0.25_f32);
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let root = tree.add(
            SplitView::new(split)
                .first(FixedLeaf(100.0, 40.0))
                .second(FixedLeaf(100.0, 40.0)),
        );

        tree.layout(SizeProposal::exact(400.0, 200.0));

        let first = tree.child_widget(root, 0);
        let handle = tree.child_widget(root, 1);
        let second = tree.child_widget(root, 2);

        let default_thickness = SPLIT_VIEW_GUTTER_THICKNESS;
        let available = 400.0 - default_thickness;
        assert!((tree.bounds(first).width - available * 0.25).abs() < 0.01);
        assert!((tree.bounds(handle).width - default_thickness).abs() < 0.01);
        assert!((tree.bounds(second).width - available * 0.75).abs() < 0.01);
    }

    #[test]
    fn drag_updates_split_fraction() {
        let split = Signal::new(0.5_f32);
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
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
    fn vertical_split_places_panes_and_divider() {
        let split = Signal::new(0.25_f32);
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let root = tree.add(
            SplitView::new(split)
                .orientation(Orientation::Vertical)
                .first(FixedLeaf(80.0, 100.0))
                .second(FixedLeaf(80.0, 100.0)),
        );

        tree.layout(SizeProposal::exact(200.0, 400.0));

        let first = tree.child_widget(root, 0);
        let handle = tree.child_widget(root, 1);
        let second = tree.child_widget(root, 2);

        let default_thickness = SPLIT_VIEW_GUTTER_THICKNESS;
        let available = 400.0 - default_thickness;
        assert!((tree.bounds(first).height - available * 0.25).abs() < 0.01);
        assert!((tree.bounds(handle).height - default_thickness).abs() < 0.01);
        assert!((tree.bounds(second).height - available * 0.75).abs() < 0.01);
    }

    #[test]
    fn rtl_vertical_split_still_stacks_top_to_bottom() {
        // Vertical orientation stacks along the Y axis, which is not
        // affected by layout direction — RTL only mirrors horizontal.
        let split = Signal::new(0.5_f32);
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        tree.set_layout_direction(fern_core::environment::LayoutDirection::RightToLeft);
        let root = tree.add(
            SplitView::new(split)
                .orientation(Orientation::Vertical)
                .first(FixedLeaf(80.0, 100.0))
                .second(FixedLeaf(80.0, 100.0)),
        );

        tree.layout(SizeProposal::exact(200.0, 400.0));

        let first = tree.child_widget(root, 0);
        let second = tree.child_widget(root, 2);
        assert!(
            tree.bounds(first).y < tree.bounds(second).y,
            "first pane should remain above second under RTL+vertical"
        );
    }

    #[test]
    fn panes_are_wrapped_in_clipping_container() {
        // Each pane is wrapped in a ClipPane (clips_children = true) so
        // an overflowing descendant can't bleed into the gutter or the
        // sibling pane. Guard the structural invariant: SplitView's
        // first/second children should each have exactly one child —
        // the user's widget — sitting underneath the clip.
        let split = Signal::new(0.5_f32);
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let root = tree.add(
            SplitView::new(split)
                .first(FixedLeaf(500.0, 40.0))
                .second(FixedLeaf(500.0, 40.0)),
        );

        tree.layout(SizeProposal::exact(400.0, 200.0));

        let first_pane = tree.child_widget(root, 0);
        let second_pane = tree.child_widget(root, 2);
        assert_eq!(
            tree.children(first_pane).len(),
            1,
            "first pane should wrap one user widget"
        );
        assert_eq!(
            tree.children(second_pane).len(),
            1,
            "second pane should wrap one user widget"
        );
    }

    #[test]
    fn splitter_exposes_accessibility_role() {
        let split = Signal::new(0.5_f32);
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
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
