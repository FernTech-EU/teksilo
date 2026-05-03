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
a11y-shortcut-settings-name = Paramètres des raccourcis
a11y-shortcut-settings-capture-hint = Appuyez sur une touche. Suppr pour effacer. Échap pour annuler.
keystroke-modifier-ctrl = Ctrl
keystroke-modifier-shift = Maj
keystroke-modifier-alt = Alt
keystroke-modifier-super = Super
keystroke-separator = +
keystroke-key-space = Espace
keystroke-key-enter = Entrée
keystroke-key-escape = Échap
keystroke-key-tab = Tab
keystroke-key-backspace = Retour
keystroke-key-delete = Suppr
keystroke-key-arrow-up = Haut
keystroke-key-arrow-down = Bas
keystroke-key-arrow-left = Gauche
keystroke-key-arrow-right = Droite
keystroke-key-home = Début
keystroke-key-end = Fin
keystroke-key-page-up = Pg.préc
keystroke-key-page-down = Pg.suiv

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

# Widget PrivacySettings. Voir crates/fern-widgets/src/privacy_settings.rs.
# Mention d'information RGPD Art. 13 + boutons d'action. Les clés à
# paramètres utilisent la syntaxe Fluent { $nom }.
privacy-not-configured = La télémétrie n'est pas configurée pour cette application.
privacy-heading = Confidentialité et télémétrie
privacy-notice-controller = Les données sont traitées par { $processor } ; le sous-traitant technique est { $adapter } (point de collecte : { $endpoint }).
privacy-notice-purposes = Finalités : amélioration de l'application — quelles fonctionnalités sont utilisées, où se concentrent les bugs, sur quelles plates-formes l'application tourne. Aucun contenu de document, ni presse-papiers, ni frappe clavier, ni capture d'écran.
privacy-notice-lawful-anonymous = Base légale : notre intérêt légitime à améliorer le produit (RGPD Art. 6(1)(f) ; exemption CNIL « mesure d'audience »).
privacy-notice-lawful-pseudonymous = Base légale : votre consentement explicite (RGPD Art. 6(1)(a)).
privacy-notice-retention = Conservation : les données côté serveur sont conservées au plus { $days } jours.
privacy-notice-withdrawal-right = Droit de retrait : vous pouvez désactiver à tout moment les bascules ci-dessous, cliquer sur « Retirer le consentement » pour interrompre toute collecte, ou en mode pseudonyme cliquer sur « Effacer mes données » pour supprimer les enregistrements du serveur.
privacy-notice-policy-link = Politique de confidentialité complète : { $url }

privacy-scope-section-heading = Que peut partager l'application ?
privacy-scope-anonymous-metrics-label = Statistiques d'usage anonymes
privacy-scope-anonymous-metrics-description = Comptage des boutons / menus / raccourcis utilisés, version de l'application et système d'exploitation.
privacy-scope-crash-reports-label = Rapports de plantage
privacy-scope-crash-reports-description = Traces d'appel et métadonnées du processus en cas de plantage. Aucun contenu de document, aucun chemin de fichier.
privacy-scope-feature-flags-label = Drapeaux de fonctionnalités
privacy-scope-feature-flags-description = Permet à l'application de recevoir des mises à jour de drapeaux de fonctionnalités (déploiement progressif d'outils).

privacy-btn-reject-all = Tout refuser
privacy-btn-accept-all = Tout accepter
privacy-btn-erase = Effacer mes données
privacy-btn-erase-tooltip = Demande au serveur de supprimer tous les événements enregistrés pour cette installation, puis retire le consentement localement.
privacy-btn-fetch = Récupérer mes données
privacy-btn-fetch-tooltip = Récupère tous les événements que le serveur a enregistrés sous votre identifiant d'installation. Le résultat peut être enregistré au format JSON.
privacy-btn-withdraw = Retirer le consentement
privacy-btn-withdraw-tooltip = Interrompt toute nouvelle collecte. Les données déjà enregistrées sur le serveur sont conservées — utilisez « Effacer mes données » d'abord si vous souhaitez les supprimer.
privacy-btn-switch-to-anonymous = Basculer en mode Anonyme
privacy-btn-switch-to-pseudonymous = Basculer en mode Pseudonyme

privacy-identity-heading = Vos données sur le serveur
privacy-identity-install-id = Identifiant d'installation : { $id }
privacy-identity-retention = Le serveur conserve vos enregistrements au plus { $days } jours.

privacy-mode-heading = Mode de confidentialité
privacy-mode-current-anonymous = Actuel : Anonyme (aucun identifiant d'installation)
privacy-mode-current-pseudonymous = Actuel : Pseudonyme (identifiant d'installation présent)
privacy-mode-blurb-anonymous = Le mode anonyme ne transmet aucun identifiant par appareil. Basculer effacera vos enregistrements côté serveur et supprimera l'UUID d'installation local — cette action est irréversible.
privacy-mode-blurb-pseudonymous = Le mode pseudonyme génère un UUID d'installation aléatoire. Vous pourrez récupérer ou effacer vos enregistrements côté serveur. Nécessite un consentement explicite et redemande votre choix lors du basculement.

privacy-confirm-mode-switch-title = Changer de mode de confidentialité ?
privacy-confirm-mode-switch-leaving-pseudonymous = Cette action demandera au serveur d'effacer tous les événements enregistrés sous votre identifiant d'installation, supprimera l'UUID d'installation local, réinitialisera votre décision de consentement et changera le mode de confidentialité. Voulez-vous continuer ?
privacy-confirm-mode-switch-leaving-anonymous = Cette action réinitialisera votre décision de consentement et changera le mode de confidentialité. Vous serez à nouveau invité avant toute nouvelle collecte. Continuer ?
privacy-confirm-erase-title = Effacer vos données ?
privacy-confirm-erase-text = Cette action envoie une demande de suppression pour chaque événement enregistré sous votre identifiant d'installation, supprime tout ce qui est encore en mémoire tampon localement, et retire le consentement pour qu'aucune autre donnée ne soit collectée. L'action ne peut pas être annulée.
privacy-confirm-withdraw-title = Retirer le consentement ?
privacy-confirm-withdraw-text = Aucun nouvel événement d'analyse ne sera collecté depuis cette application. Les données déjà enregistrées sur le serveur sont conservées — utilisez « Effacer mes données » avant de retirer le consentement si vous souhaitez les supprimer également.

privacy-fetch-success-title = Vos données sur le serveur
privacy-fetch-success-text = { $count } événements récupérés pour cette installation.
privacy-fetch-saved-to = Enregistré dans : { $path }
privacy-fetch-write-error = Impossible d'écrire le fichier { $path } : { $error }
privacy-fetch-error-title = Impossible de récupérer vos données

privacy-inspect-title = Inspecter les données envoyées ({ $count } événement(s) en mémoire)
privacy-inspect-empty = Aucun événement n'a encore été émis dans cette session. Interagissez avec l'application — clics, menus et raccourcis passent tous par ici.
privacy-inspect-summary = Affichage des { $count } derniers événements, du plus récent au plus ancien.

# Calendrier / DateEdit / TimeEdit / DateTimeEdit. Voir
# crates/fern-widgets/src/{calendar,date_edit,time_edit,date_time_edit}.rs
# et les modules communs sous crates/fern-widgets/src/common/datetime/.
calendar-month-long-january = janvier
calendar-month-long-february = février
calendar-month-long-march = mars
calendar-month-long-april = avril
calendar-month-long-may = mai
calendar-month-long-june = juin
calendar-month-long-july = juillet
calendar-month-long-august = août
calendar-month-long-september = septembre
calendar-month-long-october = octobre
calendar-month-long-november = novembre
calendar-month-long-december = décembre

calendar-month-short-january = janv.
calendar-month-short-february = févr.
calendar-month-short-march = mars
calendar-month-short-april = avr.
calendar-month-short-may = mai
calendar-month-short-june = juin
calendar-month-short-july = juil.
calendar-month-short-august = août
calendar-month-short-september = sept.
calendar-month-short-october = oct.
calendar-month-short-november = nov.
calendar-month-short-december = déc.

calendar-weekday-long-monday = lundi
calendar-weekday-long-tuesday = mardi
calendar-weekday-long-wednesday = mercredi
calendar-weekday-long-thursday = jeudi
calendar-weekday-long-friday = vendredi
calendar-weekday-long-saturday = samedi
calendar-weekday-long-sunday = dimanche

calendar-weekday-short-monday = lun.
calendar-weekday-short-tuesday = mar.
calendar-weekday-short-wednesday = mer.
calendar-weekday-short-thursday = jeu.
calendar-weekday-short-friday = ven.
calendar-weekday-short-saturday = sam.
calendar-weekday-short-sunday = dim.

calendar-weekday-narrow-monday = L
calendar-weekday-narrow-tuesday = M
calendar-weekday-narrow-wednesday = M
calendar-weekday-narrow-thursday = J
calendar-weekday-narrow-friday = V
calendar-weekday-narrow-saturday = S
calendar-weekday-narrow-sunday = D

calendar-button-previous-month = Mois précédent
calendar-button-next-month = Mois suivant
calendar-button-previous-year = Année précédente
calendar-button-next-year = Année suivante
calendar-button-today = Aujourd'hui
calendar-button-month-picker = Choisir le mois
calendar-button-year-picker = Choisir l'année
calendar-week-number-column = Sem.
calendar-name = Calendrier
calendar-name-with-month = Calendrier, { $month } { $year }
calendar-cell-name = { $weekday } { $day } { $month } { $year }
calendar-range-status = Sélection : { $start } – { $end }
calendar-months-grid-label = Mois
calendar-years-grid-label = Années

date-edit-segment-year = Année
date-edit-segment-month = Mois
date-edit-segment-day = Jour
date-edit-calendar-button = Choisir une date
date-edit-name = Date
date-edit-placeholder = Sélectionner une date

time-edit-segment-hour = Heure
time-edit-segment-minute = Minute
time-edit-segment-second = Seconde
time-edit-segment-period = AM/PM
time-edit-period-am = AM
time-edit-period-pm = PM
time-edit-name = Heure
time-edit-placeholder = Sélectionner une heure

date-time-edit-name = Date et heure
date-time-edit-placeholder = Sélectionner la date et l'heure
date-range-edit-name = Plage de dates
date-range-edit-placeholder = Sélectionner une plage de dates

# Validation feedback (TextInputField + DateEdit/TimeEdit)
validation-corrected-to = Corrigé en { $value }
validation-corrected-with-notes = Corrigé : { $notes }
validation-segment-clamped = { $segment } { $raw } → { $clamped }
validation-day-clamped-to-month = jour { $raw } → { $clamped } (dernier jour du mois)
validation-clamped-to-range = ramené à la plage autorisée
validation-segment-year = année
validation-segment-month = mois
validation-segment-day = jour
validation-segment-hour = heure
validation-segment-minute = minute
validation-segment-second = seconde
validation-segment-value = valeur
date-edit-validation-not-a-date = Date invalide
time-edit-validation-not-a-time = Heure invalide
