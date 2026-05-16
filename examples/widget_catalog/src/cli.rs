//! Hand-rolled CLI parser for the widget catalog.
//!
//! Three flags, ~20 lines of logic — clap is overkill.
//!
//! Usage:
//!
//! ```text
//! cargo run -p widget-catalog -- [--tab NAME|INDEX] [--cycle [MS]] [--mode classic|fern]
//! ```

use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct CliOptions {
    /// Index of the tab to open on startup.
    pub initial_tab: usize,
    /// If `Some`, auto-advance the selected tab on this interval.
    pub cycle: Option<Duration>,
    /// `false` → start in classic builder view; `true` → start in `fern!` view.
    pub fern_mode: bool,
}

/// Parse command-line args. On `--help`, prints usage and exits 0.
/// Unknown flags print a warning to stderr and are ignored.
///
/// `tab_names` is the canonical lowercase list of tab names — used to
/// resolve `--tab NAME` to an index. Names are matched case-insensitively
/// against the lowercase form.
pub fn parse(tab_names: &[&str]) -> CliOptions {
    let mut opts = CliOptions::default();
    let mut iter = std::env::args().skip(1).peekable();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                print_help(tab_names);
                std::process::exit(0);
            }
            "--tab" => {
                let Some(value) = iter.next() else {
                    eprintln!("--tab expects a NAME or INDEX argument");
                    continue;
                };
                opts.initial_tab = resolve_tab(&value, tab_names).unwrap_or_else(|| {
                    eprintln!(
                        "--tab: unknown tab `{value}` (known: {})",
                        tab_names.join(", ")
                    );
                    0
                });
            }
            "--cycle" => {
                // Accept either `--cycle` (default 100 ms) or `--cycle 1500`.
                // Only consume the next arg if it parses as an integer.
                let ms = iter
                    .peek()
                    .and_then(|s| s.parse::<u64>().ok())
                    .inspect(|_| {
                        iter.next();
                    })
                    .unwrap_or(100);
                opts.cycle = Some(Duration::from_millis(ms));
            }
            "--mode" => {
                let Some(value) = iter.next() else {
                    eprintln!("--mode expects `classic` or `fern`");
                    continue;
                };
                match value.as_str() {
                    "classic" => opts.fern_mode = false,
                    "fern" => opts.fern_mode = true,
                    other => eprintln!("--mode: expected `classic` or `fern`, got `{other}`"),
                }
            }
            other if other.starts_with("--") => {
                eprintln!("widget-catalog: ignoring unknown flag `{other}`");
            }
            other => {
                eprintln!("widget-catalog: ignoring positional arg `{other}`");
            }
        }
    }

    opts
}

fn resolve_tab(value: &str, tab_names: &[&str]) -> Option<usize> {
    if let Ok(idx) = value.parse::<usize>() {
        return (idx < tab_names.len()).then_some(idx);
    }
    let needle = value.to_ascii_lowercase();
    tab_names.iter().position(|n| *n == needle)
}

fn print_help(tab_names: &[&str]) {
    println!(
        "FernUI Widget Catalog — every public widget, classic vs fern! side-by-side.\n\
         \n\
         USAGE:\n  \
           widget-catalog [OPTIONS]\n\
         \n\
         OPTIONS:\n  \
           --tab <NAME|INDEX>   Open the catalog directly on this tab.\n  \
           --cycle [MS]         Auto-advance the selected tab every MS milliseconds\n  \
                                (default 100). Stops on user interaction.\n  \
           --mode <classic|fern>  Initial view mode (default `classic`).\n  \
           --help, -h           Show this help and exit.\n\
         \n\
         TABS:\n  \
           {}\n",
        tab_names.join(", ")
    );
}
