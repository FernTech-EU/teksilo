# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

# Rammeverkstrenger for teksilo-widgets — oversettelse til norsk bokmål.
#
# Kun ved kjøretid: programmer som registrerer denne lokaliteten via
# `I18nConfig::framework_locales(teksilo_widgets::framework_locales())`
# får disse oversettelsene ved siden av en-US. Nøkler som mangler i
# nb-NO, faller tilbake til kilden i en-US via den manuelle
# tilbakefallskjeden i `I18nManager::resolve_widget` (programoverstyring
# aktiv → rammeverk aktivt → programoverstyring kilde → rammeverk kilde
# → nøkkelplassholder). Dette er teksilo-i18n sin egen mekanisme, ikke
# den innebygde per-nøkkel-mekanismen i `fluent-bundle` — hver
# `FluentBundle` bygges med bare én lokalitet i kjeden, og oppslag på
# tvers av lokaliteter håndteres i `I18nManager`-laget.

a11y-status-bar-name = Status
a11y-dialog-name = Dialogboks
a11y-tooltip-name = Verktøytips
a11y-snackbar-name = Varsel
a11y-splitter-divider-name = Skillelinje
a11y-splitter-pane = Rute
a11y-splitter-collapsed = Skjult
a11y-splitter-expanded = Utvidet
a11y-breadcrumb-current-page-value = gjeldende side
a11y-toolbar-name = Verktøylinje
toolbar-more = Mer
segmented-control-more = Flere alternativer
breadcrumb-overflow = Vis skjult bane
a11y-title-bar-name = Tittellinje for vinduet
a11y-window-controls-name = Vinduskontroller
a11y-window-minimize-name = Minimer
a11y-window-maximize-name = Maksimer
a11y-window-restore-name = Gjenopprett
a11y-window-close-name = Lukk
a11y-stepper-indicator-strip-name = Trinn
a11y-stepper-content-name = Trinninnhold
tab-close-tooltip = Lukk fane
a11y-builtin-browse = Bla gjennom
a11y-builtin-expand = Utvid
a11y-builtin-search = Søk
a11y-builtin-copy = Kopier
a11y-builtin-clear = Tøm
a11y-builtin-add = Legg til
a11y-builtin-bell = Varsler
a11y-builtin-menu = Meny
a11y-builtin-more = Flere handlinger
a11y-builtin-visibility = Vis eller skjul
a11y-password-reveal = Vis eller skjul passord
a11y-caps-lock-on = Caps Lock er på
notifications-title = Varsler
notifications-empty = Ingen varsler
notifications-mark-all-read = Merk alle som lest
notifications-clear = Fjern alle
notifications-filter-placeholder = Søk i varsler
notifications-bucket-today = I dag
notifications-bucket-yesterday = I går
notifications-bucket-this-week = Denne uken
notifications-bucket-earlier = Tidligere
notifications-archive-replay-disabled = (ikke lenger tilgjengelig)
a11y-shortcut-settings-name = Innstillinger for hurtigtaster
a11y-shortcut-settings-capture-hint = Trykk en tast. Delete for å tømme. Esc for å avbryte.
keystroke-modifier-ctrl = Ctrl
keystroke-modifier-shift = Skift
keystroke-modifier-alt = Alt
keystroke-modifier-super = Super
keystroke-separator = +
keystroke-key-space = Mellomrom
keystroke-key-enter = Enter
keystroke-key-escape = Esc
keystroke-key-tab = Tab
keystroke-key-backspace = Backspace
keystroke-key-delete = Del
keystroke-key-arrow-up = Opp
keystroke-key-arrow-down = Ned
keystroke-key-arrow-left = Venstre
keystroke-key-arrow-right = Høyre
keystroke-key-home = Home
keystroke-key-end = End
keystroke-key-page-up = PgUp
keystroke-key-page-down = PgDn

# MessageBox — standardknapper og visning av detaljer. Se
# crates/teksilo-widgets/src/message_box.rs.
messagebox-btn-ok = OK
messagebox-btn-cancel = Avbryt
messagebox-btn-close = Lukk
messagebox-btn-yes = Ja
messagebox-btn-no = Nei
messagebox-btn-yes-to-all = Ja til alle
messagebox-btn-no-to-all = Nei til alle
messagebox-btn-save = Lagre
messagebox-btn-save-all = Lagre alle
messagebox-btn-discard = Forkast
messagebox-btn-apply = Bruk
messagebox-btn-reset = Tilbakestill
messagebox-btn-restore-defaults = Gjenopprett standardverdier
messagebox-btn-abort = Avbryt
messagebox-btn-retry = Prøv på nytt
messagebox-btn-ignore = Ignorer
messagebox-btn-open = Åpne
messagebox-btn-help = Hjelp
messagebox-show-details = Vis detaljer

# Widgeten PrivacySettings. Se
# crates/teksilo-widgets/src/privacy_settings.rs. Informasjon etter
# GDPR art. 13 pluss handlingsknapper. Nøkler med parametere bruker
# Fluent-syntaksen { $navn }.
privacy-not-configured = Telemetri er ikke konfigurert for dette programmet.
privacy-a11y-group-name = Innstillinger for personvern og telemetri
privacy-heading = Personvern og telemetri
privacy-notice-controller = Dataene behandles av { $processor }; teknisk databehandler er { $adapter } (endepunkt: { $endpoint }).
privacy-notice-purposes = Formål: å forbedre programmet — hvilke funksjoner som brukes, hvor feilene samler seg, hvilke plattformer vi kjører på. Ikke noe dokumentinnhold, ingen utklippstavle, ingen tastetrykk, ingen skjermbilder.
privacy-notice-lawful-anonymous = Behandlingsgrunnlag: vår berettigede interesse i å forbedre produktet (GDPR art. 6 nr. 1 bokstav f; CNILs unntak for publikumsmåling).
privacy-notice-lawful-pseudonymous = Behandlingsgrunnlag: ditt uttrykkelige samtykke (GDPR art. 6 nr. 1 bokstav a).
privacy-notice-retention = Lagringstid: data på serveren oppbevares i høyst { $days } døgn.
privacy-notice-withdrawal-right = Rett til å trekke tilbake samtykket: du kan når som helst slå av bryterne nedenfor, klikke «Trekk tilbake samtykket» for å stanse all innsamling, eller i pseudonym modus klikke «Slett dataene mine» for å slette oppføringene fra serveren.
privacy-notice-policy-link = Fullstendig personvernerklæring: { $url }

privacy-scope-section-heading = Hva kan programmet dele?
privacy-scope-anonymous-metrics-label = Anonym bruksstatistikk
privacy-scope-anonymous-metrics-description = Telling av hvilke knapper, menyvalg og hurtigtaster som brukes, samt programversjon og operativsystem.
privacy-scope-crash-reports-label = Krasjrapporter
privacy-scope-crash-reports-description = Stakksporinger og prosessmetadata når programmet krasjer. Ikke noe dokumentinnhold, ingen filbaner.
privacy-scope-feature-flags-label = Funksjonsflagg
privacy-scope-feature-flags-description = Lar programmet motta oppdateringer av funksjonsflagg (for eksempel gradvis utrulling av nye verktøy).

privacy-btn-reject-all = Avvis alle
privacy-btn-accept-all = Godta alle
privacy-btn-erase = Slett dataene mine
privacy-btn-erase-tooltip = Ber serveren om å slette alle hendelser som er registrert for denne installasjonen, og trekker deretter tilbake samtykket lokalt.
privacy-btn-fetch = Hent dataene mine
privacy-btn-fetch-tooltip = Henter alle hendelsene serveren har registrert under installasjons-ID-en din. Resultatet kan lagres som JSON.
privacy-btn-withdraw = Trekk tilbake samtykket
privacy-btn-withdraw-tooltip = Stanser all ny datainnsamling. Data som allerede er registrert på serveren, beholdes — bruk «Slett dataene mine» først hvis du vil ha dem slettet.
privacy-btn-switch-to-anonymous = Bytt til anonym modus
privacy-btn-switch-to-pseudonymous = Bytt til pseudonym modus

privacy-identity-heading = Dataene dine på serveren
privacy-identity-install-id = Installasjons-ID: { $id }
privacy-identity-retention = Serveren oppbevarer oppføringene dine i høyst { $days } døgn.

privacy-mode-heading = Personvernmodus
privacy-mode-current-anonymous = Nå: Anonym (ingen installasjons-ID)
privacy-mode-current-pseudonymous = Nå: Pseudonym (installasjons-ID finnes)
privacy-mode-blurb-anonymous = Anonym modus overfører ingen identifikator per enhet. Å bytte sletter de eksisterende oppføringene dine på serveren og forkaster den lokale installasjons-UUID-en — dette kan ikke angres.
privacy-mode-blurb-pseudonymous = Pseudonym modus genererer en tilfeldig installasjons-UUID. Du kan da hente eller slette oppføringene dine på serveren. Krever uttrykkelig samtykke, og du blir spurt på nytt ved bytte.

privacy-confirm-mode-switch-title = Vil du bytte personvernmodus?
privacy-confirm-mode-switch-leaving-pseudonymous = Dette ber serveren om å slette alle hendelser som er registrert under installasjons-ID-en din, forkaster den lokale installasjons-UUID-en, tilbakestiller samtykkevalget ditt og bytter personvernmodus. Vil du fortsette?
privacy-confirm-mode-switch-leaving-anonymous = Dette tilbakestiller samtykkevalget ditt og bytter personvernmodus. Du blir spurt på nytt før nye data samles inn. Vil du fortsette?
privacy-confirm-erase-title = Vil du slette dataene dine?
privacy-confirm-erase-text = Dette sender en sletteforespørsel for alle hendelser som er registrert under installasjons-ID-en din, forkaster alt som fortsatt ligger i den lokale bufferen, og trekker tilbake samtykket slik at det ikke samles inn flere data. Handlingen kan ikke angres.
privacy-confirm-withdraw-title = Vil du trekke tilbake samtykket?
privacy-confirm-withdraw-text = Det blir ikke samlet inn flere analysehendelser fra dette programmet. Data som allerede er registrert på serveren, beholdes — bruk «Slett dataene mine» før du trekker tilbake samtykket hvis du vil ha dem slettet også.

privacy-fetch-success-title = Dataene dine på serveren
privacy-fetch-success-text = Antall hendelser hentet for denne installasjonen: { $count }.
privacy-fetch-saved-to = Lagret i: { $path }
privacy-fetch-write-error = Kunne ikke skrive filen { $path }: { $error }
privacy-fetch-error-title = Kunne ikke hente dataene dine

privacy-inspect-title = Inspiser sendte data (hendelser i buffer: { $count })
privacy-inspect-empty = Ingen hendelser er sendt i denne økten ennå. Prøv å bruke programmet — klikk, menyer og hurtigtaster går alle gjennom her.
privacy-inspect-summary = Viser de siste hendelsene, nyeste først. Antall: { $count }.

# Kalender / DateEdit / TimeEdit / DateTimeEdit. Se
# crates/teksilo-widgets/src/{calendar,date_edit,time_edit,date_time_edit}.rs
# og fellesmodulene under crates/teksilo-widgets/src/common/datetime/.
# Måneds- og ukedagsnavn følger CLDR for nb i lang, kort og smal bredde.
calendar-month-long-january = januar
calendar-month-long-february = februar
calendar-month-long-march = mars
calendar-month-long-april = april
calendar-month-long-may = mai
calendar-month-long-june = juni
calendar-month-long-july = juli
calendar-month-long-august = august
calendar-month-long-september = september
calendar-month-long-october = oktober
calendar-month-long-november = november
calendar-month-long-december = desember

calendar-month-short-january = jan.
calendar-month-short-february = feb.
calendar-month-short-march = mars
calendar-month-short-april = apr.
calendar-month-short-may = mai
calendar-month-short-june = juni
calendar-month-short-july = juli
calendar-month-short-august = aug.
calendar-month-short-september = sep.
calendar-month-short-october = okt.
calendar-month-short-november = nov.
calendar-month-short-december = des.

calendar-weekday-long-monday = mandag
calendar-weekday-long-tuesday = tirsdag
calendar-weekday-long-wednesday = onsdag
calendar-weekday-long-thursday = torsdag
calendar-weekday-long-friday = fredag
calendar-weekday-long-saturday = lørdag
calendar-weekday-long-sunday = søndag

calendar-weekday-short-monday = man.
calendar-weekday-short-tuesday = tir.
calendar-weekday-short-wednesday = ons.
calendar-weekday-short-thursday = tor.
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
calendar-button-next-month = Neste måned
calendar-button-previous-year = Forrige år
calendar-button-next-year = Neste år
calendar-button-today = I dag
calendar-button-month-picker = Velg måned
calendar-button-year-picker = Velg år
calendar-week-number-column = Uke
calendar-name = Kalender
calendar-months-grid-label = Måneder
calendar-years-grid-label = År
calendar-name-with-month = Kalender, { $month } { $year }
calendar-cell-name = { $weekday } { $day }. { $month } { $year }
calendar-range-status = Valgt: { $start } – { $end }

date-edit-segment-year = År
date-edit-segment-month = Måned
date-edit-segment-day = Dag
date-edit-calendar-button = Velg dato
date-edit-trigger-tooltip = Åpne kalenderen
date-edit-name = Dato
date-edit-placeholder = Velg en dato

time-edit-segment-hour = Time
time-edit-segment-minute = Minutt
time-edit-segment-second = Sekund
time-edit-segment-period = a.m./p.m.
time-edit-period-am = a.m.
time-edit-period-pm = p.m.
time-edit-name = Klokkeslett
time-edit-placeholder = Velg et klokkeslett

date-time-edit-name = Dato og klokkeslett
date-time-edit-placeholder = Velg dato og klokkeslett
date-time-edit-date-name = Dato
date-time-edit-time-name = Klokkeslett
date-time-edit-trigger-tooltip = Åpne kalenderen
date-range-edit-name = Datointervall
date-range-edit-placeholder = Velg datointervall
date-range-edit-start-name = Startdato
date-range-edit-end-name = Sluttdato
date-range-edit-trigger-tooltip = Åpne intervallkalenderen

# Validering (TextInputField + DateEdit/TimeEdit)
validation-corrected-to = Rettet automatisk til { $value }
validation-corrected-with-notes = Rettet automatisk: { $notes }
validation-segment-clamped = { $segment } { $raw } → { $clamped }
validation-day-clamped-to-month = dag { $raw } → { $clamped } (siste dag i måneden)
validation-clamped-to-range = justert til tillatt område
validation-segment-year = år
validation-segment-month = måned
validation-segment-day = dag
validation-segment-hour = time
validation-segment-minute = minutt
validation-segment-second = sekund
validation-segment-value = verdi
date-edit-validation-not-a-date = Ugyldig dato
time-edit-validation-not-a-time = Ugyldig klokkeslett

# ── fargevelger ──
color-picker-name = Fargevelger
color-picker-hue-label = Fargetone
color-picker-saturation-label = Metning
color-picker-value-label = Lysstyrke
color-picker-alpha-label = Dekkevne
color-picker-red-label = Rød
color-picker-green-label = Grønn
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
color-picker-current-color-label = Valgt farge
color-picker-current-color-readout = Valgt farge { $hex }
color-picker-swatches-name = Forhåndsvalgte farger
color-picker-swatch-label = Fargeprøve { $hex }
color-picker-swatch-selected-suffix = , valgt
color-picker-changed-announcement = Fargen er endret til { $hex }
color-picker-done-label = Ferdig
color-picker-cancel-label = Avbryt
color-edit-trigger-name = Farge { $hex }
color-edit-trigger-name-empty = Farge, ingen
color-edit-trigger-tooltip = Åpne fargevelgeren
hex-color-input-invalid = Ugyldig heksadesimal farge (forventet #RRGGBB)
hex-color-input-invalid-with-alpha = Ugyldig heksadesimal farge (forventet #RRGGBB eller #RRGGBBAA)
hex-color-input-corrected-shortform = { $raw } utvidet til { $value }
hex-color-input-corrected-uppercase = Normalisert til { $value }
hex-color-input-placeholder = #RRGGBB
hex-color-input-placeholder-with-alpha = #RRGGBBAA
color-edit-trigger-empty-placeholder = —

# «Mer»-etiketten i rike verktøytips (tittelen på trekkspillet som viser
# den lange teksten i et festet, rikt verktøytips).
tooltip-more = Mer

# Innebygde kontekstmenyvalg for tekstfelt og rik tekst.
menu-cut = Klipp ut
menu-copy = Kopier
menu-paste = Lim inn
menu-paste-unformatted = Lim inn uformatert
menu-select-all = Merk alt
menu-toggle-blockquote = Slå sitatblokk på/av
menu-remove-blockquote = Fjern sitatblokk

# DropZone — meldinger i «live»-området for skjermlesere. Entall og
# flertall velges i Rust, ikke med et Fluent select-uttrykk. Se
# en-US.ftl for fullstendig kontekst og
# crates/teksilo-widgets/src/drop_zone.rs.
drop-zone-hover-file-one = Slipp for å legge til 1 fil
drop-zone-hover-file-many = Slipp for å legge til { $count } filer
drop-zone-hover-text = Slipp for å legge til tekst
drop-zone-hover-link-one = Slipp for å legge til 1 lenke
drop-zone-hover-link-many = Slipp for å legge til { $count } lenker
drop-zone-hover-generic = Slipp her
drop-zone-hover-reject = Dette elementet kan ikke slippes her
drop-zone-added-file-one = 1 fil lagt til
drop-zone-added-file-many = { $count } filer lagt til
drop-zone-added-text = Tekst lagt til
drop-zone-added-link-one = 1 lenke lagt til
drop-zone-added-link-many = { $count } lenker lagt til
drop-zone-rejected = Elementet ble ikke godtatt

# Widgeten ThemeSwitcher. Se crates/teksilo-widgets/src/theme_switcher.rs.
theme-switcher-label = Tema
theme-switcher-light = Lyst
theme-switcher-dark = Mørkt
theme-switcher-system = System

# Widgeten FontPicker. Se crates/teksilo-widgets/src/font_picker.rs.
font-picker-label = Skrift
font-picker-placeholder = Velg en skrift…

# Varsel om mislykket lagring av innstillinger. Se en-US.ftl for
# fullstendig kontekst (utløses av
# ToastRegistry::show_settings_write_failed via teksilo::install_toast).
# Dette melder om reelt datatap, så varselet har feilalvorlighet og
# forsvinner ikke av seg selv.
settings-write-failed-toast-title = Innstillingene kunne ikke lagres
settings-write-failed-toast-body = Kunne ikke lagre { $file } etter { $attempts } forsøk. Forkastede endringer i kø: { $dropped }. { $message }

# Reserve-vindusmeny, åpnet med høyreklikk på en egendefinert TitleBar
# der operativsystemet ikke har noen vindusmeny (X11). Se en-US.ftl for
# fullstendig kontekst og
# crates/teksilo-widgets/src/title_bar/window_menu.rs. Gjenopprett og
# Maksimer vises aldri samtidig.
window-menu-restore = Gjenopprett
window-menu-maximize = Maksimer
window-menu-minimize = Minimer
window-menu-close = Lukk

# Utvidelse av varselteksten. Se en-US.ftl for fullstendig kontekst og
# crates/teksilo-widgets/src/toast/body.rs.
toast-show-more = Vis mer
toast-show-less = Vis mindre
toast-copy-body = Kopier
toast-body-copied = Kopiert

# Kommandopalett. Se en-US.ftl for fullstendig kontekst og
# crates/teksilo-widgets/src/command_palette.rs.
command-palette-placeholder = Skriv en kommando
command-palette-empty = Ingen kommandoer samsvarer
# Tilgjengelig navn på dialogen og søkefeltet i paletten. Vises aldri på
# skjermen, så dette er det eneste som forteller en skjermleserbruker
# hva som nettopp ble åpnet.
command-palette-title = Kommandopalett
# Leses opp som beskrivelse av dialogen og på nytt etter hvert som søket
# innsnevres, slik at treffantallet er tilgjengelig uten å bla gjennom
# hele listen.
command-palette-result-count =
    { $count ->
        [0] Ingen kommandoer samsvarer
        [one] 1 kommando
       *[other] { $count } kommandoer
    }
