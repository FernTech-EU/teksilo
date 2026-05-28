//! Notification archive model — the persistent layer behind
//! [`Toast`](crate::toast::Toast).
//!
//! Every toast presented through the registry is mirrored into a
//! [`NotificationArchiveModel`]
//! (when archiving is enabled via `ToastInstallOptions::archive`).
//! The model is a [`ListModel<NotificationEntry>`](bastyde_data::ListModel)
//! plus an unread-count signal — pre-shaped for binding to a
//! `NotificationLog` / `NotificationCenterButton` (the
//! widgets that consume this archive).
//!
//! Two storage variants are supported:
//! - [`NotificationArchive::InMemory`] — session-only ring buffer.
//! - [`NotificationArchive::Persistent`] — file-backed via
//!   [`bastyde_settings::PersistedListModel`] so the archive survives
//!   app restarts. Standard atomic write-temp+rename through the
//!   shared bastyde-settings I/O thread.
//!
//! Archive entries are independent of `Toast` request types — they
//! carry plain owned fields (no closures) so they survive
//! serialization. Action callbacks attached via raw closures are
//! lost on archival; actions that should remain re-invokable from
//! the log carry an `intent_name` (set via
//! [`ToastAction::shortcut_id`](crate::toast::ToastAction::shortcut_id))
//! that the log replays through the existing `ctx.send_intent(...)`
//! dispatcher.
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
use serde::{Deserialize, Serialize};

pub use archive::{
    ARCHIVE_FILE_NAME, DEFAULT_ARCHIVE_LIMIT, NotificationArchive, NotificationArchiveError,
    NotificationArchiveModel,
};
pub use center_button::NotificationCenterButton;
pub use log::NotificationLog;
pub use log_dialog::NotificationLogDialog;

/// A single archived notification — what
/// [`NotificationLog`] renders, what survives across
/// app restarts under `NotificationArchive::Persistent`. Owned, plain
/// fields only; no closures, no `Rc<dyn Fn>`.
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
}

/// One mutation applied via the update-in-place pattern (a `Toast`
/// presented with the same `id` as an existing live entry). The
/// archive merges these onto the existing row rather than appending
/// a new one — that's the "Uploading 3 of 7… → Upload complete"
/// pattern.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NotificationUpdate {
    pub timestamp: jiff::Timestamp,
    pub title: Option<String>,
    pub body: Option<String>,
    pub progress: Option<f32>,
}

/// Style hint for an archived action. Re-declared here as a small
/// owned enum (rather than re-using `crate::toast::ToastActionStyle`)
/// because the archive type must be `Serialize`-friendly — the toast
/// variant carries a `ButtonVariant`, which has its own dependencies
/// and `Serialize` would have to be re-exported through bastyde-tokens.
/// The mapping is one-to-one with `ToastActionStyle`.
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

/// A single action stored alongside an archived notification.
/// Re-invokable from the [`NotificationLog`] only when `intent_name`
/// is set (see the module-level docs).
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
