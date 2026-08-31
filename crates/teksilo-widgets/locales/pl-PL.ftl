# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

# Komunikaty frameworku teksilo-widgets — tłumaczenie polskie.
#
# Tylko w czasie działania programu: aplikacje, które rejestrują tę lokalizację
# przez `I18nConfig::framework_locales(teksilo_widgets::framework_locales())`,
# otrzymują te tłumaczenia obok en-US. Klucze brakujące w pl-PL wracają do
# źródłowego en-US poprzez ręczny łańcuch zastępczy w
# `I18nManager::resolve_widget` (nadpisanie aplikacji aktywne → framework
# aktywny → źródło nadpisania aplikacji → źródło frameworku → nazwa klucza).
# Jest to własny mechanizm zastępczy teksilo-i18n, a nie wbudowany mechanizm
# `fluent-bundle` — każdy `FluentBundle` powstaje z jedną lokalizacją w
# łańcuchu, a wyszukiwanie wielojęzyczne odbywa się w warstwie `I18nManager`.

a11y-status-bar-name = Stan
a11y-dialog-name = Okno dialogowe
a11y-tooltip-name = Podpowiedź
a11y-snackbar-name = Powiadomienie
a11y-splitter-divider-name = Separator paneli
a11y-splitter-pane = Panel
a11y-splitter-collapsed = Zwinięty
a11y-splitter-expanded = Rozwinięty
a11y-breadcrumb-current-page-value = bieżąca strona
a11y-toolbar-name = Pasek narzędzi
toolbar-more = Więcej
segmented-control-more = Więcej opcji
breadcrumb-overflow = Pokaż ukrytą ścieżkę
a11y-title-bar-name = Pasek tytułu okna
a11y-window-controls-name = Przyciski okna
a11y-window-minimize-name = Minimalizuj
a11y-window-maximize-name = Maksymalizuj
a11y-window-restore-name = Przywróć
a11y-window-close-name = Zamknij
a11y-stepper-indicator-strip-name = Kroki
a11y-stepper-content-name = Zawartość kroku
tab-close-tooltip = Zamknij kartę
a11y-builtin-browse = Przeglądaj
a11y-builtin-expand = Powiększ
a11y-builtin-search = Szukaj
a11y-builtin-copy = Kopiuj
a11y-builtin-clear = Wyczyść
a11y-builtin-add = Dodaj
a11y-builtin-bell = Powiadomienia
a11y-builtin-menu = Menu
a11y-builtin-more = Więcej akcji
a11y-builtin-visibility = Przełącz widoczność
a11y-password-reveal = Pokaż lub ukryj hasło
a11y-caps-lock-on = Caps Lock jest włączony
notifications-title = Powiadomienia
notifications-empty = Brak powiadomień
notifications-mark-all-read = Oznacz wszystkie jako przeczytane
notifications-clear = Wyczyść wszystkie
notifications-filter-placeholder = Szukaj w powiadomieniach
notifications-bucket-today = Dzisiaj
notifications-bucket-yesterday = Wczoraj
notifications-bucket-this-week = W tym tygodniu
notifications-bucket-earlier = Wcześniej
notifications-archive-replay-disabled = (niedostępne)
a11y-shortcut-settings-name = Ustawienia skrótów klawiszowych
a11y-shortcut-settings-capture-hint = Naciśnij dowolny klawisz. Del, aby wyczyścić. Esc, aby anulować.
keystroke-modifier-ctrl = Ctrl
keystroke-modifier-shift = Shift
keystroke-modifier-alt = Alt
keystroke-modifier-super = Super
keystroke-separator = +
keystroke-key-space = Spacja
keystroke-key-enter = Enter
keystroke-key-escape = Esc
keystroke-key-tab = Tab
keystroke-key-backspace = Backspace
keystroke-key-delete = Del
keystroke-key-arrow-up = Góra
keystroke-key-arrow-down = Dół
keystroke-key-arrow-left = Lewo
keystroke-key-arrow-right = Prawo
keystroke-key-home = Home
keystroke-key-end = End
keystroke-key-page-up = PgUp
keystroke-key-page-down = PgDn

# MessageBox — standardowe przyciski i ujawnianie szczegółów.
messagebox-btn-ok = OK
messagebox-btn-cancel = Anuluj
messagebox-btn-close = Zamknij
messagebox-btn-yes = Tak
messagebox-btn-no = Nie
messagebox-btn-yes-to-all = Tak dla wszystkich
messagebox-btn-no-to-all = Nie dla wszystkich
messagebox-btn-save = Zapisz
messagebox-btn-save-all = Zapisz wszystko
messagebox-btn-discard = Odrzuć
messagebox-btn-apply = Zastosuj
messagebox-btn-reset = Resetuj
messagebox-btn-restore-defaults = Przywróć domyślne
messagebox-btn-abort = Przerwij
messagebox-btn-retry = Ponów
messagebox-btn-ignore = Zignoruj
messagebox-btn-open = Otwórz
messagebox-btn-help = Pomoc
messagebox-show-details = Pokaż szczegóły

# Widget PrivacySettings. Zobacz crates/teksilo-widgets/src/privacy_settings.rs.
# Informacja z art. 13 RODO oraz przyciski akcji. Klucze z parametrami
# używają składni Fluent { $nazwa }.
privacy-not-configured = Telemetria nie jest skonfigurowana dla tej aplikacji.
privacy-a11y-group-name = Ustawienia prywatności i telemetrii
privacy-heading = Prywatność i telemetria
privacy-notice-controller = Dane są przetwarzane przez { $processor }; podmiotem przetwarzającym od strony technicznej jest { $adapter } (punkt zbierania danych: { $endpoint }).
privacy-notice-purposes = Cele: ulepszanie aplikacji — z których funkcji korzystasz, gdzie skupiają się błędy, na jakich platformach aplikacja działa. Żadnej treści dokumentów, żadnego schowka, żadnych naciśnięć klawiszy, żadnych zrzutów ekranu.
privacy-notice-lawful-anonymous = Podstawa prawna: nasz prawnie uzasadniony interes polegający na ulepszaniu produktu (art. 6 ust. 1 lit. f RODO; zwolnienie CNIL dotyczące pomiaru oglądalności).
privacy-notice-lawful-pseudonymous = Podstawa prawna: Twoja wyraźna zgoda (art. 6 ust. 1 lit. a RODO).
privacy-notice-retention = Okres przechowywania danych po stronie serwera (w dniach): maksymalnie { $days }.
privacy-notice-withdrawal-right = Prawo do wycofania zgody: w każdej chwili możesz wyłączyć dowolny przełącznik poniżej, kliknąć „Wycofaj zgodę”, aby zatrzymać całe zbieranie danych, albo w trybie pseudonimowym kliknąć „Usuń moje dane”, aby usunąć rekordy z serwera.
privacy-notice-policy-link = Pełna polityka prywatności: { $url }

privacy-scope-section-heading = Co aplikacja może udostępniać?
privacy-scope-anonymous-metrics-label = Anonimowe statystyki użycia
privacy-scope-anonymous-metrics-description = Liczba użyć przycisków / pozycji menu / skrótów klawiszowych, a także wersja aplikacji i system operacyjny.
privacy-scope-crash-reports-label = Raporty o awariach
privacy-scope-crash-reports-description = Ślady stosu i metadane procesu w chwili awarii aplikacji. Bez treści dokumentów, bez ścieżek plików.
privacy-scope-feature-flags-label = Flagi funkcji
privacy-scope-feature-flags-description = Pozwala aplikacji odbierać aktualizacje flag funkcji (np. stopniowe udostępnianie nowych narzędzi).

privacy-btn-reject-all = Odrzuć wszystko
privacy-btn-accept-all = Zaakceptuj wszystko
privacy-btn-erase = Usuń moje dane
privacy-btn-erase-tooltip = Prosi serwer o usunięcie wszystkich zdarzeń zapisanych dla tej instalacji, a następnie wycofuje zgodę lokalnie.
privacy-btn-fetch = Pobierz moje dane
privacy-btn-fetch-tooltip = Pobiera wszystkie zdarzenia zapisane na serwerze pod Twoim identyfikatorem instalacji. Wynik możesz zapisać w formacie JSON.
privacy-btn-withdraw = Wycofaj zgodę
privacy-btn-withdraw-tooltip = Zatrzymuje zbieranie nowych danych. Dane już zapisane na serwerze zostają zachowane — jeśli chcesz je usunąć, użyj najpierw opcji „Usuń moje dane”.
privacy-btn-switch-to-anonymous = Przełącz na tryb anonimowy
privacy-btn-switch-to-pseudonymous = Przełącz na tryb pseudonimowy

privacy-identity-heading = Twoje dane na serwerze
privacy-identity-install-id = Identyfikator instalacji: { $id }
privacy-identity-retention = Okres przechowywania Twoich rekordów na serwerze (w dniach): maksymalnie { $days }.

privacy-mode-heading = Tryb prywatności
privacy-mode-current-anonymous = Bieżący: anonimowy (bez identyfikatora instalacji)
privacy-mode-current-pseudonymous = Bieżący: pseudonimowy (identyfikator instalacji obecny)
privacy-mode-blurb-anonymous = Tryb anonimowy nie przesyła żadnego identyfikatora urządzenia. Przełączenie usunie Twoje dotychczasowe rekordy na serwerze i skasuje lokalny UUID instalacji — tej operacji nie można cofnąć.
privacy-mode-blurb-pseudonymous = Tryb pseudonimowy generuje losowy UUID instalacji. Będziesz mieć możliwość pobrania lub usunięcia swoich rekordów na serwerze. Wymaga wyraźnej zgody i pyta o nią ponownie przy przełączeniu.

privacy-confirm-mode-switch-title = Zmienić tryb prywatności?
privacy-confirm-mode-switch-leaving-pseudonymous = Ta operacja poprosi serwer o usunięcie wszystkich zdarzeń zapisanych pod Twoim identyfikatorem instalacji, skasuje lokalny UUID instalacji, zresetuje Twoją decyzję o zgodzie i zmieni tryb prywatności. Czy chcesz kontynuować?
privacy-confirm-mode-switch-leaving-anonymous = Ta operacja zresetuje Twoją decyzję o zgodzie i zmieni tryb prywatności. Zanim zostaną zebrane jakiekolwiek nowe dane, zapytamy ponownie o zgodę. Kontynuować?
privacy-confirm-erase-title = Usunąć Twoje dane?
privacy-confirm-erase-text = Ta operacja wysyła żądanie usunięcia każdego zdarzenia zapisanego pod Twoim identyfikatorem instalacji, kasuje wszystko, co pozostaje jeszcze w lokalnym buforze, i wycofuje zgodę, aby nie zbierano dalszych danych. Tej operacji nie można cofnąć.
privacy-confirm-withdraw-title = Wycofać zgodę?
privacy-confirm-withdraw-text = Z tej aplikacji nie będą już zbierane żadne zdarzenia analityczne. Dane już zapisane na serwerze zostają zachowane — jeśli chcesz je również usunąć, przed wycofaniem zgody użyj opcji „Usuń moje dane”.

privacy-fetch-success-title = Twoje dane na serwerze
privacy-fetch-success-text = Pobrano zdarzenia dla tej instalacji: { $count }.
privacy-fetch-saved-to = Zapisano w: { $path }
privacy-fetch-write-error = Nie można zapisać pliku { $path }: { $error }
privacy-fetch-error-title = Nie udało się pobrać Twoich danych

privacy-inspect-title = Podgląd wysyłanych danych (zdarzenia w buforze: { $count })
privacy-inspect-empty = W tej sesji nie wyemitowano jeszcze żadnych zdarzeń. Skorzystaj z aplikacji — kliknięcia, menu i skróty klawiszowe przechodzą właśnie tędy.
privacy-inspect-summary = Wyświetlane są ostatnie zdarzenia ({ $count }), od najnowszych.

# Kalendarz / DateEdit / TimeEdit / DateTimeEdit. Zobacz
# crates/teksilo-widgets/src/{calendar,date_edit,time_edit,date_time_edit}.rs
# oraz wspólne moduły w crates/teksilo-widgets/src/common/datetime/.
# Nazwy miesięcy i dni tygodnia w formie samodzielnej (CLDR stand-alone).
calendar-month-long-january = styczeń
calendar-month-long-february = luty
calendar-month-long-march = marzec
calendar-month-long-april = kwiecień
calendar-month-long-may = maj
calendar-month-long-june = czerwiec
calendar-month-long-july = lipiec
calendar-month-long-august = sierpień
calendar-month-long-september = wrzesień
calendar-month-long-october = październik
calendar-month-long-november = listopad
calendar-month-long-december = grudzień

calendar-month-short-january = sty
calendar-month-short-february = lut
calendar-month-short-march = mar
calendar-month-short-april = kwi
calendar-month-short-may = maj
calendar-month-short-june = cze
calendar-month-short-july = lip
calendar-month-short-august = sie
calendar-month-short-september = wrz
calendar-month-short-october = paź
calendar-month-short-november = lis
calendar-month-short-december = gru

calendar-weekday-long-monday = poniedziałek
calendar-weekday-long-tuesday = wtorek
calendar-weekday-long-wednesday = środa
calendar-weekday-long-thursday = czwartek
calendar-weekday-long-friday = piątek
calendar-weekday-long-saturday = sobota
calendar-weekday-long-sunday = niedziela

calendar-weekday-short-monday = pon.
calendar-weekday-short-tuesday = wt.
calendar-weekday-short-wednesday = śr.
calendar-weekday-short-thursday = czw.
calendar-weekday-short-friday = pt.
calendar-weekday-short-saturday = sob.
calendar-weekday-short-sunday = niedz.

calendar-weekday-narrow-monday = P
calendar-weekday-narrow-tuesday = W
calendar-weekday-narrow-wednesday = Ś
calendar-weekday-narrow-thursday = C
calendar-weekday-narrow-friday = P
calendar-weekday-narrow-saturday = S
calendar-weekday-narrow-sunday = N

calendar-button-previous-month = Poprzedni miesiąc
calendar-button-next-month = Następny miesiąc
calendar-button-previous-year = Poprzedni rok
calendar-button-next-year = Następny rok
calendar-button-today = Dzisiaj
calendar-button-month-picker = Wybierz miesiąc
calendar-button-year-picker = Wybierz rok
calendar-week-number-column = Tydz.
calendar-name = Kalendarz
calendar-months-grid-label = Miesiące
calendar-years-grid-label = Lata
calendar-name-with-month = Kalendarz, { $month } { $year }
calendar-cell-name = { $weekday }, { $day }, { $month } { $year }
calendar-range-status = Zaznaczono: { $start } – { $end }

date-edit-segment-year = Rok
date-edit-segment-month = Miesiąc
date-edit-segment-day = Dzień
date-edit-calendar-button = Wybierz datę
date-edit-trigger-tooltip = Otwórz kalendarz
date-edit-name = Data
date-edit-placeholder = Wybierz datę

time-edit-segment-hour = Godzina
time-edit-segment-minute = Minuta
time-edit-segment-second = Sekunda
time-edit-segment-period = AM/PM
time-edit-period-am = AM
time-edit-period-pm = PM
time-edit-name = Godzina
time-edit-placeholder = Wybierz godzinę

date-time-edit-name = Data i godzina
date-time-edit-placeholder = Wybierz datę i godzinę
date-time-edit-date-name = Data
date-time-edit-time-name = Godzina
date-time-edit-trigger-tooltip = Otwórz kalendarz
date-range-edit-name = Zakres dat
date-range-edit-placeholder = Wybierz zakres dat
date-range-edit-start-name = Data początkowa
date-range-edit-end-name = Data końcowa
date-range-edit-trigger-tooltip = Otwórz kalendarz zakresu

# Komunikaty walidacji (TextInputField + DateEdit/TimeEdit)
validation-corrected-to = Poprawiono na { $value }
validation-corrected-with-notes = Poprawiono: { $notes }
validation-segment-clamped = { $segment } { $raw } → { $clamped }
validation-day-clamped-to-month = dzień { $raw } → { $clamped } (ostatni dzień miesiąca)
validation-clamped-to-range = ograniczono do dozwolonego zakresu
validation-segment-year = rok
validation-segment-month = miesiąc
validation-segment-day = dzień
validation-segment-hour = godzina
validation-segment-minute = minuta
validation-segment-second = sekunda
validation-segment-value = wartość
date-edit-validation-not-a-date = Nieprawidłowa data
time-edit-validation-not-a-time = Nieprawidłowa godzina

# ── wybór koloru ──
color-picker-name = Wybór koloru
color-picker-hue-label = Odcień
color-picker-saturation-label = Nasycenie
color-picker-value-label = Jasność
color-picker-alpha-label = Krycie
color-picker-red-label = Czerwony
color-picker-green-label = Zielony
color-picker-blue-label = Niebieski
color-picker-red-short = R
color-picker-green-short = G
color-picker-blue-short = B
color-picker-alpha-short = A
color-picker-hue-short = H
color-picker-saturation-short = S
color-picker-value-short = V
color-picker-hex-label = Hex
color-picker-hex-placeholder = #RRGGBB
color-picker-current-color-label = Wybrany kolor
color-picker-current-color-readout = Wybrany kolor { $hex }
color-picker-swatches-name = Predefiniowane kolory
color-picker-swatch-label = Próbka { $hex }
color-picker-swatch-selected-suffix = , wybrana
color-picker-changed-announcement = Kolor zmieniony na { $hex }
color-picker-done-label = Gotowe
color-picker-cancel-label = Anuluj
color-edit-trigger-name = Kolor { $hex }
color-edit-trigger-name-empty = Kolor, brak
color-edit-trigger-tooltip = Otwórz wybór koloru
hex-color-input-invalid = Nieprawidłowy kolor szesnastkowy (oczekiwano #RRGGBB)
hex-color-input-invalid-with-alpha = Nieprawidłowy kolor szesnastkowy (oczekiwano #RRGGBB lub #RRGGBBAA)
hex-color-input-corrected-shortform = Rozwinięto { $raw } do { $value }
hex-color-input-corrected-uppercase = Znormalizowano do { $value }
hex-color-input-placeholder = #RRGGBB
hex-color-input-placeholder-with-alpha = #RRGGBBAA
color-edit-trigger-empty-placeholder = —

# Etykieta „więcej” w rozwinięciu bogatej podpowiedzi (tytuł akordeonu
# odsłaniający rozbudowaną treść w przypiętej podpowiedzi).
tooltip-more = Więcej

# Wbudowane pozycje menu kontekstowego pól tekstowych i edytora tekstu
# sformatowanego.
menu-cut = Wytnij
menu-copy = Kopiuj
menu-paste = Wklej
menu-paste-unformatted = Wklej bez formatowania
menu-select-all = Zaznacz wszystko
menu-toggle-blockquote = Przełącz cytat blokowy
menu-remove-blockquote = Usuń cytat blokowy

# DropZone — komunikaty obszaru „live” (czytniki ekranu). Liczba pojedyncza
# i mnoga są wybierane w kodzie Rust, a nie wyrażeniem select Fluent, dlatego
# formy z licznikiem zapisano tak, by były poprawne dla każdej liczby.
# Zobacz en-US.ftl po pełny kontekst i crates/teksilo-widgets/src/drop_zone.rs.
drop-zone-hover-file-one = Upuść, aby dodać 1 plik
drop-zone-hover-file-many = Upuść, aby dodać pliki: { $count }
drop-zone-hover-text = Upuść, aby dodać tekst
drop-zone-hover-link-one = Upuść, aby dodać 1 link
drop-zone-hover-link-many = Upuść, aby dodać linki: { $count }
drop-zone-hover-generic = Upuść tutaj
drop-zone-hover-reject = Tego elementu nie można tutaj upuścić
drop-zone-added-file-one = Dodano 1 plik
drop-zone-added-file-many = Dodano pliki: { $count }
drop-zone-added-text = Dodano tekst
drop-zone-added-link-one = Dodano 1 link
drop-zone-added-link-many = Dodano linki: { $count }
drop-zone-rejected = Element nie został przyjęty

# Widget ThemeSwitcher. Zobacz crates/teksilo-widgets/src/theme_switcher.rs.
theme-switcher-label = Motyw
theme-switcher-light = Jasny
theme-switcher-dark = Ciemny
theme-switcher-system = Systemowy

# Widget FontPicker. Zobacz crates/teksilo-widgets/src/font_picker.rs.
font-picker-label = Czcionka
font-picker-placeholder = Wybierz czcionkę…

# Powiadomienie o niepowodzeniu zapisu ustawień. Zobacz en-US.ftl po pełny
# kontekst (wywoływane przez ToastRegistry::show_settings_write_failed
# poprzez teksilo::install_toast).
settings-write-failed-toast-title = Nie można zapisać ustawień
settings-write-failed-toast-body = Nie udało się zapisać { $file }; liczba prób: { $attempts }. Odrzucone zmiany oczekujące w kolejce: { $dropped }. { $message }

# Zapasowe menu okna, otwierane prawym przyciskiem myszy na własnym pasku
# tytułu tam, gdzie system nie udostępnia menu okna (X11). Zobacz en-US.ftl
# po pełny kontekst i crates/teksilo-widgets/src/title_bar/window_menu.rs.
window-menu-restore = Przywróć
window-menu-maximize = Maksymalizuj
window-menu-minimize = Minimalizuj
window-menu-close = Zamknij

# Rozwijanie treści powiadomienia. Zobacz en-US.ftl po pełny kontekst
# i crates/teksilo-widgets/src/toast/body.rs.
toast-show-more = Pokaż więcej
toast-show-less = Pokaż mniej
toast-copy-body = Kopiuj
toast-body-copied = Skopiowano

# Paleta poleceń. Zobacz en-US.ftl po pełny kontekst
# i crates/teksilo-widgets/src/command_palette.rs.
command-palette-placeholder = Wpisz polecenie
command-palette-empty = Brak pasujących poleceń
command-palette-title = Paleta poleceń
command-palette-result-count =
    { $count ->
        [0] Brak pasujących poleceń
        [one] 1 polecenie
        [few] { $count } polecenia
        [many] { $count } poleceń
       *[other] { $count } polecenia
    }
