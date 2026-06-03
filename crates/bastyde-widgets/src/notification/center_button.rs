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

        // Bell + popover combo. On open, mark archive entries read so
        // the badge resets — bumps the unread_count signal, which
        // re-rebuilds this widget and removes the badge.
        let archive_for_open = archive.clone();
        let pib = PopoverIconButton::new(trigger)
            .content(log)
            .placement(self.placement.clone())
            .dismiss_behavior(DismissBehavior::EscapeOrClickOutside)
            .on_open(move || archive_for_open.mark_all_read());
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
        let mut stack = ZStack::new().add_child(pib_id);
        if unread_count > 0 || show_when_zero {
            let badge_id = ctx.add(Badge::new(lit!(label)));
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
