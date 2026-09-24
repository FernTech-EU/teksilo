// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Integration tests for the locale-aware Number/DateTime formatters and
//! the `tr_signal!` macro.
//!
//! Three layers under test:
//!
//! 1. **Bundle-side path** — `bundle.set_formatter(...)` + `DATETIME()`
//!    custom function wired by `manager::configure_bundle`. Tested via
//!    real `.ftl` resources resolved through `I18nManager::resolve_app`.
//! 2. **Signal-side path** — `NumberFormatter` / `TeksiloDateTimeFormatter`
//!    public types producing `Signal<String>` from `Signal<T>` + locale
//!    signal.
//! 3. **`tr_signal!` macro** — the reactive variant of `tr!` for
//!    `Signal<T>`-inside-translated-sentence patterns.

use std::rc::Rc;

use teksilo_core::signal::Signal;
use teksilo_i18n::format::{
    CalendarSystem, DateFields, DateStyle, NumberFormatter, TeksiloDateTime,
    TeksiloDateTimeFormatter,
};
use teksilo_i18n::{I18nConfig, I18nManager, LanguageIdentifier, tr_signal};

fn lid(s: &str) -> LanguageIdentifier {
    s.parse().unwrap()
}

/// Install a manager with hardcoded en-US + fr-FR `.ftl` resources so we
/// can flip between locales and observe the formatter output. The two
/// resources include identical message keys but different surrounding
/// text; the *formatted* number/date pieces inside `NUMBER()` and
/// `DATETIME()` come from CLDR via ICU regardless of the locale's `.ftl`
/// content.
fn install_en_fr() -> Rc<I18nManager> {
    teksilo_i18n::thread_local::clear();
    let cfg = I18nConfig::test_only(
        "en-US",
        &[
            ("price-display", "The price is { NUMBER($v) }"),
            (
                "cart-total",
                "Total: { NUMBER($price, style: \"currency\", currency: \"USD\") }",
            ),
            (
                "percent-done",
                "{ NUMBER($ratio, style: \"percent\") } complete",
            ),
            (
                "last-saved",
                "Last saved on { DATETIME($ts, dateStyle: \"long\") }",
            ),
            (
                "cart-summary",
                "{ $count } items at { NUMBER($price) } each",
            ),
        ],
    )
    .with_locale(
        "fr-FR",
        &[
            ("price-display", "Le prix est de { NUMBER($v) }"),
            (
                "cart-total",
                "Total : { NUMBER($price, style: \"currency\", currency: \"EUR\") }",
            ),
            (
                "percent-done",
                "{ NUMBER($ratio, style: \"percent\") } terminé",
            ),
            (
                "last-saved",
                "Enregistré le { DATETIME($ts, dateStyle: \"long\") }",
            ),
            (
                "cart-summary",
                "{ $count } articles à { NUMBER($price) } chacun",
            ),
        ],
    );
    let mgr = I18nManager::from_config(&cfg);
    teksilo_i18n::thread_local::install(mgr.clone());
    mgr
}

// -----------------------------------------------------------------
// Bundle-side path
// -----------------------------------------------------------------

#[test]
fn bundle_number_uses_locale_grouping() {
    let mgr = install_en_fr();

    let en = mgr.resolve_app("price-display", &[("v", 1234.5_f64.into())]);
    assert!(
        en.contains("1,234.5"),
        "expected en-US digit grouping with comma; got `{en}`"
    );

    mgr.set_locale(lid("fr-FR"));
    let fr = mgr.resolve_app("price-display", &[("v", 1234.5_f64.into())]);
    // fr-FR uses U+202F NARROW NO-BREAK SPACE as group separator and
    // `,` as decimal separator. Don't pin the exact byte sequence;
    // assert on the locale-distinguishing markers instead.
    assert!(
        fr.contains("1") && fr.contains(",5") && !fr.contains("1,234"),
        "expected fr-FR-style grouping/decimal; got `{fr}`"
    );

    teksilo_i18n::thread_local::clear();
}

#[test]
fn bundle_currency_renders_the_locale_symbol_not_the_iso_code() {
    // Backed by ICU's `CurrencyFormatter`: the short symbol, positioned
    // where the locale puts it, with the currency's own CLDR precision.
    let mgr = install_en_fr();
    let s = mgr.resolve_app("cart-total", &[("price", 42.5_f64.into())]);
    assert!(
        s.contains("$42.50"),
        "expected the USD symbol and 2 fraction digits in en-US; got `{s}`"
    );
    assert!(
        !s.contains("USD"),
        "expected the symbol, not the ISO code; got `{s}`"
    );
    teksilo_i18n::thread_local::clear();
}

#[test]
fn bundle_currency_affix_follows_the_locale() {
    // fr-FR writes the euro sign *after* the amount, with a no-break
    // space — the case the old ISO-code suffix could not express.
    let mgr = install_en_fr();
    mgr.set_locale(lid("fr-FR"));
    let s = mgr.resolve_app("cart-total", &[("price", 42.5_f64.into())]);
    assert!(
        s.contains('€'),
        "expected the euro sign in fr-FR; got `{s}`"
    );
    assert!(
        s.trim_end().ends_with('€'),
        "expected a trailing euro sign in fr-FR; got `{s}`"
    );
    teksilo_i18n::thread_local::clear();
}

#[test]
fn bundle_currency_uses_the_currencys_own_precision() {
    // JPY has zero CLDR fraction digits, so ICU rounds rather than
    // padding to two the way a hardcoded suffix would.
    teksilo_i18n::thread_local::clear();
    let cfg = I18nConfig::test_only(
        "en-US",
        &[(
            "yen",
            "{ NUMBER($v, style: \"currency\", currency: \"JPY\") }",
        )],
    );
    let mgr = Rc::new(I18nManager::from_config(&cfg));
    teksilo_i18n::thread_local::install(I18nManager::from_config(&cfg));
    let s = mgr.resolve_app("yen", &[("v", 1234.5_f64.into())]);
    assert!(
        s.contains("1,235") && !s.contains(".50"),
        "expected JPY rounded to whole yen; got `{s}`"
    );
    teksilo_i18n::thread_local::clear();
}

#[test]
fn bundle_percent_renders_the_locale_percent_form() {
    let mgr = install_en_fr();
    let s = mgr.resolve_app("percent-done", &[("ratio", 0.125_f64.into())]);
    // Percent multiplies by 100; ICU renders the sign.
    assert!(s.contains("12.5%"), "expected `12.5%` (en-US); got `{s}`");
    teksilo_i18n::thread_local::clear();
}

#[test]
fn bundle_percent_sign_placement_follows_the_locale() {
    // tr-TR is the canonical prefix-percent locale — the case an
    // unconditional ASCII `%` suffix got wrong.
    teksilo_i18n::thread_local::clear();
    let cfg = I18nConfig::test_only("tr-TR", &[("pct", "{ NUMBER($v, style: \"percent\") }")]);
    let mgr = Rc::new(I18nManager::from_config(&cfg));
    teksilo_i18n::thread_local::install(I18nManager::from_config(&cfg));
    let s = mgr.resolve_app("pct", &[("v", 0.125_f64.into())]);
    assert!(
        s.trim().starts_with('%'),
        "expected a leading percent sign in tr-TR; got `{s}`"
    );
    teksilo_i18n::thread_local::clear();
}

#[test]
fn bundle_datetime_via_teksilo_datetime_arg() {
    let mgr = install_en_fr();
    let dt = jiff::civil::date(2026, 5, 4).at(14, 35, 0, 0);
    let fdt: fluent_bundle::FluentValue<'static> = TeksiloDateTime::from(dt).into();

    let en = mgr.resolve_app("last-saved", &[("ts", fdt.clone())]);
    // Long en-US date contains the year and month name.
    assert!(en.contains("2026"), "expected year in `{en}`");
    assert!(
        en.contains("May") || en.contains("may"),
        "expected en-US month name in `{en}`"
    );

    mgr.set_locale(lid("fr-FR"));
    let fr = mgr.resolve_app("last-saved", &[("ts", fdt)]);
    assert!(fr.contains("2026"), "expected year in `{fr}`");
    // fr-FR Long date uses the lowercased month name "mai".
    assert!(fr.contains("mai"), "expected French month `mai` in `{fr}`");

    teksilo_i18n::thread_local::clear();
}

// -----------------------------------------------------------------
// Signal-side path
// -----------------------------------------------------------------

#[test]
fn number_formatter_signal_reacts_to_locale_change() {
    let mgr = install_en_fr();
    let value = Signal::new(1234.56_f64);
    let display = NumberFormatter::new().format(value.clone());

    let en = display.get();
    assert!(en.contains("1,234.56"), "expected en-US output; got `{en}`");

    mgr.set_locale(lid("fr-FR"));
    let fr = display.get();
    assert!(
        !fr.contains("1,234"),
        "expected fr-FR-style output, not en-US; got `{fr}`"
    );
    assert!(
        fr.contains(",56"),
        "expected fr-FR decimal comma; got `{fr}`"
    );

    teksilo_i18n::thread_local::clear();
}

#[test]
fn number_formatter_signal_reacts_to_value_change() {
    let _mgr = install_en_fr();
    let value = Signal::new(100.0_f64);
    let display = NumberFormatter::new()
        .fraction_digits(2, 2)
        .format(value.clone());

    assert_eq!(display.get(), "100.00");
    value.set(2_500.5);
    assert!(
        display.get().contains("2,500.50"),
        "expected `2,500.50`; got `{}`",
        display.get()
    );

    teksilo_i18n::thread_local::clear();
}

#[test]
fn number_formatter_static_value_still_renders() {
    // Prop::Static path: no value-signal subscription, but the locale
    // signal still drives re-renders when installed.
    let _mgr = install_en_fr();
    let display = NumberFormatter::new().format(987_654.321_f64);
    assert!(
        display.get().contains("987,654"),
        "expected en-US grouping on static value; got `{}`",
        display.get()
    );
    teksilo_i18n::thread_local::clear();
}

#[test]
fn datetime_formatter_signal_reacts_to_locale_change() {
    let mgr = install_en_fr();
    let dt = Signal::new(jiff::civil::date(2026, 5, 4).at(14, 35, 0, 0));
    let display = TeksiloDateTimeFormatter::new()
        .date_style(teksilo_i18n::DateStyle::Long)
        .format(dt);

    let en = display.get();
    assert!(en.contains("2026"));
    assert!(
        en.contains("May") || en.contains("may"),
        "expected en-US month; got `{en}`"
    );

    mgr.set_locale(lid("fr-FR"));
    let fr = display.get();
    assert!(fr.contains("mai"), "expected fr-FR month; got `{fr}`");

    teksilo_i18n::thread_local::clear();
}

#[test]
fn signal_formatter_no_manager_falls_back_gracefully() {
    teksilo_i18n::thread_local::clear();
    // No I18nManager installed — formatter falls back to the default
    // (`und`) locale and still produces a string. Just assert it
    // doesn't panic and produces a non-empty result.
    let s = NumberFormatter::new().format(42.0_f64).get();
    assert!(!s.is_empty(), "fallback output must be non-empty");
}

// -----------------------------------------------------------------
// Date fields, and the one-shot path
// -----------------------------------------------------------------

/// `date` in `lang` at `DateStyle::Long` with the given fields, through the
/// one-shot path: no manager involved.
fn long_date(lang: &str, fields: DateFields, date: jiff::civil::Date) -> String {
    TeksiloDateTimeFormatter::new()
        .date_style(DateStyle::Long)
        .date_fields(fields)
        .format_in_locale(date.at(0, 0, 0, 0), &lid(lang))
}

#[test]
fn a_date_with_its_weekday_is_written_the_way_the_locale_writes_it() {
    // The strings a screen reader speaks for a calendar day. Each language
    // puts the weekday where it goes and inflects the month the way a day
    // requires, which no concatenation of translated names can do: French
    // puts the day before the month, Hungarian the weekday last, Russian the
    // month in the genitive.
    let day = jiff::civil::date(2026, 8, 31);
    let with_weekday = DateFields::YearMonthDayWeekday;
    assert_eq!(
        long_date("en-US", with_weekday, day),
        "Monday, August 31, 2026"
    );
    assert_eq!(long_date("fr-FR", with_weekday, day), "lundi 31 août 2026");
    assert_eq!(
        long_date("hu-HU", with_weekday, day),
        "2026. augusztus 31., hétfő"
    );
    assert_eq!(
        long_date("ru-RU", with_weekday, day),
        "понедельник, 31 августа 2026\u{202f}г."
    );
}

#[test]
fn a_month_with_its_year_is_written_without_a_day() {
    // Russian shows why this is a field set and not a trimmed date: a month
    // standing with its year is nominative, "август", where the day above
    // had the genitive "августа".
    let day = jiff::civil::date(2026, 8, 31);
    assert_eq!(
        long_date("en-US", DateFields::YearMonth, day),
        "August 2026"
    );
    assert_eq!(long_date("fr-FR", DateFields::YearMonth, day), "août 2026");
    assert_eq!(
        long_date("ru-RU", DateFields::YearMonth, day),
        "август 2026\u{202f}г."
    );
}

#[test]
fn a_month_with_its_year_ignores_a_time_style() {
    // ICU refuses a time on a calendar period. Passing the refusal on would
    // have degraded the whole rendering to the ISO fallback.
    let s = TeksiloDateTimeFormatter::new()
        .date_style(DateStyle::Long)
        .date_fields(DateFields::YearMonth)
        .time_style(teksilo_i18n::TimeStyle::Short)
        .format_in_locale(
            jiff::civil::date(2026, 8, 31).at(14, 35, 0, 0),
            &lid("en-US"),
        );
    assert_eq!(s, "August 2026");
}

#[test]
fn a_date_style_alone_still_names_year_month_and_day() {
    // The default the existing callers rely on: no weekday unless asked.
    let day = jiff::civil::date(2026, 8, 31);
    assert_eq!(
        long_date("en-US", DateFields::default(), day),
        "August 31, 2026"
    );
    assert_eq!(
        long_date("fr-FR", DateFields::default(), day),
        "31 août 2026"
    );
}

#[test]
fn a_date_can_stay_on_the_gregorian_calendar_where_the_locale_prefers_another() {
    // CLDR gives fa-IR the Persian calendar and th-TH the Buddhist era, so
    // by default 24 September 2026 is the 2nd of Mehr 1405, and a Thai year
    // is 2569. A date naming a day of a Gregorian month grid has to stay on
    // the grid's calendar, in the locale's own words and digits.
    let day = jiff::civil::date(2026, 9, 24).at(0, 0, 0, 0);
    let full = |lang: &str, system: CalendarSystem| {
        TeksiloDateTimeFormatter::new()
            .date_style(DateStyle::Long)
            .date_fields(DateFields::YearMonthDayWeekday)
            .calendar_system(system)
            .format_in_locale(day, &lid(lang))
    };
    assert_eq!(
        full("fa-IR", CalendarSystem::LocalePreferred),
        "پنجشنبه ۲ مهر ۱۴۰۵"
    );
    assert_eq!(
        full("fa-IR", CalendarSystem::Gregorian),
        "پنجشنبه ۲۴ سپتامبر ۲۰۲۶"
    );
    assert_eq!(
        full("th-TH", CalendarSystem::LocalePreferred),
        "วันพฤหัสบดีที่ 24 กันยายน 2569"
    );
    assert_eq!(
        full("th-TH", CalendarSystem::Gregorian),
        "วันพฤหัสบดีที่ 24 กันยายน ค.ศ. 2026"
    );
    // Where the locale counts in Gregorian already, nothing changes.
    assert_eq!(
        full("fr-FR", CalendarSystem::Gregorian),
        full("fr-FR", CalendarSystem::LocalePreferred)
    );
}

#[test]
fn format_in_locale_follows_its_argument_and_agrees_with_the_signal_path() {
    // The manager is on en-US; the one-shot path is told fr-FR and must not
    // consult the manager. Once the manager moves to fr-FR, the signal path
    // must render the very same string, since both share one ICU cache.
    let mgr = install_en_fr();
    let dt = jiff::civil::date(2026, 8, 31).at(9, 5, 0, 0);
    let formatter = TeksiloDateTimeFormatter::new()
        .date_style(DateStyle::Long)
        .date_fields(DateFields::YearMonthDayWeekday);

    let one_shot = formatter.format_in_locale(dt, &lid("fr-FR"));
    assert_eq!(one_shot, "lundi 31 août 2026");

    let display = formatter.format(dt);
    assert_eq!(display.get(), "Monday, August 31, 2026");
    mgr.set_locale(lid("fr-FR"));
    assert_eq!(display.get(), one_shot);

    teksilo_i18n::thread_local::clear();
}

// -----------------------------------------------------------------
// tr_signal! macro
// -----------------------------------------------------------------

#[test]
fn tr_signal_reacts_to_arg_signal() {
    let _mgr = install_en_fr();
    let count = Signal::new(3_i64);
    let price = Signal::new(9.99_f64);

    let label = tr_signal!(cart_summary(count = count, price = price));
    let initial = label.get();
    assert!(initial.contains("3 items"), "got `{initial}`");

    count.set(5);
    let after = label.get();
    assert!(
        after.contains("5 items") && !after.contains("3 items"),
        "expected re-render after count change; got `{after}`"
    );

    teksilo_i18n::thread_local::clear();
}

#[test]
fn tr_signal_reacts_to_locale_change() {
    let mgr = install_en_fr();
    let count = Signal::new(7_i64);
    let price = Signal::new(2.5_f64);

    let label = tr_signal!(cart_summary(count = count, price = price));
    let en = label.get();
    assert!(en.contains("items"), "expected en-US wording; got `{en}`");

    mgr.set_locale(lid("fr-FR"));
    let fr = label.get();
    assert!(
        fr.contains("articles"),
        "expected French translation; got `{fr}`"
    );

    teksilo_i18n::thread_local::clear();
}

#[test]
fn tr_signal_observers_clean_up_on_drop() {
    let _mgr = install_en_fr();
    let count = Signal::new(1_i64);
    let price = Signal::new(1.0_f64);

    // Before label exists, the signals have no observers from this
    // path (other observers may exist from `install_en_fr`).
    let count_before = count.observer_count();
    let price_before = price.observer_count();

    let label = tr_signal!(cart_summary(count = count, price = price));
    let _initial = label.get();

    // After label creation, both signals must have at least one more
    // observer attached via `attach_keepalive`.
    assert!(
        count.observer_count() > count_before,
        "tr_signal! must subscribe to its arg signals"
    );
    assert!(price.observer_count() > price_before);

    drop(label);

    // Dropping the label drops its keepalive guards, which detaches
    // every observer the macro registered. Counts return to baseline.
    assert_eq!(
        count.observer_count(),
        count_before,
        "tr_signal! must release its observers on drop"
    );
    assert_eq!(price.observer_count(), price_before);

    teksilo_i18n::thread_local::clear();
}

// -----------------------------------------------------------------
// Cross-path consistency
// -----------------------------------------------------------------

#[test]
fn bundle_and_signal_paths_produce_same_number() {
    // The bundle-side `set_formatter` callback and the Signal-side
    // `NumberFormatter` both route through `IcuNumberFormatter` —
    // identical inputs must yield identical strings, otherwise mixed
    // translated/untranslated displays in the same UI would disagree
    // on separators or grouping.
    let mgr = install_en_fr();
    let value = 1234.5_f64;

    let via_bundle = mgr.resolve_app("price-display", &[("v", value.into())]);
    // The bundle output is `"The price is 1,234.5"`; extract the
    // formatted number portion by stripping the surrounding text.
    let bundle_number = via_bundle
        .strip_prefix("The price is ")
        .expect("en-US message starts with `The price is `");

    let via_signal = NumberFormatter::new().format(value).get();

    assert_eq!(
        bundle_number, via_signal,
        "bundle and Signal paths disagreed on formatting"
    );

    teksilo_i18n::thread_local::clear();
}
