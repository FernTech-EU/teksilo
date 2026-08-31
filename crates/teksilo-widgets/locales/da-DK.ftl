# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

# teksilo-widgets framework-strenge — dansk oversættelse.
#
# Kun ved kørsel: programmer, der registrerer denne lokalitet via
# `I18nConfig::framework_locales(teksilo_widgets::framework_locales())`,
# får disse oversættelser ved siden af en-US. Nøgler, der mangler i
# da-DK, falder tilbage til en-US-kilden via
# `I18nManager::resolve_widget`s manuelle fallback-kæde
# (programoverstyring aktiv → framework aktiv → programoverstyring kilde
# → framework kilde → nøgleerstatning). Det er teksilo-i18n's egen
# fallback, ikke `fluent-bundle`s indbyggede fallback pr. nøgle — hvert
# `FluentBundle` oprettes med én enkelt lokalitet i sin kæde, og
# opslag på tværs af lokaliteter håndteres i `I18nManager`-laget.

a11y-status-bar-name = Status
a11y-dialog-name = Dialogboks
a11y-tooltip-name = Værktøjstip
a11y-snackbar-name = Meddelelse
a11y-splitter-divider-name = Skillelinje
a11y-splitter-pane = Rude
a11y-splitter-collapsed = Sammenfoldet
a11y-splitter-expanded = Udvidet
a11y-breadcrumb-current-page-value = aktuel side
a11y-toolbar-name = Værktøjslinje
toolbar-more = Mere
segmented-control-more = Flere valgmuligheder
breadcrumb-overflow = Vis skjult sti
a11y-title-bar-name = Vinduets titellinje
a11y-window-controls-name = Vinduesknapper
a11y-window-minimize-name = Minimer
a11y-window-maximize-name = Maksimer
a11y-window-restore-name = Gendan
a11y-window-close-name = Luk
a11y-stepper-indicator-strip-name = Trin
a11y-stepper-content-name = Trinindhold
tab-close-tooltip = Luk fane
a11y-builtin-browse = Gennemse
a11y-builtin-expand = Udvid
a11y-builtin-search = Søg
a11y-builtin-copy = Kopiér
a11y-builtin-clear = Ryd
a11y-builtin-add = Tilføj
a11y-builtin-bell = Notifikationer
a11y-builtin-menu = Menu
a11y-builtin-more = Flere handlinger
a11y-builtin-visibility = Vis eller skjul
a11y-password-reveal = Vis eller skjul adgangskode
a11y-caps-lock-on = Caps Lock er slået til
notifications-title = Notifikationer
notifications-empty = Ingen notifikationer
notifications-mark-all-read = Markér alle som læst
notifications-clear = Ryd alle
notifications-filter-placeholder = Søg i notifikationer
notifications-bucket-today = I dag
notifications-bucket-yesterday = I går
notifications-bucket-this-week = Denne uge
notifications-bucket-earlier = Tidligere
notifications-archive-replay-disabled = (ikke længere tilgængelig)
a11y-shortcut-settings-name = Indstillinger for tastaturgenveje
a11y-shortcut-settings-capture-hint = Tryk på en vilkårlig tast. Delete rydder. Esc annullerer.
keystroke-modifier-ctrl = Ctrl
keystroke-modifier-shift = Skift
keystroke-modifier-alt = Alt
keystroke-modifier-super = Super
keystroke-separator = +
keystroke-key-space = Mellemrum
keystroke-key-enter = Enter
keystroke-key-escape = Esc
keystroke-key-tab = Tab
keystroke-key-backspace = Backspace
keystroke-key-delete = Del
keystroke-key-arrow-up = Op
keystroke-key-arrow-down = Ned
keystroke-key-arrow-left = Venstre
keystroke-key-arrow-right = Højre
keystroke-key-home = Home
keystroke-key-end = End
keystroke-key-page-up = PgUp
keystroke-key-page-down = PgDn

# MessageBox — standardknapper og visning af detaljer.
messagebox-btn-ok = OK
messagebox-btn-cancel = Annuller
messagebox-btn-close = Luk
messagebox-btn-yes = Ja
messagebox-btn-no = Nej
messagebox-btn-yes-to-all = Ja til alle
messagebox-btn-no-to-all = Nej til alle
messagebox-btn-save = Gem
messagebox-btn-save-all = Gem alle
messagebox-btn-discard = Kassér
messagebox-btn-apply = Anvend
messagebox-btn-reset = Nulstil
messagebox-btn-restore-defaults = Gendan standardindstillinger
messagebox-btn-abort = Afbryd
messagebox-btn-retry = Prøv igen
messagebox-btn-ignore = Ignorer
messagebox-btn-open = Åbn
messagebox-btn-help = Hjælp
messagebox-show-details = Vis detaljer

# PrivacySettings-widget. Se crates/teksilo-widgets/src/privacy_settings.rs.
# Oplysningspligt efter GDPR art. 13 + handlingsknapper. Nøgler med
# parametre bruger Fluent-syntaksen { $navn }.
privacy-not-configured = Telemetri er ikke konfigureret for dette program.
privacy-a11y-group-name = Indstillinger for privatliv og telemetri
privacy-heading = Privatliv og telemetri
privacy-notice-controller = Data behandles af { $processor }; den tekniske databehandler er { $adapter } (endepunkt: { $endpoint }).
privacy-notice-purposes = Formål: at forbedre programmet — hvilke funktioner der bruges, hvor fejlene samler sig, hvilke platforme vi kører på. Intet dokumentindhold, ingen udklipsholder, ingen tastetryk, ingen skærmbilleder.
privacy-notice-lawful-anonymous = Retsgrundlag: vores legitime interesse i at forbedre produktet (GDPR art. 6, stk. 1, litra f; CNIL's undtagelse for publikumsmåling).
privacy-notice-lawful-pseudonymous = Retsgrundlag: dit udtrykkelige samtykke (GDPR art. 6, stk. 1, litra a).
privacy-notice-retention = Opbevaring: data på serveren opbevares i højst { $days } døgn.
privacy-notice-withdrawal-right = Ret til at trække samtykket tilbage: du kan til enhver tid slå indstillingerne nedenfor fra, klikke på »Træk samtykke tilbage« for at standse al indsamling, eller i pseudonym tilstand klikke på »Slet mine data« for at slette registreringerne på serveren.
privacy-notice-policy-link = Fuld privatlivspolitik: { $url }

privacy-scope-section-heading = Hvad må programmet dele?
privacy-scope-anonymous-metrics-label = Anonyme brugsstatistikker
privacy-scope-anonymous-metrics-description = Optælling af hvilke knapper/menupunkter/genveje der bruges, samt programversion og operativsystem.
privacy-scope-crash-reports-label = Nedbrudsrapporter
privacy-scope-crash-reports-description = Kaldstakke og procesmetadata, når programmet går ned. Intet dokumentindhold, ingen filstier.
privacy-scope-feature-flags-label = Funktionsflag
privacy-scope-feature-flags-description = Gør det muligt for programmet at modtage opdateringer af funktionsflag (f.eks. gradvis udrulning af nye værktøjer).

privacy-btn-reject-all = Afvis alle
privacy-btn-accept-all = Accepter alle
privacy-btn-erase = Slet mine data
privacy-btn-erase-tooltip = Beder serveren om at slette alle hændelser, der er registreret for denne installation, og trækker derefter samtykket tilbage lokalt.
privacy-btn-fetch = Hent mine data
privacy-btn-fetch-tooltip = Henter alle de hændelser, serveren har registreret under dit installations-id. Resultatet kan gemmes som JSON.
privacy-btn-withdraw = Træk samtykke tilbage
privacy-btn-withdraw-tooltip = Standser al ny dataindsamling. Allerede registrerede data på serveren bevares — brug »Slet mine data« først, hvis de skal slettes.
privacy-btn-switch-to-anonymous = Skift til anonym tilstand
privacy-btn-switch-to-pseudonymous = Skift til pseudonym tilstand

privacy-identity-heading = Dine data på serveren
privacy-identity-install-id = Installations-id: { $id }
privacy-identity-retention = Serveren opbevarer dine registreringer i højst { $days } døgn.

privacy-mode-heading = Privatlivstilstand
privacy-mode-current-anonymous = Aktuelt: Anonym (intet installations-id)
privacy-mode-current-pseudonymous = Aktuelt: Pseudonym (installations-id findes)
privacy-mode-blurb-anonymous = Anonym tilstand sender ingen identifikator pr. enhed. Hvis du skifter, slettes dine eksisterende registreringer på serveren, og den lokale installations-UUID kasseres — det kan ikke fortrydes.
privacy-mode-blurb-pseudonymous = Pseudonym tilstand genererer en tilfældig installations-UUID. Du kan derefter hente eller slette dine registreringer på serveren. Kræver udtrykkeligt samtykke og spørger igen ved skift.

privacy-confirm-mode-switch-title = Vil du skifte privatlivstilstand?
privacy-confirm-mode-switch-leaving-pseudonymous = Dette beder serveren om at slette alle hændelser, der er registreret under dit installations-id, fjerner den lokale installations-UUID, nulstiller din samtykkebeslutning og skifter privatlivstilstand. Vil du fortsætte?
privacy-confirm-mode-switch-leaving-anonymous = Dette nulstiller din samtykkebeslutning og skifter privatlivstilstand. Du bliver spurgt igen, før der indsamles nye data. Vil du fortsætte?
privacy-confirm-erase-title = Vil du slette dine data?
privacy-confirm-erase-text = Dette sender en anmodning om sletning af hver hændelse, der er registreret under dit installations-id, fjerner alt, der stadig ligger i den lokale buffer, og trækker samtykket tilbage, så der ikke indsamles flere data. Handlingen kan ikke fortrydes.
privacy-confirm-withdraw-title = Vil du trække samtykket tilbage?
privacy-confirm-withdraw-text = Der indsamles ikke flere analysehændelser fra dette program. Allerede registrerede data på serveren bevares — brug »Slet mine data«, før du trækker samtykket tilbage, hvis de også skal slettes.

privacy-fetch-success-title = Dine data på serveren
privacy-fetch-success-text = Hentede hændelser for denne installation: { $count }.
privacy-fetch-saved-to = Gemt i: { $path }
privacy-fetch-write-error = Filen { $path } kunne ikke skrives: { $error }
privacy-fetch-error-title = Dine data kunne ikke hentes

privacy-inspect-title = Undersøg sendte data (hændelser i buffer: { $count })
privacy-inspect-empty = Der er endnu ikke sendt nogen hændelser i denne session. Prøv at bruge programmet — klik, menuer og genveje går alle gennem dette.
privacy-inspect-summary = Viser de seneste hændelser, nyeste først (antal: { $count }).

# Kalender / DateEdit / TimeEdit / DateTimeEdit. Se
# crates/teksilo-widgets/src/{calendar,date_edit,time_edit,date_time_edit}.rs
# og fællesmodulerne under crates/teksilo-widgets/src/common/datetime/.
calendar-month-long-january = januar
calendar-month-long-february = februar
calendar-month-long-march = marts
calendar-month-long-april = april
calendar-month-long-may = maj
calendar-month-long-june = juni
calendar-month-long-july = juli
calendar-month-long-august = august
calendar-month-long-september = september
calendar-month-long-october = oktober
calendar-month-long-november = november
calendar-month-long-december = december

calendar-month-short-january = jan.
calendar-month-short-february = feb.
calendar-month-short-march = mar.
calendar-month-short-april = apr.
calendar-month-short-may = maj
calendar-month-short-june = jun.
calendar-month-short-july = jul.
calendar-month-short-august = aug.
calendar-month-short-september = sep.
calendar-month-short-october = okt.
calendar-month-short-november = nov.
calendar-month-short-december = dec.

calendar-weekday-long-monday = mandag
calendar-weekday-long-tuesday = tirsdag
calendar-weekday-long-wednesday = onsdag
calendar-weekday-long-thursday = torsdag
calendar-weekday-long-friday = fredag
calendar-weekday-long-saturday = lørdag
calendar-weekday-long-sunday = søndag

calendar-weekday-short-monday = man.
calendar-weekday-short-tuesday = tirs.
calendar-weekday-short-wednesday = ons.
calendar-weekday-short-thursday = tors.
calendar-weekday-short-friday = fre.
calendar-weekday-short-saturday = lør.
calendar-weekday-short-sunday = søn.

calendar-weekday-narrow-monday = M
calendar-weekday-narrow-tuesday = T
calendar-weekday-narrow-wednesday = O
calendar-weekday-narrow-thursday = T
calendar-weekday-narrow-friday = F
calendar-weekday-narrow-saturday = L
calendar-weekday-narrow-sunday = S

calendar-button-previous-month = Forrige måned
calendar-button-next-month = Næste måned
calendar-button-previous-year = Forrige år
calendar-button-next-year = Næste år
calendar-button-today = I dag
calendar-button-month-picker = Vælg måned
calendar-button-year-picker = Vælg år
calendar-week-number-column = Uge
calendar-name = Kalender
calendar-months-grid-label = Måneder
calendar-years-grid-label = År
calendar-name-with-month = Kalender, { $month } { $year }
calendar-cell-name = { $weekday } den { $day }. { $month } { $year }
calendar-range-status = Valgt: { $start } – { $end }

date-edit-segment-year = År
date-edit-segment-month = Måned
date-edit-segment-day = Dag
date-edit-calendar-button = Vælg dato
date-edit-trigger-tooltip = Åbn kalender
date-edit-name = Dato
date-edit-placeholder = Vælg en dato

time-edit-segment-hour = Time
time-edit-segment-minute = Minut
time-edit-segment-second = Sekund
time-edit-segment-period = AM/PM
time-edit-period-am = AM
time-edit-period-pm = PM
time-edit-name = Klokkeslæt
time-edit-placeholder = Vælg et klokkeslæt

date-time-edit-name = Dato og klokkeslæt
date-time-edit-placeholder = Vælg dato og klokkeslæt
date-time-edit-date-name = Dato
date-time-edit-time-name = Klokkeslæt
date-time-edit-trigger-tooltip = Åbn kalender
date-range-edit-name = Datointerval
date-range-edit-placeholder = Vælg datointerval
date-range-edit-start-name = Startdato
date-range-edit-end-name = Slutdato
date-range-edit-trigger-tooltip = Åbn intervalkalender

# Valideringsbeskeder (TextInputField + DateEdit/TimeEdit)
validation-corrected-to = Rettet automatisk til { $value }
validation-corrected-with-notes = Rettet automatisk: { $notes }
validation-segment-clamped = { $segment } { $raw } → { $clamped }
validation-day-clamped-to-month = dag { $raw } → { $clamped } (sidste dag i måneden)
validation-clamped-to-range = begrænset til det tilladte interval
validation-segment-year = år
validation-segment-month = måned
validation-segment-day = dag
validation-segment-hour = time
validation-segment-minute = minut
validation-segment-second = sekund
validation-segment-value = værdi
date-edit-validation-not-a-date = Ugyldig dato
time-edit-validation-not-a-time = Ugyldigt klokkeslæt

# ── farvevælger ──
color-picker-name = Farvevælger
color-picker-hue-label = Farvetone
color-picker-saturation-label = Mætning
color-picker-value-label = Lysstyrke
color-picker-alpha-label = Uigennemsigtighed
color-picker-red-label = Rød
color-picker-green-label = Grøn
color-picker-blue-label = Blå
color-picker-red-short = R
color-picker-green-short = G
color-picker-blue-short = B
color-picker-alpha-short = A
color-picker-hue-short = F
color-picker-saturation-short = M
color-picker-value-short = L
color-picker-hex-label = Hex
color-picker-hex-placeholder = #RRGGBB
color-picker-current-color-label = Valgt farve
color-picker-current-color-readout = Valgt farve { $hex }
color-picker-swatches-name = Foruddefinerede farver
color-picker-swatch-label = Farveprøve { $hex }
color-picker-swatch-selected-suffix = , valgt
color-picker-changed-announcement = Farve ændret til { $hex }
color-picker-done-label = Færdig
color-picker-cancel-label = Annuller
color-edit-trigger-name = Farve { $hex }
color-edit-trigger-name-empty = Farve, ingen
color-edit-trigger-tooltip = Åbn farvevælger
hex-color-input-invalid = Ugyldig hexadecimal farve (forventet #RRGGBB)
hex-color-input-invalid-with-alpha = Ugyldig hexadecimal farve (forventet #RRGGBB eller #RRGGBBAA)
hex-color-input-corrected-shortform = { $raw } udvidet til { $value }
hex-color-input-corrected-uppercase = Normaliseret til { $value }
hex-color-input-placeholder = #RRGGBB
hex-color-input-placeholder-with-alpha = #RRGGBBAA
color-edit-trigger-empty-placeholder = —

# »Mere«-udfoldningen i rige værktøjstip (accordion-titlen, der viser den
# lange brødtekst i et fastgjort rigt værktøjstip).
tooltip-more = Mere

# Indbyggede genvejsmenupunkter for tekstfelter og den rige teksteditor.
menu-cut = Klip
menu-copy = Kopiér
menu-paste = Sæt ind
menu-paste-unformatted = Sæt ind uden formatering
menu-select-all = Markér alt
menu-toggle-blockquote = Slå blokcitat til/fra
menu-remove-blockquote = Fjern blokcitat

# DropZone — oplæste statusbeskeder til skærmlæsere (live-område).
drop-zone-hover-file-one = Slip for at tilføje 1 fil
drop-zone-hover-file-many = Slip for at tilføje { $count } filer
drop-zone-hover-text = Slip for at tilføje tekst
drop-zone-hover-link-one = Slip for at tilføje 1 link
drop-zone-hover-link-many = Slip for at tilføje { $count } links
drop-zone-hover-generic = Slip her
drop-zone-hover-reject = Dette element kan ikke slippes her
drop-zone-added-file-one = 1 fil tilføjet
drop-zone-added-file-many = { $count } filer tilføjet
drop-zone-added-text = Tekst tilføjet
drop-zone-added-link-one = 1 link tilføjet
drop-zone-added-link-many = { $count } links tilføjet
drop-zone-rejected = Elementet blev ikke accepteret

# ThemeSwitcher-widget. Se crates/teksilo-widgets/src/theme_switcher.rs.
theme-switcher-label = Tema
theme-switcher-light = Lys
theme-switcher-dark = Mørk
theme-switcher-system = System

# FontPicker-widget. Se crates/teksilo-widgets/src/font_picker.rs.
font-picker-label = Skrifttype
font-picker-placeholder = Vælg en skrifttype…

# Notifikation om mislykket skrivning af indstillinger. Se en-US.ftl for
# den fulde kontekst (udløses af ToastRegistry::show_settings_write_failed
# via teksilo::install_toast).
settings-write-failed-toast-title = Indstillingerne kunne ikke gemmes
settings-write-failed-toast-body = { $file } kunne ikke gemmes efter { $attempts } forsøg; antal kasserede ændringer i køen: { $dropped }. { $message }

# Reserve-vinduesmenu, der åbnes ved højreklik på en tilpasset TitleBar,
# hvor styresystemet ikke selv leverer en (X11). Se en-US.ftl for den fulde
# kontekst og crates/teksilo-widgets/src/title_bar/window_menu.rs.
window-menu-restore = Gendan
window-menu-maximize = Maksimer
window-menu-minimize = Minimer
window-menu-close = Luk

# Udfoldning af notifikationens brødtekst. Se en-US.ftl for den fulde
# kontekst og crates/teksilo-widgets/src/toast/body.rs.
toast-show-more = Vis mere
toast-show-less = Vis mindre
toast-copy-body = Kopiér
toast-body-copied = Kopieret

# Kommandopalet. Se en-US.ftl for den fulde kontekst og
# crates/teksilo-widgets/src/command_palette.rs.
command-palette-placeholder = Skriv en kommando
command-palette-empty = Ingen matchende kommando
command-palette-title = Kommandopalet
command-palette-result-count =
    { $count ->
        [0] Ingen matchende kommando
        [one] 1 kommando
       *[other] { $count } kommandoer
    }
