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
use fern_ui::prelude::*;
use fern_ui::widgets::{Button, ButtonVariant, HStack, Panel, TextWidget, VStack};

// ---------------------------------------------------------------------------
// Root composite widget.
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Root {
    user_name: String,
    root_child_id: Option<WidgetId>,
}

impl Root {
    fn new() -> Self {
        Self {
            user_name: "Alice".to_string(),
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
        let leading_btn = ctx.add(
            Button::new(tr!(leading_button())).style(ButtonVariant::Regular),
        );
        let trailing_btn = ctx.add(
            Button::new(tr!(trailing_button())).style(ButtonVariant::Regular),
        );
        let direction_row = ctx.add(
            HStack::new()
                .spacing(12.0)
                .add_child(leading_btn)
                .add_child(trailing_btn),
        );

        let column = ctx.add(
            VStack::new()
                .spacing(16.0)
                .add_child(heading)
                .add_child(greeting)
                .add_child(body)
                .add_child(direction_note)
                .add_child(language_row)
                .add_child(direction_row),
        );

        let root_id = ctx.add(Panel::new().padding(24.0).child_id(column));
        self.root_child_id = Some(root_id);
        vec![root_id]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
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

/// Resolve the direction-note label for the given layout direction.
/// Uses `tr!(...)` so the keys are validated at compile time against
/// `locales/en-US.ftl` — a rename or typo would produce a compile
/// error instead of a silent runtime placeholder.
fn direction_note_label(direction: fern_ui::i18n::LayoutDirection) -> String {
    match direction {
        fern_ui::i18n::LayoutDirection::LeftToRight => {
            tr!(direction_note_ltr()).resolve_now()
        }
        fern_ui::i18n::LayoutDirection::RightToLeft => {
            tr!(direction_note_rtl()).resolve_now()
        }
    }
}

/// Initial direction label, used to seed the reactive `Signal<String>`
/// before the first `set_locale` fires an effect. Matches what the
/// `ctx.effect(...)` body will compute on its next run.
fn direction_note_label_for(
    direction: Option<&Signal<fern_ui::i18n::LayoutDirection>>,
) -> String {
    direction
        .map(|s| s.get())
        .map(direction_note_label)
        .unwrap_or_else(|| {
            direction_note_label(fern_ui::i18n::LayoutDirection::LeftToRight)
        })
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
            eprintln!(
                "--translation-dev expects LOCALE=PATH, got `{pair}` (missing `=`)"
            );
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
