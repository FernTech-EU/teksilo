# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

# teksilo-widgets Framework-Zeichenketten — deutsche Übersetzung.
#
# Nur zur Laufzeit: Anwendungen, die dieses Gebietsschema über
# `I18nConfig::framework_locales(teksilo_widgets::framework_locales())`
# registrieren, erhalten diese Übersetzungen zusätzlich zu en-US. In
# de-DE fehlende Schlüssel fallen über die manuelle Fallback-Kette von
# `I18nManager::resolve_widget` auf die Quelle en-US zurück
# (App-Überschreibung aktiv → Framework aktiv → App-Überschreibung
# Quelle → Framework-Quelle → Schlüsselplatzhalter). Das ist der
# eigene Fallback von teksilo-i18n, nicht der eingebaute Fallback pro
# Schlüssel von `fluent-bundle` — jedes `FluentBundle` wird mit nur
# einem Gebietsschema in seiner Kette erzeugt, die Suche über mehrere
# Gebietsschemata erfolgt in der Schicht `I18nManager`.

a11y-status-bar-name = Status
a11y-dialog-name = Dialogfeld
a11y-tooltip-name = QuickInfo
a11y-snackbar-name = Benachrichtigung
a11y-splitter-divider-name = Trennleiste
a11y-splitter-pane = Bereich
a11y-splitter-collapsed = Eingeklappt
a11y-splitter-expanded = Ausgeklappt
a11y-breadcrumb-current-page-value = aktuelle Seite
a11y-toolbar-name = Symbolleiste
toolbar-more = Mehr
segmented-control-more = Weitere Optionen
breadcrumb-overflow = Ausgeblendeten Pfad anzeigen
a11y-title-bar-name = Titelleiste des Fensters
a11y-window-controls-name = Fenstersteuerelemente
a11y-window-minimize-name = Minimieren
a11y-window-maximize-name = Maximieren
a11y-window-restore-name = Wiederherstellen
a11y-window-close-name = Schließen
a11y-stepper-indicator-strip-name = Schritte
a11y-stepper-content-name = Schrittinhalt
tab-close-tooltip = Registerkarte schließen
a11y-builtin-browse = Durchsuchen
a11y-builtin-expand = Vergrößern
a11y-builtin-search = Suchen
a11y-builtin-copy = Kopieren
a11y-builtin-clear = Löschen
a11y-builtin-add = Hinzufügen
a11y-builtin-bell = Benachrichtigungen
a11y-builtin-menu = Menü
a11y-builtin-more = Weitere Aktionen
a11y-builtin-visibility = Ein-/ausblenden
a11y-password-reveal = Kennwort ein-/ausblenden
a11y-caps-lock-on = Feststelltaste ist aktiviert
notifications-title = Benachrichtigungen
notifications-empty = Keine Benachrichtigungen
notifications-mark-all-read = Alle als gelesen markieren
notifications-clear = Alle löschen
notifications-filter-placeholder = Benachrichtigungen durchsuchen
notifications-bucket-today = Heute
notifications-bucket-yesterday = Gestern
notifications-bucket-this-week = Diese Woche
notifications-bucket-earlier = Früher
notifications-archive-replay-disabled = (nicht mehr verfügbar)
a11y-shortcut-settings-name = Einstellungen für Tastenkombinationen
a11y-shortcut-settings-capture-hint = Beliebige Taste drücken. Entf zum Löschen. Esc zum Abbrechen.
keystroke-modifier-ctrl = Strg
keystroke-modifier-shift = Umschalt
keystroke-modifier-alt = Alt
keystroke-modifier-super = Super
keystroke-separator = +
keystroke-key-space = Leertaste
keystroke-key-enter = Eingabe
keystroke-key-escape = Esc
keystroke-key-tab = Tab
keystroke-key-backspace = Rücktaste
keystroke-key-delete = Entf
keystroke-key-arrow-up = Auf
keystroke-key-arrow-down = Ab
keystroke-key-arrow-left = Links
keystroke-key-arrow-right = Rechts
keystroke-key-home = Pos1
keystroke-key-end = Ende
keystroke-key-page-up = Bild auf
keystroke-key-page-down = Bild ab

# MessageBox — Standardschaltflächen und Detailanzeige.
messagebox-btn-ok = OK
messagebox-btn-cancel = Abbrechen
messagebox-btn-close = Schließen
messagebox-btn-yes = Ja
messagebox-btn-no = Nein
messagebox-btn-yes-to-all = Ja für alle
messagebox-btn-no-to-all = Nein für alle
messagebox-btn-save = Speichern
messagebox-btn-save-all = Alle speichern
messagebox-btn-discard = Verwerfen
messagebox-btn-apply = Übernehmen
messagebox-btn-reset = Zurücksetzen
messagebox-btn-restore-defaults = Standardwerte wiederherstellen
messagebox-btn-abort = Abbruch
messagebox-btn-retry = Wiederholen
messagebox-btn-ignore = Ignorieren
messagebox-btn-open = Öffnen
messagebox-btn-help = Hilfe
messagebox-show-details = Details anzeigen

# Widget PrivacySettings. Siehe crates/teksilo-widgets/src/privacy_settings.rs.
# Informationspflicht nach DSGVO Art. 13 sowie Aktionsschaltflächen. Schlüssel
# mit Parametern verwenden die Fluent-Syntax { $name }.
privacy-not-configured = Für diese Anwendung ist keine Telemetrie konfiguriert.
privacy-a11y-group-name = Einstellungen zu Datenschutz und Telemetrie
privacy-heading = Datenschutz und Telemetrie
privacy-notice-controller = Die Daten werden von { $processor } verarbeitet; technischer Auftragsverarbeiter ist { $adapter } (Endpunkt: { $endpoint }).
privacy-notice-purposes = Zwecke: Verbesserung der Anwendung — welche Funktionen genutzt werden, wo sich Fehler häufen, auf welchen Plattformen die Anwendung läuft. Keine Dokumentinhalte, keine Zwischenablage, keine Tastenanschläge, keine Bildschirmaufnahmen.
privacy-notice-lawful-anonymous = Rechtsgrundlage: unser berechtigtes Interesse an der Produktverbesserung (Art. 6 Abs. 1 lit. f DSGVO; Ausnahme der CNIL für die Reichweitenmessung).
privacy-notice-lawful-pseudonymous = Rechtsgrundlage: Ihre ausdrückliche Einwilligung (Art. 6 Abs. 1 lit. a DSGVO).
privacy-notice-retention = Speicherdauer serverseitiger Daten in Tagen: höchstens { $days }.
privacy-notice-withdrawal-right = Widerrufsrecht: Sie können die untenstehenden Schalter jederzeit deaktivieren, auf „Einwilligung widerrufen“ klicken, um jede Erhebung zu beenden, oder im pseudonymen Modus über „Meine Daten löschen“ die Datensätze auf dem Server löschen lassen.
privacy-notice-policy-link = Vollständige Datenschutzerklärung: { $url }

privacy-scope-section-heading = Was darf die Anwendung übermitteln?
privacy-scope-anonymous-metrics-label = Anonyme Nutzungsstatistiken
privacy-scope-anonymous-metrics-description = Zählungen, welche Schaltflächen / Menüeinträge / Tastenkombinationen verwendet werden, dazu Anwendungsversion und Betriebssystem.
privacy-scope-crash-reports-label = Absturzberichte
privacy-scope-crash-reports-description = Stapelüberwachungen und Prozessmetadaten beim Absturz der Anwendung. Keine Dokumentinhalte, keine Dateipfade.
privacy-scope-feature-flags-label = Feature-Flags
privacy-scope-feature-flags-description = Ermöglicht der Anwendung, Aktualisierungen von Feature-Flags zu empfangen (z. B. schrittweise Einführung neuer Werkzeuge).

privacy-btn-reject-all = Alle ablehnen
privacy-btn-accept-all = Alle akzeptieren
privacy-btn-erase = Meine Daten löschen
privacy-btn-erase-tooltip = Fordert den Server auf, jedes für diese Installation aufgezeichnete Ereignis zu löschen, und widerruft anschließend lokal die Einwilligung.
privacy-btn-fetch = Meine Daten abrufen
privacy-btn-fetch-tooltip = Ruft jedes Ereignis ab, das der Server unter Ihrer Installations-ID aufgezeichnet hat. Das Ergebnis lässt sich als JSON speichern.
privacy-btn-withdraw = Einwilligung widerrufen
privacy-btn-withdraw-tooltip = Beendet die Erhebung neuer Daten. Bereits auf dem Server gespeicherte Daten bleiben erhalten — verwenden Sie zuerst „Meine Daten löschen“, wenn diese gelöscht werden sollen.
privacy-btn-switch-to-anonymous = Zum anonymen Modus wechseln
privacy-btn-switch-to-pseudonymous = Zum pseudonymen Modus wechseln

privacy-identity-heading = Ihre Daten auf dem Server
privacy-identity-install-id = Installations-ID: { $id }
privacy-identity-retention = Aufbewahrungsdauer Ihrer Datensätze auf dem Server in Tagen: höchstens { $days }.

privacy-mode-heading = Datenschutzmodus
privacy-mode-current-anonymous = Aktuell: Anonym (keine Installations-ID)
privacy-mode-current-pseudonymous = Aktuell: Pseudonym (Installations-ID vorhanden)
privacy-mode-blurb-anonymous = Der anonyme Modus überträgt keine gerätebezogene Kennung. Beim Wechsel werden Ihre vorhandenen Datensätze auf dem Server gelöscht und die lokale Installations-UUID verworfen — das lässt sich nicht rückgängig machen.
privacy-mode-blurb-pseudonymous = Der pseudonyme Modus erzeugt eine zufällige Installations-UUID. Sie können Ihre Datensätze auf dem Server abrufen oder löschen. Erfordert eine ausdrückliche Einwilligung und fragt beim Wechsel erneut nach.

privacy-confirm-mode-switch-title = Datenschutzmodus wechseln?
privacy-confirm-mode-switch-leaving-pseudonymous = Dadurch wird der Server aufgefordert, jedes unter Ihrer Installations-ID aufgezeichnete Ereignis zu löschen; die lokale Installations-UUID wird verworfen, Ihre Einwilligungsentscheidung zurückgesetzt und der Datenschutzmodus gewechselt. Möchten Sie fortfahren?
privacy-confirm-mode-switch-leaving-anonymous = Dadurch wird Ihre Einwilligungsentscheidung zurückgesetzt und der Datenschutzmodus gewechselt. Sie werden erneut gefragt, bevor neue Daten erhoben werden. Fortfahren?
privacy-confirm-erase-title = Ihre Daten löschen?
privacy-confirm-erase-text = Dadurch wird für jedes unter Ihrer Installations-ID aufgezeichnete Ereignis ein Löschantrag gesendet, alles lokal noch Gepufferte verworfen und die Einwilligung widerrufen, sodass keine weiteren Daten erhoben werden. Die Aktion lässt sich nicht rückgängig machen.
privacy-confirm-withdraw-title = Einwilligung widerrufen?
privacy-confirm-withdraw-text = Aus dieser Anwendung werden keine weiteren Analyseereignisse erhoben. Bereits auf dem Server gespeicherte Daten bleiben erhalten — verwenden Sie vor dem Widerruf „Meine Daten löschen“, wenn diese ebenfalls gelöscht werden sollen.

privacy-fetch-success-title = Ihre Daten auf dem Server
privacy-fetch-success-text = Für diese Installation abgerufene Ereignisse: { $count }.
privacy-fetch-saved-to = Gespeichert unter: { $path }
privacy-fetch-write-error = Datei { $path } konnte nicht geschrieben werden: { $error }
privacy-fetch-error-title = Ihre Daten konnten nicht abgerufen werden

privacy-inspect-title = Gesendete Daten prüfen (gepufferte Ereignisse: { $count })
privacy-inspect-empty = In dieser Sitzung wurden noch keine Ereignisse gesendet. Interagieren Sie mit der Anwendung — Klicks, Menüs und Tastenkombinationen laufen alle hier durch.
privacy-inspect-summary = Zuletzt erfasste Ereignisse: { $count }, die neuesten zuerst.

# Kalender / DateEdit / TimeEdit / DateTimeEdit. Siehe
# crates/teksilo-widgets/src/{calendar,date_edit,time_edit,date_time_edit}.rs
# und die gemeinsamen Module unter crates/teksilo-widgets/src/common/datetime/.
calendar-month-long-january = Januar
calendar-month-long-february = Februar
calendar-month-long-march = März
calendar-month-long-april = April
calendar-month-long-may = Mai
calendar-month-long-june = Juni
calendar-month-long-july = Juli
calendar-month-long-august = August
calendar-month-long-september = September
calendar-month-long-october = Oktober
calendar-month-long-november = November
calendar-month-long-december = Dezember

calendar-month-short-january = Jan.
calendar-month-short-february = Feb.
calendar-month-short-march = März
calendar-month-short-april = Apr.
calendar-month-short-may = Mai
calendar-month-short-june = Juni
calendar-month-short-july = Juli
calendar-month-short-august = Aug.
calendar-month-short-september = Sept.
calendar-month-short-october = Okt.
calendar-month-short-november = Nov.
calendar-month-short-december = Dez.

calendar-weekday-long-monday = Montag
calendar-weekday-long-tuesday = Dienstag
calendar-weekday-long-wednesday = Mittwoch
calendar-weekday-long-thursday = Donnerstag
calendar-weekday-long-friday = Freitag
calendar-weekday-long-saturday = Samstag
calendar-weekday-long-sunday = Sonntag

calendar-weekday-short-monday = Mo.
calendar-weekday-short-tuesday = Di.
calendar-weekday-short-wednesday = Mi.
calendar-weekday-short-thursday = Do.
calendar-weekday-short-friday = Fr.
calendar-weekday-short-saturday = Sa.
calendar-weekday-short-sunday = So.

calendar-weekday-narrow-monday = M
calendar-weekday-narrow-tuesday = D
calendar-weekday-narrow-wednesday = M
calendar-weekday-narrow-thursday = D
calendar-weekday-narrow-friday = F
calendar-weekday-narrow-saturday = S
calendar-weekday-narrow-sunday = S

calendar-button-previous-month = Vorheriger Monat
calendar-button-next-month = Nächster Monat
calendar-button-previous-year = Vorheriges Jahr
calendar-button-next-year = Nächstes Jahr
calendar-button-today = Heute
calendar-button-month-picker = Monat wählen
calendar-button-year-picker = Jahr wählen
calendar-week-number-column = KW
calendar-name = Kalender
calendar-months-grid-label = Monate
calendar-years-grid-label = Jahre
calendar-name-with-month = Kalender, { $month } { $year }
calendar-cell-name = { $weekday }, { $day }. { $month } { $year }
calendar-range-status = Auswahl: { $start } – { $end }

date-edit-segment-year = Jahr
date-edit-segment-month = Monat
date-edit-segment-day = Tag
date-edit-calendar-button = Datum wählen
date-edit-trigger-tooltip = Kalender öffnen
date-edit-name = Datum
date-edit-placeholder = Datum auswählen

time-edit-segment-hour = Stunde
time-edit-segment-minute = Minute
time-edit-segment-second = Sekunde
time-edit-segment-period = AM/PM
time-edit-period-am = AM
time-edit-period-pm = PM
time-edit-name = Uhrzeit
time-edit-placeholder = Uhrzeit auswählen

date-time-edit-name = Datum und Uhrzeit
date-time-edit-placeholder = Datum und Uhrzeit auswählen
date-time-edit-date-name = Datum
date-time-edit-time-name = Uhrzeit
date-time-edit-trigger-tooltip = Kalender öffnen
date-range-edit-name = Datumsbereich
date-range-edit-placeholder = Datumsbereich auswählen
date-range-edit-start-name = Startdatum
date-range-edit-end-name = Enddatum
date-range-edit-trigger-tooltip = Bereichskalender öffnen

# Validierungsrückmeldung (TextInputField + DateEdit/TimeEdit)
validation-corrected-to = Automatisch korrigiert auf { $value }
validation-corrected-with-notes = Automatisch korrigiert: { $notes }
validation-segment-clamped = { $segment } { $raw } → { $clamped }
validation-day-clamped-to-month = Tag { $raw } → { $clamped } (letzter Tag des Monats)
validation-clamped-to-range = auf den zulässigen Bereich begrenzt
validation-segment-year = Jahr
validation-segment-month = Monat
validation-segment-day = Tag
validation-segment-hour = Stunde
validation-segment-minute = Minute
validation-segment-second = Sekunde
validation-segment-value = Wert
date-edit-validation-not-a-date = Kein gültiges Datum
time-edit-validation-not-a-time = Keine gültige Uhrzeit

# ── Farbauswahl ──
color-picker-name = Farbauswahl
color-picker-hue-label = Farbton
color-picker-saturation-label = Sättigung
color-picker-value-label = Helligkeit
color-picker-alpha-label = Deckkraft
color-picker-red-label = Rot
color-picker-green-label = Grün
color-picker-blue-label = Blau
color-picker-red-short = R
color-picker-green-short = G
color-picker-blue-short = B
color-picker-alpha-short = A
color-picker-hue-short = F
color-picker-saturation-short = S
color-picker-value-short = H
color-picker-hex-label = Hex
color-picker-hex-placeholder = #RRGGBB
color-picker-current-color-label = Ausgewählte Farbe
color-picker-current-color-readout = Ausgewählte Farbe { $hex }
color-picker-swatches-name = Farbvorgaben
color-picker-swatch-label = Farbfeld { $hex }
color-picker-swatch-selected-suffix = , ausgewählt
color-picker-changed-announcement = Farbe geändert in { $hex }
color-picker-done-label = Fertig
color-picker-cancel-label = Abbrechen
color-edit-trigger-name = Farbe { $hex }
color-edit-trigger-name-empty = Farbe, keine
color-edit-trigger-tooltip = Farbauswahl öffnen
hex-color-input-invalid = Kein gültiger Hex-Farbwert (erwartet: #RRGGBB)
hex-color-input-invalid-with-alpha = Kein gültiger Hex-Farbwert (erwartet: #RRGGBB oder #RRGGBBAA)
hex-color-input-corrected-shortform = { $raw } zu { $value } erweitert
hex-color-input-corrected-uppercase = Normalisiert zu { $value }
hex-color-input-placeholder = #RRGGBB
hex-color-input-placeholder-with-alpha = #RRGGBBAA
color-edit-trigger-empty-placeholder = —

# Beschriftung „Mehr“ der Aufklappzeile in erweiterten QuickInfos (der
# Akkordeontitel, der den ausführlichen Text einer angehefteten
# erweiterten QuickInfo einblendet).
tooltip-more = Mehr

# Einträge des Kontextmenüs von Textfeldern und des Rich-Text-Editors.
menu-cut = Ausschneiden
menu-copy = Kopieren
menu-paste = Einfügen
menu-paste-unformatted = Unformatiert einfügen
menu-select-all = Alles auswählen
menu-toggle-blockquote = Zitatblock umschalten
menu-remove-blockquote = Zitatblock entfernen

# DropZone — Ansagen des Live-Bereichs (Bildschirmleseprogramme). Singular
# und Plural werden in Rust ausgewählt, nicht per Fluent-Select. Siehe
# en-US.ftl für den vollständigen Kontext und
# crates/teksilo-widgets/src/drop_zone.rs.
drop-zone-hover-file-one = Ablegen, um 1 Datei hinzuzufügen
drop-zone-hover-file-many = Ablegen, um { $count } Dateien hinzuzufügen
drop-zone-hover-text = Ablegen, um Text hinzuzufügen
drop-zone-hover-link-one = Ablegen, um 1 Link hinzuzufügen
drop-zone-hover-link-many = Ablegen, um { $count } Links hinzuzufügen
drop-zone-hover-generic = Hier ablegen
drop-zone-hover-reject = Dieses Element kann hier nicht abgelegt werden
drop-zone-added-file-one = 1 Datei hinzugefügt
drop-zone-added-file-many = { $count } Dateien hinzugefügt
drop-zone-added-text = Text hinzugefügt
drop-zone-added-link-one = 1 Link hinzugefügt
drop-zone-added-link-many = { $count } Links hinzugefügt
drop-zone-rejected = Element nicht akzeptiert

# Widget ThemeSwitcher. Siehe crates/teksilo-widgets/src/theme_switcher.rs.
theme-switcher-label = Design
theme-switcher-light = Hell
theme-switcher-dark = Dunkel
theme-switcher-system = System

# Widget FontPicker. Siehe crates/teksilo-widgets/src/font_picker.rs.
font-picker-label = Schriftart
font-picker-placeholder = Schriftart auswählen…

# Benachrichtigung bei fehlgeschlagenem Schreiben der Einstellungen. Siehe
# en-US.ftl für den vollständigen Kontext (ausgelöst von
# ToastRegistry::show_settings_write_failed über teksilo::install_toast).
settings-write-failed-toast-title = Einstellungen konnten nicht gespeichert werden
settings-write-failed-toast-body = { $file } konnte nicht gespeichert werden; fehlgeschlagene Versuche: { $attempts }; verworfene Änderungen in der Warteschlange: { $dropped }. { $message }

# Ersatz-Fenstermenü, geöffnet per Rechtsklick auf eine angepasste TitleBar
# dort, wo das Betriebssystem keines bereitstellt (X11). Siehe en-US.ftl für
# den vollständigen Kontext und
# crates/teksilo-widgets/src/title_bar/window_menu.rs.
window-menu-restore = Wiederherstellen
window-menu-maximize = Maximieren
window-menu-minimize = Minimieren
window-menu-close = Schließen

# Aufklappzeile für den Text einer Benachrichtigung. Siehe en-US.ftl für den
# vollständigen Kontext und crates/teksilo-widgets/src/toast/body.rs.
toast-show-more = Mehr anzeigen
toast-show-less = Weniger anzeigen
toast-copy-body = Kopieren
toast-body-copied = Kopiert

# Befehlspalette. Siehe en-US.ftl für den vollständigen Kontext und
# crates/teksilo-widgets/src/command_palette.rs.
command-palette-placeholder = Befehl eingeben
command-palette-empty = Kein passender Befehl
command-palette-title = Befehlspalette
command-palette-result-count =
    { $count ->
        [0] Kein passender Befehl
        [one] 1 Befehl
       *[other] { $count } Befehle
    }
