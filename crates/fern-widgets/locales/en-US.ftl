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
