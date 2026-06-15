// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `ToastHost` — invisible sibling widget that owns the toast queue.
//!
//! Installed by `install_toast(opts)` in the `bastyde` umbrella. The
//! umbrella's `BastydeAppBuilderToastExt::install_toast` registers a
//! `DefaultPostRoot` closure that wraps
//! every window's root with a `ZStack` of `[user_root, ToastHost]`.
//! The host renders its toast surfaces as direct children, positioned
//! absolutely at the configured viewport corner. The wrapping ZStack
//! ensures toasts paint above the user content; the host itself fills
//! the viewport (so its children — the toasts — have absolute screen
//! coordinates to anchor against) and is `event_pass_through` outside
//! the toast bounds so the user can still interact with content below.
//!
//! No overlay system involvement — toasts are regular widgets in the
//! arena. The host owns the per-frame timer + hover-pause; expired
//! entries are removed from the registry's queue, the version signal
//! is bumped, the host rebuilds, the surface widgets are destroyed.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use bastyde_canvas::{Rect, SizeProposal, Vec2};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::widget::{LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::Corner;

use crate::notification::NotificationArchive;
use crate::toast::registry::ToastRegistry;
use crate::toast::surface::{ToastSurface, ToastSurfaceData};

/// Configuration for the installed [`ToastHost`]. Passed to
/// `install_toast` in the `bastyde` umbrella crate.
#[derive(Clone, Debug)]
pub struct ToastInstallOptions {
    /// Which viewport corner the toasts anchor to. Default
    /// `BottomTrailing` (matches JetBrains IntelliJ and Windows
    /// system tray conventions). Under RTL, Trailing flips to the
    /// physical left edge.
    pub corner: Corner,
    /// Outer margin from the corner — (x, y) in logical pixels.
    /// Default `(24, 24)`.
    pub margin: Vec2,
    /// Vertical gap between stacked toasts. Default `8.0`.
    pub gap: f32,
    /// Maximum simultaneously visible toasts. Default `5`. Normal
    /// priority overflow drops; High / Urgent evict the oldest Normal.
    pub max_visible: usize,
    /// Fixed width for each toast surface. Default `380.0` (matches
    /// IntelliJ balloon width).
    pub entry_width: f32,
    /// When true (default), hovering any toast pauses every timer.
    /// When false, only the hovered toast pauses (libadwaita
    /// behaviour).
    pub pause_on_hover_group: bool,
    /// Notification archive — `None` disables archival entirely
    /// (apps don't need a NotificationLog). Default:
    /// `Some(NotificationArchive::persistent(ARCHIVE_FILE_NAME))`,
    /// which writes to `<config>/notifications.toml` via
    /// `PersistedListModel`. Apps that don't have `AppPaths`
    /// configured (`SettingsBundle` not installed) should explicitly
    /// override to `Some(NotificationArchive::in_memory())` or
    /// `None` — `Persistent` will fail at install time without a
    /// `config_dir`.
    pub archive: Option<NotificationArchive>,
}

impl Default for ToastInstallOptions {
    fn default() -> Self {
        Self {
            corner: Corner::BottomTrailing,
            margin: Vec2::new(24.0, 24.0),
            gap: 8.0,
            max_visible: 5,
            entry_width: 380.0,
            pause_on_hover_group: true,
            archive: Some(NotificationArchive::persistent(
                crate::notification::ARCHIVE_FILE_NAME,
            )),
        }
    }
}

/// Invisible sibling widget that owns the toast queue. Installed once
/// per window by the `install_toast` extension trait via a
/// `DefaultPostRoot` closure (see `bastyde::toast_install`).
///
/// Renders its toast surfaces as direct children positioned at the
/// configured corner. Use `ZStack::new().child(user_root).child(host)`
/// to put the host above the user content.
pub struct ToastHost {
    registry: ToastRegistry,
    options: ToastInstallOptions,
    /// Toast surface ids matched 1:1 with the registry's live entry
    /// ids at the time of the last `build()`. Used by `place_children`
    /// to know the placement order.
    toast_surface_ids: Vec<WidgetId>,
    /// `Instant` of the last timer tick — used to compute `dt`. The
    /// auto-dismiss timer is driven by a `wake_at` deadline (see
    /// `build`), not a per-frame subscription, so a visible toast does
    /// not pin the event loop at 60 fps.
    last_tick_at: Rc<RefCell<Option<Instant>>>,
    /// Set true once any pointer-event handler has been attached so
    /// subsequent rebuilds don't re-attach. The handler drives the
    /// pending dismiss-callback drain.
    has_pending_drain_handler: Cell<bool>,
}

impl ToastHost {
    /// Construct a host bound to the given registry. Add to the tree
    /// alongside the user root inside a `ZStack`.
    pub fn new(registry: ToastRegistry, options: ToastInstallOptions) -> Self {
        Self {
            registry,
            options,
            toast_surface_ids: Vec::new(),
            last_tick_at: Rc::new(RefCell::new(None)),
            has_pending_drain_handler: Cell::new(false),
        }
    }

    /// Backwards-compatibility alias for ergonomic post-root
    /// installation: an app that already has a wrapping ZStack can
    /// construct a host via the standalone `new(...)`. This helper
    /// returns a fresh wrapper that uses `ZStack` internally — but
    /// since the wrapping is owned by `install_toast` itself, this is
    /// rarely called by user code.
    pub fn wrapping(
        _user_root: WidgetId,
        registry: ToastRegistry,
        options: ToastInstallOptions,
    ) -> Self {
        // Legacy shape kept for the existing tests + initial install
        // call site; the actual ZStack wrapping is performed in the
        // install closure (which calls `new` on the host alongside
        // the user-root id). The argument is documented but ignored.
        Self::new(registry, options)
    }
}

impl std::fmt::Debug for ToastHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToastHost")
            .field("toast_count", &self.toast_surface_ids.len())
            .field("options", &self.options)
            .finish()
    }
}

/// While a hovered toast freezes its countdown we can't know in advance
/// when the pointer will leave, so we poll on this coarse interval just
/// to notice the un-hover. Hovering is a brief, deliberate user action,
/// so ~8 Hz here is negligible (and only while actually hovering).
const HOVER_POLL_INTERVAL: Duration = Duration::from_millis(120);
/// Floor on the scheduled wake delay so an almost-expired toast can't
/// schedule a zero/near-zero deadline and busy-loop for one frame.
const MIN_WAKE_DELAY: Duration = Duration::from_millis(8);

/// (Re)arm the auto-dismiss deadline. When a toast is hovered (and
/// hover-pause is on) the countdown is frozen, so we schedule a short
/// poll to detect the un-hover; otherwise we sleep right up to the
/// soonest expiry. Merges with any existing earlier deadline so we never
/// push another widget's pending `wake_at` out.
fn schedule_toast_wake(
    registry: &ToastRegistry,
    wake_at: &Rc<Cell<Option<Instant>>>,
    pause_on_hover_group: bool,
    now: Instant,
) {
    let paused = pause_on_hover_group && registry.hover_count_signal().get() > 0;
    let delay = if paused {
        HOVER_POLL_INTERVAL
    } else {
        match registry.min_running_timer() {
            Some(remaining) => remaining.max(MIN_WAKE_DELAY),
            None => return, // nothing left to wait for
        }
    };
    let target = now + delay;
    let merged = match wake_at.get() {
        Some(existing) if existing <= target => existing,
        _ => target,
    };
    wake_at.set(Some(merged));
}

impl Widget for ToastHost {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Rebuild on any queue mutation (show, dismiss, timer expiry).
        self.registry.version_signal().bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Rebuild,
        );

        // Build one ToastSurface per live entry. Each rebuild creates
        // fresh ToastSurface widget instances — old surfaces are torn
        // down by the framework (no preserve_children).
        let entry_ids = self.registry.live_entry_ids();
        let mut surface_ids = Vec::with_capacity(entry_ids.len());
        for entry_id in &entry_ids {
            let Some(data) = self.registry.with_entry(*entry_id, |e| ToastSurfaceData {
                entry_id: e.entry_id,
                severity: e.severity,
                priority: e.priority,
                title: e.title.clone(),
                body: e.body.clone(),
                announcement: e.announcement.clone(),
                actions: e.actions.clone(),
                show_close_button: e.show_close_button,
                on_click: e.on_click.clone(),
                style_override: e.style_override.clone(),
            }) else {
                continue;
            };
            let leading = self.registry.take_leading(*entry_id);
            let closable_on_escape = self
                .registry
                .with_entry(*entry_id, |e| e.closable_on_escape)
                .unwrap_or(true);
            let surface =
                ToastSurface::new(data, leading, self.registry.clone(), closable_on_escape);
            surface_ids.push(ctx.add(surface));
        }

        // Auto-dismiss timer. Driven by a one-shot `wake_at` deadline,
        // NOT a per-frame subscription: a visible toast lets the event
        // loop SLEEP until its soonest expiry instead of repainting the
        // whole window at 60 fps just to decrement an invisible counter.
        // `build()` re-runs on every queue mutation (the `version_signal`
        // Rebuild binding above), so the deadline is re-armed whenever a
        // timed toast appears and torn down when the last one expires.
        // (Spinners inside `loading` toasts animate via their own
        // AnimatedQuad path and keep ticking regardless of this.)
        if self.registry.has_running_timers() {
            let registry_for_tick = self.registry.clone();
            let last_tick_at = self.last_tick_at.clone();
            let wake_at = ctx.wake_at_handle();
            let pause_on_hover_group = self.options.pause_on_hover_group;

            // Stamp the dt baseline at arm time so the first deadline wake
            // measures a real elapsed delta. (The effect consults
            // wall-clock, not the frame-tick signal — whose delta is
            // clamped to 0.1 s and would under-count a multi-second sleep.)
            if last_tick_at.borrow().is_none() {
                *last_tick_at.borrow_mut() = Some(Instant::now());
            }

            let wake_for_tick = wake_at.clone();
            ctx.effect(&ctx.frame_tick(), move |_delta_from_signal| {
                let now = Instant::now();
                let dt = {
                    let mut last = last_tick_at.borrow_mut();
                    let result = last
                        .map(|t| now.saturating_duration_since(t))
                        .unwrap_or_default();
                    *last = Some(now);
                    result
                };
                let paused =
                    pause_on_hover_group && registry_for_tick.hover_count_signal().get() > 0;
                registry_for_tick.tick_timers(dt, paused);
                // Re-arm for the next expiry. (An expiry dismisses via a
                // version bump → rebuild, which re-arms too; rescheduling
                // here also covers the case where an unrelated frame ran
                // the effect before the deadline.)
                if registry_for_tick.has_running_timers() {
                    schedule_toast_wake(
                        &registry_for_tick,
                        &wake_for_tick,
                        pause_on_hover_group,
                        now,
                    );
                }
            });

            // Arm the initial deadline so the loop wakes at expiry even if
            // nothing else requests a frame in the meantime.
            schedule_toast_wake(
                &self.registry,
                &wake_at,
                pause_on_hover_group,
                Instant::now(),
            );
        } else {
            // No running timer: reset the dt baseline so the next timed
            // toast measures from its own arrival, not from a stale
            // timestamp left over from a previous toast session (which
            // would otherwise instant-expire it on the first tick).
            *self.last_tick_at.borrow_mut() = None;
        }

        // Pending-dismiss-callback drain handler (attached once).
        //
        // The host fills the whole viewport so its toast children can
        // anchor at absolute corner coordinates, but it must NOT eat
        // clicks meant for the user content below it in the wrapping
        // `ZStack`. `event_pass_through(true)` makes the host
        // transparent to hit-testing: its toast children are still
        // hit-tested first (clicks on a toast land on the toast), but a
        // click that misses every toast falls through to the user root
        // instead of being swallowed by the host's full-viewport
        // background. Without this, *all* pointer input is blocked.
        if !self.has_pending_drain_handler.get() {
            let registry_for_drain = self.registry.clone();
            let handlers =
                HandlerSet::new()
                    .event_pass_through(true)
                    .on_pointer_event(move |_event, ctx| {
                        registry_for_drain.drain_pending_dismiss_callbacks(ctx);
                        bastyde_core::event::EventResponse::Ignored
                    });
            ctx.apply_self_handlers(handlers);
            self.has_pending_drain_handler.set(true);
        }

        self.toast_surface_ids = surface_ids.clone();
        surface_ids
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        // Host fills the proposed viewport. `place_children` positions
        // each toast surface at the configured corner.
        proposal
            .resolve(
                proposal.width.unwrap_or(0.0),
                proposal.height.unwrap_or(0.0),
            )
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        if children.is_empty() {
            return;
        }
        let rtl = ctx.is_rtl();
        let vw = proposal.width.unwrap_or(bounds.width);
        let vh = proposal.height.unwrap_or(bounds.height);

        // Probe each surface's natural size against the host's
        // entry_width so all toasts share a uniform width but hug
        // their natural height.
        let mut surface_sizes = Vec::with_capacity(children.len());
        for placement in children.iter() {
            let resp = ctx
                .child_size(
                    placement.id,
                    SizeProposal {
                        width: Some(self.options.entry_width),
                        height: None,
                    },
                )
                .unwrap_or_else(|| bastyde_canvas::Size::new(self.options.entry_width, 0.0));
            surface_sizes.push(bastyde_canvas::Size::new(
                self.options.entry_width,
                resp.height,
            ));
        }

        // For bottom corners, newer-at-bottom (closest to anchor).
        // For top corners, newer-at-top. Iteration is FIFO by insertion;
        // the offset of each entry from the corner = sum of subsequent
        // entries' heights + gaps.
        let len = children.len();
        for i in 0..len {
            let size = surface_sizes[i];
            let mut stack_offset = self.options.margin.y;
            for j in (i + 1)..len {
                stack_offset += surface_sizes[j].height + self.options.gap;
            }
            let (x, y) = self.options.corner.resolve(
                (size.width, size.height),
                (vw, vh),
                (self.options.margin.x, stack_offset),
                rtl,
            );
            children[i].origin = bastyde_canvas::Point::new(x, y);
            children[i].size = size;
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // The host is invisible chrome — toasts contribute their own
        // AT nodes as descendants. Mark generic + hidden so VoiceOver
        // / NVDA don't insert a dead GenericContainer in the tree.
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
        builder.set_hidden();
    }

    fn children(&self) -> Vec<WidgetId> {
        self.toast_surface_ids.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{Expand, FixedSize, VStack, ZStack};
    use crate::toast::Toast;
    use crate::toast::registry::ToastRegistry;
    use bastyde_canvas::SizeProposal;
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_i18n::LocalizedString;

    fn opts() -> ToastInstallOptions {
        ToastInstallOptions {
            archive: None,
            ..ToastInstallOptions::default()
        }
    }

    /// A user root smaller than the window (the common case: a VStack of
    /// content that does not itself fill the height).
    fn small_root() -> impl Widget {
        VStack::new().child(
            FixedSize::new()
                .bind_width(200.0)
                .bind_height(120.0)
                .child(crate::primitives::Spacer::new()),
        )
    }

    fn surface_bounds(structure: &str) -> (Rect, Rect) {
        let o = opts();
        let registry = ToastRegistry::new(o.clone());
        registry.enqueue(Toast::info(LocalizedString::literal("Hello")));

        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let user_root = tree.add(small_root());
        let host_id = tree.add(ToastHost::new(registry.clone(), o));
        match structure {
            // install_toast as shipped before the fix: ZStack { root, host }.
            "bare" => {
                tree.add(ZStack::new().add_child(user_root).add_child(host_id));
            }
            // install_toast with the Expand fill wrap: ZStack { Expand(root), host }.
            "expand" => {
                let filled = tree.add(Expand::new().respect_intrinsic().child_id(user_root));
                tree.add(ZStack::new().add_child(filled).add_child(host_id));
            }
            _ => unreachable!(),
        }

        tree.layout(SizeProposal::exact(900.0, 600.0));

        let host_bounds = tree.bounds(host_id);
        let surfaces = tree.children(host_id);
        assert_eq!(
            surfaces.len(),
            1,
            "[{structure}] expected one toast surface"
        );
        (host_bounds, tree.bounds(surfaces[0]))
    }

    /// The host must fill the window and place its toast surface at the
    /// bottom-trailing corner, fully on-screen — regardless of whether
    /// the user root is wrapped in an `Expand`. This is the regression
    /// guard for "no toast anywhere" / "toast off-screen".
    #[test]
    fn toast_surface_is_visible_at_bottom_right_with_and_without_expand() {
        for structure in ["bare", "expand"] {
            let (host_bounds, sb) = surface_bounds(structure);

            assert!(
                (host_bounds.width - 900.0).abs() < 0.5 && (host_bounds.height - 600.0).abs() < 0.5,
                "[{structure}] host should fill window, got {host_bounds:?}"
            );
            assert!(sb.height > 1.0, "[{structure}] surface collapsed: {sb:?}");
            assert!(
                sb.y >= -0.5 && sb.y + sb.height <= 600.5,
                "[{structure}] surface vertically off-screen: {sb:?}"
            );
            assert!(
                sb.x >= -0.5 && sb.x + sb.width <= 900.5,
                "[{structure}] surface horizontally off-screen: {sb:?}"
            );
            // bottom-trailing anchor: lower-right quadrant.
            assert!(
                sb.y + sb.height > 400.0,
                "[{structure}] surface not near bottom: {sb:?}"
            );
            assert!(
                sb.x + sb.width > 500.0,
                "[{structure}] surface not near right edge: {sb:?}"
            );
        }
    }

    /// Investigation: a flexless VStack root fills the window (bounds
    /// 900x600) but top-clusters its children, leaving the slack at the
    /// bottom; inserting an `Expand::vertical` between body and status
    /// pins the status bar to the bottom edge. Documents the layout
    /// contract so a future reader doesn't mistake the top-clustering
    /// for a window bug.
    #[test]
    fn flexless_root_top_clusters_expand_pins_to_bottom() {
        use crate::primitives::Spacer;
        let build = |with_expand: bool| {
            let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
            let toolbar = tree.add(
                FixedSize::new()
                    .bind_width(900.0)
                    .bind_height(40.0)
                    .child(Spacer::new()),
            );
            let status = tree.add(
                FixedSize::new()
                    .bind_width(900.0)
                    .bind_height(30.0)
                    .child(Spacer::new()),
            );
            let mut vstack = VStack::new().spacing(0.0).add_child(toolbar);
            if with_expand {
                let body = tree.add(
                    FixedSize::new()
                        .bind_width(900.0)
                        .bind_height(100.0)
                        .child(Spacer::new()),
                );
                let filled = tree.add(Expand::vertical().respect_intrinsic().child_id(body));
                vstack = vstack.add_child(filled);
            } else {
                let body = tree.add(
                    FixedSize::new()
                        .bind_width(900.0)
                        .bind_height(100.0)
                        .child(Spacer::new()),
                );
                vstack = vstack.add_child(body);
            }
            vstack = vstack.add_child(status);
            let root = tree.add(vstack);
            tree.layout(SizeProposal::exact(900.0, 600.0));
            (tree.bounds(root), tree.bounds(status))
        };

        let (root_plain, status_plain) = build(false);
        assert!((root_plain.height - 600.0).abs() < 0.5, "root fills window");
        assert!(
            (status_plain.y - 140.0).abs() < 0.5,
            "flexless: status top-clusters at 140"
        );

        let (root_exp, status_exp) = build(true);
        assert!(
            (root_exp.height - 600.0).abs() < 0.5,
            "root still fills window"
        );
        assert!(
            (status_exp.y + status_exp.height - 600.0).abs() < 0.5,
            "with Expand::vertical the status bar pins to the bottom edge, got {status_exp:?}"
        );
    }

    /// The host is built at startup with an EMPTY registry (the common
    /// case: the main window). A toast enqueued LATER (via
    /// `show_toast`) must drive a rebuild so the surface appears on the
    /// next layout pass. This reproduces the real-app path that the
    /// first test (toast present at build time) does not exercise.
    #[test]
    fn host_shows_toast_enqueued_after_initial_layout() {
        let o = opts();
        let registry = ToastRegistry::new(o.clone());

        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let user_root = tree.add(small_root());
        let host_id = tree.add(ToastHost::new(registry.clone(), o));
        let filled = tree.add(Expand::new().respect_intrinsic().child_id(user_root));
        tree.add(ZStack::new().add_child(filled).add_child(host_id));

        tree.layout(SizeProposal::exact(900.0, 600.0));
        assert_eq!(
            tree.children(host_id).len(),
            0,
            "no toast should be present before any enqueue"
        );

        // Equivalent to `ctx.show_toast(...)` after startup.
        registry.enqueue(Toast::info(LocalizedString::literal("Later")));

        // A subsequent layout pass (next frame) must rebuild the host
        // and materialise the surface.
        tree.layout(SizeProposal::exact(900.0, 600.0));
        assert_eq!(
            tree.children(host_id).len(),
            1,
            "toast enqueued after initial layout did not appear — host did not rebuild"
        );
    }

    /// Full auto-dismiss lifecycle in a live `WidgetTree`: a timed toast
    /// appears, the host arms its per-frame timer (gate true), then the
    /// timer expires, the surface is removed, and the gate goes false so
    /// the event loop can sleep again. This is the headless analogue of
    /// the real-window CPU time-series: pump while a toast is alive, idle
    /// once it auto-dismisses.
    #[test]
    fn timed_toast_auto_dismisses_and_releases_the_frame_loop() {
        use std::time::Duration;

        let o = opts();
        let registry = ToastRegistry::new(o.clone());

        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let user_root = tree.add(small_root());
        let host_id = tree.add(ToastHost::new(registry.clone(), o));
        let filled = tree.add(Expand::new().respect_intrinsic().child_id(user_root));
        tree.add(ZStack::new().add_child(filled).add_child(host_id));

        tree.layout(SizeProposal::exact(900.0, 600.0));
        assert_eq!(tree.children(host_id).len(), 0);
        assert!(
            !registry.has_running_timers(),
            "empty host must not keep the frame loop awake"
        );

        // Show a toast with a finite auto-dismiss timer.
        registry.enqueue(
            Toast::info(LocalizedString::literal("Saved"))
                .auto_dismiss_after(Duration::from_millis(500)),
        );
        tree.layout(SizeProposal::exact(900.0, 600.0));
        assert_eq!(tree.children(host_id).len(), 1, "surface should appear");
        assert!(
            registry.has_running_timers(),
            "a live timed toast must arm the frame loop"
        );

        // Frames elapse past the timeout (the host's frame-tick effect
        // calls this with wall-clock dt; we drive it directly).
        let expired = registry.tick_timers(Duration::from_millis(600), false);
        assert!(expired, "the toast should expire after its timeout");

        // Next layout pass rebuilds the host: surface gone, loop idle.
        tree.layout(SizeProposal::exact(900.0, 600.0));
        assert_eq!(
            tree.children(host_id).len(),
            0,
            "expired toast surface should be torn down"
        );
        assert!(
            !registry.has_running_timers(),
            "after the last timer expires the host must release the frame loop"
        );
    }

    /// A visible timed toast must arm a one-shot `wake_at` deadline so
    /// the event loop sleeps until expiry — not pin a 60 fps poll. An
    /// empty host, or one holding only sticky toasts, arms no deadline.
    #[test]
    fn timed_toast_schedules_a_deadline_not_a_poll() {
        use std::time::{Duration, Instant};

        let o = opts();
        let registry = ToastRegistry::new(o.clone());

        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let user_root = tree.add(small_root());
        let host_id = tree.add(ToastHost::new(registry.clone(), o));
        let filled = tree.add(Expand::new().respect_intrinsic().child_id(user_root));
        tree.add(ZStack::new().add_child(filled).add_child(host_id));

        let wake = tree.wake_at_handle();

        // Empty host: nothing to wait for.
        tree.layout(SizeProposal::exact(900.0, 600.0));
        assert!(
            wake.get().is_none(),
            "empty host must not arm a wake deadline"
        );

        // Sticky-only: a persistent toast has no timer → still no deadline.
        registry.enqueue(Toast::error(LocalizedString::literal("sticky")).persistent());
        tree.layout(SizeProposal::exact(900.0, 600.0));
        assert!(
            wake.get().is_none(),
            "a sticky toast has no timer, so no deadline is armed"
        );

        // Timed toast: a future deadline appears (not an immediate wake).
        let before = Instant::now();
        registry.enqueue(
            Toast::info(LocalizedString::literal("timed"))
                .auto_dismiss_after(Duration::from_secs(5)),
        );
        tree.layout(SizeProposal::exact(900.0, 600.0));
        let deadline = wake.get();
        assert!(deadline.is_some(), "timed toast must arm a wake deadline");
        assert!(
            deadline.unwrap() > before,
            "deadline must be in the future, not an immediate busy-wake"
        );
    }
}
