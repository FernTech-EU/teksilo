# fern-widgets framework strings — French translation.
#
# Runtime-only: applications that register this locale via
# `I18nConfig::framework_locales(fern_widgets::framework_locales())`
# get these translations alongside en-US. Keys missing from fr-FR
# fall back to the en-US source via `I18nManager::resolve_widget`'s
# manual fallback chain (app override active → framework active →
# app override source → framework source → key placeholder). This is
# fern-i18n's own fallback, not `fluent-bundle`'s built-in per-key
# fallback — each `FluentBundle` is constructed with a single locale
# in its chain, and the multi-locale lookup is handled at the
# `I18nManager` layer.

a11y-status-bar-name = État
a11y-dialog-name = Boîte de dialogue
a11y-snackbar-name = Notification
a11y-split-view-divider-name = Séparateur de vue divisée
a11y-breadcrumb-current-page-value = page actuelle
a11y-builtin-browse = Parcourir
a11y-builtin-expand = Agrandir
a11y-builtin-search = Rechercher
a11y-builtin-copy = Copier
a11y-builtin-clear = Effacer
a11y-builtin-add = Ajouter
a11y-builtin-visibility = Afficher/masquer
keystroke-modifier-ctrl = Ctrl
keystroke-modifier-shift = Maj
keystroke-modifier-alt = Alt
keystroke-modifier-super = Super
keystroke-separator = +
