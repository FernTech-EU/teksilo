# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

# Рядки фреймворку teksilo-widgets — український переклад.
#
# Лише під час виконання: застосунки, які реєструють цю локаль через
# `I18nConfig::framework_locales(teksilo_widgets::framework_locales())`,
# отримують ці переклади поряд з en-US. Ключі, відсутні в uk-UA,
# повертаються до джерела en-US через ланцюжок ручного резервування
# `I18nManager::resolve_widget` (активне перевизначення застосунку →
# активний фреймворк → джерело перевизначення застосунку → джерело
# фреймворку → заповнювач ключа). Це власне резервування teksilo-i18n,
# а не вбудоване поключове резервування `fluent-bundle`: кожен
# `FluentBundle` створюється з однією локаллю в ланцюжку, а
# багатолокальний пошук виконується на рівні `I18nManager`.

a11y-status-bar-name = Стан
a11y-dialog-name = Діалогове вікно
a11y-tooltip-name = Підказка
a11y-snackbar-name = Сповіщення
a11y-splitter-divider-name = Роздільник
a11y-splitter-pane = Панель
a11y-splitter-collapsed = Згорнуто
a11y-splitter-expanded = Розгорнуто
a11y-breadcrumb-current-page-value = поточна сторінка
a11y-toolbar-name = Панель інструментів
toolbar-more = Більше
segmented-control-more = Більше параметрів
breadcrumb-overflow = Показати прихований шлях
a11y-title-bar-name = Рядок заголовка вікна
a11y-window-controls-name = Елементи керування вікном
a11y-window-minimize-name = Згорнути
a11y-window-maximize-name = Розгорнути
a11y-window-restore-name = Відновити
a11y-window-close-name = Закрити
a11y-stepper-indicator-strip-name = Кроки
a11y-stepper-content-name = Вміст кроку
tab-close-tooltip = Закрити вкладку
a11y-builtin-browse = Огляд
a11y-builtin-expand = Розгорнути
a11y-builtin-search = Пошук
a11y-builtin-copy = Копіювати
a11y-builtin-clear = Очистити
a11y-builtin-add = Додати
a11y-builtin-bell = Сповіщення
a11y-builtin-menu = Меню
a11y-builtin-more = Більше дій
a11y-builtin-visibility = Показати або приховати
a11y-password-reveal = Показати або приховати пароль
a11y-caps-lock-on = Увімкнено Caps Lock
notifications-title = Сповіщення
notifications-empty = Немає сповіщень
notifications-mark-all-read = Позначити всі як прочитані
notifications-clear = Очистити все
notifications-filter-placeholder = Пошук сповіщень
notifications-bucket-today = Сьогодні
notifications-bucket-yesterday = Учора
notifications-bucket-this-week = Цього тижня
notifications-bucket-earlier = Раніше
notifications-archive-replay-disabled = (більше недоступно)
a11y-shortcut-settings-name = Параметри сполучень клавіш
a11y-shortcut-settings-capture-hint = Натисніть будь-яку клавішу. Delete — очистити. Esc — скасувати.
keystroke-modifier-ctrl = Ctrl
keystroke-modifier-shift = Shift
keystroke-modifier-alt = Alt
keystroke-modifier-super = Super
keystroke-separator = +
keystroke-key-space = Пробіл
keystroke-key-enter = Enter
keystroke-key-escape = Esc
keystroke-key-tab = Tab
keystroke-key-backspace = Backspace
keystroke-key-delete = Del
keystroke-key-arrow-up = Вгору
keystroke-key-arrow-down = Вниз
keystroke-key-arrow-left = Ліворуч
keystroke-key-arrow-right = Праворуч
keystroke-key-home = Home
keystroke-key-end = End
keystroke-key-page-up = PgUp
keystroke-key-page-down = PgDn

# MessageBox — стандартні кнопки та розкриття подробиць.
messagebox-btn-ok = OK
messagebox-btn-cancel = Скасувати
messagebox-btn-close = Закрити
messagebox-btn-yes = Так
messagebox-btn-no = Ні
messagebox-btn-yes-to-all = Так для всіх
messagebox-btn-no-to-all = Ні для всіх
messagebox-btn-save = Зберегти
messagebox-btn-save-all = Зберегти все
messagebox-btn-discard = Відкинути
messagebox-btn-apply = Застосувати
messagebox-btn-reset = Скинути
messagebox-btn-restore-defaults = Відновити типові значення
messagebox-btn-abort = Перервати
messagebox-btn-retry = Повторити
messagebox-btn-ignore = Ігнорувати
messagebox-btn-open = Відкрити
messagebox-btn-help = Довідка
messagebox-show-details = Показати подробиці

# Віджет PrivacySettings. Див. crates/teksilo-widgets/src/privacy_settings.rs.
# Інформування за ст. 13 GDPR + кнопки дій. Ключі з параметрами
# використовують синтаксис Fluent { $назва }.
privacy-not-configured = Телеметрію не налаштовано для цього застосунку.
privacy-a11y-group-name = Параметри конфіденційності та телеметрії
privacy-heading = Конфіденційність і телеметрія
privacy-notice-controller = Дані опрацьовує { $processor }; технічний оператор — { $adapter } (кінцева точка: { $endpoint }).
privacy-notice-purposes = Цілі: удосконалення застосунку — які функції використовуються, де найчастіше виникають помилки, на яких платформах він працює. Жодного вмісту документів, жодного буфера обміну, жодних натискань клавіш, жодних знімків екрана.
privacy-notice-lawful-anonymous = Правова підстава: наш законний інтерес в удосконаленні продукту (GDPR, ст. 6(1)(f); виняток CNIL щодо вимірювання аудиторії).
privacy-notice-lawful-pseudonymous = Правова підстава: ваша явна згода (GDPR, ст. 6(1)(a)).
privacy-notice-retention = Зберігання: дані на боці сервера зберігаються щонайбільше { $days } дн.
privacy-notice-withdrawal-right = Право на відкликання: ви можете будь-коли вимкнути будь-який перемикач нижче, натиснути «Відкликати згоду», щоб припинити весь збір, або в псевдонімному режимі натиснути «Стерти мої дані», щоб видалити записи із сервера.
privacy-notice-policy-link = Повна політика конфіденційності: { $url }

privacy-scope-section-heading = Чим може ділитися застосунок?
privacy-scope-anonymous-metrics-label = Анонімні показники використання
privacy-scope-anonymous-metrics-description = Підрахунок використаних кнопок / пунктів меню / сполучень клавіш, а також версія застосунку та операційна система.
privacy-scope-crash-reports-label = Звіти про збої
privacy-scope-crash-reports-description = Трасування стека та метадані процесу під час збою застосунку. Жодного вмісту документів, жодних шляхів до файлів.
privacy-scope-feature-flags-label = Прапорці функцій
privacy-scope-feature-flags-description = Дає застосунку змогу отримувати оновлення прапорців функцій (наприклад, поступове впровадження нових інструментів).

privacy-btn-reject-all = Відхилити все
privacy-btn-accept-all = Прийняти все
privacy-btn-erase = Стерти мої дані
privacy-btn-erase-tooltip = Надсилає серверу запит на видалення всіх подій, записаних для цього встановлення, після чого локально відкликає згоду.
privacy-btn-fetch = Отримати мої дані
privacy-btn-fetch-tooltip = Отримує всі події, які сервер записав під вашим ідентифікатором встановлення. Результат можна зберегти у форматі JSON.
privacy-btn-withdraw = Відкликати згоду
privacy-btn-withdraw-tooltip = Припиняє збір нових даних. Уже записані дані на сервері зберігаються — спершу скористайтеся кнопкою «Стерти мої дані», якщо хочете їх видалити.
privacy-btn-switch-to-anonymous = Перейти в анонімний режим
privacy-btn-switch-to-pseudonymous = Перейти в псевдонімний режим

privacy-identity-heading = Ваші дані на сервері
privacy-identity-install-id = Ідентифікатор встановлення: { $id }
privacy-identity-retention = Сервер зберігає ваші записи щонайбільше { $days } дн.

privacy-mode-heading = Режим конфіденційності
privacy-mode-current-anonymous = Поточний: анонімний (без ідентифікатора встановлення)
privacy-mode-current-pseudonymous = Поточний: псевдонімний (ідентифікатор встановлення наявний)
privacy-mode-blurb-anonymous = Анонімний режим не передає жодного ідентифікатора пристрою. Перехід зітре ваші наявні записи на сервері та відкине локальний UUID встановлення — цю дію не можна скасувати.
privacy-mode-blurb-pseudonymous = Псевдонімний режим створює випадковий UUID встановлення. Ви зможете отримати або стерти свої записи на сервері. Потребує явної згоди й повторно запитує її під час переходу.

privacy-confirm-mode-switch-title = Змінити режим конфіденційності?
privacy-confirm-mode-switch-leaving-pseudonymous = Ця дія попросить сервер стерти всі події, записані під вашим ідентифікатором встановлення, відкине локальний UUID встановлення, скине ваше рішення щодо згоди та змінить режим конфіденційності. Продовжити?
privacy-confirm-mode-switch-leaving-anonymous = Ця дія скине ваше рішення щодо згоди та змінить режим конфіденційності. Перш ніж збиратимуться нові дані, вас запитають знову. Продовжити?
privacy-confirm-erase-title = Стерти ваші дані?
privacy-confirm-erase-text = Ця дія надсилає запит на видалення кожної події, записаної під вашим ідентифікатором встановлення, відкидає все, що ще зберігається в локальному буфері, і відкликає згоду, щоб дані більше не збиралися. Цю дію не можна скасувати.
privacy-confirm-withdraw-title = Відкликати згоду?
privacy-confirm-withdraw-text = Із цього застосунку більше не збиратимуться аналітичні події. Уже записані дані на сервері зберігаються — скористайтеся кнопкою «Стерти мої дані» перед відкликанням, якщо хочете видалити і їх.

privacy-fetch-success-title = Ваші дані на сервері
privacy-fetch-success-text = Отримано подій для цього встановлення: { $count }.
privacy-fetch-saved-to = Збережено у файл: { $path }
privacy-fetch-write-error = Не вдалося записати файл { $path }: { $error }
privacy-fetch-error-title = Не вдалося отримати ваші дані

privacy-inspect-title = Перегляд надісланих даних (подій у буфері: { $count })
privacy-inspect-empty = У цьому сеансі ще не надіслано жодної події. Спробуйте попрацювати із застосунком — клацання, меню та сполучення клавіш проходять саме тут.
privacy-inspect-summary = Показано останні події (кількість: { $count }), спочатку найновіші.

# Календар / DateEdit / TimeEdit / DateTimeEdit. Див.
# crates/teksilo-widgets/src/{calendar,date_edit,time_edit,date_time_edit}.rs
# та спільні модулі в crates/teksilo-widgets/src/common/datetime/.
# Назви місяців і днів тижня подано в самостійній (називній) формі за CLDR.
calendar-month-long-january = січень
calendar-month-long-february = лютий
calendar-month-long-march = березень
calendar-month-long-april = квітень
calendar-month-long-may = травень
calendar-month-long-june = червень
calendar-month-long-july = липень
calendar-month-long-august = серпень
calendar-month-long-september = вересень
calendar-month-long-october = жовтень
calendar-month-long-november = листопад
calendar-month-long-december = грудень

calendar-month-short-january = січ.
calendar-month-short-february = лют.
calendar-month-short-march = бер.
calendar-month-short-april = квіт.
calendar-month-short-may = трав.
calendar-month-short-june = черв.
calendar-month-short-july = лип.
calendar-month-short-august = серп.
calendar-month-short-september = вер.
calendar-month-short-october = жовт.
calendar-month-short-november = лист.
calendar-month-short-december = груд.

calendar-weekday-long-monday = понеділок
calendar-weekday-long-tuesday = вівторок
calendar-weekday-long-wednesday = середа
calendar-weekday-long-thursday = четвер
calendar-weekday-long-friday = пʼятниця
calendar-weekday-long-saturday = субота
calendar-weekday-long-sunday = неділя

calendar-weekday-short-monday = пн
calendar-weekday-short-tuesday = вт
calendar-weekday-short-wednesday = ср
calendar-weekday-short-thursday = чт
calendar-weekday-short-friday = пт
calendar-weekday-short-saturday = сб
calendar-weekday-short-sunday = нд

calendar-weekday-narrow-monday = П
calendar-weekday-narrow-tuesday = В
calendar-weekday-narrow-wednesday = С
calendar-weekday-narrow-thursday = Ч
calendar-weekday-narrow-friday = П
calendar-weekday-narrow-saturday = С
calendar-weekday-narrow-sunday = Н

calendar-button-previous-month = Попередній місяць
calendar-button-next-month = Наступний місяць
calendar-button-previous-year = Попередній рік
calendar-button-next-year = Наступний рік
calendar-button-today = Сьогодні
calendar-button-month-picker = Вибрати місяць
calendar-button-year-picker = Вибрати рік
calendar-week-number-column = Тиж.
calendar-name = Календар
calendar-months-grid-label = Місяці
calendar-years-grid-label = Роки
calendar-name-with-month = Календар, { $month } { $year }
calendar-cell-name = { $weekday }, { $day }, { $month } { $year }
calendar-range-status = Вибрано: { $start } – { $end }

date-edit-segment-year = Рік
date-edit-segment-month = Місяць
date-edit-segment-day = День
date-edit-calendar-button = Вибрати дату
date-edit-trigger-tooltip = Відкрити календар
date-edit-name = Дата
date-edit-placeholder = Виберіть дату

time-edit-segment-hour = Година
time-edit-segment-minute = Хвилина
time-edit-segment-second = Секунда
time-edit-segment-period = дп/пп
time-edit-period-am = дп
time-edit-period-pm = пп
time-edit-name = Час
time-edit-placeholder = Виберіть час

date-time-edit-name = Дата й час
date-time-edit-placeholder = Виберіть дату й час
date-time-edit-date-name = Дата
date-time-edit-time-name = Час
date-time-edit-trigger-tooltip = Відкрити календар
date-range-edit-name = Діапазон дат
date-range-edit-placeholder = Виберіть діапазон дат
date-range-edit-start-name = Початкова дата
date-range-edit-end-name = Кінцева дата
date-range-edit-trigger-tooltip = Відкрити календар діапазону

# Повідомлення перевірки (TextInputField + DateEdit/TimeEdit)
validation-corrected-to = Автоматично виправлено на { $value }
validation-corrected-with-notes = Автоматично виправлено: { $notes }
validation-segment-clamped = { $segment } { $raw } → { $clamped }
validation-day-clamped-to-month = день { $raw } → { $clamped } (останній день місяця)
validation-clamped-to-range = обмежено дозволеним діапазоном
validation-segment-year = рік
validation-segment-month = місяць
validation-segment-day = день
validation-segment-hour = година
validation-segment-minute = хвилина
validation-segment-second = секунда
validation-segment-value = значення
date-edit-validation-not-a-date = Некоректна дата
time-edit-validation-not-a-time = Некоректний час

# ── вибір кольору ──
color-picker-name = Вибір кольору
color-picker-hue-label = Відтінок
color-picker-saturation-label = Насиченість
color-picker-value-label = Яскравість
color-picker-alpha-label = Непрозорість
color-picker-red-label = Червоний
color-picker-green-label = Зелений
color-picker-blue-label = Синій
color-picker-red-short = Ч
color-picker-green-short = З
color-picker-blue-short = С
color-picker-alpha-short = А
color-picker-hue-short = В
color-picker-saturation-short = Н
color-picker-value-short = Я
color-picker-hex-label = Hex
color-picker-hex-placeholder = #RRGGBB
color-picker-current-color-label = Вибраний колір
color-picker-current-color-readout = Вибраний колір { $hex }
color-picker-swatches-name = Набір кольорів
color-picker-swatch-label = Зразок { $hex }
color-picker-swatch-selected-suffix = , вибрано
color-picker-changed-announcement = Колір змінено на { $hex }
color-picker-done-label = Готово
color-picker-cancel-label = Скасувати
color-edit-trigger-name = Колір { $hex }
color-edit-trigger-name-empty = Колір, немає
color-edit-trigger-tooltip = Відкрити вибір кольору
hex-color-input-invalid = Некоректний шістнадцятковий колір (очікується #RRGGBB)
hex-color-input-invalid-with-alpha = Некоректний шістнадцятковий колір (очікується #RRGGBB або #RRGGBBAA)
hex-color-input-corrected-shortform = { $raw } розгорнуто до { $value }
hex-color-input-corrected-uppercase = Нормалізовано до { $value }
hex-color-input-placeholder = #RRGGBB
hex-color-input-placeholder-with-alpha = #RRGGBBAA
color-edit-trigger-empty-placeholder = —

# Напис «докладніше» для розкриття розширеної підказки (заголовок акордеона,
# що відкриває докладний текст у закріпленій розширеній підказці).
tooltip-more = Докладніше

# Пункти вбудованого контекстного меню текстових полів і редактора.
menu-cut = Вирізати
menu-copy = Копіювати
menu-paste = Вставити
menu-paste-unformatted = Вставити без форматування
menu-select-all = Виділити все
menu-toggle-blockquote = Перемкнути цитату
menu-remove-blockquote = Видалити цитату

# DropZone — оголошення «живої» області для читачів екрана. Однину й множину
# обирає код Rust, а не вираз Fluent, тому формулювання побудовано так, щоб
# бути правильним за будь-якої кількості. Див. en-US.ftl для повного контексту
# та crates/teksilo-widgets/src/drop_zone.rs.
drop-zone-hover-file-one = Відпустіть, щоб додати 1 файл
drop-zone-hover-file-many = Відпустіть, щоб додати файли ({ $count })
drop-zone-hover-text = Відпустіть, щоб додати текст
drop-zone-hover-link-one = Відпустіть, щоб додати 1 посилання
drop-zone-hover-link-many = Відпустіть, щоб додати посилання ({ $count })
drop-zone-hover-generic = Відпустіть тут
drop-zone-hover-reject = Цей елемент не можна сюди перетягнути
drop-zone-added-file-one = Додано 1 файл
drop-zone-added-file-many = Додано файлів: { $count }
drop-zone-added-text = Додано текст
drop-zone-added-link-one = Додано 1 посилання
drop-zone-added-link-many = Додано посилань: { $count }
drop-zone-rejected = Елемент не прийнято

# Віджет ThemeSwitcher. Див. crates/teksilo-widgets/src/theme_switcher.rs.
theme-switcher-label = Тема
theme-switcher-light = Світла
theme-switcher-dark = Темна
theme-switcher-system = Системна

# Віджет FontPicker. Див. crates/teksilo-widgets/src/font_picker.rs.
font-picker-label = Шрифт
font-picker-placeholder = Виберіть шрифт…

# Сповіщення про невдале збереження параметрів. Див. en-US.ftl для повного
# контексту (запускається ToastRegistry::show_settings_write_failed через
# teksilo::install_toast).
settings-write-failed-toast-title = Не вдалося зберегти параметри
settings-write-failed-toast-body = Не вдалося зберегти { $file } (кількість спроб: { $attempts }); відкинуто змін у черзі: { $dropped }. { $message }

# Резервне меню вікна, що відкривається правим кліком на власному TitleBar
# там, де ОС не надає системного меню (X11). Див. en-US.ftl для повного
# контексту та crates/teksilo-widgets/src/title_bar/window_menu.rs.
window-menu-restore = Відновити
window-menu-maximize = Розгорнути
window-menu-minimize = Згорнути
window-menu-close = Закрити

# Розкриття тексту сповіщення. Див. en-US.ftl для повного контексту
# та crates/teksilo-widgets/src/toast/body.rs.
toast-show-more = Показати більше
toast-show-less = Показати менше
toast-copy-body = Копіювати
toast-body-copied = Скопійовано

# Палітра команд. Див. en-US.ftl для повного контексту
# та crates/teksilo-widgets/src/command_palette.rs.
command-palette-placeholder = Введіть команду
command-palette-empty = Немає відповідних команд
command-palette-title = Палітра команд
command-palette-result-count =
    { $count ->
        [0] Немає відповідних команд
        [one] { $count } команда
        [few] { $count } команди
        [many] { $count } команд
       *[other] { $count } команди
    }
