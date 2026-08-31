# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

# Cadeias do teksilo-widgets — tradução para português europeu.
#
# Apenas em tempo de execução: as aplicações que registam esta localização
# através de `I18nConfig::framework_locales(teksilo_widgets::framework_locales())`
# obtêm estas traduções juntamente com en-US. As chaves em falta em pt-PT
# recorrem à origem en-US através da cadeia de recurso manual de
# `I18nManager::resolve_widget` (substituição da aplicação ativa → framework
# ativo → origem da substituição da aplicação → origem do framework →
# marcador da chave). Este é o recurso do próprio teksilo-i18n e não o
# recurso por chave incorporado no `fluent-bundle` — cada `FluentBundle` é
# construído com uma única localização na sua cadeia e a pesquisa
# multilocalização é tratada na camada `I18nManager`.

a11y-status-bar-name = Estado
a11y-dialog-name = Caixa de diálogo
a11y-tooltip-name = Dica
a11y-snackbar-name = Notificação
a11y-splitter-divider-name = Divisória de painéis
a11y-splitter-pane = Painel
a11y-splitter-collapsed = Recolhido
a11y-splitter-expanded = Expandido
a11y-breadcrumb-current-page-value = página atual
a11y-toolbar-name = Barra de ferramentas
toolbar-more = Mais
segmented-control-more = Mais opções
breadcrumb-overflow = Mostrar caminho oculto
a11y-title-bar-name = Barra de título da janela
a11y-window-controls-name = Controlos da janela
a11y-window-minimize-name = Minimizar
a11y-window-maximize-name = Maximizar
a11y-window-restore-name = Restaurar
a11y-window-close-name = Fechar
a11y-stepper-indicator-strip-name = Passos
a11y-stepper-content-name = Conteúdo do passo
tab-close-tooltip = Fechar separador
a11y-builtin-browse = Procurar
a11y-builtin-expand = Expandir
a11y-builtin-search = Pesquisar
a11y-builtin-copy = Copiar
a11y-builtin-clear = Limpar
a11y-builtin-add = Adicionar
a11y-builtin-bell = Notificações
a11y-builtin-menu = Menu
a11y-builtin-more = Mais ações
a11y-builtin-visibility = Mostrar/ocultar
a11y-password-reveal = Mostrar ou ocultar a palavra-passe
a11y-caps-lock-on = Caps Lock ativado
notifications-title = Notificações
notifications-empty = Sem notificações
notifications-mark-all-read = Marcar tudo como lido
notifications-clear = Limpar tudo
notifications-filter-placeholder = Pesquisar notificações
notifications-bucket-today = Hoje
notifications-bucket-yesterday = Ontem
notifications-bucket-this-week = Esta semana
notifications-bucket-earlier = Mais antigas
notifications-archive-replay-disabled = (já não disponível)
a11y-shortcut-settings-name = Definições de atalhos
a11y-shortcut-settings-capture-hint = Prima uma tecla. Del para limpar. Esc para cancelar.
keystroke-modifier-ctrl = Ctrl
keystroke-modifier-shift = Shift
keystroke-modifier-alt = Alt
keystroke-modifier-super = Super
keystroke-separator = +
keystroke-key-space = Espaço
keystroke-key-enter = Enter
keystroke-key-escape = Esc
keystroke-key-tab = Tab
keystroke-key-backspace = Backspace
keystroke-key-delete = Del
keystroke-key-arrow-up = Cima
keystroke-key-arrow-down = Baixo
keystroke-key-arrow-left = Esquerda
keystroke-key-arrow-right = Direita
keystroke-key-home = Home
keystroke-key-end = End
keystroke-key-page-up = PgUp
keystroke-key-page-down = PgDn

# MessageBox — botões padrão e divulgação dos detalhes.
messagebox-btn-ok = OK
messagebox-btn-cancel = Cancelar
messagebox-btn-close = Fechar
messagebox-btn-yes = Sim
messagebox-btn-no = Não
messagebox-btn-yes-to-all = Sim a tudo
messagebox-btn-no-to-all = Não a tudo
messagebox-btn-save = Guardar
messagebox-btn-save-all = Guardar tudo
messagebox-btn-discard = Descartar
messagebox-btn-apply = Aplicar
messagebox-btn-reset = Repor
messagebox-btn-restore-defaults = Restaurar predefinições
messagebox-btn-abort = Abortar
messagebox-btn-retry = Tentar novamente
messagebox-btn-ignore = Ignorar
messagebox-btn-open = Abrir
messagebox-btn-help = Ajuda
messagebox-show-details = Mostrar detalhes

# Widget PrivacySettings. Ver crates/teksilo-widgets/src/privacy_settings.rs.
# Informação nos termos do art. 13.º do RGPD + botões de ação. As chaves com
# parâmetros usam a sintaxe Fluent { $nome }.
privacy-not-configured = A telemetria não está configurada para esta aplicação.
privacy-a11y-group-name = Definições de privacidade e telemetria
privacy-heading = Privacidade e telemetria
privacy-notice-controller = Os dados são tratados por { $processor }; o subcontratante técnico é { $adapter } (ponto de recolha: { $endpoint }).
privacy-notice-purposes = Finalidades: melhorar a aplicação — que funcionalidades são utilizadas, onde se concentram os erros, em que plataformas a aplicação é executada. Nenhum conteúdo de documentos, nada da área de transferência, nenhuma tecla premida, nenhuma captura de ecrã.
privacy-notice-lawful-anonymous = Fundamento jurídico: o nosso interesse legítimo na melhoria do produto (RGPD Art. 6(1)(f); isenção «medição de audiência» da CNIL).
privacy-notice-lawful-pseudonymous = Fundamento jurídico: o seu consentimento explícito (RGPD Art. 6(1)(a)).
privacy-notice-retention =
    Conservação: os dados no servidor são conservados durante { $days } { $days ->
        [one] dia
       *[other] dias
    }, no máximo.
privacy-notice-withdrawal-right = Direito de retirar o consentimento: pode desativar a qualquer momento qualquer das opções abaixo, clicar em «Retirar o consentimento» para interromper toda a recolha ou, no modo pseudonimizado, clicar em «Apagar os meus dados» para eliminar os registos do servidor.
privacy-notice-policy-link = Política de privacidade completa: { $url }

privacy-scope-section-heading = O que pode a aplicação partilhar?
privacy-scope-anonymous-metrics-label = Métricas de utilização anónimas
privacy-scope-anonymous-metrics-description = Contagem dos botões / itens de menu / atalhos utilizados, além da versão da aplicação e do sistema operativo.
privacy-scope-crash-reports-label = Relatórios de falhas
privacy-scope-crash-reports-description = Rastreios da pilha e metadados do processo quando a aplicação falha. Nenhum conteúdo de documentos, nenhum caminho de ficheiro.
privacy-scope-feature-flags-label = Sinalizadores de funcionalidades
privacy-scope-feature-flags-description = Permite que a aplicação receba atualizações dos sinalizadores de funcionalidades (por exemplo, a disponibilização gradual de novas ferramentas).

privacy-btn-reject-all = Rejeitar tudo
privacy-btn-accept-all = Aceitar tudo
privacy-btn-erase = Apagar os meus dados
privacy-btn-erase-tooltip = Pede ao servidor que elimine todos os eventos registados para esta instalação e retira em seguida o consentimento localmente.
privacy-btn-fetch = Obter os meus dados
privacy-btn-fetch-tooltip = Obtém todos os eventos que o servidor registou com o seu ID de instalação. Pode guardar o resultado em JSON.
privacy-btn-withdraw = Retirar o consentimento
privacy-btn-withdraw-tooltip = Interrompe a recolha de novos dados. Os dados já registados no servidor são conservados — utilize primeiro «Apagar os meus dados» se pretender eliminá-los.
privacy-btn-switch-to-anonymous = Mudar para o modo anónimo
privacy-btn-switch-to-pseudonymous = Mudar para o modo pseudonimizado

privacy-identity-heading = Os seus dados no servidor
privacy-identity-install-id = ID de instalação: { $id }
privacy-identity-retention =
    O servidor conserva os seus registos durante { $days } { $days ->
        [one] dia
       *[other] dias
    }, no máximo.

privacy-mode-heading = Modo de privacidade
privacy-mode-current-anonymous = Atual: anónimo (sem ID de instalação)
privacy-mode-current-pseudonymous = Atual: pseudonimizado (ID de instalação presente)
privacy-mode-blurb-anonymous = O modo anónimo não transmite qualquer identificador por dispositivo. A mudança apagará os seus registos no servidor e eliminará o UUID de instalação local — esta ação é irreversível.
privacy-mode-blurb-pseudonymous = O modo pseudonimizado gera um UUID de instalação aleatório. Poderá obter ou apagar os seus registos no servidor. Exige consentimento explícito e volta a pedi-lo na mudança de modo.

privacy-confirm-mode-switch-title = Mudar o modo de privacidade?
privacy-confirm-mode-switch-leaving-pseudonymous = Esta ação pedirá ao servidor que apague todos os eventos registados com o seu ID de instalação, eliminará o UUID de instalação local, reporá a sua decisão de consentimento e mudará o modo de privacidade. Pretende continuar?
privacy-confirm-mode-switch-leaving-anonymous = Esta ação reporá a sua decisão de consentimento e mudará o modo de privacidade. Ser-lhe-á pedido de novo o consentimento antes de qualquer nova recolha de dados. Continuar?
privacy-confirm-erase-title = Apagar os seus dados?
privacy-confirm-erase-text = Esta ação envia um pedido de eliminação de todos os eventos registados com o seu ID de instalação, elimina tudo o que ainda esteja em memória localmente e retira o consentimento para que não sejam recolhidos mais dados. A ação é irreversível.
privacy-confirm-withdraw-title = Retirar o consentimento?
privacy-confirm-withdraw-text = Não serão recolhidos mais eventos de análise a partir desta aplicação. Os dados já registados no servidor são conservados — utilize «Apagar os meus dados» antes de retirar o consentimento se pretender eliminá-los também.

privacy-fetch-success-title = Os seus dados no servidor
privacy-fetch-success-text = Eventos obtidos para esta instalação: { $count }.
privacy-fetch-saved-to = Guardado em: { $path }
privacy-fetch-write-error = Não foi possível escrever o ficheiro { $path }: { $error }
privacy-fetch-error-title = Não foi possível obter os seus dados

privacy-inspect-title = Inspecionar os dados enviados (eventos em memória: { $count })
privacy-inspect-empty = Ainda não foi emitido qualquer evento nesta sessão. Experimente interagir com a aplicação — cliques, menus e atalhos passam todos por aqui.
privacy-inspect-summary = Eventos mostrados, do mais recente para o mais antigo: { $count }.

# Calendário / DateEdit / TimeEdit / DateTimeEdit. Ver
# crates/teksilo-widgets/src/{calendar,date_edit,time_edit,date_time_edit}.rs
# e os módulos comuns em crates/teksilo-widgets/src/common/datetime/.
calendar-month-long-january = janeiro
calendar-month-long-february = fevereiro
calendar-month-long-march = março
calendar-month-long-april = abril
calendar-month-long-may = maio
calendar-month-long-june = junho
calendar-month-long-july = julho
calendar-month-long-august = agosto
calendar-month-long-september = setembro
calendar-month-long-october = outubro
calendar-month-long-november = novembro
calendar-month-long-december = dezembro

calendar-month-short-january = jan.
calendar-month-short-february = fev.
calendar-month-short-march = mar.
calendar-month-short-april = abr.
calendar-month-short-may = mai.
calendar-month-short-june = jun.
calendar-month-short-july = jul.
calendar-month-short-august = ago.
calendar-month-short-september = set.
calendar-month-short-october = out.
calendar-month-short-november = nov.
calendar-month-short-december = dez.

calendar-weekday-long-monday = segunda-feira
calendar-weekday-long-tuesday = terça-feira
calendar-weekday-long-wednesday = quarta-feira
calendar-weekday-long-thursday = quinta-feira
calendar-weekday-long-friday = sexta-feira
calendar-weekday-long-saturday = sábado
calendar-weekday-long-sunday = domingo

calendar-weekday-short-monday = seg.
calendar-weekday-short-tuesday = ter.
calendar-weekday-short-wednesday = qua.
calendar-weekday-short-thursday = qui.
calendar-weekday-short-friday = sex.
calendar-weekday-short-saturday = sáb.
calendar-weekday-short-sunday = dom.

calendar-weekday-narrow-monday = S
calendar-weekday-narrow-tuesday = T
calendar-weekday-narrow-wednesday = Q
calendar-weekday-narrow-thursday = Q
calendar-weekday-narrow-friday = S
calendar-weekday-narrow-saturday = S
calendar-weekday-narrow-sunday = D

calendar-button-previous-month = Mês anterior
calendar-button-next-month = Mês seguinte
calendar-button-previous-year = Ano anterior
calendar-button-next-year = Ano seguinte
calendar-button-today = Hoje
calendar-button-month-picker = Escolher mês
calendar-button-year-picker = Escolher ano
calendar-week-number-column = Sem.
calendar-name = Calendário
calendar-months-grid-label = Meses
calendar-years-grid-label = Anos
calendar-name-with-month = Calendário, { $month } de { $year }
calendar-cell-name = { $weekday }, { $day } de { $month } de { $year }
calendar-range-status = Seleção: { $start } – { $end }

date-edit-segment-year = Ano
date-edit-segment-month = Mês
date-edit-segment-day = Dia
date-edit-calendar-button = Escolher data
date-edit-trigger-tooltip = Abrir calendário
date-edit-name = Data
date-edit-placeholder = Selecionar uma data

time-edit-segment-hour = Hora
time-edit-segment-minute = Minuto
time-edit-segment-second = Segundo
time-edit-segment-period = a.m./p.m.
time-edit-period-am = a.m.
time-edit-period-pm = p.m.
time-edit-name = Hora
time-edit-placeholder = Selecionar uma hora

date-time-edit-name = Data e hora
date-time-edit-placeholder = Selecionar a data e a hora
date-time-edit-date-name = Data
date-time-edit-time-name = Hora
date-time-edit-trigger-tooltip = Abrir calendário
date-range-edit-name = Intervalo de datas
date-range-edit-placeholder = Selecionar um intervalo de datas
date-range-edit-start-name = Data de início
date-range-edit-end-name = Data de fim
date-range-edit-trigger-tooltip = Abrir calendário de intervalos

# Comentários de validação (TextInputField + DateEdit/TimeEdit)
validation-corrected-to = Corrigido automaticamente para { $value }
validation-corrected-with-notes = Corrigido automaticamente: { $notes }
validation-segment-clamped = { $segment } { $raw } → { $clamped }
validation-day-clamped-to-month = dia { $raw } → { $clamped } (último dia do mês)
validation-clamped-to-range = ajustado ao intervalo permitido
validation-segment-year = ano
validation-segment-month = mês
validation-segment-day = dia
validation-segment-hour = hora
validation-segment-minute = minuto
validation-segment-second = segundo
validation-segment-value = valor
date-edit-validation-not-a-date = Data inválida
time-edit-validation-not-a-time = Hora inválida

# ── seletor de cor ──
color-picker-name = Seletor de cor
color-picker-hue-label = Matiz
color-picker-saturation-label = Saturação
color-picker-value-label = Brilho
color-picker-alpha-label = Opacidade
color-picker-red-label = Vermelho
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
color-picker-current-color-label = Cor selecionada
color-picker-current-color-readout = Cor selecionada { $hex }
color-picker-swatches-name = Cores predefinidas
color-picker-swatch-label = Amostra { $hex }
color-picker-swatch-selected-suffix = , selecionada
color-picker-changed-announcement = Cor alterada para { $hex }
color-picker-done-label = Concluído
color-picker-cancel-label = Cancelar
color-edit-trigger-name = Cor { $hex }
color-edit-trigger-name-empty = Cor, nenhuma
color-edit-trigger-tooltip = Abrir seletor de cor
hex-color-input-invalid = Cor hexadecimal inválida (esperado #RRGGBB)
hex-color-input-invalid-with-alpha = Cor hexadecimal inválida (esperado #RRGGBB ou #RRGGBBAA)
hex-color-input-corrected-shortform = { $raw } expandido para { $value }
hex-color-input-corrected-uppercase = Normalizado para { $value }
hex-color-input-placeholder = #RRGGBB
hex-color-input-placeholder-with-alpha = #RRGGBBAA
color-edit-trigger-empty-placeholder = —

# Etiqueta «mais» das dicas ricas (o título do acordeão que revela o corpo
# detalhado numa dica rica fixada).
tooltip-more = Mais

# Entradas do menu de contexto dos campos de texto e do editor de texto rico.
menu-cut = Cortar
menu-copy = Copiar
menu-paste = Colar
menu-paste-unformatted = Colar sem formatação
menu-select-all = Selecionar tudo
menu-toggle-blockquote = Alternar citação
menu-remove-blockquote = Remover citação

# DropZone — anúncios da região «live» (leitores de ecrã). O singular e o
# plural são escolhidos em Rust, não por uma expressão de seleção Fluent.
# Ver en-US.ftl para o contexto completo e
# crates/teksilo-widgets/src/drop_zone.rs.
drop-zone-hover-file-one = Largar para adicionar 1 ficheiro
drop-zone-hover-file-many = Largar para adicionar { $count } ficheiros
drop-zone-hover-text = Largar para adicionar texto
drop-zone-hover-link-one = Largar para adicionar 1 ligação
drop-zone-hover-link-many = Largar para adicionar { $count } ligações
drop-zone-hover-generic = Largar aqui
drop-zone-hover-reject = Não é possível largar este item aqui
drop-zone-added-file-one = 1 ficheiro adicionado
drop-zone-added-file-many = { $count } ficheiros adicionados
drop-zone-added-text = Texto adicionado
drop-zone-added-link-one = 1 ligação adicionada
drop-zone-added-link-many = { $count } ligações adicionadas
drop-zone-rejected = Item não aceite

# Widget ThemeSwitcher. Ver crates/teksilo-widgets/src/theme_switcher.rs.
theme-switcher-label = Tema
theme-switcher-light = Claro
theme-switcher-dark = Escuro
theme-switcher-system = Sistema

# Widget FontPicker. Ver crates/teksilo-widgets/src/font_picker.rs.
font-picker-label = Tipo de letra
font-picker-placeholder = Escolher um tipo de letra…

# Notificação de falha na escrita das definições. Ver en-US.ftl para o
# contexto completo (despoletada por ToastRegistry::show_settings_write_failed
# através de teksilo::install_toast).
settings-write-failed-toast-title = Não foi possível guardar as definições
settings-write-failed-toast-body = Falha ao guardar { $file }; tentativas: { $attempts }; alterações pendentes descartadas: { $dropped }. { $message }

# Menu de janela de recurso, aberto com o botão direito numa TitleBar
# personalizada nas plataformas sem menu de janela do sistema (X11). Ver
# en-US.ftl para o contexto completo e
# crates/teksilo-widgets/src/title_bar/window_menu.rs.
window-menu-restore = Restaurar
window-menu-maximize = Maximizar
window-menu-minimize = Minimizar
window-menu-close = Fechar

# Divulgação do corpo de uma notificação. Ver en-US.ftl para o contexto
# completo e crates/teksilo-widgets/src/toast/body.rs.
toast-show-more = Mostrar mais
toast-show-less = Mostrar menos
toast-copy-body = Copiar
toast-body-copied = Copiado

# Paleta de comandos. Ver en-US.ftl para o contexto completo e
# crates/teksilo-widgets/src/command_palette.rs.
command-palette-placeholder = Escrever um comando
command-palette-empty = Nenhum comando correspondente
# Nome acessível da caixa de diálogo da paleta e do seu campo de pesquisa.
command-palette-title = Paleta de comandos
# Anunciado como descrição da caixa de diálogo e reanunciado à medida que a
# pesquisa é refinada.
command-palette-result-count =
    { $count ->
        [0] Nenhum comando correspondente
        [one] 1 comando
        [many] { $count } comandos
       *[other] { $count } comandos
    }
