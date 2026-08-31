# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

# Frameworkteksten van teksilo-widgets — Nederlandse vertaling.
#
# Alleen tijdens runtime: toepassingen die deze locale registreren via
# `I18nConfig::framework_locales(teksilo_widgets::framework_locales())`
# krijgen deze vertalingen naast en-US. Sleutels die in nl-NL ontbreken
# vallen terug op de bron en-US via de handmatige terugvalketen van
# `I18nManager::resolve_widget` (app-override actief → framework actief →
# app-override bron → framework bron → sleutelplaatshouder). Dit is de
# eigen terugval van teksilo-i18n, niet de ingebouwde terugval per sleutel
# van `fluent-bundle` — elke `FluentBundle` wordt met één enkele locale in
# zijn keten opgebouwd, en het opzoeken over meerdere locales gebeurt op
# het niveau van `I18nManager`.

a11y-status-bar-name = Status
a11y-dialog-name = Dialoogvenster
a11y-tooltip-name = Knopinfo
a11y-snackbar-name = Melding
a11y-splitter-divider-name = Splitsbalk
a11y-splitter-pane = Deelvenster
a11y-splitter-collapsed = Samengevouwen
a11y-splitter-expanded = Uitgevouwen
a11y-breadcrumb-current-page-value = huidige pagina
a11y-toolbar-name = Werkbalk
toolbar-more = Meer
segmented-control-more = Meer opties
breadcrumb-overflow = Verborgen pad tonen
a11y-title-bar-name = Titelbalk van het venster
a11y-window-controls-name = Vensterknoppen
a11y-window-minimize-name = Minimaliseren
a11y-window-maximize-name = Maximaliseren
a11y-window-restore-name = Herstellen
a11y-window-close-name = Sluiten
a11y-stepper-indicator-strip-name = Stappen
a11y-stepper-content-name = Inhoud van de stap
tab-close-tooltip = Tabblad sluiten
a11y-builtin-browse = Bladeren
a11y-builtin-expand = Vergroten
a11y-builtin-search = Zoeken
a11y-builtin-copy = Kopiëren
a11y-builtin-clear = Wissen
a11y-builtin-add = Toevoegen
a11y-builtin-bell = Meldingen
a11y-builtin-menu = Menu
a11y-builtin-more = Meer acties
a11y-builtin-visibility = Zichtbaarheid in- of uitschakelen
a11y-password-reveal = Wachtwoord tonen of verbergen
a11y-caps-lock-on = Caps Lock staat aan
notifications-title = Meldingen
notifications-empty = Geen meldingen
notifications-mark-all-read = Alles als gelezen markeren
notifications-clear = Alles wissen
notifications-filter-placeholder = Meldingen zoeken
notifications-bucket-today = Vandaag
notifications-bucket-yesterday = Gisteren
notifications-bucket-this-week = Deze week
notifications-bucket-earlier = Eerder
notifications-archive-replay-disabled = (niet meer beschikbaar)
a11y-shortcut-settings-name = Sneltoetsinstellingen
a11y-shortcut-settings-capture-hint = Druk op een toets. Del om te wissen. Esc om te annuleren.
keystroke-modifier-ctrl = Ctrl
keystroke-modifier-shift = Shift
keystroke-modifier-alt = Alt
keystroke-modifier-super = Super
keystroke-separator = +
keystroke-key-space = Spatie
keystroke-key-enter = Enter
keystroke-key-escape = Esc
keystroke-key-tab = Tab
keystroke-key-backspace = Backspace
keystroke-key-delete = Del
keystroke-key-arrow-up = Omhoog
keystroke-key-arrow-down = Omlaag
keystroke-key-arrow-left = Links
keystroke-key-arrow-right = Rechts
keystroke-key-home = Home
keystroke-key-end = End
keystroke-key-page-up = PgUp
keystroke-key-page-down = PgDn

# MessageBox — standaardknoppen en het tonen van details.
messagebox-btn-ok = OK
messagebox-btn-cancel = Annuleren
messagebox-btn-close = Sluiten
messagebox-btn-yes = Ja
messagebox-btn-no = Nee
messagebox-btn-yes-to-all = Ja op alles
messagebox-btn-no-to-all = Nee op alles
messagebox-btn-save = Opslaan
messagebox-btn-save-all = Alles opslaan
messagebox-btn-discard = Wijzigingen negeren
messagebox-btn-apply = Toepassen
messagebox-btn-reset = Herstellen
messagebox-btn-restore-defaults = Standaardwaarden herstellen
messagebox-btn-abort = Afbreken
messagebox-btn-retry = Opnieuw proberen
messagebox-btn-ignore = Negeren
messagebox-btn-open = Openen
messagebox-btn-help = Help
messagebox-show-details = Details tonen

# Widget PrivacySettings. Zie crates/teksilo-widgets/src/privacy_settings.rs.
# Informatieverstrekking volgens AVG art. 13 + actieknoppen. Sleutels met
# parameters gebruiken de Fluent-syntaxis { $naam }.
privacy-not-configured = Telemetrie is niet geconfigureerd voor deze toepassing.
privacy-a11y-group-name = Instellingen voor privacy en telemetrie
privacy-heading = Privacy en telemetrie
privacy-notice-controller = De gegevens worden verwerkt door { $processor }; de technische verwerker is { $adapter } (eindpunt: { $endpoint }).
privacy-notice-purposes = Doeleinden: verbetering van de toepassing — welke functies worden gebruikt, waar fouten zich opstapelen, op welke platformen we draaien. Geen documentinhoud, geen klembord, geen toetsaanslagen, geen schermafbeeldingen.
privacy-notice-lawful-anonymous = Rechtsgrond: ons gerechtvaardigd belang bij productverbetering (AVG art. 6, lid 1, onder f; CNIL-vrijstelling voor publieksmeting).
privacy-notice-lawful-pseudonymous = Rechtsgrond: uw uitdrukkelijke toestemming (AVG art. 6, lid 1, onder a).
privacy-notice-retention =
    Bewaartermijn: gegevens op de server worden maximaal { $days ->
        [one] { $days } dag
       *[other] { $days } dagen
    } bewaard.
privacy-notice-withdrawal-right = Recht op intrekking: u kunt de onderstaande schakelaars op elk moment uitzetten, op "Toestemming intrekken" klikken om alle verzameling te stoppen, of in de pseudonieme modus op "Mijn gegevens wissen" klikken om de records van de server te laten verwijderen.
privacy-notice-policy-link = Volledig privacybeleid: { $url }

privacy-scope-section-heading = Wat mag de toepassing delen?
privacy-scope-anonymous-metrics-label = Anonieme gebruiksstatistieken
privacy-scope-anonymous-metrics-description = Hoe vaak knoppen, menu-items en sneltoetsen worden gebruikt, plus de versie van de toepassing en het besturingssysteem.
privacy-scope-crash-reports-label = Crashrapporten
privacy-scope-crash-reports-description = Stacktraces en procesmetagegevens wanneer de toepassing vastloopt. Geen documentinhoud, geen bestandspaden.
privacy-scope-feature-flags-label = Functievlaggen
privacy-scope-feature-flags-description = Stelt de toepassing in staat updates van functievlaggen te ontvangen (bijvoorbeeld de geleidelijke uitrol van nieuwe hulpmiddelen).

privacy-btn-reject-all = Alles weigeren
privacy-btn-accept-all = Alles accepteren
privacy-btn-erase = Mijn gegevens wissen
privacy-btn-erase-tooltip = Vraagt de server om elke gebeurtenis die voor deze installatie is vastgelegd te wissen en trekt daarna lokaal de toestemming in.
privacy-btn-fetch = Mijn gegevens opvragen
privacy-btn-fetch-tooltip = Haalt elke gebeurtenis op die de server onder uw installatie-ID heeft vastgelegd. U kunt het resultaat als JSON opslaan.
privacy-btn-withdraw = Toestemming intrekken
privacy-btn-withdraw-tooltip = Stopt het verzamelen van nieuwe gegevens. Reeds vastgelegde gegevens op de server blijven bewaard — gebruik eerst "Mijn gegevens wissen" als u die wilt laten verwijderen.
privacy-btn-switch-to-anonymous = Overschakelen naar de anonieme modus
privacy-btn-switch-to-pseudonymous = Overschakelen naar de pseudonieme modus

privacy-identity-heading = Uw gegevens op de server
privacy-identity-install-id = Installatie-ID: { $id }
privacy-identity-retention =
    De server bewaart uw records maximaal { $days ->
        [one] { $days } dag
       *[other] { $days } dagen
    }.

privacy-mode-heading = Privacymodus
privacy-mode-current-anonymous = Nu: anoniem (geen installatie-ID)
privacy-mode-current-pseudonymous = Nu: pseudoniem (installatie-ID aanwezig)
privacy-mode-blurb-anonymous = De anonieme modus verzendt geen identificatie per apparaat. Overschakelen wist uw bestaande records op de server en verwijdert de lokale installatie-UUID — dit kan niet ongedaan worden gemaakt.
privacy-mode-blurb-pseudonymous = De pseudonieme modus genereert een willekeurige installatie-UUID. U kunt uw records op de server dan opvragen of wissen. Vereist uitdrukkelijke toestemming en vraagt bij het overschakelen opnieuw om uw keuze.

privacy-confirm-mode-switch-title = Privacymodus wijzigen?
privacy-confirm-mode-switch-leaving-pseudonymous = Hiermee wordt de server gevraagd elke gebeurtenis die onder uw installatie-ID is vastgelegd te wissen, wordt de lokale installatie-UUID verwijderd, wordt uw toestemmingskeuze opnieuw ingesteld en wordt de privacymodus gewijzigd. Wilt u doorgaan?
privacy-confirm-mode-switch-leaving-anonymous = Hiermee wordt uw toestemmingskeuze opnieuw ingesteld en wordt de privacymodus gewijzigd. U wordt opnieuw om toestemming gevraagd voordat er nieuwe gegevens worden verzameld. Doorgaan?
privacy-confirm-erase-title = Uw gegevens wissen?
privacy-confirm-erase-text = Hiermee wordt een verwijderingsverzoek verstuurd voor elke gebeurtenis die onder uw installatie-ID is vastgelegd, wordt alles wat lokaal nog in de buffer staat verwijderd en wordt de toestemming ingetrokken zodat er geen gegevens meer worden verzameld. Deze actie kan niet ongedaan worden gemaakt.
privacy-confirm-withdraw-title = Toestemming intrekken?
privacy-confirm-withdraw-text = Er worden vanuit deze toepassing geen analysegebeurtenissen meer verzameld. Reeds vastgelegde gegevens op de server blijven bewaard — gebruik "Mijn gegevens wissen" voordat u de toestemming intrekt als u die ook wilt laten verwijderen.

privacy-fetch-success-title = Uw gegevens op de server
privacy-fetch-success-text = Aantal opgehaalde gebeurtenissen voor deze installatie: { $count }.
privacy-fetch-saved-to = Opgeslagen in: { $path }
privacy-fetch-write-error = Kan bestand { $path } niet schrijven: { $error }
privacy-fetch-error-title = Kan uw gegevens niet ophalen

privacy-inspect-title = Verzonden gegevens inspecteren (aantal gebufferde gebeurtenissen: { $count })
privacy-inspect-empty = Er zijn in deze sessie nog geen gebeurtenissen verzonden. Werk even met de toepassing — klikken, menu's en sneltoetsen komen hier allemaal langs.
privacy-inspect-summary = Weergave van de laatste gebeurtenissen (aantal: { $count }), nieuwste eerst.

# Kalender / DateEdit / TimeEdit / DateTimeEdit. Zie
# crates/teksilo-widgets/src/{calendar,date_edit,time_edit,date_time_edit}.rs
# en de gedeelde modules onder crates/teksilo-widgets/src/common/datetime/.
calendar-month-long-january = januari
calendar-month-long-february = februari
calendar-month-long-march = maart
calendar-month-long-april = april
calendar-month-long-may = mei
calendar-month-long-june = juni
calendar-month-long-july = juli
calendar-month-long-august = augustus
calendar-month-long-september = september
calendar-month-long-october = oktober
calendar-month-long-november = november
calendar-month-long-december = december

calendar-month-short-january = jan
calendar-month-short-february = feb
calendar-month-short-march = mrt
calendar-month-short-april = apr
calendar-month-short-may = mei
calendar-month-short-june = jun
calendar-month-short-july = jul
calendar-month-short-august = aug
calendar-month-short-september = sep
calendar-month-short-october = okt
calendar-month-short-november = nov
calendar-month-short-december = dec

calendar-weekday-long-monday = maandag
calendar-weekday-long-tuesday = dinsdag
calendar-weekday-long-wednesday = woensdag
calendar-weekday-long-thursday = donderdag
calendar-weekday-long-friday = vrijdag
calendar-weekday-long-saturday = zaterdag
calendar-weekday-long-sunday = zondag

calendar-weekday-short-monday = ma
calendar-weekday-short-tuesday = di
calendar-weekday-short-wednesday = wo
calendar-weekday-short-thursday = do
calendar-weekday-short-friday = vr
calendar-weekday-short-saturday = za
calendar-weekday-short-sunday = zo

calendar-weekday-narrow-monday = M
calendar-weekday-narrow-tuesday = D
calendar-weekday-narrow-wednesday = W
calendar-weekday-narrow-thursday = D
calendar-weekday-narrow-friday = V
calendar-weekday-narrow-saturday = Z
calendar-weekday-narrow-sunday = Z

calendar-button-previous-month = Vorige maand
calendar-button-next-month = Volgende maand
calendar-button-previous-year = Vorig jaar
calendar-button-next-year = Volgend jaar
calendar-button-today = Vandaag
calendar-button-month-picker = Maand kiezen
calendar-button-year-picker = Jaar kiezen
calendar-week-number-column = Wk
calendar-name = Kalender
calendar-months-grid-label = Maanden
calendar-years-grid-label = Jaren
calendar-name-with-month = Kalender, { $month } { $year }
calendar-cell-name = { $weekday } { $day } { $month } { $year }
calendar-range-status = Geselecteerd: { $start } – { $end }

date-edit-segment-year = Jaar
date-edit-segment-month = Maand
date-edit-segment-day = Dag
date-edit-calendar-button = Datum kiezen
date-edit-trigger-tooltip = Kalender openen
date-edit-name = Datum
date-edit-placeholder = Selecteer een datum

time-edit-segment-hour = Uur
time-edit-segment-minute = Minuut
time-edit-segment-second = Seconde
time-edit-segment-period = a.m./p.m.
time-edit-period-am = a.m.
time-edit-period-pm = p.m.
time-edit-name = Tijd
time-edit-placeholder = Selecteer een tijd

date-time-edit-name = Datum en tijd
date-time-edit-placeholder = Selecteer datum en tijd
date-time-edit-date-name = Datum
date-time-edit-time-name = Tijd
date-time-edit-trigger-tooltip = Kalender openen
date-range-edit-name = Datumbereik
date-range-edit-placeholder = Selecteer een datumbereik
date-range-edit-start-name = Begindatum
date-range-edit-end-name = Einddatum
date-range-edit-trigger-tooltip = Bereikkalender openen

# Validatiemeldingen (TextInputField + DateEdit/TimeEdit)
validation-corrected-to = Automatisch gecorrigeerd naar { $value }
validation-corrected-with-notes = Automatisch gecorrigeerd: { $notes }
validation-segment-clamped = { $segment } { $raw } → { $clamped }
validation-day-clamped-to-month = dag { $raw } → { $clamped } (laatste dag van de maand)
validation-clamped-to-range = teruggebracht tot het toegestane bereik
validation-segment-year = jaar
validation-segment-month = maand
validation-segment-day = dag
validation-segment-hour = uur
validation-segment-minute = minuut
validation-segment-second = seconde
validation-segment-value = waarde
date-edit-validation-not-a-date = Ongeldige datum
time-edit-validation-not-a-time = Ongeldige tijd

# ── kleurkiezer ──
color-picker-name = Kleurkiezer
color-picker-hue-label = Tint
color-picker-saturation-label = Verzadiging
color-picker-value-label = Helderheid
color-picker-alpha-label = Dekking
color-picker-red-label = Rood
color-picker-green-label = Groen
color-picker-blue-label = Blauw
color-picker-red-short = R
color-picker-green-short = G
color-picker-blue-short = B
color-picker-alpha-short = A
color-picker-hue-short = T
color-picker-saturation-short = V
color-picker-value-short = H
color-picker-hex-label = Hex
color-picker-hex-placeholder = #RRGGBB
color-picker-current-color-label = Geselecteerde kleur
color-picker-current-color-readout = Geselecteerde kleur { $hex }
color-picker-swatches-name = Voorgedefinieerde kleuren
color-picker-swatch-label = Kleurstaal { $hex }
color-picker-swatch-selected-suffix = , geselecteerd
color-picker-changed-announcement = Kleur gewijzigd in { $hex }
color-picker-done-label = Gereed
color-picker-cancel-label = Annuleren
color-edit-trigger-name = Kleur { $hex }
color-edit-trigger-name-empty = Kleur, geen
color-edit-trigger-tooltip = Kleurkiezer openen
hex-color-input-invalid = Ongeldige hexadecimale kleurcode (verwacht: #RRGGBB)
hex-color-input-invalid-with-alpha = Ongeldige hexadecimale kleurcode (verwacht: #RRGGBB of #RRGGBBAA)
hex-color-input-corrected-shortform = { $raw } uitgebreid naar { $value }
hex-color-input-corrected-uppercase = Genormaliseerd naar { $value }
hex-color-input-placeholder = #RRGGBB
hex-color-input-placeholder-with-alpha = #RRGGBBAA
color-edit-trigger-empty-placeholder = —

# Label "meer" van de uitklap in rijke knopinfo (de accordeontitel die de
# uitgebreide tekst in een vastgezette rijke knopinfo laat zien).
tooltip-more = Meer

# Ingebouwde contextmenu-items van tekstvelden en de tekstbewerker met opmaak.
menu-cut = Knippen
menu-copy = Kopiëren
menu-paste = Plakken
menu-paste-unformatted = Plakken zonder opmaak
menu-select-all = Alles selecteren
menu-toggle-blockquote = Blokcitaat in- of uitschakelen
menu-remove-blockquote = Blokcitaat verwijderen

# DropZone — aankondigingen in de live-regio (schermlezers). Zie en-US.ftl
# voor de volledige context en crates/teksilo-widgets/src/drop_zone.rs.
drop-zone-hover-file-one = Neerzetten om 1 bestand toe te voegen
drop-zone-hover-file-many = Neerzetten om { $count } bestanden toe te voegen
drop-zone-hover-text = Neerzetten om tekst toe te voegen
drop-zone-hover-link-one = Neerzetten om 1 koppeling toe te voegen
drop-zone-hover-link-many = Neerzetten om { $count } koppelingen toe te voegen
drop-zone-hover-generic = Hier neerzetten
drop-zone-hover-reject = Dit item kan hier niet worden neergezet
drop-zone-added-file-one = 1 bestand toegevoegd
drop-zone-added-file-many = { $count } bestanden toegevoegd
drop-zone-added-text = Tekst toegevoegd
drop-zone-added-link-one = 1 koppeling toegevoegd
drop-zone-added-link-many = { $count } koppelingen toegevoegd
drop-zone-rejected = Item niet geaccepteerd

# Widget ThemeSwitcher. Zie crates/teksilo-widgets/src/theme_switcher.rs.
theme-switcher-label = Thema
theme-switcher-light = Licht
theme-switcher-dark = Donker
theme-switcher-system = Systeem

# Widget FontPicker. Zie crates/teksilo-widgets/src/font_picker.rs.
font-picker-label = Lettertype
font-picker-placeholder = Kies een lettertype…

# Melding bij een mislukte schrijfactie van de instellingen. Zie en-US.ftl
# voor de volledige context (uitgestuurd door
# ToastRegistry::show_settings_write_failed via teksilo::install_toast).
settings-write-failed-toast-title = Kan de instellingen niet opslaan
settings-write-failed-toast-body = Opslaan van { $file } is na { $attempts } pogingen mislukt; aantal genegeerde wijzigingen in de wachtrij: { $dropped }. { $message }

# Terugvalvenstermenu, geopend met een rechtermuisklik op een aangepaste
# TitleBar op platformen zonder venstermenu van het besturingssysteem (X11).
# Zie en-US.ftl voor de volledige context en
# crates/teksilo-widgets/src/title_bar/window_menu.rs.
window-menu-restore = Herstellen
window-menu-maximize = Maximaliseren
window-menu-minimize = Minimaliseren
window-menu-close = Sluiten

# Uitklappen van de tekst van een melding. Zie en-US.ftl voor de volledige
# context en crates/teksilo-widgets/src/toast/body.rs.
toast-show-more = Meer tonen
toast-show-less = Minder tonen
toast-copy-body = Kopiëren
toast-body-copied = Gekopieerd

# Opdrachtenpalet. Zie en-US.ftl voor de volledige context en
# crates/teksilo-widgets/src/command_palette.rs.
command-palette-placeholder = Typ een opdracht
command-palette-empty = Geen overeenkomende opdracht
command-palette-title = Opdrachtenpalet
command-palette-result-count =
    { $count ->
        [0] Geen overeenkomende opdracht
        [one] 1 opdracht
       *[other] { $count } opdrachten
    }
