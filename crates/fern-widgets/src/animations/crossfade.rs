//! `Crossfade` — when an external `Signal<K>` changes, the
//! previous content fades out while the new content fades in over
//! the same window. Like [`Switcher`](crate::primitives::Switcher),
//! but animated.
//!
//! ```ignore
//! let tab = Signal::new(Tab::Overview);
//! ctx.add(
//!     Crossfade::new(tab.clone(), |t| match t {
//!         Tab::Overview => Box::new(overview_panel()),
//!         Tab::Details  => Box::new(details_panel()),
//!     }),
//! );
//! ```
//!
//! ## Behavior
//!
//! On each `key` change, both the previous-key widget and the
//! current-key widget are rebuilt (via the supplied builder) and
//! mounted side-by-side in a `ZStack`. The previous fades 1→0 while
//! the current fades 0→1 over the configured duration. On the *next*
//! key change, the previously-outgoing widget is torn down and the
//! cycle repeats.
//!
//! Builders should be cheap — they may run more than once per
//! lifetime as the user navigates through several keys. For data-
//! heavy panels, hoist expensive state out of the builder closure.
//!
//! ## Reduced motion
//!
//! Honours `prefers-reduced-motion`: snaps the opacity changes
//! instead of tweening (instant swap).

use std::time::Duration;

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::binding::BindingLevel;
use fern_core::build_context::BuildContext;
use fern_core::signal::Signal;
use fern_core::widget::{LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;

use crate::primitives::ZStack;

/// Animated swap between widgets keyed by an external signal.
pub struct Crossfade<K: Eq + Clone + 'static> {
    key_signal: Signal<K>,
    builder: Box<dyn Fn(&K) -> Box<dyn Widget>>,
    duration: Option<Duration>,
    last_key: Option<K>,
    root_child_id: Option<WidgetId>,
}

impl<K: Eq + Clone + 'static> Crossfade<K> {
    /// New `Crossfade` driven by `key_signal`. The `builder` closure
    /// constructs the widget for a given key value. Builders can be
    /// invoked multiple times across the widget's lifetime as the
    /// user transitions through keys.
    pub fn new(
        key_signal: Signal<K>,
        builder: impl Fn(&K) -> Box<dyn Widget> + 'static,
    ) -> Self {
        Self {
            key_signal,
            builder: Box::new(builder),
            duration: None,
            last_key: None,
            root_child_id: None,
        }
    }

    /// Override the crossfade duration. Default: `MotionTokens::duration_normal`.
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }
}

impl<K: Eq + Clone + 'static> std::fmt::Debug for Crossfade<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Crossfade")
            .field("duration", &self.duration)
            .finish()
    }
}

impl<K: Eq + Clone + 'static> Widget for Crossfade<K> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let current_key = self.key_signal.get();
        let prev_key = self.last_key.take();
        let key_changed = prev_key.as_ref().is_some_and(|p| p != &current_key);

        let duration = self
            .duration
            .unwrap_or(ctx.theme().motion.duration_normal);
        let easing = ctx.theme().motion.easing_standard;
        let reduced = ctx.prefers_reduced_motion();

        let mut zstack = ZStack::new();

        if key_changed {
            let prev_key = prev_key.unwrap();
            let outgoing = (self.builder)(&prev_key);
            let outgoing_id = ctx.add_boxed(outgoing);
            let opacity = ctx.animated_signal(1.0);
            ctx.set_opacity(outgoing_id, opacity.clone());
            // Bind the outgoing's *visibility* to its own opacity so
            // it goes dormant once the fade reaches ~zero. Without
            // this the outgoing stays mounted in the ZStack at full
            // natural size — the wrapper's reported size becomes
            // `max(outgoing, incoming)` until the next key change,
            // and any layout-driving ancestor (SmoothSize, …) never
            // observes the shrink.
            ctx.visible_when(outgoing_id, opacity.map(|&o| o > 0.005));
            if reduced {
                opacity.set(0.0);
            } else {
                opacity.animate_to(0.0, duration, easing);
            }
            zstack = zstack.add_child(outgoing_id);
        }

        let incoming = (self.builder)(&current_key);
        let incoming_id = ctx.add_boxed(incoming);
        let initial = if key_changed { 0.0 } else { 1.0 };
        let opacity = ctx.animated_signal(initial);
        ctx.set_opacity(incoming_id, opacity.clone());
        if key_changed {
            if reduced {
                opacity.set(1.0);
            } else {
                opacity.animate_to(1.0, duration, easing);
            }
        }
        zstack = zstack.add_child(incoming_id);

        // Trigger a full rebuild on key change so the next transition
        // can mount fresh outgoing+incoming pair.
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.key_signal
            .bind_to(self_id, registry, BindingLevel::Rebuild);

        self.last_key = Some(current_key);
        let root = ctx.add(zstack);
        self.root_child_id = Some(root);
        vec![root]
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

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {
        // Animation wrapper. The active subtree owns its own a11y.
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::TextWidget;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;

    fn count_set_opacity(frame: &fern_canvas::RenderFrame) -> Vec<f32> {
        frame
            .draw_order
            .iter()
            .filter_map(|c| match c {
                fern_canvas::DrawCommand::SetOpacity(v) => Some(*v),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn first_build_shows_initial_key_at_full_opacity() {
        let key = Signal::new(0_u32);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(Crossfade::new(key, |k| {
            Box::new(TextWidget::new_literal(format!("page {k}")))
        }));
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: None,
        });
        let frame = tree.render();
        let ops = count_set_opacity(&frame);
        // Single visible child at opacity 1.0 — exactly one
        // SetOpacity scope around it.
        assert_eq!(ops.len(), 1);
        assert!((ops[0] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn key_change_starts_overlap_with_two_opacity_scopes() {
        let key = Signal::new(0_u32);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(Crossfade::new(key.clone(), |k| {
            Box::new(TextWidget::new_literal(format!("page {k}")))
        }));
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: None,
        });

        key.set(1);
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: None,
        });
        // Mid-tween: tick a bit so animations have started but not
        // completed. Two SetOpacity scopes (outgoing + incoming).
        tree.tick_animations(Duration::from_millis(50));
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: None,
        });
        let frame = tree.render();
        let ops = count_set_opacity(&frame);
        assert_eq!(
            ops.len(),
            2,
            "during transition, outgoing and incoming should both have opacity scopes"
        );
        // One opacity should be > 0.5 (incoming approaching 1) or
        // < 0.5 (outgoing approaching 0). Just sanity-check both are
        // strictly between 0 and 1.
        for o in &ops {
            assert!(
                *o >= 0.0 && *o <= 1.0,
                "opacity must be in [0, 1], got {o}"
            );
        }
    }

    #[test]
    fn outgoing_goes_dormant_after_fade_so_layout_can_shrink() {
        // Regression: when transitioning from a tall content key to a
        // short content key, Crossfade used to keep the tall outgoing
        // mounted at opacity=0, so the wrapper's reported size stayed
        // at max(tall, short) = tall forever. A SmoothSize ancestor
        // would never observe the shrink. Bind the outgoing's
        // visibility to its opacity so it goes dormant once faded.
        use crate::primitives::FixedSize;

        #[derive(Debug)]
        struct Sized(f32);
        impl Widget for Sized {
            fn size_that_fits(&self, _p: SizeProposal, _c: &LayoutContext) -> Size {
                Size::new(40.0, self.0)
            }
        }

        let key = Signal::new(0_u32);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        // Wrap so we can read the wrapper's bounds (which equal the
        // Crossfade's reported size).
        let id = tree.add(FixedSize::new().child(Crossfade::new(
            key.clone(),
            |&k| -> Box<dyn Widget> {
                let h = if k == 0 { 100.0 } else { 30.0 };
                Box::new(Sized(h))
            },
        )));
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        let initial = tree.bounds(id);
        assert!((initial.height - 100.0).abs() < 0.5);

        // Transition tall → short. After the fade duration plus a
        // layout pass to drain pending animations, the outgoing
        // (tall) widget must be dormant so the wrapper shrinks.
        key.set(1);
        // Two layouts to drain the queued animate_to onto the
        // scheduler, then tick well past the fade duration.
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        tree.tick_animations(Duration::from_millis(400));
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
        let after = tree.bounds(id);
        assert!(
            (after.height - 30.0).abs() < 1.0,
            "after fade-out, wrapper should shrink to incoming's natural height; got {}",
            after.height
        );
    }

    #[test]
    fn reduced_motion_snaps_instantly() {
        let key = Signal::new(0_u32);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.set_accessibility_preferences(false, true, 1.0);
        tree.add(Crossfade::new(key.clone(), |k| {
            Box::new(TextWidget::new_literal(format!("page {k}")))
        }));
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: None,
        });

        key.set(1);
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: None,
        });
        assert!(
            !tree.has_active_animations(),
            "reduced-motion path must not register animations"
        );
    }
}
