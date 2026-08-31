# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

# teksilo-widgets çerçeve metinleri — Türkçe çeviri.
#
# Yalnızca çalışma zamanında geçerlidir: bu yerel ayarı
# `I18nConfig::framework_locales(teksilo_widgets::framework_locales())`
# ile kaydeden uygulamalar bu çevirileri en-US ile birlikte alır.
# tr-TR'de bulunmayan anahtarlar, `I18nManager::resolve_widget`in elle
# kurulmuş yedekleme zinciri üzerinden en-US kaynağına döner (uygulama
# geçersiz kılması etkin → çerçeve etkin → uygulama geçersiz kılma
# kaynağı → çerçeve kaynağı → anahtar yer tutucusu). Bu, teksilo-i18n'in
# kendi yedeklemesidir; `fluent-bundle`in anahtar başına yerleşik
# yedeklemesi değildir — her `FluentBundle` zincirinde tek bir yerel ayarla
# oluşturulur ve çok yerel ayarlı arama `I18nManager` katmanında yapılır.

a11y-status-bar-name = Durum
a11y-dialog-name = İletişim kutusu
a11y-tooltip-name = Araç ipucu
a11y-snackbar-name = Bildirim
a11y-splitter-divider-name = Bölme ayırıcısı
a11y-splitter-pane = Bölme
a11y-splitter-collapsed = Daraltılmış
a11y-splitter-expanded = Genişletilmiş
a11y-breadcrumb-current-page-value = geçerli sayfa
a11y-toolbar-name = Araç çubuğu
toolbar-more = Daha fazla
segmented-control-more = Diğer seçenekler
breadcrumb-overflow = Gizli yolu göster
a11y-title-bar-name = Pencere başlık çubuğu
a11y-window-controls-name = Pencere denetimleri
a11y-window-minimize-name = Küçült
a11y-window-maximize-name = Büyült
a11y-window-restore-name = Geri yükle
a11y-window-close-name = Kapat
a11y-stepper-indicator-strip-name = Adımlar
a11y-stepper-content-name = Adım içeriği
tab-close-tooltip = Sekmeyi kapat
a11y-builtin-browse = Gözat
a11y-builtin-expand = Genişlet
a11y-builtin-search = Ara
a11y-builtin-copy = Kopyala
a11y-builtin-clear = Temizle
a11y-builtin-add = Ekle
a11y-builtin-bell = Bildirimler
a11y-builtin-menu = Menü
a11y-builtin-more = Diğer eylemler
a11y-builtin-visibility = Görünürlüğü aç/kapat
a11y-password-reveal = Parola görünürlüğünü aç/kapat
a11y-caps-lock-on = Caps Lock açık
notifications-title = Bildirimler
notifications-empty = Bildirim yok
notifications-mark-all-read = Tümünü okundu olarak işaretle
notifications-clear = Tümünü temizle
notifications-filter-placeholder = Bildirimlerde ara
notifications-bucket-today = Bugün
notifications-bucket-yesterday = Dün
notifications-bucket-this-week = Bu hafta
notifications-bucket-earlier = Daha eski
notifications-archive-replay-disabled = (artık kullanılamıyor)
a11y-shortcut-settings-name = Kısayol ayarları
a11y-shortcut-settings-capture-hint = Bir tuşa basın. Temizlemek için Del, iptal için Esc.
keystroke-modifier-ctrl = Ctrl
keystroke-modifier-shift = Shift
keystroke-modifier-alt = Alt
keystroke-modifier-super = Super
keystroke-separator = +
keystroke-key-space = Boşluk
keystroke-key-enter = Enter
keystroke-key-escape = Esc
keystroke-key-tab = Sekme
keystroke-key-backspace = Backspace
keystroke-key-delete = Del
keystroke-key-arrow-up = Yukarı
keystroke-key-arrow-down = Aşağı
keystroke-key-arrow-left = Sol
keystroke-key-arrow-right = Sağ
keystroke-key-home = Home
keystroke-key-end = End
keystroke-key-page-up = PgUp
keystroke-key-page-down = PgDn

# MessageBox — standart düğmeler ve ayrıntı açma.
messagebox-btn-ok = Tamam
messagebox-btn-cancel = İptal
messagebox-btn-close = Kapat
messagebox-btn-yes = Evet
messagebox-btn-no = Hayır
messagebox-btn-yes-to-all = Tümüne Evet
messagebox-btn-no-to-all = Tümüne Hayır
messagebox-btn-save = Kaydet
messagebox-btn-save-all = Tümünü Kaydet
messagebox-btn-discard = Gözden Çıkar
messagebox-btn-apply = Uygula
messagebox-btn-reset = Sıfırla
messagebox-btn-restore-defaults = Varsayılanları Geri Yükle
messagebox-btn-abort = Durdur
messagebox-btn-retry = Yeniden Dene
messagebox-btn-ignore = Yoksay
messagebox-btn-open = Aç
messagebox-btn-help = Yardım
messagebox-show-details = Ayrıntıları göster

# PrivacySettings widget'ı. Bkz. crates/teksilo-widgets/src/privacy_settings.rs.
# KVKK/GDPR Md. 13 aydınlatma metni ve eylem düğmeleri. Parametreli
# anahtarlar Fluent { $ad } sözdizimini kullanır.
privacy-not-configured = Bu uygulama için telemetri yapılandırılmamış.
privacy-a11y-group-name = Gizlilik ve telemetri ayarları
privacy-heading = Gizlilik ve Telemetri
privacy-notice-controller = Veriler { $processor } tarafından işlenir; teknik veri işleyen ise { $adapter } (uç nokta: { $endpoint }).
privacy-notice-purposes = Amaçlar: uygulamayı geliştirmek — hangi özelliklerin kullanıldığı, hataların nerede yoğunlaştığı, hangi platformlarda çalıştığımız. Belge içeriği, pano, tuş vuruşu veya ekran görüntüsü toplanmaz.
privacy-notice-lawful-anonymous = Hukuki sebep: ürünü geliştirmeye yönelik meşru menfaatimiz (GDPR Md. 6(1)(f); CNIL'in kitle ölçümü muafiyeti).
privacy-notice-lawful-pseudonymous = Hukuki sebep: açık rızanız (GDPR Md. 6(1)(a)).
privacy-notice-retention = Saklama: sunucu tarafındaki veriler en fazla { $days } gün saklanır.
privacy-notice-withdrawal-right = Geri çekme hakkı: aşağıdaki anahtarları istediğiniz zaman kapatabilir, tüm toplamayı durdurmak için “Rızayı geri çek” düğmesine tıklayabilir ya da takma adlı modda kayıtları sunucudan silmek için “Verilerimi sil” düğmesini kullanabilirsiniz.
privacy-notice-policy-link = Gizlilik politikasının tamamı: { $url }

privacy-scope-section-heading = Uygulama neleri paylaşabilir?
privacy-scope-anonymous-metrics-label = Anonim kullanım ölçümleri
privacy-scope-anonymous-metrics-description = Hangi düğmelerin / menü ögelerinin / kısayolların kullanıldığının sayımı, ayrıca uygulama sürümü ve işletim sistemi.
privacy-scope-crash-reports-label = Çökme raporları
privacy-scope-crash-reports-description = Uygulama çöktüğünde yığın izleri ve süreç üstverileri. Belge içeriği yok, dosya yolu yok.
privacy-scope-feature-flags-label = Özellik bayrakları
privacy-scope-feature-flags-description = Uygulamanın özellik bayrağı güncellemelerini almasını sağlar (örneğin yeni araçların kademeli dağıtımı).

privacy-btn-reject-all = Tümünü reddet
privacy-btn-accept-all = Tümünü kabul et
privacy-btn-erase = Verilerimi sil
privacy-btn-erase-tooltip = Bu kurulum için kaydedilmiş tüm olayların silinmesini sunucudan ister, ardından rızayı yerel olarak geri çeker.
privacy-btn-fetch = Verilerimi getir
privacy-btn-fetch-tooltip = Sunucunun, kurulum kimliğiniz altında kaydettiği tüm olayları getirir. Sonucu JSON olarak kaydedebilirsiniz.
privacy-btn-withdraw = Rızayı geri çek
privacy-btn-withdraw-tooltip = Yeni veri toplamayı durdurur. Sunucuda kayıtlı veriler korunur — silinmesini istiyorsanız önce “Verilerimi sil” düğmesini kullanın.
privacy-btn-switch-to-anonymous = Anonim moda geç
privacy-btn-switch-to-pseudonymous = Takma adlı moda geç

privacy-identity-heading = Sunucudaki verileriniz
privacy-identity-install-id = Kurulum kimliği: { $id }
privacy-identity-retention = Sunucu, kayıtlarınızı en fazla { $days } gün saklar.

privacy-mode-heading = Gizlilik modu
privacy-mode-current-anonymous = Şu an: Anonim (kurulum kimliği yok)
privacy-mode-current-pseudonymous = Şu an: Takma adlı (kurulum kimliği var)
privacy-mode-blurb-anonymous = Anonim mod, cihaz başına hiçbir tanımlayıcı iletmez. Geçiş yapmak, sunucudaki mevcut kayıtlarınızı silecek ve yerel kurulum UUID'sini kaldıracaktır — bu işlem geri alınamaz.
privacy-mode-blurb-pseudonymous = Takma adlı mod, rastgele bir kurulum UUID'si üretir. Sunucudaki kayıtlarınızı getirebilir ya da silebilirsiniz. Açık rıza gerektirir ve geçişte yeniden sorulur.

privacy-confirm-mode-switch-title = Gizlilik modu değiştirilsin mi?
privacy-confirm-mode-switch-leaving-pseudonymous = Bu işlem, kurulum kimliğiniz altında kaydedilmiş tüm olayların silinmesini sunucudan isteyecek, yerel kurulum UUID'sini kaldıracak, rıza kararınızı sıfırlayacak ve gizlilik modunu değiştirecek. Devam etmek istiyor musunuz?
privacy-confirm-mode-switch-leaving-anonymous = Bu işlem, rıza kararınızı sıfırlayacak ve gizlilik modunu değiştirecek. Yeni veri toplanmadan önce size yeniden sorulacak. Devam edilsin mi?
privacy-confirm-erase-title = Verileriniz silinsin mi?
privacy-confirm-erase-text = Bu işlem, kurulum kimliğiniz altında kaydedilmiş her olay için silme isteği gönderir, yerelde ara bellekte bekleyen her şeyi atar ve başka veri toplanmaması için rızayı geri çeker. İşlem geri alınamaz.
privacy-confirm-withdraw-title = Rıza geri çekilsin mi?
privacy-confirm-withdraw-text = Bu uygulamadan başka analiz olayı toplanmayacak. Sunucuda kayıtlı veriler korunur — bunların da silinmesini istiyorsanız geri çekmeden önce “Verilerimi sil” düğmesini kullanın.

privacy-fetch-success-title = Sunucudaki verileriniz
privacy-fetch-success-text = Bu kurulum için { $count } olay getirildi.
privacy-fetch-saved-to = Şuraya kaydedildi: { $path }
privacy-fetch-write-error = { $path } dosyası yazılamadı: { $error }
privacy-fetch-error-title = Verileriniz getirilemedi

privacy-inspect-title = Gönderilen verileri incele (ara bellekte { $count } olay)
privacy-inspect-empty = Bu oturumda henüz hiçbir olay yayımlanmadı. Uygulamayla etkileşime geçin — tıklamalar, menüler ve kısayolların tümü buradan geçer.
privacy-inspect-summary = Son { $count } olay, en yenisi başta olacak şekilde gösteriliyor.

# Takvim / DateEdit / TimeEdit / DateTimeEdit. Bkz.
# crates/teksilo-widgets/src/{calendar,date_edit,time_edit,date_time_edit}.rs
# ve crates/teksilo-widgets/src/common/datetime/ altındaki ortak modüller.
calendar-month-long-january = Ocak
calendar-month-long-february = Şubat
calendar-month-long-march = Mart
calendar-month-long-april = Nisan
calendar-month-long-may = Mayıs
calendar-month-long-june = Haziran
calendar-month-long-july = Temmuz
calendar-month-long-august = Ağustos
calendar-month-long-september = Eylül
calendar-month-long-october = Ekim
calendar-month-long-november = Kasım
calendar-month-long-december = Aralık

calendar-month-short-january = Oca
calendar-month-short-february = Şub
calendar-month-short-march = Mar
calendar-month-short-april = Nis
calendar-month-short-may = May
calendar-month-short-june = Haz
calendar-month-short-july = Tem
calendar-month-short-august = Ağu
calendar-month-short-september = Eyl
calendar-month-short-october = Eki
calendar-month-short-november = Kas
calendar-month-short-december = Ara

calendar-weekday-long-monday = Pazartesi
calendar-weekday-long-tuesday = Salı
calendar-weekday-long-wednesday = Çarşamba
calendar-weekday-long-thursday = Perşembe
calendar-weekday-long-friday = Cuma
calendar-weekday-long-saturday = Cumartesi
calendar-weekday-long-sunday = Pazar

calendar-weekday-short-monday = Pzt
calendar-weekday-short-tuesday = Sal
calendar-weekday-short-wednesday = Çar
calendar-weekday-short-thursday = Per
calendar-weekday-short-friday = Cum
calendar-weekday-short-saturday = Cmt
calendar-weekday-short-sunday = Paz

calendar-weekday-narrow-monday = P
calendar-weekday-narrow-tuesday = S
calendar-weekday-narrow-wednesday = Ç
calendar-weekday-narrow-thursday = P
calendar-weekday-narrow-friday = C
calendar-weekday-narrow-saturday = C
calendar-weekday-narrow-sunday = P

calendar-button-previous-month = Önceki ay
calendar-button-next-month = Sonraki ay
calendar-button-previous-year = Önceki yıl
calendar-button-next-year = Sonraki yıl
calendar-button-today = Bugün
calendar-button-month-picker = Ay seç
calendar-button-year-picker = Yıl seç
calendar-week-number-column = Hafta
calendar-name = Takvim
calendar-months-grid-label = Aylar
calendar-years-grid-label = Yıllar
calendar-name-with-month = Takvim, { $month } { $year }
calendar-cell-name = { $day } { $month } { $year }, { $weekday }
calendar-range-status = Seçilen: { $start } – { $end }

date-edit-segment-year = Yıl
date-edit-segment-month = Ay
date-edit-segment-day = Gün
date-edit-calendar-button = Tarih seç
date-edit-trigger-tooltip = Takvimi aç
date-edit-name = Tarih
date-edit-placeholder = Bir tarih seçin

time-edit-segment-hour = Saat
time-edit-segment-minute = Dakika
time-edit-segment-second = Saniye
time-edit-segment-period = ÖÖ/ÖS
time-edit-period-am = ÖÖ
time-edit-period-pm = ÖS
time-edit-name = Saat
time-edit-placeholder = Bir saat seçin

date-time-edit-name = Tarih ve saat
date-time-edit-placeholder = Tarih ve saat seçin
date-time-edit-date-name = Tarih
date-time-edit-time-name = Saat
date-time-edit-trigger-tooltip = Takvimi aç
date-range-edit-name = Tarih aralığı
date-range-edit-placeholder = Tarih aralığı seçin
date-range-edit-start-name = Başlangıç tarihi
date-range-edit-end-name = Bitiş tarihi
date-range-edit-trigger-tooltip = Aralık takvimini aç

# Doğrulama geri bildirimi (TextInputField + DateEdit/TimeEdit)
validation-corrected-to = Otomatik düzeltildi: { $value }
validation-corrected-with-notes = Otomatik düzeltildi: { $notes }
validation-segment-clamped = { $segment } { $raw } → { $clamped }
validation-day-clamped-to-month = gün { $raw } → { $clamped } (ayın son günü)
validation-clamped-to-range = izin verilen aralığa sınırlandı
validation-segment-year = yıl
validation-segment-month = ay
validation-segment-day = gün
validation-segment-hour = saat
validation-segment-minute = dakika
validation-segment-second = saniye
validation-segment-value = değer
date-edit-validation-not-a-date = Geçersiz tarih
time-edit-validation-not-a-time = Geçersiz saat

# ── renk seçici ──
color-picker-name = Renk seçici
color-picker-hue-label = Renk tonu
color-picker-saturation-label = Doygunluk
color-picker-value-label = Parlaklık
color-picker-alpha-label = Opaklık
color-picker-red-label = Kırmızı
color-picker-green-label = Yeşil
color-picker-blue-label = Mavi
color-picker-red-short = K
color-picker-green-short = Y
color-picker-blue-short = M
color-picker-alpha-short = A
color-picker-hue-short = T
color-picker-saturation-short = D
color-picker-value-short = P
color-picker-hex-label = Onaltılık
color-picker-hex-placeholder = #RRGGBB
color-picker-current-color-label = Seçili renk
color-picker-current-color-readout = Seçili renk { $hex }
color-picker-swatches-name = Hazır renkler
color-picker-swatch-label = Renk örneği { $hex }
color-picker-swatch-selected-suffix = , seçili
color-picker-changed-announcement = Renk { $hex } olarak değiştirildi
color-picker-done-label = Bitti
color-picker-cancel-label = İptal
color-edit-trigger-name = Renk { $hex }
color-edit-trigger-name-empty = Renk, yok
color-edit-trigger-tooltip = Renk seçiciyi aç
hex-color-input-invalid = Geçersiz onaltılık renk kodu (beklenen: #RRGGBB)
hex-color-input-invalid-with-alpha = Geçersiz onaltılık renk kodu (beklenen: #RRGGBB veya #RRGGBBAA)
hex-color-input-corrected-shortform = Genişletildi: { $raw } → { $value }
hex-color-input-corrected-uppercase = Normalleştirildi: { $value }
hex-color-input-placeholder = #RRGGBB
hex-color-input-placeholder-with-alpha = #RRGGBBAA
color-edit-trigger-empty-placeholder = —

# Zengin araç ipucundaki “daha fazla” açma etiketi (sabitlenmiş zengin araç
# ipucunun uzun gövdesini açan akordiyon başlığı).
tooltip-more = Daha fazla

# Metin alanlarının ve zengin metin düzenleyicinin yerleşik bağlam menüsü.
menu-cut = Kes
menu-copy = Kopyala
menu-paste = Yapıştır
menu-paste-unformatted = Biçimlendirmeden Yapıştır
menu-select-all = Tümünü Seç
menu-toggle-blockquote = Alıntıyı aç/kapat
menu-remove-blockquote = Alıntıyı kaldır

# DropZone — canlı bölge duyuruları (ekran okuyucular). Tekil/çoğul seçimi
# Fluent'te değil Rust tarafında yapılır. Tam bağlam için bkz. en-US.ftl ve
# crates/teksilo-widgets/src/drop_zone.rs.
drop-zone-hover-file-one = 1 dosya eklemek için bırakın
drop-zone-hover-file-many = { $count } dosya eklemek için bırakın
drop-zone-hover-text = Metin eklemek için bırakın
drop-zone-hover-link-one = 1 bağlantı eklemek için bırakın
drop-zone-hover-link-many = { $count } bağlantı eklemek için bırakın
drop-zone-hover-generic = Buraya bırakın
drop-zone-hover-reject = Bu öge buraya bırakılamaz
drop-zone-added-file-one = 1 dosya eklendi
drop-zone-added-file-many = { $count } dosya eklendi
drop-zone-added-text = Metin eklendi
drop-zone-added-link-one = 1 bağlantı eklendi
drop-zone-added-link-many = { $count } bağlantı eklendi
drop-zone-rejected = Öge kabul edilmedi

# ThemeSwitcher widget'ı. Bkz. crates/teksilo-widgets/src/theme_switcher.rs.
theme-switcher-label = Tema
theme-switcher-light = Açık
theme-switcher-dark = Koyu
theme-switcher-system = Sistem

# FontPicker widget'ı. Bkz. crates/teksilo-widgets/src/font_picker.rs.
font-picker-label = Yazı tipi
font-picker-placeholder = Bir yazı tipi seçin…

# Ayar yazma hatası bildirimi. Tam bağlam için bkz. en-US.ftl
# (ToastRegistry::show_settings_write_failed tarafından, teksilo::install_toast
# üzerinden tetiklenir).
settings-write-failed-toast-title = Ayarlar kaydedilemedi
settings-write-failed-toast-body = { $file } dosyası { $attempts } denemeden sonra kaydedilemedi; sırada bekleyen { $dropped } değişiklik atıldı. { $message }

# Yedek pencere menüsü; işletim sisteminin pencere menüsü sunmadığı yerlerde
# (X11) özel bir TitleBar üzerine sağ tıklayınca açılır. Tam bağlam için bkz.
# en-US.ftl ve crates/teksilo-widgets/src/title_bar/window_menu.rs.
window-menu-restore = Geri Yükle
window-menu-maximize = Büyült
window-menu-minimize = Küçült
window-menu-close = Kapat

# Bildirim gövdesinin açılması. Tam bağlam için bkz. en-US.ftl ve
# crates/teksilo-widgets/src/toast/body.rs.
toast-show-more = Daha fazla göster
toast-show-less = Daha az göster
toast-copy-body = Kopyala
toast-body-copied = Kopyalandı

# Komut paleti. Tam bağlam için bkz. en-US.ftl ve
# crates/teksilo-widgets/src/command_palette.rs.
command-palette-placeholder = Bir komut yazın
command-palette-empty = Eşleşen komut yok
command-palette-title = Komut paleti
command-palette-result-count =
    { $count ->
        [0] Eşleşen komut yok
        [one] 1 komut
       *[other] { $count } komut
    }
