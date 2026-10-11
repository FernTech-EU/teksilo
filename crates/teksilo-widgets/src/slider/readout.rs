// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The live value readout behind [`Slider::value_tooltip`](super::Slider::value_tooltip).
//!
//! A hover tooltip is the wrong machine for it on every count. A tooltip waits
//! for a dwell, opens below its anchor, closes on the first press and never
//! moves; a readout has to be up the moment a pointer arrives, a drag starts or
//! a key lands, stay up for the whole drag, and ride the thumb. So the readout
//! is not a tooltip entry but an overlay the slider raises itself, from the
//! same handlers that move the value. Its surface is still a
//! [`TooltipWidget`], so a theme's tooltip chrome reaches it.
//!
//! Showing needs an `EventContext` and happens in those handlers. Hiding does
//! not need one: the surface's `visible_when` gate is the conjunction of
//! everything that keeps the readout wanted, so a reason to go that arrives
//! while no slider handler runs (the slider disabled under the pointer, parked
//! in a hidden `Switcher` branch, keyboard modality ended by a press elsewhere)
//! parks the surface, and the tree retires an overlay whose content is dormant.
//! Placing does not need one either: the overlay re-derives its placement from
//! the slider's bounds on every layout pass, so a scroll, a resize or a reflow
//! that moves the slider moves the readout with it.
//!
//! The surface takes no input at all, so the overlay is shown
//! [inert](OverlayRequest::inert): a press or a hover over the bubble belongs
//! to whatever is under it, a press there is outside every overlay it is
//! outside of, Escape retires it without being spent on it, and it neither
//! records a focus to give back nor closes when focus leaves the slider. The
//! slider alone decides when it goes.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect};
use teksilo_core::build_context::BuildContext;
use teksilo_core::environment::LayoutDirection;
use teksilo_core::overlay::{
    DismissBehavior, OverlayDismissCallback, OverlayPlacement, OverlayRequest,
};
use teksilo_core::signal::Signal;
use teksilo_core::styles::{SharedTooltipStyle, TooltipStyle, TooltipStyleConfig};
use teksilo_core::widget::EventContext;
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::LocalizedString;
use teksilo_tokens::Orientation;

use crate::tooltip::TooltipWidget;

use super::SliderValueTooltipPlacement;

/// The formatter [`Slider::value_tooltip`](super::Slider::value_tooltip) takes.
pub(super) type ValueFormat = Rc<dyn Fn(f32) -> LocalizedString>;

/// What the readout reads to decide whether it is wanted, and where it goes.
pub(super) struct ReadoutInputs {
    pub(super) value: Signal<f32>,
    pub(super) min: f32,
    pub(super) max: f32,
    pub(super) orientation: Orientation,
    pub(super) thumb_diameter: f32,
    /// The slider's bounds, as its `place_children` last saw them. Only the
    /// placement the overlay is first shown with reads them; every layout pass
    /// after that hands the overlay the bounds it has just given the slider.
    pub(super) bounds: Rc<Cell<Rect>>,
    pub(super) hovered: Signal<bool>,
    pub(super) dragging: Signal<bool>,
    pub(super) focused: Signal<bool>,
    /// Which side of the thumb, or the pointer, the readout goes by.
    pub(super) placement: SliderValueTooltipPlacement,
    /// Where a drag's pointer is, widget-local; `None` outside a drag. Written
    /// only for [`SliderValueTooltipPlacement::FollowPointer`].
    pub(super) drag_at: Signal<Option<Point>>,
    /// The chrome [`Slider::value_tooltip_style`](super::Slider::value_tooltip_style)
    /// gives the surface, else the theme's.
    pub(super) style: Option<SharedTooltipStyle>,
}

/// A shared [`TooltipStyle`] handed on to [`TooltipWidget::style`], which
/// takes one by value: the slider keeps its style across rebuilds and hands
/// each generation's surface the same one.
struct SharedStyle(SharedTooltipStyle);

impl TooltipStyle for SharedStyle {
    fn make_body(&self, cfg: &TooltipStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        self.0.make_body(cfg, ctx)
    }
}

/// One slider's readout: its parked surface and the state of the overlay
/// showing it.
pub(super) struct Readout {
    inputs: ReadoutInputs,
    /// The surface: a [`TooltipWidget`] detached from the slider's own subtree,
    /// parked until an overlay shows it.
    content: WidgetId,
    /// The slider, which the overlay records as its anchor.
    anchor: WidgetId,
    /// Whether this readout's overlay is on the stack. Built per generation, so
    /// a rebuild's fresh surface starts hidden whatever the last one was doing.
    shown: Signal<bool>,
    /// Hovered, dragged, or focused from the keyboard, and enabled and active:
    /// everything that keeps the readout up.
    live: Signal<bool>,
}

impl Readout {
    /// Build the parked surface for the slider being built and gate it.
    pub(super) fn mount(
        ctx: &mut BuildContext,
        inputs: ReadoutInputs,
        format: ValueFormat,
        enabled: Signal<bool>,
    ) -> Rc<Self> {
        let anchor = ctx.self_id();
        // Re-resolved whenever the value moves, and on a locale change once an
        // `I18nManager` is installed. `to_signal` rather than the
        // `LocalizedString` itself: converting that to a `Prop` takes a static
        // snapshot when no manager is installed, which would drop the value
        // dependency along with the locale one.
        let text = {
            let value = inputs.value.clone();
            teksilo_i18n::localized(move || format(value.get()).resolve_now())
                .also_observing(&inputs.value)
                .to_signal()
        };
        let mut surface = TooltipWidget::bound(text);
        if let Some(style) = inputs.style.clone() {
            surface = surface.style(SharedStyle(style));
        }
        let content = ctx.add_detached(
            surface
                // The slider's own node carries the same text as its value, so
                // a reader reaching this one too would hear it twice.
                .access_hidden(true),
        );
        ctx.set_dormant(content);

        // `:focus-visible`, the same reading the focus ring makes: focus that a
        // key or a Tab put there, not a click.
        let keyboard_focus = inputs.focused.and(&ctx.focus_visible());
        let wanted = inputs.hovered.or(&inputs.dragging).or(&keyboard_focus);
        let live = wanted.and(&enabled).and(&ctx.activation_signal(anchor));
        let shown = Signal::new(false);
        ctx.visible_when(content, shown.and(&live));

        Rc::new(Self {
            inputs,
            content,
            anchor,
            shown,
            live,
        })
    }

    /// Bring the overlay in line with what the slider is doing: raise it or
    /// take it down.
    ///
    /// Every handler that changes a reason to show the readout ends with this
    /// call. Where the overlay goes is not decided here: once it is up it
    /// follows the thumb on its own, see [`Self::follow`].
    pub(super) fn sync(&self, ctx: &mut EventContext) {
        if !self.live.get() {
            if self.shown.get() {
                self.shown.set(false);
                ctx.dismiss_overlay_by_content(self.content);
            }
            return;
        }
        if self.shown.get() {
            return;
        }
        self.shown.set(true);
        // Before the request: the tree applies the activation ahead of the
        // overlay requests, and an overlay whose content is still dormant at
        // the next layout is retired as orphaned.
        ctx.activate(self.content);
        let direction = if ctx.is_rtl() {
            LayoutDirection::RightToLeft
        } else {
            LayoutDirection::LeftToRight
        };
        // No fade: the readout is due the moment a drag starts, and a
        // fade-out keeps the content on the stack, where the next show
        // would find it and decline to raise it again.
        ctx.show_overlay(
            OverlayRequest::new(
                self.content,
                self.anchor,
                self.follow()(self.inputs.bounds.get(), direction),
                // Escape, because content shown on hover or focus has to be
                // dismissible without moving either (WCAG 2.2 SC 1.4.13), and
                // nothing else: a press must not close it, since a press is how a
                // drag starts. The overlay being inert, Escape takes it down without
                // being spent on it, the way it takes down a plain tooltip.
                DismissBehavior::EscapeKey,
            )
            .on_dismiss(self.on_dismiss())
            .inert(),
        );
        ctx.track_overlay_placement_by_content(self.content, self.follow());
    }

    /// Clears `shown` whichever way the overlay went: Escape, a parent overlay
    /// closing, the gate parking the surface.
    fn on_dismiss(&self) -> OverlayDismissCallback {
        let shown = self.shown.clone();
        Rc::new(move |_, _| {
            if shown.get() {
                shown.set(false);
            }
        })
    }

    /// Where the readout goes for a slider at a given rectangle: clear of the
    /// thumb and of the track, by default above a horizontal slider's thumb
    /// and beside a vertical one's, else on the side
    /// [`SliderValueTooltipPlacement`] asks for.
    ///
    /// Re-run by the tree on every layout pass with the bounds that pass gave
    /// the slider, and with the value as it stands, so the readout rides the
    /// thumb through a drag, a key, a value the application writes, a scroll
    /// and a resize alike.
    ///
    /// Both rectangles run across the whole cross axis of the slider at the
    /// thumb, so the readout clears whatever the style draws beside the track
    /// (tick marks, a taller body) and not only the knob. Above, for the
    /// horizontal, because a thumb moving sideways would run into a readout
    /// beside it; `AboveSelection` centres on the thumb and goes below when
    /// the slider is against the top of the window. Beside, for the vertical,
    /// for the same reason turned round: `AtPointerAvoiding` puts the readout
    /// on the slider's inline-start side, the far side from a hand coming in
    /// from the reader's own, or on the other side when that one has no room,
    /// just below the strip the thumb occupies (above it near the bottom of
    /// the window), and never over that strip. `Above` and `Below` centre on
    /// that strip in either orientation, flipping when the window has no room.
    ///
    /// `FollowPointer` places as `Auto` does, about a thumb-sized rectangle
    /// at the drag's pointer rather than at the thumb, for as long as a drag
    /// is on.
    fn follow(&self) -> impl Fn(Rect, LayoutDirection) -> OverlayPlacement + 'static {
        let ReadoutInputs {
            ref value,
            min,
            max,
            orientation,
            thumb_diameter,
            placement,
            ref drag_at,
            ..
        } = self.inputs;
        let value = value.clone();
        let drag_at = drag_at.clone();
        move |bounds, direction| {
            let thumb = super::thumb_rect(
                bounds,
                super::fraction(value.get(), min, max),
                thumb_diameter,
                orientation,
                matches!(direction, LayoutDirection::RightToLeft),
            );
            // What the readout hangs off: the thumb, or a thumb-sized
            // rectangle at the dragging pointer along the slider's axis.
            let at = match (placement, drag_at.get()) {
                (SliderValueTooltipPlacement::FollowPointer, Some(p)) => match orientation {
                    Orientation::Horizontal => Rect::new(
                        bounds.x + p.x - thumb.width / 2.0,
                        thumb.y,
                        thumb.width,
                        thumb.height,
                    ),
                    Orientation::Vertical => Rect::new(
                        thumb.x,
                        bounds.y + p.y - thumb.height / 2.0,
                        thumb.width,
                        thumb.height,
                    ),
                },
                _ => thumb,
            };
            // The strip `at` occupies across the whole slider.
            let strip = match orientation {
                Orientation::Horizontal => Rect::new(at.x, bounds.y, at.width, bounds.height),
                Orientation::Vertical => Rect::new(bounds.x, at.y, bounds.width, at.height),
            };
            match (placement, orientation) {
                (SliderValueTooltipPlacement::Above, _) => {
                    OverlayPlacement::AboveSelection { selection: strip }
                }
                (SliderValueTooltipPlacement::Below, _) => {
                    OverlayPlacement::BelowSelection { selection: strip }
                }
                (_, Orientation::Horizontal) => {
                    OverlayPlacement::AboveSelection { selection: strip }
                }
                (_, Orientation::Vertical) => OverlayPlacement::AtPointerAvoiding {
                    point: at.center(),
                    avoid: strip,
                },
            }
        }
    }
}
