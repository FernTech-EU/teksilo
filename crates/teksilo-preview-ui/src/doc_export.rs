// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Batch export of catalog images for the generated mdBook widget pages.
//!
//! `tools/extract_widget_api.py` emits `![<Title> preview](img/<slug>.png)`
//! on a catalog page whenever `docs/widgets/img/<slug>.png` exists. This
//! module is the producer of those files.
//!
//! Two registries feed it, both keyed by the widget's **source file**
//! (whose stem is the page slug — the same rule
//! `extract_widget_api.py::_build_slugs` uses):
//!
//! - [`teksilo_preview::iter_entries`] — full `WidgetCatalog` impls. Their
//!   first variant is rendered with default knobs.
//! - [`teksilo_preview::iter_doc_snippets`] — one-line documentation-only
//!   registrations. A snippet **wins** over a catalog entry for the same
//!   file: it is authored for the page, whereas a catalog default variant
//!   is chosen for the previewer's editing form.
//!
//! Three guards keep the output honest:
//!
//! - a slug with no `<docs>/<slug>.md` page is reported, not written
//!   (no orphan images);
//! - a build/layout panic is caught per widget, so one bad subject can't
//!   abort the batch;
//! - a render whose ink ratio is below [`MIN_INK_RATIO`] is dropped — an
//!   invisible layout primitive publishing an empty rectangle is worse
//!   than no image at all.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use teksilo_core::styles::Theme;
use teksilo_core::widget::Widget;

use crate::shot::{Shooter, ShotOptions, write_png};

/// Minimum share of non-background pixels for an image to be published.
/// A `Spacer` renders at `0.0`; a lone 1 dp `Divider` on a 900 × 64 canvas
/// still lands well above this.
const MIN_INK_RATIO: f32 = 0.002;

/// Options for [`export_doc_images`].
#[derive(Debug, Clone)]
pub struct DocExportOptions {
    /// Where the images are written (`docs/widgets/img`).
    pub out_dir: PathBuf,
    /// Directory holding the generated catalog pages. A subject whose
    /// `<slug>.md` is missing there is reported and skipped. `None`
    /// writes every subject unconditionally.
    pub pages_dir: Option<PathBuf>,
    /// Render dark-theme images instead of the light default. The book's
    /// `default-theme` is light, so light is what the pages reference.
    pub dark: bool,
    /// HiDPI factor. 2× keeps the images crisp at the book's content width.
    pub scale: f32,
    /// Only export subjects whose slug is in this list (empty = all).
    pub only: Vec<String>,
}

impl Default for DocExportOptions {
    fn default() -> Self {
        Self {
            out_dir: PathBuf::from("docs/widgets/img"),
            pages_dir: Some(PathBuf::from("docs/widgets")),
            dark: false,
            scale: 2.0,
            only: Vec::new(),
        }
    }
}

/// Outcome for one subject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubjectOutcome {
    Written {
        bytes: u64,
        width: u32,
        height: u32,
    },
    /// Rendered, but essentially empty — not published.
    Blank,
    /// No `<slug>.md` catalog page to attach the image to.
    NoPage,
    Failed(String),
}

/// What the batch did, subject by subject (sorted by slug).
#[derive(Debug, Default)]
pub struct DocExportReport {
    pub results: Vec<(String, SubjectOutcome)>,
}

impl DocExportReport {
    pub fn written(&self) -> usize {
        self.count(|o| matches!(o, SubjectOutcome::Written { .. }))
    }
    pub fn blank(&self) -> usize {
        self.count(|o| matches!(o, SubjectOutcome::Blank))
    }
    pub fn no_page(&self) -> usize {
        self.count(|o| matches!(o, SubjectOutcome::NoPage))
    }
    pub fn failed(&self) -> usize {
        self.count(|o| matches!(o, SubjectOutcome::Failed(_)))
    }
    fn count(&self, f: impl Fn(&SubjectOutcome) -> bool) -> usize {
        self.results.iter().filter(|(_, o)| f(o)).count()
    }
}

/// One thing to render, resolved from either registry.
struct Subject {
    slug: String,
    build: Box<dyn Fn() -> Box<dyn Widget>>,
    exact_size: Option<(f32, f32)>,
}

/// Collect both registries into one slug-keyed set, snippets winning.
fn collect_subjects() -> Vec<Subject> {
    let mut by_slug: BTreeMap<String, Subject> = BTreeMap::new();

    // `inventory` yields entries in link order, which is not stable across
    // builds. Two entries can map to one slug (both `StandardListItem` and
    // `StandardTreeItem` live in `standard_item.rs`), so sort by id first
    // and let the lowest id win deterministically.
    let mut entries: Vec<_> = teksilo_preview::iter_entries().collect();
    entries.sort_by_key(|e| e.id());
    for entry in entries {
        let Some(slug) = slug_for(entry.source().file) else {
            continue;
        };
        // Build with the first variant's overrides and otherwise default
        // knobs — the same thing the previewer shows on selection.
        let variants = entry.variants();
        let variant_name = variants
            .first()
            .map(|v| v.name().to_string())
            .unwrap_or_else(|| "default".to_string());
        by_slug.entry(slug.clone()).or_insert_with(|| Subject {
            slug,
            build: Box::new(move || {
                let knobs = teksilo_preview::KnobValues::from_spec(&entry.knobs(), None);
                entry.build(&variant_name, &knobs)
            }),
            exact_size: None,
        });
    }

    for snippet in teksilo_preview::iter_doc_snippets() {
        let Some(slug) = slug_for(snippet.source_file) else {
            continue;
        };
        // Snippets win: insert unconditionally.
        by_slug.insert(
            slug.clone(),
            Subject {
                slug,
                build: Box::new(move || (snippet.build)()),
                exact_size: snippet.size,
            },
        );
    }

    by_slug.into_values().collect()
}

/// Page slug for a source path — its file stem, matching
/// `extract_widget_api.py::_build_slugs`.
fn slug_for(source_file: &str) -> Option<String> {
    Path::new(&source_file.replace('\\', "/"))
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
}

/// Render every registered subject into `opts.out_dir`.
pub fn export_doc_images(opts: &DocExportOptions) -> Result<DocExportReport, String> {
    let theme = if opts.dark {
        teksilo_core::presets::intui::dark()
    } else {
        teksilo_core::presets::intui::light()
    };
    let mut shooter = Shooter::new(opts.scale)?;
    let mut report = DocExportReport::default();

    for subject in collect_subjects() {
        if !opts.only.is_empty() && !opts.only.contains(&subject.slug) {
            continue;
        }
        if let Some(pages) = &opts.pages_dir
            && !pages.join(format!("{}.md", subject.slug)).exists()
        {
            report.results.push((subject.slug, SubjectOutcome::NoPage));
            continue;
        }

        let outcome = render_subject(&mut shooter, &subject, &theme, opts);
        report.results.push((subject.slug, outcome));
    }

    Ok(report)
}

fn render_subject(
    shooter: &mut Shooter,
    subject: &Subject,
    theme: &Theme,
    opts: &DocExportOptions,
) -> SubjectOutcome {
    let mut shot_opts = ShotOptions::default();
    if let Some((w, h)) = subject.exact_size {
        shot_opts = shot_opts.with_exact_size(w, h);
    }

    // A widget can panic deep inside `build()` when a required slot is
    // missing; one bad subject must not take the batch down with it.
    let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (subject.build)()));
    let widget = match built {
        Ok(w) => w,
        Err(e) => {
            return SubjectOutcome::Failed(format!("panicked while building: {}", panic_msg(&e)));
        }
    };

    let shot = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        shooter.capture(widget, theme.clone(), &shot_opts)
    }));
    let shot = match shot {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return SubjectOutcome::Failed(e),
        Err(e) => {
            return SubjectOutcome::Failed(format!("panicked while rendering: {}", panic_msg(&e)));
        }
    };

    if shot.ink_ratio < MIN_INK_RATIO {
        return SubjectOutcome::Blank;
    }

    let path = opts.out_dir.join(format!("{}.png", subject.slug));
    match write_png(&path, &shot.rgba, shot.width, shot.height) {
        Ok(()) => SubjectOutcome::Written {
            bytes: std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
            width: shot.width,
            height: shot.height,
        },
        Err(e) => SubjectOutcome::Failed(e),
    }
}

fn panic_msg(err: &Box<dyn std::any::Any + Send>) -> String {
    err.downcast_ref::<&'static str>()
        .map(|s| (*s).to_string())
        .or_else(|| err.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "<unknown panic>".to_string())
}

/// Print a human-readable summary and return a process exit code
/// (non-zero if any subject failed).
pub fn print_report(report: &DocExportReport, opts: &DocExportOptions) -> i32 {
    for (slug, outcome) in &report.results {
        match outcome {
            SubjectOutcome::Written {
                bytes,
                width,
                height,
            } => println!("  ✓ {slug:<26} {width}×{height}  {:>6} KiB", bytes / 1024),
            SubjectOutcome::Blank => println!("  · {slug:<26} skipped (renders blank)"),
            SubjectOutcome::NoPage => println!("  · {slug:<26} skipped (no catalog page)"),
            SubjectOutcome::Failed(e) => println!("  ✗ {slug:<26} {e}"),
        }
    }
    println!(
        "\n{} image(s) written to {}  ({} blank, {} without a page, {} failed)",
        report.written(),
        opts.out_dir.display(),
        report.blank(),
        report.no_page(),
        report.failed(),
    );
    if report.failed() > 0 { 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_is_the_source_file_stem() {
        assert_eq!(
            slug_for("crates/teksilo-widgets/src/button.rs").as_deref(),
            Some("button")
        );
        assert_eq!(
            slug_for("crates/teksilo-widgets/src/primitives/vstack.rs").as_deref(),
            Some("vstack")
        );
        assert_eq!(
            slug_for("crates\\teksilo-widgets\\src\\notification\\log.rs").as_deref(),
            Some("log")
        );
    }
}
