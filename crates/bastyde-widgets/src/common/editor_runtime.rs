// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Per-frame runtime mechanics shared by every text-editing surface.
//!
//! [`RichTextEditor`](crate::rich_text::RichTextEditor),
//! [`TextInputField`](crate::primitives::TextInputField) and
//! [`CodeEditor`](crate::code_editor::CodeEditor) are three different widgets
//! — different documents, different layout strategies, different event
//! vocabularies — but they share the same *clock*: a caret that blinks
//! against wall-clock time, a debounce window that coalesces edit bursts, and
//! a scroll-metric publish step that turns engine content metrics into the
//! signals their scroll bars bind to.
//!
//! Those three mechanisms are what lives here. They were duplicated
//! byte-for-byte between the first two surfaces before this module existed
//! (`text_input_field.rs` even carried a `// same as RichTextEditor` comment
//! on its blink constant), and a third copy for the code editor is what
//! prompted the extraction: three hand-maintained copies of a timing rule
//! drift, and drift in *this* rule is invisible in tests and obvious to
//! users — a caret that blinks at a different rate in one widget than the
//! next.
//!
//! Deliberately **not** here: anything that reads the document, the engine,
//! or the cursor. Each surface's state struct stays its own. These types own
//! a timer and nothing else, which is why they can be shared without coupling
//! three widgets to one another.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use bastyde_core::signal::Signal;

/// Caret blink half-period — the time between on/off toggles, so a full
/// on→off→on cycle takes twice this. 500 ms is the common desktop default
/// (Qt's default `QApplication::cursorFlashTime` is 1000 ms *per cycle*).
const CARET_BLINK_INTERVAL: f32 = 0.5;

/// Debounce window for coalesced signal emission (`text_changed`,
/// `format_changed`, `undo_redo_changed`). Rapid typing must not hammer
/// every toolbar observer once per keystroke.
const DEBOUNCE_WINDOW_SECS: f32 = 0.150;

/// How the caret is presented on a text surface.
///
/// Lives here rather than beside one widget's policy bundle because the shared
/// blink state machine is what interprets it, and all three text surfaces feed
/// that machine. Re-exported as `rich_text::CaretPolicy` — that is its public
/// name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaretPolicy {
    /// Caret blinks while the widget has focus (editor preset).
    Blinking,
    /// Caret visible but not blinking. Use for focusable surfaces that
    /// need a visible insertion point without distracting animation —
    /// e.g. a custom read-only editor the user can navigate and copy
    /// but that must not suggest editability. Neither built-in preset
    /// uses this value; construct a custom `PolicyBundle` to opt in.
    StaticVisible,
    /// Caret not rendered at all (read-only preset).
    Hidden,
}

/// A command a text surface's keyboard layer may emit.
///
/// Each surface defines its own vocabulary — the rich text editor's
/// `EditCommandKind` knows about tables, lists and blockquotes; the code
/// editor's `CodeCommand` knows about indent levels and line comments and
/// would be nonsense in prose. What they share is the single question a
/// read-only preset needs answered, which is this trait.
pub trait EditorCommand: Copy {
    /// Whether this command modifies the document. Navigation, selection, and
    /// copy never do.
    fn mutates_document(&self) -> bool;
}

/// Command filter consulted before any cursor call in a surface's keyboard
/// layer.
///
/// Generic over the command vocabulary rather than duplicated per surface: the
/// *rule* ("a read-only surface accepts everything that doesn't mutate") is the
/// same for prose and for code, only the list of commands differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandFilter {
    /// Every command accepted (editor preset).
    All,
    /// Mutating commands rejected; navigation and copy/select-all accepted
    /// (read-only preset).
    ReadOnly,
}

impl CommandFilter {
    pub fn accepts<C: EditorCommand>(&self, cmd: C) -> bool {
        match self {
            Self::All => true,
            // Everything that doesn't touch the document is fair game: a
            // read-only surface still navigates, selects, and copies.
            Self::ReadOnly => !cmd.mutates_document(),
        }
    }
}

/// Drives the AccessKit role a text surface reports.
///
/// Deliberately only two values. Both map to roles that
/// `accesskit_consumer::Node::supports_text_ranges()` accepts, which is a hard
/// requirement rather than a preference: a role outside that set (`Role::Code`,
/// `Role::Log`) silently disables caret and selection reporting through the
/// platform accessibility layer, so a screen reader could read the text once on
/// focus but never track the cursor through it. A surface wanting log-style
/// announcements pairs `Document` with an explicit `Live` — live-region
/// behaviour is an independent property, not a role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessibilityRole {
    /// `Role::MultilineTextInput` — editable.
    Editor,
    /// `Role::Document` — read-only body of text.
    Document,
}

/// Clipboard surface exposed by a text widget.
///
/// The command filter already rejects cut/paste for a read-only preset; this
/// drives UI affordances such as disabled menu items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardPolicy {
    Full,
    CopyAndSelectAllOnly,
}

impl ClipboardPolicy {
    pub fn allows_cut(&self) -> bool {
        matches!(self, Self::Full)
    }
    pub fn allows_paste(&self) -> bool {
        matches!(self, Self::Full)
    }
    /// `PasteUnformatted` mirrors `Paste` today: both are gated by the
    /// same policy bit. Kept as a separate accessor so a future preset
    /// that admits plain-only paste while rejecting rich paste can
    /// diverge without changing call sites.
    pub fn allows_paste_unformatted(&self) -> bool {
        matches!(self, Self::Full)
    }
    /// Always `true` — copying is allowed under every policy, including
    /// `CopyAndSelectAllOnly`. Provided as a method (rather than a
    /// hardcoded literal at call sites) so a future preset can diverge
    /// without changing callers.
    pub fn allows_copy(&self) -> bool {
        true
    }
}

/// One bundle per construction preset: the single source of truth for the four
/// independent decisions that separate an editable surface from a viewer.
///
/// Shared by every text surface. The bundle is what lets a widget never consult
/// a `read_only: bool` flag — each dimension is decided once, at construction,
/// and read where it matters.
#[derive(Debug, Clone, Copy)]
pub struct PolicyBundle {
    pub command_filter: CommandFilter,
    pub caret_policy: CaretPolicy,
    pub access_role: AccessibilityRole,
    pub clipboard_policy: ClipboardPolicy,
}

impl PolicyBundle {
    pub const fn is_read_only(&self) -> bool {
        matches!(self.access_role, AccessibilityRole::Document)
    }
}

/// The full editor preset: every command accepted, caret blinks,
/// `MultilineTextInput` role, full clipboard support.
pub const EDITOR_PRESET: PolicyBundle = PolicyBundle {
    command_filter: CommandFilter::All,
    caret_policy: CaretPolicy::Blinking,
    access_role: AccessibilityRole::Editor,
    clipboard_policy: ClipboardPolicy::Full,
};

/// The read-only preset: only navigation + copy/select-all, `Document` role,
/// no cut/paste. The caret is hidden entirely — view-only widgets ship without
/// any caret affordance. Applications that need a focusable read-only surface
/// with a visible caret can construct a custom preset via [`PolicyBundle`].
pub const READ_ONLY_PRESET: PolicyBundle = PolicyBundle {
    command_filter: CommandFilter::ReadOnly,
    caret_policy: CaretPolicy::Hidden,
    access_role: AccessibilityRole::Document,
    clipboard_policy: ClipboardPolicy::CopyAndSelectAllOnly,
};

/// Wall-clock caret blink state machine.
///
/// Blinks against `Instant::now()` rather than accumulating `delta`, so the
/// visible cadence stays locked to real seconds no matter how the frame
/// scheduler behaves: if ticks are skipped, delayed, or clamped, the next
/// tick catches up instead of the blink slowing down with the frame rate.
#[derive(Debug, Default)]
pub(crate) struct CaretBlink {
    last_toggle: Option<Instant>,
}

impl CaretBlink {
    pub(crate) fn new() -> Self {
        Self { last_toggle: None }
    }

    /// Restart the blink phase, so the next toggle is a full interval away.
    ///
    /// Call on focus gain and after every caret move: a caret that happens to
    /// be mid-off when the user moves it reads as a dropped keystroke, so every
    /// editor restarts the phase on motion rather than letting the toggle land
    /// wherever it falls.
    ///
    /// **Does not itself show the caret** — the caller must set
    /// `caret_visible` alongside this. That split is deliberate rather than an
    /// oversight: `sync_cursor_signals` has to publish the signal *after*
    /// dropping its `RefCell` borrow of the editor state (a `Signal::set` fans
    /// out to observers synchronously, and an observer that reaches back into
    /// the widget would panic on the live borrow), so this type cannot own the
    /// write. Restarting without also setting `caret_visible` leaves the caret
    /// dark for up to one full interval after a cursor move — the exact
    /// symptom this method exists to prevent.
    pub(crate) fn restart(&mut self) {
        self.last_toggle = Some(Instant::now());
    }

    /// Forget the phase entirely (next `tick` starts a fresh interval).
    pub(crate) fn reset(&mut self) {
        self.last_toggle = None;
    }

    /// Drive one frame of blinking.
    ///
    /// `active` is `has_focus && window_active` — a caret in an unfocused
    /// widget or an inactive window is hidden, which is the universal desktop
    /// convention (Qt / Cocoa / Win32 / GTK all do this).
    ///
    /// `wake_at` is the tree's one-shot wake-up slot. The blink schedules its
    /// next toggle there so the event loop can idle in `WaitUntil` between
    /// toggles. Without it a blinking caret would have to keep the frame loop
    /// pumping at the OS's maximum rate (~90 fps was observed) to catch a
    /// transition that happens twice a second.
    pub(crate) fn tick(
        &mut self,
        policy: CaretPolicy,
        active: bool,
        caret_visible: &Signal<bool>,
        wake_at: Option<&Rc<Cell<Option<Instant>>>>,
    ) {
        let blinking = active && policy == CaretPolicy::Blinking;
        if blinking {
            let now = Instant::now();
            let interval = Duration::from_secs_f32(CARET_BLINK_INTERVAL);
            match self.last_toggle {
                None => self.last_toggle = Some(now),
                Some(last) if now.saturating_duration_since(last) >= interval => {
                    self.last_toggle = Some(now);
                    let was = caret_visible.get();
                    caret_visible.set(!was);
                }
                _ => {}
            }
            if let (Some(last), Some(wake)) = (self.last_toggle, wake_at) {
                let next = last + interval;
                // Never push an earlier pending wake-up later — another
                // subsystem may need the loop awake before our next toggle.
                let merged = match wake.get() {
                    Some(existing) if existing <= next => existing,
                    _ => next,
                };
                wake.set(Some(merged));
            }
            return;
        }

        self.last_toggle = None;
        match policy {
            CaretPolicy::Blinking => {
                // Not active: caret off.
                if caret_visible.get() {
                    caret_visible.set(false);
                }
            }
            CaretPolicy::StaticVisible => {
                // A static caret still hides in an inactive window; it just
                // doesn't animate while shown.
                if caret_visible.get() != active {
                    caret_visible.set(active);
                }
            }
            // Hidden never renders a caret — the signal is seeded false and
            // nothing here should flip it on.
            CaretPolicy::Hidden => {}
        }
    }
}

/// Fixed-window coalescing timer.
///
/// Owns *only* the timer. Which flags to drain and what to publish differ per
/// surface (the rich text editor debounces text + format + undo/redo, the
/// plain input only text + undo/redo), so the caller keeps its own flags and
/// asks this type one question: has the window elapsed?
#[derive(Debug)]
pub(crate) struct Debounce {
    timer: f32,
}

impl Default for Debounce {
    fn default() -> Self {
        Self::new()
    }
}

impl Debounce {
    /// Start already-expired, so the first frame after construction publishes
    /// initial state (`can_undo` / `can_redo`) immediately instead of making
    /// a freshly-built toolbar wait 150 ms to render correctly.
    pub(crate) fn new() -> Self {
        Self { timer: 1.0 }
    }

    /// Advance by `delta` seconds. Returns `true` exactly on the frames where
    /// the window has elapsed, resetting itself for the next window.
    pub(crate) fn tick(&mut self, delta: f32) -> bool {
        self.timer += delta;
        if self.timer >= DEBOUNCE_WINDOW_SECS {
            self.timer = 0.0;
            return true;
        }
        false
    }
}

/// The scroll numbers a text surface publishes each frame, derived from the
/// engine's content metrics and the current viewport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ScrollMetrics {
    pub max_x: f32,
    pub max_y: f32,
    pub ratio_x: f32,
    pub ratio_y: f32,
}

impl ScrollMetrics {
    /// Derive the metrics. `content_height` / `max_content_width` are the
    /// engine's *unzoomed* logical metrics; `zoom` scales them into the
    /// viewport's coordinate space.
    ///
    /// A ratio is the visible fraction of the content on that axis, which is
    /// what a scroll bar sizes its thumb from. It is `1.0` (full thumb, i.e.
    /// "everything is visible") when the axis has no content or no viewport —
    /// a zero-height thumb on an empty document would read as a bug.
    pub(crate) fn compute(
        content_height: f32,
        max_content_width: f32,
        zoom: f32,
        viewport_width: f32,
        viewport_height: f32,
    ) -> Self {
        let scaled_h = content_height * zoom;
        let scaled_w = max_content_width * zoom;
        Self {
            max_x: (scaled_w - viewport_width).max(0.0),
            max_y: (scaled_h - viewport_height).max(0.0),
            ratio_x: if max_content_width > 0.0 && viewport_width > 0.0 {
                (viewport_width / scaled_w).clamp(0.0, 1.0)
            } else {
                1.0
            },
            ratio_y: if content_height > 0.0 && viewport_height > 0.0 {
                (viewport_height / scaled_h).clamp(0.0, 1.0)
            } else {
                1.0
            },
        }
    }

    /// Publish into the surface's signals, and clamp the live scroll offsets
    /// to the fresh maxima.
    ///
    /// Every write is guarded by a change-check because `Signal::set` clones
    /// and fans out to every observer unconditionally — it has no internal
    /// `PartialEq` skip — so re-setting an unchanged value still walks every
    /// scroll bar and layout listener. This ran to ~5 % of frame CPU in
    /// `set<f32>` on a flamegraph before the guards were added.
    ///
    /// The clamp is why this is one method rather than four setters: deleting
    /// text shrinks `max_y`, and a scroll offset left beyond the new maximum
    /// would leave the view parked past the end of the document.
    pub(crate) fn publish(
        &self,
        scroll_x: &Signal<f32>,
        scroll_y: &Signal<f32>,
        max_scroll_x: &Signal<f32>,
        max_scroll_y: &Signal<f32>,
        viewport_ratio_x: &Signal<f32>,
        viewport_ratio_y: &Signal<f32>,
    ) {
        max_scroll_x.set_if_changed(self.max_x);
        max_scroll_y.set_if_changed(self.max_y);
        viewport_ratio_x.set_if_changed(self.ratio_x);
        viewport_ratio_y.set_if_changed(self.ratio_y);
        scroll_x.set_if_changed(scroll_x.get().clamp(0.0, self.max_x));
        scroll_y.set_if_changed(scroll_y.get().clamp(0.0, self.max_y));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wake_slot() -> Rc<Cell<Option<Instant>>> {
        Rc::new(Cell::new(None))
    }

    #[test]
    fn blink_hides_caret_when_not_active() {
        let mut blink = CaretBlink::new();
        let visible = Signal::new(true);
        blink.tick(CaretPolicy::Blinking, false, &visible, None);
        assert!(!visible.get(), "an unfocused caret must not be drawn");
    }

    #[test]
    fn blink_does_not_toggle_before_the_interval() {
        let mut blink = CaretBlink::new();
        let visible = Signal::new(true);
        // First tick only seeds the phase.
        blink.tick(CaretPolicy::Blinking, true, &visible, None);
        // Second tick, immediately after: far short of 500 ms.
        blink.tick(CaretPolicy::Blinking, true, &visible, None);
        assert!(visible.get(), "caret must not toggle within the interval");
    }

    #[test]
    fn blink_toggles_once_the_interval_has_elapsed() {
        let mut blink = CaretBlink::new();
        let visible = Signal::new(true);
        // Backdate the phase past the interval instead of sleeping.
        blink.last_toggle =
            Some(Instant::now() - Duration::from_secs_f32(CARET_BLINK_INTERVAL + 0.01));
        blink.tick(CaretPolicy::Blinking, true, &visible, None);
        assert!(!visible.get(), "caret must toggle after the interval");
    }

    #[test]
    fn blink_schedules_a_wake_up_so_the_loop_can_idle() {
        let mut blink = CaretBlink::new();
        let visible = Signal::new(true);
        let wake = wake_slot();
        blink.tick(CaretPolicy::Blinking, true, &visible, Some(&wake));
        assert!(
            wake.get().is_some(),
            "a blinking caret must schedule its next toggle, else the event \
             loop has to poll at max rate to catch it"
        );
    }

    #[test]
    fn blink_never_delays_an_earlier_pending_wake_up() {
        let mut blink = CaretBlink::new();
        let visible = Signal::new(true);
        let wake = wake_slot();
        let sooner = Instant::now() + Duration::from_millis(10);
        wake.set(Some(sooner));
        blink.tick(CaretPolicy::Blinking, true, &visible, Some(&wake));
        assert_eq!(
            wake.get(),
            Some(sooner),
            "another subsystem's earlier wake-up must survive — pushing it \
             out to our toggle would stall whatever needed it"
        );
    }

    #[test]
    fn hidden_policy_never_shows_the_caret() {
        let mut blink = CaretBlink::new();
        let visible = Signal::new(false);
        blink.tick(CaretPolicy::Hidden, true, &visible, None);
        assert!(
            !visible.get(),
            "a hidden caret must stay hidden when focused"
        );
    }

    #[test]
    fn static_visible_tracks_activity_without_blinking() {
        let mut blink = CaretBlink::new();
        let visible = Signal::new(false);
        blink.tick(CaretPolicy::StaticVisible, true, &visible, None);
        assert!(visible.get(), "a static caret shows while active");
        blink.tick(CaretPolicy::StaticVisible, false, &visible, None);
        assert!(!visible.get(), "a static caret hides when inactive");
    }

    /// `restart` buys the caret a full interval of stillness. This is what
    /// keeps it lit while the user holds an arrow key: every move restarts the
    /// phase, so the toggle never lands mid-motion.
    #[test]
    fn restart_delays_the_next_toggle_by_a_full_interval() {
        let mut blink = CaretBlink::new();
        let visible = Signal::new(true);
        // Phase is already one interval old: the next tick would toggle.
        blink.last_toggle =
            Some(Instant::now() - Duration::from_secs_f32(CARET_BLINK_INTERVAL + 0.01));
        blink.restart();
        blink.tick(CaretPolicy::Blinking, true, &visible, None);
        assert!(
            visible.get(),
            "restart must push the pending toggle out by a full interval, else \
             the caret blinks off mid-keystroke"
        );
    }

    /// The counterpart to the doc contract: `restart` deliberately does not
    /// write `caret_visible` (the caller must, outside its state borrow). A
    /// caller that forgets leaves the caret dark for an interval, so pin the
    /// split here rather than let a reader assume either way.
    #[test]
    fn restart_does_not_itself_show_the_caret() {
        let mut blink = CaretBlink::new();
        let visible = Signal::new(false);
        blink.restart();
        blink.tick(CaretPolicy::Blinking, true, &visible, None);
        assert!(
            !visible.get(),
            "restart seeds the phase only — showing the caret is the caller's"
        );
    }

    #[test]
    fn debounce_starts_expired_so_initial_state_publishes_at_once() {
        let mut d = Debounce::new();
        assert!(
            d.tick(0.0),
            "a freshly built toolbar must not wait a window to show correct \
             undo/redo state"
        );
    }

    #[test]
    fn debounce_coalesces_within_the_window() {
        let mut d = Debounce::new();
        assert!(d.tick(0.0));
        assert!(!d.tick(0.05));
        assert!(!d.tick(0.05));
        assert!(d.tick(0.05), "0.15s total must close the window");
    }

    #[test]
    fn scroll_metrics_report_no_overflow_when_content_fits() {
        let m = ScrollMetrics::compute(50.0, 80.0, 1.0, 100.0, 100.0);
        assert_eq!(m.max_x, 0.0);
        assert_eq!(m.max_y, 0.0);
        assert_eq!(m.ratio_x, 1.0);
        assert_eq!(m.ratio_y, 1.0);
    }

    #[test]
    fn scroll_metrics_account_for_zoom() {
        // 100pt of content at 2x zoom is 200px against a 100px viewport.
        let m = ScrollMetrics::compute(100.0, 100.0, 2.0, 100.0, 100.0);
        assert_eq!(m.max_y, 100.0);
        assert!((m.ratio_y - 0.5).abs() < 1e-6);
    }

    #[test]
    fn scroll_metrics_ratio_is_full_on_an_empty_document() {
        let m = ScrollMetrics::compute(0.0, 0.0, 1.0, 100.0, 100.0);
        assert_eq!(
            m.ratio_y, 1.0,
            "an empty document must show a full thumb, not a zero-height one"
        );
    }

    /// The limits and ratios are what every scroll bar binds to. Without this,
    /// dropping a publish line leaves all 14 tests here green and surfaces
    /// only as an unrelated rich-text affinity test failing — which sends the
    /// next maintainer debugging the wrong subsystem.
    #[test]
    fn publish_writes_every_limit_and_ratio_signal() {
        let (sx, sy) = (Signal::new(0.0), Signal::new(0.0));
        let (mx, my) = (Signal::new(0.0), Signal::new(0.0));
        let (rx, ry) = (Signal::new(1.0), Signal::new(1.0));
        // 400x200 of content in a 100x100 viewport: overflows on both axes.
        let m = ScrollMetrics::compute(200.0, 400.0, 1.0, 100.0, 100.0);
        m.publish(&sx, &sy, &mx, &my, &rx, &ry);

        assert_eq!(
            mx.get(),
            300.0,
            "horizontal limit must reach the scroll bar"
        );
        assert_eq!(my.get(), 100.0, "vertical limit must reach the scroll bar");
        assert!(
            (rx.get() - 0.25).abs() < 1e-6,
            "horizontal thumb ratio must reach the scroll bar, got {}",
            rx.get()
        );
        assert!(
            (ry.get() - 0.5).abs() < 1e-6,
            "vertical thumb ratio must reach the scroll bar, got {}",
            ry.get()
        );
    }

    #[test]
    fn publish_clamps_a_scroll_offset_left_past_the_new_end() {
        let (sx, sy) = (Signal::new(0.0), Signal::new(500.0));
        let (mx, my) = (Signal::new(0.0), Signal::new(500.0));
        let (rx, ry) = (Signal::new(1.0), Signal::new(1.0));
        // The document shrank: content now fits entirely.
        let m = ScrollMetrics::compute(50.0, 50.0, 1.0, 100.0, 100.0);
        m.publish(&sx, &sy, &mx, &my, &rx, &ry);
        assert_eq!(
            sy.get(),
            0.0,
            "deleting text must not leave the view parked past the end"
        );
    }
}
