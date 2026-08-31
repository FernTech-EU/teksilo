# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

# teksilo-widgets framework strings — Czech translation (čeština).
#
# Runtime-only: applications that register this locale via
# `I18nConfig::framework_locales(teksilo_widgets::framework_locales())`
# get these translations alongside en-US. Keys missing from cs-CZ
# fall back to the en-US source via `I18nManager::resolve_widget`'s
# manual fallback chain (app override active → framework active →
# app override source → framework source → key placeholder). This is
# teksilo-i18n's own fallback, not `fluent-bundle`'s built-in per-key
# fallback — each `FluentBundle` is constructed with a single locale
# in its chain, and the multi-locale lookup is handled at the
# `I18nManager` layer.

a11y-status-bar-name = Stav
a11y-dialog-name = Dialogové okno
a11y-tooltip-name = Popisek
a11y-snackbar-name = Oznámení
a11y-splitter-divider-name = Rozdělovač
a11y-splitter-pane = Podokno
a11y-splitter-collapsed = Sbaleno
a11y-splitter-expanded = Rozbaleno
a11y-breadcrumb-current-page-value = aktuální stránka
a11y-toolbar-name = Panel nástrojů
toolbar-more = Více
segmented-control-more = Další možnosti
breadcrumb-overflow = Zobrazit skrytou cestu
a11y-title-bar-name = Záhlaví okna
a11y-window-controls-name = Ovládací prvky okna
a11y-window-minimize-name = Minimalizovat
a11y-window-maximize-name = Maximalizovat
a11y-window-restore-name = Obnovit
a11y-window-close-name = Zavřít
a11y-stepper-indicator-strip-name = Kroky
a11y-stepper-content-name = Obsah kroku
tab-close-tooltip = Zavřít kartu
a11y-builtin-browse = Procházet
a11y-builtin-expand = Zvětšit
a11y-builtin-search = Hledat
a11y-builtin-copy = Kopírovat
a11y-builtin-clear = Vymazat
a11y-builtin-add = Přidat
a11y-builtin-bell = Oznámení
a11y-builtin-menu = Nabídka
a11y-builtin-more = Další akce
a11y-builtin-visibility = Zobrazit nebo skrýt
a11y-password-reveal = Zobrazit nebo skrýt heslo
a11y-caps-lock-on = Caps Lock je zapnutý
notifications-title = Oznámení
notifications-empty = Žádná oznámení
notifications-mark-all-read = Označit vše jako přečtené
notifications-clear = Vymazat vše
notifications-filter-placeholder = Hledat v oznámeních
notifications-bucket-today = Dnes
notifications-bucket-yesterday = Včera
notifications-bucket-this-week = Tento týden
notifications-bucket-earlier = Starší
notifications-archive-replay-disabled = (již není k dispozici)
a11y-shortcut-settings-name = Nastavení klávesových zkratek
a11y-shortcut-settings-capture-hint = Stiskněte libovolnou klávesu. Del pro vymazání. Esc pro zrušení.
keystroke-modifier-ctrl = Ctrl
keystroke-modifier-shift = Shift
keystroke-modifier-alt = Alt
keystroke-modifier-super = Super
keystroke-separator = +
keystroke-key-space = Mezerník
keystroke-key-enter = Enter
keystroke-key-escape = Esc
keystroke-key-tab = Tab
keystroke-key-backspace = Backspace
keystroke-key-delete = Del
keystroke-key-arrow-up = Nahoru
keystroke-key-arrow-down = Dolů
keystroke-key-arrow-left = Doleva
keystroke-key-arrow-right = Doprava
keystroke-key-home = Home
keystroke-key-end = End
keystroke-key-page-up = PgUp
keystroke-key-page-down = PgDn

# MessageBox — standardní tlačítka a zobrazení podrobností.
messagebox-btn-ok = OK
messagebox-btn-cancel = Zrušit
messagebox-btn-close = Zavřít
messagebox-btn-yes = Ano
messagebox-btn-no = Ne
messagebox-btn-yes-to-all = Ano pro vše
messagebox-btn-no-to-all = Ne pro vše
messagebox-btn-save = Uložit
messagebox-btn-save-all = Uložit vše
messagebox-btn-discard = Zahodit
messagebox-btn-apply = Použít
messagebox-btn-reset = Obnovit
messagebox-btn-restore-defaults = Obnovit výchozí nastavení
messagebox-btn-abort = Přerušit
messagebox-btn-retry = Opakovat
messagebox-btn-ignore = Ignorovat
messagebox-btn-open = Otevřít
messagebox-btn-help = Nápověda
messagebox-show-details = Zobrazit podrobnosti

# Widget PrivacySettings. Viz crates/teksilo-widgets/src/privacy_settings.rs.
# Informace podle čl. 13 GDPR + tlačítka akcí. Klíče s parametry používají
# syntaxi Fluent { $název }.
privacy-not-configured = Telemetrie není pro tuto aplikaci nakonfigurována.
privacy-a11y-group-name = Nastavení soukromí a telemetrie
privacy-heading = Soukromí a telemetrie
privacy-notice-controller = Údaje zpracovává { $processor }; technickým zpracovatelem je { $adapter } (koncový bod: { $endpoint }).
privacy-notice-purposes = Účely zpracování: zlepšování aplikace – které funkce se používají, kde se hromadí chyby, na jakých platformách aplikace běží. Žádný obsah dokumentů, žádná schránka, žádné stisky kláves, žádné snímky obrazovky.
privacy-notice-lawful-anonymous = Právní základ: náš oprávněný zájem na zlepšování produktu (GDPR čl. 6 odst. 1 písm. f); výjimka francouzského úřadu CNIL pro měření návštěvnosti).
privacy-notice-lawful-pseudonymous = Právní základ: váš výslovný souhlas (GDPR čl. 6 odst. 1 písm. a)).
privacy-notice-retention = Doba uložení dat na serveru (ve dnech): nejvýše { $days }.
privacy-notice-withdrawal-right = Právo odvolat souhlas: kterýkoli z níže uvedených přepínačů můžete kdykoli vypnout, kliknutím na „Odvolat souhlas“ zastavit veškeré shromažďování údajů, nebo v pseudonymním režimu kliknutím na „Vymazat moje údaje“ smazat záznamy ze serveru.
privacy-notice-policy-link = Úplné zásady ochrany osobních údajů: { $url }

privacy-scope-section-heading = Co může aplikace sdílet?
privacy-scope-anonymous-metrics-label = Anonymní statistiky používání
privacy-scope-anonymous-metrics-description = Počty použití tlačítek / položek nabídky / klávesových zkratek, verze aplikace a operační systém.
privacy-scope-crash-reports-label = Hlášení o pádech
privacy-scope-crash-reports-description = Výpisy zásobníku a metadata procesu při pádu aplikace. Žádný obsah dokumentů, žádné cesty k souborům.
privacy-scope-feature-flags-label = Příznaky funkcí
privacy-scope-feature-flags-description = Umožňuje aplikaci přijímat aktualizace příznaků funkcí (např. postupné zavádění nových nástrojů).

privacy-btn-reject-all = Odmítnout vše
privacy-btn-accept-all = Přijmout vše
privacy-btn-erase = Vymazat moje údaje
privacy-btn-erase-tooltip = Požádá server o výmaz všech událostí zaznamenaných pro tuto instalaci a poté místně odvolá souhlas.
privacy-btn-fetch = Získat moje údaje
privacy-btn-fetch-tooltip = Načte všechny události, které server zaznamenal pod vaším identifikátorem instalace. Výsledek můžete uložit ve formátu JSON.
privacy-btn-withdraw = Odvolat souhlas
privacy-btn-withdraw-tooltip = Zastaví shromažďování nových údajů. Již zaznamenaná data na serveru zůstávají zachována – pokud je chcete smazat, použijte nejprve „Vymazat moje údaje“.
privacy-btn-switch-to-anonymous = Přepnout do anonymního režimu
privacy-btn-switch-to-pseudonymous = Přepnout do pseudonymního režimu

privacy-identity-heading = Vaše údaje na serveru
privacy-identity-install-id = Identifikátor instalace: { $id }
privacy-identity-retention = Doba uchovávání vašich záznamů na serveru (ve dnech): nejvýše { $days }.

privacy-mode-heading = Režim ochrany soukromí
privacy-mode-current-anonymous = Aktuálně: anonymní (bez identifikátoru instalace)
privacy-mode-current-pseudonymous = Aktuálně: pseudonymní (identifikátor instalace je přítomen)
privacy-mode-blurb-anonymous = Anonymní režim nepřenáší žádný identifikátor zařízení. Přepnutím se smažou vaše stávající záznamy na serveru a zahodí se místní UUID instalace – tuto akci nelze vrátit zpět.
privacy-mode-blurb-pseudonymous = Pseudonymní režim vygeneruje náhodné UUID instalace. Své záznamy na serveru budete moci načíst nebo vymazat. Vyžaduje výslovný souhlas a při přepnutí se na něj znovu zeptá.

privacy-confirm-mode-switch-title = Změnit režim ochrany soukromí?
privacy-confirm-mode-switch-leaving-pseudonymous = Tato akce požádá server o vymazání všech událostí zaznamenaných pod vaším identifikátorem instalace, odstraní místní UUID instalace, zruší vaše rozhodnutí o souhlasu a změní režim ochrany soukromí. Chcete pokračovat?
privacy-confirm-mode-switch-leaving-anonymous = Tato akce zruší vaše rozhodnutí o souhlasu a změní režim ochrany soukromí. Před shromažďováním jakýchkoli nových údajů budete znovu dotázáni. Pokračovat?
privacy-confirm-erase-title = Vymazat vaše údaje?
privacy-confirm-erase-text = Tato akce odešle žádost o výmaz každé události zaznamenané pod vaším identifikátorem instalace, zahodí vše, co je ještě uloženo v místní vyrovnávací paměti, a odvolá souhlas, takže se nebudou shromažďovat žádné další údaje. Akci nelze vrátit zpět.
privacy-confirm-withdraw-title = Odvolat souhlas?
privacy-confirm-withdraw-text = Z této aplikace nebudou shromažďovány žádné další analytické události. Již zaznamenaná data na serveru zůstávají zachována – pokud je chcete smazat také, použijte před odvoláním souhlasu „Vymazat moje údaje“.

privacy-fetch-success-title = Vaše údaje na serveru
privacy-fetch-success-text = Načteny události pro tuto instalaci (počet: { $count }).
privacy-fetch-saved-to = Uloženo do: { $path }
privacy-fetch-write-error = Nepodařilo se zapsat soubor { $path }: { $error }
privacy-fetch-error-title = Vaše údaje se nepodařilo načíst

privacy-inspect-title = Kontrola odesílaných dat (počet událostí ve vyrovnávací paměti: { $count })
privacy-inspect-empty = V této relaci zatím nebyly odeslány žádné události. Zkuste s aplikací pracovat – kliknutí, nabídky i klávesové zkratky procházejí právě tudy.
privacy-inspect-summary = Zobrazeny poslední události (počet: { $count }), od nejnovější po nejstarší.

# Kalendář / DateEdit / TimeEdit / DateTimeEdit. Viz
# crates/teksilo-widgets/src/{calendar,date_edit,time_edit,date_time_edit}.rs
# a společné moduly v crates/teksilo-widgets/src/common/datetime/.
# Názvy měsíců jsou v samostatném (nominativním) tvaru podle CLDR — widget je
# používá i jako samostatné popisky v záhlaví kalendáře a ve výběru měsíce.
calendar-month-long-january = leden
calendar-month-long-february = únor
calendar-month-long-march = březen
calendar-month-long-april = duben
calendar-month-long-may = květen
calendar-month-long-june = červen
calendar-month-long-july = červenec
calendar-month-long-august = srpen
calendar-month-long-september = září
calendar-month-long-october = říjen
calendar-month-long-november = listopad
calendar-month-long-december = prosinec

calendar-month-short-january = led
calendar-month-short-february = úno
calendar-month-short-march = bře
calendar-month-short-april = dub
calendar-month-short-may = kvě
calendar-month-short-june = čvn
calendar-month-short-july = čvc
calendar-month-short-august = srp
calendar-month-short-september = zář
calendar-month-short-october = říj
calendar-month-short-november = lis
calendar-month-short-december = pro

calendar-weekday-long-monday = pondělí
calendar-weekday-long-tuesday = úterý
calendar-weekday-long-wednesday = středa
calendar-weekday-long-thursday = čtvrtek
calendar-weekday-long-friday = pátek
calendar-weekday-long-saturday = sobota
calendar-weekday-long-sunday = neděle

calendar-weekday-short-monday = po
calendar-weekday-short-tuesday = út
calendar-weekday-short-wednesday = st
calendar-weekday-short-thursday = čt
calendar-weekday-short-friday = pá
calendar-weekday-short-saturday = so
calendar-weekday-short-sunday = ne

calendar-weekday-narrow-monday = P
calendar-weekday-narrow-tuesday = Ú
calendar-weekday-narrow-wednesday = S
calendar-weekday-narrow-thursday = Č
calendar-weekday-narrow-friday = P
calendar-weekday-narrow-saturday = S
calendar-weekday-narrow-sunday = N

calendar-button-previous-month = Předchozí měsíc
calendar-button-next-month = Další měsíc
calendar-button-previous-year = Předchozí rok
calendar-button-next-year = Další rok
calendar-button-today = Dnes
calendar-button-month-picker = Vybrat měsíc
calendar-button-year-picker = Vybrat rok
calendar-week-number-column = Týd.
calendar-name = Kalendář
calendar-months-grid-label = Měsíce
calendar-years-grid-label = Roky
calendar-name-with-month = Kalendář, { $month } { $year }
calendar-cell-name = { $weekday } { $day }. { $month } { $year }
calendar-range-status = Vybráno: { $start } – { $end }

date-edit-segment-year = Rok
date-edit-segment-month = Měsíc
date-edit-segment-day = Den
date-edit-calendar-button = Vybrat datum
date-edit-trigger-tooltip = Otevřít kalendář
date-edit-name = Datum
date-edit-placeholder = Vyberte datum

time-edit-segment-hour = Hodina
time-edit-segment-minute = Minuta
time-edit-segment-second = Sekunda
time-edit-segment-period = dop./odp.
time-edit-period-am = dop.
time-edit-period-pm = odp.
time-edit-name = Čas
time-edit-placeholder = Vyberte čas

date-time-edit-name = Datum a čas
date-time-edit-placeholder = Vyberte datum a čas
date-time-edit-date-name = Datum
date-time-edit-time-name = Čas
date-time-edit-trigger-tooltip = Otevřít kalendář
date-range-edit-name = Rozsah dat
date-range-edit-placeholder = Vyberte rozsah dat
date-range-edit-start-name = Počáteční datum
date-range-edit-end-name = Koncové datum
date-range-edit-trigger-tooltip = Otevřít kalendář rozsahu

# Zpětná vazba ověření (TextInputField + DateEdit/TimeEdit)
validation-corrected-to = Automaticky opraveno na { $value }
validation-corrected-with-notes = Automaticky opraveno: { $notes }
validation-segment-clamped = { $segment } { $raw } → { $clamped }
validation-day-clamped-to-month = den { $raw } → { $clamped } (poslední den měsíce)
validation-clamped-to-range = upraveno na povolený rozsah
validation-segment-year = rok
validation-segment-month = měsíc
validation-segment-day = den
validation-segment-hour = hodina
validation-segment-minute = minuta
validation-segment-second = sekunda
validation-segment-value = hodnota
date-edit-validation-not-a-date = Neplatné datum
time-edit-validation-not-a-time = Neplatný čas

# ── výběr barvy ──
color-picker-name = Výběr barvy
color-picker-hue-label = Odstín
color-picker-saturation-label = Sytost
color-picker-value-label = Jas
color-picker-alpha-label = Krytí
color-picker-red-label = Červená
color-picker-green-label = Zelená
color-picker-blue-label = Modrá
color-picker-red-short = Č
color-picker-green-short = Z
color-picker-blue-short = M
color-picker-alpha-short = A
color-picker-hue-short = O
color-picker-saturation-short = S
color-picker-value-short = J
color-picker-hex-label = Hex
color-picker-hex-placeholder = #RRGGBB
color-picker-current-color-label = Vybraná barva
color-picker-current-color-readout = Vybraná barva { $hex }
color-picker-swatches-name = Předvolby barev
color-picker-swatch-label = Vzorek { $hex }
color-picker-swatch-selected-suffix = , vybráno
color-picker-changed-announcement = Barva změněna na { $hex }
color-picker-done-label = Hotovo
color-picker-cancel-label = Zrušit
color-edit-trigger-name = Barva { $hex }
color-edit-trigger-name-empty = Barva, žádná
color-edit-trigger-tooltip = Otevřít výběr barvy
hex-color-input-invalid = Neplatný hexadecimální kód barvy (očekáváno #RRGGBB)
hex-color-input-invalid-with-alpha = Neplatný hexadecimální kód barvy (očekáváno #RRGGBB nebo #RRGGBBAA)
hex-color-input-corrected-shortform = { $raw } rozšířeno na { $value }
hex-color-input-corrected-uppercase = Normalizováno na { $value }
hex-color-input-placeholder = #RRGGBB
hex-color-input-placeholder-with-alpha = #RRGGBBAA
color-edit-trigger-empty-placeholder = —

# Popisek „více“ v rozbalovací části bohatých popisků (titulek harmoniky,
# která odhalí podrobný text v připnutém bohatém popisku).
tooltip-more = Více

# Položky vestavěné místní nabídky textových polí a editoru formátovaného textu.
menu-cut = Vyjmout
menu-copy = Kopírovat
menu-paste = Vložit
menu-paste-unformatted = Vložit bez formátování
menu-select-all = Vybrat vše
menu-toggle-blockquote = Přepnout citaci
menu-remove-blockquote = Odebrat citaci

# DropZone — hlášení oblasti „live“ pro čtečky obrazovky. Jednotné a množné
# číslo vybírá Rust, nikoli Fluent, takže tvary s počtem jsou přeformulovány
# tak, aby byly správné pro libovolný počet. Viz en-US.ftl pro úplný kontext
# a crates/teksilo-widgets/src/drop_zone.rs.
drop-zone-hover-file-one = Přetažením přidáte 1 soubor
drop-zone-hover-file-many = Přetažením přidáte soubory (počet: { $count })
drop-zone-hover-text = Přetažením přidáte text
drop-zone-hover-link-one = Přetažením přidáte 1 odkaz
drop-zone-hover-link-many = Přetažením přidáte odkazy (počet: { $count })
drop-zone-hover-generic = Přetáhněte sem
drop-zone-hover-reject = Tuto položku sem nelze přetáhnout
drop-zone-added-file-one = Přidán 1 soubor
drop-zone-added-file-many = Přidány soubory (počet: { $count })
drop-zone-added-text = Přidán text
drop-zone-added-link-one = Přidán 1 odkaz
drop-zone-added-link-many = Přidány odkazy (počet: { $count })
drop-zone-rejected = Položka nebyla přijata

# Widget ThemeSwitcher. Viz crates/teksilo-widgets/src/theme_switcher.rs.
theme-switcher-label = Motiv
theme-switcher-light = Světlý
theme-switcher-dark = Tmavý
theme-switcher-system = Systémový

# Widget FontPicker. Viz crates/teksilo-widgets/src/font_picker.rs.
font-picker-label = Písmo
font-picker-placeholder = Vyberte písmo…

# Oznámení o selhání zápisu nastavení. Viz en-US.ftl pro úplný kontext
# (spouští ToastRegistry::show_settings_write_failed přes
# teksilo::install_toast).
settings-write-failed-toast-title = Nastavení se nepodařilo uložit
settings-write-failed-toast-body = Soubor { $file } se nepodařilo uložit (počet pokusů: { $attempts }); zahozeny čekající změny (počet: { $dropped }). { $message }

# Záložní nabídka okna, otevřená klepnutím pravým tlačítkem na vlastní
# TitleBar tam, kde ji operační systém neposkytuje (X11). Viz en-US.ftl pro
# úplný kontext a crates/teksilo-widgets/src/title_bar/window_menu.rs.
window-menu-restore = Obnovit
window-menu-maximize = Maximalizovat
window-menu-minimize = Minimalizovat
window-menu-close = Zavřít

# Rozbalení textu oznámení. Viz en-US.ftl pro úplný kontext a
# crates/teksilo-widgets/src/toast/body.rs.
toast-show-more = Zobrazit více
toast-show-less = Zobrazit méně
toast-copy-body = Kopírovat
toast-body-copied = Zkopírováno

# Paleta příkazů. Viz en-US.ftl pro úplný kontext a
# crates/teksilo-widgets/src/command_palette.rs.
command-palette-placeholder = Zadejte příkaz
command-palette-empty = Žádný odpovídající příkaz
command-palette-title = Paleta příkazů
command-palette-result-count =
    { $count ->
        [0] Žádný odpovídající příkaz
        [one] 1 příkaz
        [few] { $count } příkazy
        [many] { $count } příkazu
       *[other] { $count } příkazů
    }
