# fern-widgets framework strings.
#
# These are the English source-language messages for framework-internal
# user-facing strings — accessibility names, descriptions, and similar
# labels that originate in fern-widgets source code rather than in
# application code.
#
# The proc macro `tr_widget!` (exported from `fern-i18n-macros` and
# re-exported via `fern-i18n::tr_widget`) validates every invocation in
# fern-widgets source against this file at compile time. At runtime, the
# framework bundle is registered automatically by `FernAppBuilder` (see
# architecture §12.13).
#
# Applications can override individual keys via
# `I18nConfig::override_widget_strings(...)` (deferred — slot reserved but
# not yet wired).

a11y-status-bar-name = Status
a11y-dialog-name = Dialog
a11y-snackbar-name = Snackbar
a11y-split-view-divider-name = Split view divider
a11y-breadcrumb-current-page-value = current page
