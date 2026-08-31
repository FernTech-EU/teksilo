# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

# teksilo-widgets framework strings — Japanese translation (日本語).
#
# Runtime-only: applications that register this locale via
# `I18nConfig::framework_locales(teksilo_widgets::framework_locales())`
# get these translations alongside en-US. Keys missing from ja-JP
# fall back to the en-US source via `I18nManager::resolve_widget`'s
# manual fallback chain (app override active → framework active →
# app override source → framework source → key placeholder). This is
# teksilo-i18n's own fallback, not `fluent-bundle`'s built-in per-key
# fallback — each `FluentBundle` is constructed with a single locale
# in its chain, and the multi-locale lookup is handled at the
# `I18nManager` layer.

a11y-status-bar-name = ステータス
a11y-dialog-name = ダイアログ
a11y-tooltip-name = ツールチップ
a11y-snackbar-name = 通知
a11y-splitter-divider-name = 分割バー
a11y-splitter-pane = ペイン
a11y-splitter-collapsed = 折りたたみ済み
a11y-splitter-expanded = 展開済み
a11y-breadcrumb-current-page-value = 現在のページ
a11y-toolbar-name = ツールバー
toolbar-more = その他
segmented-control-more = その他のオプション
breadcrumb-overflow = 非表示のパスを表示
a11y-title-bar-name = ウィンドウのタイトルバー
a11y-window-controls-name = ウィンドウコントロール
a11y-window-minimize-name = 最小化
a11y-window-maximize-name = 最大化
a11y-window-restore-name = 元に戻す
a11y-window-close-name = 閉じる
a11y-stepper-indicator-strip-name = ステップ
a11y-stepper-content-name = ステップの内容
tab-close-tooltip = タブを閉じる
a11y-builtin-browse = 参照
a11y-builtin-expand = 拡大
a11y-builtin-search = 検索
a11y-builtin-copy = コピー
a11y-builtin-clear = クリア
a11y-builtin-add = 追加
a11y-builtin-bell = 通知
a11y-builtin-menu = メニュー
a11y-builtin-more = その他の操作
a11y-builtin-visibility = 表示/非表示の切り替え
a11y-password-reveal = パスワードの表示/非表示の切り替え
a11y-caps-lock-on = Caps Lock がオンになっています
notifications-title = 通知
notifications-empty = 通知はありません
notifications-mark-all-read = すべて既読にする
notifications-clear = すべてクリア
notifications-filter-placeholder = 通知を検索
notifications-bucket-today = 今日
notifications-bucket-yesterday = 昨日
notifications-bucket-this-week = 今週
notifications-bucket-earlier = それ以前
notifications-archive-replay-disabled = （現在は利用できません）
a11y-shortcut-settings-name = ショートカットの設定
a11y-shortcut-settings-capture-hint = 任意のキーを押してください。Delete でクリア、Esc でキャンセルします。
keystroke-modifier-ctrl = Ctrl
keystroke-modifier-shift = Shift
keystroke-modifier-alt = Alt
keystroke-modifier-super = Win
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

# MessageBox — 標準ボタンと詳細の開示。
messagebox-btn-ok = OK
messagebox-btn-cancel = キャンセル
messagebox-btn-close = 閉じる
messagebox-btn-yes = はい
messagebox-btn-no = いいえ
messagebox-btn-yes-to-all = すべてはい
messagebox-btn-no-to-all = すべていいえ
messagebox-btn-save = 保存
messagebox-btn-save-all = すべて保存
messagebox-btn-discard = 変更を破棄
messagebox-btn-apply = 適用
messagebox-btn-reset = リセット
messagebox-btn-restore-defaults = デフォルトに戻す
messagebox-btn-abort = 中止
messagebox-btn-retry = 再試行
messagebox-btn-ignore = 無視
messagebox-btn-open = 開く
messagebox-btn-help = ヘルプ
messagebox-show-details = 詳細を表示

# PrivacySettings ウィジェット。crates/teksilo-widgets/src/privacy_settings.rs
# を参照。GDPR 第13条の情報提供と操作ボタン。引数を取るキーは Fluent の
# { $name } 構文を使用。
privacy-not-configured = このアプリケーションではテレメトリが構成されていません。
privacy-a11y-group-name = プライバシーとテレメトリの設定
privacy-heading = プライバシーとテレメトリ
privacy-notice-controller = データは { $processor } が取り扱います。技術的な処理者は { $adapter } です（エンドポイント：{ $endpoint }）。
privacy-notice-purposes = 利用目的：アプリケーションの改善（どの機能が使われているか、不具合がどこに集中しているか、どのプラットフォームで動作しているか）。ドキュメントの内容、クリップボード、キー入力、画面キャプチャは一切収集しません。
privacy-notice-lawful-anonymous = 適法性の根拠：製品改善における当社の正当な利益（GDPR Art. 6(1)(f)、CNIL のオーディエンス測定に関する適用除外）。
privacy-notice-lawful-pseudonymous = 適法性の根拠：お客様の明示的な同意（GDPR Art. 6(1)(a)）。
privacy-notice-retention = 保存期間：サーバー側のデータは最長{ $days }日間保持されます。
privacy-notice-withdrawal-right = 撤回する権利：以下のスイッチはいつでもオフにできます。「同意を撤回」をクリックすればすべての収集を停止でき、仮名化モードでは「データを消去」でサーバー上の記録を削除できます。
privacy-notice-policy-link = プライバシーポリシー全文：{ $url }

privacy-scope-section-heading = アプリケーションが共有できるもの
privacy-scope-anonymous-metrics-label = 匿名の利用状況データ
privacy-scope-anonymous-metrics-description = どのボタン・メニュー項目・ショートカットが使われたかの回数、アプリのバージョン、OS。
privacy-scope-crash-reports-label = クラッシュレポート
privacy-scope-crash-reports-description = クラッシュ時のスタックトレースとプロセスのメタデータ。ドキュメントの内容やファイルパスは含みません。
privacy-scope-feature-flags-label = 機能フラグ
privacy-scope-feature-flags-description = アプリケーションが機能フラグの更新を受け取れるようにします（新しいツールの段階的な提供など）。

privacy-btn-reject-all = すべて拒否
privacy-btn-accept-all = すべて同意
privacy-btn-erase = データを消去
privacy-btn-erase-tooltip = このインストールについて記録されたすべてのイベントの削除をサーバーに要求し、その後ローカルで同意を撤回します。
privacy-btn-fetch = データを取得
privacy-btn-fetch-tooltip = お客様のインストール ID でサーバーが記録したすべてのイベントを取得します。結果は JSON として保存できます。
privacy-btn-withdraw = 同意を撤回
privacy-btn-withdraw-tooltip = 新しいデータの収集を停止します。サーバーに記録済みのデータはそのまま残ります。削除も必要な場合は、先に「データを消去」を実行してください。
privacy-btn-switch-to-anonymous = 匿名モードに切り替え
privacy-btn-switch-to-pseudonymous = 仮名化モードに切り替え

privacy-identity-heading = サーバー上のお客様のデータ
privacy-identity-install-id = インストール ID：{ $id }
privacy-identity-retention = サーバーはお客様の記録を最長{ $days }日間保持します。

privacy-mode-heading = プライバシーモード
privacy-mode-current-anonymous = 現在：匿名（インストール ID なし）
privacy-mode-current-pseudonymous = 現在：仮名化（インストール ID あり）
privacy-mode-blurb-anonymous = 匿名モードはデバイスごとの識別子を一切送信しません。切り替えると、サーバー上の既存の記録が消去され、ローカルのインストール UUID も破棄されます。この操作は取り消せません。
privacy-mode-blurb-pseudonymous = 仮名化モードはランダムなインストール UUID を生成します。サーバー上の記録の取得や消去が可能になります。明示的な同意が必要で、切り替え時に改めて確認します。

privacy-confirm-mode-switch-title = プライバシーモードを切り替えますか？
privacy-confirm-mode-switch-leaving-pseudonymous = お客様のインストール ID で記録されたすべてのイベントの消去をサーバーに要求し、ローカルのインストール UUID を破棄し、同意の決定をリセットして、プライバシーモードを切り替えます。続行しますか？
privacy-confirm-mode-switch-leaving-anonymous = 同意の決定をリセットして、プライバシーモードを切り替えます。新しいデータを収集する前に改めて確認します。続行しますか？
privacy-confirm-erase-title = データを消去しますか？
privacy-confirm-erase-text = お客様のインストール ID で記録されたすべてのイベントの削除を要求し、ローカルのバッファに残っているデータを破棄し、以後データが収集されないよう同意を撤回します。この操作は取り消せません。
privacy-confirm-withdraw-title = 同意を撤回しますか？
privacy-confirm-withdraw-text = このアプリからは以後、分析イベントを収集しません。サーバーに記録済みのデータはそのまま残ります。あわせて削除する場合は、撤回する前に「データを消去」を実行してください。

privacy-fetch-success-title = サーバー上のお客様のデータ
privacy-fetch-success-text = このインストールについて{ $count }件のイベントを取得しました。
privacy-fetch-saved-to = 保存先：{ $path }
privacy-fetch-write-error = ファイル { $path } を書き込めませんでした：{ $error }
privacy-fetch-error-title = データを取得できませんでした

privacy-inspect-title = 送信データを確認（バッファ内{ $count }件のイベント）
privacy-inspect-empty = このセッションではまだイベントが送出されていません。クリック、メニュー、ショートカットはすべてここを通ります。アプリを操作してみてください。
privacy-inspect-summary = 直近{ $count }件のイベントを新しい順に表示しています。

# カレンダー / DateEdit / TimeEdit / DateTimeEdit。
# crates/teksilo-widgets/src/{calendar,date_edit,time_edit,date_time_edit}.rs
# と crates/teksilo-widgets/src/common/datetime/ 配下の共通モジュールを参照。
# 月名・曜日名は CLDR の ja に従う（月名は long と short が同一、曜日は
# short と narrow が同一）。
calendar-month-long-january = 1月
calendar-month-long-february = 2月
calendar-month-long-march = 3月
calendar-month-long-april = 4月
calendar-month-long-may = 5月
calendar-month-long-june = 6月
calendar-month-long-july = 7月
calendar-month-long-august = 8月
calendar-month-long-september = 9月
calendar-month-long-october = 10月
calendar-month-long-november = 11月
calendar-month-long-december = 12月

calendar-month-short-january = 1月
calendar-month-short-february = 2月
calendar-month-short-march = 3月
calendar-month-short-april = 4月
calendar-month-short-may = 5月
calendar-month-short-june = 6月
calendar-month-short-july = 7月
calendar-month-short-august = 8月
calendar-month-short-september = 9月
calendar-month-short-october = 10月
calendar-month-short-november = 11月
calendar-month-short-december = 12月

calendar-weekday-long-monday = 月曜日
calendar-weekday-long-tuesday = 火曜日
calendar-weekday-long-wednesday = 水曜日
calendar-weekday-long-thursday = 木曜日
calendar-weekday-long-friday = 金曜日
calendar-weekday-long-saturday = 土曜日
calendar-weekday-long-sunday = 日曜日

calendar-weekday-short-monday = 月
calendar-weekday-short-tuesday = 火
calendar-weekday-short-wednesday = 水
calendar-weekday-short-thursday = 木
calendar-weekday-short-friday = 金
calendar-weekday-short-saturday = 土
calendar-weekday-short-sunday = 日

calendar-weekday-narrow-monday = 月
calendar-weekday-narrow-tuesday = 火
calendar-weekday-narrow-wednesday = 水
calendar-weekday-narrow-thursday = 木
calendar-weekday-narrow-friday = 金
calendar-weekday-narrow-saturday = 土
calendar-weekday-narrow-sunday = 日

calendar-button-previous-month = 前の月
calendar-button-next-month = 次の月
calendar-button-previous-year = 前の年
calendar-button-next-year = 次の年
calendar-button-today = 今日
calendar-button-month-picker = 月を選択
calendar-button-year-picker = 年を選択
calendar-week-number-column = 週
calendar-name = カレンダー
calendar-months-grid-label = 月の一覧
calendar-years-grid-label = 年の一覧
calendar-name-with-month = カレンダー、{ $year }年{ $month }
calendar-cell-name = { $year }年{ $month }{ $day }日 { $weekday }
calendar-range-status = 選択：{ $start } – { $end }

date-edit-segment-year = 年
date-edit-segment-month = 月
date-edit-segment-day = 日
date-edit-calendar-button = 日付を選択
date-edit-trigger-tooltip = カレンダーを開く
date-edit-name = 日付
date-edit-placeholder = 日付を選択

time-edit-segment-hour = 時
time-edit-segment-minute = 分
time-edit-segment-second = 秒
time-edit-segment-period = 午前/午後
time-edit-period-am = 午前
time-edit-period-pm = 午後
time-edit-name = 時刻
time-edit-placeholder = 時刻を選択

date-time-edit-name = 日付と時刻
date-time-edit-placeholder = 日付と時刻を選択
date-time-edit-date-name = 日付
date-time-edit-time-name = 時刻
date-time-edit-trigger-tooltip = カレンダーを開く
date-range-edit-name = 日付範囲
date-range-edit-placeholder = 日付範囲を選択
date-range-edit-start-name = 開始日
date-range-edit-end-name = 終了日
date-range-edit-trigger-tooltip = 範囲選択カレンダーを開く

# 入力の検証フィードバック（TextInputField + DateEdit/TimeEdit）
validation-corrected-to = { $value } に自動修正しました
validation-corrected-with-notes = 自動修正：{ $notes }
validation-segment-clamped = { $segment } { $raw } → { $clamped }
validation-day-clamped-to-month = 日 { $raw } → { $clamped }（その月の最終日）
validation-clamped-to-range = 許容範囲に丸めました
validation-segment-year = 年
validation-segment-month = 月
validation-segment-day = 日
validation-segment-hour = 時
validation-segment-minute = 分
validation-segment-second = 秒
validation-segment-value = 値
date-edit-validation-not-a-date = 無効な日付です
time-edit-validation-not-a-time = 無効な時刻です

# ── カラーピッカー ──
color-picker-name = カラーピッカー
color-picker-hue-label = 色相
color-picker-saturation-label = 彩度
color-picker-value-label = 明度
color-picker-alpha-label = 不透明度
color-picker-red-label = 赤
color-picker-green-label = 緑
color-picker-blue-label = 青
color-picker-red-short = R
color-picker-green-short = G
color-picker-blue-short = B
color-picker-alpha-short = A
color-picker-hue-short = H
color-picker-saturation-short = S
color-picker-value-short = V
color-picker-hex-label = HEX
color-picker-hex-placeholder = #RRGGBB
color-picker-current-color-label = 選択中の色
color-picker-current-color-readout = 選択中の色 { $hex }
color-picker-swatches-name = 定義済みの色
color-picker-swatch-label = 色見本 { $hex }
color-picker-swatch-selected-suffix = 、選択中
color-picker-changed-announcement = 色を { $hex } に変更しました
color-picker-done-label = 完了
color-picker-cancel-label = キャンセル
color-edit-trigger-name = 色 { $hex }
color-edit-trigger-name-empty = 色、なし
color-edit-trigger-tooltip = カラーピッカーを開く
hex-color-input-invalid = 無効な16進カラーコードです（#RRGGBB 形式）
hex-color-input-invalid-with-alpha = 無効な16進カラーコードです（#RRGGBB または #RRGGBBAA 形式）
hex-color-input-corrected-shortform = { $raw } を { $value } に展開しました
hex-color-input-corrected-uppercase = { $value } に正規化しました
hex-color-input-placeholder = #RRGGBB
hex-color-input-placeholder-with-alpha = #RRGGBBAA
color-edit-trigger-empty-placeholder = —

# リッチツールチップの「詳細」開示ラベル（固定表示されたリッチツールチップ内で
# 詳細本文を表示するアコーディオンのタイトル）。
tooltip-more = 詳細

# テキストフィールドとリッチテキストエディターの組み込みコンテキストメニュー項目。
menu-cut = 切り取り
menu-copy = コピー
menu-paste = 貼り付け
menu-paste-unformatted = 書式なしで貼り付け
menu-select-all = すべて選択
menu-toggle-blockquote = 引用ブロックの切り替え
menu-remove-blockquote = 引用ブロックを解除

# DropZone のライブリージョン読み上げ（スクリーンリーダー）。単数・複数の
# 選択は Fluent ではなく Rust 側で行う。日本語には文法的な数がないため
# -one と -many は同じ言い回しになる。詳しい経緯は en-US.ftl を参照。
drop-zone-hover-file-one = ドロップして1個のファイルを追加
drop-zone-hover-file-many = ドロップして{ $count }個のファイルを追加
drop-zone-hover-text = ドロップしてテキストを追加
drop-zone-hover-link-one = ドロップして1件のリンクを追加
drop-zone-hover-link-many = ドロップして{ $count }件のリンクを追加
drop-zone-hover-generic = ここにドロップ
drop-zone-hover-reject = この項目はここにドロップできません
drop-zone-added-file-one = 1個のファイルを追加しました
drop-zone-added-file-many = { $count }個のファイルを追加しました
drop-zone-added-text = テキストを追加しました
drop-zone-added-link-one = 1件のリンクを追加しました
drop-zone-added-link-many = { $count }件のリンクを追加しました
drop-zone-rejected = 項目は受け付けられませんでした

# ThemeSwitcher ウィジェット。crates/teksilo-widgets/src/theme_switcher.rs を参照。
theme-switcher-label = テーマ
theme-switcher-light = ライト
theme-switcher-dark = ダーク
theme-switcher-system = システム

# FontPicker ウィジェット。crates/teksilo-widgets/src/font_picker.rs を参照。
font-picker-label = フォント
font-picker-placeholder = フォントを選択…

# 設定の書き込み失敗トースト。完全な文脈は en-US.ftl を参照
# （ToastRegistry::show_settings_write_failed が teksilo::install_toast
# 経由で発行）。実際のデータ損失を伝えるため、深刻度は Error で自動消去なし。
settings-write-failed-toast-title = 設定を保存できませんでした
settings-write-failed-toast-body = { $file } の保存を{ $attempts }回試みましたが失敗し、待機中の変更{ $dropped }件を破棄しました。{ $message }

# 代替のウィンドウメニュー。OS がウィンドウメニューを提供しない環境（X11）で
# カスタム TitleBar を右クリックすると開く。完全な文脈は en-US.ftl と
# crates/teksilo-widgets/src/title_bar/window_menu.rs を参照。「元のサイズに
# 戻す」と「最大化」は排他で、常に一方のみ表示される。
window-menu-restore = 元のサイズに戻す
window-menu-maximize = 最大化
window-menu-minimize = 最小化
window-menu-close = 閉じる

# トースト本文の開示。完全な文脈は en-US.ftl と
# crates/teksilo-widgets/src/toast/body.rs を参照。
toast-show-more = もっと見る
toast-show-less = 折りたたむ
toast-copy-body = コピー
toast-body-copied = コピーしました

# コマンドパレット。完全な文脈は en-US.ftl と
# crates/teksilo-widgets/src/command_palette.rs を参照。
command-palette-placeholder = コマンドを入力
command-palette-empty = 一致するコマンドがありません
# パレットのダイアログと検索フィールドのアクセシブル名。画面には表示されない
# ため、スクリーンリーダー利用者に何が開いたかを伝える唯一の手掛かりとなる。
command-palette-title = コマンドパレット
# ダイアログの説明として読み上げられ、絞り込みのたびに再読み上げされる。
# 日本語の CLDR 基数複数カテゴリは other のみ。
command-palette-result-count =
    { $count ->
        [0] 一致するコマンドがありません
       *[other] { $count }件のコマンド
    }
