# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

# teksilo-widgets framework strings — Swedish translation.
#
# Runtime-only: applications that register this locale via
# `I18nConfig::framework_locales(teksilo_widgets::framework_locales())`
# get these translations alongside en-US. Keys missing from sv-SE
# fall back to the en-US source via `I18nManager::resolve_widget`'s
# manual fallback chain (app override active → framework active →
# app override source → framework source → key placeholder). This is
# teksilo-i18n's own fallback, not `fluent-bundle`'s built-in per-key
# fallback — each `FluentBundle` is constructed with a single locale
# in its chain, and the multi-locale lookup is handled at the
# `I18nManager` layer.

a11y-status-bar-name = Status
a11y-dialog-name = Dialogruta
a11y-tooltip-name = Verktygstips
a11y-snackbar-name = Avisering
a11y-splitter-divider-name = Avdelare
a11y-splitter-pane = Ruta
a11y-splitter-collapsed = Komprimerad
a11y-splitter-expanded = Expanderad
a11y-breadcrumb-current-page-value = aktuell sida
a11y-toolbar-name = Verktygsfält
toolbar-more = Mer
segmented-control-more = Fler alternativ
breadcrumb-overflow = Visa dold sökväg
a11y-title-bar-name = Fönstrets namnlist
a11y-window-controls-name = Fönsterkontroller
a11y-window-minimize-name = Minimera
a11y-window-maximize-name = Maximera
a11y-window-restore-name = Återställ
a11y-window-close-name = Stäng
a11y-stepper-indicator-strip-name = Steg
a11y-stepper-content-name = Stegets innehåll
tab-close-tooltip = Stäng fliken
a11y-builtin-browse = Bläddra
a11y-builtin-expand = Förstora
a11y-builtin-search = Sök
a11y-builtin-copy = Kopiera
a11y-builtin-clear = Rensa
a11y-builtin-add = Lägg till
a11y-builtin-bell = Aviseringar
a11y-builtin-menu = Meny
a11y-builtin-more = Fler åtgärder
a11y-builtin-visibility = Visa eller dölj
a11y-password-reveal = Visa eller dölj lösenordet
a11y-caps-lock-on = Caps Lock är aktiverat
notifications-title = Aviseringar
notifications-empty = Inga aviseringar
notifications-mark-all-read = Markera alla som lästa
notifications-clear = Rensa alla
notifications-filter-placeholder = Sök bland aviseringar
notifications-bucket-today = Idag
notifications-bucket-yesterday = Igår
notifications-bucket-this-week = Denna vecka
notifications-bucket-earlier = Tidigare
notifications-archive-replay-disabled = (inte längre tillgänglig)
a11y-shortcut-settings-name = Inställningar för kortkommandon
a11y-shortcut-settings-capture-hint = Tryck på valfri tangent. Delete rensar. Esc avbryter.
keystroke-modifier-ctrl = Ctrl
keystroke-modifier-shift = Skift
keystroke-modifier-alt = Alt
keystroke-modifier-super = Super
keystroke-separator = +
keystroke-key-space = Blanksteg
keystroke-key-enter = Retur
keystroke-key-escape = Esc
keystroke-key-tab = Tabb
keystroke-key-backspace = Backsteg
keystroke-key-delete = Del
keystroke-key-arrow-up = Upp
keystroke-key-arrow-down = Ned
keystroke-key-arrow-left = Vänster
keystroke-key-arrow-right = Höger
keystroke-key-home = Home
keystroke-key-end = End
keystroke-key-page-up = PgUp
keystroke-key-page-down = PgDn

# MessageBox — standardknappar och detaljvisning.
messagebox-btn-ok = OK
messagebox-btn-cancel = Avbryt
messagebox-btn-close = Stäng
messagebox-btn-yes = Ja
messagebox-btn-no = Nej
messagebox-btn-yes-to-all = Ja till alla
messagebox-btn-no-to-all = Nej till alla
messagebox-btn-save = Spara
messagebox-btn-save-all = Spara alla
messagebox-btn-discard = Förkasta ändringar
messagebox-btn-apply = Verkställ
messagebox-btn-reset = Återställ
messagebox-btn-restore-defaults = Återställ standardvärden
messagebox-btn-abort = Avbryt åtgärd
messagebox-btn-retry = Försök igen
messagebox-btn-ignore = Ignorera
messagebox-btn-open = Öppna
messagebox-btn-help = Hjälp
messagebox-show-details = Visa detaljer

# Widgeten PrivacySettings. Se crates/teksilo-widgets/src/privacy_settings.rs.
# Informationstext enligt GDPR art. 13 samt åtgärdsknappar. Nycklar med
# parametrar använder Fluent-syntaxen { $namn }.
privacy-not-configured = Telemetri är inte konfigurerat för det här programmet.
privacy-a11y-group-name = Inställningar för integritet och telemetri
privacy-heading = Integritet och telemetri
privacy-notice-controller = Uppgifterna behandlas av { $processor }; det tekniska personuppgiftsbiträdet är { $adapter } (slutpunkt: { $endpoint }).
privacy-notice-purposes = Ändamål: att förbättra programmet — vilka funktioner som används, var buggarna uppstår och vilka plattformar programmet körs på. Inget dokumentinnehåll, inget urklipp, inga tangenttryckningar, inga skärmbilder.
privacy-notice-lawful-anonymous = Rättslig grund: vårt berättigade intresse av produktförbättring (GDPR art. 6.1 f; CNIL:s undantag för publikmätning).
privacy-notice-lawful-pseudonymous = Rättslig grund: ditt uttryckliga samtycke (GDPR art. 6.1 a).
privacy-notice-retention = Lagring: uppgifter på servern lagras i högst { $days } dygn.
privacy-notice-withdrawal-right = Rätt att återkalla: du kan när som helst stänga av reglagen nedan, klicka på ”Återkalla samtycke” för att stoppa all insamling, eller i pseudonymt läge klicka på ”Radera mina uppgifter” för att ta bort posterna från servern.
privacy-notice-policy-link = Fullständig integritetspolicy: { $url }

privacy-scope-section-heading = Vad får programmet dela?
privacy-scope-anonymous-metrics-label = Anonym användningsstatistik
privacy-scope-anonymous-metrics-description = Räkning av vilka knappar, menyalternativ och kortkommandon som används, samt programversion och operativsystem.
privacy-scope-crash-reports-label = Kraschrapporter
privacy-scope-crash-reports-description = Stackspårningar och processmetadata när programmet kraschar. Inget dokumentinnehåll, inga filsökvägar.
privacy-scope-feature-flags-label = Funktionsflaggor
privacy-scope-feature-flags-description = Gör att programmet kan ta emot uppdateringar av funktionsflaggor (t.ex. gradvis lansering av nya verktyg).

privacy-btn-reject-all = Avvisa alla
privacy-btn-accept-all = Acceptera alla
privacy-btn-erase = Radera mina uppgifter
privacy-btn-erase-tooltip = Begär att servern raderar alla händelser som registrerats för den här installationen och återkallar sedan samtycket lokalt.
privacy-btn-fetch = Hämta mina uppgifter
privacy-btn-fetch-tooltip = Hämtar alla händelser som servern har registrerat under ditt installations-ID. Du kan spara resultatet som JSON.
privacy-btn-withdraw = Återkalla samtycke
privacy-btn-withdraw-tooltip = Stoppar all ny datainsamling. Uppgifter som redan registrerats på servern bevaras — använd ”Radera mina uppgifter” först om du vill ta bort dem.
privacy-btn-switch-to-anonymous = Byt till anonymt läge
privacy-btn-switch-to-pseudonymous = Byt till pseudonymt läge

privacy-identity-heading = Dina uppgifter på servern
privacy-identity-install-id = Installations-ID: { $id }
privacy-identity-retention = Servern lagrar dina poster i högst { $days } dygn.

privacy-mode-heading = Integritetsläge
privacy-mode-current-anonymous = Nuvarande läge: Anonymt (inget installations-ID)
privacy-mode-current-pseudonymous = Nuvarande läge: Pseudonymt (installations-ID finns)
privacy-mode-blurb-anonymous = Anonymt läge överför ingen enhetsidentifierare. Om du byter raderas dina befintliga poster på servern och det lokala installations-UUID:t kastas — detta kan inte ångras.
privacy-mode-blurb-pseudonymous = Pseudonymt läge genererar ett slumpmässigt installations-UUID. Du kan då hämta eller radera dina poster på servern. Kräver uttryckligt samtycke och frågar på nytt vid byte.

privacy-confirm-mode-switch-title = Byta integritetsläge?
privacy-confirm-mode-switch-leaving-pseudonymous = Detta begär att servern raderar alla händelser som registrerats under ditt installations-ID, tar bort det lokala installations-UUID:t, återställer ditt samtyckesbeslut och byter integritetsläge. Vill du fortsätta?
privacy-confirm-mode-switch-leaving-anonymous = Detta återställer ditt samtyckesbeslut och byter integritetsläge. Du tillfrågas på nytt innan några nya uppgifter samlas in. Fortsätta?
privacy-confirm-erase-title = Radera dina uppgifter?
privacy-confirm-erase-text = Detta skickar en begäran om radering av alla händelser som registrerats under ditt installations-ID, tar bort allt som fortfarande är buffrat lokalt och återkallar samtycket så att inga fler uppgifter samlas in. Åtgärden kan inte ångras.
privacy-confirm-withdraw-title = Återkalla samtycke?
privacy-confirm-withdraw-text = Inga fler analyshändelser samlas in från det här programmet. Uppgifter som redan registrerats på servern bevaras — använd ”Radera mina uppgifter” innan du återkallar om du vill ta bort dem också.

privacy-fetch-success-title = Dina uppgifter på servern
privacy-fetch-success-text = Hämtade händelser för den här installationen: { $count }.
privacy-fetch-saved-to = Sparat till: { $path }
privacy-fetch-write-error = Det gick inte att skriva filen { $path }: { $error }
privacy-fetch-error-title = Det gick inte att hämta dina uppgifter

privacy-inspect-title = Granska skickade data (antal händelser i bufferten: { $count })
privacy-inspect-empty = Inga händelser har skickats i den här sessionen ännu. Prova att interagera med programmet — klick, menyer och kortkommandon går alla via detta.
privacy-inspect-summary = Visar de senaste händelserna ({ $count } st), nyast först.

# Kalender / DateEdit / TimeEdit / DateTimeEdit. Se
# crates/teksilo-widgets/src/{calendar,date_edit,time_edit,date_time_edit}.rs
# och de gemensamma modulerna under crates/teksilo-widgets/src/common/datetime/.
calendar-month-long-january = januari
calendar-month-long-february = februari
calendar-month-long-march = mars
calendar-month-long-april = april
calendar-month-long-may = maj
calendar-month-long-june = juni
calendar-month-long-july = juli
calendar-month-long-august = augusti
calendar-month-long-september = september
calendar-month-long-october = oktober
calendar-month-long-november = november
calendar-month-long-december = december

calendar-month-short-january = jan.
calendar-month-short-february = feb.
calendar-month-short-march = mars
calendar-month-short-april = apr.
calendar-month-short-may = maj
calendar-month-short-june = juni
calendar-month-short-july = juli
calendar-month-short-august = aug.
calendar-month-short-september = sep.
calendar-month-short-october = okt.
calendar-month-short-november = nov.
calendar-month-short-december = dec.

calendar-weekday-long-monday = måndag
calendar-weekday-long-tuesday = tisdag
calendar-weekday-long-wednesday = onsdag
calendar-weekday-long-thursday = torsdag
calendar-weekday-long-friday = fredag
calendar-weekday-long-saturday = lördag
calendar-weekday-long-sunday = söndag

calendar-weekday-short-monday = mån
calendar-weekday-short-tuesday = tis
calendar-weekday-short-wednesday = ons
calendar-weekday-short-thursday = tors
calendar-weekday-short-friday = fre
calendar-weekday-short-saturday = lör
calendar-weekday-short-sunday = sön

calendar-weekday-narrow-monday = M
calendar-weekday-narrow-tuesday = T
calendar-weekday-narrow-wednesday = O
calendar-weekday-narrow-thursday = T
calendar-weekday-narrow-friday = F
calendar-weekday-narrow-saturday = L
calendar-weekday-narrow-sunday = S

calendar-button-previous-month = Föregående månad
calendar-button-next-month = Nästa månad
calendar-button-previous-year = Föregående år
calendar-button-next-year = Nästa år
calendar-button-today = Idag
calendar-button-month-picker = Välj månad
calendar-button-year-picker = Välj år
calendar-week-number-column = V.
calendar-name = Kalender
calendar-months-grid-label = Månader
calendar-years-grid-label = År
calendar-name-with-month = Kalender, { $month } { $year }
calendar-cell-name = { $weekday } { $day } { $month } { $year }
calendar-range-status = Valt: { $start } – { $end }

date-edit-segment-year = År
date-edit-segment-month = Månad
date-edit-segment-day = Dag
date-edit-calendar-button = Välj datum
date-edit-trigger-tooltip = Öppna kalendern
date-edit-name = Datum
date-edit-placeholder = Välj ett datum

time-edit-segment-hour = Timme
time-edit-segment-minute = Minut
time-edit-segment-second = Sekund
time-edit-segment-period = fm/em
time-edit-period-am = fm
time-edit-period-pm = em
time-edit-name = Tid
time-edit-placeholder = Välj en tid

date-time-edit-name = Datum och tid
date-time-edit-placeholder = Välj datum och tid
date-time-edit-date-name = Datum
date-time-edit-time-name = Tid
date-time-edit-trigger-tooltip = Öppna kalendern
date-range-edit-name = Datumintervall
date-range-edit-placeholder = Välj datumintervall
date-range-edit-start-name = Startdatum
date-range-edit-end-name = Slutdatum
date-range-edit-trigger-tooltip = Öppna intervallkalendern

# Valideringsåterkoppling (TextInputField + DateEdit/TimeEdit)
validation-corrected-to = Rättat automatiskt till { $value }
validation-corrected-with-notes = Rättat automatiskt: { $notes }
validation-segment-clamped = { $segment } { $raw } → { $clamped }
validation-day-clamped-to-month = dag { $raw } → { $clamped } (månadens sista dag)
validation-clamped-to-range = begränsat till tillåtet intervall
validation-segment-year = år
validation-segment-month = månad
validation-segment-day = dag
validation-segment-hour = timme
validation-segment-minute = minut
validation-segment-second = sekund
validation-segment-value = värde
date-edit-validation-not-a-date = Ogiltigt datum
time-edit-validation-not-a-time = Ogiltig tid

# ── färgväljare ──
color-picker-name = Färgväljare
color-picker-hue-label = Nyans
color-picker-saturation-label = Mättnad
color-picker-value-label = Ljusstyrka
color-picker-alpha-label = Opacitet
color-picker-red-label = Röd
color-picker-green-label = Grön
color-picker-blue-label = Blå
color-picker-red-short = R
color-picker-green-short = G
color-picker-blue-short = B
color-picker-alpha-short = A
color-picker-hue-short = N
color-picker-saturation-short = M
color-picker-value-short = L
color-picker-hex-label = Hex
color-picker-hex-placeholder = #RRGGBB
color-picker-current-color-label = Vald färg
color-picker-current-color-readout = Vald färg { $hex }
color-picker-swatches-name = Färgförval
color-picker-swatch-label = Färgruta { $hex }
color-picker-swatch-selected-suffix = , vald
color-picker-changed-announcement = Färgen ändrades till { $hex }
color-picker-done-label = Klar
color-picker-cancel-label = Avbryt
color-edit-trigger-name = Färg { $hex }
color-edit-trigger-name-empty = Färg, ingen
color-edit-trigger-tooltip = Öppna färgväljaren
hex-color-input-invalid = Ogiltig hexadecimal färgkod (förväntat #RRGGBB)
hex-color-input-invalid-with-alpha = Ogiltig hexadecimal färgkod (förväntat #RRGGBB eller #RRGGBBAA)
hex-color-input-corrected-shortform = { $raw } utökades till { $value }
hex-color-input-corrected-uppercase = Normaliserat till { $value }
hex-color-input-placeholder = #RRGGBB
hex-color-input-placeholder-with-alpha = #RRGGBBAA
color-edit-trigger-empty-placeholder = —

# Etiketten ”mer” i rika verktygstips (accordion-titeln som visar den
# utförliga texten i ett fastnålat rikt verktygstips).
tooltip-more = Mer

# Poster i den inbyggda snabbmenyn för textfält och formaterad text.
menu-cut = Klipp ut
menu-copy = Kopiera
menu-paste = Klistra in
menu-paste-unformatted = Klistra in oformaterat
menu-select-all = Markera allt
menu-toggle-blockquote = Växla blockcitat
menu-remove-blockquote = Ta bort blockcitat

# DropZone — meddelanden i live-området (skärmläsare). Singular och plural
# väljs i Rust, inte av Fluent. Se en-US.ftl för fullständig kontext och
# crates/teksilo-widgets/src/drop_zone.rs.
drop-zone-hover-file-one = Släpp för att lägga till 1 fil
drop-zone-hover-file-many = Släpp för att lägga till { $count } filer
drop-zone-hover-text = Släpp för att lägga till text
drop-zone-hover-link-one = Släpp för att lägga till 1 länk
drop-zone-hover-link-many = Släpp för att lägga till { $count } länkar
drop-zone-hover-generic = Släpp här
drop-zone-hover-reject = Det här objektet kan inte släppas här
drop-zone-added-file-one = 1 fil lades till
drop-zone-added-file-many = { $count } filer lades till
drop-zone-added-text = Text lades till
drop-zone-added-link-one = 1 länk lades till
drop-zone-added-link-many = { $count } länkar lades till
drop-zone-rejected = Objektet accepterades inte

# Widgeten ThemeSwitcher. Se crates/teksilo-widgets/src/theme_switcher.rs.
theme-switcher-label = Tema
theme-switcher-light = Ljust
theme-switcher-dark = Mörkt
theme-switcher-system = System

# Widgeten FontPicker. Se crates/teksilo-widgets/src/font_picker.rs.
font-picker-label = Teckensnitt
font-picker-placeholder = Välj ett teckensnitt…

# Avisering när inställningar inte kan sparas. Se en-US.ftl för fullständig
# kontext (utlöses av ToastRegistry::show_settings_write_failed via
# teksilo::install_toast).
settings-write-failed-toast-title = Det gick inte att spara inställningarna
settings-write-failed-toast-body = Det gick inte att spara { $file } efter { $attempts } försök. Förkastade ändringar i kön: { $dropped }. { $message }

# Reservfönstermeny som öppnas med högerklick på en anpassad TitleBar där
# operativsystemet saknar fönstermeny (X11). Se en-US.ftl för fullständig
# kontext och crates/teksilo-widgets/src/title_bar/window_menu.rs.
window-menu-restore = Återställ
window-menu-maximize = Maximera
window-menu-minimize = Minimera
window-menu-close = Stäng

# Utfällning av aviseringstext. Se en-US.ftl för fullständig kontext och
# crates/teksilo-widgets/src/toast/body.rs.
toast-show-more = Visa mer
toast-show-less = Visa mindre
toast-copy-body = Kopiera
toast-body-copied = Kopierat

# Kommandopalett. Se en-US.ftl för fullständig kontext och
# crates/teksilo-widgets/src/command_palette.rs.
command-palette-placeholder = Skriv ett kommando
command-palette-empty = Inget matchande kommando
command-palette-title = Kommandopalett
command-palette-result-count =
    { $count ->
        [0] Inget matchande kommando
        [one] 1 kommando
       *[other] { $count } kommandon
    }
