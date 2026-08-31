# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

# Συμβολοσειρές του teksilo-widgets — ελληνική μετάφραση.
#
# Μόνο κατά την εκτέλεση: οι εφαρμογές που δηλώνουν αυτήν τη γλώσσα μέσω
# `I18nConfig::framework_locales(teksilo_widgets::framework_locales())`
# λαμβάνουν αυτές τις μεταφράσεις μαζί με τα en-US. Τα κλειδιά που
# λείπουν από το el-GR επανέρχονται στην πηγή en-US μέσω της χειροκίνητης
# αλυσίδας εφεδρείας της `I18nManager::resolve_widget` (παράκαμψη
# εφαρμογής ενεργή → πλαίσιο ενεργό → πηγή παράκαμψης εφαρμογής → πηγή
# πλαισίου → σύμβολο κράτησης θέσης του κλειδιού). Πρόκειται για την
# εφεδρεία του ίδιου του teksilo-i18n και όχι για την ενσωματωμένη
# ανά κλειδί εφεδρεία του `fluent-bundle` — κάθε `FluentBundle`
# κατασκευάζεται με μία μόνο γλώσσα στην αλυσίδα του και η αναζήτηση σε
# πολλές γλώσσες γίνεται στο επίπεδο του `I18nManager`.

a11y-status-bar-name = Κατάσταση
a11y-dialog-name = Παράθυρο διαλόγου
a11y-tooltip-name = Επεξήγηση εργαλείου
a11y-snackbar-name = Ειδοποίηση
a11y-splitter-divider-name = Διαχωριστικό
a11y-splitter-pane = Τμήμα
a11y-splitter-collapsed = Συμπτυγμένο
a11y-splitter-expanded = Αναπτυγμένο
a11y-breadcrumb-current-page-value = τρέχουσα σελίδα
a11y-toolbar-name = Γραμμή εργαλείων
toolbar-more = Περισσότερα
segmented-control-more = Περισσότερες επιλογές
breadcrumb-overflow = Εμφάνιση κρυφής διαδρομής
a11y-title-bar-name = Γραμμή τίτλου παραθύρου
a11y-window-controls-name = Στοιχεία ελέγχου παραθύρου
a11y-window-minimize-name = Ελαχιστοποίηση
a11y-window-maximize-name = Μεγιστοποίηση
a11y-window-restore-name = Επαναφορά
a11y-window-close-name = Κλείσιμο
a11y-stepper-indicator-strip-name = Βήματα
a11y-stepper-content-name = Περιεχόμενο βήματος
tab-close-tooltip = Κλείσιμο καρτέλας
a11y-builtin-browse = Περιήγηση
a11y-builtin-expand = Μεγέθυνση
a11y-builtin-search = Αναζήτηση
a11y-builtin-copy = Αντιγραφή
a11y-builtin-clear = Απαλοιφή
a11y-builtin-add = Προσθήκη
a11y-builtin-bell = Ειδοποιήσεις
a11y-builtin-menu = Μενού
a11y-builtin-more = Περισσότερες ενέργειες
a11y-builtin-visibility = Εμφάνιση/απόκρυψη
a11y-password-reveal = Εμφάνιση ή απόκρυψη κωδικού πρόσβασης
a11y-caps-lock-on = Το Caps Lock είναι ενεργό
notifications-title = Ειδοποιήσεις
notifications-empty = Δεν υπάρχουν ειδοποιήσεις
notifications-mark-all-read = Σήμανση όλων ως αναγνωσμένων
notifications-clear = Απαλοιφή όλων
notifications-filter-placeholder = Αναζήτηση ειδοποιήσεων
notifications-bucket-today = Σήμερα
notifications-bucket-yesterday = Χθες
notifications-bucket-this-week = Αυτήν την εβδομάδα
notifications-bucket-earlier = Παλαιότερα
notifications-archive-replay-disabled = (δεν είναι πλέον διαθέσιμο)
a11y-shortcut-settings-name = Ρυθμίσεις συντομεύσεων
a11y-shortcut-settings-capture-hint = Πατήστε ένα πλήκτρο. Delete για απαλοιφή. Escape για ακύρωση.
keystroke-modifier-ctrl = Ctrl
keystroke-modifier-shift = Shift
keystroke-modifier-alt = Alt
keystroke-modifier-super = Super
keystroke-separator = +
keystroke-key-space = Διάστημα
keystroke-key-enter = Enter
keystroke-key-escape = Esc
keystroke-key-tab = Tab
keystroke-key-backspace = Backspace
keystroke-key-delete = Del
keystroke-key-arrow-up = Επάνω
keystroke-key-arrow-down = Κάτω
keystroke-key-arrow-left = Αριστερά
keystroke-key-arrow-right = Δεξιά
keystroke-key-home = Home
keystroke-key-end = End
keystroke-key-page-up = PgUp
keystroke-key-page-down = PgDn

# MessageBox — τυπικές ετικέτες κουμπιών και εμφάνιση λεπτομερειών.
# Βλ. crates/teksilo-widgets/src/message_box.rs.
messagebox-btn-ok = OK
messagebox-btn-cancel = Άκυρο
messagebox-btn-close = Κλείσιμο
messagebox-btn-yes = Ναι
messagebox-btn-no = Όχι
messagebox-btn-yes-to-all = Ναι σε όλα
messagebox-btn-no-to-all = Όχι σε όλα
messagebox-btn-save = Αποθήκευση
messagebox-btn-save-all = Αποθήκευση όλων
messagebox-btn-discard = Απόρριψη
messagebox-btn-apply = Εφαρμογή
messagebox-btn-reset = Επαναφορά
messagebox-btn-restore-defaults = Επαναφορά προεπιλογών
messagebox-btn-abort = Ματαίωση
messagebox-btn-retry = Επανάληψη
messagebox-btn-ignore = Παράβλεψη
messagebox-btn-open = Άνοιγμα
messagebox-btn-help = Βοήθεια
messagebox-show-details = Εμφάνιση λεπτομερειών

# Widget PrivacySettings. Βλ. crates/teksilo-widgets/src/privacy_settings.rs.
# Ενημέρωση κατά το άρθρο 13 του ΓΚΠΔ και κουμπιά ενεργειών. Τα κλειδιά με
# παραμέτρους χρησιμοποιούν τη σύνταξη Fluent { $όνομα }.
privacy-not-configured = Η τηλεμετρία δεν έχει ρυθμιστεί για αυτήν την εφαρμογή.
privacy-a11y-group-name = Ρυθμίσεις απορρήτου και τηλεμετρίας
privacy-heading = Απόρρητο και τηλεμετρία
privacy-notice-controller = Τα δεδομένα υποβάλλονται σε επεξεργασία από { $processor }· ο τεχνικός εκτελών την επεξεργασία είναι { $adapter } (σημείο συλλογής: { $endpoint }).
privacy-notice-purposes = Σκοποί: βελτίωση της εφαρμογής — ποιες λειτουργίες χρησιμοποιούνται, πού συγκεντρώνονται τα σφάλματα, σε ποιες πλατφόρμες εκτελείται. Κανένα περιεχόμενο εγγράφων, κανένα πρόχειρο, καμία πληκτρολόγηση, καμία λήψη οθόνης.
privacy-notice-lawful-anonymous = Νομική βάση: το έννομο συμφέρον μας για τη βελτίωση του προϊόντος (ΓΚΠΔ Άρθ. 6(1)(στ)· εξαίρεση της CNIL για τη μέτρηση κοινού).
privacy-notice-lawful-pseudonymous = Νομική βάση: η ρητή συγκατάθεσή σας (ΓΚΠΔ Άρθ. 6(1)(α)).
privacy-notice-retention = Διατήρηση: μέγιστο διάστημα αποθήκευσης των δεδομένων στον διακομιστή, σε ημέρες: { $days }.
privacy-notice-withdrawal-right = Δικαίωμα ανάκλησης: μπορείτε ανά πάσα στιγμή να απενεργοποιήσετε οποιονδήποτε διακόπτη παρακάτω, να επιλέξετε «Ανάκληση συγκατάθεσης» για να σταματήσει κάθε συλλογή ή, στην ψευδωνυμοποιημένη λειτουργία, «Διαγραφή των δεδομένων μου» για να διαγραφούν οι εγγραφές από τον διακομιστή.
privacy-notice-policy-link = Πλήρης πολιτική απορρήτου: { $url }

privacy-scope-section-heading = Τι μπορεί να κοινοποιεί η εφαρμογή;
privacy-scope-anonymous-metrics-label = Ανώνυμα στατιστικά χρήσης
privacy-scope-anonymous-metrics-description = Πλήθος χρήσεων για κουμπιά / στοιχεία μενού / συντομεύσεις, καθώς και έκδοση εφαρμογής και λειτουργικό σύστημα.
privacy-scope-crash-reports-label = Αναφορές σφαλμάτων
privacy-scope-crash-reports-description = Ίχνη στοίβας και μεταδεδομένα της διεργασίας όταν η εφαρμογή καταρρέει. Κανένα περιεχόμενο εγγράφων, καμία διαδρομή αρχείων.
privacy-scope-feature-flags-label = Σημαίες λειτουργιών
privacy-scope-feature-flags-description = Επιτρέπει στην εφαρμογή να λαμβάνει ενημερώσεις σημαιών λειτουργιών (π.χ. σταδιακή διάθεση νέων εργαλείων).

privacy-btn-reject-all = Απόρριψη όλων
privacy-btn-accept-all = Αποδοχή όλων
privacy-btn-erase = Διαγραφή των δεδομένων μου
privacy-btn-erase-tooltip = Ζητά από τον διακομιστή να διαγράψει κάθε συμβάν που έχει καταγραφεί για αυτήν την εγκατάσταση και στη συνέχεια ανακαλεί τη συγκατάθεση τοπικά.
privacy-btn-fetch = Λήψη των δεδομένων μου
privacy-btn-fetch-tooltip = Ανακτά κάθε συμβάν που έχει καταγράψει ο διακομιστής με το αναγνωριστικό εγκατάστασής σας. Μπορείτε να αποθηκεύσετε το αποτέλεσμα σε μορφή JSON.
privacy-btn-withdraw = Ανάκληση συγκατάθεσης
privacy-btn-withdraw-tooltip = Σταματά τη συλλογή νέων δεδομένων. Τα δεδομένα που έχουν ήδη καταγραφεί στον διακομιστή διατηρούνται — χρησιμοποιήστε πρώτα το «Διαγραφή των δεδομένων μου» αν θέλετε να διαγραφούν.
privacy-btn-switch-to-anonymous = Μετάβαση σε ανώνυμη λειτουργία
privacy-btn-switch-to-pseudonymous = Μετάβαση σε ψευδωνυμοποιημένη λειτουργία

privacy-identity-heading = Τα δεδομένα σας στον διακομιστή
privacy-identity-install-id = Αναγνωριστικό εγκατάστασης: { $id }
privacy-identity-retention = Μέγιστο διάστημα αποθήκευσης των εγγραφών σας στον διακομιστή, σε ημέρες: { $days }.

privacy-mode-heading = Λειτουργία απορρήτου
privacy-mode-current-anonymous = Τρέχουσα: Ανώνυμη (χωρίς αναγνωριστικό εγκατάστασης)
privacy-mode-current-pseudonymous = Τρέχουσα: Ψευδωνυμοποιημένη (υπάρχει αναγνωριστικό εγκατάστασης)
privacy-mode-blurb-anonymous = Η ανώνυμη λειτουργία δεν μεταδίδει κανένα αναγνωριστικό ανά συσκευή. Η μετάβαση θα διαγράψει τις υπάρχουσες εγγραφές σας στον διακομιστή και θα απορρίψει το τοπικό UUID εγκατάστασης — η ενέργεια είναι μη αναστρέψιμη.
privacy-mode-blurb-pseudonymous = Η ψευδωνυμοποιημένη λειτουργία δημιουργεί ένα τυχαίο UUID εγκατάστασης. Θα μπορείτε να ανακτήσετε ή να διαγράψετε τις εγγραφές σας στον διακομιστή. Απαιτεί ρητή συγκατάθεση και ζητά εκ νέου τη συγκατάθεσή σας κατά τη μετάβαση.

privacy-confirm-mode-switch-title = Αλλαγή λειτουργίας απορρήτου;
privacy-confirm-mode-switch-leaving-pseudonymous = Αυτή η ενέργεια θα ζητήσει από τον διακομιστή να διαγράψει κάθε συμβάν που έχει καταγραφεί με το αναγνωριστικό εγκατάστασής σας, θα απορρίψει το τοπικό UUID εγκατάστασης, θα επαναφέρει την απόφασή σας για τη συγκατάθεση και θα αλλάξει τη λειτουργία απορρήτου. Θέλετε να συνεχίσετε;
privacy-confirm-mode-switch-leaving-anonymous = Αυτή η ενέργεια θα επαναφέρει την απόφασή σας για τη συγκατάθεση και θα αλλάξει τη λειτουργία απορρήτου. Θα ερωτηθείτε ξανά πριν από τη συλλογή οποιωνδήποτε νέων δεδομένων. Συνέχεια;
privacy-confirm-erase-title = Διαγραφή των δεδομένων σας;
privacy-confirm-erase-text = Αυτή η ενέργεια στέλνει αίτημα διαγραφής για κάθε συμβάν που έχει καταγραφεί με το αναγνωριστικό εγκατάστασής σας, απορρίπτει ό,τι παραμένει στην τοπική προσωρινή μνήμη και ανακαλεί τη συγκατάθεση, ώστε να μη συλλέγονται άλλα δεδομένα. Η ενέργεια δεν μπορεί να αναιρεθεί.
privacy-confirm-withdraw-title = Ανάκληση της συγκατάθεσης;
privacy-confirm-withdraw-text = Δεν θα συλλέγονται άλλα συμβάντα ανάλυσης από αυτήν την εφαρμογή. Τα δεδομένα που έχουν ήδη καταγραφεί στον διακομιστή διατηρούνται — χρησιμοποιήστε το «Διαγραφή των δεδομένων μου» πριν από την ανάκληση, αν θέλετε να διαγραφούν και αυτά.

privacy-fetch-success-title = Τα δεδομένα σας στον διακομιστή
privacy-fetch-success-text = Συμβάντα που ανακτήθηκαν για αυτήν την εγκατάσταση: { $count }.
privacy-fetch-saved-to = Αποθηκεύτηκε στο: { $path }
privacy-fetch-write-error = Δεν ήταν δυνατή η εγγραφή του αρχείου { $path }: { $error }
privacy-fetch-error-title = Δεν ήταν δυνατή η ανάκτηση των δεδομένων σας

privacy-inspect-title = Έλεγχος δεδομένων που στάλθηκαν (συμβάντα σε προσωρινή μνήμη: { $count })
privacy-inspect-empty = Δεν έχει εκπεμφθεί ακόμη κανένα συμβάν σε αυτήν τη συνεδρία. Δοκιμάστε να αλληλεπιδράσετε με την εφαρμογή — κλικ, μενού και συντομεύσεις περνούν όλα από εδώ.
privacy-inspect-summary = Εμφάνιση των τελευταίων συμβάντων (πλήθος: { $count }), με τα πιο πρόσφατα πρώτα.

# Ημερολόγιο / DateEdit / TimeEdit / DateTimeEdit. Βλ.
# crates/teksilo-widgets/src/{calendar,date_edit,time_edit,date_time_edit}.rs
# και τα κοινά αρθρώματα στο crates/teksilo-widgets/src/common/datetime/.
# Ονόματα μηνών και ημερών κατά CLDR (πλήρη / σύντομα / στενά).
calendar-month-long-january = Ιανουάριος
calendar-month-long-february = Φεβρουάριος
calendar-month-long-march = Μάρτιος
calendar-month-long-april = Απρίλιος
calendar-month-long-may = Μάιος
calendar-month-long-june = Ιούνιος
calendar-month-long-july = Ιούλιος
calendar-month-long-august = Αύγουστος
calendar-month-long-september = Σεπτέμβριος
calendar-month-long-october = Οκτώβριος
calendar-month-long-november = Νοέμβριος
calendar-month-long-december = Δεκέμβριος

calendar-month-short-january = Ιαν
calendar-month-short-february = Φεβ
calendar-month-short-march = Μάρ
calendar-month-short-april = Απρ
calendar-month-short-may = Μάι
calendar-month-short-june = Ιούν
calendar-month-short-july = Ιούλ
calendar-month-short-august = Αύγ
calendar-month-short-september = Σεπ
calendar-month-short-october = Οκτ
calendar-month-short-november = Νοέ
calendar-month-short-december = Δεκ

calendar-weekday-long-monday = Δευτέρα
calendar-weekday-long-tuesday = Τρίτη
calendar-weekday-long-wednesday = Τετάρτη
calendar-weekday-long-thursday = Πέμπτη
calendar-weekday-long-friday = Παρασκευή
calendar-weekday-long-saturday = Σάββατο
calendar-weekday-long-sunday = Κυριακή

calendar-weekday-short-monday = Δευ
calendar-weekday-short-tuesday = Τρί
calendar-weekday-short-wednesday = Τετ
calendar-weekday-short-thursday = Πέμ
calendar-weekday-short-friday = Παρ
calendar-weekday-short-saturday = Σάβ
calendar-weekday-short-sunday = Κυρ

calendar-weekday-narrow-monday = Δ
calendar-weekday-narrow-tuesday = Τ
calendar-weekday-narrow-wednesday = Τ
calendar-weekday-narrow-thursday = Π
calendar-weekday-narrow-friday = Π
calendar-weekday-narrow-saturday = Σ
calendar-weekday-narrow-sunday = Κ

calendar-button-previous-month = Προηγούμενος μήνας
calendar-button-next-month = Επόμενος μήνας
calendar-button-previous-year = Προηγούμενο έτος
calendar-button-next-year = Επόμενο έτος
calendar-button-today = Σήμερα
calendar-button-month-picker = Επιλογή μήνα
calendar-button-year-picker = Επιλογή έτους
calendar-week-number-column = Εβδ.
calendar-name = Ημερολόγιο
calendar-months-grid-label = Μήνες
calendar-years-grid-label = Έτη
calendar-name-with-month = Ημερολόγιο, { $month } { $year }
calendar-cell-name = { $weekday } { $day }, { $month } { $year }
calendar-range-status = Επιλογή: { $start } – { $end }

date-edit-segment-year = Έτος
date-edit-segment-month = Μήνας
date-edit-segment-day = Ημέρα
date-edit-calendar-button = Επιλογή ημερομηνίας
date-edit-trigger-tooltip = Άνοιγμα ημερολογίου
date-edit-name = Ημερομηνία
date-edit-placeholder = Επιλέξτε ημερομηνία

time-edit-segment-hour = Ώρα
time-edit-segment-minute = Λεπτό
time-edit-segment-second = Δευτερόλεπτο
time-edit-segment-period = π.μ./μ.μ.
time-edit-period-am = π.μ.
time-edit-period-pm = μ.μ.
time-edit-name = Ώρα
time-edit-placeholder = Επιλέξτε ώρα

date-time-edit-name = Ημερομηνία και ώρα
date-time-edit-placeholder = Επιλέξτε ημερομηνία και ώρα
date-time-edit-date-name = Ημερομηνία
date-time-edit-time-name = Ώρα
date-time-edit-trigger-tooltip = Άνοιγμα ημερολογίου
date-range-edit-name = Εύρος ημερομηνιών
date-range-edit-placeholder = Επιλέξτε εύρος ημερομηνιών
date-range-edit-start-name = Ημερομηνία έναρξης
date-range-edit-end-name = Ημερομηνία λήξης
date-range-edit-trigger-tooltip = Άνοιγμα ημερολογίου εύρους

# Σχόλια επικύρωσης (TextInputField + DateEdit/TimeEdit)
validation-corrected-to = Αυτόματη διόρθωση σε { $value }
validation-corrected-with-notes = Αυτόματη διόρθωση: { $notes }
validation-segment-clamped = { $segment } { $raw } → { $clamped }
validation-day-clamped-to-month = ημέρα { $raw } → { $clamped } (τελευταία ημέρα του μήνα)
validation-clamped-to-range = περιορίστηκε στο επιτρεπτό εύρος
validation-segment-year = έτος
validation-segment-month = μήνας
validation-segment-day = ημέρα
validation-segment-hour = ώρα
validation-segment-minute = λεπτό
validation-segment-second = δευτερόλεπτο
validation-segment-value = τιμή
date-edit-validation-not-a-date = Μη έγκυρη ημερομηνία
time-edit-validation-not-a-time = Μη έγκυρη ώρα

# ── επιλογέας χρώματος ──
color-picker-name = Επιλογέας χρώματος
color-picker-hue-label = Απόχρωση
color-picker-saturation-label = Κορεσμός
color-picker-value-label = Φωτεινότητα
color-picker-alpha-label = Αδιαφάνεια
color-picker-red-label = Κόκκινο
color-picker-green-label = Πράσινο
color-picker-blue-label = Μπλε
color-picker-red-short = R
color-picker-green-short = G
color-picker-blue-short = B
color-picker-alpha-short = A
color-picker-hue-short = H
color-picker-saturation-short = S
color-picker-value-short = V
color-picker-hex-label = Hex
color-picker-hex-placeholder = #RRGGBB
color-picker-current-color-label = Επιλεγμένο χρώμα
color-picker-current-color-readout = Επιλεγμένο χρώμα { $hex }
color-picker-swatches-name = Προκαθορισμένα χρώματα
color-picker-swatch-label = Δείγμα χρώματος { $hex }
color-picker-swatch-selected-suffix = , επιλεγμένο
color-picker-changed-announcement = Το χρώμα άλλαξε σε { $hex }
color-picker-done-label = Τέλος
color-picker-cancel-label = Άκυρο
color-edit-trigger-name = Χρώμα { $hex }
color-edit-trigger-name-empty = Χρώμα, κανένα
color-edit-trigger-tooltip = Άνοιγμα επιλογέα χρώματος
hex-color-input-invalid = Μη έγκυρος δεκαεξαδικός κωδικός χρώματος (αναμένεται #RRGGBB)
hex-color-input-invalid-with-alpha = Μη έγκυρος δεκαεξαδικός κωδικός χρώματος (αναμένεται #RRGGBB ή #RRGGBBAA)
hex-color-input-corrected-shortform = Το { $raw } αναπτύχθηκε σε { $value }
hex-color-input-corrected-uppercase = Κανονικοποιήθηκε σε { $value }
hex-color-input-placeholder = #RRGGBB
hex-color-input-placeholder-with-alpha = #RRGGBBAA
color-edit-trigger-empty-placeholder = —

# Ετικέτα «περισσότερα» για την ανάπτυξη των εμπλουτισμένων επεξηγήσεων
# (ο τίτλος του πτυσσόμενου τμήματος που αποκαλύπτει το εκτενές κείμενο).
tooltip-more = Περισσότερα

# Ενσωματωμένο μενού περιβάλλοντος πεδίων κειμένου και εμπλουτισμένου κειμένου.
menu-cut = Αποκοπή
menu-copy = Αντιγραφή
menu-paste = Επικόλληση
menu-paste-unformatted = Επικόλληση χωρίς μορφοποίηση
menu-select-all = Επιλογή όλων
menu-toggle-blockquote = Εναλλαγή παράθεσης
menu-remove-blockquote = Αφαίρεση παράθεσης

# DropZone — ανακοινώσεις ζωντανής περιοχής για αναγνώστες οθόνης. Ο
# ενικός και ο πληθυντικός επιλέγονται στη Rust και όχι με έκφραση
# select του Fluent. Βλ. en-US.ftl για το πλήρες πλαίσιο και
# crates/teksilo-widgets/src/drop_zone.rs.
drop-zone-hover-file-one = Αποθέστε για να προσθέσετε 1 αρχείο
drop-zone-hover-file-many = Αποθέστε για να προσθέσετε { $count } αρχεία
drop-zone-hover-text = Αποθέστε για να προσθέσετε κείμενο
drop-zone-hover-link-one = Αποθέστε για να προσθέσετε 1 σύνδεσμο
drop-zone-hover-link-many = Αποθέστε για να προσθέσετε { $count } συνδέσμους
drop-zone-hover-generic = Αποθέστε εδώ
drop-zone-hover-reject = Αυτό το στοιχείο δεν μπορεί να αποτεθεί εδώ
drop-zone-added-file-one = Προστέθηκε 1 αρχείο
drop-zone-added-file-many = Προστέθηκαν { $count } αρχεία
drop-zone-added-text = Προστέθηκε κείμενο
drop-zone-added-link-one = Προστέθηκε 1 σύνδεσμος
drop-zone-added-link-many = Προστέθηκαν { $count } σύνδεσμοι
drop-zone-rejected = Το στοιχείο δεν έγινε δεκτό

# Widget ThemeSwitcher. Βλ. crates/teksilo-widgets/src/theme_switcher.rs.
theme-switcher-label = Θέμα
theme-switcher-light = Ανοιχτό
theme-switcher-dark = Σκούρο
theme-switcher-system = Σύστημα

# Widget FontPicker. Βλ. crates/teksilo-widgets/src/font_picker.rs.
font-picker-label = Γραμματοσειρά
font-picker-placeholder = Επιλέξτε γραμματοσειρά…

# Ειδοποίηση αποτυχίας εγγραφής των ρυθμίσεων. Βλ. en-US.ftl για το πλήρες
# πλαίσιο (ενεργοποιείται από το ToastRegistry::show_settings_write_failed
# μέσω του teksilo::install_toast).
settings-write-failed-toast-title = Δεν ήταν δυνατή η αποθήκευση των ρυθμίσεων
settings-write-failed-toast-body = Η αποθήκευση του { $file } απέτυχε (προσπάθειες: { $attempts })· αλλαγές σε αναμονή που απορρίφθηκαν: { $dropped }. { $message }

# Εφεδρικό μενού παραθύρου, που ανοίγει με δεξί κλικ σε προσαρμοσμένη
# TitleBar όπου το λειτουργικό σύστημα δεν παρέχει δικό του (X11). Βλ.
# en-US.ftl για το πλήρες πλαίσιο και
# crates/teksilo-widgets/src/title_bar/window_menu.rs.
window-menu-restore = Επαναφορά
window-menu-maximize = Μεγιστοποίηση
window-menu-minimize = Ελαχιστοποίηση
window-menu-close = Κλείσιμο

# Ανάπτυξη του σώματος μιας ειδοποίησης. Βλ. en-US.ftl για το πλήρες
# πλαίσιο και crates/teksilo-widgets/src/toast/body.rs.
toast-show-more = Εμφάνιση περισσότερων
toast-show-less = Εμφάνιση λιγότερων
toast-copy-body = Αντιγραφή
toast-body-copied = Αντιγράφηκε

# Παλέτα εντολών. Βλ. en-US.ftl για το πλήρες πλαίσιο και
# crates/teksilo-widgets/src/command_palette.rs.
command-palette-placeholder = Πληκτρολογήστε μια εντολή
command-palette-empty = Καμία αντίστοιχη εντολή
command-palette-title = Παλέτα εντολών
command-palette-result-count =
    { $count ->
        [0] Καμία αντίστοιχη εντολή
        [one] 1 εντολή
       *[other] { $count } εντολές
    }
