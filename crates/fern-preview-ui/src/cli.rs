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
//! ```
//!
//! Parsing is intentionally hand-rolled (no `clap`) — six flags, no
//! sub-commands, no external dependency. `--help` prints the usage and
//! exits.

use fern_preview::find_by_file;

/// Configured options for [`crate::run_previewer`].
#[derive(Debug, Clone)]
pub struct PreviewerOptions {
    pub window_title: String,
    pub window_size: (u32, u32),
    pub initial_widget: Option<String>,
    pub initial_variant: Option<String>,
}

impl Default for PreviewerOptions {
    fn default() -> Self {
        Self {
            window_title: "FernUI Widget Previewer".to_string(),
            window_size: (1400, 900),
            initial_widget: None,
            initial_variant: None,
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

    /// Parse from `std::env::args`. Exits the process on `--help` or on
    /// any malformed argument (after printing a brief usage block).
    pub fn from_args() -> Self {
        Self::from_iter(std::env::args().skip(1))
    }

    /// Parse from any iterator of `String`-like tokens. Used by tests
    /// and by `from_args`.
    #[allow(clippy::should_implement_trait)]
    pub fn from_iter<I, S>(iter: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut opts = Self::default();
        for arg in iter {
            let arg: String = arg.into();
            if let Some(value) = arg.strip_prefix("--widget=") {
                opts.initial_widget = Some(value.to_string());
            } else if let Some(value) = arg.strip_prefix("--variant=") {
                opts.initial_variant = Some(value.to_string());
            } else if let Some(value) = arg.strip_prefix("--file=") {
                match find_by_file(value) {
                    Some(entry) => opts.initial_widget = Some(entry.id().to_string()),
                    None => {
                        eprintln!(
                            "fern-previewer: no widget catalog entry registered \
                             from file matching '{}'",
                            value
                        );
                        std::process::exit(2);
                    }
                }
            } else if let Some(value) = arg.strip_prefix("--window=") {
                if let Some((w, h)) = value.split_once('x') {
                    let w = w.parse::<u32>().unwrap_or_else(|_| {
                        eprintln!("fern-previewer: invalid --window width '{}'", w);
                        std::process::exit(2);
                    });
                    let h = h.parse::<u32>().unwrap_or_else(|_| {
                        eprintln!("fern-previewer: invalid --window height '{}'", h);
                        std::process::exit(2);
                    });
                    opts.window_size = (w, h);
                } else {
                    eprintln!("fern-previewer: --window must be WIDTHxHEIGHT (e.g. 1600x900)");
                    std::process::exit(2);
                }
            } else if let Some(value) = arg.strip_prefix("--title=") {
                opts.window_title = value.to_string();
            } else if arg == "--help" || arg == "-h" {
                print_usage();
                std::process::exit(0);
            } else {
                eprintln!("fern-previewer: unrecognised argument '{}'", arg);
                print_usage();
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
        // Skip validation if no widget supplied at all.
        let want_id = match &self.initial_widget {
            Some(id) => id.clone(),
            None => {
                if self.initial_variant.is_some() {
                    eprintln!(
                        "fern-previewer: --variant requires --widget to also be \
                         specified."
                    );
                    std::process::exit(2);
                }
                return;
            }
        };

        let entry = fern_preview::find_by_id(&want_id);
        if entry.is_none() {
            // Build a hint listing available widget ids.
            let available: Vec<&'static str> =
                fern_preview::iter_entries().map(|e| e.id()).collect();
            eprintln!(
                "fern-previewer: no widget registered with id '{}'.",
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
                    "fern-previewer: widget '{}' has no variant named '{}'.",
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

fn print_usage() {
    eprintln!(
        "FernUI Widget Previewer\n\
         \n\
         USAGE:\n    \
             <previewer-binary> [OPTIONS]\n\
         \n\
         OPTIONS:\n    \
             --widget=<ID>          Focus the named widget at startup.\n    \
             --variant=<NAME>       Combine with --widget to focus a variant.\n    \
             --file=<PATH>          Focus whichever widget registered a catalog entry\n                            \
                                from the given source file (suffix match).\n    \
             --window=<WxH>         Override the initial window size (default 1400x900).\n    \
             --title=<TEXT>         Override the window title.\n    \
             -h, --help             Print this help text and exit.\n"
    );
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
}
