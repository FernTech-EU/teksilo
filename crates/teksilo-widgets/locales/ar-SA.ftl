# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

# teksilo-widgets framework strings — Arabic (العربية) translation.
#
# ترجمة عربية لسلاسل إطار العمل teksilo-widgets.
#
# تُستعمل في وقت التشغيل فقط: التطبيقات التي تسجّل هذه اللغة عبر
#   `I18nConfig::framework_locales(teksilo_widgets::framework_locales())`
# تحصل على هذه الترجمات إلى جانب en-US. وأي مفتاح غير موجود هنا يعود
# تلقائيًا إلى النص المصدر في en-US عبر سلسلة الاحتياط اليدوية في
# `I18nManager::resolve_widget` (تجاوز التطبيق النشط ← إطار العمل النشط ←
# مصدر تجاوز التطبيق ← مصدر إطار العمل ← اسم المفتاح). وهذه سلسلة احتياط
# خاصة بـ teksilo-i18n وليست الاحتياط المدمج في `fluent-bundle`.

a11y-status-bar-name = الحالة
a11y-dialog-name = مربع حوار
a11y-tooltip-name = تلميح
a11y-snackbar-name = إشعار
a11y-splitter-divider-name = فاصل التقسيم
a11y-splitter-pane = جزء
a11y-splitter-collapsed = مطوي
a11y-splitter-expanded = موسّع
a11y-breadcrumb-current-page-value = الصفحة الحالية
a11y-toolbar-name = شريط الأدوات
toolbar-more = المزيد
segmented-control-more = مزيد من الخيارات
breadcrumb-overflow = إظهار المسار المخفي
a11y-title-bar-name = شريط عنوان النافذة
a11y-window-controls-name = عناصر التحكم بالنافذة
a11y-window-minimize-name = تصغير
a11y-window-maximize-name = تكبير
a11y-window-restore-name = استعادة
a11y-window-close-name = إغلاق
a11y-stepper-indicator-strip-name = الخطوات
a11y-stepper-content-name = محتوى الخطوة
tab-close-tooltip = إغلاق علامة التبويب
a11y-builtin-browse = استعراض
a11y-builtin-expand = توسيع
a11y-builtin-search = بحث
a11y-builtin-copy = نسخ
a11y-builtin-clear = مسح
a11y-builtin-add = إضافة
a11y-builtin-bell = الإشعارات
a11y-builtin-menu = قائمة
a11y-builtin-more = مزيد من الإجراءات
a11y-builtin-visibility = تبديل الرؤية
a11y-password-reveal = إظهار كلمة المرور أو إخفاؤها
a11y-caps-lock-on = مفتاح Caps Lock مفعَّل
notifications-title = الإشعارات
notifications-empty = لا توجد إشعارات
notifications-mark-all-read = تحديد الكل كمقروء
notifications-clear = مسح الكل
notifications-filter-placeholder = البحث في الإشعارات
notifications-bucket-today = اليوم
notifications-bucket-yesterday = أمس
notifications-bucket-this-week = هذا الأسبوع
notifications-bucket-earlier = أقدم
notifications-archive-replay-disabled = (لم يعد متاحًا)
a11y-shortcut-settings-name = إعدادات الاختصارات
a11y-shortcut-settings-capture-hint = اضغط أي مفتاح. Delete للمسح. Esc للإلغاء.
keystroke-modifier-ctrl = Ctrl
keystroke-modifier-shift = Shift
keystroke-modifier-alt = Alt
keystroke-modifier-super = Super
keystroke-separator = +
keystroke-key-space = مسافة
keystroke-key-enter = Enter
keystroke-key-escape = Esc
keystroke-key-tab = Tab
keystroke-key-backspace = Backspace
keystroke-key-delete = Del
keystroke-key-arrow-up = أعلى
keystroke-key-arrow-down = أسفل
keystroke-key-arrow-left = يسار
keystroke-key-arrow-right = يمين
keystroke-key-home = Home
keystroke-key-end = End
keystroke-key-page-up = PageUp
keystroke-key-page-down = PageDown

# MessageBox — الأزرار القياسية وإظهار التفاصيل. انظر
# crates/teksilo-widgets/src/message_box.rs.
messagebox-btn-ok = موافق
messagebox-btn-cancel = إلغاء
messagebox-btn-close = إغلاق
messagebox-btn-yes = نعم
messagebox-btn-no = لا
messagebox-btn-yes-to-all = نعم للكل
messagebox-btn-no-to-all = لا للكل
messagebox-btn-save = حفظ
messagebox-btn-save-all = حفظ الكل
messagebox-btn-discard = تجاهل التغييرات
messagebox-btn-apply = تطبيق
messagebox-btn-reset = إعادة تعيين
messagebox-btn-restore-defaults = استعادة الإعدادات الافتراضية
messagebox-btn-abort = إحباط
messagebox-btn-retry = إعادة المحاولة
messagebox-btn-ignore = تجاهل
messagebox-btn-open = فتح
messagebox-btn-help = مساعدة
messagebox-show-details = إظهار التفاصيل

# الودجة PrivacySettings. انظر crates/teksilo-widgets/src/privacy_settings.rs.
# إفصاح المادة 13 من اللائحة العامة لحماية البيانات + أزرار الإجراءات.
# المفاتيح ذات المعاملات تستعمل صيغة Fluent { $name }.
privacy-not-configured = القياس عن بُعد غير مُهيَّأ لهذا التطبيق.
privacy-a11y-group-name = إعدادات الخصوصية والقياس عن بُعد
privacy-heading = الخصوصية والقياس عن بُعد
privacy-notice-controller = تُعالَج البيانات بواسطة { $processor }؛ والمعالج التقني هو { $adapter } (نقطة النهاية: { $endpoint }).
privacy-notice-purposes = الأغراض: تحسين التطبيق — أي الميزات تُستعمل، وأين تتركّز الأخطاء البرمجية، وعلى أي أنظمة تشغيل نعمل. لا يُجمع أي محتوى للمستندات، ولا الحافظة، ولا ضغطات المفاتيح، ولا لقطات الشاشة.
privacy-notice-lawful-anonymous = الأساس القانوني: مصلحتنا المشروعة في تحسين المنتج (اللائحة العامة لحماية البيانات، المادة 6(1)(f)؛ واستثناء قياس الجمهور الصادر عن الهيئة الفرنسية CNIL).
privacy-notice-lawful-pseudonymous = الأساس القانوني: موافقتك الصريحة (اللائحة العامة لحماية البيانات، المادة 6(1)(a)).
privacy-notice-retention = مدة الاحتفاظ: الحد الأقصى لبقاء البيانات على الخادم بالأيام ({ $days }).
privacy-notice-withdrawal-right = حق السحب: يمكنك في أي وقت إيقاف أي مفتاح تبديل أدناه، أو النقر على «سحب الموافقة» لإيقاف كل عمليات الجمع، أو النقر في الوضع المستعار على «محو بياناتي» لحذف السجلات من الخادم.
privacy-notice-policy-link = سياسة الخصوصية الكاملة: { $url }

privacy-scope-section-heading = ما الذي يمكن للتطبيق مشاركته؟
privacy-scope-anonymous-metrics-label = مقاييس استعمال مجهولة الهوية
privacy-scope-anonymous-metrics-description = عدد مرات استعمال الأزرار وعناصر القوائم والاختصارات، إضافةً إلى إصدار التطبيق ونظام التشغيل.
privacy-scope-crash-reports-label = تقارير الأعطال
privacy-scope-crash-reports-description = تتبُّع المكدّس وبيانات العملية الوصفية عند تعطّل التطبيق. لا يُرسَل أي محتوى للمستندات ولا أي مسارات ملفات.
privacy-scope-feature-flags-label = أعلام الميزات
privacy-scope-feature-flags-description = تتيح للتطبيق تلقّي تحديثات أعلام الميزات (مثل الطرح التدريجي لأدوات جديدة).

privacy-btn-reject-all = رفض الكل
privacy-btn-accept-all = قبول الكل
privacy-btn-erase = محو بياناتي
privacy-btn-erase-tooltip = يطلب من الخادم حذف كل حدث مُسجَّل لعملية التثبيت هذه، ثم يسحب الموافقة محليًا.
privacy-btn-fetch = جلب بياناتي
privacy-btn-fetch-tooltip = يجلب كل حدث سجّله الخادم تحت معرّف التثبيت الخاص بك. ويمكنك حفظ النتيجة بتنسيق JSON.
privacy-btn-withdraw = سحب الموافقة
privacy-btn-withdraw-tooltip = يوقف جمع أي بيانات جديدة. أما البيانات المسجَّلة على الخادم فتبقى محفوظة — استعمل «محو بياناتي» أولًا إذا أردت حذفها.
privacy-btn-switch-to-anonymous = التبديل إلى الوضع المجهول
privacy-btn-switch-to-pseudonymous = التبديل إلى الوضع المستعار

privacy-identity-heading = بياناتك على الخادم
privacy-identity-install-id = معرّف التثبيت: { $id }
privacy-identity-retention = الحد الأقصى لمدة احتفاظ الخادم بسجلاتك بالأيام ({ $days }).

privacy-mode-heading = وضع الخصوصية
privacy-mode-current-anonymous = الوضع الحالي: مجهول (بلا معرّف تثبيت)
privacy-mode-current-pseudonymous = الوضع الحالي: مستعار (يوجد معرّف تثبيت)
privacy-mode-blurb-anonymous = لا يرسل الوضع المجهول أي معرّف خاص بالجهاز. وسيؤدي التبديل إليه إلى محو سجلاتك الموجودة على الخادم وحذف معرّف التثبيت المحلي (UUID) — ولا يمكن التراجع عن ذلك.
privacy-mode-blurb-pseudonymous = ينشئ الوضع المستعار معرّف تثبيت عشوائيًا (UUID). وسيصبح بإمكانك جلب سجلاتك على الخادم أو محوها. ويتطلب هذا الوضع موافقة صريحة، ويعيد طلبها عند التبديل.

privacy-confirm-mode-switch-title = تغيير وضع الخصوصية؟
privacy-confirm-mode-switch-leaving-pseudonymous = سيؤدي هذا الإجراء إلى مطالبة الخادم بمحو كل حدث مُسجَّل تحت معرّف التثبيت الخاص بك، وحذف معرّف التثبيت المحلي (UUID)، وإعادة تعيين قرار الموافقة، وتغيير وضع الخصوصية. هل تريد المتابعة؟
privacy-confirm-mode-switch-leaving-anonymous = سيؤدي هذا الإجراء إلى إعادة تعيين قرار الموافقة وتغيير وضع الخصوصية. وستُسأل من جديد قبل جمع أي بيانات جديدة. هل تريد المتابعة؟
privacy-confirm-erase-title = محو بياناتك؟
privacy-confirm-erase-text = يرسل هذا الإجراء طلب حذف لكل حدث مُسجَّل تحت معرّف التثبيت الخاص بك، ويتخلّص من كل ما لا يزال في الذاكرة المؤقتة محليًا، ويسحب الموافقة حتى لا تُجمع أي بيانات أخرى. ولا يمكن التراجع عن هذا الإجراء.
privacy-confirm-withdraw-title = سحب الموافقة؟
privacy-confirm-withdraw-text = لن تُجمع أي أحداث تحليلية أخرى من هذا التطبيق. أما البيانات المسجَّلة على الخادم فتبقى محفوظة — استعمل «محو بياناتي» قبل سحب الموافقة إذا أردت حذفها أيضًا.

privacy-fetch-success-title = بياناتك على الخادم
privacy-fetch-success-text = تم جلب الأحداث الخاصة بعملية التثبيت هذه: { $count }.
privacy-fetch-saved-to = حُفظ في: { $path }
privacy-fetch-write-error = تعذّرت كتابة الملف { $path }: { $error }
privacy-fetch-error-title = تعذّر جلب بياناتك

privacy-inspect-title = فحص البيانات المُرسَلة (الأحداث في الذاكرة المؤقتة: { $count })
privacy-inspect-empty = لم يُرسَل أي حدث في هذه الجلسة بعد. جرّب التفاعل مع التطبيق — فالنقرات والقوائم والاختصارات تمرّ جميعها من هنا.
privacy-inspect-summary = عرض آخر الأحداث ({ $count })، من الأحدث إلى الأقدم.

# التقويم / DateEdit / TimeEdit / DateTimeEdit. انظر
# crates/teksilo-widgets/src/{calendar,date_edit,time_edit,date_time_edit}.rs
# والوحدات المشتركة تحت crates/teksilo-widgets/src/common/datetime/.
# أسماء الأشهر والأيام مطابقة لبيانات CLDR للعربية؛ الصيغة المختصرة في
# العربية مطابقة للصيغة الطويلة وفق CLDR.
calendar-month-long-january = يناير
calendar-month-long-february = فبراير
calendar-month-long-march = مارس
calendar-month-long-april = أبريل
calendar-month-long-may = مايو
calendar-month-long-june = يونيو
calendar-month-long-july = يوليو
calendar-month-long-august = أغسطس
calendar-month-long-september = سبتمبر
calendar-month-long-october = أكتوبر
calendar-month-long-november = نوفمبر
calendar-month-long-december = ديسمبر

calendar-month-short-january = يناير
calendar-month-short-february = فبراير
calendar-month-short-march = مارس
calendar-month-short-april = أبريل
calendar-month-short-may = مايو
calendar-month-short-june = يونيو
calendar-month-short-july = يوليو
calendar-month-short-august = أغسطس
calendar-month-short-september = سبتمبر
calendar-month-short-october = أكتوبر
calendar-month-short-november = نوفمبر
calendar-month-short-december = ديسمبر

calendar-weekday-long-monday = الاثنين
calendar-weekday-long-tuesday = الثلاثاء
calendar-weekday-long-wednesday = الأربعاء
calendar-weekday-long-thursday = الخميس
calendar-weekday-long-friday = الجمعة
calendar-weekday-long-saturday = السبت
calendar-weekday-long-sunday = الأحد

calendar-weekday-short-monday = الاثنين
calendar-weekday-short-tuesday = الثلاثاء
calendar-weekday-short-wednesday = الأربعاء
calendar-weekday-short-thursday = الخميس
calendar-weekday-short-friday = الجمعة
calendar-weekday-short-saturday = السبت
calendar-weekday-short-sunday = الأحد

calendar-weekday-narrow-monday = ن
calendar-weekday-narrow-tuesday = ث
calendar-weekday-narrow-wednesday = ر
calendar-weekday-narrow-thursday = خ
calendar-weekday-narrow-friday = ج
calendar-weekday-narrow-saturday = س
calendar-weekday-narrow-sunday = ح

calendar-button-previous-month = الشهر السابق
calendar-button-next-month = الشهر التالي
calendar-button-previous-year = السنة السابقة
calendar-button-next-year = السنة التالية
calendar-button-today = اليوم
calendar-button-month-picker = اختيار الشهر
calendar-button-year-picker = اختيار السنة
calendar-week-number-column = أسبوع
calendar-name = التقويم
calendar-months-grid-label = الأشهر
calendar-years-grid-label = السنوات
calendar-name-with-month = التقويم، { $month } { $year }
calendar-cell-name = { $weekday }، { $day } { $month } { $year }
calendar-range-status = المحدَّد: { $start } – { $end }

date-edit-segment-year = السنة
date-edit-segment-month = الشهر
date-edit-segment-day = اليوم
date-edit-calendar-button = اختيار التاريخ
date-edit-trigger-tooltip = فتح التقويم
date-edit-name = التاريخ
date-edit-placeholder = حدّد تاريخًا

time-edit-segment-hour = الساعة
time-edit-segment-minute = الدقيقة
time-edit-segment-second = الثانية
time-edit-segment-period = ص/م
time-edit-period-am = ص
time-edit-period-pm = م
time-edit-name = الوقت
time-edit-placeholder = حدّد وقتًا

date-time-edit-name = التاريخ والوقت
date-time-edit-placeholder = حدّد التاريخ والوقت
date-time-edit-date-name = التاريخ
date-time-edit-time-name = الوقت
date-time-edit-trigger-tooltip = فتح التقويم
date-range-edit-name = نطاق التواريخ
date-range-edit-placeholder = حدّد نطاق التواريخ
date-range-edit-start-name = تاريخ البداية
date-range-edit-end-name = تاريخ النهاية
date-range-edit-trigger-tooltip = فتح تقويم النطاق

# ملاحظات التحقق من الإدخال (TextInputField + DateEdit/TimeEdit)
validation-corrected-to = تم التصحيح تلقائيًا إلى { $value }
validation-corrected-with-notes = تم التصحيح تلقائيًا: { $notes }
validation-segment-clamped = { $segment } { $raw } → { $clamped }
validation-day-clamped-to-month = اليوم { $raw } → { $clamped } (آخر يوم في الشهر)
validation-clamped-to-range = تم حصره ضمن النطاق المسموح به
validation-segment-year = السنة
validation-segment-month = الشهر
validation-segment-day = اليوم
validation-segment-hour = الساعة
validation-segment-minute = الدقيقة
validation-segment-second = الثانية
validation-segment-value = القيمة
date-edit-validation-not-a-date = تاريخ غير صالح
time-edit-validation-not-a-time = وقت غير صالح

# ── منتقي الألوان ──
color-picker-name = منتقي الألوان
color-picker-hue-label = تدرج اللون
color-picker-saturation-label = التشبع
color-picker-value-label = السطوع
color-picker-alpha-label = العتامة
color-picker-red-label = أحمر
color-picker-green-label = أخضر
color-picker-blue-label = أزرق
color-picker-red-short = R
color-picker-green-short = G
color-picker-blue-short = B
color-picker-alpha-short = A
color-picker-hue-short = H
color-picker-saturation-short = S
color-picker-value-short = V
color-picker-hex-label = سداسي عشري
color-picker-hex-placeholder = #RRGGBB
color-picker-current-color-label = اللون المحدَّد
color-picker-current-color-readout = اللون المحدَّد { $hex }
color-picker-swatches-name = ألوان معدّة مسبقًا
color-picker-swatch-label = عينة لون { $hex }
color-picker-swatch-selected-suffix = ، محدَّدة
color-picker-changed-announcement = تم تغيير اللون إلى { $hex }
color-picker-done-label = تم
color-picker-cancel-label = إلغاء
color-edit-trigger-name = اللون { $hex }
color-edit-trigger-name-empty = اللون، بلا تحديد
color-edit-trigger-tooltip = فتح منتقي الألوان
hex-color-input-invalid = رمز لون سداسي عشري غير صالح (المتوقع #RRGGBB)
hex-color-input-invalid-with-alpha = رمز لون سداسي عشري غير صالح (المتوقع #RRGGBB أو #RRGGBBAA)
hex-color-input-corrected-shortform = تم توسيع { $raw } إلى { $value }
hex-color-input-corrected-uppercase = تم التوحيد إلى { $value }
hex-color-input-placeholder = #RRGGBB
hex-color-input-placeholder-with-alpha = #RRGGBBAA
color-edit-trigger-empty-placeholder = —

# تسمية «المزيد» في التلميحات الغنية (عنوان الأكورديون الذي يكشف النص
# المطوّل داخل تلميح غني مثبّت).
tooltip-more = المزيد

# عناصر القائمة السياقية المدمجة لحقول النص والمحرر الغني.
menu-cut = قص
menu-copy = نسخ
menu-paste = لصق
menu-paste-unformatted = لصق بدون تنسيق
menu-select-all = تحديد الكل
menu-toggle-blockquote = تبديل الاقتباس
menu-remove-blockquote = إزالة الاقتباس

# DropZone — إعلانات المنطقة الحيّة لقارئات الشاشة. يُختار المفرد أو الجمع
# في كود Rust وليس عبر تعبير select في Fluent، لذا صيغت هذه السلاسل بحيث
# تصح مع كل الأعداد (العدد بين قوسين بعد الاسم). انظر en-US.ftl للسياق
# الكامل و crates/teksilo-widgets/src/drop_zone.rs.
drop-zone-hover-file-one = أفلِت لإضافة ملف واحد
drop-zone-hover-file-many = أفلِت لإضافة ملفات ({ $count })
drop-zone-hover-text = أفلِت لإضافة نص
drop-zone-hover-link-one = أفلِت لإضافة رابط واحد
drop-zone-hover-link-many = أفلِت لإضافة روابط ({ $count })
drop-zone-hover-generic = أفلِت هنا
drop-zone-hover-reject = لا يمكن إفلات هذا العنصر هنا
drop-zone-added-file-one = تمت إضافة ملف واحد
drop-zone-added-file-many = تمت إضافة ملفات ({ $count })
drop-zone-added-text = تمت إضافة نص
drop-zone-added-link-one = تمت إضافة رابط واحد
drop-zone-added-link-many = تمت إضافة روابط ({ $count })
drop-zone-rejected = العنصر غير مقبول

# الودجة ThemeSwitcher. انظر crates/teksilo-widgets/src/theme_switcher.rs.
theme-switcher-label = السمة
theme-switcher-light = فاتحة
theme-switcher-dark = داكنة
theme-switcher-system = النظام

# الودجة FontPicker. انظر crates/teksilo-widgets/src/font_picker.rs.
font-picker-label = الخط
font-picker-placeholder = اختر خطًا…

# إشعار فشل كتابة الإعدادات. انظر en-US.ftl للسياق الكامل (يُطلقه
# ToastRegistry::show_settings_write_failed عبر teksilo::install_toast).
settings-write-failed-toast-title = تعذّر حفظ الإعدادات
settings-write-failed-toast-body = تعذّر حفظ { $file } بعد { $attempts } من المحاولات؛ وتم تجاهل التغييرات المُعلَّقة ({ $dropped }). { $message }

# قائمة النافذة الاحتياطية، تُفتح بالنقر بالزر الأيمن على شريط عنوان مخصص
# على الأنظمة التي لا توفّر قائمة نافذة (X11). انظر en-US.ftl للسياق الكامل
# و crates/teksilo-widgets/src/title_bar/window_menu.rs.
window-menu-restore = استعادة
window-menu-maximize = تكبير
window-menu-minimize = تصغير
window-menu-close = إغلاق

# كشف نص الإشعار المنبثق. انظر en-US.ftl للسياق الكامل و
# crates/teksilo-widgets/src/toast/body.rs.
toast-show-more = عرض المزيد
toast-show-less = عرض أقل
toast-copy-body = نسخ
toast-body-copied = تم النسخ

# لوحة الأوامر. انظر en-US.ftl للسياق الكامل و
# crates/teksilo-widgets/src/command_palette.rs.
command-palette-placeholder = اكتب أمرًا
command-palette-empty = لا يوجد أمر مطابق
# الاسم المتاح لقارئات الشاشة لمربع حوار اللوحة ولحقل البحث فيها. لا يظهر
# على الشاشة إطلاقًا، فهو الشيء الوحيد الذي يخبر مستخدم قارئ الشاشة بما فُتح.
command-palette-title = لوحة الأوامر
# يُعلَن كوصف لمربع الحوار ويُعاد إعلانه كلما ضاق نطاق البحث. أذرع الجمع
# تتبع فئات CLDR العربية الست (zero/one/two/few/many/other)؛ الذراع الحرفي
# [0] يسبق فئة zero ويعطي صياغة «لا نتائج».
command-palette-result-count =
    { $count ->
        [0] لا يوجد أمر مطابق
        [zero] لا يوجد أمر مطابق
        [one] أمر واحد
        [two] أمران
        [few] { $count } أوامر
        [many] { $count } أمرًا
       *[other] { $count } أمر
    }
