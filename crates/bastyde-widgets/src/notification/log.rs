// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `NotificationLog` — a scrollable, day-bucketed list of archived notifications.
//!
//! Renders a [`NotificationArchiveModel`] as a scrollable column of
//! [`StandardListItem`] rows grouped under section headers (Today /
//! Yesterday / This week / Earlier), computed against the user's local
//! timezone on every archive mutation. An optional toolbar row provides
//! mark-all-read and clear buttons. Unread rows show the title in
//! `BodyBold`; read rows use `Body`. An empty-state hint is shown when the
//! archive is empty.
//!
//! ## Sizing
//!
//! The log grows into a host that bounds its height and compresses
//! inside one shorter than its natural height (floored at one row);
//! only a host that hugs its content — which is how the overlay layer
//! measures the [`NotificationCenterButton`](super::center_button::NotificationCenterButton)
//! popover — falls back to [`preferred_width`](NotificationLog::preferred_width) /
//! [`preferred_height`](NotificationLog::preferred_height). Row text is
//! **elided**, not wrapped, with the full text on the row's rich
//! tooltip: notification prose is arbitrary and the log does not
//! control its own width, so a wrapping row would over-constrain
//! itself and push its trailing action buttons out of view.
//!
//! ## When to use
//!
//! - Embed directly inside a side panel or settings page for an in-app
//!   notification centre.
//! - Wrap in [`NotificationCenterButton`](super::center_button::NotificationCenterButton)
//!   for the standard bell-icon-with-popover pattern.
//! - Call [`NotificationLogDialog::show`](super::log_dialog::NotificationLogDialog::show)
//!   for a one-line modal presentation.
//!
//! ```ignore
//! let archive: Rc<NotificationArchiveModel> = ctx.app_state().unwrap();
//! let log = NotificationLog::new(archive)
//!     .on_action_invoked(|_entry, action, ctx| {
//!         if let Some(name) = &action.intent_name {
//!             ctx.send_intent(bastyde_core::Intent::new(name));
//!         }
//!     });
//! ```

use bastyde_i18n::lit;
use std::rc::Rc;

use bastyde_canvas::{EllipsisMode, Rect, SizeProposal, TextOverflow};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::widget::{EventContext, LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::WidgetBuilder;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{TextRole, TextStyleRole};

use crate::button::{Button, ButtonVariant};
use crate::link::Link;
use crate::notification::{
    ArchivedAction, ArchivedActionStyle, NotificationArchiveModel, NotificationEntry, route_visible,
};
use crate::primitives::{Center, Expand, HStack, Padding, Shrinkable, Spacer, TextWidget, VStack};
use crate::scroll_area::ScrollArea;
use crate::severity_badge::SeverityBadge;
use crate::standard_item::StandardListItem;
use crate::styles::recipe_standard_item_style as si;
use crate::toast::{ToastAudience, ToastRoute};
use crate::tooltip::TooltipContent;
use bastyde_core::window::BastydeWindowId;
use bastyde_i18n::LocalizedString;

/// Width the log reports when its host proposes an unbounded one.
/// Wide enough for a severity glyph, a two-line entry and a trailing
/// action button without eliding the title to a stub.
const DEFAULT_PREFERRED_WIDTH: f32 = 380.0;

/// Height of the scrolling list area when the host proposes an
/// unbounded height (roughly seven two-line rows).
const DEFAULT_PREFERRED_HEIGHT: f32 = 320.0;

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
    /// Factory, not a stored widget: `build` re-runs on every archive
    /// mutation (the `version_signal` binding below), and a
    /// `Box<dyn Widget>` can only be consumed once — the custom empty
    /// state would vanish the first time the archive went non-empty
    /// and never come back on the next `clear()`. Same shape as
    /// [`GridView::empty_view`](crate::grid_view::GridView::empty_view).
    empty_state: Option<Rc<dyn Fn() -> Box<dyn Widget>>>,
    on_entry_invoked: Option<Rc<dyn Fn(&NotificationEntry, &mut EventContext)>>,
    on_action_invoked: Option<Rc<dyn Fn(&NotificationEntry, &ArchivedAction, &mut EventContext)>>,
    root_child_id: Option<WidgetId>,
    /// Width used when the host proposes an unbounded one — i.e. when
    /// it hugs its content, which is exactly what the overlay layer
    /// does to size a popover. See [`Self::preferred_width`].
    preferred_width: f32,
    /// Height of the scrolling list area when the host proposes an
    /// unbounded height. See [`Self::preferred_height`].
    preferred_height: f32,
    /// `None` (default) = unscoped — every entry is shown, matching
    /// this widget's behaviour before routing existed. `Some(route)`
    /// restricts the rendered rows AND the toolbar's mark-all-read /
    /// clear actions to entries matching `route` (plus `Broadcast`,
    /// always visible). Set via [`Self::for_window`] / [`Self::for_audience`].
    route_scope: Option<ToastRoute>,
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
            preferred_width: DEFAULT_PREFERRED_WIDTH,
            preferred_height: DEFAULT_PREFERRED_HEIGHT,
            route_scope: None,
        }
    }

    /// Scope this log to entries routed to window `window_id` (plus
    /// any `Broadcast` entry) — the shape a `NotificationCenterButton`
    /// mounted in that window wants for its popover body. Overrides
    /// any previous `for_window` / `for_audience` call.
    pub fn for_window(mut self, window_id: BastydeWindowId) -> Self {
        self.route_scope = Some(ToastRoute::Window(window_id));
        self
    }

    /// Scope this log to entries routed to `audience` (plus any
    /// `Broadcast` entry). Overrides any previous `for_window` /
    /// `for_audience` call.
    pub fn for_audience(mut self, audience: ToastAudience) -> Self {
        self.route_scope = Some(ToastRoute::Audience(audience));
        self
    }

    /// Whether to render the toolbar row (mark-all-read + clear).
    /// Default `true`. Apps that want a chrome-less log (e.g. inside
    /// a custom panel that supplies its own toolbar) pass `false`.
    pub fn show_toolbar(mut self, show: bool) -> Self {
        self.show_toolbar = show;
        self
    }

    /// Override the empty-state hint. Default: a centered
    /// "No notifications" text. Pass a factory returning any widget
    /// for a custom empty view (illustration, call-to-action, …).
    ///
    /// A factory rather than a widget because the log rebuilds on
    /// every archive mutation: the view has to be re-creatable each
    /// time the archive goes empty again, not just the first time.
    ///
    /// ```ignore
    /// log.empty_state(|| Box::new(TextWidget::new(tr!(inbox_zero()))))
    /// ```
    pub fn empty_state(mut self, f: impl Fn() -> Box<dyn Widget> + 'static) -> Self {
        self.empty_state = Some(Rc::new(f));
        self
    }

    /// Width the log reports when the host proposes an unbounded one.
    /// Default `380` dp.
    ///
    /// This is load-bearing for the popover presentation
    /// ([`NotificationCenterButton`](super::center_button::NotificationCenterButton)):
    /// the overlay layer measures its content with a fully unbounded
    /// proposal, and a [`StandardListItem`] asked for an intrinsic
    /// width reports only its chrome, so without a preferred width the
    /// popover would size itself to whatever the two toolbar buttons
    /// happen to measure (~248 dp with the stock labels) and elide
    /// every title to a stub. Hosts that DO bound the width (a dialog,
    /// a side panel) ignore this value.
    pub fn preferred_width(mut self, width: f32) -> Self {
        self.preferred_width = width;
        self
    }

    /// Height of the scrolling list area when the host proposes an
    /// unbounded height. Default `320` dp.
    ///
    /// The log always *grows* into a host that bounds its height (it
    /// reports a flex weight), so this only sets the natural height a
    /// content-hugging host — again, the popover — sizes itself to.
    pub fn preferred_height(mut self, height: f32) -> Self {
        self.preferred_height = height;
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
        let glyph: Box<dyn Widget> = Box::new(SeverityBadge::new(entry.severity.into(), 14.0));
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
            })
            // Notification text is arbitrary app-supplied prose and
            // the log does not control its own width (popover, dialog,
            // side panel). `StandardListItem` defaults to
            // `TextOverflow::Wrap`, which reports the label's FULL
            // one-line intrinsic width and shrinks for nobody — so a
            // long title over-constrained the row: the text ran past
            // the clip edge (with no horizontal scrollbar to reach it)
            // and the trailing action buttons were pushed clean out of
            // the row, unreachable. Eliding keeps every row inside its
            // width and lets the label column absorb the deficit; the
            // full text stays readable through the row tooltip below.
            .label_overflow(TextOverflow::Ellipsis(EllipsisMode::Trailing))
            .subtitle_overflow(TextOverflow::Ellipsis(EllipsisMode::Trailing));
        if let Some(body) = &entry.body {
            row = row.subtitle(lit!(body.clone()));
        }
        // Full, un-elided text on hover. The *rich* tier, not the plain
        // one: `TooltipWidget` is single-line, so a long body would
        // render as one enormous streak, while `RichTooltipWidget`
        // clamps to the theme's tooltip max-width and wraps.
        row = row.rich_tooltip_content(TooltipContent::new(
            format!("notification.entry.{}", entry.id),
            lit!(match &entry.body {
                Some(body) => format!("{}\n{}", entry.title, body),
                None => entry.title.clone(),
            }),
        ));
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
            .field("preferred_width", &self.preferred_width)
            .field("preferred_height", &self.preferred_height)
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
        //
        // One signal for every window's log — a log embedded in one
        // window shares the signal with a log (or bell) in another
        // without either being able to consume the other's rebuild.
        archive.version_signal().bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Rebuild,
        );

        let scope = self.route_scope;

        // Snapshot + filter entries up front — both the empty-state
        // decision and the toolbar/section rendering below need the
        // SCOPED view, not the raw archive (a scoped log must look
        // empty when only other windows'/audiences' entries exist,
        // not fall through to the unscoped empty-state check).
        let model = archive.entries();
        let entries: Vec<NotificationEntry> = (0..model.len())
            .filter_map(|i| model.with_item(i, |e| e.clone()))
            .filter(|e| route_visible(e.route, scope))
            .collect();

        let mut column = VStack::new().spacing(6.0);

        // Toolbar row. Mark-read/clear are scoped identically to the
        // rendered rows: a scoped log must only affect ITS entries —
        // reaching for the unscoped `mark_all_read`/`clear` from a
        // scoped log would incorrectly touch every other window's or
        // audience's history too.
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
                            .on_activate_fn(move |_| match scope {
                                Some(s) => archive_for_mark
                                    .mark_read_where(|e| route_visible(e.route, Some(s))),
                                None => archive_for_mark.mark_all_read(),
                            }),
                    ),
                )
                .add_child(
                    ctx.add(
                        Button::new(bastyde_i18n::tr_widget!(notifications_clear()))
                            .variant(ButtonVariant::Plain)
                            .on_activate_fn(move |_| match scope {
                                Some(s) => archive_for_clear
                                    .clear_where(|e| route_visible(e.route, Some(s))),
                                None => archive_for_clear.clear(),
                            }),
                    ),
                );
            column = column.add_child(ctx.add(toolbar));
        }

        // Empty state or bucketed sections — against the SCOPED
        // entries snapshot taken above, not the raw archive.
        if entries.is_empty() {
            let empty = match &self.empty_state {
                Some(factory) => ctx.add_boxed(factory()),
                None => ctx.add(
                    TextWidget::new(bastyde_i18n::tr_widget!(notifications_empty()))
                        .color(TextRole::Secondary)
                        .style(TextStyleRole::Body),
                ),
            };
            // Centred in whatever room the host leaves, as documented —
            // it used to sit flush against the top-leading corner, which
            // in a 720x520 dialog read as a stray line of grey text.
            // `Expand::vertical` claims the slack without competing for
            // the horizontal axis; `Center` does the centring (a bare
            // `Center` reports no flex, so it would not claim anything).
            column = column.add_child(
                ctx.add(
                    Expand::vertical()
                        .flex(1.0)
                        .child(Center::new().child_id(empty)),
                ),
            );
        } else {
            // Compute buckets relative to the local "today". Bucket
            // transitions across midnight are recomputed on the next
            // archive mutation (the log's version-signal binding); a
            // log that stays open across midnight without any push
            // will keep stale labels until the user closes / reopens
            // it — acceptable for a popover-shaped UI.
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
                        .color(TextRole::Secondary);
                    // Indent to the rows' content inset so the header
                    // lines up with the severity glyph below it instead
                    // of hanging 8 dp further left than every row.
                    sections = sections.add_child(ctx.add(
                        Padding::symmetric(0.0, si::STANDARD_ITEM_PADDING_HORIZONTAL).child(header),
                    ));
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
            //
            // `ScrollArea` deliberately takes its height from its
            // PARENT (else it grows to fit its content and never
            // scrolls), falling back to a fixed default when the
            // parent offers none. A bare `ScrollArea` in this VStack
            // is rigid, so it kept that fallback height in every host:
            // a 520 dp dialog rendered a 200 dp list with ~300 dp of
            // dead space below it. `Expand::vertical` gives it the
            // flex weight to claim the leftover; `respect_intrinsic`
            // keeps the preferred height as the floor so a
            // content-hugging host (the popover) still gets a sized
            // list instead of a zero-basis collapse.
            //
            // `Shrinkable` on the outside handles the mirror case: a
            // host SHORTER than the preferred height. `Expand` reports
            // `shrink = 0`, so on its own the list would keep its full
            // wanted height and spill out the bottom of a short panel.
            // `Shrinkable` preserves the inner flex weight and adds the
            // compression path, floored at one two-line row.
            let scrollable = ScrollArea::new()
                .preferred_height(self.preferred_height)
                .child(sections);
            column = column.add_child(
                ctx.add(
                    Shrinkable::new()
                        .min_height(si::STANDARD_ITEM_MIN_HEIGHT_TWO_LINE)
                        .child(
                            Expand::vertical()
                                .flex(1.0)
                                .respect_intrinsic()
                                .child(scrollable),
                        ),
                ),
            );
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
        // On an unbounded width the host is hugging its content (the
        // overlay layer measures popover content with a fully
        // unbounded proposal). Rows cannot answer that — a
        // `StandardListItem` reports only its chrome width when asked
        // for an intrinsic one — so substitute the preferred width and
        // let the rows lay out inside it.
        let effective = SizeProposal {
            width: proposal.width.or(Some(self.preferred_width)),
            height: proposal.height,
        };
        // Forward the child's FULL response — flattening it to a
        // `Size` (via `child_size`) reports flex 0 / shrink 0 / min =
        // size, i.e. a rigid log that neither grows into a tall dialog
        // nor compresses inside a short one, whatever the inner
        // column says. Same rule the `DeadZone` wrapper follows.
        self.root_child_id
            .and_then(|id| ctx.child_layout_response(id, effective))
            .unwrap_or_else(|| {
                effective
                    .resolve(self.preferred_width, self.preferred_height)
                    .into()
            })
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
                    .color(TextRole::Secondary),
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
            route: ToastRoute::Broadcast,
        }
    }

    fn fresh_archive() -> Rc<NotificationArchiveModel> {
        Rc::new(NotificationArchiveModel::in_memory())
    }

    fn tree_with(log: NotificationLog) -> WidgetTree {
        let (tree, _) = tree_sized(log, SizeProposal::exact(480.0, 360.0));
        tree
    }

    /// Mount `log` as the root at an explicit proposal and hand back
    /// its `WidgetId` too, for the layout assertions below. A real text
    /// backend is installed (fixed 8 dp/char) — without one every
    /// string measures zero and no width assertion means anything.
    fn tree_sized(log: NotificationLog, proposal: SizeProposal) -> (WidgetTree, WidgetId) {
        let mut tree = WidgetTree::new()
            .with_theme(bastyde_core::presets::intui::light())
            .with_text_backend(Rc::new(std::cell::RefCell::new(
                bastyde_canvas::MockTextBackend::new(),
            )));
        let id = tree.add(log);
        tree.layout(proposal);
        (tree, id)
    }

    /// First widget in `root`'s subtree whose concrete type name ends
    /// with `suffix` (e.g. `"ScrollArea"`), in walk order.
    fn find_by_type(tree: &WidgetTree, root: WidgetId, suffix: &str) -> Option<WidgetId> {
        if tree
            .widget_type_name(root)
            .is_some_and(|n| n.ends_with(suffix))
        {
            return Some(root);
        }
        tree.children(root)
            .into_iter()
            .find_map(|c| find_by_type(tree, c, suffix))
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

    // ----- Layout -----

    /// The scrolling list claims the height its host offers. It used to
    /// report `ScrollArea`'s fixed fallback height in every host, so a
    /// 600 dp panel rendered a 200 dp list over 370 dp of dead space.
    #[test]
    fn the_list_area_fills_the_height_its_host_offers() {
        let archive = fresh_archive();
        for i in 0..8 {
            archive.push(entry(&format!("Notice {i}"), Some("body"), Vec::new()));
        }
        let (tree, root) = tree_sized(
            NotificationLog::new(archive),
            SizeProposal::exact(480.0, 600.0),
        );
        let scroll = find_by_type(&tree, root, "ScrollArea").expect("log has a ScrollArea");
        let scroll_h = tree.bounds(scroll).height;
        let root_h = tree.bounds(root).height;
        // Everything below the toolbar row belongs to the list.
        assert!(
            scroll_h > root_h - 60.0,
            "list must fill the host height: list {scroll_h} in a {root_h} tall log"
        );
    }

    /// The mirror case: a host SHORTER than the preferred height
    /// compresses the list rather than spilling out the bottom.
    #[test]
    fn the_list_area_compresses_inside_a_short_host() {
        let archive = fresh_archive();
        for i in 0..8 {
            archive.push(entry(&format!("Notice {i}"), Some("body"), Vec::new()));
        }
        let (tree, root) = tree_sized(
            NotificationLog::new(archive),
            SizeProposal::exact(480.0, 160.0),
        );
        let scroll = find_by_type(&tree, root, "ScrollArea").expect("log has a ScrollArea");
        let b = tree.bounds(scroll);
        assert!(
            b.y + b.height <= 160.0 + 0.01,
            "list bottom {} must stay inside the 160 dp host",
            b.y + b.height
        );
    }

    /// The readability regression: a title longer than the row is
    /// elided inside it. With `StandardListItem`'s wrapping default the
    /// label reported its full one-line intrinsic width, over-
    /// constrained the row, and shoved the trailing action button clean
    /// outside the row — clipped, unclickable, and with no horizontal
    /// scrollbar to reach it.
    #[test]
    fn a_long_title_keeps_the_action_button_inside_the_row() {
        let archive = fresh_archive();
        archive.push(entry(
            "Build failed for target aarch64-unknown-linux-gnu after 42 seconds",
            Some("the linker could not resolve symbol __bastyde_frobnicate_v2"),
            vec![ArchivedAction {
                label: "Retry".into(),
                intent_name: Some("app.build.retry".into()),
                style: ArchivedActionStyle::PrimaryButton,
                closes_on_invoke: true,
            }],
        ));
        let (tree, root) = tree_sized(
            NotificationLog::new(archive).on_action_invoked(|_e, _a, _c| {}),
            SizeProposal::exact(320.0, 400.0),
        );
        let button = tree.find_by_label("Retry").expect("Retry button");
        let b = tree.bounds(button);
        let right_edge = tree.bounds(root).width;
        assert!(
            b.x + b.width <= right_edge + 0.01,
            "action button right edge {} must stay within the {right_edge} dp row",
            b.x + b.width
        );
    }

    /// A content-hugging host — which is exactly how the overlay layer
    /// measures a popover — gets the preferred width, not whatever the
    /// two toolbar buttons happen to measure (~248 dp, the old result).
    #[test]
    fn a_content_hugging_host_gets_the_preferred_width() {
        let archive = fresh_archive();
        archive.push(entry("Export finished", Some("14 chapters"), Vec::new()));
        let unbounded = SizeProposal {
            width: None,
            height: None,
        };
        let (tree, root) = tree_sized(NotificationLog::new(archive.clone()), unbounded);
        assert!(
            (tree.bounds(root).width - DEFAULT_PREFERRED_WIDTH).abs() < 0.01,
            "unbounded width = {}, expected the preferred {DEFAULT_PREFERRED_WIDTH}",
            tree.bounds(root).width
        );

        let (tree, root) = tree_sized(
            NotificationLog::new(archive).preferred_width(520.0),
            unbounded,
        );
        assert!(
            (tree.bounds(root).width - 520.0).abs() < 0.01,
            "preferred_width override ignored: got {}",
            tree.bounds(root).width
        );
    }

    /// The empty state is a factory because `build` re-runs on every
    /// archive mutation: a stored `Box<dyn Widget>` was consumed by the
    /// first build and the custom view never came back.
    #[test]
    fn a_custom_empty_state_survives_a_rebuild() {
        let archive = fresh_archive();
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(
            NotificationLog::new(archive.clone())
                .empty_state(|| Box::new(TextWidget::new(lit!("Inbox zero")))),
        );
        tree.layout(SizeProposal::exact(480.0, 360.0));
        assert!(
            tree.find_by_label("Inbox zero").is_some(),
            "shown initially"
        );

        archive.push(entry("Notice", None, Vec::new()));
        tree.layout(SizeProposal::exact(480.0, 360.0));
        archive.clear();
        tree.layout(SizeProposal::exact(480.0, 360.0));
        assert!(
            tree.find_by_label("Inbox zero").is_some(),
            "the custom empty state must come back when the archive empties again"
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
