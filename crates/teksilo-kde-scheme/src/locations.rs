// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Where KDE keeps its configuration and colour schemes.
//!
//! Everything here is a pure function of an environment getter: nothing
//! reads the process environment and nothing touches the file system. The
//! caller decides which candidate files exist and reads them.

use std::path::{Path, PathBuf};

use crate::config::{ExpansionEnv, KConfig, LayerKind};

/// One file of a cascade, with the kind of layer it is.
///
/// A cascade lists its layers lowest priority first; only files that exist
/// count, and the caller skips the others.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigLayerPath {
    /// The file.
    pub path: PathBuf,
    /// How the layer takes part in the cascade.
    pub kind: LayerKind,
}

/// The configuration and data directories KDE reads, resolved from the XDG
/// environment.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct KdeConfigLocations {
    /// `$HOME`, when set to an absolute path.
    pub home: Option<PathBuf>,
    /// `$XDG_CONFIG_HOME`, default `$HOME/.config`.
    pub config_home: Option<PathBuf>,
    /// `$XDG_CONFIG_DIRS` in the order listed (default `/etc/xdg`), with
    /// `$XDG_CONFIG_HOME/kdedefaults` prepended in a KDE session when it is
    /// not already listed.
    pub config_dirs: Vec<PathBuf>,
    /// `$XDG_DATA_HOME`, default `$HOME/.local/share`.
    pub data_home: Option<PathBuf>,
    /// `$XDG_DATA_DIRS` in the order listed, default `/usr/local/share`
    /// then `/usr/share`.
    pub data_dirs: Vec<PathBuf>,
    /// `$XDG_CACHE_HOME`, default `$HOME/.cache`.
    pub cache_home: Option<PathBuf>,
    /// Whether the session is a KDE session ([`is_kde_session`]).
    pub kde_session: bool,
    /// Whether `KDE_SKIP_KDERC` is set, which leaves `/etc/kde5rc` out of
    /// the `kdeglobals` cascade.
    pub skip_kderc: bool,
}

/// Whether the environment describes a KDE session: `XDG_CURRENT_DESKTOP`,
/// split at `:`, contains `KDE` (ignoring ASCII case), or `KDE_FULL_SESSION`
/// is `true`.
pub fn is_kde_session(get: impl Fn(&str) -> Option<String>) -> bool {
    let desktop = get("XDG_CURRENT_DESKTOP")
        .is_some_and(|v| v.split(':').any(|d| d.eq_ignore_ascii_case("KDE")));
    desktop || get("KDE_FULL_SESSION").as_deref() == Some("true")
}

impl KdeConfigLocations {
    /// Resolves the locations from `get`, which returns an environment
    /// variable's value (`None` when unset).
    ///
    /// An empty or relative home variable is ignored in favour of its
    /// default; empty and relative entries of a directory list are dropped,
    /// and a list left empty takes its default.
    pub fn from_env(get: impl Fn(&str) -> Option<String>) -> KdeConfigLocations {
        let home = absolute(get("HOME"));
        let under_home = |rel: &str| home.as_ref().map(|h| h.join(rel));
        let config_home = absolute(get("XDG_CONFIG_HOME")).or_else(|| under_home(".config"));
        let data_home = absolute(get("XDG_DATA_HOME")).or_else(|| under_home(".local/share"));
        let cache_home = absolute(get("XDG_CACHE_HOME")).or_else(|| under_home(".cache"));
        let kde_session = is_kde_session(&get);

        let mut config_dirs = dir_list(get("XDG_CONFIG_DIRS"), &["/etc/xdg"]);
        if kde_session && let Some(home) = &config_home {
            let kdedefaults = home.join("kdedefaults");
            if !config_dirs.contains(&kdedefaults) {
                config_dirs.insert(0, kdedefaults);
            }
        }
        let data_dirs = dir_list(get("XDG_DATA_DIRS"), &["/usr/local/share", "/usr/share"]);

        KdeConfigLocations {
            home,
            config_home,
            config_dirs,
            data_home,
            data_dirs,
            cache_home,
            kde_session,
            skip_kderc: get("KDE_SKIP_KDERC").is_some(),
        }
    }

    /// The `kdeglobals` cascade, lowest priority first:
    ///
    /// 1. `/etc/kde5rc`, unless `KDE_SKIP_KDERC` is set;
    /// 2. `system.kdeglobals` in each configuration directory, the last
    ///    listed first;
    /// 3. `kdeglobals` in each configuration directory, the last listed
    ///    first (so `kdedefaults`, listed first in a KDE session, is the
    ///    highest system layer);
    /// 4. the user's `system.kdeglobals`, a default layer;
    /// 5. the user's `kdeglobals`, the user layer.
    pub fn kdeglobals_cascade(&self) -> Vec<ConfigLayerPath> {
        let mut layers = Vec::new();
        if !self.skip_kderc {
            layers.push(default_layer(PathBuf::from("/etc/kde5rc")));
        }
        for name in ["system.kdeglobals", "kdeglobals"] {
            for dir in self.config_dirs.iter().rev() {
                layers.push(default_layer(dir.join(name)));
            }
        }
        if let Some(home) = &self.config_home {
            layers.push(default_layer(home.join("system.kdeglobals")));
            layers.push(ConfigLayerPath {
                path: home.join("kdeglobals"),
                kind: LayerKind::User,
            });
        }
        layers
    }

    /// The `breezerc` cascade, lowest priority first: `breezerc` in each
    /// configuration directory, the last listed first, then the user's
    /// `breezerc` as the user layer.
    pub fn breezerc_cascade(&self) -> Vec<ConfigLayerPath> {
        let mut layers: Vec<ConfigLayerPath> = self
            .config_dirs
            .iter()
            .rev()
            .map(|dir| default_layer(dir.join("breezerc")))
            .collect();
        if let Some(home) = &self.config_home {
            layers.push(ConfigLayerPath {
                path: home.join("breezerc"),
                kind: LayerKind::User,
            });
        }
        layers
    }

    /// The candidates for a relative data path, in order of preference:
    /// the data home, then each data directory as listed. The first one
    /// that exists is the file.
    pub fn data_file_candidates(&self, relative: impl AsRef<Path>) -> Vec<PathBuf> {
        let relative = relative.as_ref();
        self.data_home
            .iter()
            .chain(&self.data_dirs)
            .map(|dir| dir.join(relative))
            .collect()
    }

    /// The candidates for the colour scheme named `name`
    /// (`color-schemes/<name>.colors`), in order of preference.
    pub fn color_scheme_file(&self, name: &str) -> Vec<PathBuf> {
        self.data_file_candidates(format!("color-schemes/{name}.colors"))
    }

    /// The candidate files a palette is read from when `kdeglobals` does
    /// not carry the colours itself, in order of preference; `None` when
    /// the merged `kdeglobals` has a `[Colors:View]` group, which means it
    /// does.
    ///
    /// The candidates are, each through [`Self::data_file_candidates`]:
    /// the colours of the look-and-feel package (`[KDE]
    /// LookAndFeelPackage`, default `org.kde.breeze.desktop`), the scheme
    /// named by `[General] ColorScheme` (default `BreezeLight`), then
    /// `BreezeLight`. The first that exists is read with the `kdeglobals`
    /// cascade below it.
    pub fn scheme_fallback_candidates(
        &self,
        kdeglobals: &KConfig,
        env: &ExpansionEnv<'_>,
    ) -> Option<Vec<PathBuf>> {
        if kdeglobals.group_exists(&["Colors:View"]) {
            return None;
        }
        let package = kdeglobals
            .read_string(&["KDE"], "LookAndFeelPackage", env)
            .unwrap_or_else(|| "org.kde.breeze.desktop".to_owned());
        let scheme = kdeglobals
            .read_string(&["General"], "ColorScheme", env)
            .unwrap_or_else(|| "BreezeLight".to_owned());
        let mut candidates =
            self.data_file_candidates(format!("plasma/look-and-feel/{package}/contents/colors"));
        candidates.extend(self.color_scheme_file(&scheme));
        candidates.extend(self.color_scheme_file("BreezeLight"));
        Some(candidates)
    }

    /// The environment `[$e]` entries expand in: variables come from `var`,
    /// and an unset `QT_DATA_HOME`, `QT_CONFIG_HOME` or `QT_CACHE_HOME`
    /// reads as the matching XDG home directory.
    pub fn expansion_env<'a>(&self, var: &'a dyn Fn(&str) -> Option<String>) -> ExpansionEnv<'a> {
        let text = |p: &Option<PathBuf>| {
            p.as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        ExpansionEnv::new(
            var,
            text(&self.data_home),
            text(&self.config_home),
            text(&self.cache_home),
        )
    }
}

fn default_layer(path: PathBuf) -> ConfigLayerPath {
    ConfigLayerPath {
        path,
        kind: LayerKind::Default,
    }
}

/// XDG paths are POSIX paths and must be absolute: a value that does not
/// start with `/` is ignored, on every host.
fn is_absolute(value: &str) -> bool {
    value.starts_with('/')
}

fn absolute(value: Option<String>) -> Option<PathBuf> {
    value.filter(|v| is_absolute(v)).map(PathBuf::from)
}

fn dir_list(value: Option<String>, default: &[&str]) -> Vec<PathBuf> {
    let dirs: Vec<PathBuf> = value
        .as_deref()
        .unwrap_or_default()
        .split(':')
        .filter(|d| is_absolute(d))
        .map(PathBuf::from)
        .collect();
    if dirs.is_empty() {
        default.iter().map(PathBuf::from).collect()
    } else {
        dirs
    }
}
