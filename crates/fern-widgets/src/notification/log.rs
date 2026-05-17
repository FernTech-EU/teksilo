//! `NotificationLog` — the persistent-archive UI.
//!
//! Composition: `VStack { toolbar, list_view, empty_state }` where
//! - toolbar is a horizontal row with mark-all-read + clear buttons,
//! - list_view is a [`ListView<NotificationEntry>`] bound to the
//!   archive's entries (newest first, severity glyph as leading,
//!   title + body as subtitle, action buttons trailing),
//! - empty_state is a centered hint shown only when the archive is
//!   empty.
//!
//! Apps embed the log directly (e.g. inside a sidebar panel), wrap
//! it in [`NotificationCenterButton`](super::center_button::NotificationCenterButton)
//! to get the bell-icon-with-popover pattern, or call
//! [`NotificationLogDialog::show`](super::log_dialog::NotificationLogDialog::show)
//! for a one-line modal presentation.

use std::rc::Rc;

use fern_canvas::{Canvas, Path, Point, Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::styles::BannerSeverity;
use fern_core::widget::{EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_builder::WidgetBuilder;
use fern_core::widget_id::WidgetId;
use fern_tokens::{TextRole, TextStyleRole};

use crate::button::{Button, ButtonVariant};
use crate::link::Link;
use crate::list_view::ListView;
use crate::notification::{
    ArchivedAction, ArchivedActionStyle, NotificationArchiveModel, NotificationEntry,
};
use crate::primitives::{HStack, Spacer, TextWidget, VStack};
use crate::standard_item::StandardListItem;

/// Configurable archive log. Shipped chrome:
/// - mark-all-read + clear buttons in a toolbar row;
/// - empty-state hint when the archive is empty;
/// - list of [`StandardListItem`] rows, one per [`NotificationEntry`].
///
/// Phase 4 ships the core list + replay UX. Day-bucket section
/// headers (Today / Yesterday / This week / Earlier) and a
/// SearchField-driven filter (`rich-text` feature) are documented in
/// the plan and will land in Phase 4 refinements.
pub struct NotificationLog {
    archive: Rc<NotificationArchiveModel>,
    show_toolbar: bool,
    empty_state: Option<Box<dyn Widget>>,
    on_entry_invoked: Option<Rc<dyn Fn(&NotificationEntry, &mut EventContext)>>,
    on_action_invoked: Option<Rc<dyn Fn(&NotificationEntry, &ArchivedAction, &mut EventContext)>>,
    root_child_id: Option<WidgetId>,
}

impl NotificationLog {
    /// Construct a log bound to the shared archive. The archive is
    /// expected to outlive the log (typically held in `app_state`).
    pub fn new(archive: Rc<NotificationArchiveModel>) -> Self {
        Self {
            archive,
            show_toolbar: true,
            empty_state: None,
            on_entry_invoked: None,
            on_action_invoked: None,
            root_child_id: None,
        }
    }

    /// Whether to render the toolbar row (mark-all-read + clear).
    /// Default `true`. Apps that want a chrome-less log (e.g. inside
    /// a custom panel that supplies its own toolbar) pass `false`.
    pub fn show_toolbar(mut self, show: bool) -> Self {
        self.show_toolbar = show;
        self
    }

    /// Override the empty-state hint widget. Default: a centered
    /// "No notifications" text. Pass any widget for a custom empty
    /// view (illustration, call-to-action, …).
    pub fn empty_state(mut self, widget: impl Widget + 'static) -> Self {
        self.empty_state = Some(Box::new(widget));
        self
    }

    /// Called when the user clicks anywhere on an archived entry's
    /// row body (outside any specific action button). The default
    /// behaviour is no-op — the log is read-only display unless
    /// callers wire this hook.
    pub fn on_entry_invoked(
        mut self,
        f: impl Fn(&NotificationEntry, &mut EventContext) + 'static,
    ) -> Self {
        self.on_entry_invoked = Some(Rc::new(f));
        self
    }

    /// Called when an archived action button is clicked. Apps wire
    /// this hook to replay the action — typically by mapping the
    /// `ArchivedAction::intent_name` to one of the app's registered
    /// `Action`s via `ctx.send_intent(...)`. Without this hook
    /// configured the action buttons are inert (the log keeps them
    /// visible for archival context).
    ///
    /// Actions without an `intent_name` render as non-clickable
    /// past-action tags regardless of this hook — there's nothing
    /// for the framework to dispatch against once the live closure
    /// has torn down.
    ///
    /// ```ignore
    /// log.on_action_invoked(|_entry, action, ctx| {
    ///     // Bridge the dynamic intent_name to one of the app's
    ///     // typed AppIntent variants:
    ///     match action.intent_name.as_deref() {
    ///         Some("app.build.retry") => ctx.send_intent(AppIntent::BuildRetry),
    ///         Some(name) => log::warn!("unknown archived intent: {name}"),
    ///         None => {}
    ///     }
    /// })
    /// ```
    pub fn on_action_invoked(
        mut self,
        f: impl Fn(&NotificationEntry, &ArchivedAction, &mut EventContext) + 'static,
    ) -> Self {
        self.on_action_invoked = Some(Rc::new(f));
        self
    }

    fn build_row(
        archive: &Rc<NotificationArchiveModel>,
        entry: &NotificationEntry,
        on_entry: Option<&Rc<dyn Fn(&NotificationEntry, &mut EventContext)>>,
        on_action: Option<&Rc<dyn Fn(&NotificationEntry, &ArchivedAction, &mut EventContext)>>,
    ) -> Box<dyn Widget> {
        let glyph: Box<dyn Widget> = Box::new(SeverityGlyph {
            severity: entry.severity,
            size: 14.0,
        });
        let mut row = StandardListItem::new_literal(entry.title.clone()).leading_slot_boxed(glyph);
        if let Some(body) = &entry.body {
            row = row.subtitle_literal(body.clone());
        }
        if !entry.actions.is_empty() {
            // Trailing action strip — Links inline, Buttons as buttons.
            // The trailing slot accepts a single widget; we wrap the
            // multiple actions in an HStack.
            let entry_clone_for_actions = entry.clone();
            let archive_clone = archive.clone();
            let on_action_clone = on_action.cloned();
            let on_entry_for_actions = on_entry.cloned();
            let actions_row = build_actions_row(
                &entry_clone_for_actions,
                archive_clone,
                on_action_clone,
                on_entry_for_actions,
            );
            row = row.trailing_slot_boxed(actions_row);
        }
        // Unread rows render with a slight bold-title accent; we
        // approximate that via a subtitle of "•" prefix on the title
        // only when unread. For Phase 4 MVP we use the entry's `read`
        // bit informationally only (the visual differentiation lands
        // with a future styling pass that exposes a "row variant" on
        // StandardListItem).
        let _ = entry.read;
        // Wire the body-click handler if requested.
        if let Some(cb) = on_entry {
            let cb = cb.clone();
            let entry_clone = entry.clone();
            Box::new(
                row.on_tap(move |_event, ctx| {
                    cb(&entry_clone, ctx);
                })
                .cursor(fern_core::widget::CursorIcon::Pointer),
            )
        } else {
            Box::new(row)
        }
    }
}

impl std::fmt::Debug for NotificationLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NotificationLog")
            .field("archive_entries", &self.archive.entries().len())
            .field("show_toolbar", &self.show_toolbar)
            .field("has_empty_state", &self.empty_state.is_some())
            .finish_non_exhaustive()
    }
}

impl Widget for NotificationLog {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let archive = self.archive.clone();
        let on_entry = self.on_entry_invoked.clone();
        let on_action = self.on_action_invoked.clone();

        let mut column = VStack::new().spacing(6.0);

        // Toolbar row.
        if self.show_toolbar {
            let archive_for_mark = archive.clone();
            let archive_for_clear = archive.clone();
            let toolbar = HStack::new()
                .spacing(8.0)
                .add_child(ctx.add(Spacer::new()))
                .add_child(
                    ctx.add(
                        Button::new(fern_i18n::tr_widget!(notifications_mark_all_read()))
                            .variant(ButtonVariant::Plain)
                            .on_activate_fn(move |_| archive_for_mark.mark_all_read()),
                    ),
                )
                .add_child(
                    ctx.add(
                        Button::new(fern_i18n::tr_widget!(notifications_clear()))
                            .variant(ButtonVariant::Plain)
                            .on_activate_fn(move |_| archive_for_clear.clear()),
                    ),
                );
            column = column.add_child(ctx.add(toolbar));
        }

        // Empty state or list view.
        if archive.entries().is_empty() {
            let empty = match self.empty_state.take() {
                Some(w) => ctx.add_boxed(w),
                None => ctx.add(
                    TextWidget::new(fern_i18n::tr_widget!(notifications_empty()))
                        .bind_color(TextRole::Secondary)
                        .style(TextStyleRole::Body),
                ),
            };
            column = column.add_child(empty);
        } else {
            let model = archive.entries().clone();
            let archive_for_delegate = archive.clone();
            let list = ListView::new(model, move |_idx, entry, _selected| {
                Self::build_row(
                    &archive_for_delegate,
                    entry,
                    on_entry.as_ref(),
                    on_action.as_ref(),
                )
            })
            .item_height(64.0);
            column = column.add_child(ctx.add(list));
        }

        let root = ctx.add(column);
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(360.0, 240.0))
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
        builder.set_role(fern_core::accesskit::Role::List);
        builder.set_name(fern_i18n::tr_widget!(notifications_title()).resolve_now());
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

/// Build the trailing-slot HStack of action widgets for one entry.
/// `intent_name`-bearing actions become clickable Link / Button
/// widgets that invoke the caller-supplied `on_action_invoked` hook.
/// Actions without `intent_name` (or without the hook set) render
/// as disabled descriptive labels — the log keeps the action
/// visible for archival context but the closure that powered the
/// live toast is long gone.
fn build_actions_row(
    entry: &NotificationEntry,
    _archive: Rc<NotificationArchiveModel>,
    on_action: Option<Rc<dyn Fn(&NotificationEntry, &ArchivedAction, &mut EventContext)>>,
    _on_entry: Option<Rc<dyn Fn(&NotificationEntry, &mut EventContext)>>,
) -> Box<dyn Widget> {
    let mut row = HStack::new().spacing(8.0);
    for action in entry.actions.iter() {
        let action_owned = action.clone();
        let entry_owned = entry.clone();
        let on_action_for_handler = on_action.clone();
        let clickable = action.intent_name.is_some() && on_action_for_handler.is_some();

        if !clickable {
            // Non-clickable: descriptive tag.
            let label = format!(
                "{} {}",
                action.label,
                fern_i18n::tr_widget!(notifications_archive_replay_disabled()).resolve_now()
            );
            row = row.child(
                TextWidget::new_literal(label)
                    .style(TextStyleRole::Small)
                    .bind_color(TextRole::Secondary),
            );
            continue;
        }

        let activate = move |ctx: &mut EventContext| {
            if let Some(cb) = on_action_for_handler.as_ref() {
                cb(&entry_owned, &action_owned, ctx);
            }
        };
        row = match action.style {
            ArchivedActionStyle::Link => {
                row.child(Link::new_literal(action.label.clone()).on_activate_fn(activate))
            }
            ArchivedActionStyle::PrimaryButton => row.child(
                Button::new_literal(action.label.clone())
                    .variant(ButtonVariant::Filled)
                    .on_activate_fn(activate),
            ),
            ArchivedActionStyle::SecondaryButton => row.child(
                Button::new_literal(action.label.clone())
                    .variant(ButtonVariant::Plain)
                    .on_activate_fn(activate),
            ),
            ArchivedActionStyle::Destructive => row.child(
                Button::new_literal(action.label.clone())
                    .variant(ButtonVariant::Destructive)
                    .on_activate_fn(activate),
            ),
        };
    }
    Box::new(row)
}

// ------------------------------------------------------------------
// SeverityGlyph (mirrors the one inside ToastSurface; kept duplicated
// because both widgets sit at the same crate tier and live in
// different submodules without a shared `glyph` parent module).
// ------------------------------------------------------------------

/// Tiny leaf that paints the severity glyph for an archive row.
/// Smaller than the toast's 16 dp glyph — the row context needs
/// less visual weight.
struct SeverityGlyph {
    severity: BannerSeverity,
    size: f32,
}

impl std::fmt::Debug for SeverityGlyph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SeverityGlyph")
            .field("severity", &self.severity)
            .finish()
    }
}

impl Widget for SeverityGlyph {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        proposal.resolve(self.size, self.size).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let color = self.severity.glyph_color(ctx.theme);
        let cx = bounds.x + bounds.width / 2.0;
        let cy = bounds.y + bounds.height / 2.0;
        let half = (bounds.width.min(bounds.height) / 2.0).max(2.0);
        let path = match self.severity {
            BannerSeverity::Warning => {
                let mut p = Path::new();
                p.move_to(Point::new(cx, cy - half));
                p.line_to(Point::new(cx + half, cy + half));
                p.line_to(Point::new(cx - half, cy + half));
                p.close();
                p
            }
            _ => Path::circle(Point::new(cx, cy), half),
        };
        canvas.fill_path(&path, color);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::GenericContainer);
        builder.set_hidden();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notification::{ArchivedActionStyle, NotificationArchiveModel};
    use fern_core::styles::{BannerSeverity, ToastPriority};
    use fern_core::widget_tree::WidgetTree;

    fn entry(title: &str, body: Option<&str>, actions: Vec<ArchivedAction>) -> NotificationEntry {
        NotificationEntry {
            id: 0,
            severity: BannerSeverity::Info,
            priority: ToastPriority::Normal,
            title: title.to_string(),
            body: body.map(|s| s.to_string()),
            actions,
            timestamp: jiff::Timestamp::UNIX_EPOCH,
            group: None,
            source: None,
            read: false,
            dedup_id: None,
            updates: Vec::new(),
        }
    }

    fn fresh_archive() -> Rc<NotificationArchiveModel> {
        Rc::new(NotificationArchiveModel::in_memory())
    }

    fn tree_with(log: NotificationLog) -> WidgetTree {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        tree.add(log);
        tree.layout(SizeProposal::exact(480.0, 360.0));
        tree
    }

    #[test]
    fn empty_archive_renders_empty_state() {
        let archive = fresh_archive();
        let tree = tree_with(NotificationLog::new(archive));
        // Empty-state text is the localized "No notifications".
        let expected = fern_i18n::tr_widget!(notifications_empty()).resolve_now();
        assert!(
            tree.find_by_label(&expected).is_some(),
            "empty-state hint must be in the AT tree when the archive is empty"
        );
    }

    #[test]
    fn populated_archive_renders_list_role() {
        let archive = fresh_archive();
        archive.push(entry("first", Some("body 1"), Vec::new()));
        archive.push(entry("second", None, Vec::new()));
        let tree = tree_with(NotificationLog::new(archive));
        // Log's own root carries Role::List.
        let list_role = tree.find_by_role(fern_core::accesskit::Role::List);
        assert!(list_role.is_some(), "Log root exposes Role::List");
    }

    #[test]
    fn intent_action_without_callback_is_inert() {
        // An ArchivedAction with intent_name + no on_action_invoked
        // hook installed → renders as a non-clickable tag (no
        // Button widget appears for that action).
        let archive = fresh_archive();
        archive.push(entry(
            "Build failed",
            None,
            vec![ArchivedAction {
                label: "Retry".into(),
                intent_name: Some("app.build.retry".into()),
                style: ArchivedActionStyle::PrimaryButton,
                closes_on_invoke: true,
            }],
        ));
        let tree = tree_with(NotificationLog::new(archive));
        // No clickable "Retry" — the action renders as a Small
        // TextWidget with the disabled-suffix appended. The label
        // (with the suffix) IS in the AT tree as a text node, but
        // there's no Role::Button "Retry".
        let buttons: Vec<_> = (0..1000)
            .filter_map(|_| tree.find_by_label("Retry"))
            .collect();
        // find_by_label looks for an EXACT match; the tag includes
        // " (no longer available)" so a plain "Retry" lookup misses.
        assert!(
            buttons.is_empty(),
            "no exact-'Retry' label without the on_action hook"
        );
    }

    #[test]
    fn intent_action_with_callback_fires_on_click() {
        // The cleanest path to verify the callback fires is at the
        // widget-builder level — we can't easily simulate a real
        // click without a fully-wired dispatcher. The test here
        // confirms the action's intent_name + on_action_invoked
        // combo is captured correctly.
        use std::cell::Cell;
        let archive = fresh_archive();
        archive.push(entry(
            "Build failed",
            None,
            vec![ArchivedAction {
                label: "Retry".into(),
                intent_name: Some("app.build.retry".into()),
                style: ArchivedActionStyle::PrimaryButton,
                closes_on_invoke: true,
            }],
        ));
        let fired = Rc::new(Cell::new(false));
        let fired_clone = fired.clone();
        let log = NotificationLog::new(archive).on_action_invoked(move |_entry, action, _ctx| {
            assert_eq!(action.intent_name.as_deref(), Some("app.build.retry"));
            fired_clone.set(true);
        });
        let mut tree = tree_with(log);
        // Find the Retry button via its label + Role::Button.
        let btn = tree
            .find_by_label("Retry")
            .expect("Retry button must be in the AT tree when on_action_invoked is wired");
        tree.dispatch_event(fern_core::event::WidgetEvent::AccessAction {
            action: fern_core::accesskit::Action::Click,
            target: Some(btn),
            target_node: fern_core::accessibility::root_node_id(),
            data: None,
        });
        assert!(
            fired.get(),
            "on_action_invoked callback fires on Retry click"
        );
    }
}
