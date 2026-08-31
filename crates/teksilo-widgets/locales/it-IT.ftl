# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

# teksilo-widgets framework strings — traduzione italiana.
#
# Solo a runtime: le applicazioni che registrano questa lingua tramite
# `I18nConfig::framework_locales(teksilo_widgets::framework_locales())`
# ottengono queste traduzioni accanto a en-US. Le chiavi assenti da
# it-IT ricadono sulla sorgente en-US attraverso la catena di fallback
# manuale di `I18nManager::resolve_widget` (override applicativo attivo →
# framework attivo → sorgente dell'override applicativo → sorgente del
# framework → segnaposto della chiave). Si tratta del fallback proprio di
# teksilo-i18n, non di quello per chiave incorporato in `fluent-bundle`:
# ogni `FluentBundle` viene costruito con una sola lingua nella propria
# catena e la ricerca multilingue è gestita al livello di `I18nManager`.

a11y-status-bar-name = Stato
a11y-dialog-name = Finestra di dialogo
a11y-tooltip-name = Suggerimento
a11y-snackbar-name = Notifica
a11y-splitter-divider-name = Separatore
a11y-splitter-pane = Riquadro
a11y-splitter-collapsed = Compresso
a11y-splitter-expanded = Espanso
a11y-breadcrumb-current-page-value = pagina corrente
a11y-toolbar-name = Barra degli strumenti
toolbar-more = Altro
segmented-control-more = Altre opzioni
breadcrumb-overflow = Mostra il percorso nascosto
a11y-title-bar-name = Barra del titolo della finestra
a11y-window-controls-name = Controlli della finestra
a11y-window-minimize-name = Riduci a icona
a11y-window-maximize-name = Ingrandisci
a11y-window-restore-name = Ripristina
a11y-window-close-name = Chiudi
a11y-stepper-indicator-strip-name = Passaggi
a11y-stepper-content-name = Contenuto del passaggio
tab-close-tooltip = Chiudi scheda
a11y-builtin-browse = Sfoglia
a11y-builtin-expand = Espandi
a11y-builtin-search = Cerca
a11y-builtin-copy = Copia
a11y-builtin-clear = Cancella
a11y-builtin-add = Aggiungi
a11y-builtin-bell = Notifiche
a11y-builtin-menu = Menu
a11y-builtin-more = Altre azioni
a11y-builtin-visibility = Mostra/nascondi
a11y-password-reveal = Mostra o nascondi la password
a11y-caps-lock-on = Blocco maiuscole attivo
notifications-title = Notifiche
notifications-empty = Nessuna notifica
notifications-mark-all-read = Segna tutte come lette
notifications-clear = Cancella tutto
notifications-filter-placeholder = Cerca nelle notifiche
notifications-bucket-today = Oggi
notifications-bucket-yesterday = Ieri
notifications-bucket-this-week = Questa settimana
notifications-bucket-earlier = Meno recenti
notifications-archive-replay-disabled = (non più disponibile)
a11y-shortcut-settings-name = Impostazioni delle scelte rapide
a11y-shortcut-settings-capture-hint = Premi un tasto qualsiasi. Canc per cancellare. Esc per annullare.
keystroke-modifier-ctrl = Ctrl
keystroke-modifier-shift = Maiusc
keystroke-modifier-alt = Alt
keystroke-modifier-super = Super
keystroke-separator = +
keystroke-key-space = Spazio
keystroke-key-enter = Invio
keystroke-key-escape = Esc
keystroke-key-tab = Tab
keystroke-key-backspace = Backspace
keystroke-key-delete = Canc
keystroke-key-arrow-up = Su
keystroke-key-arrow-down = Giù
keystroke-key-arrow-left = Sinistra
keystroke-key-arrow-right = Destra
keystroke-key-home = Home
keystroke-key-end = Fine
keystroke-key-page-up = PagSu
keystroke-key-page-down = PagGiù

# MessageBox — pulsanti standard e visualizzazione dei dettagli.
messagebox-btn-ok = OK
messagebox-btn-cancel = Annulla
messagebox-btn-close = Chiudi
messagebox-btn-yes = Sì
messagebox-btn-no = No
messagebox-btn-yes-to-all = Sì a tutti
messagebox-btn-no-to-all = No a tutti
messagebox-btn-save = Salva
messagebox-btn-save-all = Salva tutto
messagebox-btn-discard = Scarta
messagebox-btn-apply = Applica
messagebox-btn-reset = Reimposta
messagebox-btn-restore-defaults = Ripristina valori predefiniti
messagebox-btn-abort = Interrompi
messagebox-btn-retry = Riprova
messagebox-btn-ignore = Ignora
messagebox-btn-open = Apri
messagebox-btn-help = Aiuto
messagebox-show-details = Mostra i dettagli

# Widget PrivacySettings. Vedi crates/teksilo-widgets/src/privacy_settings.rs.
# Informativa GDPR Art. 13 + pulsanti di azione. Le chiavi con parametri
# usano la sintassi Fluent { $nome }.
privacy-not-configured = La telemetria non è configurata per questa applicazione.
privacy-a11y-group-name = Impostazioni di privacy e telemetria
privacy-heading = Privacy e telemetria
privacy-notice-controller = I dati sono trattati da { $processor }; il responsabile del trattamento sul piano tecnico è { $adapter } (punto di raccolta: { $endpoint }).
privacy-notice-purposes = Finalità: migliorare l'applicazione — quali funzionalità vengono usate, dove si concentrano i bug, su quali piattaforme viene eseguita. Nessun contenuto dei documenti, nessun accesso agli appunti, nessuna registrazione dei tasti premuti, nessuna acquisizione dello schermo.
privacy-notice-lawful-anonymous = Base giuridica: il nostro legittimo interesse al miglioramento del prodotto (GDPR Art. 6(1)(f); esenzione CNIL per la misurazione dell'audience).
privacy-notice-lawful-pseudonymous = Base giuridica: il tuo consenso esplicito (GDPR Art. 6(1)(a)).
privacy-notice-retention =
    { $days ->
        [one] Conservazione: i dati lato server sono conservati per un massimo di 1 giorno.
       *[other] Conservazione: i dati lato server sono conservati per un massimo di { $days } giorni.
    }
privacy-notice-withdrawal-right = Diritto di revoca: puoi disattivare in qualsiasi momento gli interruttori qui sotto, fare clic su «Revoca il consenso» per interrompere ogni raccolta oppure, in modalità pseudonima, su «Cancella i miei dati» per eliminare i record dal server.
privacy-notice-policy-link = Informativa sulla privacy completa: { $url }

privacy-scope-section-heading = Che cosa può condividere l'applicazione?
privacy-scope-anonymous-metrics-label = Statistiche d'uso anonime
privacy-scope-anonymous-metrics-description = Conteggio dei pulsanti / delle voci di menu / delle scelte rapide utilizzati, oltre alla versione dell'applicazione e al sistema operativo.
privacy-scope-crash-reports-label = Segnalazioni di arresto anomalo
privacy-scope-crash-reports-description = Tracce dello stack e metadati del processo quando l'applicazione si arresta in modo anomalo. Nessun contenuto dei documenti, nessun percorso di file.
privacy-scope-feature-flags-label = Flag delle funzionalità
privacy-scope-feature-flags-description = Consente all'applicazione di ricevere aggiornamenti dei flag delle funzionalità (ad esempio il rilascio graduale di nuovi strumenti).

privacy-btn-reject-all = Rifiuta tutto
privacy-btn-accept-all = Accetta tutto
privacy-btn-erase = Cancella i miei dati
privacy-btn-erase-tooltip = Chiede al server di eliminare tutti gli eventi registrati per questa installazione, quindi revoca il consenso in locale.
privacy-btn-fetch = Recupera i miei dati
privacy-btn-fetch-tooltip = Recupera tutti gli eventi che il server ha registrato con il tuo ID di installazione. Puoi salvare il risultato in formato JSON.
privacy-btn-withdraw = Revoca il consenso
privacy-btn-withdraw-tooltip = Interrompe la raccolta di nuovi dati. I dati già registrati sul server vengono conservati: usa prima «Cancella i miei dati» se vuoi eliminarli.
privacy-btn-switch-to-anonymous = Passa alla modalità anonima
privacy-btn-switch-to-pseudonymous = Passa alla modalità pseudonima

privacy-identity-heading = I tuoi dati sul server
privacy-identity-install-id = ID di installazione: { $id }
privacy-identity-retention =
    { $days ->
        [one] Il server conserva i tuoi record per un massimo di 1 giorno.
       *[other] Il server conserva i tuoi record per un massimo di { $days } giorni.
    }

privacy-mode-heading = Modalità di privacy
privacy-mode-current-anonymous = Attuale: anonima (nessun ID di installazione)
privacy-mode-current-pseudonymous = Attuale: pseudonima (ID di installazione presente)
privacy-mode-blurb-anonymous = La modalità anonima non trasmette alcun identificatore del dispositivo. Il passaggio cancellerà i record esistenti sul server ed eliminerà l'UUID di installazione locale: l'operazione non può essere annullata.
privacy-mode-blurb-pseudonymous = La modalità pseudonima genera un UUID di installazione casuale. Potrai recuperare o cancellare i tuoi record sul server. Richiede il consenso esplicito e lo chiede di nuovo al momento del passaggio.

privacy-confirm-mode-switch-title = Cambiare modalità di privacy?
privacy-confirm-mode-switch-leaving-pseudonymous = Questa operazione chiederà al server di cancellare tutti gli eventi registrati con il tuo ID di installazione, eliminerà l'UUID di installazione locale, reimposterà la tua decisione sul consenso e cambierà la modalità di privacy. Vuoi continuare?
privacy-confirm-mode-switch-leaving-anonymous = Questa operazione reimposterà la tua decisione sul consenso e cambierà la modalità di privacy. Ti verrà chiesto di nuovo prima che vengano raccolti nuovi dati. Continuare?
privacy-confirm-erase-title = Cancellare i tuoi dati?
privacy-confirm-erase-text = Questa operazione invia una richiesta di cancellazione per ogni evento registrato con il tuo ID di installazione, elimina tutto ciò che si trova ancora nel buffer locale e revoca il consenso, così da non raccogliere altri dati. L'operazione non può essere annullata.
privacy-confirm-withdraw-title = Revocare il consenso?
privacy-confirm-withdraw-text = Da questa applicazione non verrà raccolto alcun altro evento di analisi. I dati già registrati sul server vengono conservati: usa «Cancella i miei dati» prima di revocare il consenso se vuoi eliminarli.

privacy-fetch-success-title = I tuoi dati sul server
privacy-fetch-success-text = Eventi recuperati per questa installazione: { $count }.
privacy-fetch-saved-to = Salvato in: { $path }
privacy-fetch-write-error = Impossibile scrivere il file { $path }: { $error }
privacy-fetch-error-title = Impossibile recuperare i tuoi dati

privacy-inspect-title = Ispeziona i dati inviati (eventi nel buffer: { $count })
privacy-inspect-empty = In questa sessione non è ancora stato emesso alcun evento. Prova a interagire con l'applicazione: clic, menu e scelte rapide passano tutti da qui.
privacy-inspect-summary = Vengono mostrati gli ultimi eventi ({ $count }), dal più recente al meno recente.

# Calendario / DateEdit / TimeEdit / DateTimeEdit. Vedi
# crates/teksilo-widgets/src/{calendar,date_edit,time_edit,date_time_edit}.rs
# e i moduli condivisi in crates/teksilo-widgets/src/common/datetime/.
calendar-month-long-january = gennaio
calendar-month-long-february = febbraio
calendar-month-long-march = marzo
calendar-month-long-april = aprile
calendar-month-long-may = maggio
calendar-month-long-june = giugno
calendar-month-long-july = luglio
calendar-month-long-august = agosto
calendar-month-long-september = settembre
calendar-month-long-october = ottobre
calendar-month-long-november = novembre
calendar-month-long-december = dicembre

calendar-month-short-january = gen
calendar-month-short-february = feb
calendar-month-short-march = mar
calendar-month-short-april = apr
calendar-month-short-may = mag
calendar-month-short-june = giu
calendar-month-short-july = lug
calendar-month-short-august = ago
calendar-month-short-september = set
calendar-month-short-october = ott
calendar-month-short-november = nov
calendar-month-short-december = dic

calendar-weekday-long-monday = lunedì
calendar-weekday-long-tuesday = martedì
calendar-weekday-long-wednesday = mercoledì
calendar-weekday-long-thursday = giovedì
calendar-weekday-long-friday = venerdì
calendar-weekday-long-saturday = sabato
calendar-weekday-long-sunday = domenica

calendar-weekday-short-monday = lun
calendar-weekday-short-tuesday = mar
calendar-weekday-short-wednesday = mer
calendar-weekday-short-thursday = gio
calendar-weekday-short-friday = ven
calendar-weekday-short-saturday = sab
calendar-weekday-short-sunday = dom

calendar-weekday-narrow-monday = L
calendar-weekday-narrow-tuesday = M
calendar-weekday-narrow-wednesday = M
calendar-weekday-narrow-thursday = G
calendar-weekday-narrow-friday = V
calendar-weekday-narrow-saturday = S
calendar-weekday-narrow-sunday = D

calendar-button-previous-month = Mese precedente
calendar-button-next-month = Mese successivo
calendar-button-previous-year = Anno precedente
calendar-button-next-year = Anno successivo
calendar-button-today = Oggi
calendar-button-month-picker = Scegli il mese
calendar-button-year-picker = Scegli l'anno
calendar-week-number-column = Sett.
calendar-name = Calendario
calendar-months-grid-label = Mesi
calendar-years-grid-label = Anni
calendar-name-with-month = Calendario, { $month } { $year }
calendar-cell-name = { $weekday } { $day } { $month } { $year }
calendar-range-status = Selezione: { $start } – { $end }

date-edit-segment-year = Anno
date-edit-segment-month = Mese
date-edit-segment-day = Giorno
date-edit-calendar-button = Scegli una data
date-edit-trigger-tooltip = Apri il calendario
date-edit-name = Data
date-edit-placeholder = Seleziona una data

time-edit-segment-hour = Ora
time-edit-segment-minute = Minuto
time-edit-segment-second = Secondo
time-edit-segment-period = AM/PM
time-edit-period-am = AM
time-edit-period-pm = PM
time-edit-name = Ora
time-edit-placeholder = Seleziona un'ora

date-time-edit-name = Data e ora
date-time-edit-placeholder = Seleziona data e ora
date-time-edit-date-name = Data
date-time-edit-time-name = Ora
date-time-edit-trigger-tooltip = Apri il calendario
date-range-edit-name = Intervallo di date
date-range-edit-placeholder = Seleziona un intervallo di date
date-range-edit-start-name = Data di inizio
date-range-edit-end-name = Data di fine
date-range-edit-trigger-tooltip = Apri il calendario dell'intervallo

# Riscontro di convalida (TextInputField + DateEdit/TimeEdit)
validation-corrected-to = Corretto automaticamente in { $value }
validation-corrected-with-notes = Corretto automaticamente: { $notes }
validation-segment-clamped = { $segment } { $raw } → { $clamped }
validation-day-clamped-to-month = giorno { $raw } → { $clamped } (ultimo giorno del mese)
validation-clamped-to-range = riportato all'intervallo consentito
validation-segment-year = anno
validation-segment-month = mese
validation-segment-day = giorno
validation-segment-hour = ora
validation-segment-minute = minuto
validation-segment-second = secondo
validation-segment-value = valore
date-edit-validation-not-a-date = Data non valida
time-edit-validation-not-a-time = Ora non valida

# ── selettore colore ──
color-picker-name = Selettore colore
color-picker-hue-label = Tonalità
color-picker-saturation-label = Saturazione
color-picker-value-label = Luminosità
color-picker-alpha-label = Opacità
color-picker-red-label = Rosso
color-picker-green-label = Verde
color-picker-blue-label = Blu
color-picker-red-short = R
color-picker-green-short = G
color-picker-blue-short = B
color-picker-alpha-short = A
color-picker-hue-short = H
color-picker-saturation-short = S
color-picker-value-short = V
color-picker-hex-label = Hex
color-picker-hex-placeholder = #RRGGBB
color-picker-current-color-label = Colore selezionato
color-picker-current-color-readout = Colore selezionato { $hex }
color-picker-swatches-name = Colori predefiniti
color-picker-swatch-label = Campione { $hex }
color-picker-swatch-selected-suffix = , selezionato
color-picker-changed-announcement = Colore cambiato in { $hex }
color-picker-done-label = Fine
color-picker-cancel-label = Annulla
color-edit-trigger-name = Colore { $hex }
color-edit-trigger-name-empty = Colore, nessuno
color-edit-trigger-tooltip = Apri il selettore colore
hex-color-input-invalid = Colore esadecimale non valido (previsto #RRGGBB)
hex-color-input-invalid-with-alpha = Colore esadecimale non valido (previsto #RRGGBB o #RRGGBBAA)
hex-color-input-corrected-shortform = { $raw } espanso in { $value }
hex-color-input-corrected-uppercase = Normalizzato in { $value }
hex-color-input-placeholder = #RRGGBB
hex-color-input-placeholder-with-alpha = #RRGGBBAA
color-edit-trigger-empty-placeholder = —

# Etichetta «altro» della sezione a scomparsa delle descrizioni comando
# avanzate (il titolo della fisarmonica che rivela il corpo esteso di una
# descrizione comando fissata).
tooltip-more = Altro

# Voci del menu contestuale dei campi di testo e dell'editor di testo
# formattato.
menu-cut = Taglia
menu-copy = Copia
menu-paste = Incolla
menu-paste-unformatted = Incolla senza formattazione
menu-select-all = Seleziona tutto
menu-toggle-blockquote = Attiva/disattiva citazione
menu-remove-blockquote = Rimuovi citazione

# DropZone — annunci della regione «live» (lettori di schermo). Vedi
# en-US.ftl per il contesto completo: singolare e plurale sono scelti in
# Rust, non da un'espressione select di Fluent.
drop-zone-hover-file-one = Rilascia per aggiungere 1 file
drop-zone-hover-file-many = Rilascia per aggiungere { $count } file
drop-zone-hover-text = Rilascia per aggiungere del testo
drop-zone-hover-link-one = Rilascia per aggiungere 1 link
drop-zone-hover-link-many = Rilascia per aggiungere { $count } link
drop-zone-hover-generic = Rilascia qui
drop-zone-hover-reject = Questo elemento non può essere rilasciato qui
drop-zone-added-file-one = 1 file aggiunto
drop-zone-added-file-many = { $count } file aggiunti
drop-zone-added-text = Testo aggiunto
drop-zone-added-link-one = 1 link aggiunto
drop-zone-added-link-many = { $count } link aggiunti
drop-zone-rejected = Elemento non accettato

# Widget ThemeSwitcher. Vedi crates/teksilo-widgets/src/theme_switcher.rs.
theme-switcher-label = Tema
theme-switcher-light = Chiaro
theme-switcher-dark = Scuro
theme-switcher-system = Sistema

# Widget FontPicker. Vedi crates/teksilo-widgets/src/font_picker.rs.
font-picker-label = Carattere
font-picker-placeholder = Scegli un carattere…

# Notifica di mancato salvataggio delle impostazioni. Vedi en-US.ftl per il
# contesto completo (attivata da ToastRegistry::show_settings_write_failed
# tramite teksilo::install_toast). Segnala una perdita di dati reale, quindi
# la notifica è di gravità Errore e persistente.
settings-write-failed-toast-title = Impossibile salvare le impostazioni
settings-write-failed-toast-body = Salvataggio di { $file } non riuscito (tentativi: { $attempts }). Modifiche in coda scartate: { $dropped }. { $message }

# Menu finestra di ripiego, aperto con un clic destro su una TitleBar
# personalizzata dove il sistema operativo non ne fornisce uno (X11). Vedi
# en-US.ftl per il contesto completo e
# crates/teksilo-widgets/src/title_bar/window_menu.rs. Ripristina e
# Ingrandisci si escludono a vicenda: ne viene mostrato solo uno alla volta.
window-menu-restore = Ripristina
window-menu-maximize = Ingrandisci
window-menu-minimize = Riduci a icona
window-menu-close = Chiudi

# Espansione del corpo di una notifica. Vedi en-US.ftl per il contesto
# completo e crates/teksilo-widgets/src/toast/body.rs.
toast-show-more = Mostra altro
toast-show-less = Mostra meno
toast-copy-body = Copia
toast-body-copied = Copiato

# CommandPalette. Vedi en-US.ftl per il contesto completo e
# crates/teksilo-widgets/src/command_palette.rs.
command-palette-placeholder = Digita un comando
command-palette-empty = Nessun comando corrispondente
# Nome accessibile della finestra della palette e del suo campo di ricerca.
# Non è mai visibile a schermo, quindi è l'unica indicazione che un utente di
# lettore di schermo riceve su ciò che si è appena aperto.
command-palette-title = Riquadro comandi
# Annunciato come descrizione della finestra e riannunciato man mano che la
# ricerca si restringe, così il numero di corrispondenze è disponibile senza
# scorrere tutto l'elenco.
command-palette-result-count =
    { $count ->
        [0] Nessun comando corrispondente
        [one] 1 comando
        [many] { $count } di comandi
       *[other] { $count } comandi
    }
