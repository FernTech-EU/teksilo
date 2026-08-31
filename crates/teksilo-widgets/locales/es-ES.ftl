# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

# Cadenas del framework teksilo-widgets — traducción al español (España).
#
# Solo en tiempo de ejecución: las aplicaciones que registran esta
# configuración regional mediante
# `I18nConfig::framework_locales(teksilo_widgets::framework_locales())`
# obtienen estas traducciones junto a en-US. Las claves que falten en
# es-ES recurren al original en-US a través de la cadena de reserva
# manual de `I18nManager::resolve_widget` (anulación de la aplicación
# activa → framework activo → origen de la anulación de la aplicación →
# origen del framework → marcador de la clave). Se trata de la reserva
# propia de teksilo-i18n, no de la reserva por clave integrada en
# `fluent-bundle`: cada `FluentBundle` se construye con una sola
# configuración regional en su cadena, y la búsqueda multilingüe se
# resuelve en la capa `I18nManager`.

a11y-status-bar-name = Estado
a11y-dialog-name = Cuadro de diálogo
a11y-tooltip-name = Descripción emergente
a11y-snackbar-name = Notificación
a11y-splitter-divider-name = Separador de paneles
a11y-splitter-pane = Panel
a11y-splitter-collapsed = Contraído
a11y-splitter-expanded = Expandido
a11y-breadcrumb-current-page-value = página actual
a11y-toolbar-name = Barra de herramientas
toolbar-more = Más
segmented-control-more = Más opciones
breadcrumb-overflow = Mostrar ruta oculta
a11y-title-bar-name = Barra de título de la ventana
a11y-window-controls-name = Controles de la ventana
a11y-window-minimize-name = Minimizar
a11y-window-maximize-name = Maximizar
a11y-window-restore-name = Restaurar
a11y-window-close-name = Cerrar
a11y-stepper-indicator-strip-name = Pasos
a11y-stepper-content-name = Contenido del paso
tab-close-tooltip = Cerrar pestaña
a11y-builtin-browse = Examinar
a11y-builtin-expand = Expandir
a11y-builtin-search = Buscar
a11y-builtin-copy = Copiar
a11y-builtin-clear = Borrar
a11y-builtin-add = Añadir
a11y-builtin-bell = Notificaciones
a11y-builtin-menu = Menú
a11y-builtin-more = Más acciones
a11y-builtin-visibility = Mostrar u ocultar
a11y-password-reveal = Mostrar u ocultar la contraseña
a11y-caps-lock-on = Bloq Mayús está activado
notifications-title = Notificaciones
notifications-empty = No hay notificaciones
notifications-mark-all-read = Marcar todo como leído
notifications-clear = Borrar todo
notifications-filter-placeholder = Buscar notificaciones
notifications-bucket-today = Hoy
notifications-bucket-yesterday = Ayer
notifications-bucket-this-week = Esta semana
notifications-bucket-earlier = Anteriores
notifications-archive-replay-disabled = (ya no está disponible)
a11y-shortcut-settings-name = Configuración de atajos de teclado
a11y-shortcut-settings-capture-hint = Pulse cualquier tecla. Supr para borrar. Esc para cancelar.
keystroke-modifier-ctrl = Ctrl
keystroke-modifier-shift = Mayús
keystroke-modifier-alt = Alt
keystroke-modifier-super = Super
keystroke-separator = +
keystroke-key-space = Espacio
keystroke-key-enter = Intro
keystroke-key-escape = Esc
keystroke-key-tab = Tab
keystroke-key-backspace = Retroceso
keystroke-key-delete = Supr
keystroke-key-arrow-up = Arriba
keystroke-key-arrow-down = Abajo
keystroke-key-arrow-left = Izquierda
keystroke-key-arrow-right = Derecha
keystroke-key-home = Inicio
keystroke-key-end = Fin
keystroke-key-page-up = Re Pág
keystroke-key-page-down = Av Pág

# MessageBox — botones estándar y divulgación de los detalles.
messagebox-btn-ok = Aceptar
messagebox-btn-cancel = Cancelar
messagebox-btn-close = Cerrar
messagebox-btn-yes = Sí
messagebox-btn-no = No
messagebox-btn-yes-to-all = Sí a todo
messagebox-btn-no-to-all = No a todo
messagebox-btn-save = Guardar
messagebox-btn-save-all = Guardar todo
messagebox-btn-discard = Descartar
messagebox-btn-apply = Aplicar
messagebox-btn-reset = Restablecer
messagebox-btn-restore-defaults = Restaurar valores predeterminados
messagebox-btn-abort = Anular
messagebox-btn-retry = Reintentar
messagebox-btn-ignore = Omitir
messagebox-btn-open = Abrir
messagebox-btn-help = Ayuda
messagebox-show-details = Mostrar detalles

# Widget PrivacySettings. Consulte
# crates/teksilo-widgets/src/privacy_settings.rs. Información del art. 13
# del RGPD y botones de acción. Las claves con parámetros usan la
# sintaxis Fluent { $nombre }.
privacy-not-configured = La telemetría no está configurada para esta aplicación.
privacy-a11y-group-name = Configuración de privacidad y telemetría
privacy-heading = Privacidad y telemetría
privacy-notice-controller = Los datos son tratados por { $processor }; el encargado del tratamiento técnico es { $adapter } (punto de conexión: { $endpoint }).
privacy-notice-purposes = Finalidades: mejorar la aplicación — qué funciones se utilizan, dónde se concentran los errores, en qué plataformas se ejecuta. No se recoge el contenido de los documentos, ni el portapapeles, ni las pulsaciones de teclas, ni capturas de pantalla.
privacy-notice-lawful-anonymous = Base jurídica: nuestro interés legítimo en la mejora del producto (RGPD art. 6(1)(f); exención de la CNIL para la medición de audiencia).
privacy-notice-lawful-pseudonymous = Base jurídica: su consentimiento explícito (RGPD art. 6(1)(a)).
privacy-notice-retention =
    { $days ->
        [one] Conservación: los datos del servidor se conservan un máximo de 1 día.
       *[other] Conservación: los datos del servidor se conservan un máximo de { $days } días.
    }
privacy-notice-withdrawal-right = Derecho a retirar el consentimiento: puede desactivar en cualquier momento cualquiera de las opciones siguientes, pulsar «Retirar el consentimiento» para detener toda la recogida o, en modo seudónimo, «Suprimir mis datos» para eliminar los registros del servidor.
privacy-notice-policy-link = Política de privacidad completa: { $url }

privacy-scope-section-heading = ¿Qué puede compartir la aplicación?
privacy-scope-anonymous-metrics-label = Métricas de uso anónimas
privacy-scope-anonymous-metrics-description = Recuento de los botones, elementos de menú y atajos utilizados, además de la versión de la aplicación y el sistema operativo.
privacy-scope-crash-reports-label = Informes de fallos
privacy-scope-crash-reports-description = Trazas de la pila y metadatos del proceso cuando la aplicación falla. Sin contenido de los documentos ni rutas de archivo.
privacy-scope-feature-flags-label = Indicadores de funciones
privacy-scope-feature-flags-description = Permite que la aplicación reciba actualizaciones de los indicadores de funciones (por ejemplo, el despliegue gradual de nuevas herramientas).

privacy-btn-reject-all = Rechazar todo
privacy-btn-accept-all = Aceptar todo
privacy-btn-erase = Suprimir mis datos
privacy-btn-erase-tooltip = Solicita al servidor que elimine todos los eventos registrados para esta instalación y, a continuación, retira el consentimiento localmente.
privacy-btn-fetch = Obtener mis datos
privacy-btn-fetch-tooltip = Recupera todos los eventos que el servidor ha registrado con su identificador de instalación. Puede guardar el resultado en formato JSON.
privacy-btn-withdraw = Retirar el consentimiento
privacy-btn-withdraw-tooltip = Detiene la recogida de nuevos datos. Los datos ya registrados en el servidor se conservan: utilice antes «Suprimir mis datos» si desea eliminarlos.
privacy-btn-switch-to-anonymous = Cambiar al modo anónimo
privacy-btn-switch-to-pseudonymous = Cambiar al modo seudónimo

privacy-identity-heading = Sus datos en el servidor
privacy-identity-install-id = Identificador de instalación: { $id }
privacy-identity-retention =
    { $days ->
        [one] El servidor conserva sus registros un máximo de 1 día.
       *[other] El servidor conserva sus registros un máximo de { $days } días.
    }

privacy-mode-heading = Modo de privacidad
privacy-mode-current-anonymous = Actualmente: anónimo (sin identificador de instalación)
privacy-mode-current-pseudonymous = Actualmente: seudónimo (con identificador de instalación)
privacy-mode-blurb-anonymous = El modo anónimo no transmite ningún identificador por dispositivo. Al cambiar se suprimirán sus registros del servidor y se descartará el UUID de instalación local; esta acción no se puede deshacer.
privacy-mode-blurb-pseudonymous = El modo seudónimo genera un UUID de instalación aleatorio. Podrá obtener o suprimir sus registros del servidor. Requiere un consentimiento explícito y se le vuelve a preguntar al cambiar.

privacy-confirm-mode-switch-title = ¿Cambiar el modo de privacidad?
privacy-confirm-mode-switch-leaving-pseudonymous = Esta acción pedirá al servidor que suprima todos los eventos registrados con su identificador de instalación, descartará el UUID de instalación local, restablecerá su decisión sobre el consentimiento y cambiará el modo de privacidad. ¿Desea continuar?
privacy-confirm-mode-switch-leaving-anonymous = Esta acción restablecerá su decisión sobre el consentimiento y cambiará el modo de privacidad. Se le volverá a preguntar antes de recoger cualquier dato nuevo. ¿Desea continuar?
privacy-confirm-erase-title = ¿Suprimir sus datos?
privacy-confirm-erase-text = Esta acción envía una solicitud de supresión de todos los eventos registrados con su identificador de instalación, descarta lo que aún quede almacenado localmente en el búfer y retira el consentimiento para que no se recoja ningún dato más. La acción no se puede deshacer.
privacy-confirm-withdraw-title = ¿Retirar el consentimiento?
privacy-confirm-withdraw-text = No se recogerá ningún evento de análisis más desde esta aplicación. Los datos ya registrados en el servidor se conservan: utilice «Suprimir mis datos» antes de retirar el consentimiento si desea eliminarlos también.

privacy-fetch-success-title = Sus datos en el servidor
privacy-fetch-success-text = Eventos recuperados para esta instalación: { $count }.
privacy-fetch-saved-to = Guardado en: { $path }
privacy-fetch-write-error = No se pudo escribir el archivo { $path }: { $error }
privacy-fetch-error-title = No se pudieron obtener sus datos

privacy-inspect-title = Inspeccionar los datos enviados (eventos en el búfer: { $count })
privacy-inspect-empty = Todavía no se ha emitido ningún evento en esta sesión. Pruebe a interactuar con la aplicación: los clics, los menús y los atajos pasan todos por aquí.
privacy-inspect-summary = Eventos mostrados, del más reciente al más antiguo: { $count }.

# Calendario / DateEdit / TimeEdit / DateTimeEdit. Consulte
# crates/teksilo-widgets/src/{calendar,date_edit,time_edit,date_time_edit}.rs
# y los módulos comunes bajo crates/teksilo-widgets/src/common/datetime/.
calendar-month-long-january = enero
calendar-month-long-february = febrero
calendar-month-long-march = marzo
calendar-month-long-april = abril
calendar-month-long-may = mayo
calendar-month-long-june = junio
calendar-month-long-july = julio
calendar-month-long-august = agosto
calendar-month-long-september = septiembre
calendar-month-long-october = octubre
calendar-month-long-november = noviembre
calendar-month-long-december = diciembre

calendar-month-short-january = ene
calendar-month-short-february = feb
calendar-month-short-march = mar
calendar-month-short-april = abr
calendar-month-short-may = may
calendar-month-short-june = jun
calendar-month-short-july = jul
calendar-month-short-august = ago
calendar-month-short-september = sept
calendar-month-short-october = oct
calendar-month-short-november = nov
calendar-month-short-december = dic

calendar-weekday-long-monday = lunes
calendar-weekday-long-tuesday = martes
calendar-weekday-long-wednesday = miércoles
calendar-weekday-long-thursday = jueves
calendar-weekday-long-friday = viernes
calendar-weekday-long-saturday = sábado
calendar-weekday-long-sunday = domingo

calendar-weekday-short-monday = lun
calendar-weekday-short-tuesday = mar
calendar-weekday-short-wednesday = mié
calendar-weekday-short-thursday = jue
calendar-weekday-short-friday = vie
calendar-weekday-short-saturday = sáb
calendar-weekday-short-sunday = dom

calendar-weekday-narrow-monday = L
calendar-weekday-narrow-tuesday = M
calendar-weekday-narrow-wednesday = X
calendar-weekday-narrow-thursday = J
calendar-weekday-narrow-friday = V
calendar-weekday-narrow-saturday = S
calendar-weekday-narrow-sunday = D

calendar-button-previous-month = Mes anterior
calendar-button-next-month = Mes siguiente
calendar-button-previous-year = Año anterior
calendar-button-next-year = Año siguiente
calendar-button-today = Hoy
calendar-button-month-picker = Elegir mes
calendar-button-year-picker = Elegir año
calendar-week-number-column = Sem.
calendar-name = Calendario
calendar-months-grid-label = Meses
calendar-years-grid-label = Años
calendar-name-with-month = Calendario, { $month } de { $year }
calendar-cell-name = { $weekday }, { $day } de { $month } de { $year }
calendar-range-status = Selección: { $start } – { $end }

date-edit-segment-year = Año
date-edit-segment-month = Mes
date-edit-segment-day = Día
date-edit-calendar-button = Elegir fecha
date-edit-trigger-tooltip = Abrir calendario
date-edit-name = Fecha
date-edit-placeholder = Seleccione una fecha

time-edit-segment-hour = Hora
time-edit-segment-minute = Minuto
time-edit-segment-second = Segundo
time-edit-segment-period = a. m./p. m.
time-edit-period-am = a. m.
time-edit-period-pm = p. m.
time-edit-name = Hora
time-edit-placeholder = Seleccione una hora

date-time-edit-name = Fecha y hora
date-time-edit-placeholder = Seleccione fecha y hora
date-time-edit-date-name = Fecha
date-time-edit-time-name = Hora
date-time-edit-trigger-tooltip = Abrir calendario
date-range-edit-name = Intervalo de fechas
date-range-edit-placeholder = Seleccione un intervalo de fechas
date-range-edit-start-name = Fecha de inicio
date-range-edit-end-name = Fecha de fin
date-range-edit-trigger-tooltip = Abrir calendario de intervalos

# Mensajes de validación (TextInputField + DateEdit/TimeEdit)
validation-corrected-to = Corregido automáticamente a { $value }
validation-corrected-with-notes = Corregido automáticamente: { $notes }
validation-segment-clamped = { $segment } { $raw } → { $clamped }
validation-day-clamped-to-month = día { $raw } → { $clamped } (último día del mes)
validation-clamped-to-range = ajustado al intervalo permitido
validation-segment-year = año
validation-segment-month = mes
validation-segment-day = día
validation-segment-hour = hora
validation-segment-minute = minuto
validation-segment-second = segundo
validation-segment-value = valor
date-edit-validation-not-a-date = Fecha no válida
time-edit-validation-not-a-time = Hora no válida

# ── selector de color ──
color-picker-name = Selector de color
color-picker-hue-label = Tono
color-picker-saturation-label = Saturación
color-picker-value-label = Brillo
color-picker-alpha-label = Opacidad
color-picker-red-label = Rojo
color-picker-green-label = Verde
color-picker-blue-label = Azul
color-picker-red-short = R
color-picker-green-short = G
color-picker-blue-short = B
color-picker-alpha-short = A
color-picker-hue-short = H
color-picker-saturation-short = S
color-picker-value-short = V
color-picker-hex-label = Hex
color-picker-hex-placeholder = #RRGGBB
color-picker-current-color-label = Color seleccionado
color-picker-current-color-readout = Color seleccionado { $hex }
color-picker-swatches-name = Muestras predefinidas
color-picker-swatch-label = Muestra { $hex }
color-picker-swatch-selected-suffix = , seleccionada
color-picker-changed-announcement = Color cambiado a { $hex }
color-picker-done-label = Listo
color-picker-cancel-label = Cancelar
color-edit-trigger-name = Color { $hex }
color-edit-trigger-name-empty = Color, ninguno
color-edit-trigger-tooltip = Abrir selector de color
hex-color-input-invalid = Color hexadecimal no válido (se esperaba #RRGGBB)
hex-color-input-invalid-with-alpha = Color hexadecimal no válido (se esperaba #RRGGBB o #RRGGBBAA)
hex-color-input-corrected-shortform = { $raw } ampliado a { $value }
hex-color-input-corrected-uppercase = Normalizado a { $value }
hex-color-input-placeholder = #RRGGBB
hex-color-input-placeholder-with-alpha = #RRGGBBAA
color-edit-trigger-empty-placeholder = —

# Etiqueta «más» del desplegable de las descripciones emergentes
# enriquecidas (el título del acordeón que revela el cuerpo detallado de
# una descripción emergente enriquecida fijada).
tooltip-more = Más

# Entradas del menú contextual de los campos de texto y del editor
# de texto enriquecido.
menu-cut = Cortar
menu-copy = Copiar
menu-paste = Pegar
menu-paste-unformatted = Pegar sin formato
menu-select-all = Seleccionar todo
menu-toggle-blockquote = Alternar cita en bloque
menu-remove-blockquote = Quitar cita en bloque

# DropZone — anuncios de la región «live» (lectores de pantalla).
# Consulte en-US.ftl para el contexto completo: el singular y el plural
# se eligen en Rust, no con una expresión de selección de Fluent.
drop-zone-hover-file-one = Suelte para añadir 1 archivo
drop-zone-hover-file-many = Suelte para añadir { $count } archivos
drop-zone-hover-text = Suelte para añadir texto
drop-zone-hover-link-one = Suelte para añadir 1 enlace
drop-zone-hover-link-many = Suelte para añadir { $count } enlaces
drop-zone-hover-generic = Suelte aquí
drop-zone-hover-reject = Este elemento no se puede soltar aquí
drop-zone-added-file-one = 1 archivo añadido
drop-zone-added-file-many = { $count } archivos añadidos
drop-zone-added-text = Texto añadido
drop-zone-added-link-one = 1 enlace añadido
drop-zone-added-link-many = { $count } enlaces añadidos
drop-zone-rejected = Elemento no aceptado

# Widget ThemeSwitcher. Consulte crates/teksilo-widgets/src/theme_switcher.rs.
theme-switcher-label = Tema
theme-switcher-light = Claro
theme-switcher-dark = Oscuro
theme-switcher-system = Sistema

# Widget FontPicker. Consulte crates/teksilo-widgets/src/font_picker.rs.
font-picker-label = Fuente
font-picker-placeholder = Seleccione una fuente…

# Notificación de fallo al escribir la configuración. Consulte en-US.ftl
# para el contexto completo (la lanza
# ToastRegistry::show_settings_write_failed a través de
# teksilo::install_toast).
settings-write-failed-toast-title = No se pudo guardar la configuración
settings-write-failed-toast-body = No se pudo guardar { $file } (intentos: { $attempts }); cambios en cola descartados: { $dropped }. { $message }

# Menú de ventana de reserva, abierto al hacer clic con el botón derecho
# en una TitleBar personalizada allí donde el sistema no ofrece ninguno
# (X11). Consulte en-US.ftl para el contexto completo y
# crates/teksilo-widgets/src/title_bar/window_menu.rs.
window-menu-restore = Restaurar
window-menu-maximize = Maximizar
window-menu-minimize = Minimizar
window-menu-close = Cerrar

# Despliegue del cuerpo de una notificación. Consulte en-US.ftl para el
# contexto completo y crates/teksilo-widgets/src/toast/body.rs.
toast-show-more = Mostrar más
toast-show-less = Mostrar menos
toast-copy-body = Copiar
toast-body-copied = Copiado

# Paleta de comandos. Consulte en-US.ftl para el contexto completo y
# crates/teksilo-widgets/src/command_palette.rs.
command-palette-placeholder = Escriba un comando
command-palette-empty = No hay comandos coincidentes
command-palette-title = Paleta de comandos
command-palette-result-count =
    { $count ->
        [0] No hay comandos coincidentes
        [one] 1 comando
        [many] { $count } de comandos
       *[other] { $count } comandos
    }
