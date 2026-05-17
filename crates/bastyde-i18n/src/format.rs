//! Locale-aware number and date/time formatting.
//!
//! Two consumer paths share one ICU4X layer:
//!
//! 1. **Bundle-side** — `manager.rs` installs [`bastyde_format_callback`] via
//!    `bundle.set_formatter(...)` and registers [`datetime_function`] as
//!    `DATETIME()`. This makes `{ NUMBER($n, ...) }` and `{ DATETIME($ts, ...) }`
//!    inside `.ftl` messages render correctly across locales.
//! 2. **Signal-side** — [`NumberFormatter`] and [`BastydeDateTimeFormatter`] take
//!    a `Signal<T>` (or static value) plus the i18n manager's locale signal
//!    and produce a `Signal<String>` that re-renders on either change. Used
//!    for displays that don't go through translated messages: SpinBox values,
//!    TableView cells, status bars, numeric inputs.
//!
//! Both paths funnel through the same `IcuNumberFormatter` / `IcuDateTimeFormatter`
//! `Memoizable` types, so identical inputs produce identical strings — a UI
//! mixing translated and untranslated displays stays internally consistent on
//! `,` vs `.`, grouping, etc.
//!
//! # ICU coverage in this implementation
//!
//! Backed by `icu_decimal` (full) and `icu_datetime` (full); currency and
//! percent live in the unstable `icu_experimental` crate today and are not
//! linked here. Resulting limitations:
//!
//! - **Decimal** — full locale-aware grouping, digit shaping, sign handling.
//! - **Percent** — value × 100, formatted as decimal, suffixed with ASCII `%`.
//!   The percent sign is locale-naive; in CJK/Arabic this is acceptable but
//!   visually less polished than ICU's `PercentFormatter`. Promote when the
//!   experimental crate stabilises.
//! - **Currency** — value formatted as decimal with the ISO-4217 code
//!   appended as a suffix (`"42,50 EUR"`). No symbol substitution, no
//!   per-locale prefix/suffix positioning. Promote alongside Percent.
//! - **DateTime** — full ICU support via `CompositeDateTimeFieldSet`; date
//!   style + time style runtime-selected.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::str::FromStr;

use bastyde_core::signal::{Prop, Signal};
use fluent_bundle::types::{FluentNumber, FluentType};
use fluent_bundle::{FluentArgs, FluentValue};
use icu_calendar::Date as IcuDate;
use icu_datetime::DateTimeFormatter;
use icu_datetime::fieldsets::builder::{DateFields, FieldSetBuilder};
use icu_datetime::fieldsets::enums::CompositeDateTimeFieldSet;
use icu_datetime::input::{DateTime as IcuDateTime, Time as IcuTime};
use icu_datetime::options::{Length as DtLength, TimePrecision};
use icu_decimal::DecimalFormatter;
use icu_decimal::options::{DecimalFormatterOptions, GroupingStrategy};
use icu_locale_core::Locale as IcuLocale;
use intl_memoizer::Memoizable;
use unic_langid::LanguageIdentifier;

use crate::thread_local::{current_version_signal, with_active};

// ------------------------------------------------------------
// Public enums (builder-facing)
// ------------------------------------------------------------

/// What kind of number is being formatted.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Default)]
pub enum NumberStyle {
    #[default]
    Decimal,
    /// Multiply by 100 and append `%`. The sign is locale-naive; see
    /// the module-level note on percent coverage.
    Percent,
    /// Format as decimal and append the ISO-4217 currency code from
    /// `NumberFormatter::currency(...)`. Locale-naive positioning; see
    /// the module-level note on currency coverage.
    Currency,
}

/// Length of a date sub-pattern. Maps to ICU `Length::{Long, Medium, Short}`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DateStyle {
    Long,
    Medium,
    Short,
}

impl DateStyle {
    fn from_fluent_str(s: &str) -> Option<Self> {
        match s {
            // CLDR has "full" and "long"; ICU's `Length` enum collapses
            // them into `Long`. Accept both for `.ftl` ergonomics.
            "full" | "long" => Some(Self::Long),
            "medium" => Some(Self::Medium),
            "short" => Some(Self::Short),
            _ => None,
        }
    }

    fn icu_length(self) -> DtLength {
        match self {
            Self::Long => DtLength::Long,
            Self::Medium => DtLength::Medium,
            Self::Short => DtLength::Short,
        }
    }
}

/// Length of a time sub-pattern.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TimeStyle {
    Long,
    Medium,
    Short,
}

impl TimeStyle {
    fn from_fluent_str(s: &str) -> Option<Self> {
        match s {
            "full" | "long" => Some(Self::Long),
            "medium" => Some(Self::Medium),
            "short" => Some(Self::Short),
            _ => None,
        }
    }

    fn icu_length(self) -> DtLength {
        match self {
            Self::Long => DtLength::Long,
            Self::Medium => DtLength::Medium,
            Self::Short => DtLength::Short,
        }
    }
}

// ------------------------------------------------------------
// Internal options (cache keys)
// ------------------------------------------------------------

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct NumberOptions {
    style: NumberStyle,
    currency: Option<String>,
    use_grouping: bool,
    min_fraction_digits: Option<u8>,
    max_fraction_digits: Option<u8>,
}

impl Default for NumberOptions {
    fn default() -> Self {
        Self {
            style: NumberStyle::Decimal,
            currency: None,
            use_grouping: true,
            min_fraction_digits: None,
            max_fraction_digits: None,
        }
    }
}

impl NumberOptions {
    /// Build from `FluentNumber.options` (set by Fluent's builtin `NUMBER`
    /// after merging named args). Only the fields ICU can act on are read.
    fn from_fluent(opts: &fluent_bundle::types::FluentNumberOptions) -> Self {
        let style = match opts.style {
            fluent_bundle::types::FluentNumberStyle::Decimal => NumberStyle::Decimal,
            fluent_bundle::types::FluentNumberStyle::Currency => NumberStyle::Currency,
            fluent_bundle::types::FluentNumberStyle::Percent => NumberStyle::Percent,
        };
        Self {
            style,
            currency: opts.currency.clone(),
            use_grouping: opts.use_grouping,
            min_fraction_digits: opts.minimum_fraction_digits.map(|n| n as u8),
            max_fraction_digits: opts.maximum_fraction_digits.map(|n| n as u8),
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Default)]
struct DateTimeOptions {
    date_style: Option<DateStyle>,
    time_style: Option<TimeStyle>,
}

impl DateTimeOptions {
    fn merge_named(&mut self, named: &FluentArgs) {
        for (k, v) in named.iter() {
            match (k, v) {
                ("dateStyle", FluentValue::String(s)) => {
                    if let Some(ds) = DateStyle::from_fluent_str(s.as_ref()) {
                        self.date_style = Some(ds);
                    }
                }
                ("timeStyle", FluentValue::String(s)) => {
                    if let Some(ts) = TimeStyle::from_fluent_str(s.as_ref()) {
                        self.time_style = Some(ts);
                    }
                }
                _ => {}
            }
        }
        if self.date_style.is_none() && self.time_style.is_none() {
            self.date_style = Some(DateStyle::Medium);
        }
    }
}

// ------------------------------------------------------------
// unic_langid → icu_locale_core bridge
// ------------------------------------------------------------

fn lang_to_icu_locale(lang: &LanguageIdentifier) -> IcuLocale {
    IcuLocale::from_str(&lang.to_string()).unwrap_or(IcuLocale::UNKNOWN)
}

// ------------------------------------------------------------
// Memoizable wrappers — the cache layer
// ------------------------------------------------------------

/// ICU `DecimalFormatter` keyed by `(lang, NumberOptions)`. The memoizer
/// stores one instance per `(lang, opts)` combo across the bundle's lifetime.
///
/// The decimal formatter only knows about grouping and digit shaping;
/// percent/currency styling and fraction-digit padding/truncation are
/// applied to the [`fixed_decimal::Decimal`] *before* handing it off.
struct IcuNumberFormatter {
    inner: DecimalFormatter,
    opts: NumberOptions,
}

impl IcuNumberFormatter {
    fn format(&self, value: f64) -> String {
        let scaled = if matches!(self.opts.style, NumberStyle::Percent) {
            value * 100.0
        } else {
            value
        };

        let mut decimal = match fixed_decimal::Decimal::try_from_f64(
            scaled,
            fixed_decimal::FloatPrecision::RoundTrip,
        ) {
            Ok(d) => d,
            Err(_) => return scaled.to_string(),
        };

        // Apply max-fraction-digits first (rounds half-to-even), then
        // min-fraction-digits (zero-pads). Order matters: rounding may
        // strip trailing zeros that the min then re-adds.
        if let Some(max) = self.opts.max_fraction_digits {
            decimal.round(-(max as i16));
        }
        if let Some(min) = self.opts.min_fraction_digits {
            decimal.pad_end(-(min as i16));
        }

        let body = self.inner.format(&decimal).to_string();

        match (&self.opts.style, &self.opts.currency) {
            (NumberStyle::Percent, _) => format!("{body}%"),
            (NumberStyle::Currency, Some(code)) => format!("{body} {code}"),
            _ => body,
        }
    }
}

impl Memoizable for IcuNumberFormatter {
    type Args = (NumberOptions,);
    type Error = ();

    fn construct(lang: LanguageIdentifier, args: Self::Args) -> Result<Self, Self::Error> {
        let opts = args.0;
        let icu_locale = lang_to_icu_locale(&lang);
        let mut decimal_opts = DecimalFormatterOptions::default();
        decimal_opts.grouping_strategy = Some(if opts.use_grouping {
            GroupingStrategy::Auto
        } else {
            GroupingStrategy::Never
        });
        let inner =
            DecimalFormatter::try_new((&icu_locale).into(), decimal_opts).map_err(|_| ())?;
        Ok(Self { inner, opts })
    }
}

/// ICU `DateTimeFormatter<CompositeDateTimeFieldSet>` keyed by
/// `(lang, DateTimeOptions)`.
struct IcuDateTimeFormatter {
    inner: DateTimeFormatter<CompositeDateTimeFieldSet>,
}

impl IcuDateTimeFormatter {
    fn format_civil(&self, dt: &jiff::civil::DateTime) -> String {
        let date = match IcuDate::try_new_iso(dt.year() as i32, dt.month() as u8, dt.day() as u8) {
            Ok(d) => d.to_any(),
            Err(_) => return String::new(),
        };
        let time = match IcuTime::try_new(dt.hour() as u8, dt.minute() as u8, dt.second() as u8, 0)
        {
            Ok(t) => t,
            Err(_) => return String::new(),
        };
        let icu_dt = IcuDateTime { date, time };
        self.inner.format(&icu_dt).to_string()
    }

    fn format_zoned(&self, z: &jiff::Zoned) -> String {
        // For the composite-date-time field set we don't render time-zone
        // fields, so dropping the offset and formatting as civil gives the
        // user-local wall-clock representation — which is what `Zoned`'s
        // `.datetime()` already returns.
        self.format_civil(&z.datetime())
    }
}

impl Memoizable for IcuDateTimeFormatter {
    type Args = (DateTimeOptions,);
    type Error = ();

    fn construct(lang: LanguageIdentifier, args: Self::Args) -> Result<Self, Self::Error> {
        let opts = args.0;
        let icu_locale = lang_to_icu_locale(&lang);

        let mut builder = FieldSetBuilder::new();
        if let Some(ds) = opts.date_style {
            builder.date_fields = Some(DateFields::YMD);
            builder.length = Some(ds.icu_length());
        }
        if let Some(ts) = opts.time_style {
            builder.time_precision = Some(match ts {
                TimeStyle::Short => TimePrecision::Minute,
                TimeStyle::Medium | TimeStyle::Long => TimePrecision::Second,
            });
            // If only time was requested, use its length as the field-set length.
            if builder.length.is_none() {
                builder.length = Some(ts.icu_length());
            }
        }
        if builder.length.is_none() {
            builder.length = Some(DtLength::Medium);
        }

        let field_set = builder.build_composite_datetime().map_err(|_| ())?;
        let inner = DateTimeFormatter::try_new((&icu_locale).into(), field_set).map_err(|_| ())?;
        Ok(Self { inner })
    }
}

// ------------------------------------------------------------
// BastydeDateTime — the FluentValue::Custom payload
// ------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum BastydeDateTimeInner {
    Civil(jiff::civil::DateTime),
    Zoned(jiff::Zoned),
}

/// A datetime value that round-trips through Fluent's `FluentValue::Custom`.
///
/// Wraps a `jiff::civil::DateTime` or `jiff::Zoned`. Carries optional
/// `dateStyle` / `timeStyle` overrides set by the `DATETIME()` Fluent
/// function. Stringification at format-pattern time goes through this
/// crate's `IcuDateTimeFormatter` cache, so output is locale-aware.
#[derive(Clone, Debug, PartialEq)]
pub struct BastydeDateTime {
    inner: BastydeDateTimeInner,
    options: DateTimeOptions,
}

impl BastydeDateTime {
    pub fn from_civil(dt: jiff::civil::DateTime) -> Self {
        Self {
            inner: BastydeDateTimeInner::Civil(dt),
            options: DateTimeOptions::default(),
        }
    }

    pub fn from_zoned(z: jiff::Zoned) -> Self {
        Self {
            inner: BastydeDateTimeInner::Zoned(z),
            options: DateTimeOptions::default(),
        }
    }

    fn with_options(mut self, options: DateTimeOptions) -> Self {
        self.options = options;
        self
    }
}

impl From<jiff::civil::DateTime> for BastydeDateTime {
    fn from(dt: jiff::civil::DateTime) -> Self {
        Self::from_civil(dt)
    }
}

impl From<jiff::Zoned> for BastydeDateTime {
    fn from(z: jiff::Zoned) -> Self {
        Self::from_zoned(z)
    }
}

impl From<BastydeDateTime> for FluentValue<'static> {
    fn from(f: BastydeDateTime) -> Self {
        FluentValue::Custom(Box::new(f))
    }
}

impl FluentType for BastydeDateTime {
    fn duplicate(&self) -> Box<dyn FluentType + Send> {
        Box::new(self.clone())
    }

    fn as_string(&self, intls: &intl_memoizer::IntlLangMemoizer) -> Cow<'static, str> {
        let mut opts = self.options.clone();
        if opts.date_style.is_none() && opts.time_style.is_none() {
            opts.date_style = Some(DateStyle::Medium);
        }
        let inner = self.inner.clone();
        let result =
            intls.with_try_get::<IcuDateTimeFormatter, _, _>((opts,), move |fmt| match &inner {
                BastydeDateTimeInner::Civil(dt) => fmt.format_civil(dt),
                BastydeDateTimeInner::Zoned(z) => fmt.format_zoned(z),
            });
        Cow::Owned(result.unwrap_or_default())
    }

    fn as_string_threadsafe(
        &self,
        _intls: &intl_memoizer::concurrent::IntlLangMemoizer,
    ) -> Cow<'static, str> {
        // bastyde-i18n uses fluent-bundle's default (non-threadsafe) memoizer
        // (`I18nManager` runs on a single thread; `Rc`-based `Signal` is
        // not Send anyway). The threadsafe path is unreachable in practice
        // — fall back to the value's own Debug to keep the trait satisfied
        // without pulling the concurrent ICU formatter cache up.
        Cow::Owned(format!("{:?}", self.inner))
    }
}

// ------------------------------------------------------------
// Bundle-side: set_formatter callback + DATETIME function
// ------------------------------------------------------------

/// `fn` (no captures) installed via `bundle.set_formatter(...)` in
/// `manager::configure_bundle`. Dispatches `FluentValue::Number` through
/// the ICU cache; `FluentValue::Custom` (e.g. our `BastydeDateTime`) falls
/// through to `MemoizerKind::stringify_value` which calls
/// `FluentType::as_string`.
pub(crate) fn bastyde_format_callback(
    value: &FluentValue,
    intls: &intl_memoizer::IntlLangMemoizer,
) -> Option<String> {
    match value {
        FluentValue::Number(n) => format_number_via_memoizer(n, intls),
        _ => None,
    }
}

fn format_number_via_memoizer(
    n: &FluentNumber,
    intls: &intl_memoizer::IntlLangMemoizer,
) -> Option<String> {
    let opts = NumberOptions::from_fluent(&n.options);
    let value = n.value;
    intls
        .with_try_get::<IcuNumberFormatter, _, _>((opts,), move |fmt| fmt.format(value))
        .ok()
}

/// Fluent custom function registered as `DATETIME` via
/// `bundle.add_function("DATETIME", ...)`. Expects positional[0] to be a
/// `FluentValue::Custom(BastydeDateTime)`; merges named args (`dateStyle`,
/// `timeStyle`) into a fresh copy and returns it as a Custom for the
/// resolver to stringify via `FluentType::as_string`.
pub(crate) fn datetime_function<'a>(
    positional: &[FluentValue<'a>],
    named: &FluentArgs,
) -> FluentValue<'a> {
    let Some(FluentValue::Custom(custom)) = positional.first() else {
        return FluentValue::Error;
    };
    // FluentType is dyn — downcasting through Any is the only path.
    let any = custom.as_any();
    let Some(fdt) = any.downcast_ref::<BastydeDateTime>() else {
        return FluentValue::Error;
    };
    let mut opts = fdt.options.clone();
    opts.merge_named(named);
    FluentValue::Custom(Box::new(fdt.clone().with_options(opts)))
}

// ------------------------------------------------------------
// Signal-side: thread-local cache + public formatters
// ------------------------------------------------------------

thread_local! {
    static NUMBER_CACHE: RefCell<HashMap<(LanguageIdentifier, NumberOptions), Rc<IcuNumberFormatter>>> =
        RefCell::new(HashMap::new());
    static DATETIME_CACHE: RefCell<HashMap<(LanguageIdentifier, DateTimeOptions), Rc<IcuDateTimeFormatter>>> =
        RefCell::new(HashMap::new());
}

fn cached_number_formatter(
    lang: &LanguageIdentifier,
    opts: &NumberOptions,
) -> Option<Rc<IcuNumberFormatter>> {
    NUMBER_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(existing) = c.get(&(lang.clone(), opts.clone())) {
            return Some(existing.clone());
        }
        let fmt = IcuNumberFormatter::construct(lang.clone(), (opts.clone(),)).ok()?;
        let rc = Rc::new(fmt);
        c.insert((lang.clone(), opts.clone()), rc.clone());
        Some(rc)
    })
}

fn cached_datetime_formatter(
    lang: &LanguageIdentifier,
    opts: &DateTimeOptions,
) -> Option<Rc<IcuDateTimeFormatter>> {
    DATETIME_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(existing) = c.get(&(lang.clone(), opts.clone())) {
            return Some(existing.clone());
        }
        let fmt = IcuDateTimeFormatter::construct(lang.clone(), (opts.clone(),)).ok()?;
        let rc = Rc::new(fmt);
        c.insert((lang.clone(), opts.clone()), rc.clone());
        Some(rc)
    })
}

/// Locale-aware number formatter, configured via builder methods. Produces
/// a `Signal<String>` from a value (static or signal-bound) that re-renders
/// on either the value or the active i18n locale changing.
///
/// Subscribes to `I18nManager::locale_signal()` (the user-facing i18n
/// locale), **not** `BuildContext::locale_signal()` (an engine-internal
/// override used by the debug inspector).
#[derive(Clone, Debug, Default)]
pub struct NumberFormatter {
    options: NumberOptions,
}

impl NumberFormatter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn style(mut self, style: NumberStyle) -> Self {
        self.options.style = style;
        self
    }

    /// Switch to currency style and set the ISO-4217 code to append.
    pub fn currency(mut self, code: impl Into<String>) -> Self {
        self.options.style = NumberStyle::Currency;
        self.options.currency = Some(code.into());
        self
    }

    /// Switch to percent style. The value is multiplied by 100.
    pub fn percent(mut self) -> Self {
        self.options.style = NumberStyle::Percent;
        self
    }

    pub fn fraction_digits(mut self, min: u8, max: u8) -> Self {
        self.options.min_fraction_digits = Some(min);
        self.options.max_fraction_digits = Some(max);
        self
    }

    pub fn use_grouping(mut self, on: bool) -> Self {
        self.options.use_grouping = on;
        self
    }

    /// Format the value into a `Signal<String>` that re-renders on value or
    /// locale change. If no `I18nManager` is installed on this thread (e.g.
    /// in a low-level widget test), the result is a static snapshot using
    /// the source locale's defaults.
    pub fn format(&self, value: impl Into<Prop<f64>>) -> Signal<String> {
        let prop = value.into();
        let opts = self.options.clone();
        format_number_signal(prop, opts)
    }
}

/// Locale-aware date/time formatter. Same reactive shape as
/// [`NumberFormatter`]. Named with the `Bastyde` prefix to avoid colliding
/// with `icu_datetime::DateTimeFormatter` (which bastyde-i18n uses
/// internally to back this type).
#[derive(Clone, Debug, Default)]
pub struct BastydeDateTimeFormatter {
    options: DateTimeOptions,
}

impl BastydeDateTimeFormatter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn date_style(mut self, style: DateStyle) -> Self {
        self.options.date_style = Some(style);
        self
    }

    pub fn time_style(mut self, style: TimeStyle) -> Self {
        self.options.time_style = Some(style);
        self
    }

    /// Format a civil (timezone-naive) datetime.
    pub fn format(&self, value: impl Into<Prop<jiff::civil::DateTime>>) -> Signal<String> {
        let prop = value.into();
        let mut opts = self.options.clone();
        if opts.date_style.is_none() && opts.time_style.is_none() {
            opts.date_style = Some(DateStyle::Medium);
        }
        format_datetime_signal_civil(prop, opts)
    }

    /// Format a zoned datetime; rendered as the wall-clock value at its zone.
    pub fn format_zoned(&self, value: impl Into<Prop<jiff::Zoned>>) -> Signal<String> {
        let prop = value.into();
        let mut opts = self.options.clone();
        if opts.date_style.is_none() && opts.time_style.is_none() {
            opts.date_style = Some(DateStyle::Medium);
        }
        format_datetime_signal_zoned(prop, opts)
    }
}

fn locale_signal_or_default() -> Signal<LanguageIdentifier> {
    if let Some(s) = with_active(|m| m.locale_signal().clone()) {
        s
    } else {
        Signal::new(LanguageIdentifier::default())
    }
}

// Format-signal builders use derived-signal chaining (`zip` / `zip3` /
// `map`) instead of `observe + attach_keepalive`. Reason: callers are
// allowed to pass a derived `Signal<T>` (e.g. produced by `Signal::map`)
// to `format(...)`, and `Signal::observe` panics on read-only signals.
// Derived signals dirty-track through to upstream mutable roots
// automatically, so the framework's binding system invalidates correctly
// without an explicit observer-push from this layer.
//
// All three flavours share the same shape:
//   - resolve the locale signal (always mutable; falls back to a fresh
//     `Signal::new(und)` when no manager is installed)
//   - resolve the version signal (Some when a manager is installed)
//   - chain `value × locale × version → String` via the right combinator
//     for the (Static-vs-Bound) × (version-vs-no-version) cell.

fn render_number(value: f64, lang: &LanguageIdentifier, opts: &NumberOptions) -> String {
    match cached_number_formatter(lang, opts) {
        Some(fmt) => fmt.format(value),
        None => value.to_string(),
    }
}

fn render_civil(
    value: &jiff::civil::DateTime,
    lang: &LanguageIdentifier,
    opts: &DateTimeOptions,
) -> String {
    match cached_datetime_formatter(lang, opts) {
        Some(fmt) => fmt.format_civil(value),
        None => value.to_string(),
    }
}

fn render_zoned(value: &jiff::Zoned, lang: &LanguageIdentifier, opts: &DateTimeOptions) -> String {
    match cached_datetime_formatter(lang, opts) {
        Some(fmt) => fmt.format_zoned(value),
        None => value.to_string(),
    }
}

fn format_number_signal(prop: Prop<f64>, opts: NumberOptions) -> Signal<String> {
    let locale = locale_signal_or_default();
    let version = current_version_signal();

    match (prop, version) {
        (Prop::Static(value), Some(version)) => {
            let opts = opts.clone();
            locale
                .zip(&version)
                .map(move |(lang, _ver)| render_number(value, lang, &opts))
        }
        (Prop::Static(value), None) => {
            let opts = opts.clone();
            locale.map(move |lang| render_number(value, lang, &opts))
        }
        (Prop::Bound(value_signal), Some(version)) => {
            let opts = opts.clone();
            value_signal
                .zip3(&locale, &version)
                .map(move |(v, lang, _ver)| render_number(*v, lang, &opts))
        }
        (Prop::Bound(value_signal), None) => {
            let opts = opts.clone();
            value_signal
                .zip(&locale)
                .map(move |(v, lang)| render_number(*v, lang, &opts))
        }
    }
}

fn format_datetime_signal_civil(
    prop: Prop<jiff::civil::DateTime>,
    opts: DateTimeOptions,
) -> Signal<String> {
    let locale = locale_signal_or_default();
    let version = current_version_signal();

    match (prop, version) {
        (Prop::Static(value), Some(version)) => {
            let opts = opts.clone();
            locale
                .zip(&version)
                .map(move |(lang, _ver)| render_civil(&value, lang, &opts))
        }
        (Prop::Static(value), None) => {
            let opts = opts.clone();
            locale.map(move |lang| render_civil(&value, lang, &opts))
        }
        (Prop::Bound(value_signal), Some(version)) => {
            let opts = opts.clone();
            value_signal
                .zip3(&locale, &version)
                .map(move |(v, lang, _ver)| render_civil(v, lang, &opts))
        }
        (Prop::Bound(value_signal), None) => {
            let opts = opts.clone();
            value_signal
                .zip(&locale)
                .map(move |(v, lang)| render_civil(v, lang, &opts))
        }
    }
}

fn format_datetime_signal_zoned(prop: Prop<jiff::Zoned>, opts: DateTimeOptions) -> Signal<String> {
    let locale = locale_signal_or_default();
    let version = current_version_signal();

    match (prop, version) {
        (Prop::Static(value), Some(version)) => {
            let opts = opts.clone();
            locale
                .zip(&version)
                .map(move |(lang, _ver)| render_zoned(&value, lang, &opts))
        }
        (Prop::Static(value), None) => {
            let opts = opts.clone();
            locale.map(move |lang| render_zoned(&value, lang, &opts))
        }
        (Prop::Bound(value_signal), Some(version)) => {
            let opts = opts.clone();
            value_signal
                .zip3(&locale, &version)
                .map(move |(v, lang, _ver)| render_zoned(v, lang, &opts))
        }
        (Prop::Bound(value_signal), None) => {
            let opts = opts.clone();
            value_signal
                .zip(&locale)
                .map(move |(v, lang)| render_zoned(v, lang, &opts))
        }
    }
}
