// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Hand-rolled CLI parser for the widget catalog.
//!
//! A handful of flags, ~30 lines of logic — clap is overkill.
//!
//! Usage:
//!
//! ```text
//! cargo run -p widget-catalog -- [--tab NAME|INDEX] [--cycle [MS] | --cycle-ms MS] [--mode classic|teksu]
//! ```

use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct CliOptions {
    /// Index of the tab to open on startup.
    pub initial_tab: usize,
    /// If `Some`, auto-advance the selected tab on this interval.
    pub cycle: Option<Duration>,
    /// `false` → start in classic builder view; `true` → start in `teksu!` view.
    pub teksi_mode: bool,
    /// Force the startup theme, overriding the persisted selection. One of
    /// `intui-light` / `intui-dark` / `material3-light` / `material3-dark`
    /// (aliases `m3-light` / `m3-dark`). `None` → restore the saved theme.
    pub theme: Option<String>,
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
            "--cycle-ms" => {
                // Explicit form of `--cycle <MS>`: the interval is *mandatory*,
                // not a peeked positional. Preferred by scripts that drive the
                // cycle for timed captures (tools/screenshot_examples.py), where
                // a silent fall-back to the 100 ms default would desync the
                // screenshots from the tab they're meant to show.
                let Some(value) = iter.next() else {
                    eprintln!("--cycle-ms expects a millisecond INTEGER argument");
                    continue;
                };
                match value.parse::<u64>() {
                    Ok(ms) => opts.cycle = Some(Duration::from_millis(ms)),
                    Err(_) => eprintln!("--cycle-ms: expected an integer, got `{value}`"),
                }
            }
            "--mode" => {
                let Some(value) = iter.next() else {
                    eprintln!("--mode expects `classic` or `teksu`");
                    continue;
                };
                match value.as_str() {
                    "classic" => opts.teksi_mode = false,
                    "teksu" => opts.teksi_mode = true,
                    other => eprintln!("--mode: expected `classic` or `teksu`, got `{other}`"),
                }
            }
            "--theme" => {
                let Some(value) = iter.next() else {
                    eprintln!(
                        "--theme expects one of: intui-light, intui-dark, \
                         material3-light, material3-dark"
                    );
                    continue;
                };
                opts.theme = Some(value.to_ascii_lowercase());
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
        "Teksilo Widget Catalog — every public widget, classic vs teksu! side-by-side.\n\
         \n\
         USAGE:\n  \
           widget-catalog [OPTIONS]\n\
         \n\
         OPTIONS:\n  \
           --tab <NAME|INDEX>   Open the catalog directly on this tab.\n  \
           --cycle [MS]         Auto-advance the selected tab every MS milliseconds\n  \
                                (default 100). Stops on user interaction.\n  \
           --cycle-ms <MS>      Like --cycle, but MS is mandatory. Script-friendly\n  \
                                (no silent fall-back to the default interval).\n  \
           --mode <classic|teksu>  Initial view mode (default `classic`).\n  \
           --theme <NAME>       Force the startup theme, overriding the saved one.\n  \
                                intui-light | intui-dark | material3-light | material3-dark\n  \
                                (aliases m3-light / m3-dark).\n  \
           --help, -h           Show this help and exit.\n\
         \n\
         TABS:\n  \
           {}\n",
        tab_names.join(", ")
    );
}
