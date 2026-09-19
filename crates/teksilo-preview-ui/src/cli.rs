// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Command-line argument parsing for previewer binaries.
//!
//! The expected shape — wired up by each consuming binary's `main()`:
//!
//! ```bash
//! my-previewer                                      # browse mode
//! my-previewer --widget=button                      # focus a widget
//! my-previewer --widget=button --variant=disabled   # focus widget+variant
//! my-previewer --file=path/to/button.rs             # focus file's widget
//! my-previewer --window=1600x900 --title="Custom"   # window overrides
//! my-previewer --density=touch                      # preview at Touch density
//! ```
//!
//! Two types, because a previewer binary is rarely *only* a previewer: this
//! crate's own has `--list` and `--export-docs` beside these flags.
//! [`PreviewerArgs`] is the clap fragment — `#[command(flatten)]` it into a
//! binary's own `Parser` — and [`PreviewerOptions`] is the resolved
//! configuration [`crate::run_previewer`] takes. A binary with nothing to add
//! skips the fragment and calls [`PreviewerOptions::from_args`].
//!
//! Resolution is deliberately *not* in the value parsers: `--file` and
//! `--widget` are checked against the live `inventory` registry, which a unit
//! test parsing arguments has not linked. That check lives in
//! [`PreviewerOptions::from_parsed`], where it can be skipped when there is no
//! catalog to check against.

use clap::{Args, Parser};

use teksilo_preview::find_by_file;

/// The previewer's own flags, as a clap fragment.
///
/// Field types are the *parsed* values, not strings, so a malformed
/// `--window` or `--density` is reported by clap with the rest of the usage
/// error rather than by a hand-written `eprintln` halfway through the loop.
#[derive(Debug, Clone, Args)]
pub struct PreviewerArgs {
    /// Focus the named widget at startup.
    #[arg(long, value_name = "ID", conflicts_with = "file")]
    pub widget: Option<String>,

    /// Combine with --widget to focus one of its variants.
    #[arg(long, value_name = "NAME")]
    pub variant: Option<String>,

    /// Focus whichever widget registered a catalog entry from this source
    /// file (suffix match).
    #[arg(long, value_name = "PATH")]
    pub file: Option<String>,

    /// Override the initial window size (default 1400x900).
    #[arg(long, value_name = "WxH", value_parser = parse_window_size)]
    pub window: Option<(u32, u32)>,

    /// Override the window title.
    #[arg(long, value_name = "TEXT")]
    pub title: Option<String>,

    /// Start at compact | comfortable | touch (default compact).
    ///
    /// A comma-separated list is accepted for the sake of a batch export that
    /// runs one pass per density; the previewer window itself starts at one,
    /// and says so rather than silently picking.
    #[arg(long, value_name = "NAME", value_delimiter = ',', value_parser = parse_density)]
    pub density: Vec<teksilo_preview::PreviewPass>,
}

/// `WIDTHxHEIGHT`, as `--window` spells it.
fn parse_window_size(raw: &str) -> Result<(u32, u32), String> {
    let (w, h) = raw
        .split_once('x')
        .ok_or_else(|| format!("'{raw}' must be WIDTHxHEIGHT (e.g. 1600x900)"))?;
    let w: u32 = w
        .parse()
        .map_err(|_| format!("invalid width '{w}' in '{raw}'"))?;
    let h: u32 = h
        .parse()
        .map_err(|_| format!("invalid height '{h}' in '{raw}'"))?;
    Ok((w, h))
}

/// One density name. The table lives in `PreviewPass`, so `--density` and the
/// image suffixes it decides cannot drift apart.
fn parse_density(raw: &str) -> Result<teksilo_preview::PreviewPass, String> {
    teksilo_preview::PreviewPass::from_name(raw)
        .ok_or_else(|| format!("unknown density '{raw}' (compact | comfortable | touch)"))
}

/// A previewer binary that adds no flags of its own.
///
/// Private: a binary that wants more flags flattens [`PreviewerArgs`] into its
/// own `Parser` instead, which is what `teksilo-widgets-previewer` does.
#[derive(Parser)]
#[command(
    about = "Teksilo Widget Previewer",
    long_about = "Browse a Teksilo widget catalog: navigator, live canvas and knob form."
)]
struct StandaloneCli {
    #[command(flatten)]
    args: PreviewerArgs,
}

/// Configured options for [`crate::run_previewer`].
#[derive(Debug, Clone)]
pub struct PreviewerOptions {
    pub window_title: String,
    pub window_size: (u32, u32),
    pub initial_widget: Option<String>,
    pub initial_variant: Option<String>,
    /// Density the previewer starts at. Applied to the window's tree at
    /// creation, which is the one place an app can set a density outright —
    /// see `crate::app_state::PreviewerRoot` for the live switch.
    pub density: teksilo_tokens::TargetDensity,
}

impl Default for PreviewerOptions {
    fn default() -> Self {
        Self {
            window_title: "Teksilo Widget Previewer".to_string(),
            window_size: (1400, 900),
            initial_widget: None,
            initial_variant: None,
            density: teksilo_tokens::TargetDensity::Compact,
        }
    }
}

impl PreviewerOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.window_title = title.into();
        self
    }

    pub fn size(mut self, width: u32, height: u32) -> Self {
        self.window_size = (width, height);
        self
    }

    pub fn widget(mut self, id: impl Into<String>) -> Self {
        self.initial_widget = Some(id.into());
        self
    }

    pub fn variant(mut self, name: impl Into<String>) -> Self {
        self.initial_variant = Some(name.into());
        self
    }

    /// Start at `density` instead of `Compact`.
    pub fn density(mut self, density: teksilo_tokens::TargetDensity) -> Self {
        self.density = density;
        self
    }

    /// Parse from `std::env::args`. Exits the process on `--help` or on
    /// any malformed argument (after printing a brief usage block).
    pub fn from_args() -> Self {
        Self::from_parsed(StandaloneCli::parse().args)
    }

    /// Parse from any iterator of `String`-like tokens. Used by tests
    /// and by `from_args`.
    ///
    /// The tokens are the arguments *after* the program name — clap wants
    /// `argv[0]` too, so one is supplied here rather than at every call site.
    #[allow(clippy::should_implement_trait)]
    pub fn from_iter<I, S>(iter: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let argv = std::iter::once("previewer".to_string()).chain(iter.into_iter().map(Into::into));
        Self::from_parsed(StandaloneCli::parse_from(argv).args)
    }

    /// Resolve already-parsed flags into options, against the live registry.
    ///
    /// Separate from parsing because a binary with extra flags of its own
    /// parses once, for everything, and hands the fragment here.
    pub fn from_parsed(args: PreviewerArgs) -> Self {
        let mut opts = Self {
            initial_widget: args.widget,
            initial_variant: args.variant,
            ..Self::default()
        };
        if let Some(path) = args.file {
            match find_by_file(&path) {
                Some(entry) => opts.initial_widget = Some(entry.id().to_string()),
                None => {
                    eprintln!(
                        "teksilo-previewer: no widget catalog entry registered \
                         from file matching '{}'",
                        path
                    );
                    std::process::exit(2);
                }
            }
        }
        if let Some((w, h)) = args.window {
            opts.window_size = (w, h);
        }
        if let Some(title) = args.title {
            opts.window_title = title;
        }
        match args.density.as_slice() {
            [] => {}
            [pass] => opts.density = pass.density(),
            many => {
                // A list is meaningful for a batch that renders one pass per
                // density; a window can only be at one of them, and picking
                // silently would make `--density=compact,touch` look like it
                // had done something it had not.
                eprintln!(
                    "teksilo-previewer: --density names {} densities ({}); the previewer \
                     window starts at exactly one.",
                    many.len(),
                    many.iter()
                        .map(|p| p.label())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                std::process::exit(2);
            }
        }
        // Validate the resolved widget/variant against the live
        // registry now (rather than silently falling back at runtime
        // when `PreviewerRoot::build` can't find them). Catches typos
        // and case mismatches at parse time.
        opts.validate_against_registry();
        opts
    }

    fn validate_against_registry(&mut self) {
        // No catalog registered at all (unit tests parsing args, or a binary
        // that hasn't linked its widget set): there's nothing to validate
        // against, and exiting the process here would abort those callers.
        // Defer to runtime resolution instead.
        if teksilo_preview::iter_entries().next().is_none() {
            return;
        }
        // Skip validation if no widget supplied at all.
        let want_id = match &self.initial_widget {
            Some(id) => id.clone(),
            None => {
                if self.initial_variant.is_some() {
                    eprintln!(
                        "teksilo-previewer: --variant requires --widget to also be \
                         specified."
                    );
                    std::process::exit(2);
                }
                return;
            }
        };

        let entry = teksilo_preview::find_by_id(&want_id);
        if entry.is_none() {
            // Build a hint listing available widget ids.
            let available: Vec<&'static str> =
                teksilo_preview::iter_entries().map(|e| e.id()).collect();
            eprintln!(
                "teksilo-previewer: no widget registered with id '{}'.",
                want_id
            );
            if !available.is_empty() {
                let suggestion = closest_match(&want_id, &available);
                if let Some(s) = suggestion {
                    eprintln!("              did you mean '{}' ?", s);
                }
                eprintln!("\nAvailable widget ids:");
                let mut sorted = available.clone();
                sorted.sort();
                for id in sorted {
                    eprintln!("    {}", id);
                }
            }
            std::process::exit(2);
        }

        if let Some(want_variant) = &self.initial_variant {
            let entry = entry.expect("entry.is_none() exits the process above");
            let variants = entry.variants();
            let names: Vec<&'static str> = variants.iter().map(|v| v.name()).collect();
            if !names.contains(&want_variant.as_str()) {
                eprintln!(
                    "teksilo-previewer: widget '{}' has no variant named '{}'.",
                    want_id, want_variant
                );
                eprintln!("\nAvailable variants for '{}':", want_id);
                for n in &names {
                    eprintln!("    {}", n);
                }
                std::process::exit(2);
            }
        }
    }
}

/// Tiny case-insensitive substring / Levenshtein-ish suggester used by
/// the CLI to point a user at the right widget id when they typo.
fn closest_match<'a>(query: &str, options: &[&'a str]) -> Option<&'a str> {
    let q = query.to_lowercase();
    // 1) Exact case-insensitive match.
    if let Some(o) = options.iter().find(|o| o.eq_ignore_ascii_case(query)) {
        return Some(*o);
    }
    // 2) Substring match in either direction.
    if let Some(o) = options.iter().find(|o| {
        let l = o.to_lowercase();
        l.contains(&q) || q.contains(&l)
    }) {
        return Some(*o);
    }
    // 3) Closest by simple character-overlap heuristic.
    options
        .iter()
        .min_by_key(|o| {
            let l = o.to_lowercase();
            let common = l.chars().filter(|c| q.contains(*c)).count();
            (l.len() as i32 - common as i32).unsigned_abs()
        })
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_window_size() {
        let opts = PreviewerOptions::default();
        assert_eq!(opts.window_size, (1400, 900));
        assert!(opts.initial_widget.is_none());
    }

    #[test]
    fn parses_widget_and_variant_args() {
        let opts = PreviewerOptions::from_iter(["--widget=button", "--variant=disabled"]);
        assert_eq!(opts.initial_widget.as_deref(), Some("button"));
        assert_eq!(opts.initial_variant.as_deref(), Some("disabled"));
    }

    #[test]
    fn parses_window_size_arg() {
        let opts = PreviewerOptions::from_iter(["--window=1024x768"]);
        assert_eq!(opts.window_size, (1024, 768));
    }

    #[test]
    fn parses_density_arg_and_defaults_to_compact() {
        assert_eq!(
            PreviewerOptions::default().density,
            teksilo_tokens::TargetDensity::Compact
        );
        let opts = PreviewerOptions::from_iter(["--density=touch"]);
        assert_eq!(opts.density, teksilo_tokens::TargetDensity::Touch);
        let opts = PreviewerOptions::from_iter(["--density=Comfortable"]);
        assert_eq!(opts.density, teksilo_tokens::TargetDensity::Comfortable);
    }

    /// The flag surface is the fragment's, so the gate is clap's own
    /// consistency check over it — which catches a duplicated id or a
    /// `conflicts_with` naming an argument that does not exist, neither of
    /// which is a compile error.
    #[test]
    fn the_flag_surface_is_internally_consistent() {
        use clap::CommandFactory;
        StandaloneCli::command().debug_assert();
    }

    /// A malformed value is a usage error rather than a value silently
    /// standing in for the default.
    #[test]
    fn malformed_values_are_rejected_by_the_parser() {
        use clap::Parser;
        for bad in [
            "--window=1600",
            "--window=axb",
            "--window=1600x",
            "--density=dense",
        ] {
            assert!(
                StandaloneCli::try_parse_from(["previewer", bad]).is_err(),
                "'{bad}' should not parse"
            );
        }
    }

    /// `--widget` and `--file` are two ways to name one widget, so asking
    /// with both is a question the previewer cannot answer.
    #[test]
    fn widget_and_file_are_mutually_exclusive() {
        use clap::Parser;
        assert!(
            StandaloneCli::try_parse_from(["previewer", "--widget=button", "--file=button.rs"])
                .is_err()
        );
    }
}
