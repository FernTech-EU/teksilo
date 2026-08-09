# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

# teksilo-widgets framework strings.
#
# These are the English source-language messages for framework-internal
# user-facing strings — accessibility names, descriptions, and similar
# labels that originate in teksilo-widgets source code rather than in
# application code.
#
# The proc macro `tr_widget!` (exported from `teksilo-i18n-macros` and
# re-exported via `teksilo-i18n::tr_widget`) validates every invocation in
# teksilo-widgets source against this file at compile time.
#
# At runtime, applications opt into the framework bundle via
# `I18nConfig::framework_locales(teksilo_widgets::framework_locales())`
# on the builder chain. This is **not** automatic — teksilo-app is
# deliberately widget-agnostic, so each application that uses
# teksilo-widgets must register the bundle explicitly. Applications that
# don't register the bundle still see the correct English text via the
# macro's compile-time fallback (see architecture §12.13.3 for the
# deviation from the spec's auto-registration).
#
# Applications can also override individual keys via
# `I18nConfig::override_widget_strings(...)` — those overrides win over
# the framework bundle in the §12.13.5 lookup precedence. Use this to
# ship a Japanese translation of the a11y labels when teksilo-widgets
# itself only ships English and French.

a11y-status-bar-name = Status
a11y-dialog-name = Dialog
a11y-tooltip-name = Tooltip
a11y-snackbar-name = Snackbar
a11y-splitter-divider-name = Splitter divider
a11y-splitter-pane = Pane
a11y-splitter-collapsed = Collapsed
a11y-splitter-expanded = Expanded
a11y-breadcrumb-current-page-value = current page
a11y-toolbar-name = Toolbar
toolbar-more = More
segmented-control-more = More options
breadcrumb-overflow = Show hidden path
a11y-title-bar-name = Window title bar
a11y-window-controls-name = Window controls
a11y-window-minimize-name = Minimize
a11y-window-maximize-name = Maximize
a11y-window-restore-name = Restore
a11y-window-close-name = Close
a11y-stepper-indicator-strip-name = Steps
a11y-stepper-content-name = Step content
tab-close-tooltip = Close tab
a11y-builtin-browse = Browse
a11y-builtin-expand = Expand
a11y-builtin-search = Search
a11y-builtin-copy = Copy
a11y-builtin-clear = Clear
a11y-builtin-add = Add
a11y-builtin-bell = Notifications
a11y-builtin-menu = Menu
a11y-builtin-more = More actions
a11y-builtin-visibility = Toggle visibility
a11y-password-reveal = Toggle password visibility
a11y-caps-lock-on = Caps Lock is on
notifications-title = Notifications
notifications-empty = No notifications
notifications-mark-all-read = Mark all read
notifications-clear = Clear all
notifications-filter-placeholder = Search notifications
notifications-bucket-today = Today
notifications-bucket-yesterday = Yesterday
notifications-bucket-this-week = This week
notifications-bucket-earlier = Earlier
notifications-archive-replay-disabled = (no longer available)
a11y-shortcut-settings-name = Shortcut settings
a11y-shortcut-settings-capture-hint = Press any key. Delete to clear. Escape to cancel.
keystroke-modifier-ctrl = Ctrl
keystroke-modifier-shift = Shift
keystroke-modifier-alt = Alt
keystroke-modifier-super = Super
keystroke-separator = +
keystroke-key-space = Space
keystroke-key-enter = Enter
keystroke-key-escape = Esc
keystroke-key-tab = Tab
keystroke-key-backspace = Backspace
keystroke-key-delete = Del
keystroke-key-arrow-up = Up
keystroke-key-arrow-down = Down
keystroke-key-arrow-left = Left
keystroke-key-arrow-right = Right
keystroke-key-home = Home
keystroke-key-end = End
keystroke-key-page-up = PageUp
keystroke-key-page-down = PageDown

# MessageBox standard button labels. These are the canonical English
# labels used when a caller doesn't override with
# `MessageBoxButton::label`. See `crates/teksilo-widgets/src/message_box.rs`.
messagebox-btn-ok = OK
messagebox-btn-cancel = Cancel
messagebox-btn-close = Close
messagebox-btn-yes = Yes
messagebox-btn-no = No
messagebox-btn-yes-to-all = Yes to All
messagebox-btn-no-to-all = No to All
messagebox-btn-save = Save
messagebox-btn-save-all = Save All
messagebox-btn-discard = Discard
messagebox-btn-apply = Apply
messagebox-btn-reset = Reset
messagebox-btn-restore-defaults = Restore Defaults
messagebox-btn-abort = Abort
messagebox-btn-retry = Retry
messagebox-btn-ignore = Ignore
messagebox-btn-open = Open
messagebox-btn-help = Help
messagebox-show-details = Show details

# PrivacySettings widget. Strings are surfaced by
# `crates/teksilo-widgets/src/privacy_settings.rs`. RGPD/GDPR Art. 13
# disclosure plus action buttons. Param-bearing keys use Fluent
# variable syntax `{ $name }`.
privacy-not-configured = Telemetry is not configured for this application.
privacy-a11y-group-name = Privacy & Telemetry settings
privacy-heading = Privacy & Telemetry
privacy-notice-controller = Data is processed by { $processor }; the technical processor is { $adapter } (endpoint: { $endpoint }).
privacy-notice-purposes = Purposes: improve the application — which features are used, where bugs cluster, what platforms we run on. No content of documents, no clipboard, no keystrokes, no screen captures.
privacy-notice-lawful-anonymous = Lawful basis: our legitimate interest in product improvement (GDPR Art. 6(1)(f); CNIL audience-measurement exemption).
privacy-notice-lawful-pseudonymous = Lawful basis: your explicit consent (GDPR Art. 6(1)(a)).
privacy-notice-retention = Retention: server-side data is kept for at most { $days } days.
privacy-notice-withdrawal-right = Right to withdraw: you can disable any toggle below at any time, click "Withdraw consent" to stop all collection, or in pseudonymous mode "Erase my data" to delete the records from the server.
privacy-notice-policy-link = Full privacy policy: { $url }

privacy-scope-section-heading = What can the application share?
privacy-scope-anonymous-metrics-label = Anonymous usage metrics
privacy-scope-anonymous-metrics-description = Counts of which buttons / menu items / shortcuts are used, plus app version and OS.
privacy-scope-crash-reports-label = Crash reports
privacy-scope-crash-reports-description = Stack traces and process metadata when the app crashes. No document content, no file paths.
privacy-scope-feature-flags-label = Feature flags
privacy-scope-feature-flags-description = Lets the application receive feature-flag updates (e.g. gradual rollout of new tools).

privacy-btn-reject-all = Reject all
privacy-btn-accept-all = Accept all
privacy-btn-erase = Erase my data
privacy-btn-erase-tooltip = Asks the server to delete every event recorded for this install, then withdraws consent locally.
privacy-btn-fetch = Get my data
privacy-btn-fetch-tooltip = Fetches every event the server has recorded under your install ID. You can save the result as JSON.
privacy-btn-withdraw = Withdraw consent
privacy-btn-withdraw-tooltip = Stops new data collection. Already-recorded server data is preserved — use "Erase my data" first if you want it deleted.
privacy-btn-switch-to-anonymous = Switch to Anonymous mode
privacy-btn-switch-to-pseudonymous = Switch to Pseudonymous mode

privacy-identity-heading = Your data on the server
privacy-identity-install-id = Install ID: { $id }
privacy-identity-retention = Server retains your records for at most { $days } days.

privacy-mode-heading = Privacy mode
privacy-mode-current-anonymous = Currently: Anonymous (no install ID)
privacy-mode-current-pseudonymous = Currently: Pseudonymous (install ID present)
privacy-mode-blurb-anonymous = Anonymous mode transmits no per-device identifier. Switching will erase your existing server records and discard the local install UUID — this cannot be undone.
privacy-mode-blurb-pseudonymous = Pseudonymous mode generates a random install UUID. You'll be able to fetch or erase your records on the server. Requires explicit consent and re-prompts on switch.

privacy-confirm-mode-switch-title = Switch privacy mode?
privacy-confirm-mode-switch-leaving-pseudonymous = This will ask the server to erase every event recorded under your install ID, drop the local install UUID, reset your consent decision, and switch the privacy mode. Do you want to continue?
privacy-confirm-mode-switch-leaving-anonymous = This will reset your consent decision and switch the privacy mode. You'll be re-prompted before any new data is collected. Continue?
privacy-confirm-erase-title = Erase your data?
privacy-confirm-erase-text = This sends a deletion request for every event recorded under your install ID, drops anything still buffered locally, and withdraws consent so no further data is collected. The action cannot be undone.
privacy-confirm-withdraw-title = Withdraw consent?
privacy-confirm-withdraw-text = No further analytics events will be collected from this app. Already-recorded server data is preserved — use "Erase my data" before withdrawing if you want it deleted as well.

privacy-fetch-success-title = Your data on the server
privacy-fetch-success-text = Fetched { $count } events for this install.
privacy-fetch-saved-to = Saved to: { $path }
privacy-fetch-write-error = Could not write file { $path }: { $error }
privacy-fetch-error-title = Couldn't fetch your data

privacy-inspect-title = Inspect data sent ({ $count } event(s) buffered)
privacy-inspect-empty = No events have been emitted in this session yet. Try interacting with the app — clicks, menus, and shortcuts all flow through here.
privacy-inspect-summary = Showing the last { $count } event(s), newest first.

# Calendar / DateEdit / TimeEdit / DateTimeEdit. Strings consumed by
# crates/teksilo-widgets/src/{calendar,date_edit,time_edit,date_time_edit}.rs
# and the shared modules under crates/teksilo-widgets/src/common/datetime/.
# Month and weekday names use long/short/narrow widths; the widget picks
# the variant that fits the available cell width.
calendar-month-long-january = January
calendar-month-long-february = February
calendar-month-long-march = March
calendar-month-long-april = April
calendar-month-long-may = May
calendar-month-long-june = June
calendar-month-long-july = July
calendar-month-long-august = August
calendar-month-long-september = September
calendar-month-long-october = October
calendar-month-long-november = November
calendar-month-long-december = December

calendar-month-short-january = Jan
calendar-month-short-february = Feb
calendar-month-short-march = Mar
calendar-month-short-april = Apr
calendar-month-short-may = May
calendar-month-short-june = Jun
calendar-month-short-july = Jul
calendar-month-short-august = Aug
calendar-month-short-september = Sep
calendar-month-short-october = Oct
calendar-month-short-november = Nov
calendar-month-short-december = Dec

calendar-weekday-long-monday = Monday
calendar-weekday-long-tuesday = Tuesday
calendar-weekday-long-wednesday = Wednesday
calendar-weekday-long-thursday = Thursday
calendar-weekday-long-friday = Friday
calendar-weekday-long-saturday = Saturday
calendar-weekday-long-sunday = Sunday

calendar-weekday-short-monday = Mon
calendar-weekday-short-tuesday = Tue
calendar-weekday-short-wednesday = Wed
calendar-weekday-short-thursday = Thu
calendar-weekday-short-friday = Fri
calendar-weekday-short-saturday = Sat
calendar-weekday-short-sunday = Sun

calendar-weekday-narrow-monday = M
calendar-weekday-narrow-tuesday = T
calendar-weekday-narrow-wednesday = W
calendar-weekday-narrow-thursday = T
calendar-weekday-narrow-friday = F
calendar-weekday-narrow-saturday = S
calendar-weekday-narrow-sunday = S

calendar-button-previous-month = Previous month
calendar-button-next-month = Next month
calendar-button-previous-year = Previous year
calendar-button-next-year = Next year
calendar-button-today = Today
calendar-button-month-picker = Pick month
calendar-button-year-picker = Pick year
calendar-week-number-column = Week
calendar-name = Calendar
calendar-months-grid-label = Months
calendar-years-grid-label = Years
calendar-name-with-month = Calendar, { $month } { $year }
calendar-cell-name = { $weekday }, { $month } { $day }, { $year }
calendar-range-status = Selected: { $start } – { $end }

date-edit-segment-year = Year
date-edit-segment-month = Month
date-edit-segment-day = Day
date-edit-calendar-button = Choose date
date-edit-trigger-tooltip = Open calendar
date-edit-name = Date
date-edit-placeholder = Select a date

time-edit-segment-hour = Hour
time-edit-segment-minute = Minute
time-edit-segment-second = Second
time-edit-segment-period = AM/PM
time-edit-period-am = AM
time-edit-period-pm = PM
time-edit-name = Time
time-edit-placeholder = Select a time

date-time-edit-name = Date and time
date-time-edit-placeholder = Select date and time
date-time-edit-date-name = Date
date-time-edit-time-name = Time
date-time-edit-trigger-tooltip = Open calendar
date-range-edit-name = Date range
date-range-edit-placeholder = Select date range
date-range-edit-start-name = Start date
date-range-edit-end-name = End date
date-range-edit-trigger-tooltip = Open range calendar

# Validation feedback (TextInputField + DateEdit/TimeEdit)
validation-corrected-to = Auto-corrected to { $value }
validation-corrected-with-notes = Auto-corrected: { $notes }
validation-segment-clamped = { $segment } { $raw } → { $clamped }
validation-day-clamped-to-month = day { $raw } → { $clamped } (last day of month)
validation-clamped-to-range = clamped to allowed range
validation-segment-year = year
validation-segment-month = month
validation-segment-day = day
validation-segment-hour = hour
validation-segment-minute = minute
validation-segment-second = second
validation-segment-value = value
date-edit-validation-not-a-date = Not a valid date
time-edit-validation-not-a-time = Not a valid time

# ── color picker ──
color-picker-name = Color picker
color-picker-hue-label = Hue
color-picker-saturation-label = Saturation
color-picker-value-label = Brightness
color-picker-alpha-label = Opacity
color-picker-red-label = Red
color-picker-green-label = Green
color-picker-blue-label = Blue
color-picker-red-short = R
color-picker-green-short = G
color-picker-blue-short = B
color-picker-alpha-short = A
color-picker-hue-short = H
color-picker-saturation-short = S
color-picker-value-short = V
color-picker-hex-label = Hex
color-picker-hex-placeholder = #RRGGBB
color-picker-current-color-label = Selected color
color-picker-current-color-readout = Selected color { $hex }
color-picker-swatches-name = Color presets
color-picker-swatch-label = Swatch { $hex }
color-picker-swatch-selected-suffix = , selected
color-picker-changed-announcement = Color changed to { $hex }
color-picker-done-label = Done
color-picker-cancel-label = Cancel
color-edit-trigger-name = Color { $hex }
color-edit-trigger-name-empty = Color, none
color-edit-trigger-tooltip = Open color picker
hex-color-input-invalid = Not a valid hex color (expected #RRGGBB)
hex-color-input-invalid-with-alpha = Not a valid hex color (expected #RRGGBB or #RRGGBBAA)
hex-color-input-corrected-shortform = Expanded { $raw } to { $value }
hex-color-input-corrected-uppercase = Normalized to { $value }
hex-color-input-placeholder = #RRGGBB
hex-color-input-placeholder-with-alpha = #RRGGBBAA
color-edit-trigger-empty-placeholder = —

# Rich tooltip "more" disclosure label (the Accordion title revealing the
# long-form body inside a sticky rich tooltip).
tooltip-more = More

# Built-in text-field / rich-text context menu items.
menu-cut = Cut
menu-copy = Copy
menu-paste = Paste
menu-paste-unformatted = Paste Unformatted
menu-select-all = Select All
menu-toggle-blockquote = Toggle blockquote
menu-remove-blockquote = Remove blockquote

# DropZone live-region announcements. An OS drag-drop has no AT verb, so
# a Live::Polite status line narrates hover / drop for screen readers.
# Singular vs plural is selected in Rust (not a Fluent select expression)
# so the `tr_widget!` compile-time English fallback works for apps that
# don't register the framework bundle. Consumed by
# crates/teksilo-widgets/src/drop_zone.rs.
drop-zone-hover-file-one = Drop to add 1 file
drop-zone-hover-file-many = Drop to add { $count } files
drop-zone-hover-text = Drop to add text
drop-zone-hover-link-one = Drop to add 1 link
drop-zone-hover-link-many = Drop to add { $count } links
drop-zone-hover-generic = Drop here
drop-zone-hover-reject = This item can't be dropped here
drop-zone-added-file-one = Added 1 file
drop-zone-added-file-many = Added { $count } files
drop-zone-added-text = Added text
drop-zone-added-link-one = Added 1 link
drop-zone-added-link-many = Added { $count } links
drop-zone-rejected = Item not accepted

# ThemeSwitcher widget. See crates/teksilo-widgets/src/theme_switcher.rs.
theme-switcher-label = Theme
theme-switcher-light = Light
theme-switcher-dark = Dark
theme-switcher-system = System

# FontPicker widget. See crates/teksilo-widgets/src/font_picker.rs.
font-picker-label = Font
font-picker-placeholder = Select a font…

# Settings-write-failure toast. Enqueued by
# `ToastRegistry::show_settings_write_failed` when a `teksilo-settings`
# background write permanently discards its queued patches
# (`AppEvent::SettingsWriteFailed`), wired up by
# `teksilo::install_toast`'s app-event observer in
# crates/teksilo/src/toast_install.rs. This reports real data loss — a
# preference change, a recents entry, a window position — not a status
# blip, so the toast is Error severity and persistent (no auto-dismiss).
settings-write-failed-toast-title = Settings couldn't be saved
settings-write-failed-toast-body = { $file } failed to save after { $attempts } attempts; { $dropped } queued change(s) discarded. { $message }

# Fallback window menu, opened by right-clicking a custom TitleBar on
# platforms with no OS window menu (X11 — winit's show_window_menu is a stub
# there and KWin does not implement _GTK_SHOW_WINDOW_MENU). See
# crates/teksilo-widgets/src/title_bar/window_menu.rs. Restore and Maximize
# are mutually exclusive; only one is shown at a time.
window-menu-restore = Restore
window-menu-maximize = Maximize
window-menu-minimize = Minimize
window-menu-close = Close

# Toast body disclosure. A toast body long enough to need more than
# `TOAST_BODY_COLLAPSED_LINES` lines is clamped and gets this thin toggle row —
# without it a raw error chain (paths, causes) can grow a toast to a quarter of
# the window. See crates/teksilo-widgets/src/toast/body.rs.
toast-show-more = Show more
toast-show-less = Show less
toast-copy-body = Copy
toast-body-copied = Copied
