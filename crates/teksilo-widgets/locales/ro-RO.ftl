# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

# Șirurile cadrului teksilo-widgets — traducerea în limba română.
#
# Doar la execuție: aplicațiile care înregistrează această limbă prin
# `I18nConfig::framework_locales(teksilo_widgets::framework_locales())`
# primesc aceste traduceri alături de en-US. Cheile absente din ro-RO
# revin la sursa en-US prin lanțul manual de rezervă din
# `I18nManager::resolve_widget` (suprascrierea aplicației activă →
# cadrul activ → sursa suprascrierii aplicației → sursa cadrului →
# substituentul cheii). Este mecanismul propriu al teksilo-i18n, nu
# rezerva per-cheie a `fluent-bundle` — fiecare `FluentBundle` este
# construit cu o singură limbă în lanț, iar căutarea multilingvă se
# face la nivelul `I18nManager`.

a11y-status-bar-name = Stare
a11y-dialog-name = Casetă de dialog
a11y-tooltip-name = Sfat ecran
a11y-snackbar-name = Notificare
a11y-splitter-divider-name = Separator de panouri
a11y-splitter-pane = Panou
a11y-splitter-collapsed = Restrâns
a11y-splitter-expanded = Extins
a11y-breadcrumb-current-page-value = pagina curentă
a11y-toolbar-name = Bară de instrumente
toolbar-more = Mai multe
segmented-control-more = Mai multe opțiuni
breadcrumb-overflow = Afișare cale ascunsă
a11y-title-bar-name = Bara de titlu a ferestrei
a11y-window-controls-name = Controale fereastră
a11y-window-minimize-name = Minimizare
a11y-window-maximize-name = Maximizare
a11y-window-restore-name = Restaurare
a11y-window-close-name = Închidere
a11y-stepper-indicator-strip-name = Pași
a11y-stepper-content-name = Conținutul pasului
tab-close-tooltip = Închidere filă
a11y-builtin-browse = Răsfoire
a11y-builtin-expand = Extindere
a11y-builtin-search = Căutare
a11y-builtin-copy = Copiere
a11y-builtin-clear = Golire
a11y-builtin-add = Adăugare
a11y-builtin-bell = Notificări
a11y-builtin-menu = Meniu
a11y-builtin-more = Mai multe acțiuni
a11y-builtin-visibility = Comutare vizibilitate
a11y-password-reveal = Afișare sau ascundere parolă
a11y-caps-lock-on = Caps Lock este activat
notifications-title = Notificări
notifications-empty = Nicio notificare
notifications-mark-all-read = Marcare toate ca citite
notifications-clear = Ștergere totală
notifications-filter-placeholder = Căutați în notificări
notifications-bucket-today = Astăzi
notifications-bucket-yesterday = Ieri
notifications-bucket-this-week = Săptămâna aceasta
notifications-bucket-earlier = Mai devreme
notifications-archive-replay-disabled = (nu mai este disponibilă)
a11y-shortcut-settings-name = Setări pentru comenzi rapide
a11y-shortcut-settings-capture-hint = Apăsați orice tastă. Delete pentru golire. Escape pentru anulare.
keystroke-modifier-ctrl = Ctrl
keystroke-modifier-shift = Shift
keystroke-modifier-alt = Alt
keystroke-modifier-super = Super
keystroke-separator = +
keystroke-key-space = Spațiu
keystroke-key-enter = Enter
keystroke-key-escape = Esc
keystroke-key-tab = Tab
keystroke-key-backspace = Backspace
keystroke-key-delete = Del
keystroke-key-arrow-up = Sus
keystroke-key-arrow-down = Jos
keystroke-key-arrow-left = Stânga
keystroke-key-arrow-right = Dreapta
keystroke-key-home = Home
keystroke-key-end = End
keystroke-key-page-up = PgUp
keystroke-key-page-down = PgDn

# MessageBox — butoane standard și afișarea detaliilor. Vezi
# crates/teksilo-widgets/src/message_box.rs.
messagebox-btn-ok = OK
messagebox-btn-cancel = Anulare
messagebox-btn-close = Închidere
messagebox-btn-yes = Da
messagebox-btn-no = Nu
messagebox-btn-yes-to-all = Da la toate
messagebox-btn-no-to-all = Nu la toate
messagebox-btn-save = Salvare
messagebox-btn-save-all = Salvare toate
messagebox-btn-discard = Renunțare
messagebox-btn-apply = Aplicare
messagebox-btn-reset = Resetare
messagebox-btn-restore-defaults = Restabilire valori implicite
messagebox-btn-abort = Abandonare
messagebox-btn-retry = Reîncercare
messagebox-btn-ignore = Ignorare
messagebox-btn-open = Deschidere
messagebox-btn-help = Ajutor
messagebox-show-details = Afișare detalii

# Widgetul PrivacySettings. Vezi crates/teksilo-widgets/src/privacy_settings.rs.
# Informare RGPD art. 13 plus butoanele de acțiune. Cheile cu parametri
# folosesc sintaxa Fluent { $nume }.
privacy-not-configured = Telemetria nu este configurată pentru această aplicație.
privacy-a11y-group-name = Setări de confidențialitate și telemetrie
privacy-heading = Confidențialitate și telemetrie
privacy-notice-controller = Datele sunt prelucrate de { $processor }; persoana împuternicită de operator pentru partea tehnică este { $adapter } (punct de colectare: { $endpoint }).
privacy-notice-purposes = Scopuri: îmbunătățirea aplicației — ce funcționalități sunt utilizate, unde se concentrează erorile, pe ce platforme rulăm. Niciun conținut al documentelor, nimic din clipboard, nicio tastă apăsată, nicio captură de ecran.
privacy-notice-lawful-anonymous = Temei juridic: interesul nostru legitim de a îmbunătăți produsul (RGPD art. 6 alin. (1) lit. (f); exceptarea CNIL pentru măsurarea audienței).
privacy-notice-lawful-pseudonymous = Temei juridic: consimțământul dumneavoastră explicit (RGPD art. 6 alin. (1) lit. (a)).
privacy-notice-retention = Perioadă de stocare: numărul maxim de zile în care datele sunt păstrate pe server este { $days }.
privacy-notice-withdrawal-right = Dreptul de retragere: puteți dezactiva oricând oricare dintre comutatoarele de mai jos, puteți apăsa „Retragerea consimțământului” pentru a opri orice colectare sau, în modul pseudonim, „Ștergerea datelor mele” pentru a elimina înregistrările de pe server.
privacy-notice-policy-link = Politica de confidențialitate completă: { $url }

privacy-scope-section-heading = Ce poate partaja aplicația?
privacy-scope-anonymous-metrics-label = Statistici anonime de utilizare
privacy-scope-anonymous-metrics-description = Numărul de utilizări ale butoanelor / elementelor de meniu / comenzilor rapide, plus versiunea aplicației și sistemul de operare.
privacy-scope-crash-reports-label = Rapoarte de blocare
privacy-scope-crash-reports-description = Urme de stivă și metadate ale procesului atunci când aplicația se blochează. Niciun conținut al documentelor, nicio cale de fișier.
privacy-scope-feature-flags-label = Indicatori de funcționalitate
privacy-scope-feature-flags-description = Permite aplicației să primească actualizări ale indicatorilor de funcționalitate (de exemplu, lansarea treptată a unor instrumente noi).

privacy-btn-reject-all = Respingere totală
privacy-btn-accept-all = Acceptare totală
privacy-btn-erase = Ștergerea datelor mele
privacy-btn-erase-tooltip = Solicită serverului să șteargă toate evenimentele înregistrate pentru această instalare, apoi retrage consimțământul la nivel local.
privacy-btn-fetch = Obținerea datelor mele
privacy-btn-fetch-tooltip = Preia toate evenimentele pe care serverul le-a înregistrat sub identificatorul dumneavoastră de instalare. Rezultatul poate fi salvat în format JSON.
privacy-btn-withdraw = Retragerea consimțământului
privacy-btn-withdraw-tooltip = Oprește colectarea de date noi. Datele deja înregistrate pe server sunt păstrate — folosiți mai întâi „Ștergerea datelor mele” dacă doriți eliminarea lor.
privacy-btn-switch-to-anonymous = Comutare la modul Anonim
privacy-btn-switch-to-pseudonymous = Comutare la modul Pseudonim

privacy-identity-heading = Datele dumneavoastră de pe server
privacy-identity-install-id = Identificator de instalare: { $id }
privacy-identity-retention = Numărul maxim de zile în care serverul păstrează înregistrările dumneavoastră este { $days }.

privacy-mode-heading = Mod de confidențialitate
privacy-mode-current-anonymous = În prezent: Anonim (fără identificator de instalare)
privacy-mode-current-pseudonymous = În prezent: Pseudonim (identificator de instalare prezent)
privacy-mode-blurb-anonymous = Modul anonim nu transmite niciun identificator de dispozitiv. Comutarea va șterge înregistrările dumneavoastră existente de pe server și va elimina UUID-ul de instalare local — această acțiune este ireversibilă.
privacy-mode-blurb-pseudonymous = Modul pseudonim generează un UUID de instalare aleatoriu. Veți putea obține sau șterge înregistrările dumneavoastră de pe server. Necesită consimțământ explicit și îl solicită din nou la comutare.

privacy-confirm-mode-switch-title = Schimbați modul de confidențialitate?
privacy-confirm-mode-switch-leaving-pseudonymous = Această acțiune va solicita serverului să șteargă toate evenimentele înregistrate sub identificatorul dumneavoastră de instalare, va elimina UUID-ul de instalare local, va reseta decizia dumneavoastră privind consimțământul și va schimba modul de confidențialitate. Doriți să continuați?
privacy-confirm-mode-switch-leaving-anonymous = Această acțiune va reseta decizia dumneavoastră privind consimțământul și va schimba modul de confidențialitate. Veți fi întrebat din nou înainte de colectarea oricăror date noi. Continuați?
privacy-confirm-erase-title = Ștergeți datele dumneavoastră?
privacy-confirm-erase-text = Această acțiune trimite o cerere de ștergere pentru fiecare eveniment înregistrat sub identificatorul dumneavoastră de instalare, elimină tot ce se află încă în memoria tampon locală și retrage consimțământul, astfel încât nicio altă dată să nu mai fie colectată. Acțiunea este ireversibilă.
privacy-confirm-withdraw-title = Retrageți consimțământul?
privacy-confirm-withdraw-text = Nu vor mai fi colectate evenimente de analiză din această aplicație. Datele deja înregistrate pe server sunt păstrate — folosiți „Ștergerea datelor mele” înainte de retragere dacă doriți eliminarea lor.

privacy-fetch-success-title = Datele dumneavoastră de pe server
privacy-fetch-success-text = Evenimente preluate pentru această instalare: { $count }.
privacy-fetch-saved-to = Salvat în: { $path }
privacy-fetch-write-error = Nu s-a putut scrie fișierul { $path }: { $error }
privacy-fetch-error-title = Nu s-au putut prelua datele dumneavoastră

privacy-inspect-title = Inspectați datele trimise (evenimente în memoria tampon: { $count })
privacy-inspect-empty = Niciun eveniment nu a fost emis încă în această sesiune. Interacționați cu aplicația — clicurile, meniurile și comenzile rapide trec toate pe aici.
privacy-inspect-summary = Se afișează cele mai recente evenimente ({ $count }), în ordine invers cronologică.

# Calendar / DateEdit / TimeEdit / DateTimeEdit. Vezi
# crates/teksilo-widgets/src/{calendar,date_edit,time_edit,date_time_edit}.rs
# și modulele comune din crates/teksilo-widgets/src/common/datetime/.
calendar-month-long-january = ianuarie
calendar-month-long-february = februarie
calendar-month-long-march = martie
calendar-month-long-april = aprilie
calendar-month-long-may = mai
calendar-month-long-june = iunie
calendar-month-long-july = iulie
calendar-month-long-august = august
calendar-month-long-september = septembrie
calendar-month-long-october = octombrie
calendar-month-long-november = noiembrie
calendar-month-long-december = decembrie

calendar-month-short-january = ian.
calendar-month-short-february = feb.
calendar-month-short-march = mar.
calendar-month-short-april = apr.
calendar-month-short-may = mai
calendar-month-short-june = iun.
calendar-month-short-july = iul.
calendar-month-short-august = aug.
calendar-month-short-september = sept.
calendar-month-short-october = oct.
calendar-month-short-november = nov.
calendar-month-short-december = dec.

calendar-weekday-long-monday = luni
calendar-weekday-long-tuesday = marți
calendar-weekday-long-wednesday = miercuri
calendar-weekday-long-thursday = joi
calendar-weekday-long-friday = vineri
calendar-weekday-long-saturday = sâmbătă
calendar-weekday-long-sunday = duminică

calendar-weekday-short-monday = lun.
calendar-weekday-short-tuesday = mar.
calendar-weekday-short-wednesday = mie.
calendar-weekday-short-thursday = joi
calendar-weekday-short-friday = vin.
calendar-weekday-short-saturday = sâm.
calendar-weekday-short-sunday = dum.

calendar-weekday-narrow-monday = L
calendar-weekday-narrow-tuesday = M
calendar-weekday-narrow-wednesday = M
calendar-weekday-narrow-thursday = J
calendar-weekday-narrow-friday = V
calendar-weekday-narrow-saturday = S
calendar-weekday-narrow-sunday = D

calendar-button-previous-month = Luna anterioară
calendar-button-next-month = Luna următoare
calendar-button-previous-year = Anul anterior
calendar-button-next-year = Anul următor
calendar-button-today = Astăzi
calendar-button-month-picker = Alegere lună
calendar-button-year-picker = Alegere an
calendar-week-number-column = Săpt.
calendar-name = Calendar
calendar-months-grid-label = Luni
calendar-years-grid-label = Ani
calendar-name-with-month = Calendar, { $month } { $year }
calendar-cell-name = { $weekday }, { $day } { $month } { $year }
calendar-range-status = Selecție: { $start } – { $end }

date-edit-segment-year = An
date-edit-segment-month = Lună
date-edit-segment-day = Zi
date-edit-calendar-button = Alegere dată
date-edit-trigger-tooltip = Deschidere calendar
date-edit-name = Dată
date-edit-placeholder = Selectați o dată

time-edit-segment-hour = Oră
time-edit-segment-minute = Minut
time-edit-segment-second = Secundă
time-edit-segment-period = a.m./p.m.
time-edit-period-am = a.m.
time-edit-period-pm = p.m.
time-edit-name = Oră
time-edit-placeholder = Selectați o oră

date-time-edit-name = Dată și oră
date-time-edit-placeholder = Selectați data și ora
date-time-edit-date-name = Dată
date-time-edit-time-name = Oră
date-time-edit-trigger-tooltip = Deschidere calendar
date-range-edit-name = Interval de date
date-range-edit-placeholder = Selectați un interval de date
date-range-edit-start-name = Dată de început
date-range-edit-end-name = Dată de sfârșit
date-range-edit-trigger-tooltip = Deschidere calendar de interval

# Mesaje de validare (TextInputField + DateEdit/TimeEdit)
validation-corrected-to = Corectat automat la { $value }
validation-corrected-with-notes = Corectat automat: { $notes }
validation-segment-clamped = { $segment } { $raw } → { $clamped }
validation-day-clamped-to-month = zi { $raw } → { $clamped } (ultima zi a lunii)
validation-clamped-to-range = limitat la intervalul permis
validation-segment-year = an
validation-segment-month = lună
validation-segment-day = zi
validation-segment-hour = oră
validation-segment-minute = minut
validation-segment-second = secundă
validation-segment-value = valoare
date-edit-validation-not-a-date = Dată nevalidă
time-edit-validation-not-a-time = Oră nevalidă

# ── selector de culoare ──
color-picker-name = Selector de culoare
color-picker-hue-label = Nuanță
color-picker-saturation-label = Saturație
color-picker-value-label = Luminozitate
color-picker-alpha-label = Opacitate
color-picker-red-label = Roșu
color-picker-green-label = Verde
color-picker-blue-label = Albastru
color-picker-red-short = R
color-picker-green-short = G
color-picker-blue-short = B
color-picker-alpha-short = A
color-picker-hue-short = H
color-picker-saturation-short = S
color-picker-value-short = V
color-picker-hex-label = Hex
color-picker-hex-placeholder = #RRGGBB
color-picker-current-color-label = Culoare selectată
color-picker-current-color-readout = Culoare selectată { $hex }
color-picker-swatches-name = Culori predefinite
color-picker-swatch-label = Eșantion { $hex }
color-picker-swatch-selected-suffix = , selectat
color-picker-changed-announcement = Culoare schimbată în { $hex }
color-picker-done-label = Gata
color-picker-cancel-label = Anulare
color-edit-trigger-name = Culoare { $hex }
color-edit-trigger-name-empty = Culoare, niciuna
color-edit-trigger-tooltip = Deschidere selector de culoare
hex-color-input-invalid = Cod hexazecimal de culoare nevalid (se așteaptă #RRGGBB)
hex-color-input-invalid-with-alpha = Cod hexazecimal de culoare nevalid (se așteaptă #RRGGBB sau #RRGGBBAA)
hex-color-input-corrected-shortform = { $raw } a fost extins la { $value }
hex-color-input-corrected-uppercase = Normalizat la { $value }
hex-color-input-placeholder = #RRGGBB
hex-color-input-placeholder-with-alpha = #RRGGBBAA
color-edit-trigger-empty-placeholder = —

# Eticheta „mai multe” a secțiunii pliante dintr-un sfat ecran îmbogățit
# (titlul acordeonului care dezvăluie corpul detaliat).
tooltip-more = Mai multe

# Elementele meniului contextual al câmpurilor de text și al editorului
# de text îmbogățit.
menu-cut = Decupare
menu-copy = Copiere
menu-paste = Lipire
menu-paste-unformatted = Lipire fără formatare
menu-select-all = Selectare totală
menu-toggle-blockquote = Comutare citat
menu-remove-blockquote = Eliminare citat

# DropZone — anunțuri ale regiunii „live” pentru cititoarele de ecran.
# Singularul și pluralul sunt alese în Rust, nu printr-o expresie de
# selecție Fluent, așa că formele „many” sunt formulate ca număr după
# substantiv, corect pentru orice valoare. Vezi en-US.ftl pentru contextul
# complet și crates/teksilo-widgets/src/drop_zone.rs.
drop-zone-hover-file-one = Plasați pentru a adăuga un fișier
drop-zone-hover-file-many = Plasați pentru a adăuga fișiere ({ $count })
drop-zone-hover-text = Plasați pentru a adăuga text
drop-zone-hover-link-one = Plasați pentru a adăuga un link
drop-zone-hover-link-many = Plasați pentru a adăuga linkuri ({ $count })
drop-zone-hover-generic = Plasați aici
drop-zone-hover-reject = Acest element nu poate fi plasat aici
drop-zone-added-file-one = Un fișier adăugat
drop-zone-added-file-many = Fișiere adăugate: { $count }
drop-zone-added-text = Text adăugat
drop-zone-added-link-one = Un link adăugat
drop-zone-added-link-many = Linkuri adăugate: { $count }
drop-zone-rejected = Element neacceptat

# Widgetul ThemeSwitcher. Vezi crates/teksilo-widgets/src/theme_switcher.rs.
theme-switcher-label = Temă
theme-switcher-light = Luminoasă
theme-switcher-dark = Întunecată
theme-switcher-system = Sistem

# Widgetul FontPicker. Vezi crates/teksilo-widgets/src/font_picker.rs.
font-picker-label = Font
font-picker-placeholder = Alegeți un font…

# Notificare de eșec la scrierea setărilor. Vezi en-US.ftl pentru contextul
# complet (declanșată de ToastRegistry::show_settings_write_failed prin
# teksilo::install_toast). Semnalează o pierdere reală de date, deci este
# de severitate Eroare și persistentă.
settings-write-failed-toast-title = Setările nu au putut fi salvate
settings-write-failed-toast-body = Salvarea { $file } a eșuat; număr de încercări: { $attempts }; modificări în așteptare abandonate: { $dropped }. { $message }

# Meniu de fereastră de rezervă, deschis cu clic dreapta pe o TitleBar
# personalizată acolo unde sistemul nu oferă unul (X11). Restaurare și
# Maximizare se exclud reciproc; se afișează doar unul. Vezi en-US.ftl
# pentru contextul complet și
# crates/teksilo-widgets/src/title_bar/window_menu.rs.
window-menu-restore = Restaurare
window-menu-maximize = Maximizare
window-menu-minimize = Minimizare
window-menu-close = Închidere

# Extinderea corpului unei notificări toast. Vezi en-US.ftl pentru contextul
# complet și crates/teksilo-widgets/src/toast/body.rs.
toast-show-more = Mai mult
toast-show-less = Mai puțin
toast-copy-body = Copiere
toast-body-copied = Copiat

# Paleta de comenzi. Vezi en-US.ftl pentru contextul complet și
# crates/teksilo-widgets/src/command_palette.rs.
command-palette-placeholder = Tastați o comandă
command-palette-empty = Nicio comandă corespunzătoare
# Numele accesibil al casetei de dialog a paletei și al câmpului său de
# căutare. Nu apare niciodată pe ecran.
command-palette-title = Paletă de comenzi
# Anunțat ca descriere a casetei de dialog și reanunțat pe măsură ce
# interogarea se restrânge. Categoriile de plural CLDR pentru română sunt
# one / few / other (few acoperă 0, 2–19 și n % 100 = 1–19).
command-palette-result-count =
    { $count ->
        [0] Nicio comandă corespunzătoare
        [one] O comandă
        [few] { $count } comenzi
       *[other] { $count } de comenzi
    }
