// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A `RepaintTrigger` flooded from four threads while the UI thread renders,
//! only when woken, and keeps rebuilding the widget it is attached to: every
//! request is counted, nothing deadlocks, and the final state reaches the
//! screen with no request after the producers' own.
//!
//! It catches a request that never wakes, and a deadlock. An ordering slip
//! (the walker taking the request after `paint()` reads the state) loses a
//! wake only in a window too narrow for a stress test to hit: the loom
//! models pin those.

#![cfg(not(teksilo_loom))]

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use teksilo_canvas::wake::{CountingWaker, RedrawWaker};
use teksilo_canvas::{Canvas, Rect, SizeProposal};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget};
use teksilo_core::widget_tree::WidgetTree;
use teksilo_core::{BuildContext, RepaintTrigger, WidgetId};
use teksilo_tokens::Color;

const PRODUCERS: u64 = 4;
const REQUESTS: u64 = 20_000;

#[derive(Debug)]
struct Counter {
    trigger: RepaintTrigger,
    value: Arc<AtomicU64>,
    painted: Rc<Cell<u64>>,
}

impl Widget for Counter {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        ctx.attach_repaint_trigger(&self.trigger);
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(40.0, 40.0).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        self.painted.set(self.value.load(Ordering::Relaxed));
        canvas.fill_rect(bounds, Color::from_rgb(0.3, 0.3, 0.3));
    }
}

#[test]
fn a_flooded_trigger_counts_every_request_and_paints_the_final_state() {
    let mut tree = WidgetTree::new();
    let waker = Arc::new(CountingWaker::new());
    tree.set_redraw_waker(Some(waker.clone() as Arc<dyn RedrawWaker>));
    let trigger = RepaintTrigger::new();
    let value = Arc::new(AtomicU64::new(0));
    let painted = Rc::new(Cell::new(0));
    let id = tree.add(Counter {
        trigger: trigger.clone(),
        value: value.clone(),
        painted: painted.clone(),
    });
    let frame = |tree: &mut WidgetTree| {
        tree.layout(SizeProposal::exact(200.0, 200.0));
        let _ = tree.render();
    };
    frame(&mut tree);
    // Before any producer runs: every wake from here on is one a frame owes.
    let mut seen = waker.count();

    let producers: Vec<_> = (0..PRODUCERS)
        .map(|_| {
            let (trigger, value) = (trigger.clone(), value.clone());
            std::thread::spawn(move || {
                for i in 0..REQUESTS {
                    value.fetch_add(1, Ordering::Relaxed);
                    if i % 64 == 0 {
                        trigger.request_relayout();
                    } else {
                        trigger.request_repaint();
                    }
                }
            })
        })
        .collect();

    // A frame only for a wake: one the trigger issued since the last frame.
    // Once every producer is done, nothing else requests, so the final state
    // reaches the screen only if the last stores were seen by a frame or
    // woke one.
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut frames = 0_u64;
    loop {
        assert!(Instant::now() < deadline, "never caught up, or deadlocked");
        let done = producers.iter().all(|p| p.is_finished());
        if done && painted.get() == PRODUCERS * REQUESTS {
            break;
        }
        let woke = waker.wait_for(seen + 1, Duration::from_millis(50));
        assert!(
            woke || !done,
            "every producer is done, the final state is not painted ({} of {}), \
             and no wake came: a wake was lost",
            painted.get(),
            PRODUCERS * REQUESTS
        );
        if !woke {
            continue;
        }
        seen = waker.count();
        frames += 1;
        if frames.is_multiple_of(32) {
            tree.arena_mark_needs_rebuild_for_testing(id);
        }
        frame(&mut tree);
    }
    for p in producers {
        p.join().unwrap();
    }

    let stats = trigger.stats();
    assert_eq!(
        stats.requests,
        PRODUCERS * REQUESTS,
        "every request counted"
    );
    assert_eq!(
        stats.wakes + stats.wakes_coalesced,
        stats.requests,
        "with one attachment, a request either woke or merged: {stats:?}"
    );
    assert_eq!(stats.wakes, waker.count(), "each wake reached the window");
    assert_eq!(stats.attachments, 1, "the rebuilds kept one attachment");
    drop(tree);
    assert_eq!(trigger.stats().attachments, 0);
}
