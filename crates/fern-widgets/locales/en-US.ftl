# fern-widgets framework strings.
#
# These are the English source-language messages for framework-internal
# user-facing strings — accessibility names, descriptions, and similar
# labels that originate in fern-widgets source code rather than in
# application code.
#
# The proc macro `tr_widget!` (exported from `fern-i18n-macros` and
# re-exported via `fern-i18n::tr_widget`) validates every invocation in
# fern-widgets source against this file at compile time.
#
# At runtime, applications opt into the framework bundle via
# `I18nConfig::framework_locales(fern_widgets::framework_locales())`
# on the builder chain. This is **not** automatic — fern-app is
# deliberately widget-agnostic, so each application that uses
# fern-widgets must register the bundle explicitly. Applications that
# don't register the bundle still see the correct English text via the
# macro's compile-time fallback (see architecture §12.13.3 for the
# deviation from the spec's auto-registration).
#
# Applications can also override individual keys via
# `I18nConfig::override_widget_strings(...)` — those overrides win over
# the framework bundle in the §12.13.5 lookup precedence. Use this to
# ship a Japanese translation of the a11y labels when fern-widgets
# itself only ships English and French.

a11y-status-bar-name = Status
a11y-dialog-name = Dialog
a11y-snackbar-name = Snackbar
a11y-split-view-divider-name = Split view divider
a11y-breadcrumb-current-page-value = current page
a11y-toolbar-name = Toolbar
a11y-title-bar-name = Window title bar
a11y-window-controls-name = Window controls
a11y-window-minimize-name = Minimize
a11y-window-maximize-name = Maximize
a11y-window-restore-name = Restore
a11y-window-close-name = Close
a11y-wizard-progress-name = Wizard progress
a11y-wizard-content-name = Wizard content
a11y-builtin-browse = Browse
a11y-builtin-expand = Expand
a11y-builtin-search = Search
a11y-builtin-copy = Copy
a11y-builtin-clear = Clear
a11y-builtin-add = Add
a11y-builtin-visibility = Toggle visibility
a11y-shortcut-settings-name = Shortcut settings
a11y-shortcut-settings-capture-hint = Press any key. Delete to clear. Escape to cancel.
keystroke-modifier-ctrl = Ctrl
keystroke-modifier-shift = Shift
keystroke-modifier-alt = Alt
keystroke-modifier-super = Super
keystroke-separator = +

# MessageBox standard button labels. These are the canonical English
# labels used when a caller doesn't override with
# `MessageBoxButton::label`. See `crates/fern-widgets/src/message_box.rs`.
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
# `crates/fern-widgets/src/privacy_settings.rs`. RGPD/GDPR Art. 13
# disclosure plus action buttons. Param-bearing keys use Fluent
# variable syntax `{ $name }`.
privacy-not-configured = Telemetry is not configured for this application.
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
