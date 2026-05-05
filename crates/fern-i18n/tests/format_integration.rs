//! Integration tests for the locale-aware Number/DateTime formatters and
//! the `tr_signal!` macro.
//!
//! Three layers under test:
//!
//! 1. **Bundle-side path** — `bundle.set_formatter(...)` + `DATETIME()`
//!    custom function wired by `manager::configure_bundle`. Tested via
//!    real `.ftl` resources resolved through `I18nManager::resolve_app`.
//! 2. **Signal-side path** — `NumberFormatter` / `FernDateTimeFormatter`
//!    public types producing `Signal<String>` from `Signal<T>` + locale
//!    signal.
//! 3. **`tr_signal!` macro** — the reactive variant of `tr!` for
//!    `Signal<T>`-inside-translated-sentence patterns.

use std::rc::Rc;

use fern_core::signal::Signal;
use fern_i18n::format::{FernDateTime, FernDateTimeFormatter, NumberFormatter};
use fern_i18n::{I18nConfig, I18nManager, LanguageIdentifier, tr_signal};

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
    fern_i18n::thread_local::clear();
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
    fern_i18n::thread_local::install(mgr.clone());
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

    fern_i18n::thread_local::clear();
}

#[test]
fn bundle_currency_appends_iso_code() {
    // Currency is locale-naive in this implementation: it formats the
    // value with locale grouping and appends the ISO-4217 code.
    let mgr = install_en_fr();
    let s = mgr.resolve_app("cart-total", &[("price", 42.5_f64.into())]);
    assert!(s.contains("USD"), "expected USD suffix in en-US; got `{s}`");
    fern_i18n::thread_local::clear();
}

#[test]
fn bundle_percent_appends_percent_sign() {
    let mgr = install_en_fr();
    let s = mgr.resolve_app("percent-done", &[("ratio", 0.125_f64.into())]);
    // Percent multiplies by 100 and appends `%`.
    assert!(s.contains("12.5%"), "expected `12.5%` (en-US); got `{s}`");
    fern_i18n::thread_local::clear();
}

#[test]
fn bundle_datetime_via_fern_datetime_arg() {
    let mgr = install_en_fr();
    let dt = jiff::civil::date(2026, 5, 4).at(14, 35, 0, 0);
    let fdt: fluent_bundle::FluentValue<'static> = FernDateTime::from(dt).into();

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

    fern_i18n::thread_local::clear();
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

    fern_i18n::thread_local::clear();
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

    fern_i18n::thread_local::clear();
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
    fern_i18n::thread_local::clear();
}

#[test]
fn datetime_formatter_signal_reacts_to_locale_change() {
    let mgr = install_en_fr();
    let dt = Signal::new(jiff::civil::date(2026, 5, 4).at(14, 35, 0, 0));
    let display = FernDateTimeFormatter::new()
        .date_style(fern_i18n::DateStyle::Long)
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

    fern_i18n::thread_local::clear();
}

#[test]
fn signal_formatter_no_manager_falls_back_gracefully() {
    fern_i18n::thread_local::clear();
    // No I18nManager installed — formatter falls back to the default
    // (`und`) locale and still produces a string. Just assert it
    // doesn't panic and produces a non-empty result.
    let s = NumberFormatter::new().format(42.0_f64).get();
    assert!(!s.is_empty(), "fallback output must be non-empty");
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

    fern_i18n::thread_local::clear();
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

    fern_i18n::thread_local::clear();
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

    fern_i18n::thread_local::clear();
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

    fern_i18n::thread_local::clear();
}
