//! `NotificationLog` — the persistent-archive UI.
//!
//! Composition: `VStack { toolbar, ScrollArea { day_bucket_sections },
//! empty_state }` where
//! - toolbar is a horizontal row with mark-all-read + clear buttons;
//! - day-bucket sections are `[Today header, …rows, Yesterday
//!   header, …rows, This week header, …rows, Earlier header,
//!   …rows]` — entries grouped by their local-calendar bucket;
//! - each row is a [`StandardListItem`] (severity glyph leading,
//!   title + body subtitle, action buttons trailing). Unread rows
//!   render the title in `BodyBold`; read rows in `Body`;
//! - empty_state is shown only when the archive is empty.
//!
//! Apps embed the log directly (e.g. inside a sidebar panel), wrap
//! it in [`NotificationCenterButton`](super::center_button::NotificationCenterButton)
//! to get the bell-icon-with-popover pattern, or call
//! [`NotificationLogDialog::show`](super::log_dialog::NotificationLogDialog::show)
//! for a one-line modal presentation.

use bastyde_i18n::lit;
use std::rc::Rc;

use bastyde_canvas::{Canvas, Path, Point, Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::styles::BannerSeverity;
use bastyde_core::widget::{EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::WidgetBuilder;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{TextRole, TextStyleRole};

use crate::button::{Button, ButtonVariant};
use crate::link::Link;
use crate::notification::{
    ArchivedAction, ArchivedActionStyle, NotificationArchiveModel, NotificationEntry,
};
use crate::primitives::{HStack, Spacer, TextWidget, VStack};
use crate::scroll_area::ScrollArea;
use crate::standard_item::StandardListItem;
use bastyde_i18n::LocalizedString;

/// Configurable archive log. Shipped chrome:
/// - mark-all-read + clear buttons in a toolbar row;
/// - empty-state hint when the archive is empty;
/// - day-bucket section headers (Today / Yesterday / This week /
///   Earlier) above the rows for each bucket — computed against the
///   user's local timezone, recomputed on every archive mutation;
/// - [`StandardListItem`] rows with unread-as-bold differentiation.
///
/// A SearchField filter and a severity-chip filter can be composed by
/// apps using the existing widget toolkit.
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
        entry: &NotificationEntry,
        on_entry: Option<&Rc<dyn Fn(&NotificationEntry, &mut EventContext)>>,
        on_action: Option<&Rc<dyn Fn(&NotificationEntry, &ArchivedAction, &mut EventContext)>>,
    ) -> Box<dyn Widget> {
        let glyph: Box<dyn Widget> = Box::new(SeverityGlyph {
            severity: entry.severity,
            size: 14.0,
        });
        let mut row = StandardListItem::new(lit!(entry.title.clone()))
            .leading_slot_boxed(glyph)
            // Unread rows get a bold title (`BodyBold`); read rows
            // fall back to the StandardListItem default (`Body`).
            // This is the visual differentiation between "you
            // haven't seen this yet" and archived history.
            .label_style(if entry.read {
                TextStyleRole::Body
            } else {
                TextStyleRole::BodyBold
            });
        if let Some(body) = &entry.body {
            row = row.subtitle(lit!(body.clone()));
        }
        if !entry.actions.is_empty() {
            // Trailing action strip — Links inline, Buttons as buttons.
            // The trailing slot accepts a single widget; we wrap the
            // multiple actions in an HStack.
            let actions_row = build_actions_row(entry, on_action.cloned());
            row = row.trailing_slot_boxed(actions_row);
        }
        // Wire the body-click handler if requested.
        if let Some(cb) = on_entry {
            let cb = cb.clone();
            let entry_clone = entry.clone();
            Box::new(
                row.on_tap(move |_event, ctx| {
                    cb(&entry_clone, ctx);
                })
                .cursor(bastyde_core::widget::CursorIcon::Pointer),
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

        // Rebuild the log whenever the archive mutates (push,
        // in-place merge, mark_all_read, clear, remove). The
        // day-bucket headers must re-compute when entries appear /
        // disappear, and StandardListItem's read/unread title style
        // needs to flip on mark_all_read.
        archive.version_signal().bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Rebuild,
        );

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
                        Button::new(bastyde_i18n::tr_widget!(notifications_mark_all_read()))
                            .variant(ButtonVariant::Plain)
                            .on_activate_fn(move |_| archive_for_mark.mark_all_read()),
                    ),
                )
                .add_child(
                    ctx.add(
                        Button::new(bastyde_i18n::tr_widget!(notifications_clear()))
                            .variant(ButtonVariant::Plain)
                            .on_activate_fn(move |_| archive_for_clear.clear()),
                    ),
                );
            column = column.add_child(ctx.add(toolbar));
        }

        // Empty state or bucketed sections.
        if archive.entries().is_empty() {
            let empty = match self.empty_state.take() {
                Some(w) => ctx.add_boxed(w),
                None => ctx.add(
                    TextWidget::new(bastyde_i18n::tr_widget!(notifications_empty()))
                        .bind_color(TextRole::Secondary)
                        .style(TextStyleRole::Body),
                ),
            };
            column = column.add_child(empty);
        } else {
            // Snapshot entries + compute buckets relative to the
            // local "today". Bucket transitions across midnight are
            // recomputed on the next archive mutation (the log's
            // version-signal binding); a log that stays open across
            // midnight without any push will keep stale labels
            // until the user closes / reopens it — acceptable for a
            // popover-shaped UI.
            let model = archive.entries();
            let entries: Vec<NotificationEntry> = (0..model.len())
                .filter_map(|i| model.with_item(i, |e| e.clone()))
                .collect();
            let now = jiff::Zoned::now();
            let today = now.date();
            let zone = now.time_zone().clone();

            let mut sections = VStack::new().spacing(8.0);
            let mut current_bucket: Option<DayBucket> = None;
            for entry in &entries {
                let bucket = day_bucket_for(entry.timestamp, today, &zone);
                if Some(bucket) != current_bucket {
                    let header = TextWidget::new(bucket_label(bucket))
                        .style(TextStyleRole::SmallBold)
                        .bind_color(TextRole::Secondary);
                    sections = sections.add_child(ctx.add(header));
                    current_bucket = Some(bucket);
                }
                sections = sections.add_child(ctx.add_boxed(Self::build_row(
                    entry,
                    on_entry.as_ref(),
                    on_action.as_ref(),
                )));
            }
            // Wrap in ScrollArea so the dialog/popover scrolls when
            // the archive grows past the visible height.
            let scrollable = ScrollArea::new().child(sections);
            column = column.add_child(ctx.add(scrollable));
        }

        let root = ctx.add(column);
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
        builder.set_role(bastyde_core::accesskit::Role::List);
        builder.set_name(bastyde_i18n::tr_widget!(notifications_title()).resolve_now());
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
    on_action: Option<Rc<dyn Fn(&NotificationEntry, &ArchivedAction, &mut EventContext)>>,
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
                bastyde_i18n::tr_widget!(notifications_archive_replay_disabled()).resolve_now()
            );
            row = row.child(
                TextWidget::new(lit!(label))
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
                row.child(Link::new(lit!(action.label.clone())).on_activate_fn(activate))
            }
            ArchivedActionStyle::PrimaryButton => row.child(
                Button::new(lit!(action.label.clone()))
                    .variant(ButtonVariant::Filled)
                    .on_activate_fn(activate),
            ),
            ArchivedActionStyle::SecondaryButton => row.child(
                Button::new(lit!(action.label.clone()))
                    .variant(ButtonVariant::Plain)
                    .on_activate_fn(activate),
            ),
            ArchivedActionStyle::Destructive => row.child(
                Button::new(lit!(action.label.clone()))
                    .variant(ButtonVariant::Destructive)
                    .on_activate_fn(activate),
            ),
        };
    }
    Box::new(row)
}

// ------------------------------------------------------------------
// Day-bucket section headers
// ------------------------------------------------------------------

/// Coarse time bucket — drives the section header that appears
/// above the first entry of each bucket in the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DayBucket {
    /// Same local-calendar date as `today`.
    Today,
    /// `today - 1` day.
    Yesterday,
    /// 2..=6 days ago (within the same calendar week conceptually).
    ThisWeek,
    /// 7 or more days ago.
    Earlier,
}

/// Compute the bucket for an archive entry. Uses the user's local
/// timezone to map the entry's UTC timestamp onto a calendar date,
/// then compares against `today` (also in the local timezone).
///
/// Two entries that landed within the same local-calendar date both
/// get `Today`, regardless of the UTC hours between them — that's
/// the user-facing notion of "today".
fn day_bucket_for(
    timestamp: jiff::Timestamp,
    today: jiff::civil::Date,
    zone: &jiff::tz::TimeZone,
) -> DayBucket {
    let entry_date = timestamp.to_zoned(zone.clone()).date();
    // Compare via day delta. `today - entry_date` returns a Span;
    // we extract the day count. Future entries (clock skew, sync
    // from a peer) bucket as Today so they don't slip into Earlier
    // by accident.
    let delta_days = today
        .since(entry_date)
        .map(|span| span.get_days())
        .unwrap_or(0);
    if delta_days <= 0 {
        DayBucket::Today
    } else if delta_days == 1 {
        DayBucket::Yesterday
    } else if delta_days <= 6 {
        DayBucket::ThisWeek
    } else {
        DayBucket::Earlier
    }
}

fn bucket_label(bucket: DayBucket) -> LocalizedString {
    match bucket {
        DayBucket::Today => bastyde_i18n::tr_widget!(notifications_bucket_today()),
        DayBucket::Yesterday => bastyde_i18n::tr_widget!(notifications_bucket_yesterday()),
        DayBucket::ThisWeek => bastyde_i18n::tr_widget!(notifications_bucket_this_week()),
        DayBucket::Earlier => bastyde_i18n::tr_widget!(notifications_bucket_earlier()),
    }
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
    ) -> bastyde_core::widget::LayoutResponse {
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
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
        builder.set_hidden();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notification::{ArchivedActionStyle, NotificationArchiveModel};
    use bastyde_core::styles::{BannerSeverity, ToastPriority};
    use bastyde_core::widget_tree::WidgetTree;

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
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(log);
        tree.layout(SizeProposal::exact(480.0, 360.0));
        tree
    }

    #[test]
    fn empty_archive_renders_empty_state() {
        let archive = fresh_archive();
        let tree = tree_with(NotificationLog::new(archive));
        // Empty-state text is the localized "No notifications".
        let expected = bastyde_i18n::tr_widget!(notifications_empty()).resolve_now();
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
        let list_role = tree.find_by_role(bastyde_core::accesskit::Role::List);
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
        // The inert tag's label is "Retry" + the localized
        // "(no longer available)" suffix; an exact lookup of just
        // "Retry" misses — that's the contract.
        assert!(
            tree.find_by_label("Retry").is_none(),
            "without on_action_invoked, archive actions render as inert text tags with a \
             suffix — no exact-'Retry' label appears"
        );
    }

    // ----- Day-bucket helper -----

    fn ts(year: i16, month: i8, day: i8, hour: i8, minute: i8) -> jiff::Timestamp {
        let utc_zone = jiff::tz::TimeZone::UTC;
        jiff::civil::DateTime::new(year, month, day, hour, minute, 0, 0)
            .unwrap()
            .to_zoned(utc_zone)
            .unwrap()
            .timestamp()
    }

    #[test]
    fn day_bucket_today_for_same_calendar_date() {
        let today = jiff::civil::Date::new(2025, 5, 17).unwrap();
        // Same date, different hour → Today.
        let entry = ts(2025, 5, 17, 8, 30);
        assert_eq!(
            day_bucket_for(entry, today, &jiff::tz::TimeZone::UTC),
            DayBucket::Today
        );
    }

    #[test]
    fn day_bucket_yesterday_for_t_minus_one() {
        let today = jiff::civil::Date::new(2025, 5, 17).unwrap();
        let entry = ts(2025, 5, 16, 23, 0);
        assert_eq!(
            day_bucket_for(entry, today, &jiff::tz::TimeZone::UTC),
            DayBucket::Yesterday
        );
    }

    #[test]
    fn day_bucket_this_week_for_2_to_6_days_ago() {
        let today = jiff::civil::Date::new(2025, 5, 17).unwrap();
        for days_ago in 2..=6 {
            let date = today
                .checked_sub(jiff::ToSpan::days(days_ago as i64))
                .unwrap();
            let entry = ts(date.year(), date.month(), date.day(), 12, 0);
            assert_eq!(
                day_bucket_for(entry, today, &jiff::tz::TimeZone::UTC),
                DayBucket::ThisWeek,
                "{days_ago} days ago must bucket as ThisWeek"
            );
        }
    }

    #[test]
    fn day_bucket_earlier_for_7_plus_days_ago() {
        let today = jiff::civil::Date::new(2025, 5, 17).unwrap();
        let week_ago_date = today.checked_sub(jiff::ToSpan::days(7)).unwrap();
        let entry = ts(
            week_ago_date.year(),
            week_ago_date.month(),
            week_ago_date.day(),
            12,
            0,
        );
        assert_eq!(
            day_bucket_for(entry, today, &jiff::tz::TimeZone::UTC),
            DayBucket::Earlier
        );
    }

    #[test]
    fn day_bucket_future_entries_count_as_today() {
        // Clock skew or peer sync — an entry stamped in the future
        // (delta_days < 0) should be considered Today, not Earlier.
        let today = jiff::civil::Date::new(2025, 5, 17).unwrap();
        let entry = ts(2025, 5, 18, 0, 0);
        assert_eq!(
            day_bucket_for(entry, today, &jiff::tz::TimeZone::UTC),
            DayBucket::Today
        );
    }

    #[test]
    fn day_bucket_label_resolves_through_i18n() {
        // Sanity-check that each variant maps to a non-empty
        // localized string (the en-US source bundle is present in
        // test runs).
        for bucket in [
            DayBucket::Today,
            DayBucket::Yesterday,
            DayBucket::ThisWeek,
            DayBucket::Earlier,
        ] {
            let label = bucket_label(bucket).resolve_now();
            assert!(!label.is_empty(), "{bucket:?} has an empty label");
        }
    }

    #[test]
    fn log_with_entries_across_buckets_renders_each_header() {
        // Push entries with timestamps in different buckets and
        // confirm each bucket header text appears in the AT tree.
        let archive = fresh_archive();
        let now = jiff::Zoned::now();
        let today = now.date();
        let zone = now.time_zone().clone();
        let today_ts = today
            .at(12, 0, 0, 0)
            .to_zoned(zone.clone())
            .unwrap()
            .timestamp();
        let yesterday_ts = today
            .checked_sub(jiff::ToSpan::days(1))
            .unwrap()
            .at(12, 0, 0, 0)
            .to_zoned(zone.clone())
            .unwrap()
            .timestamp();
        let earlier_ts = today
            .checked_sub(jiff::ToSpan::days(30))
            .unwrap()
            .at(12, 0, 0, 0)
            .to_zoned(zone)
            .unwrap()
            .timestamp();

        // Push oldest first so the newest (Today) ends up at index 0.
        let mut earlier = entry("Very old notice", None, Vec::new());
        earlier.timestamp = earlier_ts;
        archive.push(earlier);
        let mut yesterday = entry("Yesterday's notice", None, Vec::new());
        yesterday.timestamp = yesterday_ts;
        archive.push(yesterday);
        let mut today_entry = entry("Today's notice", None, Vec::new());
        today_entry.timestamp = today_ts;
        archive.push(today_entry);

        let tree = tree_with(NotificationLog::new(archive));

        let today_label = bastyde_i18n::tr_widget!(notifications_bucket_today()).resolve_now();
        let yesterday_label =
            bastyde_i18n::tr_widget!(notifications_bucket_yesterday()).resolve_now();
        let earlier_label = bastyde_i18n::tr_widget!(notifications_bucket_earlier()).resolve_now();

        assert!(
            tree.find_by_label(&today_label).is_some(),
            "Today header must appear"
        );
        assert!(
            tree.find_by_label(&yesterday_label).is_some(),
            "Yesterday header must appear"
        );
        assert!(
            tree.find_by_label(&earlier_label).is_some(),
            "Earlier header must appear"
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
        tree.dispatch_event(bastyde_core::event::WidgetEvent::AccessAction {
            action: bastyde_core::accesskit::Action::Click,
            target: Some(btn),
            target_node: bastyde_core::accessibility::root_node_id(),
            data: None,
        });
        assert!(
            fired.get(),
            "on_action_invoked callback fires on Retry click"
        );
    }
}
