// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Teksilo's own widget previewer binary.
//!
//! Run:
//!
//! ```text
//! cargo run -p teksilo-widgets-previewer
//! cargo run -p teksilo-widgets-previewer -- --widget=button --variant=disabled
//! cargo run -p teksilo-widgets-previewer -- --file=crates/teksilo-widgets/src/button.rs
//! cargo run -p teksilo-widgets-previewer -- --density=touch
//! cargo run -p teksilo-widgets-previewer -- --list
//! cargo run -p teksilo-widgets-previewer -- --export-docs
//! ```
//!
//! `--density=compact|comfortable|touch` starts the previewer on that input
//! density, which is the whole catalog rendered at that ladder; the toolbar
//! switches it live.
//!
//! `--export-docs` is the headless batch that fills `docs/widgets/img/`
//! with the pictures the generated mdBook catalog pages reference. It
//! needs a wgpu adapter but no display server. It takes the same
//! `--density=` flag, and accepts a comma-separated list — one pass per
//! density, `Compact` writing `img/<slug>.png` and every other density
//! writing `img/<slug>-<density>.png` beside it.
//!
//! The binary intentionally has no logic beyond delegation —
//! everything happens inside `teksilo_preview_ui::run_previewer`.
//! Downstream applications create their own analogous thin binary that
//! links their own widget set with the `preview` feature.

use std::path::PathBuf;

use clap::Parser;
use teksilo_preview_ui::PreviewerArgs;

/// Browse mode's flags come from the library fragment; the three modes —
/// browse, `--list` and `--export-docs` — are declared mutually exclusive
/// rather than resolved by precedence, so asking for two is a message instead
/// of a silent win for whichever branch `main` happened to test first.
///
/// The export sub-flags all `requires` the batch they configure, because a
/// `--dark` with no `--export-docs` used to be reported as an unrecognised
/// argument and should still be reported as something.
#[derive(Parser)]
#[command(
    name = "teksilo-widgets-previewer",
    version,
    about = "Teksilo Widget Previewer",
    long_about = "Browse the stock Teksilo widget catalog — navigator, live canvas and knob \
                  form — or run the headless batch that renders the mdBook catalog's images."
)]
struct Cli {
    #[command(flatten)]
    preview: PreviewerArgs,

    /// Print every registered catalog entry (group, id, name, source file) and exit.
    #[arg(long, conflicts_with = "export_docs")]
    list: bool,

    /// Render the catalog's documentation images into OUT_DIR (docs/widgets/img).
    ///
    /// Needs a wgpu adapter, but no display server. One pass per --density:
    /// compact is canonical and writes `img/<slug>.png`, every other density
    /// is additive and writes `img/<slug>-<density>.png` beside it.
    #[arg(
        long,
        value_name = "OUT_DIR",
        num_args = 0..=1,
        require_equals = true,
        conflicts_with_all = ["widget", "variant", "file", "window", "title"]
    )]
    export_docs: Option<Option<PathBuf>>,

    /// Catalog pages to check against (default: docs/widgets).
    ///
    /// A subject with no `<slug>.md` there is reported and skipped, so an
    /// image is never written with no page to appear on.
    #[arg(long, value_name = "DIR", requires = "export_docs")]
    pages: Option<PathBuf>,

    /// Write images even for slugs with no catalog page.
    #[arg(long, requires = "export_docs", conflicts_with = "pages")]
    all_subjects: bool,

    /// Render the dark theme instead of light.
    #[arg(long, requires = "export_docs")]
    dark: bool,

    /// HiDPI factor (default 2).
    #[arg(long, value_name = "N", requires = "export_docs", value_parser = positive_scale)]
    scale: Option<f32>,

    /// Restrict the batch to these slugs.
    #[arg(
        long,
        value_name = "SLUG",
        value_delimiter = ',',
        requires = "export_docs"
    )]
    only: Vec<String>,
}

/// `--scale` is a multiplier, so zero and negatives are not small values —
/// they are a canvas with no pixels in it.
fn positive_scale(raw: &str) -> Result<f32, String> {
    match raw.parse::<f32>() {
        Ok(s) if s > 0.0 => Ok(s),
        _ => Err(format!("invalid scale '{raw}' (expected a number above 0)")),
    }
}

fn main() {
    let cli = Cli::parse();

    if let Some(out_dir) = cli.export_docs.clone() {
        std::process::exit(run_doc_export(&cli, out_dir));
    }
    if cli.list {
        for entry in teksilo_preview::iter_entries() {
            println!(
                "{}\t{}\t{}\t{}",
                entry.group(),
                entry.id(),
                entry.display_name(),
                entry.source().file
            );
        }
        return;
    }
    let opts = teksilo_preview_ui::PreviewerOptions::from_parsed(cli.preview);
    teksilo_preview_ui::run_previewer(opts);
}

/// Headless documentation-image export.
///
/// Each density is a separate pass over the whole catalog. `compact` is the
/// canonical one and writes `img/<slug>.png`; the others are additive and
/// write `img/<slug>-<density>.png`, so `--density=compact,touch`
/// regenerates the committed images and adds the Touch variants in one run.
///
/// The returned code is the bitwise-or of every pass's, so a run that renders
/// three densities and fails one still says so.
fn run_doc_export(cli: &Cli, out_dir: Option<PathBuf>) -> i32 {
    let mut opts = teksilo_preview_ui::DocExportOptions::default();
    if let Some(dir) = out_dir {
        opts.out_dir = dir;
    }
    if let Some(dir) = cli.pages.clone() {
        opts.pages_dir = Some(dir);
    }
    if cli.all_subjects {
        opts.pages_dir = None;
    }
    opts.dark = cli.dark;
    if let Some(scale) = cli.scale {
        opts.scale = scale;
    }
    opts.only = cli.only.clone();

    let mut passes = cli.preview.density.clone();
    if passes.is_empty() {
        passes.push(teksilo_preview::PreviewPass::compact());
    }
    passes.dedup();

    let mut code = 0;
    for pass in passes {
        opts.pass = pass;
        println!(
            "Exporting catalog images to {} at {} density …",
            opts.out_dir.display(),
            opts.pass.label()
        );
        code |= match teksilo_preview_ui::export_doc_images(&opts) {
            Ok(report) => teksilo_preview_ui::print_report(&report, &opts),
            Err(e) => {
                eprintln!("teksilo-previewer: {}", e);
                1
            }
        };
    }
    code
}

// ---------------------------------------------------------------------------
// Smoke tests — unit tests inside the binary so they link the same
// inventory section as `main`. Integration tests in `tests/` against a
// binary-only crate would not see these symbols.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use teksilo_preview::{find_by_id, iter_entries};

    #[test]
    fn registry_contains_button() {
        let entry = find_by_id("button").expect("Button catalog entry should be registered");
        assert_eq!(entry.id(), "button");
        assert_eq!(entry.group(), "Controls");
        assert_eq!(entry.display_name(), "Button");
        assert!(
            !entry.variants().is_empty(),
            "Button must declare at least one variant"
        );
        assert!(
            !entry.knobs().declarations().is_empty(),
            "Button must declare at least one knob"
        );
    }

    #[test]
    fn registry_lists_all_tier_a_widgets() {
        let ids: Vec<&'static str> = iter_entries().map(|e| e.id()).collect();
        let expected = [
            "button",
            "checkbox",
            "radio_button",
            "toggle",
            "slider",
            "progress_bar",
            "badge",
            "link",
            "segmented_control",
            "combo_box",
            "divider",
            "icon_widget",
        ];
        for want in &expected {
            assert!(
                ids.contains(want),
                "expected id '{}' to be registered (found: {:?})",
                want,
                ids
            );
        }
    }

    #[test]
    fn entries_carry_source_locations() {
        let entry = find_by_id("button").unwrap();
        let loc = entry.source();
        assert!(
            loc.file.contains("preview_catalog") || loc.file.contains("button"),
            "expected source file to reference catalog, got '{}'",
            loc.file
        );
        assert!(loc.line > 0);
    }

    #[test]
    fn cli_widget_arg_round_trips_through_registry() {
        let opts =
            teksilo_preview_ui::PreviewerOptions::from_iter(["--widget=slider", "--variant=max"]);
        assert_eq!(opts.initial_widget.as_deref(), Some("slider"));
        assert_eq!(opts.initial_variant.as_deref(), Some("max"));
    }

    /// Confirms `--widget` validation accepts every registered id.
    /// This is the regression for the original report where the user
    /// said `--widget` "didn't work" — the parser accepted unknown
    /// ids silently and the runtime fell back to the first registered
    /// entry. We now reject unknown ids at parse time (which exits
    /// the process) so this test only checks the happy path; the
    /// rejection path is hard to test inside cargo because
    /// `process::exit` would kill the test runner.
    #[test]
    fn cli_widget_accepts_every_registered_id() {
        for entry in iter_entries() {
            let arg = format!("--widget={}", entry.id());
            let opts = teksilo_preview_ui::PreviewerOptions::from_iter([arg.as_str()]);
            assert_eq!(opts.initial_widget.as_deref(), Some(entry.id()));
        }
    }

    /// Same thing for `--variant=` against every variant of every
    /// registered widget — catches the second class of typo.
    #[test]
    fn cli_variant_accepts_every_registered_variant() {
        for entry in iter_entries() {
            for variant in entry.variants() {
                let opts = teksilo_preview_ui::PreviewerOptions::from_iter([
                    format!("--widget={}", entry.id()).as_str(),
                    format!("--variant={}", variant.name()).as_str(),
                ]);
                assert_eq!(opts.initial_widget.as_deref(), Some(entry.id()));
                assert_eq!(opts.initial_variant.as_deref(), Some(variant.name()));
            }
        }
    }

    /// Build and lay out every (widget, variant) pair through a real
    /// `WidgetTree`. Catches catalog impls that panic deep inside
    /// `Widget::build()` because they failed to satisfy a widget's
    /// required-content invariant — e.g. a `Snackbar` constructed
    /// without `.content(...)` panics during layout, not at
    /// construction. Without this test, those latent panics only
    /// surface when the user navigates to the offending widget.
    #[test]
    fn every_catalog_variant_lays_out_without_panic() {
        use teksilo_canvas::SizeProposal;
        use teksilo_core::widget_tree::WidgetTree;

        let mut failures: Vec<String> = Vec::new();
        // Every density the previewer and `--export-docs` can be asked for:
        // a catalog entry is a documentation subject too, and the ladder is
        // applied before `build()` runs.
        for pass in [
            teksilo_preview::PreviewPass::compact(),
            teksilo_preview::PreviewPass::new(teksilo_tokens::TargetDensity::Comfortable),
            teksilo_preview::PreviewPass::new(teksilo_tokens::TargetDensity::Touch),
        ] {
            for entry in iter_entries() {
                for variant in entry.variants() {
                    let label = format!("[{}] {}/{}", pass.label(), entry.id(), variant.name());
                    // Run each (widget, variant) pair in a separate
                    // `catch_unwind` so one failure doesn't prevent the
                    // rest from being checked — the failure list at the
                    // end is more useful than a single first-failure stack.
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let knobs = teksilo_preview::KnobValues::from_spec(&entry.knobs(), None);
                        let widget = entry.build(variant.name(), &knobs);
                        let mut tree =
                            WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
                        tree.set_input_density(pass.density());
                        assert_eq!(
                            tree.theme().input.density,
                            pass.density(),
                            "the pass must reach the tree, or this loop checks Compact three times"
                        );
                        let _ = tree.add_boxed(widget);
                        tree.layout(SizeProposal::exact(800.0, 600.0));
                    }));
                    if let Err(err) = result {
                        let msg = err
                            .downcast_ref::<&'static str>()
                            .map(|s| s.to_string())
                            .or_else(|| err.downcast_ref::<String>().cloned())
                            .unwrap_or_else(|| "<unknown panic>".to_string());
                        failures.push(format!("{}: {}", label, msg));
                    }
                }
            }
        }
        assert!(
            failures.is_empty(),
            "the following catalog (widget, variant) pairs panicked during \
             layout:\n  {}",
            failures.join("\n  "),
        );
    }

    /// A widget that paints a string must not also announce it as an
    /// ancestor's name.
    ///
    /// The rule is string equality against a strict ancestor's *own* name,
    /// whatever that ancestor's role: a `Button`, a `Role::Status` banner
    /// and a tab header all leak the same way, and a screen reader reads
    /// the string twice on the way into the control. A container named by
    /// pointing at its title through `labelled_by` is exempt — that
    /// relation working is not a duplicate.
    ///
    /// The catalog is the closest thing to a census of the widget set, so
    /// running the rule over it is what keeps a new widget from
    /// reintroducing a leak the audit has just cleared.
    #[test]
    fn no_label_duplicates_a_name_owning_ancestor() {
        let mut failures: Vec<String> = Vec::new();
        for_every_catalog_subject(|label, tree| {
            let update = tree.sync_accessibility();
            for leak in teksilo_core::accessibility::audit::duplicate_label_leaks(&update) {
                failures.push(format!(
                    "{label}: {:?} named \"{}\" also has a label reading it",
                    leak.owner_role, leak.name
                ));
            }
        });
        assert!(
            failures.is_empty(),
            "these labels repeat an ancestor's accessible name:\n  {}",
            failures.join("\n  "),
        );
    }

    /// Every visible label must be reviewable, and must review as what it
    /// announces.
    ///
    /// Three failures, all invisible in a screenshot: a label with no text
    /// runs cannot be reviewed by character or routed to on a braille
    /// display; a label whose runs assemble to a different string than its
    /// value makes a reader hear one thing and review another; and a label
    /// whose range reports no bounding boxes stops a magnifier tracking —
    /// which one geometry-less run is enough to cause for the whole label.
    #[test]
    fn every_visible_label_supports_text_ranges_and_never_diverges() {
        use teksilo_core::accessibility::audit;

        let mut failures: Vec<String> = Vec::new();
        for_every_catalog_subject(|label, tree| {
            let update = tree.sync_accessibility();
            for id in audit::labels_without_text_ranges(&update) {
                failures.push(format!("{label}: label {id:?} carries no text ranges"));
            }
            for divergence in audit::text_range_divergences(&update) {
                failures.push(format!("{label}: {divergence:?}"));
            }
        });
        assert!(
            failures.is_empty(),
            "these subjects fail the text-range invariants:\n  {}",
            failures.join("\n  "),
        );
    }

    /// Build, lay out and hand every catalog subject — each (widget,
    /// variant) pair and every documentation snippet — to `check`.
    ///
    /// A real text backend is installed: without one a label reports no
    /// geometry, and every geometry assertion below would pass by being
    /// unmeasurable rather than by being right.
    fn for_every_catalog_subject(
        mut check: impl FnMut(&str, &mut teksilo_core::widget_tree::WidgetTree),
    ) {
        use std::cell::RefCell;
        use std::rc::Rc;
        use teksilo_canvas::SizeProposal;
        use teksilo_core::widget_tree::WidgetTree;

        let mut run =
            |label: String, build: &mut dyn FnMut() -> Box<dyn teksilo_core::widget::Widget>| {
                let backend: Rc<RefCell<dyn teksilo_canvas::TextBackend>> =
                    Rc::new(RefCell::new(teksilo_canvas::MockTextBackend::new()));
                let mut tree = WidgetTree::new()
                    .with_theme(teksilo_core::presets::intui::light())
                    .with_text_backend(backend);
                let _ = tree.add_boxed(build());
                tree.layout(SizeProposal::exact(800.0, 600.0));
                check(&label, &mut tree);
            };

        for entry in iter_entries() {
            for variant in entry.variants() {
                let label = format!("{}/{}", entry.id(), variant.name());
                let knobs = teksilo_preview::KnobValues::from_spec(&entry.knobs(), None);
                run(label, &mut || entry.build(variant.name(), &knobs));
            }
        }
        for snippet in teksilo_preview::iter_doc_snippets() {
            run(snippet.source_file.to_string(), &mut || (snippet.build)());
        }
    }

    /// The documentation snippets are a second registry feeding the same
    /// image exporter, and they are only exercised by a `--export-docs`
    /// run. Build and lay out every one so a snippet that panics deep
    /// inside `build()` fails here rather than in the docs job.
    #[test]
    fn every_doc_snippet_lays_out_without_panic() {
        use teksilo_canvas::SizeProposal;
        use teksilo_core::widget_tree::WidgetTree;

        let mut failures: Vec<String> = Vec::new();
        // Every pass `--export-docs` can be asked for, not just the canonical
        // one: a density is applied before `build()` runs, so a snippet that
        // survives Compact can still panic on the ladder that moves its
        // dimensions — and the exporter would only report it as a failed
        // subject long after CI was green.
        for pass in [
            teksilo_preview::PreviewPass::compact(),
            teksilo_preview::PreviewPass::new(teksilo_tokens::TargetDensity::Comfortable),
            teksilo_preview::PreviewPass::new(teksilo_tokens::TargetDensity::Touch),
        ] {
            for snippet in teksilo_preview::iter_doc_snippets() {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let widget = (snippet.build)();
                    let mut tree =
                        WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
                    tree.set_input_density(pass.density());
                    assert_eq!(
                        tree.theme().input.density,
                        pass.density(),
                        "the pass must reach the tree, or this loop checks Compact three times"
                    );
                    let _ = tree.add_boxed(widget);
                    tree.layout(SizeProposal::exact(800.0, 600.0));
                }));
                if let Err(err) = result {
                    let msg = err
                        .downcast_ref::<&'static str>()
                        .map(|s| s.to_string())
                        .or_else(|| err.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "<unknown panic>".to_string());
                    failures.push(format!(
                        "[{}] {}: {}",
                        pass.label(),
                        snippet.source_file,
                        msg
                    ));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "the following documentation snippets panicked during layout:\n  {}",
            failures.join("\n  "),
        );
    }

    /// Every snippet must name a source file that actually exists — the
    /// path is what files the image under `docs/widgets/<stem>.md`, so a
    /// typo silently produces an orphan PNG.
    #[test]
    fn doc_snippet_source_paths_exist() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root");
        for snippet in teksilo_preview::iter_doc_snippets() {
            let path = root.join(snippet.source_file);
            assert!(
                path.exists(),
                "doc snippet references a missing source file: {}",
                snippet.source_file
            );
        }
    }

    #[test]
    fn knob_overrides_apply_to_runtime_values() {
        let entry = find_by_id("button").unwrap();
        let variants = entry.variants();
        let disabled = variants
            .into_iter()
            .find(|v| v.name() == "disabled")
            .expect("button has a 'disabled' variant");
        if let teksilo_preview::PreviewVariant::Knobs { overrides, .. } = disabled {
            let knobs = teksilo_preview::KnobValues::from_spec(&entry.knobs(), Some(&overrides));
            assert!(
                !knobs.bool_("enabled").get(),
                "disabled variant should set enabled=false"
            );
        } else {
            panic!("expected Knobs variant");
        }
    }

    /// Regression — clicking a navigator item that maps to a different
    /// widget whose variants share a name with the previous widget's
    /// variants used to panic with "RefCell already borrowed".
    /// The cause was the inspector's variant-radio bridge calling
    /// `selected_variant.set` recursively while a borrow was still
    /// held; the fix is the equality guard on the forward observer.
    /// We don't have a Widget tree to drive here, but we can at least
    /// confirm that observer-chain-style equality guards fire
    /// correctly.
    #[test]
    fn signal_observer_chain_does_not_panic_on_shared_variant_names() {
        use teksilo_core::signal::Signal;

        let names_a: Vec<&'static str> = vec!["default", "primary"];
        let names_b: Vec<&'static str> = vec!["primary", "default"];

        let selected_name: Signal<Option<&'static str>> = Signal::new(Some("default"));
        let idx_sig: Signal<usize> = Signal::new(0);

        // Mirror the inspector's two observers, including the equality
        // guard on the forward direction that prevents the recursion.
        let names_for_forward = names_a.clone();
        let selected_for_forward = selected_name.clone();
        let _h_forward = idx_sig.observe(move |i| {
            if let Some(name) = names_for_forward.get(*i) {
                let new_val = Some(*name);
                if selected_for_forward.get() != new_val {
                    selected_for_forward.set(new_val);
                }
            }
        });
        let names_for_reverse = names_a.clone();
        let idx_for_reverse = idx_sig.clone();
        let _h_reverse = selected_name.observe(move |opt| {
            if let Some(target) = opt
                .as_ref()
                .and_then(|n| names_for_reverse.iter().position(|m| m == n))
                && idx_for_reverse.get() != target
            {
                idx_for_reverse.set(target);
            }
        });

        // Simulate clicking a navigator item whose first variant
        // shares a name with one of A's variants but at a different
        // index. The bridge must converge without panicking.
        selected_name.set(Some(names_b[0]));
        assert_eq!(selected_name.get(), Some("primary"));
        // The reverse observer fires, finds "primary" at A-index 1,
        // sets idx_sig to 1. The forward observer then sees i=1, name
        // = "primary", and the equality guard sees selected_name is
        // already "primary" → no recursive set → no panic.
        assert_eq!(idx_sig.get(), 1);
    }
}
