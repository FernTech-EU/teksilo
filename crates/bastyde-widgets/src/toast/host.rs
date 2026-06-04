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
use std::time::Instant;

use bastyde_canvas::{Rect, SizeProposal, Vec2};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::frame_tick_scheduler::FrameTickSubscription;
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
    /// Owner subscription — dropped on rebuild and re-issued. Drives
    /// the per-frame timer + hover-pause logic.
    frame_tick_sub: Option<FrameTickSubscription>,
    /// `Instant` of the last frame-tick — used to compute `dt`.
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
            frame_tick_sub: None,
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

        // Frame-tick effect: decrement timers, dismiss expired.
        let registry_for_tick = self.registry.clone();
        let last_tick_at = self.last_tick_at.clone();
        let pause_on_hover_group = self.options.pause_on_hover_group;
        ctx.effect(&ctx.frame_tick(), move |_delta_from_signal| {
            // Use real-time delta. (The signal carries the framework's
            // measured delta, but for tests + simulated clocks we
            // recompute from wall-clock so the host stays in sync
            // regardless of how the tick was scheduled.)
            let now = Instant::now();
            let dt = {
                let mut last = last_tick_at.borrow_mut();
                let result = last
                    .map(|t| now.saturating_duration_since(t))
                    .unwrap_or_default();
                *last = Some(now);
                result
            };
            if dt.is_zero() {
                return;
            }
            let paused = pause_on_hover_group && registry_for_tick.hover_count_signal().get() > 0;
            registry_for_tick.tick_timers(dt, paused);
        });
        // Replace any prior subscription with a fresh one.
        self.frame_tick_sub = None;
        self.frame_tick_sub = Some(ctx.subscribe_frame_tick());

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
            let handlers = HandlerSet::new()
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
        assert_eq!(surfaces.len(), 1, "[{structure}] expected one toast surface");
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
                (host_bounds.width - 900.0).abs() < 0.5
                    && (host_bounds.height - 600.0).abs() < 0.5,
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
            let toolbar = tree.add(FixedSize::new().bind_width(900.0).bind_height(40.0).child(Spacer::new()));
            let status = tree.add(FixedSize::new().bind_width(900.0).bind_height(30.0).child(Spacer::new()));
            let mut vstack = VStack::new().spacing(0.0).add_child(toolbar);
            if with_expand {
                let body = tree.add(FixedSize::new().bind_width(900.0).bind_height(100.0).child(Spacer::new()));
                let filled = tree.add(Expand::vertical().respect_intrinsic().child_id(body));
                vstack = vstack.add_child(filled);
            } else {
                let body = tree.add(FixedSize::new().bind_width(900.0).bind_height(100.0).child(Spacer::new()));
                vstack = vstack.add_child(body);
            }
            vstack = vstack.add_child(status);
            let root = tree.add(vstack);
            tree.layout(SizeProposal::exact(900.0, 600.0));
            (tree.bounds(root), tree.bounds(status))
        };

        let (root_plain, status_plain) = build(false);
        assert!((root_plain.height - 600.0).abs() < 0.5, "root fills window");
        assert!((status_plain.y - 140.0).abs() < 0.5, "flexless: status top-clusters at 140");

        let (root_exp, status_exp) = build(true);
        assert!((root_exp.height - 600.0).abs() < 0.5, "root still fills window");
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
}
