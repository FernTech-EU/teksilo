// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The scroll behaviour the three text surfaces share.
//!
//! [`RichTextEditor`](crate::rich_text::RichTextEditor), [`CodeEditor`](crate::CodeEditor)
//! and [`LogView`](crate::LogView) are not built on
//! [`ScrollArea`](crate::ScrollArea) — wrap width depends on bar visibility and
//! bar visibility depends on content height, which is circular inside one — so
//! each drives its own overlay bars over four offset signals of its own. Those
//! four are shaped the same way in all three, and differ from every other
//! scrollable in this crate in two ways that decide this configuration:
//!
//! * **They are plain signals, not animated ones.** No text surface has ever
//!   tweened a wheel notch, so [`smooth`](ScrollableBehavior::smooth) is off —
//!   which is also what keeps `animate_to`, a panic on a plain signal, out of
//!   reach.
//! * **Nothing binds them to the node.** The paint pass reads them directly, so
//!   moving one repaints nothing on its own; the surface has to ask. That is
//!   what the two arms below are for — the offsets are snapshotted before the
//!   delta lands and compared after it, so a frame is requested exactly when
//!   one moved, and a boundary notch that moved nothing still costs none.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use teksilo_core::OverscrollBehavior;
use teksilo_core::event::EventResponse;
use teksilo_core::kinetic::KineticScroller;
use teksilo_core::pointer::touch_action::PanAxes;
use teksilo_core::signal::Signal;
use teksilo_tokens::ScrollPhysicsTokens;

use crate::common::scrollable::{ScrollableAxes, ScrollableBehavior};

/// Pixels one line of a [`ScrollDelta::Lines`](teksilo_core::event::ScrollDelta)
/// notch is worth in a text surface.
///
/// The three editors' own constant, not the one
/// [`ScrollArea`](crate::ScrollArea) uses — a wheel notch has always moved a
/// text surface a little less than a list.
const TEXT_LINE_HEIGHT: f32 = 16.0;

/// The four offsets a text surface scrolls, and where its physics live.
pub(crate) struct TextScrollState {
    /// Horizontal offset, and its maximum.
    pub x: Signal<f32>,
    pub max_x: Signal<f32>,
    /// Vertical offset, and its maximum.
    pub y: Signal<f32>,
    pub max_y: Signal<f32>,
    /// The surface's own scroller, so the viewport it publishes from layout
    /// reaches the object the handler reads.
    pub scroller: Rc<RefCell<KineticScroller>>,
}

/// The behaviour a text surface installs: the wheel path it always had, a
/// finger's pan, and the [`PanClaim`](teksilo_core::pointer::touch_action::PanClaim)
/// that puts the surface on a pan's claimant chain.
pub(crate) fn text_surface_behavior(
    state: TextScrollState,
    overscroll: OverscrollBehavior,
    reduced_motion: bool,
    physics: ScrollPhysicsTokens,
) -> ScrollableBehavior {
    let TextScrollState {
        x,
        max_x,
        y,
        max_y,
        scroller,
    } = state;

    // Snapshotted by the `before` arm, read by the `after` arm. A plain `Cell`
    // is enough: both arms run inside one dispatch of one handler, so nothing
    // can interleave between them.
    let seen = Rc::new(Cell::new((x.get(), y.get())));

    let before = {
        let (x, y, seen) = (x.clone(), y.clone(), seen.clone());
        move |_event: &teksilo_core::event::WidgetEvent,
              _ctx: &mut teksilo_core::widget::EventContext| {
            seen.set((x.get(), y.get()));
            // Declined on purpose: the delta belongs to the shared handler.
            // `None` is "not mine", which is what keeps that handler running
            // on this very event rather than being skipped.
            None
        }
    };
    let after = {
        let (x, y, seen) = (x.clone(), y.clone(), seen.clone());
        move |_event: &teksilo_core::event::WidgetEvent,
              _response: EventResponse,
              ctx: &mut teksilo_core::widget::EventContext| {
            let (px, py) = seen.get();
            if (x.get() - px).abs() > f32::EPSILON || (y.get() - py).abs() > f32::EPSILON {
                ctx.request_frame();
            }
        }
    };

    ScrollableBehavior::new(ScrollableAxes::new(x, y, max_x, max_y))
        .with_scroller(scroller)
        // BOTH, not the `PAN_Y` the plan asked for. All three surfaces keep a
        // real `max_scroll_x`, and two of them need it: `CodeEditor` defaults
        // to `WrapMode::None` because source lines do not wrap, and `LogView`
        // tails unwrapped log lines — a finger that cannot reach the right of
        // a long line cannot read it. `RichTextEditor` wraps, but a table or an
        // image wider than its viewport gives it a horizontal range too.
        // Claiming an axis the surface cannot currently move costs nothing:
        // that axis declines and the chain re-offers the whole event outward,
        // which is the same argument `ScrollArea` makes at its own call site.
        .axes(PanAxes::BOTH)
        .overscroll(overscroll)
        // A text surface has never tweened a notch, and its offsets are plain
        // signals — the tween that would panic on one is unreachable only
        // while this stays off.
        .smooth(false)
        .line_height(TEXT_LINE_HEIGHT)
        .reduced_motion(reduced_motion)
        .physics(physics)
        .before(before)
        .after(after)
}
