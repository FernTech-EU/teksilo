//! Phase H demo — §12 internationalization showcase.
//!
//! Run with: `cargo run -p internationalization`
//!
//! This example wires up the full FernUI i18n stack end-to-end:
//!
//! - Three locales compiled in via `I18nConfig::compile_in`: English
//!   (source language), French, and Arabic.
//! - Labels produced via `tr!(...)` — compile-time validated against
//!   `locales/en-US.ftl` by the proc macro, reactively resolved at
//!   runtime through the active `I18nManager`.
//! - A `greeting` message with a `{ $name }` argument, demonstrating
//!   the per-arg `let`-binding + `clone` pattern the macro emits.
//! - A language selector (three `Button`s) that call
//!   `EventContext::set_locale(...)` directly from their
//!   `on_activate_fn` — no intent plumbing needed for ambient
//!   framework mutations.
//! - Arabic (`ar-SA`) triggers an RTL layout direction flip, so the
//!   bottom row of `HStack(Leading, Trailing)` visibly swaps its
//!   children. English and French are both LTR, so switching between
//!   them only changes the text — no layout reshuffle.
//!
//! Tree rebuild policy (§12.7, §Phase G): `WindowManager::set_locale`
//! applies `tree.set_layout_direction(...)` before `tree.set_locale(...)`
//! when the direction changes, so the composite rebuild observes the
//! new direction. Same-direction switches still rebuild today — a
//! future optimization can skip the rebuild once the widget binding
//! story is stable.

use fern_ui::core::widget::WidgetPlacement;
use fern_ui::i18n::{DateStyle, FernDateTime, FernDateTimeFormatter, NumberFormatter, tr_signal};
use fern_ui::prelude::*;
use fern_ui::widgets::{Button, ButtonVariant, HStack, Panel, TextWidget, VStack};

// ---------------------------------------------------------------------------
// Root composite widget.
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Root {
    user_name: String,
    /// Persistent reactive state for the formatting showcase. Lives on the
    /// widget struct (not inside `build()`) so + / − button clicks survive
    /// the composite rebuild that locale-switches trigger.
    price: Signal<f64>,
    count: Signal<i64>,
    /// Static "today" used for the date demos. Real apps would derive this
    /// from `jiff::Zoned::now()` and refresh on a timer; we use a fixed
    /// value to keep the example deterministic.
    today: jiff::civil::DateTime,
    root_child_id: Option<WidgetId>,
}

impl Root {
    fn new() -> Self {
        Self {
            user_name: "Alice".to_string(),
            price: Signal::new(1234.56),
            count: Signal::new(3),
            today: jiff::civil::date(2026, 5, 4).at(14, 35, 0, 0),
            root_child_id: None,
        }
    }
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme_signal().get();
        let name = self.user_name.clone();

        // A `Signal<String>` that tracks which direction the tree is
        // currently laid out in. `current_direction()` reaches the
        // i18n thread-local, so this is `None` only when the
        // application did not register an `I18nConfig` — in which
        // case the demo still shows the English compile-time fallback
        // text. Map the enum to the translated label via `tr!` so the
        // keys are validated at compile time (renaming the key in
        // `en-US.ftl` would then produce a compile error instead of a
        // silent runtime placeholder).
        let direction_signal = fern_ui::i18n::current_direction();
        let direction_label = ctx.signal(direction_note_label_for(direction_signal.as_ref()));
        if let Some(sig) = direction_signal.as_ref() {
            let target = direction_label.clone();
            ctx.effect(sig, move |dir| {
                target.set(direction_note_label(*dir));
            });
        }

        let heading = ctx.add(
            TextWidget::new(tr!(heading()))
                .style(theme.typography.body_bold.clone())
                .color(theme.colors.text_primary),
        );

        let greeting = ctx.add(
            TextWidget::new(tr!(greeting(name = name)))
                .style(theme.typography.body_bold.clone())
                .color(theme.colors.text_primary),
        );

        let body = ctx.add(
            TextWidget::new(tr!(body_paragraph()))
                .style(theme.typography.body.clone())
                .color(theme.colors.text_primary),
        );

        let direction_note = ctx.add(
            TextWidget::new_literal("")
                .bind_text(direction_label)
                .style(theme.typography.small.clone())
                .color(theme.colors.text_secondary),
        );

        let lang_label = ctx.add(
            TextWidget::new(tr!(language_label()))
                .style(theme.typography.body_bold.clone())
                .color(theme.colors.text_primary),
        );

        let en_btn = ctx.add(
            Button::new(tr!(lang_english()))
                .style(ButtonVariant::Regular)
                .on_activate_fn(|ctx| ctx.set_locale("en-US")),
        );
        let fr_btn = ctx.add(
            Button::new(tr!(lang_french()))
                .style(ButtonVariant::Regular)
                .on_activate_fn(|ctx| ctx.set_locale("fr-FR")),
        );
        let ar_btn = ctx.add(
            Button::new(tr!(lang_arabic()))
                .style(ButtonVariant::Regular)
                .on_activate_fn(|ctx| ctx.set_locale("ar-SA")),
        );

        let language_row = ctx.add(
            HStack::new()
                .spacing(8.0)
                .add_child(lang_label)
                .add_child(en_btn)
                .add_child(fr_btn)
                .add_child(ar_btn),
        );

        // The RTL showcase: `Button::new(tr!(leading_button()))` + trailing
        // button inside an `HStack`. When the active locale flips to
        // `ar-SA`, `tree.set_layout_direction(RightToLeft)` changes how
        // `HStack` resolves leading/trailing, visibly reversing the
        // button order on screen.
        let leading_btn = ctx.add(Button::new(tr!(leading_button())).style(ButtonVariant::Regular));
        let trailing_btn =
            ctx.add(Button::new(tr!(trailing_button())).style(ButtonVariant::Regular));
        let direction_row = ctx.add(
            HStack::new()
                .spacing(12.0)
                .add_child(leading_btn)
                .add_child(trailing_btn),
        );

        // ====== Locale-aware formatting showcase ======
        //
        // Demonstrates the four `fern-i18n` formatting paths against a
        // single `Signal<f64> price` + `Signal<i64> count`:
        //
        //   1. Bundle-side `NUMBER()` / `DATETIME()` inside `.ftl` —
        //      `bundle_currency_row` and `bundle_date_row` keys carry
        //      the formatting calls. These rows update on **locale**
        //      flips (composite rebuild re-evaluates `tr!`); they do
        //      NOT react to the price/count signals because `tr!`
        //      captures arguments by value.
        //
        //   2. Signal-side `NumberFormatter` — three rows (decimal,
        //      currency, percent) bound directly to `Signal<f64>`,
        //      reactive to **both** value changes and locale flips.
        //
        //   3. Signal-side `FernDateTimeFormatter` — bound to a
        //      `Signal<jiff::civil::DateTime>`. Same reactivity model.
        //
        //   4. `tr_signal!` — translated message with `Signal<T>` args
        //      interpolated reactively. Re-renders on count, price,
        //      AND locale changes. The correct path for "I want a
        //      Signal<T> *inside* a translated sentence."

        let formatting_heading = ctx.add(
            TextWidget::new(tr!(formatting_heading()))
                .style(theme.typography.body_bold.clone())
                .color(theme.colors.text_primary),
        );

        // ---- Bundle-side rows (locale-reactive only) ----
        let bundle_currency = ctx.add(
            TextWidget::new(tr!(bundle_currency_row(price = self.price.get())))
                .style(theme.typography.body.clone())
                .color(theme.colors.text_primary),
        );
        let bundle_date = ctx.add(
            TextWidget::new(tr!(bundle_date_row(ts = FernDateTime::from(self.today))))
                .style(theme.typography.body.clone())
                .color(theme.colors.text_primary),
        );

        // ---- Signal-side rows (value- AND locale-reactive) ----
        let decimal_value = NumberFormatter::new()
            .fraction_digits(2, 2)
            .format(self.price.clone());
        let currency_value = NumberFormatter::new()
            .currency(per_locale_currency(ctx))
            .fraction_digits(2, 2)
            .format(self.price.clone());
        // Derive a 0..1 ratio from the price so the percent row has
        // something natural to display. Caps at 100 % so very large
        // prices don't push it off-screen.
        let ratio = self.price.map(|p| (p / 2000.0).clamp(0.0, 1.0));
        let percent_value = NumberFormatter::new()
            .percent()
            .fraction_digits(0, 1)
            .format(ratio);
        let date_value = FernDateTimeFormatter::new()
            .date_style(DateStyle::Long)
            .format(self.today);

        let signal_decimal_row = formatting_row(ctx, &theme, "Decimal:", decimal_value);
        let signal_currency_row = formatting_row(ctx, &theme, "Currency:", currency_value);
        let signal_percent_row = formatting_row(ctx, &theme, "Percent:", percent_value);
        let signal_date_row = formatting_row(ctx, &theme, "Date:", date_value);

        // ---- tr_signal! row (everything-reactive) ----
        let cart_summary_signal: Signal<String> =
            tr_signal!(cart_summary(count = self.count, price = self.price,));
        let cart_summary_text = ctx.add(
            TextWidget::new_literal("")
                .bind_text(cart_summary_signal)
                .style(theme.typography.body_bold.clone())
                .color(theme.colors.text_primary),
        );

        // ---- Controls: ± price and ± count ----
        let price_label = ctx.add(
            TextWidget::new(tr!(price_label()))
                .style(theme.typography.body.clone())
                .color(theme.colors.text_secondary),
        );
        let price_minus = ctx.add(
            Button::new_literal("− 100")
                .style(ButtonVariant::Regular)
                .on_activate_fn({
                    let price = self.price.clone();
                    move |_| price.set(price.get() - 100.0)
                }),
        );
        let price_plus = ctx.add(
            Button::new_literal("+ 100")
                .style(ButtonVariant::Regular)
                .on_activate_fn({
                    let price = self.price.clone();
                    move |_| price.set(price.get() + 100.0)
                }),
        );
        let price_controls = ctx.add(
            HStack::new()
                .spacing(8.0)
                .add_child(price_label)
                .add_child(price_minus)
                .add_child(price_plus),
        );

        let count_label = ctx.add(
            TextWidget::new(tr!(count_label()))
                .style(theme.typography.body.clone())
                .color(theme.colors.text_secondary),
        );
        let count_minus = ctx.add(
            Button::new_literal("− 1")
                .style(ButtonVariant::Regular)
                .on_activate_fn({
                    let count = self.count.clone();
                    move |_| count.set((count.get() - 1).max(0))
                }),
        );
        let count_plus = ctx.add(
            Button::new_literal("+ 1")
                .style(ButtonVariant::Regular)
                .on_activate_fn({
                    let count = self.count.clone();
                    move |_| count.set(count.get() + 1)
                }),
        );
        let count_controls = ctx.add(
            HStack::new()
                .spacing(8.0)
                .add_child(count_label)
                .add_child(count_minus)
                .add_child(count_plus),
        );

        let column = ctx.add(
            VStack::new()
                .spacing(16.0)
                .add_child(heading)
                .add_child(greeting)
                .add_child(body)
                .add_child(direction_note)
                .add_child(language_row)
                .add_child(direction_row)
                .add_child(formatting_heading)
                .add_child(bundle_currency)
                .add_child(bundle_date)
                .add_child(signal_decimal_row)
                .add_child(signal_currency_row)
                .add_child(signal_percent_row)
                .add_child(signal_date_row)
                .add_child(cart_summary_text)
                .add_child(price_controls)
                .add_child(count_controls),
        );

        let root_id = ctx.add(Panel::new().padding(24.0).child_id(column));
        self.root_child_id = Some(root_id);
        vec![root_id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

/// One row of the formatting showcase: a literal English label paired
/// with a `Signal<String>` that the Number/DateTime formatters produced.
/// Pulled out as a helper because four rows share the same layout.
fn formatting_row(
    ctx: &mut BuildContext,
    theme: &fern_ui::tokens::Theme,
    label: &'static str,
    value: Signal<String>,
) -> WidgetId {
    let label_widget = ctx.add(
        TextWidget::new_literal(label)
            .style(theme.typography.body.clone())
            .color(theme.colors.text_secondary),
    );
    let value_widget = ctx.add(
        TextWidget::new_literal("")
            .bind_text(value)
            .style(theme.typography.body.clone())
            .color(theme.colors.text_primary),
    );
    ctx.add(
        HStack::new()
            .spacing(8.0)
            .add_child(label_widget)
            .add_child(value_widget),
    )
}

/// Pick a locale-appropriate currency code for the Signal-side currency
/// demo. Matches the `bundle-currency-row` choice in each `.ftl` so the
/// bundle-side and Signal-side rows display the same currency for the
/// same active locale — translators choose the natural currency for
/// their market.
fn per_locale_currency(_ctx: &mut BuildContext) -> &'static str {
    let lang = fern_ui::i18n::current_locale()
        .map(|s| s.get().to_string())
        .unwrap_or_else(|| "en-US".to_string());
    if lang.starts_with("fr") {
        "EUR"
    } else if lang.starts_with("ar") {
        "SAR"
    } else {
        "USD"
    }
}

/// Resolve the direction-note label for the given layout direction.
/// Uses `tr!(...)` so the keys are validated at compile time against
/// `locales/en-US.ftl` — a rename or typo would produce a compile
/// error instead of a silent runtime placeholder.
fn direction_note_label(direction: fern_ui::i18n::LayoutDirection) -> String {
    match direction {
        fern_ui::i18n::LayoutDirection::LeftToRight => tr!(direction_note_ltr()).resolve_now(),
        fern_ui::i18n::LayoutDirection::RightToLeft => tr!(direction_note_rtl()).resolve_now(),
    }
}

/// Initial direction label, used to seed the reactive `Signal<String>`
/// before the first `set_locale` fires an effect. Matches what the
/// `ctx.effect(...)` body will compute on its next run.
fn direction_note_label_for(direction: Option<&Signal<fern_ui::i18n::LayoutDirection>>) -> String {
    direction
        .map(|s| s.get())
        .map(direction_note_label)
        .unwrap_or_else(|| direction_note_label(fern_ui::i18n::LayoutDirection::LeftToRight))
}

// ---------------------------------------------------------------------------
// Entry point.
// ---------------------------------------------------------------------------

/// Parse `--translation-dev LOCALE=PATH` flags from the command line.
///
/// Architecture §12.6: translator hot-reload is an application-layer
/// feature — the framework provides
/// `I18nConfig::runtime_override(locale, path)` and the watcher
/// machinery, but each application does its own CLI parsing. This
/// helper is small enough to roll without pulling in `clap`.
///
/// Usage:
///
/// ```text
/// cargo run -p internationalization -- \
///     --translation-dev fr-FR=/tmp/fr.ftl \
///     --translation-dev ar-SA=/tmp/ar.ftl
/// ```
///
/// Each `--translation-dev` flag registers one `.ftl` file whose
/// contents **replace** the compiled-in bundle for that locale at
/// startup. A `notify`-backed file watcher observes each path for
/// modifications and hot-reloads the bundle on save — the running UI
/// updates within ~100ms, no restart, no composite rebuild.
fn parse_translation_dev_flags() -> Vec<(LanguageIdentifier, std::path::PathBuf)> {
    let mut out = Vec::new();
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        if arg != "--translation-dev" {
            continue;
        }
        let Some(pair) = iter.next() else {
            eprintln!("--translation-dev expects a LOCALE=PATH argument");
            continue;
        };
        let Some((loc, path)) = pair.split_once('=') else {
            eprintln!("--translation-dev expects LOCALE=PATH, got `{pair}` (missing `=`)");
            continue;
        };
        let Ok(parsed): Result<LanguageIdentifier, _> = loc.parse() else {
            eprintln!("--translation-dev: invalid locale tag `{loc}`");
            continue;
        };
        out.push((parsed, std::path::PathBuf::from(path)));
    }
    out
}

fn main() {
    let mut config = I18nConfig::new()
        .source_locale("en-US".parse().unwrap())
        .supported_locales([
            "en-US".parse().unwrap(),
            "fr-FR".parse().unwrap(),
            "ar-SA".parse().unwrap(),
        ])
        .compile_in(&[
            ("en-US", &[include_str!("../locales/en-US.ftl")]),
            ("fr-FR", &[include_str!("../locales/fr-FR.ftl")]),
            ("ar-SA", &[include_str!("../locales/ar-SA.ftl")]),
        ])
        .auto_detect_os_locale(false)
        .fallback_locale("en-US".parse().unwrap())
        .framework_locales(fern_ui::widgets::framework_locales());

    // Apply any `--translation-dev LOCALE=PATH` overrides. Missing
    // files at startup are logged by the watcher and skipped — the
    // compile-in bundle stays in place for that locale.
    for (loc, path) in parse_translation_dev_flags() {
        println!(
            "fern-i18n: hot-reloading `{}` from `{}`",
            loc,
            path.display()
        );
        config = config.runtime_override(loc, path);
    }

    FernAppBuilder::new()
        .theme(Theme::light_default())
        .i18n(config)
        .initial_window(
            WindowConfig::new()
                .title("FernUI — Internationalization Demo")
                .size(720, 520)
                .root(|tree, _state| tree.add(Root::new())),
        )
        .run();
}
