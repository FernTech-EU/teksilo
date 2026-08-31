# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

# teksilo-widgets keretrendszer-szövegek — magyar (hu-HU) fordítás.
#
# Csak futásidőben érvényes: azok az alkalmazások kapják meg, amelyek az
# `I18nConfig::framework_locales(teksilo_widgets::framework_locales())`
# hívással regisztrálják ezt a nyelvet. A hu-HU fájlból hiányzó kulcsok
# az en-US forrásra esnek vissza az `I18nManager::resolve_widget` saját
# tartalék láncán keresztül (alkalmazás-felülbírálás aktív → keretrendszer
# aktív → alkalmazás-felülbírálás forrás → keretrendszer forrás → kulcs
# helyőrzőként). Ez a teksilo-i18n saját tartalékmechanizmusa, nem a
# `fluent-bundle` beépített, kulcsonkénti tartaléka: minden `FluentBundle`
# egyetlen nyelvvel jön létre, a többnyelvű keresés az `I18nManager`
# rétegben történik.

a11y-status-bar-name = Állapot
a11y-dialog-name = Párbeszédablak
a11y-tooltip-name = Buboréksúgó
a11y-snackbar-name = Értesítés
a11y-splitter-divider-name = Elválasztó
a11y-splitter-pane = Ablaktábla
a11y-splitter-collapsed = Összecsukva
a11y-splitter-expanded = Kibontva
a11y-breadcrumb-current-page-value = aktuális oldal
a11y-toolbar-name = Eszköztár
toolbar-more = Továbbiak
segmented-control-more = További lehetőségek
breadcrumb-overflow = Rejtett útvonal megjelenítése
a11y-title-bar-name = Ablak címsora
a11y-window-controls-name = Ablakvezérlők
a11y-window-minimize-name = Minimalizálás
a11y-window-maximize-name = Maximalizálás
a11y-window-restore-name = Visszaállítás
a11y-window-close-name = Bezárás
a11y-stepper-indicator-strip-name = Lépések
a11y-stepper-content-name = Lépés tartalma
tab-close-tooltip = Lap bezárása
a11y-builtin-browse = Tallózás
a11y-builtin-expand = Kibontás
a11y-builtin-search = Keresés
a11y-builtin-copy = Másolás
a11y-builtin-clear = Törlés
a11y-builtin-add = Hozzáadás
a11y-builtin-bell = Értesítések
a11y-builtin-menu = Menü
a11y-builtin-more = További műveletek
a11y-builtin-visibility = Megjelenítés vagy elrejtés
a11y-password-reveal = Jelszó megjelenítése vagy elrejtése
a11y-caps-lock-on = A Caps Lock be van kapcsolva
notifications-title = Értesítések
notifications-empty = Nincsenek értesítések
notifications-mark-all-read = Összes megjelölése olvasottként
notifications-clear = Összes törlése
notifications-filter-placeholder = Értesítések keresése
notifications-bucket-today = Ma
notifications-bucket-yesterday = Tegnap
notifications-bucket-this-week = Ezen a héten
notifications-bucket-earlier = Korábbi
notifications-archive-replay-disabled = (már nem érhető el)
a11y-shortcut-settings-name = Gyorsbillentyűk beállításai
a11y-shortcut-settings-capture-hint = Nyomjon meg egy billentyűt. Del a törléshez, Esc a megszakításhoz.
keystroke-modifier-ctrl = Ctrl
keystroke-modifier-shift = Shift
keystroke-modifier-alt = Alt
keystroke-modifier-super = Super
keystroke-separator = +
keystroke-key-space = Szóköz
keystroke-key-enter = Enter
keystroke-key-escape = Esc
keystroke-key-tab = Tab
keystroke-key-backspace = Backspace
keystroke-key-delete = Del
keystroke-key-arrow-up = Fel
keystroke-key-arrow-down = Le
keystroke-key-arrow-left = Balra
keystroke-key-arrow-right = Jobbra
keystroke-key-home = Home
keystroke-key-end = End
keystroke-key-page-up = PageUp
keystroke-key-page-down = PageDown

# MessageBox — szabványos gombfeliratok és a részletek megjelenítése.
messagebox-btn-ok = OK
messagebox-btn-cancel = Mégse
messagebox-btn-close = Bezárás
messagebox-btn-yes = Igen
messagebox-btn-no = Nem
messagebox-btn-yes-to-all = Igen, mindre
messagebox-btn-no-to-all = Nem, mindre
messagebox-btn-save = Mentés
messagebox-btn-save-all = Összes mentése
messagebox-btn-discard = Elvetés
messagebox-btn-apply = Alkalmaz
messagebox-btn-reset = Alaphelyzet
messagebox-btn-restore-defaults = Alapértelmezések visszaállítása
messagebox-btn-abort = Megszakítás
messagebox-btn-retry = Újra
messagebox-btn-ignore = Mellőzés
messagebox-btn-open = Megnyitás
messagebox-btn-help = Súgó
messagebox-show-details = Részletek megjelenítése

# PrivacySettings widget. Lásd crates/teksilo-widgets/src/privacy_settings.rs.
# GDPR 13. cikk szerinti tájékoztatás + műveletgombok. A paraméteres
# kulcsok a Fluent { $név } szintaxist használják.
privacy-not-configured = A telemetria nincs beállítva ehhez az alkalmazáshoz.
privacy-a11y-group-name = Adatvédelmi és telemetriai beállítások
privacy-heading = Adatvédelem és telemetria
privacy-notice-controller = Az adatokat { $processor } kezeli; a technikai adatfeldolgozó { $adapter } (végpont: { $endpoint }).
privacy-notice-purposes = Az adatkezelés céljai: az alkalmazás fejlesztése — mely funkciókat használják, hol csoportosulnak a hibák, mely platformokon fut az alkalmazás. Dokumentumtartalmat, vágólapadatokat, billentyűleütéseket és képernyőképeket nem gyűjtünk.
privacy-notice-lawful-anonymous = Az adatkezelés jogalapja: a termék fejlesztéséhez fűződő jogos érdekünk (GDPR 6. cikk (1) bekezdés f) pont; a CNIL közönségmérésre vonatkozó mentessége).
privacy-notice-lawful-pseudonymous = Az adatkezelés jogalapja: az Ön kifejezett hozzájárulása (GDPR 6. cikk (1) bekezdés a) pont).
privacy-notice-retention = Megőrzési idő: a kiszolgálón tárolt adatokat legfeljebb { $days } napig őrizzük meg.
privacy-notice-withdrawal-right = A hozzájárulás visszavonásához való jog: az alábbi kapcsolókat bármikor kikapcsolhatja, a „Hozzájárulás visszavonása” gombra kattintva leállíthatja a teljes adatgyűjtést, álnevesített módban pedig az „Adataim törlése” gombbal töröltetheti a rekordokat a kiszolgálóról.
privacy-notice-policy-link = Teljes adatvédelmi tájékoztató: { $url }

privacy-scope-section-heading = Mit oszthat meg az alkalmazás?
privacy-scope-anonymous-metrics-label = Anonim használati statisztikák
privacy-scope-anonymous-metrics-description = Annak megszámlálása, mely gombokat, menüelemeket és gyorsbillentyűket használják, valamint az alkalmazás verziója és az operációs rendszer.
privacy-scope-crash-reports-label = Összeomlási jelentések
privacy-scope-crash-reports-description = Veremkivonatok és folyamat-metaadatok az alkalmazás összeomlásakor. Dokumentumtartalmat és fájlútvonalakat nem tartalmaznak.
privacy-scope-feature-flags-label = Funkciókapcsolók
privacy-scope-feature-flags-description = Lehetővé teszi, hogy az alkalmazás funkciókapcsoló-frissítéseket fogadjon (például új eszközök fokozatos bevezetéséhez).

privacy-btn-reject-all = Összes elutasítása
privacy-btn-accept-all = Összes elfogadása
privacy-btn-erase = Adataim törlése
privacy-btn-erase-tooltip = Kéri a kiszolgálótól az ehhez a telepítéshez rögzített összes esemény törlését, majd helyben visszavonja a hozzájárulást.
privacy-btn-fetch = Adataim lekérése
privacy-btn-fetch-tooltip = Lekéri az összes eseményt, amelyet a kiszolgáló az Ön telepítési azonosítója alatt rögzített. Az eredményt JSON formátumban mentheti.
privacy-btn-withdraw = Hozzájárulás visszavonása
privacy-btn-withdraw-tooltip = Leállítja az új adatok gyűjtését. A kiszolgálón már rögzített adatok megmaradnak — ha ezeket is törölni szeretné, előbb használja az „Adataim törlése” lehetőséget.
privacy-btn-switch-to-anonymous = Váltás anonim módra
privacy-btn-switch-to-pseudonymous = Váltás álnevesített módra

privacy-identity-heading = Az Ön adatai a kiszolgálón
privacy-identity-install-id = Telepítési azonosító: { $id }
privacy-identity-retention = A kiszolgáló legfeljebb { $days } napig őrzi meg az Ön rekordjait.

privacy-mode-heading = Adatvédelmi mód
privacy-mode-current-anonymous = Jelenleg: anonim (nincs telepítési azonosító)
privacy-mode-current-pseudonymous = Jelenleg: álnevesített (van telepítési azonosító)
privacy-mode-blurb-anonymous = Az anonim mód nem továbbít eszközönkénti azonosítót. A váltás törli a kiszolgálón meglévő rekordjait, és eldobja a helyi telepítési UUID-t — ez a művelet nem vonható vissza.
privacy-mode-blurb-pseudonymous = Az álnevesített mód véletlenszerű telepítési UUID-t hoz létre. Így lekérheti vagy töröltetheti a rekordjait a kiszolgálón. Kifejezett hozzájárulást igényel, és váltáskor újra rákérdez.

privacy-confirm-mode-switch-title = Megváltoztatja az adatvédelmi módot?
privacy-confirm-mode-switch-leaving-pseudonymous = Ezzel kéri a kiszolgálótól az Ön telepítési azonosítója alatt rögzített összes esemény törlését, eldobja a helyi telepítési UUID-t, visszaállítja a hozzájárulásról hozott döntését, és megváltoztatja az adatvédelmi módot. Folytatja?
privacy-confirm-mode-switch-leaving-anonymous = Ezzel visszaállítja a hozzájárulásról hozott döntését, és megváltoztatja az adatvédelmi módot. Bármely új adat gyűjtése előtt újra megkérdezzük. Folytatja?
privacy-confirm-erase-title = Törli az adatait?
privacy-confirm-erase-text = Ezzel törlési kérelmet küld az Ön telepítési azonosítója alatt rögzített minden eseményre, eldobja a helyben még pufferelt adatokat, és visszavonja a hozzájárulást, így további adatok gyűjtésére nem kerül sor. A művelet nem vonható vissza.
privacy-confirm-withdraw-title = Visszavonja a hozzájárulást?
privacy-confirm-withdraw-text = Ebből az alkalmazásból nem gyűjtünk több analitikai eseményt. A kiszolgálón már rögzített adatok megmaradnak — ha ezeket is törölni szeretné, a visszavonás előtt használja az „Adataim törlése” lehetőséget.

privacy-fetch-success-title = Az Ön adatai a kiszolgálón
privacy-fetch-success-text = { $count } esemény lekérve ehhez a telepítéshez.
privacy-fetch-saved-to = Mentve ide: { $path }
privacy-fetch-write-error = Nem sikerült írni a(z) { $path } fájlt: { $error }
privacy-fetch-error-title = Nem sikerült lekérni az adatait

privacy-inspect-title = Elküldött adatok vizsgálata ({ $count } esemény a pufferben)
privacy-inspect-empty = Ebben a munkamenetben még egyetlen esemény sem keletkezett. Próbálja használni az alkalmazást — a kattintások, a menük és a gyorsbillentyűk mind itt haladnak át.
privacy-inspect-summary = Az utolsó { $count } esemény látható, a legújabbal kezdve.

# Naptár / DateEdit / TimeEdit / DateTimeEdit. Lásd
# crates/teksilo-widgets/src/{calendar,date_edit,time_edit,date_time_edit}.rs
# és a közös modulokat a crates/teksilo-widgets/src/common/datetime/ alatt.
calendar-month-long-january = január
calendar-month-long-february = február
calendar-month-long-march = március
calendar-month-long-april = április
calendar-month-long-may = május
calendar-month-long-june = június
calendar-month-long-july = július
calendar-month-long-august = augusztus
calendar-month-long-september = szeptember
calendar-month-long-october = október
calendar-month-long-november = november
calendar-month-long-december = december

calendar-month-short-january = jan.
calendar-month-short-february = febr.
calendar-month-short-march = márc.
calendar-month-short-april = ápr.
calendar-month-short-may = máj.
calendar-month-short-june = jún.
calendar-month-short-july = júl.
calendar-month-short-august = aug.
calendar-month-short-september = szept.
calendar-month-short-october = okt.
calendar-month-short-november = nov.
calendar-month-short-december = dec.

calendar-weekday-long-monday = hétfő
calendar-weekday-long-tuesday = kedd
calendar-weekday-long-wednesday = szerda
calendar-weekday-long-thursday = csütörtök
calendar-weekday-long-friday = péntek
calendar-weekday-long-saturday = szombat
calendar-weekday-long-sunday = vasárnap

calendar-weekday-short-monday = H
calendar-weekday-short-tuesday = K
calendar-weekday-short-wednesday = Sze
calendar-weekday-short-thursday = Cs
calendar-weekday-short-friday = P
calendar-weekday-short-saturday = Szo
calendar-weekday-short-sunday = V

calendar-weekday-narrow-monday = H
calendar-weekday-narrow-tuesday = K
calendar-weekday-narrow-wednesday = Sz
calendar-weekday-narrow-thursday = Cs
calendar-weekday-narrow-friday = P
calendar-weekday-narrow-saturday = Sz
calendar-weekday-narrow-sunday = V

calendar-button-previous-month = Előző hónap
calendar-button-next-month = Következő hónap
calendar-button-previous-year = Előző év
calendar-button-next-year = Következő év
calendar-button-today = Ma
calendar-button-month-picker = Hónap kiválasztása
calendar-button-year-picker = Év kiválasztása
calendar-week-number-column = Hét
calendar-name = Naptár
calendar-months-grid-label = Hónapok
calendar-years-grid-label = Évek
calendar-name-with-month = Naptár, { $year }. { $month }
calendar-cell-name = { $year }. { $month } { $day }., { $weekday }
calendar-range-status = Kijelölve: { $start } – { $end }

date-edit-segment-year = Év
date-edit-segment-month = Hónap
date-edit-segment-day = Nap
date-edit-calendar-button = Dátum kiválasztása
date-edit-trigger-tooltip = Naptár megnyitása
date-edit-name = Dátum
date-edit-placeholder = Válasszon dátumot

time-edit-segment-hour = Óra
time-edit-segment-minute = Perc
time-edit-segment-second = Másodperc
time-edit-segment-period = de./du.
time-edit-period-am = de.
time-edit-period-pm = du.
time-edit-name = Idő
time-edit-placeholder = Válasszon időpontot

date-time-edit-name = Dátum és idő
date-time-edit-placeholder = Válasszon dátumot és időpontot
date-time-edit-date-name = Dátum
date-time-edit-time-name = Idő
date-time-edit-trigger-tooltip = Naptár megnyitása
date-range-edit-name = Dátumtartomány
date-range-edit-placeholder = Válasszon dátumtartományt
date-range-edit-start-name = Kezdő dátum
date-range-edit-end-name = Záró dátum
date-range-edit-trigger-tooltip = Tartománynaptár megnyitása

# Ellenőrzési visszajelzés (TextInputField + DateEdit/TimeEdit)
validation-corrected-to = Automatikusan javítva erre: { $value }
validation-corrected-with-notes = Automatikus javítás: { $notes }
validation-segment-clamped = { $segment } { $raw } → { $clamped }
validation-day-clamped-to-month = nap { $raw } → { $clamped } (a hónap utolsó napja)
validation-clamped-to-range = a megengedett tartományra korlátozva
validation-segment-year = év
validation-segment-month = hónap
validation-segment-day = nap
validation-segment-hour = óra
validation-segment-minute = perc
validation-segment-second = másodperc
validation-segment-value = érték
date-edit-validation-not-a-date = Érvénytelen dátum
time-edit-validation-not-a-time = Érvénytelen időpont

# ── színválasztó ──
color-picker-name = Színválasztó
color-picker-hue-label = Árnyalat
color-picker-saturation-label = Telítettség
color-picker-value-label = Fényerő
color-picker-alpha-label = Átlátszatlanság
color-picker-red-label = Vörös
color-picker-green-label = Zöld
color-picker-blue-label = Kék
color-picker-red-short = R
color-picker-green-short = G
color-picker-blue-short = B
color-picker-alpha-short = A
color-picker-hue-short = H
color-picker-saturation-short = S
color-picker-value-short = V
color-picker-hex-label = Hex
color-picker-hex-placeholder = #RRGGBB
color-picker-current-color-label = Kiválasztott szín
color-picker-current-color-readout = Kiválasztott szín: { $hex }
color-picker-swatches-name = Előre beállított színek
color-picker-swatch-label = Színminta: { $hex }
color-picker-swatch-selected-suffix = , kiválasztva
color-picker-changed-announcement = A szín erre változott: { $hex }
color-picker-done-label = Kész
color-picker-cancel-label = Mégse
color-edit-trigger-name = Szín: { $hex }
color-edit-trigger-name-empty = Szín, nincs
color-edit-trigger-tooltip = Színválasztó megnyitása
hex-color-input-invalid = Érvénytelen hexadecimális színkód (várt formátum: #RRGGBB)
hex-color-input-invalid-with-alpha = Érvénytelen hexadecimális színkód (várt formátum: #RRGGBB vagy #RRGGBBAA)
hex-color-input-corrected-shortform = { $raw } kibontva erre: { $value }
hex-color-input-corrected-uppercase = Normalizálva erre: { $value }
hex-color-input-placeholder = #RRGGBB
hex-color-input-placeholder-with-alpha = #RRGGBBAA
color-edit-trigger-empty-placeholder = —

# A gazdag buboréksúgó „továbbiak” lenyitójának felirata (a részletes
# törzsszöveget felfedő harmonika címe egy rögzített, gazdag buboréksúgóban).
tooltip-more = Továbbiak

# A szövegmezők és a gazdag szövegszerkesztő beépített helyi menüjének elemei.
menu-cut = Kivágás
menu-copy = Másolás
menu-paste = Beillesztés
menu-paste-unformatted = Beillesztés formázás nélkül
menu-select-all = Összes kijelölése
menu-toggle-blockquote = Idézetblokk be- és kikapcsolása
menu-remove-blockquote = Idézetblokk eltávolítása

# DropZone — élő régió bejelentései képernyőolvasóknak. Az egyes és a
# többes számú alakot a Rust oldal választja ki, nem Fluent. Lásd
# en-US.ftl a teljes kontextusért és
# crates/teksilo-widgets/src/drop_zone.rs.
drop-zone-hover-file-one = Ejtse ide 1 fájl hozzáadásához
drop-zone-hover-file-many = Ejtse ide { $count } fájl hozzáadásához
drop-zone-hover-text = Ejtse ide szöveg hozzáadásához
drop-zone-hover-link-one = Ejtse ide 1 hivatkozás hozzáadásához
drop-zone-hover-link-many = Ejtse ide { $count } hivatkozás hozzáadásához
drop-zone-hover-generic = Ejtse ide
drop-zone-hover-reject = Ez az elem nem ejthető ide
drop-zone-added-file-one = 1 fájl hozzáadva
drop-zone-added-file-many = { $count } fájl hozzáadva
drop-zone-added-text = Szöveg hozzáadva
drop-zone-added-link-one = 1 hivatkozás hozzáadva
drop-zone-added-link-many = { $count } hivatkozás hozzáadva
drop-zone-rejected = Az elem nem fogadható el

# ThemeSwitcher widget. Lásd crates/teksilo-widgets/src/theme_switcher.rs.
theme-switcher-label = Téma
theme-switcher-light = Világos
theme-switcher-dark = Sötét
theme-switcher-system = Rendszer

# FontPicker widget. Lásd crates/teksilo-widgets/src/font_picker.rs.
font-picker-label = Betűtípus
font-picker-placeholder = Válasszon betűtípust…

# A beállítások mentésének meghiúsulását jelző értesítés. Lásd en-US.ftl a
# teljes kontextusért (a ToastRegistry::show_settings_write_failed váltja
# ki a teksilo::install_toast közvetítésével).
settings-write-failed-toast-title = Nem sikerült menteni a beállításokat
settings-write-failed-toast-body = A(z) { $file } mentése { $attempts } próbálkozás után sem sikerült; { $dropped } várakozó módosítás elveszett. { $message }

# Tartalék ablakmenü, amely egyéni TitleBar jobb gombos kattintására nyílik
# meg azokon a platformokon, ahol az operációs rendszer nem kínál ablakmenüt
# (X11). Lásd en-US.ftl a teljes kontextusért és
# crates/teksilo-widgets/src/title_bar/window_menu.rs.
window-menu-restore = Visszaállítás
window-menu-maximize = Maximalizálás
window-menu-minimize = Minimalizálás
window-menu-close = Bezárás

# Az értesítés törzsének lenyitása. Lásd en-US.ftl a teljes kontextusért és
# crates/teksilo-widgets/src/toast/body.rs.
toast-show-more = Több megjelenítése
toast-show-less = Kevesebb megjelenítése
toast-copy-body = Másolás
toast-body-copied = Másolva

# Parancskatalógus. Lásd en-US.ftl a teljes kontextusért és
# crates/teksilo-widgets/src/command_palette.rs.
command-palette-placeholder = Írjon be egy parancsot
command-palette-empty = Nincs megfelelő parancs
command-palette-title = Parancskatalógus
command-palette-result-count =
    { $count ->
        [0] Nincs megfelelő parancs
        [one] 1 parancs
       *[other] { $count } parancs
    }
