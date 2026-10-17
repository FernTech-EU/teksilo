// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Reading KDE Plasma's colour schemes and desktop settings.
//!
//! This crate understands the files a KDE Plasma desktop keeps its look
//! in: the configuration text format and its system-to-user cascade
//! ([`config`]), where those files live ([`locations`]), the colour values
//! they store ([`color`]), the colour-scheme model of a `.colors` file or of
//! `kdeglobals` ([`scheme`]), Qt's font descriptions ([`fonts`]) and the
//! desktop settings built from all of them ([`desktop`]).
//!
//! It does no I/O. The environment is passed in as a getter and files are
//! passed in as text, so everything here runs the same in a test as on a
//! desktop:
//!
//! ```
//! use teksilo_kde_scheme::{KConfig, KdeColorScheme, KdeConfigLocations};
//!
//! let env = |name: &str| match name {
//!     "HOME" => Some("/home/ada".to_owned()),
//!     "XDG_CURRENT_DESKTOP" => Some("KDE".to_owned()),
//!     _ => None,
//! };
//! let locations = KdeConfigLocations::from_env(env);
//! // The caller reads each existing file of the cascade.
//! let layers: Vec<KConfig> = locations
//!     .kdeglobals_cascade()
//!     .into_iter()
//!     .filter(|layer| layer.path.ends_with("kdedefaults/kdeglobals"))
//!     .map(|layer| KConfig::parse("[General]\nColorScheme=BreezeDark\n").0.with_kind(layer.kind))
//!     .collect();
//! let globals = KConfig::cascade(&layers);
//! assert_eq!(globals.get(&["General"], "ColorScheme"), Some("BreezeDark"));
//!
//! let scheme = KdeColorScheme::breeze_dark();
//! assert_eq!(scheme.contrast, 4);
//! ```
//!
//! The behaviour is implemented from Teksilo's written specification of
//! KDE's formats (`engineering/docs/kde-breeze-spec.md` in the Teksilo
//! repository), not from KDE's code. The stock Breeze `.colors` files the
//! tests compare against are vendored as test fixtures only and are not
//! part of the published crate.

pub mod color;
pub mod config;
pub mod desktop;
pub mod fonts;
pub mod locations;
pub mod scheme;

pub use color::KdeColor;
pub use config::{DiagnosticKind, Entry, ExpansionEnv, KConfig, LayerKind, ParseDiagnostic};
pub use desktop::{
    BreezeSettings, KdeDesktop, KdeDesktopLayers, KdeFonts, MnemonicsMode, SchemeOrigin,
    ScrollBarButtons, ShadowSize,
};
pub use fonts::{FontStyle, KdeFont};
pub use locations::{ConfigLayerPath, KdeConfigLocations, is_kde_session};
pub use scheme::{
    ColorEffect, ColorRole, ColorSet, ColorState, ContrastEffect, IntensityEffect, KdeColorScheme,
    SetColors, StateEffect, WmColors,
};
