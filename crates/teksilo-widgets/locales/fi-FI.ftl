# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

# teksilo-widgets-kehyksen käyttöliittymätekstit — suomenkielinen käännös.
#
# Vain ajonaikainen: sovellukset, jotka rekisteröivät tämän kielialueen
# kutsulla `I18nConfig::framework_locales(teksilo_widgets::framework_locales())`,
# saavat nämä käännökset en-US-tiedoston rinnalle. Avaimet, joita fi-FI:stä
# puuttuu, haetaan varalta en-US-lähdetiedostosta `I18nManager::resolve_widget`-
# funktion varamekanismilla (sovelluksen ohitus aktiivisena → kehys aktiivisena
# → sovelluksen ohitus lähdekielellä → kehys lähdekielellä → avaimen nimi).
# Kyseessä on teksilo-i18n:n oma varamekanismi, ei `fluent-bundle`-kirjaston
# avainkohtainen vara — jokainen `FluentBundle` rakennetaan yhdellä
# kielialueella, ja monikielinen haku hoidetaan `I18nManager`-kerroksessa.

a11y-status-bar-name = Tila
a11y-dialog-name = Valintaikkuna
a11y-tooltip-name = Työkaluvihje
a11y-snackbar-name = Ilmoitus
a11y-splitter-divider-name = Jakaja
a11y-splitter-pane = Ruutu
a11y-splitter-collapsed = Kutistettu
a11y-splitter-expanded = Laajennettu
a11y-breadcrumb-current-page-value = nykyinen sivu
a11y-toolbar-name = Työkalurivi
toolbar-more = Lisää toimintoja
segmented-control-more = Lisää vaihtoehtoja
breadcrumb-overflow = Näytä piilotettu polku
a11y-title-bar-name = Ikkunan otsikkorivi
a11y-window-controls-name = Ikkunan hallintapainikkeet
a11y-window-minimize-name = Pienennä
a11y-window-maximize-name = Suurenna
a11y-window-restore-name = Palauta
a11y-window-close-name = Sulje
a11y-stepper-indicator-strip-name = Vaiheet
a11y-stepper-content-name = Vaiheen sisältö
tab-close-tooltip = Sulje välilehti
a11y-builtin-browse = Selaa
a11y-builtin-expand = Suurenna
a11y-builtin-search = Hae
a11y-builtin-copy = Kopioi
a11y-builtin-clear = Tyhjennä
a11y-builtin-add = Lisää
a11y-builtin-bell = Ilmoitukset
a11y-builtin-menu = Valikko
a11y-builtin-more = Lisää toimintoja
a11y-builtin-visibility = Näytä tai piilota
a11y-password-reveal = Näytä tai piilota salasana
a11y-caps-lock-on = Caps Lock on käytössä
notifications-title = Ilmoitukset
notifications-empty = Ei ilmoituksia
notifications-mark-all-read = Merkitse kaikki luetuiksi
notifications-clear = Tyhjennä kaikki
notifications-filter-placeholder = Hae ilmoituksia
notifications-bucket-today = Tänään
notifications-bucket-yesterday = Eilen
notifications-bucket-this-week = Tällä viikolla
notifications-bucket-earlier = Aiemmin
notifications-archive-replay-disabled = (ei enää käytettävissä)
a11y-shortcut-settings-name = Pikanäppäinasetukset
a11y-shortcut-settings-capture-hint = Paina mitä tahansa näppäintä. Del tyhjentää. Esc peruuttaa.
keystroke-modifier-ctrl = Ctrl
keystroke-modifier-shift = Vaihto
keystroke-modifier-alt = Alt
keystroke-modifier-super = Super
keystroke-separator = +
keystroke-key-space = Välilyönti
keystroke-key-enter = Enter
keystroke-key-escape = Esc
keystroke-key-tab = Sarkain
keystroke-key-backspace = Askelpalautin
keystroke-key-delete = Del
keystroke-key-arrow-up = Ylös
keystroke-key-arrow-down = Alas
keystroke-key-arrow-left = Vasen
keystroke-key-arrow-right = Oikea
keystroke-key-home = Home
keystroke-key-end = End
keystroke-key-page-up = PgUp
keystroke-key-page-down = PgDn

# MessageBox — vakiopainikkeet ja lisätietojen näyttäminen.
messagebox-btn-ok = OK
messagebox-btn-cancel = Peruuta
messagebox-btn-close = Sulje
messagebox-btn-yes = Kyllä
messagebox-btn-no = Ei
messagebox-btn-yes-to-all = Kyllä kaikkiin
messagebox-btn-no-to-all = Ei kaikkiin
messagebox-btn-save = Tallenna
messagebox-btn-save-all = Tallenna kaikki
messagebox-btn-discard = Hylkää
messagebox-btn-apply = Käytä
messagebox-btn-reset = Palauta
messagebox-btn-restore-defaults = Palauta oletusasetukset
messagebox-btn-abort = Keskeytä
messagebox-btn-retry = Yritä uudelleen
messagebox-btn-ignore = Ohita
messagebox-btn-open = Avaa
messagebox-btn-help = Ohje
messagebox-show-details = Näytä tiedot

# PrivacySettings-osa. Katso crates/teksilo-widgets/src/privacy_settings.rs.
# Tietosuoja-asetuksen 13 artiklan mukainen informointi + toimintopainikkeet.
# Parametrilliset avaimet käyttävät Fluentin { $nimi }-syntaksia.
privacy-not-configured = Telemetriaa ei ole määritetty tälle sovellukselle.
privacy-a11y-group-name = Tietosuoja- ja telemetria-asetukset
privacy-heading = Tietosuoja ja telemetria
privacy-notice-controller = Tietoja käsittelee { $processor }; tekninen käsittelijä on { $adapter } (päätepiste: { $endpoint }).
privacy-notice-purposes = Käsittelyn tarkoitukset: sovelluksen kehittäminen — mitä toimintoja käytetään, mihin virheet keskittyvät ja millä alustoilla sovellusta suoritetaan. Ei asiakirjojen sisältöä, ei leikepöytää, ei näppäinpainalluksia, ei näyttökuvia.
privacy-notice-lawful-anonymous = Käsittelyn oikeusperuste: oikeutettu etumme tuotteen kehittämisessä (yleinen tietosuoja-asetus, 6 artiklan 1 kohdan f alakohta; CNIL:n kävijämittausta koskeva poikkeus).
privacy-notice-lawful-pseudonymous = Käsittelyn oikeusperuste: nimenomainen suostumuksesi (yleinen tietosuoja-asetus, 6 artiklan 1 kohdan a alakohta).
privacy-notice-retention = Säilytysaika: palvelimella olevia tietoja säilytetään enintään { $days } päivän ajan.
privacy-notice-withdrawal-right = Oikeus peruuttaa suostumus: voit poistaa alla olevat valinnat käytöstä milloin tahansa, lopettaa kaiken keräämisen valitsemalla ”Peruuta suostumus” tai pseudonyymitilassa poistaa tiedot palvelimelta valitsemalla ”Poista tietoni”.
privacy-notice-policy-link = Koko tietosuojaseloste: { $url }

privacy-scope-section-heading = Mitä sovellus saa jakaa?
privacy-scope-anonymous-metrics-label = Anonyymit käyttötiedot
privacy-scope-anonymous-metrics-description = Tiedot siitä, kuinka usein painikkeita, valikkokomentoja ja pikanäppäimiä käytetään, sekä sovelluksen versio ja käyttöjärjestelmä.
privacy-scope-crash-reports-label = Kaatumisraportit
privacy-scope-crash-reports-description = Kutsupinot ja prosessin metatiedot sovelluksen kaatuessa. Ei asiakirjojen sisältöä, ei tiedostopolkuja.
privacy-scope-feature-flags-label = Ominaisuusliput
privacy-scope-feature-flags-description = Sallii sovelluksen vastaanottaa ominaisuuslippujen päivityksiä (esimerkiksi uusien työkalujen vaiheittainen käyttöönotto).

privacy-btn-reject-all = Hylkää kaikki
privacy-btn-accept-all = Hyväksy kaikki
privacy-btn-erase = Poista tietoni
privacy-btn-erase-tooltip = Pyytää palvelinta poistamaan kaikki tälle asennukselle tallennetut tapahtumat ja peruuttaa sitten suostumuksen paikallisesti.
privacy-btn-fetch = Hae tietoni
privacy-btn-fetch-tooltip = Hakee kaikki tapahtumat, jotka palvelin on tallentanut asennustunnuksellasi. Voit tallentaa tuloksen JSON-muodossa.
privacy-btn-withdraw = Peruuta suostumus
privacy-btn-withdraw-tooltip = Lopettaa uusien tietojen keräämisen. Palvelimelle jo tallennetut tiedot säilytetään — valitse ensin ”Poista tietoni”, jos haluat poistaa myös ne.
privacy-btn-switch-to-anonymous = Vaihda anonyymitilaan
privacy-btn-switch-to-pseudonymous = Vaihda pseudonyymitilaan

privacy-identity-heading = Tietosi palvelimella
privacy-identity-install-id = Asennustunnus: { $id }
privacy-identity-retention = Palvelin säilyttää tietosi enintään { $days } päivän ajan.

privacy-mode-heading = Tietosuojatila
privacy-mode-current-anonymous = Nykyinen: anonyymi (ei asennustunnusta)
privacy-mode-current-pseudonymous = Nykyinen: pseudonyymi (asennustunnus käytössä)
privacy-mode-blurb-anonymous = Anonyymitila ei lähetä laitekohtaista tunnistetta. Vaihtaminen poistaa palvelimella olevat tietosi ja hävittää paikallisen asennus-UUID:n — tätä ei voi kumota.
privacy-mode-blurb-pseudonymous = Pseudonyymitila luo satunnaisen asennus-UUID:n. Voit hakea tai poistaa tietosi palvelimelta. Edellyttää nimenomaista suostumusta ja kysyy sen uudelleen tilaa vaihdettaessa.

privacy-confirm-mode-switch-title = Vaihdetaanko tietosuojatilaa?
privacy-confirm-mode-switch-leaving-pseudonymous = Tämä pyytää palvelinta poistamaan kaikki asennustunnuksellasi tallennetut tapahtumat, hävittää paikallisen asennus-UUID:n, palauttaa suostumusvalintasi alkutilaan ja vaihtaa tietosuojatilan. Haluatko jatkaa?
privacy-confirm-mode-switch-leaving-anonymous = Tämä palauttaa suostumusvalintasi alkutilaan ja vaihtaa tietosuojatilan. Sinulta kysytään uudelleen ennen kuin uusia tietoja kerätään. Jatketaanko?
privacy-confirm-erase-title = Poistetaanko tietosi?
privacy-confirm-erase-text = Tämä lähettää poistopyynnön kaikista asennustunnuksellasi tallennetuista tapahtumista, hävittää paikallisesti vielä puskurissa olevat tiedot ja peruuttaa suostumuksen, jottei uusia tietoja enää kerätä. Toimintoa ei voi kumota.
privacy-confirm-withdraw-title = Peruutetaanko suostumus?
privacy-confirm-withdraw-text = Tästä sovelluksesta ei enää kerätä analytiikkatapahtumia. Palvelimelle jo tallennetut tiedot säilytetään — valitse ”Poista tietoni” ennen suostumuksen peruuttamista, jos haluat poistaa myös ne.

privacy-fetch-success-title = Tietosi palvelimella
privacy-fetch-success-text = Tälle asennukselle haettiin tapahtumia: { $count }.
privacy-fetch-saved-to = Tallennettu sijaintiin: { $path }
privacy-fetch-write-error = Tiedostoa { $path } ei voitu kirjoittaa: { $error }
privacy-fetch-error-title = Tietojasi ei voitu hakea

privacy-inspect-title = Tarkastele lähetettyjä tietoja (tapahtumia puskurissa: { $count })
privacy-inspect-empty = Tässä istunnossa ei ole vielä lähetetty yhtään tapahtumaa. Kokeile käyttää sovellusta — napsautukset, valikot ja pikanäppäimet kulkevat kaikki tämän kautta.
privacy-inspect-summary = Näytetään viimeisimmät tapahtumat ({ $count } kpl), uusin ensin.

# Kalenteri / DateEdit / TimeEdit / DateTimeEdit. Katso
# crates/teksilo-widgets/src/{calendar,date_edit,time_edit,date_time_edit}.rs
# ja yhteiset moduulit polussa crates/teksilo-widgets/src/common/datetime/.
# Kuukausien nimet ovat CLDR:n erillismuotoja (nominatiivi), koska samaa
# avainta käytetään kalenterin otsikossa ja kuukausiruudukossa; päivämäärän
# sisällä tarvittava partitiivi muodostetaan avaimessa calendar-cell-name
# päätteellä -ta (kaikki kuukaudet päättyvät -kuu).
calendar-month-long-january = tammikuu
calendar-month-long-february = helmikuu
calendar-month-long-march = maaliskuu
calendar-month-long-april = huhtikuu
calendar-month-long-may = toukokuu
calendar-month-long-june = kesäkuu
calendar-month-long-july = heinäkuu
calendar-month-long-august = elokuu
calendar-month-long-september = syyskuu
calendar-month-long-october = lokakuu
calendar-month-long-november = marraskuu
calendar-month-long-december = joulukuu

calendar-month-short-january = tammi
calendar-month-short-february = helmi
calendar-month-short-march = maalis
calendar-month-short-april = huhti
calendar-month-short-may = touko
calendar-month-short-june = kesä
calendar-month-short-july = heinä
calendar-month-short-august = elo
calendar-month-short-september = syys
calendar-month-short-october = loka
calendar-month-short-november = marras
calendar-month-short-december = joulu

calendar-weekday-long-monday = maanantai
calendar-weekday-long-tuesday = tiistai
calendar-weekday-long-wednesday = keskiviikko
calendar-weekday-long-thursday = torstai
calendar-weekday-long-friday = perjantai
calendar-weekday-long-saturday = lauantai
calendar-weekday-long-sunday = sunnuntai

calendar-weekday-short-monday = ma
calendar-weekday-short-tuesday = ti
calendar-weekday-short-wednesday = ke
calendar-weekday-short-thursday = to
calendar-weekday-short-friday = pe
calendar-weekday-short-saturday = la
calendar-weekday-short-sunday = su

calendar-weekday-narrow-monday = M
calendar-weekday-narrow-tuesday = T
calendar-weekday-narrow-wednesday = K
calendar-weekday-narrow-thursday = T
calendar-weekday-narrow-friday = P
calendar-weekday-narrow-saturday = L
calendar-weekday-narrow-sunday = S

calendar-button-previous-month = Edellinen kuukausi
calendar-button-next-month = Seuraava kuukausi
calendar-button-previous-year = Edellinen vuosi
calendar-button-next-year = Seuraava vuosi
calendar-button-today = Tänään
calendar-button-month-picker = Valitse kuukausi
calendar-button-year-picker = Valitse vuosi
calendar-week-number-column = Vk
calendar-name = Kalenteri
calendar-months-grid-label = Kuukaudet
calendar-years-grid-label = Vuodet
calendar-name-with-month = Kalenteri, { $month } { $year }
calendar-cell-name = { $weekday } { $day }. { $month }ta { $year }
calendar-range-status = Valittu: { $start } – { $end }

date-edit-segment-year = Vuosi
date-edit-segment-month = Kuukausi
date-edit-segment-day = Päivä
date-edit-calendar-button = Valitse päivämäärä
date-edit-trigger-tooltip = Avaa kalenteri
date-edit-name = Päivämäärä
date-edit-placeholder = Valitse päivämäärä

time-edit-segment-hour = Tunti
time-edit-segment-minute = Minuutti
time-edit-segment-second = Sekunti
time-edit-segment-period = ap./ip.
time-edit-period-am = ap.
time-edit-period-pm = ip.
time-edit-name = Kellonaika
time-edit-placeholder = Valitse kellonaika

date-time-edit-name = Päivämäärä ja kellonaika
date-time-edit-placeholder = Valitse päivämäärä ja kellonaika
date-time-edit-date-name = Päivämäärä
date-time-edit-time-name = Kellonaika
date-time-edit-trigger-tooltip = Avaa kalenteri
date-range-edit-name = Päivämääräväli
date-range-edit-placeholder = Valitse päivämääräväli
date-range-edit-start-name = Alkamispäivä
date-range-edit-end-name = Päättymispäivä
date-range-edit-trigger-tooltip = Avaa päivämäärävälin kalenteri

# Kelpoisuustarkistuksen palaute (TextInputField + DateEdit/TimeEdit)
validation-corrected-to = Korjattu automaattisesti arvoksi { $value }
validation-corrected-with-notes = Korjattu automaattisesti: { $notes }
validation-segment-clamped = { $segment } { $raw } → { $clamped }
validation-day-clamped-to-month = päivä { $raw } → { $clamped } (kuukauden viimeinen päivä)
validation-clamped-to-range = rajattu sallitulle välille
validation-segment-year = vuosi
validation-segment-month = kuukausi
validation-segment-day = päivä
validation-segment-hour = tunti
validation-segment-minute = minuutti
validation-segment-second = sekunti
validation-segment-value = arvo
date-edit-validation-not-a-date = Virheellinen päivämäärä
time-edit-validation-not-a-time = Virheellinen kellonaika

# ── värivalitsin ──
color-picker-name = Värivalitsin
color-picker-hue-label = Sävy
color-picker-saturation-label = Kylläisyys
color-picker-value-label = Kirkkaus
color-picker-alpha-label = Peittävyys
color-picker-red-label = Punainen
color-picker-green-label = Vihreä
color-picker-blue-label = Sininen
color-picker-red-short = R
color-picker-green-short = G
color-picker-blue-short = B
color-picker-alpha-short = A
color-picker-hue-short = H
color-picker-saturation-short = S
color-picker-value-short = V
color-picker-hex-label = Heksa
color-picker-hex-placeholder = #RRGGBB
color-picker-current-color-label = Valittu väri
color-picker-current-color-readout = Valittu väri { $hex }
color-picker-swatches-name = Värimallit
color-picker-swatch-label = Värimalli { $hex }
color-picker-swatch-selected-suffix = , valittu
color-picker-changed-announcement = Väriksi vaihdettiin { $hex }
color-picker-done-label = Valmis
color-picker-cancel-label = Peruuta
color-edit-trigger-name = Väri { $hex }
color-edit-trigger-name-empty = Väri, ei valintaa
color-edit-trigger-tooltip = Avaa värivalitsin
hex-color-input-invalid = Virheellinen heksaväri (odotettiin muotoa #RRGGBB)
hex-color-input-invalid-with-alpha = Virheellinen heksaväri (odotettiin muotoa #RRGGBB tai #RRGGBBAA)
hex-color-input-corrected-shortform = { $raw } laajennettiin muotoon { $value }
hex-color-input-corrected-uppercase = Normalisoitiin muotoon { $value }
hex-color-input-placeholder = #RRGGBB
hex-color-input-placeholder-with-alpha = #RRGGBBAA
color-edit-trigger-empty-placeholder = —

# Rikkaan työkaluvihjeen ”lisää”-otsikko (haitarin otsikko, joka paljastaa
# pitkän tekstiosan kiinnitetyssä työkaluvihjeessä).
tooltip-more = Lisätietoja

# Tekstikenttien ja rikkaan tekstin muokkaimen sisäänrakennetut
# pikavalikkokomennot.
menu-cut = Leikkaa
menu-copy = Kopioi
menu-paste = Liitä
menu-paste-unformatted = Liitä muotoilemattomana
menu-select-all = Valitse kaikki
menu-toggle-blockquote = Lisää lainauslohko
menu-remove-blockquote = Poista lainauslohko

# DropZone — live-alueen kuulutukset ruudunlukijoille. Yksikkö ja monikko
# valitaan Rust-koodissa, ei Fluentin select-lausekkeella. Katso en-US.ftl
# koko konteksti ja crates/teksilo-widgets/src/drop_zone.rs.
drop-zone-hover-file-one = Lisää 1 tiedosto pudottamalla
drop-zone-hover-file-many = Lisää { $count } tiedostoa pudottamalla
drop-zone-hover-text = Lisää teksti pudottamalla
drop-zone-hover-link-one = Lisää 1 linkki pudottamalla
drop-zone-hover-link-many = Lisää { $count } linkkiä pudottamalla
drop-zone-hover-generic = Pudota tähän
drop-zone-hover-reject = Tätä kohdetta ei voi pudottaa tähän
drop-zone-added-file-one = Lisättiin 1 tiedosto
drop-zone-added-file-many = Lisättiin { $count } tiedostoa
drop-zone-added-text = Teksti lisättiin
drop-zone-added-link-one = Lisättiin 1 linkki
drop-zone-added-link-many = Lisättiin { $count } linkkiä
drop-zone-rejected = Kohdetta ei hyväksytty

# ThemeSwitcher-osa. Katso crates/teksilo-widgets/src/theme_switcher.rs.
theme-switcher-label = Teema
theme-switcher-light = Vaalea
theme-switcher-dark = Tumma
theme-switcher-system = Järjestelmä

# FontPicker-osa. Katso crates/teksilo-widgets/src/font_picker.rs.
font-picker-label = Fontti
font-picker-placeholder = Valitse fontti…

# Asetusten tallennusvirheen ilmoitus. Katso en-US.ftl koko konteksti
# (laukaisee ToastRegistry::show_settings_write_failed teksilo::install_toast
# -koukun kautta).
settings-write-failed-toast-title = Asetuksia ei voitu tallentaa
settings-write-failed-toast-body = Kohteen { $file } tallennus epäonnistui { $attempts } yrityksen jälkeen; jonossa olleita muutoksia hylättiin: { $dropped }. { $message }

# Varajärjestelmän ikkunavalikko, joka avautuu napsauttamalla mukautettua
# TitleBar-palkkia hiiren oikealla painikkeella alustoilla, joilla
# käyttöjärjestelmä ei tarjoa omaa ikkunavalikkoa (X11). Katso en-US.ftl koko
# konteksti ja crates/teksilo-widgets/src/title_bar/window_menu.rs.
window-menu-restore = Palauta
window-menu-maximize = Suurenna
window-menu-minimize = Pienennä
window-menu-close = Sulje

# Ilmoituksen tekstin laajennus. Katso en-US.ftl koko konteksti ja
# crates/teksilo-widgets/src/toast/body.rs.
toast-show-more = Näytä lisää
toast-show-less = Näytä vähemmän
toast-copy-body = Kopioi
toast-body-copied = Kopioitu

# Komentopaletti. Katso en-US.ftl koko konteksti ja
# crates/teksilo-widgets/src/command_palette.rs.
command-palette-placeholder = Kirjoita komento
command-palette-empty = Ei vastaavia komentoja
command-palette-title = Komentopaletti
command-palette-result-count =
    { $count ->
        [0] Ei vastaavia komentoja
        [one] 1 komento
       *[other] { $count } komentoa
    }
