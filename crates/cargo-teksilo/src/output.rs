// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Shared output preferences. Payloads are never suppressed by quiet mode.
use std::sync::OnceLock;
#[derive(Default)]
pub struct Options {
    pub quiet: bool,
    pub verbose: bool,
    pub json: bool,
}
static OPTIONS: OnceLock<Options> = OnceLock::new();
pub fn set(options: Options) {
    let _ = OPTIONS.set(options);
}
pub fn quiet() -> bool {
    OPTIONS.get().is_some_and(|o| o.quiet)
}
pub fn verbose() -> bool {
    OPTIONS.get().is_some_and(|o| o.verbose)
}
pub fn json() -> bool {
    OPTIONS.get().is_some_and(|o| o.json)
}
pub fn note(message: impl std::fmt::Display) {
    if !quiet() {
        eprintln!("{message}");
    }
}
