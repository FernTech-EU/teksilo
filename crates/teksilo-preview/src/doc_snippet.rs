// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Documentation snippets — a one-line registration for widgets that
//! want a picture in the generated mdBook catalog without carrying a
//! full [`WidgetCatalog`](crate::WidgetCatalog) impl.
//!
//! A catalog entry is a *live* previewer subject: it declares an id, a
//! group, typed knobs, and a set of variants, because the previewer GUI
//! builds an editing form out of them. A documentation image needs none
//! of that — one representative instance is the whole requirement. So a
//! widget that would otherwise go unpictured registers a snippet:
//!
//! ```ignore
//! use teksilo_preview::doc_snippet;
//!
//! doc_snippet!("crates/teksilo-widgets/src/banner.rs", {
//!     Box::new(Banner::info(lit!("Your trial ends in 3 days.")))
//! });
//! ```
//!
//! The exporter keys images by the **source file** (its stem is the
//! catalog page's slug), so the path decides which documentation page
//! the image lands on — exactly like
//! [`register_widget_catalog_at!`](crate::register_widget_catalog_at).
//!
//! A widget that fills whatever space it is given (a data view, a
//! docking layout, a scroll area) reports no useful intrinsic size, so
//! it pins the canvas:
//!
//! ```ignore
//! doc_snippet!("crates/teksilo-widgets/src/table_view.rs", size = (640.0, 240.0), {
//!     Box::new(build_sample_table())
//! });
//! ```
//!
//! Snippets take precedence over a catalog entry for the same file: the
//! catalog's default variant is chosen for the previewer's benefit, and
//! a few of them (`Spacer`, `Expand`) paint nothing at all.

use teksilo_core::widget::Widget;

/// One registered documentation image subject.
pub struct DocSnippet {
    /// Workspace-relative path of the widget's own source file. Its stem
    /// is the catalog page slug the image is filed under.
    pub source_file: &'static str,
    /// Constructs a fresh instance. A plain `fn` (not a closure) so the
    /// whole record is a `const`-constructible static.
    pub build: fn() -> Box<dyn Widget>,
    /// Pin the canvas to this logical size instead of measuring the
    /// widget's intrinsic size. For widgets that fill their parent.
    pub size: Option<(f32, f32)>,
}

inventory::collect!(DocSnippet);

/// Iterate every documentation snippet registered into the current
/// binary's link graph.
pub fn iter_doc_snippets() -> impl Iterator<Item = &'static DocSnippet> {
    inventory::iter::<DocSnippet>()
}

/// Register a documentation image subject for a widget source file.
///
/// ```ignore
/// doc_snippet!("crates/teksilo-widgets/src/banner.rs", { Box::new(Banner::info(lit!("Hi"))) });
/// doc_snippet!("crates/teksilo-widgets/src/log_view.rs", size = (620.0, 220.0), { Box::new(v) });
/// ```
#[macro_export]
macro_rules! doc_snippet {
    ($file:literal, size = ($w:expr, $h:expr), $build:block) => {
        $crate::__doc_snippet_with!($file, ::std::option::Option::Some(($w, $h)), $build);
    };
    ($file:literal, $build:block) => {
        $crate::__doc_snippet_with!($file, ::std::option::Option::None, $build);
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __doc_snippet_with {
    ($file:literal, $size:expr, $build:block) => {
        const _: () = {
            fn __build() -> ::std::boxed::Box<dyn $crate::__widget::Widget> {
                $build
            }
            $crate::__inventory::submit! {
                $crate::DocSnippet {
                    source_file: $file,
                    build: __build,
                    size: $size,
                }
            }
        };
    };
}
