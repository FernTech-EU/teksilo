// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Persistent notification archive — the storage and data-model layer
//! backing [`NotificationLog`], [`NotificationCenterButton`], and
//! [`NotificationLogDialog`].
//!
//! Every toast presented through the toast registry is mirrored into a
//! [`NotificationArchiveModel`] when archiving is enabled via
//! `ToastInstallOptions::archive`. The model is a
//! [`ListModel<NotificationEntry>`](bastyde_data::ListModel) plus an
//! unread-count signal — shaped for one-line binding to the notification
//! UI family. Two storage variants are available: an in-memory session-only
//! ring buffer ([`NotificationArchive::InMemory`]) and a file-backed
//! persistent store ([`NotificationArchive::Persistent`]) that survives app
//! restarts. Action callbacks attached via raw closures are lost on
//! archival; actions that should remain re-invokable from the log carry an
//! `intent_name` that the log replays through `ctx.send_intent(...)`.
//!
//! ## When to use
//!
//! - Pair with `BastydeAppBuilder::install_toast_default()` to get the full
//!   bell-button + log + persistence stack for free.
//! - Construct [`NotificationArchiveModel::in_memory`] directly in tests or
//!   custom toast setups.
//!
//! ```ignore
//! // In app boot, after install_toast:
//! let archive = ctx.app_state::<Rc<RefCell<NotificationArchiveModel>>>().unwrap();
//! let log = NotificationLog::new(archive.clone());
//! ```

pub mod archive;
pub mod center_button;
pub mod log;
pub mod log_dialog;

use bastyde_core::styles::{BannerSeverity, ToastPriority};
use bastyde_settings::Keyed;
use serde::{Deserialize, Serialize};

use crate::toast::ToastRoute;

pub use archive::{
    ARCHIVE_FILE_NAME, DEFAULT_ARCHIVE_LIMIT, NotificationArchive, NotificationArchiveError,
    NotificationArchiveModel,
};
pub use center_button::NotificationCenterButton;
pub use log::NotificationLog;
pub use log_dialog::NotificationLogDialog;

/// A single archived notification entry rendered by [`NotificationLog`] and
/// persisted under `NotificationArchive::Persistent`. Carries plain owned
/// fields only — no closures, no `Rc<dyn Fn>` — so it is `Serialize`-friendly.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NotificationEntry {
    /// Stable per-archive id (separate from the runtime `Toast::id`
    /// dedup key — that one lives in `dedup_id` below). Assigned by
    /// the archive on first push; never reused.
    pub id: u64,
    /// Severity at the time of the original push. Drives the log's
    /// row glyph + severity-chip filter.
    pub severity: BannerSeverity,
    pub priority: ToastPriority,
    /// Resolved title (`LocalizedString::resolve_now()` snapshot).
    pub title: String,
    pub body: Option<String>,
    pub actions: Vec<ArchivedAction>,
    /// Wall-clock timestamp at first push. The log's day-bucket
    /// computation runs against this in the user's local timezone.
    pub timestamp: jiff::Timestamp,
    /// Optional grouping key for the log's visual section headers.
    pub group: Option<String>,
    /// Optional originating-feature tag (e.g. `"build"`, `"sync"`).
    /// Surfaced as a chip in the log row.
    pub source: Option<String>,
    /// Flipped when the user opens the log popover. Drives the bell
    /// badge's `unread_count` signal.
    pub read: bool,
    /// `Toast::id(...)` value, if any — used for update-in-place
    /// merge logic. New entries with a matching `dedup_id` append
    /// to the existing entry's `updates` list rather than creating
    /// a separate row.
    pub dedup_id: Option<String>,
    /// In-place updates from subsequent `Toast::id(...)` presents.
    /// Empty on a freshly-pushed entry.
    pub updates: Vec<NotificationUpdate>,
    /// Mirrored from the originating `LiveEntry::route` (see
    /// `ToastRegistry::entry_to_archive`) — drives which bell(s) show
    /// this entry and which bell's "mark all read" / "clear" affects
    /// it. Defaults to [`ToastRoute::Broadcast`] on deserialization
    /// when the field is absent (a `notifications.toml` written before
    /// this feature existed): treating pre-upgrade history as
    /// broadcast keeps it visible in every window's bell, rather than
    /// it silently vanishing from all of them the moment routing scopes
    /// are introduced.
    #[serde(default = "default_notification_route")]
    pub route: ToastRoute,
}

fn default_notification_route() -> ToastRoute {
    ToastRoute::Broadcast
}

/// Keyed by the stable, never-reused archive `id` (not the transient
/// `dedup_id`, which is only used to *find* the row to merge into — see
/// [`NotificationArchiveModel::push`](archive::NotificationArchiveModel::push)).
/// This is what lets [`PersistedListModel`](bastyde_settings::PersistedListModel)
/// merge a peer process's concurrent archive write by row identity
/// instead of by whole-document snapshot.
impl Keyed for NotificationEntry {
    type Key = u64;

    fn key(&self) -> u64 {
        self.id
    }
}

/// One in-place mutation applied when a `Toast` with the same `id` as an
/// existing entry is presented again. The archive merges these onto the
/// existing row — the "Uploading 3 of 7 → Upload complete" pattern.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NotificationUpdate {
    pub timestamp: jiff::Timestamp,
    pub title: Option<String>,
    pub body: Option<String>,
    pub progress: Option<f32>,
}

/// Visual presentation of an archived action button. Maps one-to-one to
/// `ToastActionStyle`; re-declared as a self-contained `Serialize`-friendly
/// enum so the archive type does not depend on `ButtonVariant`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ArchivedActionStyle {
    /// JetBrains-style hyperlink in the body row.
    Link,
    /// Filled (primary CTA).
    PrimaryButton,
    /// Plain (secondary).
    SecondaryButton,
    /// Destructive (red-tinted).
    Destructive,
}

/// A single action stored alongside an archived notification entry. Only
/// re-invokable from [`NotificationLog`] when `intent_name` is set — actions
/// whose live closure has torn down render as inert descriptive labels.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArchivedAction {
    /// Resolved label snapshot.
    pub label: String,
    /// Intent name for archive replay through `ctx.send_intent(...)`.
    /// `None` for closure-only actions; the log renders these as
    /// non-clickable past-action tags.
    pub intent_name: Option<String>,
    pub style: ArchivedActionStyle,
    /// Mirrors the live action's `closes_toast` flag. The log uses
    /// it informationally only — a replayed Intent fires and the
    /// archive row itself doesn't dismiss (since it's not a live
    /// toast).
    pub closes_on_invoke: bool,
}

/// Whether an entry carrying `route` should be visible to a bell/log
/// scoped to `scope`. `scope: None` means "unscoped" — the legacy
/// "see everything" behaviour, so an existing single-window app that
/// never calls `NotificationCenterButton::for_window` /
/// `for_audience` (or the matching `NotificationLog` methods) keeps
/// showing every entry exactly as before this feature existed.
/// `Broadcast` entries are always visible regardless of `scope` —
/// that's the entire point of a genuinely app-wide message.
pub(crate) fn route_visible(route: ToastRoute, scope: Option<ToastRoute>) -> bool {
    match scope {
        None => true,
        Some(scope) => route == scope || matches!(route, ToastRoute::Broadcast),
    }
}
