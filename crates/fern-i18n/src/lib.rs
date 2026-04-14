//! Internationalization runtime for FernUI.
//!
//! Implements §12 of `docs/fern-ui-architecture.md`. This crate is the
//! runtime half of the i18n stack: `LocalizedString`, `I18nConfig`,
//! `I18nManager`, locale resolution, and the thread-local that bridges
//! macro-expanded code into the active translation bundles.
//!
//! The compile-time `tr!` / `tr_widget!` proc macros live in the sister
//! `fern-i18n-macros` crate (Phase C) and are re-exported here as part of
//! `fern-i18n`'s public surface.

pub mod config;
pub mod direction;
pub mod file_watcher;
pub mod localized_string;
pub mod manager;
pub mod resolve;
pub mod thread_local;

pub use config::I18nConfig;
pub use direction::rtl_from_locale;
pub use fern_core::environment::LayoutDirection;
pub use fern_i18n_macros::{tr, tr_widget};
pub use file_watcher::{FtlFileWatcher, ReloadSink};
pub use fluent_bundle::FluentValue;
pub use localized_string::{LocalizedString, localized};
pub use manager::{I18nManager, LocaleSwitchOutcome, ReloadError};
pub use resolve::{resolve_message, resolve_message_widget};
pub use thread_local::{
    current_direction, current_locale, current_version_signal,
};
pub use unic_langid::LanguageIdentifier;

/// Declarative sugar for populating `I18nConfig::compile_in` when an
/// application ships many locales × many `.ftl` files per locale
/// (architecture §12.4).
///
/// # Example
///
/// ```ignore
/// use fern_i18n::{compile_in_locales, I18nConfig};
///
/// let cfg = I18nConfig::new()
///     .compile_in(compile_in_locales!(
///         base = "../locales/",
///         locales = ["en-US", "fr-FR", "es-ES", "ar-SA"],
///         files = ["main.ftl", "auth.ftl", "editor.ftl"],
///     ));
/// ```
///
/// expands to:
///
/// ```ignore
/// &[
///     ("en-US", &[
///         include_str!("../locales/en-US/main.ftl"),
///         include_str!("../locales/en-US/auth.ftl"),
///         include_str!("../locales/en-US/editor.ftl"),
///     ]),
///     ("fr-FR", &[ /* … same files … */ ]),
///     // …
/// ]
/// ```
///
/// Every `locale × file` combination must exist on disk (the expansion
/// uses `include_str!`, which fails at compile time if the file is
/// missing). If a locale ships a different subset of files, fall back
/// to writing the explicit slice by hand — the sugar assumes uniform
/// coverage.
///
/// `base` is the path prefix (usually `"../locales/"` relative to
/// `src/main.rs`); the macro joins `base`, `locale`, `/`, and `file`
/// via the compiler built-in `concat!`, producing a literal path
/// suitable for `include_str!`.
#[macro_export]
macro_rules! compile_in_locales {
    (
        base = $base:literal,
        locales = [$($locale:literal),* $(,)?],
        files = $files:tt $(,)?
    ) => {
        &[
            $(
                $crate::compile_in_locales!(@one $base, $locale, $files),
            )*
        ] as &'static [(&'static str, &'static [&'static str])]
    };
    // Internal helper: expand one locale's entry. The file list is
    // captured as a `tt` at the outer site so it survives the outer
    // `$(...)*` unchanged, then destructured here where `$locale` is
    // a fixed literal — sidestepping `macro_rules!`'s inability to
    // express a Cartesian product in a single nested repetition.
    (@one $base:literal, $locale:literal, [$($file:literal),* $(,)?]) => {
        (
            $locale,
            &[
                $(
                    include_str!(concat!($base, $locale, "/", $file)),
                )*
            ] as &'static [&'static str],
        )
    };
}
