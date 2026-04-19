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
a11y-toolbar-name = Barre d'outils
a11y-title-bar-name = Barre de titre de la fenêtre
a11y-window-controls-name = Contrôles de la fenêtre
a11y-window-minimize-name = Réduire
a11y-window-maximize-name = Agrandir
a11y-window-restore-name = Restaurer
a11y-window-close-name = Fermer
a11y-wizard-progress-name = Progression de l'assistant
a11y-wizard-content-name = Contenu de l'assistant
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

# MessageBox — boutons standards et divulgation des détails.
messagebox-btn-ok = OK
messagebox-btn-cancel = Annuler
messagebox-btn-close = Fermer
messagebox-btn-yes = Oui
messagebox-btn-no = Non
messagebox-btn-yes-to-all = Oui à tout
messagebox-btn-no-to-all = Non à tout
messagebox-btn-save = Enregistrer
messagebox-btn-save-all = Tout enregistrer
messagebox-btn-discard = Ignorer les modifications
messagebox-btn-apply = Appliquer
messagebox-btn-reset = Réinitialiser
messagebox-btn-restore-defaults = Valeurs par défaut
messagebox-btn-abort = Abandonner
messagebox-btn-retry = Réessayer
messagebox-btn-ignore = Ignorer
messagebox-btn-open = Ouvrir
messagebox-btn-help = Aide
messagebox-show-details = Afficher les détails
