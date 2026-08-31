# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

# teksilo-widgets framework strings — Russian (русский) translation.
#
# Runtime-only: applications that register this locale via
# `I18nConfig::framework_locales(teksilo_widgets::framework_locales())`
# get these translations alongside en-US. Keys missing from ru-RU
# fall back to the en-US source via `I18nManager::resolve_widget`'s
# manual fallback chain (app override active → framework active →
# app override source → framework source → key placeholder). This is
# teksilo-i18n's own fallback, not `fluent-bundle`'s built-in per-key
# fallback — each `FluentBundle` is constructed with a single locale
# in its chain, and the multi-locale lookup is handled at the
# `I18nManager` layer.

a11y-status-bar-name = Состояние
a11y-dialog-name = Диалоговое окно
a11y-tooltip-name = Всплывающая подсказка
a11y-snackbar-name = Уведомление
a11y-splitter-divider-name = Разделитель
a11y-splitter-pane = Область
a11y-splitter-collapsed = Свёрнуто
a11y-splitter-expanded = Развёрнуто
a11y-breadcrumb-current-page-value = текущая страница
a11y-toolbar-name = Панель инструментов
toolbar-more = Ещё
segmented-control-more = Другие параметры
breadcrumb-overflow = Показать скрытый путь
a11y-title-bar-name = Строка заголовка окна
a11y-window-controls-name = Элементы управления окном
a11y-window-minimize-name = Свернуть
a11y-window-maximize-name = Развернуть
a11y-window-restore-name = Восстановить
a11y-window-close-name = Закрыть
a11y-stepper-indicator-strip-name = Шаги
a11y-stepper-content-name = Содержимое шага
tab-close-tooltip = Закрыть вкладку
a11y-builtin-browse = Обзор
a11y-builtin-expand = Развернуть
a11y-builtin-search = Поиск
a11y-builtin-copy = Копировать
a11y-builtin-clear = Очистить
a11y-builtin-add = Добавить
a11y-builtin-bell = Уведомления
a11y-builtin-menu = Меню
a11y-builtin-more = Другие действия
a11y-builtin-visibility = Показать или скрыть
a11y-password-reveal = Показать или скрыть пароль
a11y-caps-lock-on = Caps Lock включён
notifications-title = Уведомления
notifications-empty = Нет уведомлений
notifications-mark-all-read = Отметить все как прочитанные
notifications-clear = Очистить все
notifications-filter-placeholder = Поиск по уведомлениям
notifications-bucket-today = Сегодня
notifications-bucket-yesterday = Вчера
notifications-bucket-this-week = На этой неделе
notifications-bucket-earlier = Ранее
notifications-archive-replay-disabled = (больше недоступно)
a11y-shortcut-settings-name = Настройка сочетаний клавиш
a11y-shortcut-settings-capture-hint = Нажмите любую клавишу. Del — очистить. Esc — отмена.
keystroke-modifier-ctrl = Ctrl
keystroke-modifier-shift = Shift
keystroke-modifier-alt = Alt
keystroke-modifier-super = Super
keystroke-separator = +
keystroke-key-space = Пробел
keystroke-key-enter = Enter
keystroke-key-escape = Esc
keystroke-key-tab = Tab
keystroke-key-backspace = Backspace
keystroke-key-delete = Del
keystroke-key-arrow-up = Вверх
keystroke-key-arrow-down = Вниз
keystroke-key-arrow-left = Влево
keystroke-key-arrow-right = Вправо
keystroke-key-home = Home
keystroke-key-end = End
keystroke-key-page-up = PgUp
keystroke-key-page-down = PgDn

# MessageBox — стандартные кнопки и раскрытие подробностей.
messagebox-btn-ok = ОК
messagebox-btn-cancel = Отмена
messagebox-btn-close = Закрыть
messagebox-btn-yes = Да
messagebox-btn-no = Нет
messagebox-btn-yes-to-all = Да для всех
messagebox-btn-no-to-all = Нет для всех
messagebox-btn-save = Сохранить
messagebox-btn-save-all = Сохранить все
messagebox-btn-discard = Отменить изменения
messagebox-btn-apply = Применить
messagebox-btn-reset = Сбросить
messagebox-btn-restore-defaults = По умолчанию
messagebox-btn-abort = Прервать
messagebox-btn-retry = Повторить
messagebox-btn-ignore = Пропустить
messagebox-btn-open = Открыть
messagebox-btn-help = Справка
messagebox-show-details = Показать подробности

# Виджет PrivacySettings. См. crates/teksilo-widgets/src/privacy_settings.rs.
# Информирование по ст. 13 GDPR и кнопки действий. Ключи с параметрами
# используют синтаксис Fluent { $имя }.
privacy-not-configured = Телеметрия не настроена для этого приложения.
privacy-a11y-group-name = Параметры конфиденциальности и телеметрии
privacy-heading = Конфиденциальность и телеметрия
privacy-notice-controller = Обработку данных осуществляет { $processor }; технический обработчик — { $adapter } (точка приёма: { $endpoint }).
privacy-notice-purposes = Цели: улучшение приложения — какие функции используются, где чаще возникают ошибки, на каких платформах приложение работает. Никакого содержимого документов, буфера обмена, нажатий клавиш и снимков экрана.
privacy-notice-lawful-anonymous = Правовое основание: наш законный интерес в улучшении продукта (GDPR, ст. 6(1)(f); исключение CNIL для измерения аудитории).
privacy-notice-lawful-pseudonymous = Правовое основание: ваше явное согласие (GDPR, ст. 6(1)(a)).
privacy-notice-retention = Хранение: данные на сервере хранятся не более { $days } дн.
privacy-notice-withdrawal-right = Право на отзыв: вы можете в любой момент отключить любой переключатель ниже, нажать «Отозвать согласие», чтобы прекратить весь сбор данных, либо в псевдонимном режиме нажать «Удалить мои данные», чтобы удалить записи с сервера.
privacy-notice-policy-link = Полная политика конфиденциальности: { $url }

privacy-scope-section-heading = Чем приложение может делиться?
privacy-scope-anonymous-metrics-label = Анонимная статистика использования
privacy-scope-anonymous-metrics-description = Подсчёт используемых кнопок, пунктов меню и сочетаний клавиш, а также версия приложения и операционная система.
privacy-scope-crash-reports-label = Отчёты о сбоях
privacy-scope-crash-reports-description = Трассировки стека и метаданные процесса при сбое приложения. Никакого содержимого документов, никаких путей к файлам.
privacy-scope-feature-flags-label = Флаги функций
privacy-scope-feature-flags-description = Позволяет приложению получать обновления флагов функций (например, постепенное включение новых инструментов).

privacy-btn-reject-all = Отклонить все
privacy-btn-accept-all = Принять все
privacy-btn-erase = Удалить мои данные
privacy-btn-erase-tooltip = Запрашивает у сервера удаление всех событий, записанных для этой установки, затем локально отзывает согласие.
privacy-btn-fetch = Получить мои данные
privacy-btn-fetch-tooltip = Загружает все события, записанные сервером под вашим идентификатором установки. Результат можно сохранить в формате JSON.
privacy-btn-withdraw = Отозвать согласие
privacy-btn-withdraw-tooltip = Прекращает сбор новых данных. Уже записанные на сервере данные сохраняются — сначала нажмите «Удалить мои данные», если хотите их удалить.
privacy-btn-switch-to-anonymous = Перейти в анонимный режим
privacy-btn-switch-to-pseudonymous = Перейти в псевдонимный режим

privacy-identity-heading = Ваши данные на сервере
privacy-identity-install-id = Идентификатор установки: { $id }
privacy-identity-retention = Сервер хранит ваши записи не более { $days } дн.

privacy-mode-heading = Режим конфиденциальности
privacy-mode-current-anonymous = Сейчас: анонимный (без идентификатора установки)
privacy-mode-current-pseudonymous = Сейчас: псевдонимный (идентификатор установки задан)
privacy-mode-blurb-anonymous = Анонимный режим не передаёт идентификатор устройства. Переключение удалит ваши записи на сервере и локальный UUID установки — это действие необратимо.
privacy-mode-blurb-pseudonymous = Псевдонимный режим создаёт случайный UUID установки. Вы сможете получить или удалить свои записи на сервере. Требуется явное согласие, которое запрашивается заново при переключении.

privacy-confirm-mode-switch-title = Сменить режим конфиденциальности?
privacy-confirm-mode-switch-leaving-pseudonymous = Это действие запросит у сервера удаление всех событий, записанных под вашим идентификатором установки, удалит локальный UUID установки, сбросит ваше решение о согласии и сменит режим конфиденциальности. Продолжить?
privacy-confirm-mode-switch-leaving-anonymous = Это действие сбросит ваше решение о согласии и сменит режим конфиденциальности. Перед сбором новых данных вас спросят снова. Продолжить?
privacy-confirm-erase-title = Удалить ваши данные?
privacy-confirm-erase-text = Будет отправлен запрос на удаление каждого события, записанного под вашим идентификатором установки, удалено всё, что ещё хранится в локальном буфере, и отозвано согласие, чтобы данные больше не собирались. Это действие необратимо.
privacy-confirm-withdraw-title = Отозвать согласие?
privacy-confirm-withdraw-text = Аналитические события из этого приложения больше собираться не будут. Уже записанные на сервере данные сохраняются — если вы хотите удалить и их, нажмите «Удалить мои данные» перед отзывом согласия.

privacy-fetch-success-title = Ваши данные на сервере
privacy-fetch-success-text = Получено событий для этой установки: { $count }.
privacy-fetch-saved-to = Сохранено в: { $path }
privacy-fetch-write-error = Не удалось записать файл { $path }: { $error }
privacy-fetch-error-title = Не удалось получить ваши данные

privacy-inspect-title = Просмотр отправляемых данных (событий в буфере: { $count })
privacy-inspect-empty = В этом сеансе ещё не было отправлено ни одного события. Поработайте с приложением — щелчки, меню и сочетания клавиш проходят через этот механизм.
privacy-inspect-summary = Показано последних событий: { $count } (начиная с самых новых).

# Календарь / DateEdit / TimeEdit / DateTimeEdit. См.
# crates/teksilo-widgets/src/{calendar,date_edit,time_edit,date_time_edit}.rs
# и общие модули в crates/teksilo-widgets/src/common/datetime/.
# Названия месяцев и дней недели — самостоятельные (stand-alone) формы CLDR.
calendar-month-long-january = январь
calendar-month-long-february = февраль
calendar-month-long-march = март
calendar-month-long-april = апрель
calendar-month-long-may = май
calendar-month-long-june = июнь
calendar-month-long-july = июль
calendar-month-long-august = август
calendar-month-long-september = сентябрь
calendar-month-long-october = октябрь
calendar-month-long-november = ноябрь
calendar-month-long-december = декабрь

calendar-month-short-january = янв.
calendar-month-short-february = февр.
calendar-month-short-march = март
calendar-month-short-april = апр.
calendar-month-short-may = май
calendar-month-short-june = июнь
calendar-month-short-july = июль
calendar-month-short-august = авг.
calendar-month-short-september = сент.
calendar-month-short-october = окт.
calendar-month-short-november = нояб.
calendar-month-short-december = дек.

calendar-weekday-long-monday = понедельник
calendar-weekday-long-tuesday = вторник
calendar-weekday-long-wednesday = среда
calendar-weekday-long-thursday = четверг
calendar-weekday-long-friday = пятница
calendar-weekday-long-saturday = суббота
calendar-weekday-long-sunday = воскресенье

calendar-weekday-short-monday = пн
calendar-weekday-short-tuesday = вт
calendar-weekday-short-wednesday = ср
calendar-weekday-short-thursday = чт
calendar-weekday-short-friday = пт
calendar-weekday-short-saturday = сб
calendar-weekday-short-sunday = вс

calendar-weekday-narrow-monday = П
calendar-weekday-narrow-tuesday = В
calendar-weekday-narrow-wednesday = С
calendar-weekday-narrow-thursday = Ч
calendar-weekday-narrow-friday = П
calendar-weekday-narrow-saturday = С
calendar-weekday-narrow-sunday = В

calendar-button-previous-month = Предыдущий месяц
calendar-button-next-month = Следующий месяц
calendar-button-previous-year = Предыдущий год
calendar-button-next-year = Следующий год
calendar-button-today = Сегодня
calendar-button-month-picker = Выбрать месяц
calendar-button-year-picker = Выбрать год
calendar-week-number-column = Нед.
calendar-name = Календарь
calendar-months-grid-label = Месяцы
calendar-years-grid-label = Годы
calendar-name-with-month = Календарь, { $month } { $year }
calendar-cell-name = { $weekday }, { $day }, { $month } { $year }
calendar-range-status = Выбрано: { $start } – { $end }

date-edit-segment-year = Год
date-edit-segment-month = Месяц
date-edit-segment-day = День
date-edit-calendar-button = Выбрать дату
date-edit-trigger-tooltip = Открыть календарь
date-edit-name = Дата
date-edit-placeholder = Выберите дату

time-edit-segment-hour = Час
time-edit-segment-minute = Минута
time-edit-segment-second = Секунда
time-edit-segment-period = AM/PM
time-edit-period-am = AM
time-edit-period-pm = PM
time-edit-name = Время
time-edit-placeholder = Выберите время

date-time-edit-name = Дата и время
date-time-edit-placeholder = Выберите дату и время
date-time-edit-date-name = Дата
date-time-edit-time-name = Время
date-time-edit-trigger-tooltip = Открыть календарь
date-range-edit-name = Диапазон дат
date-range-edit-placeholder = Выберите диапазон дат
date-range-edit-start-name = Начальная дата
date-range-edit-end-name = Конечная дата
date-range-edit-trigger-tooltip = Открыть календарь диапазона

# Сообщения проверки ввода (TextInputField + DateEdit/TimeEdit)
validation-corrected-to = Исправлено на { $value }
validation-corrected-with-notes = Исправлено: { $notes }
validation-segment-clamped = { $segment } { $raw } → { $clamped }
validation-day-clamped-to-month = день { $raw } → { $clamped } (последний день месяца)
validation-clamped-to-range = приведено к допустимому диапазону
validation-segment-year = год
validation-segment-month = месяц
validation-segment-day = день
validation-segment-hour = час
validation-segment-minute = минута
validation-segment-second = секунда
validation-segment-value = значение
date-edit-validation-not-a-date = Недопустимая дата
time-edit-validation-not-a-time = Недопустимое время

# ── выбор цвета ──
color-picker-name = Выбор цвета
color-picker-hue-label = Цветовой тон
color-picker-saturation-label = Насыщенность
color-picker-value-label = Яркость
color-picker-alpha-label = Непрозрачность
color-picker-red-label = Красный
color-picker-green-label = Зелёный
color-picker-blue-label = Синий
color-picker-red-short = К
color-picker-green-short = З
color-picker-blue-short = С
color-picker-alpha-short = А
color-picker-hue-short = Ц
color-picker-saturation-short = Н
color-picker-value-short = Я
color-picker-hex-label = Hex
color-picker-hex-placeholder = #RRGGBB
color-picker-current-color-label = Выбранный цвет
color-picker-current-color-readout = Выбранный цвет { $hex }
color-picker-swatches-name = Образцы цветов
color-picker-swatch-label = Образец { $hex }
color-picker-swatch-selected-suffix = , выбран
color-picker-changed-announcement = Цвет изменён на { $hex }
color-picker-done-label = Готово
color-picker-cancel-label = Отмена
color-edit-trigger-name = Цвет { $hex }
color-edit-trigger-name-empty = Цвет, не выбран
color-edit-trigger-tooltip = Открыть выбор цвета
hex-color-input-invalid = Недопустимый шестнадцатеричный код цвета (ожидается #RRGGBB)
hex-color-input-invalid-with-alpha = Недопустимый шестнадцатеричный код цвета (ожидается #RRGGBB или #RRGGBBAA)
hex-color-input-corrected-shortform = { $raw } развёрнуто до { $value }
hex-color-input-corrected-uppercase = Приведено к виду { $value }
hex-color-input-placeholder = #RRGGBB
hex-color-input-placeholder-with-alpha = #RRGGBBAA
color-edit-trigger-empty-placeholder = —

# Подпись раскрытия «ещё» у расширенных подсказок (заголовок аккордеона,
# раскрывающий подробное содержимое закреплённой расширенной подсказки).
tooltip-more = Ещё

# Пункты встроенного контекстного меню текстовых полей и редактора
# форматированного текста.
menu-cut = Вырезать
menu-copy = Копировать
menu-paste = Вставить
menu-paste-unformatted = Вставить без форматирования
menu-select-all = Выделить всё
menu-toggle-blockquote = Переключить цитату
menu-remove-blockquote = Убрать цитату

# DropZone — объявления «живой» области для программ чтения с экрана.
# Форма единственного/множественного числа выбирается в Rust, а не
# выражением Fluent, поэтому строки со счётчиком построены так, чтобы
# оставаться грамматичными при любом значении. См. en-US.ftl для полного
# контекста и crates/teksilo-widgets/src/drop_zone.rs.
drop-zone-hover-file-one = Отпустите, чтобы добавить 1 файл
drop-zone-hover-file-many = Отпустите, чтобы добавить файлы: { $count }
drop-zone-hover-text = Отпустите, чтобы добавить текст
drop-zone-hover-link-one = Отпустите, чтобы добавить 1 ссылку
drop-zone-hover-link-many = Отпустите, чтобы добавить ссылки: { $count }
drop-zone-hover-generic = Отпустите здесь
drop-zone-hover-reject = Этот элемент нельзя поместить сюда
drop-zone-added-file-one = Добавлен 1 файл
drop-zone-added-file-many = Добавлено файлов: { $count }
drop-zone-added-text = Текст добавлен
drop-zone-added-link-one = Добавлена 1 ссылка
drop-zone-added-link-many = Добавлено ссылок: { $count }
drop-zone-rejected = Элемент не принят

# Виджет ThemeSwitcher. См. crates/teksilo-widgets/src/theme_switcher.rs.
theme-switcher-label = Тема
theme-switcher-light = Светлая
theme-switcher-dark = Тёмная
theme-switcher-system = Системная

# Виджет FontPicker. См. crates/teksilo-widgets/src/font_picker.rs.
font-picker-label = Шрифт
font-picker-placeholder = Выберите шрифт…

# Уведомление об ошибке записи параметров. См. en-US.ftl для полного
# контекста (вызывается из ToastRegistry::show_settings_write_failed
# через teksilo::install_toast).
settings-write-failed-toast-title = Не удалось сохранить параметры
settings-write-failed-toast-body = Не удалось сохранить { $file }; число попыток: { $attempts }; отброшено изменений в очереди: { $dropped }. { $message }

# Резервное меню окна, открываемое правой кнопкой мыши по пользовательской
# строке заголовка там, где ОС не предоставляет собственного меню (X11).
# См. en-US.ftl для полного контекста и
# crates/teksilo-widgets/src/title_bar/window_menu.rs.
window-menu-restore = Восстановить
window-menu-maximize = Развернуть
window-menu-minimize = Свернуть
window-menu-close = Закрыть

# Раскрытие текста уведомления. См. en-US.ftl для полного контекста и
# crates/teksilo-widgets/src/toast/body.rs.
toast-show-more = Показать больше
toast-show-less = Показать меньше
toast-copy-body = Копировать
toast-body-copied = Скопировано

# Палитра команд. См. en-US.ftl для полного контекста и
# crates/teksilo-widgets/src/command_palette.rs.
command-palette-placeholder = Введите команду
command-palette-empty = Нет подходящих команд
command-palette-title = Палитра команд
command-palette-result-count =
    { $count ->
        [0] Нет подходящих команд
        [one] { $count } команда
        [few] { $count } команды
        [many] { $count } команд
       *[other] { $count } команды
    }
