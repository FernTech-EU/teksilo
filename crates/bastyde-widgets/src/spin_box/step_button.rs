//! `StepButton` — private helper widget for `SpinBox`.
//!
//! A small non-focusable button stacked inside a `SpinBox`'s
//! trailing column. Features:
//!
//! - Fires `on_tap(&mut EventContext)` on the initial press (so
//!   downstream `on_value_changed` callbacks can send intents).
//! - Optional hold-to-repeat: when
//!   [`on_auto_repeat`](StepButton::on_auto_repeat) is set, the
//!   button fires that closure periodically while the pointer is
//!   held down, after an initial delay. Matches Qt's `accelerated`
//!   / GTK's `climb-rate` behaviour. No `EventContext` is passed
//!   to the repeat closure — frame-tick effects have no way to
//!   synthesise one, so value updates must go through signal
//!   mutation only.
//! - Visual `Pressed` state flashes while the pointer is down.
//! - Not focusable: focus stays on the `TextInputField`, so arrow
//!   keys and typing keep working while the user holds the
//!   pointer on a step button.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, PointerButton, WidgetEvent};
use bastyde_core::signal::Signal;
use bastyde_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{CornerRadius, SurfaceRole, TextRole};

use crate::button::InteractionState;
use crate::primitives::icon_widget::IconWidget;
use crate::primitives::{Center, RectWidget, ZStack};

/// Delay from pointer-down to the first auto-repeat fire. Matches
/// Qt / GTK hold-to-repeat defaults.
const INITIAL_DELAY_SECS: f32 = 0.400;
/// Interval between auto-repeat fires at full speed (after
/// acceleration ramps down from the initial interval).
const MIN_INTERVAL_SECS: f32 = 0.045;
/// Initial repeat interval right after the initial delay expires.
const START_INTERVAL_SECS: f32 = 0.120;
/// Multiplier applied to the repeat interval after each fire
/// (acceleration). `< 1.0` shortens the gap; clamped to
/// `MIN_INTERVAL_SECS`.
const ACCELERATION: f32 = 0.88;

/// Internal hold-and-repeat state machine. Owned by the button so
/// it survives across frame-tick effect fires.
#[derive(Debug, Clone, Copy)]
enum RepeatState {
    Idle,
    /// Pointer is down. `next_fire` is the wall-clock instant the
    /// next auto-repeat should run; `interval` is the current
    /// cadence (decreases with acceleration).
    Held {
        next_fire: Instant,
        interval: f32,
    },
}

pub(super) struct StepButton {
    icon: IconWidget,
    on_tap: Rc<dyn Fn(&mut EventContext)>,
    on_auto_repeat: Option<Rc<dyn Fn()>>,
    enabled_signal: Signal<bool>,
    width: f32,
    height: f32,
    corner_radius: CornerRadius,
    interaction: Signal<InteractionState>,
    root_child_id: Option<WidgetId>,
}

impl std::fmt::Debug for StepButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StepButton")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("auto_repeat", &self.on_auto_repeat.is_some())
            .finish_non_exhaustive()
    }
}

impl StepButton {
    pub(super) fn new(
        icon: IconWidget,
        enabled_signal: Signal<bool>,
        on_tap: impl Fn(&mut EventContext) + 'static,
    ) -> Self {
        Self {
            icon,
            on_tap: Rc::new(on_tap),
            on_auto_repeat: None,
            enabled_signal,
            width: 18.0,
            height: 12.0,
            corner_radius: CornerRadius::uniform(0.0),
            interaction: Signal::new(InteractionState::Idle),
            root_child_id: None,
        }
    }

    pub(super) fn size(mut self, width: f32, height: f32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    pub(super) fn corner_radius(mut self, radius: CornerRadius) -> Self {
        self.corner_radius = radius;
        self
    }

    /// Install a signal-only repeat callback. When set, holding the
    /// pointer down on the button fires this closure every
    /// ~45–120 ms after an initial 400 ms delay (acceleration
    /// ramps the interval down over time). The closure must do its
    /// work through signal mutations only — it has no
    /// `EventContext`.
    pub(super) fn on_auto_repeat(mut self, f: impl Fn() + 'static) -> Self {
        self.on_auto_repeat = Some(Rc::new(f));
        self
    }
}

impl Widget for StepButton {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let interaction = self.interaction.clone();

        // Register the enabled signal so the button re-renders (and
        // its interaction state re-resolves) when the caller toggles
        // it in response to value → bound transitions.
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.enabled_signal.bind_to(
            self_id,
            registry,
            bastyde_core::binding::BindingLevel::RepaintOnly,
        );

        let enabled_signal_bg = self.enabled_signal.clone();
        let bg_role =
            interaction.map(move |state| resolve_bg_role(*state, enabled_signal_bg.get()));
        let enabled_signal_icon = self.enabled_signal.clone();
        let icon_role =
            interaction.map(move |state| resolve_icon_role(*state, enabled_signal_icon.get()));

        let bg = RectWidget::new()
            .bind_background(bg_role)
            .corner_radius(self.corner_radius);
        let bg_id = ctx.add(bg);

        // Swap icon color in via bind_color. The icon's design size
        // is preserved; only the tint follows the signal.
        let sized_icon = std::mem::replace(
            &mut self.icon,
            IconWidget::from_path(bastyde_canvas::Path::new(), 0.0),
        )
        .bind_color(icon_role);
        let icon_id = ctx.add(Center::new().child(sized_icon));

        let zstack_id = ctx.add(ZStack::new().add_child(bg_id).add_child(icon_id));

        let root_id = ctx.add(
            crate::primitives::FixedSize::new()
                .bind_width(self.width)
                .bind_height(self.height)
                .child_id(zstack_id),
        );

        // Hold-to-repeat plumbing: one cell of state shared between
        // the pointer-event handler (which flips it on/off) and the
        // frame-tick effect (which polls for the next fire). `hovered`
        // is a separate cell because on_hover is the authoritative
        // source for pointer-is-over-me, used to restore the
        // correct resting state on pointer-up.
        let repeat_state = Rc::new(Cell::new(RepeatState::Idle));
        let hovered = Rc::new(Cell::new(false));
        let frame_req = ctx.frame_request_handle();
        let wake_at = ctx.wake_at_handle();

        // Frame-tick effect drives the hold-to-repeat timer when
        // `on_auto_repeat` is set. We read-cell → maybe-fire →
        // write-cell so the pointer handler can observe consistent
        // state. A wake-at deadline is scheduled for the next fire
        // so the event loop doesn't burn CPU polling while the
        // button sits pressed.
        if let Some(auto_fn) = self.on_auto_repeat.clone() {
            let state_for_tick = repeat_state.clone();
            let frame_for_tick = frame_req.clone();
            let wake_for_tick = wake_at.clone();
            let tick = ctx.frame_tick();
            ctx.effect(&tick, move |_delta| {
                if let RepeatState::Held {
                    next_fire,
                    interval,
                } = state_for_tick.get()
                {
                    let now = Instant::now();
                    if now >= next_fire {
                        auto_fn();
                        let new_interval = (interval * ACCELERATION).max(MIN_INTERVAL_SECS);
                        let new_next = now + Duration::from_secs_f32(new_interval);
                        state_for_tick.set(RepeatState::Held {
                            next_fire: new_next,
                            interval: new_interval,
                        });
                        // Schedule a wake-up for the next tick and
                        // ask for a frame so the event loop pumps
                        // us back through here.
                        wake_for_tick.set(Some(new_next));
                        frame_for_tick.set(true);
                    } else {
                        // Not time yet — make sure we're scheduled
                        // to wake when it is.
                        let current = wake_for_tick.get();
                        let merged = match current {
                            Some(existing) if existing <= next_fire => existing,
                            _ => next_fire,
                        };
                        wake_for_tick.set(Some(merged));
                    }
                }
            });
        }

        // Handlers. Not focusable: focus stays on the text field.
        let on_tap_for_press = self.on_tap.clone();
        let on_auto = self.on_auto_repeat.clone();
        let int_pointer = interaction.clone();
        let int_enter = interaction.clone();
        let int_leave = interaction.clone();
        let enabled_pointer = self.enabled_signal.clone();
        let enabled_hover = self.enabled_signal.clone();
        let repeat_for_pointer = repeat_state.clone();
        let hovered_for_pointer = hovered.clone();
        let hovered_for_hover = hovered.clone();
        let frame_for_pointer = frame_req.clone();
        let wake_for_pointer = wake_at.clone();

        let handlers = HandlerSet::new()
            .focusable(false)
            .cursor(CursorIcon::Pointer)
            .on_pointer_event(move |event, ctx| {
                match event {
                    WidgetEvent::PointerDown { button, .. }
                        if *button == PointerButton::Primary =>
                    {
                        if !enabled_pointer.get() {
                            return EventResponse::Ignored;
                        }
                        // Fire once on press (Qt convention).
                        (on_tap_for_press)(ctx);
                        int_pointer.set(InteractionState::Pressed);
                        // Arm hold-to-repeat if configured.
                        if on_auto.is_some() {
                            let first_fire =
                                Instant::now() + Duration::from_secs_f32(INITIAL_DELAY_SECS);
                            repeat_for_pointer.set(RepeatState::Held {
                                next_fire: first_fire,
                                interval: START_INTERVAL_SECS,
                            });
                            let current = wake_for_pointer.get();
                            let merged = match current {
                                Some(existing) if existing <= first_fire => existing,
                                _ => first_fire,
                            };
                            wake_for_pointer.set(Some(merged));
                            frame_for_pointer.set(true);
                        }
                        EventResponse::Handled
                    }
                    WidgetEvent::PointerUp { button, .. } if *button == PointerButton::Primary => {
                        repeat_for_pointer.set(RepeatState::Idle);
                        // Resting state depends on whether the
                        // pointer is still over the button.
                        int_pointer.set(if hovered_for_pointer.get() {
                            InteractionState::Hovered
                        } else {
                            InteractionState::Idle
                        });
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }
            })
            .on_hover(move |entered, _ctx| {
                hovered_for_hover.set(entered);
                if !enabled_hover.get() {
                    return;
                }
                // Don't clobber Pressed on pointer-leave — wait
                // for PointerUp.
                let current = int_enter.get();
                if matches!(current, InteractionState::Pressed) {
                    return;
                }
                if entered {
                    int_enter.set(InteractionState::Hovered);
                } else {
                    int_leave.set(InteractionState::Idle);
                }
            });

        ctx.apply_self_handlers(handlers);

        self.root_child_id = Some(root_id);
        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(self.width, self.height))
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
        self.root_child_id.into_iter().collect()
    }

    fn accessibility(&self, builder: &mut bastyde_core::accessibility::AccessNodeBuilder) {
        // Hidden from a11y: the SpinBox root is Role::SpinButton and
        // owns the Increment/Decrement actions. Exposing the buttons
        // separately would make screen readers announce a redundant
        // Button under the SpinButton, which is the wrong mental
        // model (the step buttons are affordances of the SpinBox
        // itself, not standalone controls).
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
    }
}

fn resolve_bg_role(state: InteractionState, enabled: bool) -> SurfaceRole {
    if !enabled {
        return SurfaceRole::Transparent;
    }
    match state {
        InteractionState::Pressed => SurfaceRole::Pressed,
        InteractionState::Hovered => SurfaceRole::Hover,
        _ => SurfaceRole::Transparent,
    }
}

fn resolve_icon_role(_state: InteractionState, enabled: bool) -> TextRole {
    if enabled {
        TextRole::Primary
    } else {
        TextRole::Disabled
    }
}
