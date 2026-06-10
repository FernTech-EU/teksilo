//! `NotificationCenterButton` — bell icon with unread-count badge,
//! opening the notification log as a popover.
//!
//! Composition: `ZStack { popover_icon_button(bell), badge }` where
//! the popover content is a [`NotificationLog`]. On popover open the
//! archive is `mark_all_read`-ed so the badge resets — matches the
//! convention (GitHub, Slack, JetBrains).
//!
//! Most apps mount this in their `StatusBar` or `TitleBar`. The
//! button is one widget; the popover behaviour is fully self-managed.

use bastyde_i18n::lit;
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
use crate::notification::{ArchivedAction, NotificationArchiveModel, NotificationEntry};
use crate::popover_widget::PopoverIconButton;
use crate::primitives::ZStack;

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
        }
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

        // Bind to the unread count at Rebuild so any push/dismiss
        // rebuilds the button (Badge has no signal-bound label
        // surface — we re-create it on each rebuild with the current
        // count baked in).
        archive.unread_count().bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Rebuild,
        );

        // Bell trigger: an IconButton(bell) at the requested size.
        let trigger = IconButton::bell().size(self.size);

        // Popover content: a NotificationLog. The on_action_invoked
        // hook is forwarded if present.
        let mut log = NotificationLog::new(archive.clone());
        if let Some(cb) = self.on_action_invoked.clone() {
            log = log.on_action_invoked(move |e, a, ctx| cb(e, a, ctx));
        }

        // `PopoverIconButton` wraps the content in the themed popover
        // surface (background, border, padding, shadow) by default, so
        // the chrome-less `NotificationLog` gets a proper surface for
        // free — no manual `Panel` needed.

        // Bell + popover combo. Mark archive entries read when the
        // popover *closes*, NOT when it opens. `mark_all_read` bumps
        // the unread-count signal, which fires this widget's `Rebuild`
        // binding — and a rebuild on OPEN would tear down the
        // `PopoverIconButton` (and its just-shown overlay) and replace
        // it with a fresh, closed one, so the popover would flash and
        // vanish, leaving only the cleared badge. Deferring to close
        // lets the rebuild happen after the popover is already gone.
        let archive_for_close = archive.clone();
        let pib = PopoverIconButton::new(trigger)
            .content(log)
            .placement(self.placement.clone())
            .dismiss_behavior(DismissBehavior::EscapeOrClickOutside)
            .on_close(move || archive_for_close.mark_all_read());
        let pib_id = ctx.add(pib);

        // Compute the badge label for this build.
        let unread_count = archive.unread_count().get();
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
        let spacer = tree.add(FixedSize::new().bind_height(500.0).child(Spacer::new()));
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
        let spacer = tree.add(FixedSize::new().bind_height(500.0).child(Spacer::new()));
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
}
