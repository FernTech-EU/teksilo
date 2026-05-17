# Bastyde Widget Catalog — Arabic translations.
#
# المفاتيح بصيغة kebab-case في طبقة Fluent، وتُحوَّل إلى snake_case في
# Rust بواسطة الماكرو. ترتيب علامات التبويب ثابت — الفهرس N يقابل
# استدعاء `static_tab(...)` رقم N في main.rs.

# ── واجهة التطبيق ───────────────────────────────────────────────────────
app-title = Bastyde — كتالوج الودجات
app-subtitle = اسحب · انقر مزدوجًا للتكبير · انقر بزر الماوس الأيمن للقائمة
app-unsupported-chrome = (الإطار المخصص غير مدعوم على هذه المنصة — العودة إلى الزخارف الأصلية)

# ── مفتاح وضع العرض (الفتحة الخلفية لـ TabWidget) ────────────────────────
mode-label = ‏bati! DSL
mode-tooltip = بدّل كل علامة تبويب بين البنّاء التقليدي ونسخة الماكرو bati! للشجرة نفسها.

# ── مبدِّل اللغة ────────────────────────────────────────────────────────
locale-en = English
locale-fr = Français
locale-ar = العربية

# ── مبدِّل المظهر ───────────────────────────────────────────────────────
theme-label = المظهر
theme-tooltip = التبديل بين المظهرين الفاتح والداكن.

# ── عناوين علامات التبويب ───────────────────────────────────────────────
tab-palette-title = اللوحة
tab-layout-title = التخطيط
tab-visuals-title = العناصر المرئية
tab-containers-title = الحاويات
tab-chrome-title = الإطار
tab-buttons-title = الأزرار
tab-styling-title = التنسيق
tab-inputs-title = الإدخال
tab-indicators-title = المؤشرات
tab-text-title = النصوص
tab-datetime-title = التاريخ والوقت
tab-color-title = الألوان
tab-menus-title = القوائم
tab-overlays-title = الطبقات العلوية
tab-data-title = البيانات
tab-animations-title = الحركات
tab-settings-title = الإعدادات

# ── المراجع التفصيلية ───────────────────────────────────────────────────
tab-palette-refs = جميع أدوار الأسطح والنصوص والمحرّر، مع عبارة شاملة بالنص الغني وبالإيموجي لإظهار تأثير تبديل المظهر بصريًا. انظر: docs/reactive-theme.md.
tab-layout-refs = المكدِّسات والشبكات وتوزيع المساحة الفائضة. انظر: cargo run -p text_and_layout, cargo run -p split_view.
tab-visuals-refs = أوّليّات العرض والأيقونات والصور وأشرطة التحقق. انظر: cargo run -p simple_button.
tab-containers-refs = الألواح والبطاقات والأكورديون ومناطق التمرير والعروض المنقسمة. انظر: cargo run -p split_view, cargo run -p tool_box.
tab-chrome-refs = إطار التطبيق: شريط الأدوات وشريط الحالة وفُتات الخبز والمعالجات والشعارات. انظر: cargo run -p title_bar_demo.
tab-buttons-refs = جميع أنواع الأزرار. انظر: cargo run -p simple_button, cargo run -p menus_and_dropdowns.
tab-styling-refs = سلّم التنسيق رباعي المستويات: شبكات المتغيّرات (المستوى 1) وتجاوزات .style(impl FooStyle) لكل استدعاء (المستوى 3). انظر: docs/styling-system.md، cargo run -p theme_styles.
tab-inputs-refs = إدخالات منطقية وانتقائية. التنقل العميق بلوحة المفاتيح موجود في الأمثلة المخصصة.
tab-indicators-refs = حالة للقراءة فقط: التقدم والدوّامات والشارات والصور الرمزية والروابط.
tab-text-refs = تحرير النصوص أحادية السطر والغنية. انظر: cargo run -p rich_text_editor, cargo run -p spin_box.
tab-datetime-refs = منتقيات التقويم والتاريخ والوقت ونطاق التاريخ. انظر: cargo run -p datetime_pickers.
tab-color-refs = إدخال الست عشري وتحرير الألوان المضغوط ومنتقي HSV الكامل. انظر: cargo run -p color_picker.
tab-menus-refs = شريط القوائم وقائمة العناصر والقوائم السياقية. انظر: cargo run -p menus_and_dropdowns.
tab-overlays-refs = التلميحات والنوافذ المنبثقة والحوارات وأشرطة التنبيه. انظر: cargo run -p tooltips_showcase, cargo run -p dialogs_and_popovers, cargo run -p file_dialogs.
tab-data-refs = ListView و TreeView و TableView و TreeTable. انظر: cargo run -p data_grid, cargo run -p tree_table, cargo run -p data_collections.
tab-animations-refs = التلاشي والنبض والانزلاق والضبابية وما إلى ذلك. انظر: cargo run -p animations, cargo run -p animations_kit.
tab-settings-refs = ودجات إعادة ربط الاختصارات وإعدادات الخصوصية. انظر: cargo run -p shortcuts_demo.

# ── حشو لمحتوى علامة التبويب (للأقسام المؤقتة) ──────────────────────────
stub-heading = قريبًا
stub-body = ستُملأ علامة التبويب هذه خلال المرحلتين 3 و 4 من إعادة كتابة الكتالوج.

# ── تسميات تجريبية مشتركة قابلة لإعادة الاستخدام ────────────────────────
demo-save = حفظ
demo-cancel = إلغاء
demo-open = فتح
demo-new = جديد
demo-edit = تحرير
demo-quit = خروج
demo-undo = تراجع
demo-redo = إعادة
demo-cut = قص
demo-copy = نسخ
demo-paste = لصق
demo-find = بحث…
demo-confirm = تأكيد
demo-learn-more = المزيد من المعلومات
demo-next = التالي
demo-back = السابق
demo-finish = إنهاء
demo-loading = جاري التحميل…

# ── علامة تبويب المؤشرات ────────────────────────────────────────────────
ind-progress-determinate-label = ٦٠ ٪
ind-link-docs = افتح وثائق Bastyde
ind-link-handler = مع معالج نقر

# ── علامة تبويب الإدخال ─────────────────────────────────────────────────
inp-checkbox-two-state = خانة اختيار بحالتين
inp-checkbox-tristate = خانة اختيار بثلاث حالات
inp-checkbox-disabled = معطّل (لا يمكن التبديل)
inp-radio-a = الخيار أ
inp-radio-b = الخيار ب
inp-radio-c = الخيار ج
inp-toggle-feature = تفعيل الميزة
inp-toggle-with-label = مع تسمية
inp-toggle-disabled = مفتاح معطّل
inp-slider-volume = مستوى الصوت
inp-slider-stepped = الجودة (بخطوات ٢٥)
inp-slider-vertical = شريط تمرير عمودي
inp-segment-first = الأول
inp-segment-second = الثاني
inp-segment-third = الثالث
inp-combo-apple = تفاحة
inp-combo-banana = موزة
inp-combo-cherry = كرز
inp-combo-placeholder = اختر فاكهة

# ── علامة تبويب الأزرار ─────────────────────────────────────────────────
btn-default = افتراضي
btn-regular = عادي
btn-flat = مسطّح
btn-confirm-label = تأكيد
btn-cmdlink-signin-title = تسجيل الدخول إلى حساب Bastyde الخاص بك
btn-cmdlink-signin-desc = استخدم بيانات الاعتماد الموجودة للوصول إلى المشاريع.
btn-cmdlink-signup-title = إنشاء حساب جديد
btn-cmdlink-signup-desc = مجاني للاستخدام الشخصي ومفتوح المصدر.
btn-popover-trigger = افتح النافذة المنبثقة
btn-popover-title = محتوى النافذة المنبثقة
btn-popover-body = انقر خارجها للإغلاق.
btn-popover-icon-body = قائمة الإضافة السريعة

# ── علامة تبويب الحاويات ────────────────────────────────────────────────
cnt-panel-body = سطح اللوحة بخلفية وحدود مدفوعتين بالدور
cnt-card-header = رأس البطاقة
cnt-card-body = للبطاقة ارتفاع (ظل) ورأس اختياري ومحتوى وتذييل.
cnt-card-footer = التذييل · مظلَّل تلقائيًا
cnt-groupbox-title = الإشعارات
cnt-cb-sounds = تشغيل الأصوات
cnt-cb-banner = إظهار الشعار
cnt-groupheader-title = عنوان القسم
cnt-groupheader-body = …المحتوى تحت العنوان
cnt-accordion-1-title = إظهار التفاصيل
cnt-accordion-1-body = نص قسم الأكورديون الأول.
cnt-accordion-2-title = متقدّم
cnt-accordion-2-body = نص قسم الأكورديون الثاني.
cnt-toolbox-general = عام
cnt-toolbox-general-body = التفضيلات العامة
cnt-toolbox-editor = المحرّر
cnt-toolbox-editor-body = إعدادات المحرّر
cnt-toolbox-privacy = الخصوصية
cnt-toolbox-privacy-body = إعدادات الخصوصية والقياس عن بُعد
cnt-split-leading = اللوحة الأمامية
cnt-split-trailing = اللوحة الخلفية

# ── علامة تبويب الإطار ──────────────────────────────────────────────────
chr-status = جاهز · ١٢٤٧ سطرًا · UTF-8 · Rust
chr-banner-info-title = معلومة
chr-banner-info-body = هل علمت أن Bastyde يدعم الكتابة من اليمين إلى اليسار؟
chr-banner-success-title = نجاح
chr-banner-success-body = تم حفظ الإعدادات.
chr-banner-warning-title = تحذير
chr-banner-warning-body = القرص ممتلئ بنسبة ٩٠٪.
chr-banner-error-title = خطأ
chr-banner-error-body = انقطع الاتصال بالشبكة.
chr-breadcrumb-home = الرئيسية
chr-breadcrumb-docs = المستندات
chr-breadcrumb-bastyde = Bastyde
chr-breadcrumb-current = widget-catalog
chr-wizard-title = الإعداد الأوّلي
chr-wizard-step1 = مرحبًا
chr-wizard-step1-body = الخطوة ١ — مرحبًا بك في Bastyde
chr-wizard-step2 = إعداد
chr-wizard-step2-body = الخطوة ٢ — أعدّ المحرّر
chr-wizard-step3 = إنهاء
chr-wizard-step3-body = الخطوة ٣ — أنت جاهز
chr-wizard-trigger = افتح المعالج

# ── علامة تبويب العناصر المرئية ─────────────────────────────────────────
vis-text-body = نص المتن
vis-text-bold = متن عريض
vis-text-small = صغير ثانوي
vis-text-tiny = دقيق معطّل
vis-image-alt-1 = أيقونة نجمة (صورة نقطية)
vis-image-alt-2 = أيقونة نجمة بقناع دائري
vis-image-alt-3 = أيقونة نجمة بقناع مربّع مدوّر
vis-panel-body = اللوحة: خلفية + حدود + نصف قطر + حشو

# ── علامة تبويب التخطيط (أوصاف قصيرة لكل ودجة) ─────────────────────────
lay-overlay = طبقة فوقية
lay-padding-body = حشو ١٦ بكسل من جميع الجوانب
lay-fixed-size = ١٤٠ × ٤٠
lay-min-size = حدّ أدنى ١٦٠ × ٣٢
lay-max-size = مقيّد إلى ≤ ٢٤٠ × ٣٢ حتى مع نص طويل جدًا بداخله
lay-aspect-label = ١٦:٩
lay-centered = متمركز
lay-form-label-a = التسمية أ
lay-form-value-a = القيمة أ
lay-form-label-b = التسمية ب
lay-form-value-b = القيمة ب
lay-switcher-next = الصفحة التالية

# ── علامة تبويب النصوص ──────────────────────────────────────────────────
txt-username-label = اسم المستخدم
txt-username-placeholder = مثلًا ferris
txt-readonly-label = حقل للقراءة فقط
txt-search-placeholder = اكتب اسم فاكهة — Apple، Banana، …
txt-file-label = اختر ملفًا
txt-file-placeholder = لم يتم اختيار ملف
txt-input-dialog-trigger = افتح InputDialog
txt-input-dialog-title = إعادة تسمية الملف
txt-input-dialog-prompt = أدخل اسم الملف الجديد:
txt-input-dialog-placeholder = بدون-عنوان.txt

# ── علامة تبويب الألوان ─────────────────────────────────────────────────
clr-brand-label = لون العلامة التجارية
clr-accent-label = لون التمييز للسمة

# ── علامة تبويب اللوحة ──────────────────────────────────────────────────
pal-surfaces = الأسطح
pal-text = النصوص
pal-editor = المحرّر

# ── علامة تبويب القوائم ─────────────────────────────────────────────────
mnu-file = ملف
mnu-menu-edit = تحرير
mnu-standalone-a = عنصر مستقل أ
mnu-with-shortcut = مع اختصار
mnu-disabled = عنصر معطّل

# ── علامة تبويب الطبقات العلوية ─────────────────────────────────────────
ovr-tooltip-hover = حرّك المؤشر فوقي
ovr-tooltip-hover-body = نص تلميح بسيط
ovr-tooltip-longer = مع نص أطول
ovr-tooltip-longer-body = يمكن أن يلتفّ التلميح على عدة أسطر عند الحاجة.
ovr-popover-anchor = نقطة الإرساء
ovr-popover-title = محتوى النافذة المنبثقة
ovr-popover-body = انقر خارجها للإغلاق.
ovr-dialog-trigger = افتح الحوار
ovr-dialog-title = مثال على الحوار
ovr-dialog-body = هذا حوار (مُقدَّم عبر MessageBox::information).
ovr-mb-info = معلومات
ovr-mb-info-body = حوار معلوماتي.
ovr-mb-warning = تحذير
ovr-mb-warning-body = القرص يكاد يمتلئ.
ovr-mb-error = خطأ
ovr-mb-error-body = حدث خطأ ما.
ovr-mb-confirm = هل أنت متأكد؟
ovr-mb-confirm-body = لا يمكن التراجع عن هذا الإجراء.
ovr-snackbar-trigger = إظهار شريط التنبيه
ovr-snackbar-body = تم حفظ الملف بنجاح
ovr-shadow-body = سطح يشبه البطاقة بظل اللوحة الافتراضي

# ── علامة تبويب البيانات ────────────────────────────────────────────────
dat-fruit-apple = تفاحة
dat-fruit-banana = موزة
dat-fruit-cherry = كرز
dat-fruit-date = تمر
dat-list-row = صف
dat-list-item-1 = العنصر الأول
dat-list-item-2 = العنصر الثاني
dat-list-item-3 = العنصر الثالث
dat-tree-root = الجذر
dat-tree-child-a = الفرع أ
dat-tree-child-b = الفرع ب
dat-tree-grandchild = الحفيد
dat-tree-note = يتطلب TreeView نموذج TreeModel<T>. انظر `cargo run -p tree-table` للعرض الكامل.
dat-table-note = يتطلب TableView تعريفات أعمدة و ListModel. انظر `cargo run -p data-grid` لعرض شبكة ١٠٠٠ × ٦.
dat-treetable-note = يجمع TreeTable بين أعمدة TableView وتسلسل TreeView. انظر `cargo run -p tree-table` لعرض نظام ملفات وهمي.

# ── علامة تبويب الحركات ─────────────────────────────────────────────────
anim-visible = ظاهر
anim-expanded = موسَّع
anim-tip-1 = نصيحة ١ — اسحب الفاصل
anim-tip-2 = نصيحة ٢ — جرّب Ctrl+P
anim-tip-3 = نصيحة ٣ — يفتح F12 الفاحص
anim-crossfade-next = البديل التالي
anim-collapse-body = محتوى قابل للطيّ
anim-smooth-body = يتحرّك إلى الحجم الجوهري لطفله عند كل تغيير.
anim-shake = اهتزاز
anim-rotate = تدوير ٤٥+ درجة
anim-blur-toggle = تبديل الضبابية
anim-blur-body = محتوى حسّاس — بدّل الضبابية للكشف

# ── علامة تبويب الإعدادات ───────────────────────────────────────────────
set-privacy-note = خلف الميزة `telemetry` في cargo. شغّل `cargo run -p telemetry_plausible` أو ما يماثله لرؤية واجهة الموافقة.

# ── مولَّد تلقائيًا بواسطة /tmp/find_literals.py ─────────────
animations-tip-1-drag-the-divider = نصيحة ١ — اسحب الفاصل
animations-tip-2-try-ctrl-p = نصيحة ٢ — جرّب Ctrl+P
animations-tip-3-f12-opens-the-inspector = نصيحة ٣ — يفتح F12 الفاحص
animations-next-variant = البديل التالي
animations-collapsing-content = محتوى قابل للطيّ
animations-animates-to-its-child-s-intrin = يتحرّك إلى الحجم الجوهري لطفله عند كل تغيير.
animations-rotate-45 = تدوير ٤٥+ درجة
animations-toggle-blur = تبديل الضبابية
animations-sensitive-content-toggle-blur = محتوى حسّاس — بدّل الضبابية للكشف
buttons-save-as = حفظ باسم…
data-first-item = العنصر الأول
data-second-item = العنصر الثاني
data-third-item = العنصر الثالث
data-child-a = الفرع أ
data-child-b = الفرع ب
data-treeview-requires-a-treemodel = يتطلب TreeView نموذج TreeModel<T>. انظر `cargo run -p tree-table` للعرض الكامل.
layout-cross-platform = متعدد المنصات
layout-clamped-to-240-32-even-with-ve = مقيّد إلى ≤ ٢٤٠ × ٣٢ حتى مع نص طويل جدًا بداخله
layout-cross-platform-2 = متعدد المنصات
overlays-hover-me = حرّك المؤشر فوقي
overlays-plain-tooltip-text = نص تلميح بسيط
overlays-with-longer-text = مع نص أطول
overlays-tooltips-can-wrap-onto-multipl = يمكن أن يلتفّ التلميح على عدة أسطر عند الحاجة.
overlays-popover-content = محتوى النافذة المنبثقة
overlays-click-outside-to-dismiss = انقر خارجها للإغلاق.
overlays-open-dialog = افتح الحوار
overlays-dialog-example = مثال على الحوار
overlays-this-is-a-dialog-presented-via = هذا حوار (مُقدَّم عبر MessageBox::information).
overlays-informational-dialog = حوار معلوماتي.
overlays-disk-is-almost-full = القرص يكاد يمتلئ.
overlays-something-went-wrong = حدث خطأ ما.
overlays-are-you-sure = هل أنت متأكد؟
overlays-this-action-cannot-be-undone = لا يمكن التراجع عن هذا الإجراء.
overlays-file-saved-successfully = تم حفظ الملف بنجاح
overlays-file-saved-successfully-2 = تم حفظ الملف بنجاح
overlays-show-snackbar = إظهار شريط التنبيه
overlays-card-like-surface-with-the-def = سطح يشبه البطاقة بظل اللوحة الافتراضي
