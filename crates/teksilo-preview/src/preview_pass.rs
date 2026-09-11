// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`PreviewPass`] — the environment one preview render happens in.
//!
//! A [`WidgetCatalog`](crate::WidgetCatalog) entry and a
//! [`DocSnippet`](crate::DocSnippet) both describe *what* to build. This
//! type describes the pass that builds it, and today that is exactly one
//! thing: the [`TargetDensity`] the tree and theme are projected onto.
//! The same subject rendered at `Compact` and at `Touch` is two different
//! pictures — 24 dp targets against 44 dp ones — and the widget itself
//! says nothing about which, because density is a property of the host.
//!
//! # Why the naming rule lives here
//!
//! The exporter writes one file per (subject, pass), and
//! `tools/extract_widget_api.py` references a file only when it exists.
//! For that to work the two sides must agree on the filename, so the rule
//! sits beside the slug rule it extends (a page slug is the widget's
//! source-file stem — see [`DocSnippet::source_file`](crate::DocSnippet)).
//!
//! `Compact` is the **canonical** pass: it is what every committed
//! catalog image has always been, so it carries no suffix and its
//! filenames never move. Every other density is **additive** —
//! `img/<slug>-touch.png` lands beside `img/<slug>.png`, never instead of
//! it.

use teksilo_tokens::TargetDensity;

/// One preview render pass.
///
/// ```
/// use teksilo_preview::PreviewPass;
/// use teksilo_tokens::TargetDensity;
///
/// assert_eq!(PreviewPass::default().image_stem("button"), "button");
/// assert_eq!(
///     PreviewPass::new(TargetDensity::Touch).image_stem("button"),
///     "button-touch"
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PreviewPass {
    density: TargetDensity,
}

impl PreviewPass {
    /// A pass at `density`.
    pub const fn new(density: TargetDensity) -> Self {
        Self { density }
    }

    /// The canonical pass: [`TargetDensity::Compact`], unsuffixed.
    pub const fn compact() -> Self {
        Self::new(TargetDensity::Compact)
    }

    /// The density this pass projects the theme and tree onto.
    pub const fn density(self) -> TargetDensity {
        self.density
    }

    /// Parse the lowercase density name [`label`](Self::label) prints —
    /// `compact` | `comfortable` | `touch`, case- and space-insensitive.
    /// `None` for anything else, so a CLI can report the bad value itself
    /// rather than have a library exit the process.
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "compact" => Some(Self::new(TargetDensity::Compact)),
            "comfortable" => Some(Self::new(TargetDensity::Comfortable)),
            "touch" => Some(Self::new(TargetDensity::Touch)),
            _ => None,
        }
    }

    /// The lowercase name of the density, for logs and report lines.
    pub const fn label(self) -> &'static str {
        match self.density {
            TargetDensity::Compact => "compact",
            TargetDensity::Comfortable => "comfortable",
            TargetDensity::Touch => "touch",
        }
    }

    /// The filename suffix images from this pass carry: empty for the
    /// canonical `Compact` pass, `-comfortable` / `-touch` otherwise.
    pub const fn image_suffix(self) -> &'static str {
        match self.density {
            TargetDensity::Compact => "",
            TargetDensity::Comfortable => "-comfortable",
            TargetDensity::Touch => "-touch",
        }
    }

    /// The image file stem for a page `slug` rendered in this pass, with
    /// no extension. `Compact` returns the slug unchanged, which is what
    /// keeps the committed images' filenames stable.
    pub fn image_stem(self, slug: &str) -> String {
        let mut stem = String::with_capacity(slug.len() + 11);
        stem.push_str(slug);
        stem.push_str(self.image_suffix());
        stem
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole additive contract in one assertion: `Compact` must not
    /// move a filename, every other density must not collide with it.
    #[test]
    fn compact_is_unsuffixed_and_every_other_density_is_additive() {
        assert_eq!(PreviewPass::compact().image_suffix(), "");
        assert_eq!(
            PreviewPass::compact().image_stem("color_picker"),
            "color_picker"
        );

        let mut stems = vec![PreviewPass::compact().image_stem("color_picker")];
        for density in [TargetDensity::Comfortable, TargetDensity::Touch] {
            let stem = PreviewPass::new(density).image_stem("color_picker");
            assert!(
                stem.starts_with("color_picker-"),
                "{density:?} must be additive, got {stem}"
            );
            stems.push(stem);
        }
        let unique: std::collections::BTreeSet<_> = stems.iter().collect();
        assert_eq!(
            unique.len(),
            stems.len(),
            "two densities collided: {stems:?}"
        );
    }

    /// `label` and `from_name` are two halves of one table; a name that
    /// does not round-trip is a pass a CLI can print but never ask for.
    #[test]
    fn every_label_parses_back_to_its_own_pass() {
        for density in [
            TargetDensity::Compact,
            TargetDensity::Comfortable,
            TargetDensity::Touch,
        ] {
            let pass = PreviewPass::new(density);
            assert_eq!(
                PreviewPass::from_name(pass.label()),
                Some(pass),
                "{density:?} does not round-trip through its label"
            );
        }
        assert_eq!(
            PreviewPass::from_name("  TOUCH "),
            Some(PreviewPass::new(TargetDensity::Touch))
        );
        assert_eq!(PreviewPass::from_name("dense"), None);
    }

    #[test]
    fn the_default_pass_is_compact() {
        assert_eq!(PreviewPass::default(), PreviewPass::compact());
        assert_eq!(PreviewPass::default().density(), TargetDensity::Compact);
        assert_eq!(PreviewPass::default().label(), "compact");
    }
}
