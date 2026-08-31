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
//! cargo run -p teksilo-widgets-previewer -- --list
//! cargo run -p teksilo-widgets-previewer -- --export-docs
//! ```
//!
//! `--export-docs` is the headless batch that fills `docs/widgets/img/`
//! with the pictures the generated mdBook catalog pages reference. It
//! needs a wgpu adapter but no display server.
//!
//! The binary intentionally has no logic beyond delegation —
//! everything happens inside `teksilo_preview_ui::run_previewer`.
//! Downstream applications create their own analogous thin binary that
//! links their own widget set with the `preview` feature.

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    if argv
        .iter()
        .any(|a| a == "--export-docs" || a.starts_with("--export-docs="))
    {
        std::process::exit(run_doc_export(&argv[1..]));
    }
    if argv.iter().any(|a| a == "--list") {
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
    let opts = teksilo_preview_ui::PreviewerOptions::from_args();
    teksilo_preview_ui::run_previewer(opts);
}

/// Headless documentation-image export.
///
/// ```text
/// --export-docs[=OUT_DIR]   default: docs/widgets/img
/// --pages=DIR               catalog pages to check against (default: docs/widgets)
/// --all-subjects            write images even for slugs with no catalog page
/// --dark                    render the dark theme instead of light
/// --scale=N                 HiDPI factor (default 2)
/// --only=slug[,slug...]     restrict the batch
/// ```
fn run_doc_export(args: &[String]) -> i32 {
    let mut opts = teksilo_preview_ui::DocExportOptions::default();
    for arg in args {
        if let Some(dir) = arg.strip_prefix("--export-docs=") {
            opts.out_dir = std::path::PathBuf::from(dir);
        } else if let Some(dir) = arg.strip_prefix("--pages=") {
            opts.pages_dir = Some(std::path::PathBuf::from(dir));
        } else if arg == "--all-subjects" {
            opts.pages_dir = None;
        } else if arg == "--dark" {
            opts.dark = true;
        } else if let Some(scale) = arg.strip_prefix("--scale=") {
            match scale.parse::<f32>() {
                Ok(s) if s > 0.0 => opts.scale = s,
                _ => {
                    eprintln!("teksilo-previewer: invalid --scale '{}'", scale);
                    return 2;
                }
            }
        } else if let Some(list) = arg.strip_prefix("--only=") {
            opts.only = list.split(',').map(str::to_string).collect();
        } else if arg != "--export-docs" {
            eprintln!(
                "teksilo-previewer: unrecognised argument '{}' for --export-docs",
                arg
            );
            return 2;
        }
    }

    println!("Exporting catalog images to {} …", opts.out_dir.display());
    match teksilo_preview_ui::export_doc_images(&opts) {
        Ok(report) => teksilo_preview_ui::print_report(&report, &opts),
        Err(e) => {
            eprintln!("teksilo-previewer: {}", e);
            1
        }
    }
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
        for entry in iter_entries() {
            for variant in entry.variants() {
                let label = format!("{}/{}", entry.id(), variant.name());
                // Run each (widget, variant) pair in a separate
                // `catch_unwind` so one failure doesn't prevent the
                // rest from being checked — the failure list at the
                // end is more useful than a single first-failure stack.
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let knobs = teksilo_preview::KnobValues::from_spec(&entry.knobs(), None);
                    let widget = entry.build(variant.name(), &knobs);
                    let mut tree =
                        WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        assert!(
            failures.is_empty(),
            "the following catalog (widget, variant) pairs panicked during \
             layout:\n  {}",
            failures.join("\n  "),
        );
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
        for snippet in teksilo_preview::iter_doc_snippets() {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let widget = (snippet.build)();
                let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
                let _ = tree.add_boxed(widget);
                tree.layout(SizeProposal::exact(800.0, 600.0));
            }));
            if let Err(err) = result {
                let msg = err
                    .downcast_ref::<&'static str>()
                    .map(|s| s.to_string())
                    .or_else(|| err.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "<unknown panic>".to_string());
                failures.push(format!("{}: {}", snippet.source_file, msg));
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
