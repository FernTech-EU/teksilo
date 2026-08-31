# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

# teksilo-widgets 프레임워크 문자열 — 한국어(ko-KR) 번역.
#
# 런타임 전용: `I18nConfig::framework_locales(teksilo_widgets::framework_locales())`
# 로 이 로케일을 등록한 애플리케이션만 en-US와 함께 이 번역을 사용합니다.
# ko-KR에 없는 키는 `I18nManager::resolve_widget`의 수동 폴백 체인
# (앱 재정의 활성 → 프레임워크 활성 → 앱 재정의 원본 → 프레임워크 원본 →
# 키 자리표시자)을 통해 en-US 원문으로 대체됩니다. 이는 `fluent-bundle`의
# 키 단위 폴백이 아니라 teksilo-i18n 자체의 폴백입니다. 각 `FluentBundle`은
# 체인에 로케일 하나만 두고 생성되며, 다중 로케일 조회는 `I18nManager`
# 계층에서 처리합니다.

a11y-status-bar-name = 상태
a11y-dialog-name = 대화 상자
a11y-tooltip-name = 도구 설명
a11y-snackbar-name = 스낵바
a11y-splitter-divider-name = 분할 구분선
a11y-splitter-pane = 창
a11y-splitter-collapsed = 축소됨
a11y-splitter-expanded = 확장됨
a11y-breadcrumb-current-page-value = 현재 페이지
a11y-toolbar-name = 도구 모음
toolbar-more = 더 보기
segmented-control-more = 추가 옵션
breadcrumb-overflow = 숨겨진 경로 표시
a11y-title-bar-name = 창 제목 표시줄
a11y-window-controls-name = 창 컨트롤
a11y-window-minimize-name = 최소화
a11y-window-maximize-name = 최대화
a11y-window-restore-name = 복원
a11y-window-close-name = 닫기
a11y-stepper-indicator-strip-name = 단계
a11y-stepper-content-name = 단계 내용
tab-close-tooltip = 탭 닫기
a11y-builtin-browse = 찾아보기
a11y-builtin-expand = 확장
a11y-builtin-search = 검색
a11y-builtin-copy = 복사
a11y-builtin-clear = 지우기
a11y-builtin-add = 추가
a11y-builtin-bell = 알림
a11y-builtin-menu = 메뉴
a11y-builtin-more = 추가 작업
a11y-builtin-visibility = 표시/숨기기
a11y-password-reveal = 암호 표시/숨기기
a11y-caps-lock-on = Caps Lock이 켜져 있습니다
notifications-title = 알림
notifications-empty = 알림 없음
notifications-mark-all-read = 모두 읽음으로 표시
notifications-clear = 모두 지우기
notifications-filter-placeholder = 알림 검색
notifications-bucket-today = 오늘
notifications-bucket-yesterday = 어제
notifications-bucket-this-week = 이번 주
notifications-bucket-earlier = 이전
notifications-archive-replay-disabled = (더 이상 사용할 수 없음)
a11y-shortcut-settings-name = 바로 가기 키 설정
a11y-shortcut-settings-capture-hint = 아무 키나 누르세요. 지우려면 Delete, 취소하려면 Esc를 누르세요.
keystroke-modifier-ctrl = Ctrl
keystroke-modifier-shift = Shift
keystroke-modifier-alt = Alt
keystroke-modifier-super = Super
keystroke-separator = +
keystroke-key-space = Space
keystroke-key-enter = Enter
keystroke-key-escape = Esc
keystroke-key-tab = Tab
keystroke-key-backspace = Backspace
keystroke-key-delete = Del
keystroke-key-arrow-up = ↑
keystroke-key-arrow-down = ↓
keystroke-key-arrow-left = ←
keystroke-key-arrow-right = →
keystroke-key-home = Home
keystroke-key-end = End
keystroke-key-page-up = PageUp
keystroke-key-page-down = PageDown

# MessageBox — 표준 단추 레이블과 세부 정보 표시.
messagebox-btn-ok = 확인
messagebox-btn-cancel = 취소
messagebox-btn-close = 닫기
messagebox-btn-yes = 예
messagebox-btn-no = 아니요
messagebox-btn-yes-to-all = 모두 예
messagebox-btn-no-to-all = 모두 아니요
messagebox-btn-save = 저장
messagebox-btn-save-all = 모두 저장
messagebox-btn-discard = 저장 안 함
messagebox-btn-apply = 적용
messagebox-btn-reset = 재설정
messagebox-btn-restore-defaults = 기본값 복원
messagebox-btn-abort = 중단
messagebox-btn-retry = 다시 시도
messagebox-btn-ignore = 무시
messagebox-btn-open = 열기
messagebox-btn-help = 도움말
messagebox-show-details = 자세한 내용 표시

# PrivacySettings 위젯. crates/teksilo-widgets/src/privacy_settings.rs 참조.
# GDPR 제13조 고지 사항과 작업 단추. 매개변수를 받는 키는 Fluent 변수
# 구문 { $name }을 사용합니다.
privacy-not-configured = 이 애플리케이션에는 원격 분석이 구성되어 있지 않습니다.
privacy-a11y-group-name = 개인 정보 및 원격 분석 설정
privacy-heading = 개인 정보 및 원격 분석
privacy-notice-controller = 데이터는 { $processor }에서 처리하며, 기술 수탁자는 { $adapter }입니다(수집 지점: { $endpoint }).
privacy-notice-purposes = 처리 목적: 애플리케이션 개선 — 어떤 기능이 사용되는지, 오류가 어디에 몰리는지, 어떤 플랫폼에서 실행되는지. 문서 내용, 클립보드, 키 입력, 화면 캡처는 수집하지 않습니다.
privacy-notice-lawful-anonymous = 처리 근거: 제품 개선에 대한 당사의 정당한 이익(GDPR Art. 6(1)(f); CNIL 이용자 측정 면제).
privacy-notice-lawful-pseudonymous = 처리 근거: 이용자의 명시적 동의(GDPR Art. 6(1)(a)).
privacy-notice-retention = 보유 기간: 서버에 저장된 데이터는 최대 { $days }일 동안 보관됩니다.
privacy-notice-withdrawal-right = 동의 철회권: 아래의 각 항목은 언제든지 끌 수 있습니다. "동의 철회"를 클릭하면 모든 수집이 중단되며, 가명 모드에서는 "내 데이터 삭제"를 클릭해 서버의 기록을 삭제할 수 있습니다.
privacy-notice-policy-link = 개인정보처리방침 전문: { $url }

privacy-scope-section-heading = 애플리케이션은 무엇을 공유할 수 있나요?
privacy-scope-anonymous-metrics-label = 익명 사용 통계
privacy-scope-anonymous-metrics-description = 어떤 단추 / 메뉴 항목 / 바로 가기 키를 사용했는지에 대한 집계와 앱 버전, 운영 체제 정보.
privacy-scope-crash-reports-label = 충돌 보고서
privacy-scope-crash-reports-description = 앱이 충돌했을 때의 스택 추적과 프로세스 메타데이터. 문서 내용과 파일 경로는 포함되지 않습니다.
privacy-scope-feature-flags-label = 기능 플래그
privacy-scope-feature-flags-description = 애플리케이션이 기능 플래그 업데이트를 받을 수 있도록 합니다(예: 새 도구의 단계적 배포).

privacy-btn-reject-all = 모두 거부
privacy-btn-accept-all = 모두 허용
privacy-btn-erase = 내 데이터 삭제
privacy-btn-erase-tooltip = 이 설치에 대해 기록된 모든 이벤트를 삭제하도록 서버에 요청한 다음, 로컬에서 동의를 철회합니다.
privacy-btn-fetch = 내 데이터 가져오기
privacy-btn-fetch-tooltip = 서버가 내 설치 ID로 기록한 모든 이벤트를 가져옵니다. 결과는 JSON으로 저장할 수 있습니다.
privacy-btn-withdraw = 동의 철회
privacy-btn-withdraw-tooltip = 새로운 데이터 수집을 중단합니다. 이미 서버에 기록된 데이터는 그대로 남으므로, 함께 삭제하려면 먼저 "내 데이터 삭제"를 사용하세요.
privacy-btn-switch-to-anonymous = 익명 모드로 전환
privacy-btn-switch-to-pseudonymous = 가명 모드로 전환

privacy-identity-heading = 서버에 저장된 내 데이터
privacy-identity-install-id = 설치 ID: { $id }
privacy-identity-retention = 서버는 내 기록을 최대 { $days }일 동안 보관합니다.

privacy-mode-heading = 개인 정보 모드
privacy-mode-current-anonymous = 현재: 익명(설치 ID 없음)
privacy-mode-current-pseudonymous = 현재: 가명(설치 ID 있음)
privacy-mode-blurb-anonymous = 익명 모드는 기기별 식별자를 전송하지 않습니다. 모드를 전환하면 서버에 있는 기존 기록이 삭제되고 로컬 설치 UUID도 파기됩니다. 이 작업은 되돌릴 수 없습니다.
privacy-mode-blurb-pseudonymous = 가명 모드는 임의의 설치 UUID를 생성합니다. 서버에 저장된 기록을 가져오거나 삭제할 수 있습니다. 명시적 동의가 필요하며, 전환할 때 동의를 다시 요청합니다.

privacy-confirm-mode-switch-title = 개인 정보 모드를 전환할까요?
privacy-confirm-mode-switch-leaving-pseudonymous = 내 설치 ID로 기록된 모든 이벤트를 삭제하도록 서버에 요청하고, 로컬 설치 UUID를 파기하며, 동의 결정을 초기화한 뒤 개인 정보 모드를 전환합니다. 계속할까요?
privacy-confirm-mode-switch-leaving-anonymous = 동의 결정을 초기화하고 개인 정보 모드를 전환합니다. 새로운 데이터를 수집하기 전에 동의를 다시 요청합니다. 계속할까요?
privacy-confirm-erase-title = 내 데이터를 삭제할까요?
privacy-confirm-erase-text = 내 설치 ID로 기록된 모든 이벤트에 대해 삭제 요청을 보내고, 로컬에 남아 있는 버퍼를 모두 파기하며, 더 이상 데이터가 수집되지 않도록 동의를 철회합니다. 이 작업은 되돌릴 수 없습니다.
privacy-confirm-withdraw-title = 동의를 철회할까요?
privacy-confirm-withdraw-text = 이 앱에서 더 이상 분석 이벤트를 수집하지 않습니다. 이미 서버에 기록된 데이터는 그대로 남으므로, 함께 삭제하려면 철회하기 전에 "내 데이터 삭제"를 사용하세요.

privacy-fetch-success-title = 서버에 저장된 내 데이터
privacy-fetch-success-text = 이 설치에 대해 이벤트 { $count }건을 가져왔습니다.
privacy-fetch-saved-to = 저장 위치: { $path }
privacy-fetch-write-error = 다음 파일에 쓸 수 없습니다: { $path } — { $error }
privacy-fetch-error-title = 데이터를 가져오지 못했습니다

privacy-inspect-title = 전송 데이터 검사(버퍼에 저장된 이벤트 { $count }건)
privacy-inspect-empty = 이 세션에서는 아직 전송된 이벤트가 없습니다. 앱을 사용해 보세요 — 클릭, 메뉴, 바로 가기 키가 모두 여기를 거쳐 갑니다.
privacy-inspect-summary = 최근 이벤트 { $count }건을 최신순으로 표시합니다.

# 달력 / DateEdit / TimeEdit / DateTimeEdit. 전체 맥락은 en-US.ftl 참조.
# crates/teksilo-widgets/src/{calendar,date_edit,time_edit,date_time_edit}.rs
# 및 crates/teksilo-widgets/src/common/datetime/ 아래의 공통 모듈.
calendar-month-long-january = 1월
calendar-month-long-february = 2월
calendar-month-long-march = 3월
calendar-month-long-april = 4월
calendar-month-long-may = 5월
calendar-month-long-june = 6월
calendar-month-long-july = 7월
calendar-month-long-august = 8월
calendar-month-long-september = 9월
calendar-month-long-october = 10월
calendar-month-long-november = 11월
calendar-month-long-december = 12월

calendar-month-short-january = 1월
calendar-month-short-february = 2월
calendar-month-short-march = 3월
calendar-month-short-april = 4월
calendar-month-short-may = 5월
calendar-month-short-june = 6월
calendar-month-short-july = 7월
calendar-month-short-august = 8월
calendar-month-short-september = 9월
calendar-month-short-october = 10월
calendar-month-short-november = 11월
calendar-month-short-december = 12월

calendar-weekday-long-monday = 월요일
calendar-weekday-long-tuesday = 화요일
calendar-weekday-long-wednesday = 수요일
calendar-weekday-long-thursday = 목요일
calendar-weekday-long-friday = 금요일
calendar-weekday-long-saturday = 토요일
calendar-weekday-long-sunday = 일요일

calendar-weekday-short-monday = 월
calendar-weekday-short-tuesday = 화
calendar-weekday-short-wednesday = 수
calendar-weekday-short-thursday = 목
calendar-weekday-short-friday = 금
calendar-weekday-short-saturday = 토
calendar-weekday-short-sunday = 일

calendar-weekday-narrow-monday = 월
calendar-weekday-narrow-tuesday = 화
calendar-weekday-narrow-wednesday = 수
calendar-weekday-narrow-thursday = 목
calendar-weekday-narrow-friday = 금
calendar-weekday-narrow-saturday = 토
calendar-weekday-narrow-sunday = 일

calendar-button-previous-month = 이전 달
calendar-button-next-month = 다음 달
calendar-button-previous-year = 이전 연도
calendar-button-next-year = 다음 연도
calendar-button-today = 오늘
calendar-button-month-picker = 월 선택
calendar-button-year-picker = 연도 선택
calendar-week-number-column = 주
calendar-name = 달력
calendar-months-grid-label = 월
calendar-years-grid-label = 연도
calendar-name-with-month = 달력, { $year }년 { $month }
calendar-cell-name = { $year }년 { $month } { $day }일 { $weekday }
calendar-range-status = 선택: { $start } – { $end }

date-edit-segment-year = 연도
date-edit-segment-month = 월
date-edit-segment-day = 일
date-edit-calendar-button = 날짜 선택
date-edit-trigger-tooltip = 달력 열기
date-edit-name = 날짜
date-edit-placeholder = 날짜 선택

time-edit-segment-hour = 시
time-edit-segment-minute = 분
time-edit-segment-second = 초
time-edit-segment-period = 오전/오후
time-edit-period-am = 오전
time-edit-period-pm = 오후
time-edit-name = 시간
time-edit-placeholder = 시간 선택

date-time-edit-name = 날짜 및 시간
date-time-edit-placeholder = 날짜 및 시간 선택
date-time-edit-date-name = 날짜
date-time-edit-time-name = 시간
date-time-edit-trigger-tooltip = 달력 열기
date-range-edit-name = 날짜 범위
date-range-edit-placeholder = 날짜 범위 선택
date-range-edit-start-name = 시작 날짜
date-range-edit-end-name = 종료 날짜
date-range-edit-trigger-tooltip = 범위 달력 열기

# 유효성 검사 피드백 (TextInputField + DateEdit/TimeEdit)
validation-corrected-to = 자동 수정됨: { $value }
validation-corrected-with-notes = 자동 수정됨: { $notes }
validation-segment-clamped = { $segment } { $raw } → { $clamped }
validation-day-clamped-to-month = 일 { $raw } → { $clamped }(해당 월의 마지막 날)
validation-clamped-to-range = 허용 범위로 조정됨
validation-segment-year = 연도
validation-segment-month = 월
validation-segment-day = 일
validation-segment-hour = 시
validation-segment-minute = 분
validation-segment-second = 초
validation-segment-value = 값
date-edit-validation-not-a-date = 유효한 날짜가 아닙니다
time-edit-validation-not-a-time = 유효한 시간이 아닙니다

# ── 색 선택기 ──
color-picker-name = 색 선택기
color-picker-hue-label = 색조
color-picker-saturation-label = 채도
color-picker-value-label = 명도
color-picker-alpha-label = 불투명도
color-picker-red-label = 빨강
color-picker-green-label = 초록
color-picker-blue-label = 파랑
color-picker-red-short = R
color-picker-green-short = G
color-picker-blue-short = B
color-picker-alpha-short = A
color-picker-hue-short = H
color-picker-saturation-short = S
color-picker-value-short = V
color-picker-hex-label = Hex
color-picker-hex-placeholder = #RRGGBB
color-picker-current-color-label = 선택한 색
color-picker-current-color-readout = 선택한 색 { $hex }
color-picker-swatches-name = 미리 설정된 색
color-picker-swatch-label = 색 견본 { $hex }
color-picker-swatch-selected-suffix = , 선택됨
color-picker-changed-announcement = 색 변경됨: { $hex }
color-picker-done-label = 완료
color-picker-cancel-label = 취소
color-edit-trigger-name = 색 { $hex }
color-edit-trigger-name-empty = 색, 없음
color-edit-trigger-tooltip = 색 선택기 열기
hex-color-input-invalid = 유효한 16진수 색상 코드가 아닙니다(#RRGGBB 형식이어야 합니다)
hex-color-input-invalid-with-alpha = 유효한 16진수 색상 코드가 아닙니다(#RRGGBB 또는 #RRGGBBAA 형식이어야 합니다)
hex-color-input-corrected-shortform = 확장됨: { $raw } → { $value }
hex-color-input-corrected-uppercase = 정규화됨: { $value }
hex-color-input-placeholder = #RRGGBB
hex-color-input-placeholder-with-alpha = #RRGGBBAA
color-edit-trigger-empty-placeholder = —

# 서식 있는 도구 설명의 "더 보기" 펼치기 레이블(고정된 도구 설명 안에서
# 긴 본문을 드러내는 아코디언 제목).
tooltip-more = 더 보기

# 텍스트 필드 및 서식 있는 편집기의 기본 컨텍스트 메뉴 항목.
menu-cut = 잘라내기
menu-copy = 복사
menu-paste = 붙여넣기
menu-paste-unformatted = 서식 없이 붙여넣기
menu-select-all = 모두 선택
menu-toggle-blockquote = 인용 블록 전환
menu-remove-blockquote = 인용 블록 제거

# DropZone — 라이브 영역 안내(화면 낭독기용). 단수/복수는 Fluent가 아니라
# Rust에서 선택합니다. 전체 맥락은 en-US.ftl 및
# crates/teksilo-widgets/src/drop_zone.rs 참조.
drop-zone-hover-file-one = 놓으면 파일 1개가 추가됩니다
drop-zone-hover-file-many = 놓으면 파일 { $count }개가 추가됩니다
drop-zone-hover-text = 놓으면 텍스트가 추가됩니다
drop-zone-hover-link-one = 놓으면 링크 1개가 추가됩니다
drop-zone-hover-link-many = 놓으면 링크 { $count }개가 추가됩니다
drop-zone-hover-generic = 여기에 놓기
drop-zone-hover-reject = 이 항목은 여기에 놓을 수 없습니다
drop-zone-added-file-one = 파일 1개를 추가했습니다
drop-zone-added-file-many = 파일 { $count }개를 추가했습니다
drop-zone-added-text = 텍스트를 추가했습니다
drop-zone-added-link-one = 링크 1개를 추가했습니다
drop-zone-added-link-many = 링크 { $count }개를 추가했습니다
drop-zone-rejected = 항목이 허용되지 않습니다

# ThemeSwitcher 위젯. crates/teksilo-widgets/src/theme_switcher.rs 참조.
theme-switcher-label = 테마
theme-switcher-light = 밝게
theme-switcher-dark = 어둡게
theme-switcher-system = 시스템

# FontPicker 위젯. crates/teksilo-widgets/src/font_picker.rs 참조.
font-picker-label = 글꼴
font-picker-placeholder = 글꼴 선택…

# 설정 저장 실패 알림. 전체 맥락은 en-US.ftl 참조
# (teksilo::install_toast를 통해 ToastRegistry::show_settings_write_failed에서
# 발생).
settings-write-failed-toast-title = 설정을 저장하지 못했습니다
settings-write-failed-toast-body = { $file } 저장이 { $attempts }회 시도 후 실패했습니다. 대기 중이던 변경 사항 { $dropped }건이 삭제되었습니다. { $message }

# 대체 창 메뉴. OS가 창 메뉴를 제공하지 않는 환경(X11)에서 사용자 지정
# TitleBar를 마우스 오른쪽 단추로 클릭하면 열립니다. 전체 맥락은 en-US.ftl 및
# crates/teksilo-widgets/src/title_bar/window_menu.rs 참조.
window-menu-restore = 복원
window-menu-maximize = 최대화
window-menu-minimize = 최소화
window-menu-close = 닫기

# 알림 본문 펼치기. 전체 맥락은 en-US.ftl 및
# crates/teksilo-widgets/src/toast/body.rs 참조.
toast-show-more = 자세히 보기
toast-show-less = 간략히 보기
toast-copy-body = 복사
toast-body-copied = 복사됨

# 명령 팔레트. 전체 맥락은 en-US.ftl 및
# crates/teksilo-widgets/src/command_palette.rs 참조.
command-palette-placeholder = 명령 입력
command-palette-empty = 일치하는 명령 없음
# 팔레트 대화 상자와 검색 필드의 접근성 이름. 화면에는 표시되지 않습니다.
command-palette-title = 명령 팔레트
# 대화 상자 설명으로 안내되며, 검색어를 좁힐 때마다 다시 안내됩니다.
# 한국어는 CLDR 기수 복수 범주가 other 하나뿐이므로 [0]과 *[other]만 둡니다.
command-palette-result-count =
    { $count ->
        [0] 일치하는 명령 없음
       *[other] 명령 { $count }개
    }
