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
/// A note about where an answer came from, printed even under `--quiet`.
///
/// Which release answered is part of the answer. A caller that turned on
/// `--quiet` to save output still has to know that the text it is reading
/// belongs to a release other than the one its app builds against, and has
/// no other way to learn it.
pub fn provenance(message: impl std::fmt::Display) {
    eprintln!("{message}");
}
