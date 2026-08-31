# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

# מחרוזות המסגרת teksilo-widgets — תרגום לעברית.
#
# בזמן ריצה בלבד: יישומים שרושמים את הלוקאל הזה באמצעות
# `I18nConfig::framework_locales(teksilo_widgets::framework_locales())`
# מקבלים את התרגומים האלה לצד en-US. מפתחות שחסרים ב-he-IL נופלים
# חזרה למקור ב-en-US דרך שרשרת הנפילה הידנית של
# `I18nManager::resolve_widget` (עקיפת היישום הפעילה → המסגרת הפעילה →
# מקור עקיפת היישום → מקור המסגרת → שם המפתח). זו שרשרת הנפילה של
# teksilo-i18n עצמה, ולא זו המובנית של `fluent-bundle` לכל מפתח — כל
# `FluentBundle` נבנה עם לוקאל יחיד בשרשרת שלו, והחיפוש הרב-לשוני
# מתבצע בשכבת `I18nManager`.

a11y-status-bar-name = מצב
a11y-dialog-name = תיבת דו-שיח
a11y-tooltip-name = עצת מסך
a11y-snackbar-name = התראה
a11y-splitter-divider-name = מפריד חלוניות
a11y-splitter-pane = חלונית
a11y-splitter-collapsed = מכווץ
a11y-splitter-expanded = מורחב
a11y-breadcrumb-current-page-value = העמוד הנוכחי
a11y-toolbar-name = סרגל כלים
toolbar-more = עוד
segmented-control-more = אפשרויות נוספות
breadcrumb-overflow = הצגת הנתיב המוסתר
a11y-title-bar-name = שורת הכותרת של החלון
a11y-window-controls-name = פקדי החלון
a11y-window-minimize-name = מזעור
a11y-window-maximize-name = הגדלה
a11y-window-restore-name = שחזור
a11y-window-close-name = סגירה
a11y-stepper-indicator-strip-name = שלבים
a11y-stepper-content-name = תוכן השלב
tab-close-tooltip = סגירת הלשונית
a11y-builtin-browse = עיון
a11y-builtin-expand = הגדלה
a11y-builtin-search = חיפוש
a11y-builtin-copy = העתקה
a11y-builtin-clear = ניקוי
a11y-builtin-add = הוספה
a11y-builtin-bell = התראות
a11y-builtin-menu = תפריט
a11y-builtin-more = פעולות נוספות
a11y-builtin-visibility = הצגה או הסתרה
a11y-password-reveal = הצגה או הסתרה של הסיסמה
a11y-caps-lock-on = מקש Caps Lock פעיל
notifications-title = התראות
notifications-empty = אין התראות
notifications-mark-all-read = סימון הכול כנקרא
notifications-clear = ניקוי הכול
notifications-filter-placeholder = חיפוש בהתראות
notifications-bucket-today = היום
notifications-bucket-yesterday = אתמול
notifications-bucket-this-week = השבוע
notifications-bucket-earlier = מוקדם יותר
notifications-archive-replay-disabled = (לא זמין עוד)
a11y-shortcut-settings-name = הגדרות מקשי קיצור
a11y-shortcut-settings-capture-hint = יש להקיש על מקש כלשהו. Delete לניקוי. Esc לביטול.
keystroke-modifier-ctrl = Ctrl
keystroke-modifier-shift = Shift
keystroke-modifier-alt = Alt
keystroke-modifier-super = Super
keystroke-separator = +
keystroke-key-space = רווח
keystroke-key-enter = Enter
keystroke-key-escape = Esc
keystroke-key-tab = Tab
keystroke-key-backspace = Backspace
keystroke-key-delete = Del
keystroke-key-arrow-up = למעלה
keystroke-key-arrow-down = למטה
keystroke-key-arrow-left = שמאלה
keystroke-key-arrow-right = ימינה
keystroke-key-home = Home
keystroke-key-end = End
keystroke-key-page-up = PgUp
keystroke-key-page-down = PgDn

# MessageBox — לחצנים סטנדרטיים וגילוי הפרטים.
messagebox-btn-ok = אישור
messagebox-btn-cancel = ביטול
messagebox-btn-close = סגירה
messagebox-btn-yes = כן
messagebox-btn-no = לא
messagebox-btn-yes-to-all = כן להכול
messagebox-btn-no-to-all = לא להכול
messagebox-btn-save = שמירה
messagebox-btn-save-all = שמירת הכול
messagebox-btn-discard = השלכה
messagebox-btn-apply = החלה
messagebox-btn-reset = איפוס
messagebox-btn-restore-defaults = שחזור ברירות המחדל
messagebox-btn-abort = הפסקה
messagebox-btn-retry = ניסיון חוזר
messagebox-btn-ignore = התעלמות
messagebox-btn-open = פתיחה
messagebox-btn-help = עזרה
messagebox-show-details = הצגת פרטים

# הווידג'ט PrivacySettings. ראו crates/teksilo-widgets/src/privacy_settings.rs.
# הודעת יידוע לפי GDPR סעיף 13 בתוספת לחצני פעולה. מפתחות עם פרמטרים
# משתמשים בתחביר Fluent { $name }.
privacy-not-configured = טלמטריה אינה מוגדרת ביישום הזה.
privacy-a11y-group-name = הגדרות פרטיות וטלמטריה
privacy-heading = פרטיות וטלמטריה
privacy-notice-controller = המידע מעובד על ידי { $processor }; מעבד המידע הטכני הוא { $adapter } (נקודת קצה: { $endpoint }).
privacy-notice-purposes = מטרות: שיפור היישום — אילו יכולות נמצאות בשימוש, היכן מתרכזות התקלות ועל אילו פלטפורמות היישום פועל. ללא תוכן מסמכים, ללא לוח הגזירים, ללא הקשות מקלדת וללא צילומי מסך.
privacy-notice-lawful-anonymous = בסיס חוקי: האינטרס הלגיטימי שלנו בשיפור המוצר (GDPR סעיף 6(1)(f); פטור מדידת הקהל של CNIL).
privacy-notice-lawful-pseudonymous = בסיס חוקי: הסכמתך המפורשת (GDPR סעיף 6(1)(a)).
privacy-notice-retention = שמירת המידע: תקופת השמירה המרבית של המידע בצד השרת (בימים): { $days }.
privacy-notice-withdrawal-right = זכות החזרה מההסכמה: אפשר לכבות כל מתג שלהלן בכל עת, ללחוץ על "ביטול ההסכמה" כדי להפסיק כל איסוף, או במצב פסאודונימי ללחוץ על "מחיקת המידע שלי" כדי למחוק את הרשומות מהשרת.
privacy-notice-policy-link = מדיניות הפרטיות המלאה: { $url }

privacy-scope-section-heading = מה היישום רשאי לשתף?
privacy-scope-anonymous-metrics-label = מדדי שימוש אנונימיים
privacy-scope-anonymous-metrics-description = ספירה של הלחצנים / פריטי התפריט / מקשי הקיצור שנעשה בהם שימוש, וכן גרסת היישום ומערכת ההפעלה.
privacy-scope-crash-reports-label = דוחות קריסה
privacy-scope-crash-reports-description = עקבות מחסנית ומטא-נתונים של התהליך בעת קריסת היישום. ללא תוכן מסמכים וללא נתיבי קבצים.
privacy-scope-feature-flags-label = דגלי יכולות
privacy-scope-feature-flags-description = מאפשר ליישום לקבל עדכוני דגלי יכולות (למשל הפצה הדרגתית של כלים חדשים).

privacy-btn-reject-all = דחיית הכול
privacy-btn-accept-all = אישור הכול
privacy-btn-erase = מחיקת המידע שלי
privacy-btn-erase-tooltip = מבקש מהשרת למחוק כל אירוע שנרשם עבור התקנה זו, ולאחר מכן מבטל את ההסכמה באופן מקומי.
privacy-btn-fetch = קבלת המידע שלי
privacy-btn-fetch-tooltip = מאחזר כל אירוע שהשרת רשם תחת מזהה ההתקנה שלך. אפשר לשמור את התוצאה כקובץ JSON.
privacy-btn-withdraw = ביטול ההסכמה
privacy-btn-withdraw-tooltip = מפסיק כל איסוף מידע חדש. המידע שכבר נרשם בשרת נשמר — יש להשתמש תחילה ב"מחיקת המידע שלי" כדי למחוק אותו.
privacy-btn-switch-to-anonymous = מעבר למצב אנונימי
privacy-btn-switch-to-pseudonymous = מעבר למצב פסאודונימי

privacy-identity-heading = המידע שלך בשרת
privacy-identity-install-id = מזהה התקנה: { $id }
privacy-identity-retention = תקופת השמירה המרבית של הרשומות שלך בשרת (בימים): { $days }.

privacy-mode-heading = מצב פרטיות
privacy-mode-current-anonymous = כעת: אנונימי (ללא מזהה התקנה)
privacy-mode-current-pseudonymous = כעת: פסאודונימי (קיים מזהה התקנה)
privacy-mode-blurb-anonymous = במצב אנונימי לא נשלח שום מזהה ייחודי למכשיר. המעבר ימחק את הרשומות הקיימות שלך בשרת ויסיר את מזהה ההתקנה המקומי (UUID) — פעולה זו אינה הפיכה.
privacy-mode-blurb-pseudonymous = במצב פסאודונימי נוצר מזהה התקנה אקראי (UUID). יהיה אפשר לאחזר או למחוק את הרשומות שלך בשרת. נדרשת הסכמה מפורשת, והבקשה מוצגת שוב בעת המעבר.

privacy-confirm-mode-switch-title = להחליף את מצב הפרטיות?
privacy-confirm-mode-switch-leaving-pseudonymous = פעולה זו תבקש מהשרת למחוק כל אירוע שנרשם תחת מזהה ההתקנה שלך, תסיר את מזהה ההתקנה המקומי (UUID), תאפס את החלטת ההסכמה שלך ותחליף את מצב הפרטיות. להמשיך?
privacy-confirm-mode-switch-leaving-anonymous = פעולה זו תאפס את החלטת ההסכמה שלך ותחליף את מצב הפרטיות. תוצג בקשה חוזרת לפני איסוף מידע חדש. להמשיך?
privacy-confirm-erase-title = למחוק את המידע שלך?
privacy-confirm-erase-text = פעולה זו שולחת בקשת מחיקה לכל אירוע שנרשם תחת מזהה ההתקנה שלך, מוחקת כל מה שעדיין ממתין במאגר המקומי, ומבטלת את ההסכמה כך שלא ייאסף מידע נוסף. הפעולה אינה הפיכה.
privacy-confirm-withdraw-title = לבטל את ההסכמה?
privacy-confirm-withdraw-text = לא ייאספו עוד אירועי ניתוח מהיישום הזה. המידע שכבר נרשם בשרת נשמר — יש להשתמש ב"מחיקת המידע שלי" לפני ביטול ההסכמה אם ברצונך למחוק גם אותו.

privacy-fetch-success-title = המידע שלך בשרת
privacy-fetch-success-text = אירועים שאוחזרו עבור התקנה זו: { $count }.
privacy-fetch-saved-to = נשמר אל: { $path }
privacy-fetch-write-error = לא ניתן לכתוב את הקובץ { $path }: { $error }
privacy-fetch-error-title = לא ניתן לאחזר את המידע שלך

privacy-inspect-title = בדיקת המידע הנשלח (אירועים במאגר: { $count })
privacy-inspect-empty = טרם נשלחו אירועים בהפעלה הנוכחית. כדאי להתנסות ביישום — לחיצות, תפריטים ומקשי קיצור עוברים כולם דרך כאן.
privacy-inspect-summary = מוצגים האירועים האחרונים, מהחדש לישן. מספר האירועים: { $count }.

# לוח שנה / DateEdit / TimeEdit / DateTimeEdit. ראו
# crates/teksilo-widgets/src/{calendar,date_edit,time_edit,date_time_edit}.rs
# ואת המודולים המשותפים תחת crates/teksilo-widgets/src/common/datetime/.
# שמות החודשים והימים לפי CLDR עבור he.
calendar-month-long-january = ינואר
calendar-month-long-february = פברואר
calendar-month-long-march = מרץ
calendar-month-long-april = אפריל
calendar-month-long-may = מאי
calendar-month-long-june = יוני
calendar-month-long-july = יולי
calendar-month-long-august = אוגוסט
calendar-month-long-september = ספטמבר
calendar-month-long-october = אוקטובר
calendar-month-long-november = נובמבר
calendar-month-long-december = דצמבר

calendar-month-short-january = ינו׳
calendar-month-short-february = פבר׳
calendar-month-short-march = מרץ
calendar-month-short-april = אפר׳
calendar-month-short-may = מאי
calendar-month-short-june = יוני
calendar-month-short-july = יולי
calendar-month-short-august = אוג׳
calendar-month-short-september = ספט׳
calendar-month-short-october = אוק׳
calendar-month-short-november = נוב׳
calendar-month-short-december = דצמ׳

calendar-weekday-long-monday = יום שני
calendar-weekday-long-tuesday = יום שלישי
calendar-weekday-long-wednesday = יום רביעי
calendar-weekday-long-thursday = יום חמישי
calendar-weekday-long-friday = יום שישי
calendar-weekday-long-saturday = יום שבת
calendar-weekday-long-sunday = יום ראשון

calendar-weekday-short-monday = יום ב׳
calendar-weekday-short-tuesday = יום ג׳
calendar-weekday-short-wednesday = יום ד׳
calendar-weekday-short-thursday = יום ה׳
calendar-weekday-short-friday = יום ו׳
calendar-weekday-short-saturday = שבת
calendar-weekday-short-sunday = יום א׳

calendar-weekday-narrow-monday = ב׳
calendar-weekday-narrow-tuesday = ג׳
calendar-weekday-narrow-wednesday = ד׳
calendar-weekday-narrow-thursday = ה׳
calendar-weekday-narrow-friday = ו׳
calendar-weekday-narrow-saturday = ש׳
calendar-weekday-narrow-sunday = א׳

calendar-button-previous-month = החודש הקודם
calendar-button-next-month = החודש הבא
calendar-button-previous-year = השנה הקודמת
calendar-button-next-year = השנה הבאה
calendar-button-today = היום
calendar-button-month-picker = בחירת חודש
calendar-button-year-picker = בחירת שנה
calendar-week-number-column = שבוע
calendar-name = לוח שנה
calendar-months-grid-label = חודשים
calendar-years-grid-label = שנים
calendar-name-with-month = לוח שנה, { $month } { $year }
calendar-cell-name = { $weekday }, { $day } ב{ $month } { $year }
calendar-range-status = נבחר: { $start } – { $end }

date-edit-segment-year = שנה
date-edit-segment-month = חודש
date-edit-segment-day = יום
date-edit-calendar-button = בחירת תאריך
date-edit-trigger-tooltip = פתיחת לוח השנה
date-edit-name = תאריך
date-edit-placeholder = יש לבחור תאריך

time-edit-segment-hour = שעה
time-edit-segment-minute = דקה
time-edit-segment-second = שנייה
time-edit-segment-period = AM/PM
time-edit-period-am = AM
time-edit-period-pm = PM
time-edit-name = שעה
time-edit-placeholder = יש לבחור שעה

date-time-edit-name = תאריך ושעה
date-time-edit-placeholder = יש לבחור תאריך ושעה
date-time-edit-date-name = תאריך
date-time-edit-time-name = שעה
date-time-edit-trigger-tooltip = פתיחת לוח השנה
date-range-edit-name = טווח תאריכים
date-range-edit-placeholder = יש לבחור טווח תאריכים
date-range-edit-start-name = תאריך התחלה
date-range-edit-end-name = תאריך סיום
date-range-edit-trigger-tooltip = פתיחת לוח השנה לבחירת טווח

# משוב אימות (TextInputField + DateEdit/TimeEdit)
validation-corrected-to = תוקן אוטומטית ל-{ $value }
validation-corrected-with-notes = תוקן אוטומטית: { $notes }
validation-segment-clamped = { $segment } { $raw } ← { $clamped }
validation-day-clamped-to-month = יום { $raw } ← { $clamped } (היום האחרון בחודש)
validation-clamped-to-range = הוגבל לטווח המותר
validation-segment-year = שנה
validation-segment-month = חודש
validation-segment-day = יום
validation-segment-hour = שעה
validation-segment-minute = דקה
validation-segment-second = שנייה
validation-segment-value = ערך
date-edit-validation-not-a-date = תאריך לא תקין
time-edit-validation-not-a-time = שעה לא תקינה

# ── בורר הצבעים ──
color-picker-name = בורר צבעים
color-picker-hue-label = גוון
color-picker-saturation-label = רוויה
color-picker-value-label = בהירות
color-picker-alpha-label = אטימות
color-picker-red-label = אדום
color-picker-green-label = ירוק
color-picker-blue-label = כחול
color-picker-red-short = R
color-picker-green-short = G
color-picker-blue-short = B
color-picker-alpha-short = A
color-picker-hue-short = H
color-picker-saturation-short = S
color-picker-value-short = V
color-picker-hex-label = Hex
color-picker-hex-placeholder = #RRGGBB
color-picker-current-color-label = הצבע הנבחר
color-picker-current-color-readout = הצבע הנבחר { $hex }
color-picker-swatches-name = צבעים מוגדרים מראש
color-picker-swatch-label = דוגמית { $hex }
color-picker-swatch-selected-suffix = , נבחרה
color-picker-changed-announcement = הצבע שונה ל-{ $hex }
color-picker-done-label = סיום
color-picker-cancel-label = ביטול
color-edit-trigger-name = צבע { $hex }
color-edit-trigger-name-empty = צבע, ללא
color-edit-trigger-tooltip = פתיחת בורר הצבעים
hex-color-input-invalid = קוד צבע הקסדצימלי לא תקין (נדרש #RRGGBB)
hex-color-input-invalid-with-alpha = קוד צבע הקסדצימלי לא תקין (נדרש #RRGGBB או #RRGGBBAA)
hex-color-input-corrected-shortform = { $raw } הורחב ל-{ $value }
hex-color-input-corrected-uppercase = תוקן ל-{ $value }
hex-color-input-placeholder = #RRGGBB
hex-color-input-placeholder-with-alpha = #RRGGBBAA
color-edit-trigger-empty-placeholder = —

# תווית ה"עוד" של עצת המסך המורחבת (כותרת האקורדיון שחושפת את הגוף
# המלא בתוך עצת מסך מוצמדת).
tooltip-more = עוד

# פריטי תפריט ההקשר המובנים של שדות טקסט ושל העורך המעוצב.
menu-cut = גזירה
menu-copy = העתקה
menu-paste = הדבקה
menu-paste-unformatted = הדבקה ללא עיצוב
menu-select-all = בחירת הכול
menu-toggle-blockquote = סימון כציטוט
menu-remove-blockquote = הסרת הציטוט

# DropZone — הכרזות אזור "live" (קוראי מסך). היחיד מול הרבים נבחר בקוד
# Rust ולא בביטוי select של Fluent. ראו en-US.ftl להקשר המלא ואת
# crates/teksilo-widgets/src/drop_zone.rs.
drop-zone-hover-file-one = יש לשחרר כדי להוסיף קובץ אחד
drop-zone-hover-file-many = יש לשחרר כדי להוסיף { $count } קבצים
drop-zone-hover-text = יש לשחרר כדי להוסיף טקסט
drop-zone-hover-link-one = יש לשחרר כדי להוסיף קישור אחד
drop-zone-hover-link-many = יש לשחרר כדי להוסיף { $count } קישורים
drop-zone-hover-generic = יש לשחרר כאן
drop-zone-hover-reject = לא ניתן לשחרר כאן את הפריט הזה
drop-zone-added-file-one = נוסף קובץ אחד
drop-zone-added-file-many = נוספו { $count } קבצים
drop-zone-added-text = נוסף טקסט
drop-zone-added-link-one = נוסף קישור אחד
drop-zone-added-link-many = נוספו { $count } קישורים
drop-zone-rejected = הפריט לא התקבל

# הווידג'ט ThemeSwitcher. ראו crates/teksilo-widgets/src/theme_switcher.rs.
theme-switcher-label = ערכת נושא
theme-switcher-light = בהיר
theme-switcher-dark = כהה
theme-switcher-system = מערכת

# הווידג'ט FontPicker. ראו crates/teksilo-widgets/src/font_picker.rs.
font-picker-label = גופן
font-picker-placeholder = יש לבחור גופן…

# התראת כישלון בכתיבת ההגדרות. ראו en-US.ftl להקשר המלא (נשלחת על ידי
# ToastRegistry::show_settings_write_failed דרך teksilo::install_toast).
settings-write-failed-toast-title = לא ניתן לשמור את ההגדרות
settings-write-failed-toast-body = שמירת { $file } נכשלה. מספר הניסיונות: { $attempts }. שינויים בהמתנה שנזנחו: { $dropped }. { $message }

# תפריט חלון חלופי, הנפתח בלחיצה ימנית על TitleBar מותאמת אישית במערכות
# שאין בהן תפריט חלון של מערכת ההפעלה (X11). ראו en-US.ftl להקשר המלא
# ואת crates/teksilo-widgets/src/title_bar/window_menu.rs.
window-menu-restore = שחזור
window-menu-maximize = הגדלה
window-menu-minimize = מזעור
window-menu-close = סגירה

# גילוי גוף ההתראה הצפה. ראו en-US.ftl להקשר המלא ואת
# crates/teksilo-widgets/src/toast/body.rs.
toast-show-more = הצגת עוד
toast-show-less = הצגת פחות
toast-copy-body = העתקה
toast-body-copied = הועתק

# פלטת הפקודות. ראו en-US.ftl להקשר המלא ואת
# crates/teksilo-widgets/src/command_palette.rs.
command-palette-placeholder = יש להקליד פקודה
command-palette-empty = אין פקודה מתאימה
command-palette-title = לוח הפקודות
command-palette-result-count =
    { $count ->
        [0] אין פקודה מתאימה
        [one] פקודה אחת
        [two] שתי פקודות
        [many] { $count } פקודות
       *[other] { $count } פקודות
    }
