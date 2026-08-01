// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `NotificationCenterButton` — bell icon with an unread-count badge that
//! opens a [`NotificationLog`] popover when clicked.
//!
//! Composed as a `ZStack { PopoverIconButton(bell), Badge }`. The badge
//! shows the current unread count and is hit-transparent so clicks always
//! reach the bell beneath. On popover close the archive's `mark_all_read`
//! is called and the badge resets — matching the GitHub / Slack / JetBrains
//! convention. Most apps mount this in a `StatusBar` or `TitleBar` trailing
//! slot; all popover behaviour is self-managed with no further wiring.
//!
//! ## Accessibility
//!
//! The inner `IconButton` carries the bell `Role::Button` label; the outer
//! container is `set_hidden` (presentational). The badge count is not
//! separately announced — the button label and badge label together convey
//! the state to sighted users; AT users interact through the button itself.
//!
//! ```ignore
//! // Typical setup — archive comes from install_toast_default():
//! let archive: Rc<NotificationArchiveModel> = ctx.app_state().unwrap();
//! let bell = NotificationCenterButton::new(archive)
//!     .on_action_invoked(|_entry, action, ctx| {
//!         if let Some(name) = &action.intent_name {
//!             ctx.send_intent(bastyde_core::Intent::new(name));
//!         }
//!     });
//! ```

use bastyde_i18n::{LocalizedString, lit};
use std::rc::Rc;

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::overlay::{DismissBehavior, OverlayPlacement};
use bastyde_core::widget::{EventContext, LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;

use bastyde_core::widget_builder::WidgetBuilder;
use bastyde_tokens::Alignment;

use crate::badge::Badge;
use crate::icon_button::{IconButton, IconButtonSize};
use crate::notification::log::NotificationLog;
use crate::notification::{
    ArchivedAction, NotificationArchiveModel, NotificationEntry, route_visible,
};
use crate::popover_widget::PopoverIconButton;
use crate::primitives::ZStack;
use crate::toast::{ToastAudience, ToastRoute};
use bastyde_core::window::BastydeWindowId;

/// Bell-icon trigger + unread-count badge + popover that contains a
/// [`NotificationLog`]. On popover open the archive's `mark_all_read`
/// runs (the user is presumed to have seen the toasts now).
pub struct NotificationCenterButton {
    archive: Rc<NotificationArchiveModel>,
    size: IconButtonSize,
    show_badge_when_zero: bool,
    max_badge_count: u32,
    placement: OverlayPlacement,
    on_action_invoked: Option<Rc<dyn Fn(&NotificationEntry, &ArchivedAction, &mut EventContext)>>,
    root_child_id: Option<WidgetId>,
    /// Plain single-line tooltip text shown after a hover delay.
    /// Mutually exclusive with `rich_tooltip_source` and
    /// `composite_tooltip_content` — last setter wins.
    tooltip_text: Option<LocalizedString>,
    /// Rich tooltip source (registry key or inline content).
    /// Mutually exclusive with `tooltip_text` and `composite_tooltip_content`.
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    /// Composite tooltip body (arbitrary widget tree).
    /// Mutually exclusive with `tooltip_text` and `rich_tooltip_source`.
    composite_tooltip_content: Option<Box<dyn Widget>>,
    /// `None` (default) = unscoped — the badge counts every unread
    /// entry in the whole shared archive and the popover shows every
    /// entry, matching this widget's behaviour before routing existed.
    /// `Some(route)` restricts both to entries matching `route` (plus
    /// `Broadcast`, always counted/shown). Set via [`Self::for_window`]
    /// / [`Self::for_audience`].
    route_scope: Option<ToastRoute>,
}

impl NotificationCenterButton {
    /// Construct bound to a shared archive. The archive is typically
    /// held in `app_state` and cloned to every consumer.
    pub fn new(archive: Rc<NotificationArchiveModel>) -> Self {
        Self {
            archive,
            size: IconButtonSize::Toolbar,
            show_badge_when_zero: false,
            max_badge_count: 99,
            placement: OverlayPlacement::BelowPreferred,
            on_action_invoked: None,
            root_child_id: None,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
            route_scope: None,
        }
    }

    /// Scope this bell to window `window_id`: its badge counts unread
    /// among entries routed to that window (plus any `Broadcast`
    /// entry), and its popover shows only those. Overrides any
    /// previous `for_window` / `for_audience` call.
    pub fn for_window(mut self, window_id: BastydeWindowId) -> Self {
        self.route_scope = Some(ToastRoute::Window(window_id));
        self
    }

    /// Scope this bell to `audience`: its badge counts unread among
    /// entries routed to that audience (plus any `Broadcast` entry),
    /// and its popover shows only those. Overrides any previous
    /// `for_window` / `for_audience` call.
    pub fn for_audience(mut self, audience: ToastAudience) -> Self {
        self.route_scope = Some(ToastRoute::Audience(audience));
        self
    }

    /// Bell-icon size. Default `IconButtonSize::Toolbar` (30 dp) —
    /// matches the JetBrains status-bar density.
    pub fn size(mut self, size: IconButtonSize) -> Self {
        self.size = size;
        self
    }

    /// Whether to keep the badge visible when the unread count is
    /// zero. Default `false` (badge hidden when no unread). Apps
    /// that want a persistent "0" indicator pass `true`.
    pub fn show_badge_when_zero(mut self, show: bool) -> Self {
        self.show_badge_when_zero = show;
        self
    }

    /// Cap the displayed badge count. Default `99` — counts above
    /// the cap display as `"99+"`. Set to `u32::MAX` to disable the
    /// cap.
    pub fn max_badge_count(mut self, max: u32) -> Self {
        self.max_badge_count = max;
        self
    }

    /// Popover placement relative to the bell. Default
    /// `BelowPreferred` — flips above when the button is near the
    /// viewport bottom edge.
    pub fn placement(mut self, p: OverlayPlacement) -> Self {
        self.placement = p;
        self
    }

    /// Threaded into the embedded `NotificationLog` —
    /// see [`NotificationLog::on_action_invoked`] for the contract.
    /// Wire this to dispatch archived actions; without it the
    /// action buttons in the log are inert.
    pub fn on_action_invoked(
        mut self,
        f: impl Fn(&NotificationEntry, &ArchivedAction, &mut EventContext) + 'static,
    ) -> Self {
        self.on_action_invoked = Some(Rc::new(f));
        self
    }

    /// Attach a plain single-line tooltip shown after a hover delay.
    ///
    /// Mutually exclusive with [`rich_tooltip`](Self::rich_tooltip),
    /// [`rich_tooltip_content`](Self::rich_tooltip_content), and
    /// [`composite_tooltip`](Self::composite_tooltip) — the last setter
    /// called wins.
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip identified by a registry key.
    ///
    /// Mutually exclusive with [`tooltip`](Self::tooltip),
    /// [`rich_tooltip_content`](Self::rich_tooltip_content), and
    /// [`composite_tooltip`](Self::composite_tooltip).
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip from inline [`crate::tooltip::TooltipContent`].
    ///
    /// Mutually exclusive with [`tooltip`](Self::tooltip),
    /// [`rich_tooltip`](Self::rich_tooltip), and
    /// [`composite_tooltip`](Self::composite_tooltip).
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a composite tooltip containing an arbitrary widget tree.
    ///
    /// Mutually exclusive with [`tooltip`](Self::tooltip),
    /// [`rich_tooltip`](Self::rich_tooltip), and
    /// [`rich_tooltip_content`](Self::rich_tooltip_content).
    pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
        self
    }
}

impl std::fmt::Debug for NotificationCenterButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NotificationCenterButton")
            .field("size", &self.size)
            .field("show_badge_when_zero", &self.show_badge_when_zero)
            .field("placement", &self.placement)
            .finish_non_exhaustive()
    }
}

impl Widget for NotificationCenterButton {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let archive = self.archive.clone();
        let max_badge = self.max_badge_count;
        let show_when_zero = self.show_badge_when_zero;
        let scope = self.route_scope;

        // Bind to the archive's mutation version (not `unread_count`)
        // at Rebuild — a scoped bell's badge count is a local scan
        // over `archive.entries()` (see below), so it must rebuild on
        // ANY archive mutation that could change which of ITS entries
        // are unread, not just the (global, unscoped) `unread_count`
        // signal. Matches `NotificationLog`'s own binding.
        //
        // One signal for every window's bell: this window's own
        // `BindingRegistry` remembers the generation it last
        // reconciled, so a bell in window B cannot miss a mutation
        // just because window A's tree reconciled first.
        archive.version_signal().bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Rebuild,
        );

        // Bell trigger: an IconButton(bell) at the requested size.
        let trigger = IconButton::bell().size(self.size);

        // Popover content: a NotificationLog, scoped identically to
        // this bell so the popover body and the badge always agree on
        // which entries "belong" to this window/audience. The
        // on_action_invoked hook is forwarded if present.
        let mut log = NotificationLog::new(archive.clone());
        log = match scope {
            Some(ToastRoute::Window(w)) => log.for_window(w),
            Some(ToastRoute::Audience(a)) => log.for_audience(a),
            Some(ToastRoute::Broadcast) | None => log,
        };
        if let Some(cb) = self.on_action_invoked.clone() {
            log = log.on_action_invoked(move |e, a, ctx| cb(e, a, ctx));
        }

        // `PopoverIconButton` wraps the content in the themed popover
        // surface (background, border, padding, shadow) by default, so
        // the chrome-less `NotificationLog` gets a proper surface for
        // free — no manual `Panel` needed.

        // Bell + popover combo. Mark archive entries read when the
        // popover *closes*, NOT when it opens — mutating the archive
        // bumps `version_signal`, which fires this widget's `Rebuild`
        // binding, and a rebuild on OPEN would tear down the
        // `PopoverIconButton` (and its just-shown overlay) and replace
        // it with a fresh, closed one, so the popover would flash and
        // vanish, leaving only the cleared badge. Deferring to close
        // lets the rebuild happen after the popover is already gone.
        // Scoped exactly like the toolbar's mark-all-read above: a
        // scoped bell must only mark ITS entries read, never every
        // window's/audience's history.
        let archive_for_close = archive.clone();
        let pib = PopoverIconButton::new(trigger)
            .content(log)
            .placement(self.placement.clone())
            .dismiss_behavior(DismissBehavior::EscapeOrClickOutside)
            .on_close(move || match scope {
                Some(s) => archive_for_close.mark_read_where(|e| route_visible(e.route, Some(s))),
                None => archive_for_close.mark_all_read(),
            });
        let pib_id = ctx.add(pib);

        // Compute the badge label for this build — a local scan over
        // the (bounded, ≤ DEFAULT_ARCHIVE_LIMIT) archive entries rather
        // than a dedicated per-audience counter signal: cheap, and it
        // is the single source of truth `route_visible` already uses
        // for the popover body, so the two can never disagree.
        let model = archive.entries();
        let unread_count = (0..model.len())
            .filter(|&i| {
                model
                    .with_item(i, |e| !e.read && route_visible(e.route, scope))
                    .unwrap_or(false)
            })
            .count();
        let label = if unread_count == 0 {
            String::new()
        } else if unread_count > max_badge as usize {
            format!("{max_badge}+")
        } else {
            unread_count.to_string()
        };

        // Stack bell + badge. Badge is omitted entirely when there
        // are no unread (and `show_when_zero` is false) so the bell
        // renders bare.
        //
        // The badge is pinned to the top-trailing corner (where count
        // badges belong) via the stack alignment, and its whole subtree
        // is marked hit-transparent. A `ZStack` centers its children by
        // default, so the badge sat on top of the bell icon; `Badge` is
        // a *composite* widget, so `event_pass_through` (per-node) would
        // not help — its inner text/rect children still swallowed the
        // tap, and the popover never opened whenever there were unread
        // notifications (i.e. exactly when you'd press the bell).
        // `hit_transparent` excludes the entire badge subtree from
        // hit-testing, so the click falls through to the bell beneath.
        let mut stack = ZStack::new()
            .alignment(Alignment::TOP_TRAILING)
            .add_child(pib_id);
        if unread_count > 0 || show_when_zero {
            let badge_id = ctx.add(Badge::new(lit!(label)).hit_transparent(true));
            stack = stack.add_child(badge_id);
        }
        let root = ctx.add(stack);

        // Attach tooltip if configured. The three setters
        // (`tooltip`, `rich_tooltip*`, `composite_tooltip`) are
        // mutually exclusive — every setter clears the other two so
        // exactly one branch runs.
        if let Some(content) = self.composite_tooltip_content.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed(ctx, root, content, delay);
        } else if let Some(source) = self.rich_tooltip_source.clone() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_rich_tooltip_source(ctx, root, source, delay);
        } else if let Some(text) = self.tooltip_text.clone() {
            let tooltip_widget = crate::tooltip::TooltipWidget::new(text);
            let tooltip_id = ctx.add(tooltip_widget);
            let delay = ctx.theme().motion.tooltip_delay;
            ctx.attach_tooltip(root, tooltip_id, delay);
        }

        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(30.0, 30.0))
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

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // The IconButton inside contributes its own role + name;
        // we pass through as a generic container.
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
        builder.set_hidden();
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notification::NotificationEntry;
    use bastyde_core::styles::{BannerSeverity, ToastPriority};
    use bastyde_core::widget_tree::WidgetTree;

    fn entry(title: &str) -> NotificationEntry {
        entry_with_route(title, ToastRoute::Broadcast)
    }

    fn entry_with_route(title: &str, route: ToastRoute) -> NotificationEntry {
        NotificationEntry {
            id: 0,
            severity: BannerSeverity::Info,
            priority: ToastPriority::Normal,
            title: title.to_string(),
            body: None,
            actions: Vec::new(),
            timestamp: jiff::Timestamp::UNIX_EPOCH,
            group: None,
            source: None,
            read: false,
            dedup_id: None,
            updates: Vec::new(),
            route,
        }
    }

    fn tree_with(btn: NotificationCenterButton) -> WidgetTree {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(btn);
        tree.layout(SizeProposal::exact(120.0, 60.0));
        tree
    }

    #[test]
    fn bell_label_present() {
        let archive = Rc::new(NotificationArchiveModel::in_memory());
        let tree = tree_with(NotificationCenterButton::new(archive));
        let bell_label = bastyde_i18n::tr_widget!(a11y_builtin_bell()).resolve_now();
        assert!(
            tree.find_by_label(&bell_label).is_some(),
            "bell tooltip / label present in the AT tree"
        );
    }

    #[test]
    fn badge_appears_when_unread_count_grows() {
        let archive = Rc::new(NotificationArchiveModel::in_memory());
        // Pre-populate before mounting — the rebuild-on-signal-change
        // path doesn't fully fire in unit-test layout passes
        // (same caveat as the toast host tests).
        archive.push(entry("a"));
        archive.push(entry("b"));
        assert_eq!(archive.unread_count().get(), 2);
        let tree = tree_with(NotificationCenterButton::new(archive));
        assert!(
            tree.find_by_label("2").is_some(),
            "badge with count '2' renders when unread_count > 0"
        );
    }

    /// Reproduces the real app: bell mounted at the BOTTOM of the
    /// window (status bar), under the full-viewport pass-through toast
    /// host installed by `install_toast`. Clicking it must open the
    /// popover overlay, and the popover must land on-screen.
    /// Mounts the bell at the bottom of the window (status-bar
    /// position), optionally under the full-viewport pass-through toast
    /// host installed by `install_toast`, with `unread` notifications in
    /// the archive. Returns (active overlays before click, after click).
    fn bell_popover_open_check(with_toast_host: bool, unread: usize) -> (usize, usize) {
        use crate::primitives::{Expand, FixedSize, Spacer, VStack, ZStack};
        use crate::toast::{ToastHost, ToastInstallOptions, ToastRegistry};

        let archive = Rc::new(NotificationArchiveModel::in_memory());
        for i in 0..unread {
            archive.push(entry(&format!("n{i}")));
        }

        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());

        // A spacer pushes the bell to the bottom edge (status bar).
        let spacer = tree.add(FixedSize::new().height(500.0).child(Spacer::new()));
        let bell = tree.add(NotificationCenterButton::new(archive.clone()));
        let user_root = tree.add(VStack::new().add_child(spacer).add_child(bell));

        if with_toast_host {
            // Mirror install_toast: ZStack { Expand(user_root), host }.
            let opts = ToastInstallOptions {
                archive: None,
                ..ToastInstallOptions::default()
            };
            let registry = ToastRegistry::new(opts.clone());
            let filled = tree.add(Expand::new().respect_intrinsic().child_id(user_root));
            let host = tree.add(ToastHost::new(registry, opts));
            tree.add(ZStack::new().add_child(filled).add_child(host));
        }

        tree.layout(SizeProposal::exact(400.0, 600.0));

        let before = tree.active_overlays().len();
        tree.click(bell);
        tree.layout(SizeProposal::exact(400.0, 600.0));
        let after = tree.active_overlays().len();
        (before, after)
    }

    #[test]
    fn bell_popover_opens_with_no_unread() {
        // Empty archive → no badge → isolates the popover mechanism.
        let (before, after) = bell_popover_open_check(false, 0);
        assert_eq!(after, before + 1, "popover should open (no badge)");
    }

    /// Clicking an in-content action ("mark all read" / "clear") mutates
    /// the archive, which changes `unread_count` and rebuilds the bell —
    /// destroying the popover's owner. The overlay must NOT linger as an
    /// invisible click-blocker; it must be fully dismissed.
    #[test]
    fn in_content_action_does_not_orphan_overlay() {
        use crate::primitives::{FixedSize, Spacer, VStack};
        let archive = Rc::new(NotificationArchiveModel::in_memory());
        for i in 0..3 {
            archive.push(entry(&format!("n{i}")));
        }
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let spacer = tree.add(FixedSize::new().height(500.0).child(Spacer::new()));
        let bell = tree.add(NotificationCenterButton::new(archive.clone()));
        tree.add(VStack::new().add_child(spacer).add_child(bell));
        tree.layout(SizeProposal::exact(400.0, 600.0));

        tree.click(bell);
        tree.layout(SizeProposal::exact(400.0, 600.0));
        assert_eq!(tree.active_overlays().len(), 1, "popover should be open");

        // Simulate clicking "Mark all read" inside the log.
        archive.mark_all_read();
        tree.layout(SizeProposal::exact(400.0, 600.0));
        assert_eq!(
            tree.active_overlays().len(),
            0,
            "overlay must be dismissed (not left as an invisible click-blocker) \
             after the in-content action rebuilds the bell"
        );
    }

    /// Two trees with NO window state, sharing one archive: one push,
    /// **both** must come out needing a render.
    ///
    /// Distinct from `both_unscoped_bells_pick_up_a_badge_change_*`
    /// below, which give their trees real `BastydeWindowId`s. Those
    /// used to be served by a per-window duplicate of the version
    /// signal; a windowless tree fell through to the shared one and was
    /// exactly the configuration that broke. Dirty tracking used to be
    /// a `bool` on the signal that each tree's reconcile pass read *and
    /// cleared*, so whichever tree laid out first consumed it and the
    /// other silently — and permanently — kept a stale badge. Verified
    /// against the pre-fix tree: this test failed on window B.
    ///
    /// Reconciles in the opposite order the second time round. The old
    /// failure picked its victim by `HashMap` iteration order, so a
    /// test that only ever laid out A-then-B could pass against a
    /// "fix" that merely moved which window loses.
    #[test]
    fn two_windowless_trees_both_rebuild_on_one_archive_push() {
        use crate::primitives::{FixedSize, Spacer, VStack};

        let archive = Rc::new(NotificationArchiveModel::in_memory());

        let mut window = |_| {
            let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
            let spacer = tree.add(FixedSize::new().height(500.0).child(Spacer::new()));
            let bell = tree.add(NotificationCenterButton::new(archive.clone()));
            tree.add(VStack::new().add_child(spacer).add_child(bell));
            tree.layout(SizeProposal::exact(400.0, 600.0));
            tree.render();
            tree
        };
        let (mut a, mut b) = (window(()), window(()));
        assert!(!a.needs_render() && !b.needs_render(), "both start clean");

        // Round 1 — reconcile A first, then B.
        archive.push(entry("from somewhere"));
        a.layout(SizeProposal::exact(400.0, 600.0));
        assert!(a.needs_render(), "window A's bell must rebuild");
        b.layout(SizeProposal::exact(400.0, 600.0));
        assert!(
            b.needs_render(),
            "window B's bell must rebuild too — A's reconcile consumed nothing"
        );
        a.render();
        b.render();

        // Round 2 — same push, opposite reconcile order.
        archive.push(entry("and again"));
        b.layout(SizeProposal::exact(400.0, 600.0));
        assert!(b.needs_render(), "window B first this time");
        a.layout(SizeProposal::exact(400.0, 600.0));
        assert!(a.needs_render(), "and window A still follows");
    }

    #[test]
    fn bell_popover_opens_with_unread_badge() {
        // Regression: a centered, hit-testable badge swallowed the tap,
        // so the popover never opened when there were unread items.
        let (before, after) = bell_popover_open_check(false, 3);
        assert_eq!(
            after,
            before + 1,
            "popover must open even with an unread badge"
        );
    }

    #[test]
    fn bell_popover_opens_under_toast_host_with_badge() {
        let (before, after) = bell_popover_open_check(true, 3);
        assert_eq!(
            after,
            before + 1,
            "popover must open under the toast host, with a badge"
        );
    }

    #[test]
    fn badge_caps_at_max_count() {
        let archive = Rc::new(NotificationArchiveModel::in_memory());
        for i in 0..150 {
            archive.push(entry(&format!("t{i}")));
        }
        assert_eq!(archive.unread_count().get(), 150);
        let tree = tree_with(NotificationCenterButton::new(archive).max_badge_count(99));
        assert!(
            tree.find_by_label("99+").is_some(),
            "badge caps at '99+' for counts above max"
        );
    }

    #[test]
    fn scoped_bell_only_counts_its_audience_and_broadcast_unread() {
        use crate::toast::ToastAudience;

        let archive = Rc::new(NotificationArchiveModel::in_memory());
        let audience_a = ToastAudience::new(1);
        let audience_b = ToastAudience::new(2);

        archive.push(entry_with_route("for a", ToastRoute::Audience(audience_a)));
        archive.push(entry_with_route("for b", ToastRoute::Audience(audience_b)));
        archive.push(entry_with_route("for b again", ToastRoute::Audience(audience_b)));
        archive.push(entry_with_route("everyone", ToastRoute::Broadcast));
        assert_eq!(
            archive.unread_count().get(),
            4,
            "the shared archive's global counter sees all four"
        );

        // Bell scoped to Window(1): no entry is routed to that window,
        // so only the broadcast one counts → badge shows "1". This
        // proves window-scoping and audience-scoping are independent:
        // a window-scoped bell shows only Window(_) + Broadcast, never
        // an Audience(_) entry.
        let tree_a = tree_with(NotificationCenterButton::new(archive.clone()).for_window(
            BastydeWindowId::new(1),
        ));
        assert!(
            tree_a.find_by_label("1").is_some(),
            "no entry is routed to Window(1); only the broadcast one should count"
        );

        // Bell scoped to audience A: "for a" (1) + "everyone" (1) = 2.
        let tree_scoped_a =
            tree_with(NotificationCenterButton::new(archive.clone()).for_audience(audience_a));
        assert!(
            tree_scoped_a.find_by_label("2").is_some(),
            "audience A's bell counts its own entry plus the broadcast one"
        );

        // Bell scoped to audience B: "for b" + "for b again" (2) +
        // "everyone" (1) = 3.
        let tree_scoped_b =
            tree_with(NotificationCenterButton::new(archive.clone()).for_audience(audience_b));
        assert!(
            tree_scoped_b.find_by_label("3").is_some(),
            "audience B's bell counts both of its own entries plus the broadcast one"
        );

        // Unscoped bell (legacy, back-compat path): sees the whole
        // shared archive, exactly like before routing existed.
        let tree_unscoped = tree_with(NotificationCenterButton::new(archive));
        assert!(
            tree_unscoped.find_by_label("4").is_some(),
            "an unscoped bell keeps the old 'see everything' behaviour"
        );
    }

    /// End-to-end counterpart of `scoped_bell_only_counts_its_audience_and_broadcast_unread`
    /// above: that test (and every other one in this file) proves the
    /// SCOPING FILTER is correct by hand-building `NotificationEntry`
    /// rows with `entry_with_route`. It never goes through the real
    /// `ToastRegistry::enqueue` → archive-mirror path, so it can't
    /// catch a regression in the OTHER half of the seam: whether a
    /// toast's resolved route actually survives the trip into the
    /// archive at all (see `toast.rs`'s
    /// `registry_mirrors_the_resolved_route_onto_the_archived_entry`
    /// for that half in isolation). This test drives both halves
    /// together — real toasts, real routes, real archive mirror, real
    /// scoped bell — the shape a Skribisto per-Work bell actually sees.
    #[test]
    fn scoped_bell_reflects_toasts_presented_through_the_real_registry_pipeline() {
        use crate::toast::host::ToastInstallOptions;
        use crate::toast::{Toast, ToastAudience, ToastRegistry};

        let archive = Rc::new(NotificationArchiveModel::in_memory());
        let registry = ToastRegistry::with_archive(
            ToastInstallOptions {
                archive: None,
                ..ToastInstallOptions::default()
            },
            archive.clone(),
        );
        let audience_a = ToastAudience::new(1);
        let audience_b = ToastAudience::new(2);

        registry.enqueue(Toast::info(lit!("for a")).target(audience_a));
        registry.enqueue(Toast::info(lit!("for b")).target(audience_b));
        registry.enqueue(Toast::warning(lit!("everyone")).broadcast());
        assert_eq!(
            archive.unread_count().get(),
            3,
            "all three toasts were mirrored into the shared archive"
        );

        let tree_a =
            tree_with(NotificationCenterButton::new(archive.clone()).for_audience(audience_a));
        assert!(
            tree_a.find_by_label("2").is_some(),
            "audience A's bell must count its own real toast plus the broadcast one \
             (2), excluding B's — not 3 (everything) and not 1 (missing the broadcast)"
        );

        let tree_b = tree_with(NotificationCenterButton::new(archive).for_audience(audience_b));
        assert!(
            tree_b.find_by_label("2").is_some(),
            "audience B's bell must count its own real toast plus the broadcast one, \
             excluding A's"
        );
    }

    #[test]
    fn scoped_bell_close_only_marks_its_own_entries_read() {
        use crate::primitives::{FixedSize, Spacer, VStack};
        use crate::toast::ToastAudience;

        let archive = Rc::new(NotificationArchiveModel::in_memory());
        let audience_a = ToastAudience::new(1);
        let audience_b = ToastAudience::new(2);
        archive.push(entry_with_route("for a", ToastRoute::Audience(audience_a)));
        archive.push(entry_with_route("for b", ToastRoute::Audience(audience_b)));
        assert_eq!(archive.unread_count().get(), 2);

        // Mirror `bell_popover_open_check`'s status-bar layout (bell
        // pinned to the bottom of a normal-sized window via a leading
        // spacer) rather than the bare `tree_with` 120x60 helper: in a
        // window that tiny, `BelowPreferred`'s popover has nowhere to
        // go but directly over the bell, so the second synthesized
        // click (which re-hit-tests at the bell's screen coordinate)
        // lands on the popover instead of toggling the trigger closed.
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let spacer = tree.add(FixedSize::new().height(500.0).child(Spacer::new()));
        let bell =
            tree.add(NotificationCenterButton::new(archive.clone()).for_audience(audience_a));
        tree.add(VStack::new().add_child(spacer).add_child(bell));
        tree.layout(SizeProposal::exact(400.0, 600.0));

        // Open then close the popover — closing is what triggers the
        // scoped mark-read.
        tree.click(bell);
        tree.layout(SizeProposal::exact(400.0, 600.0));
        tree.click(bell); // PopoverIconButton toggles: second click closes it.
        tree.layout(SizeProposal::exact(400.0, 600.0));

        assert_eq!(
            archive.unread_count().get(),
            1,
            "only audience A's entry was marked read; audience B's stays unread"
        );
    }

    // -----------------------------------------------------------------
    // Multi-window delivery — two REAL `NotificationCenterButton`s in
    // two REAL `WidgetTree`s sharing one archive, mirroring
    // `toast::host::tests::two_window_hosts`. Every test above builds
    // at most one tree/bell, so none of them can catch a bell in a
    // second window silently missing an archive mutation because the
    // first window's tree already consumed the shared version signal's
    // change notification — see `NotificationArchiveModel::version_signal`
    // and `bastyde_core::binding::BindingRegistry` for why one signal
    // can now serve every window.
    // -----------------------------------------------------------------

    /// Two independent windows (ids 1 and 2), each with its own
    /// `WidgetTree` + unscoped `NotificationCenterButton`, both bound
    /// to ONE shared archive.
    fn two_window_bells(archive: Rc<NotificationArchiveModel>) -> (WidgetTree, WidgetTree) {
        use bastyde_core::window::state::WindowStateInit;
        use bastyde_core::window::{WindowPlacement, WindowState};

        let build_window = |window_id: u64| {
            let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
            tree.set_window_state(WindowState::new(WindowStateInit {
                id: BastydeWindowId::new(window_id),
                string_id: Some(format!("w{window_id}")),
                placement: WindowPlacement::Floating,
                title: "Test".to_string(),
                size: (400, 600),
                position: (0, 0),
                focused: false,
                resizable: true,
                always_on_top: false,
            }));
            tree.add(NotificationCenterButton::new(archive.clone()));
            tree.layout(SizeProposal::exact(400.0, 600.0));
            tree
        };

        (build_window(1), build_window(2))
    }

    /// An archive push must update EVERY open window's bell badge —
    /// and must keep doing so regardless of which window's
    /// `WidgetTree` reconciles first, exactly like `WindowManager::
    /// request_redraw_needing_render` sweeping windows in whatever
    /// order its internal `HashMap` iterates them.
    #[test]
    fn both_unscoped_bells_pick_up_a_badge_change_regardless_of_reconcile_order() {
        let archive = Rc::new(NotificationArchiveModel::in_memory());
        let (mut tree1, mut tree2) = two_window_bells(archive.clone());
        assert!(tree1.find_by_label("1").is_none());
        assert!(tree2.find_by_label("1").is_none());

        archive.push(entry("new"));

        // Window 1 reconciles first.
        tree1.layout(SizeProposal::exact(400.0, 600.0));
        assert!(
            tree1.find_by_label("1").is_some(),
            "window 1's bell must show the new unread badge"
        );
        // Window 2 reconciles SECOND — this is exactly the case that
        // silently missed the badge update before per-window signals:
        // the shared flag was already cleared by window 1's flush.
        tree2.layout(SizeProposal::exact(400.0, 600.0));
        assert!(
            tree2.find_by_label("1").is_some(),
            "window 2's bell must ALSO show the badge, even reconciling second"
        );
    }

    /// Same scenario with the reconcile order flipped, to prove
    /// delivery genuinely doesn't depend on iteration order.
    #[test]
    fn both_unscoped_bells_pick_up_a_badge_change_in_the_reverse_reconcile_order_too() {
        let archive = Rc::new(NotificationArchiveModel::in_memory());
        let (mut tree1, mut tree2) = two_window_bells(archive.clone());

        archive.push(entry("new"));

        tree2.layout(SizeProposal::exact(400.0, 600.0));
        assert!(
            tree2.find_by_label("1").is_some(),
            "window 2's bell must show the badge when it reconciles first"
        );
        tree1.layout(SizeProposal::exact(400.0, 600.0));
        assert!(
            tree1.find_by_label("1").is_some(),
            "window 1's bell must ALSO show it, even reconciling second"
        );
    }

    #[test]
    fn tooltip_appears_on_hover() {
        let archive = Rc::new(NotificationArchiveModel::in_memory());
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(NotificationCenterButton::new(archive).tooltip(lit!("Tip")));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        tree.pointer_move(tree.bounds(id).center());
        tree.advance_time(std::time::Duration::from_secs(1));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "tooltip should appear on hover"
        );
        assert!(tree.find_by_label("Tip").is_some());
    }
}
