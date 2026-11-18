// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Icons from the desktop's icon theme, found by their freedesktop name
//! (`document-send`, `go-down-symbolic`), as the XDG Icon Theme
//! Specification describes the lookup.
//!
//! [`IconTheme::load`] reads a theme's `index.theme` and those of the themes
//! it inherits from, depth first, with `hicolor` last;
//! [`IconTheme::lookup`] then finds the file that best fits a size and a
//! scale, trying the theme, its parents, `hicolor`, and finally the
//! unthemed icons lying directly in the base directories
//! (`/usr/share/pixmaps`). [`current_icon_theme_name`] names the theme the
//! desktop uses.
//!
//! The lookup is pure: the file system is an [`IconThemeReader`] passed in,
//! and the environment a getter, so its rules run over fixture trees held in
//! memory. [`FileSystemIconReader`] is the real file system.
//!
//! Names follow the freedesktop Icon Naming Specification. A name holding a
//! path separator or `..`, and a theme or directory name that would leave the
//! theme's directories, are refused, and the sizes of what is read are
//! bounded ([`MAX_INDEX_LEN`], [`MAX_DIRECTORIES`], [`MAX_INHERITANCE_DEPTH`]).
//!
//! Icon themes exist on Linux and BSD desktops. Elsewhere
//! [`current_icon_theme_name`] answers `None` and a lookup finds what the
//! reader it is given holds.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

/// The longest `index.theme` read, in bytes; a longer one makes its theme
/// unusable. Large themes with every translation of their name stay far
/// below it.
pub const MAX_INDEX_LEN: u64 = 1024 * 1024;

/// The most icon directories one theme may list (`Directories` and
/// `ScaledDirectories` together); the ones past it are ignored.
pub const MAX_DIRECTORIES: usize = 4096;

/// How deep `Inherits` is followed below the theme asked for.
pub const MAX_INHERITANCE_DEPTH: usize = 16;

/// The most themes in one chain, `hicolor` included.
const MAX_THEMES: usize = 32;

/// The longest icon or theme name accepted, in bytes.
const MAX_NAME_LEN: usize = 255;

/// The theme every chain ends with.
const HICOLOR: &str = "hicolor";

/// The file system as the lookup sees it.
///
/// [`FileSystemIconReader`] is the real one; a test passes a tree held in
/// memory.
pub trait IconThemeReader {
    /// The bytes of the file at `path`, `None` when it does not exist or
    /// cannot be read. A reader may stop after `limit + 1` bytes: whoever
    /// asked rejects a file longer than `limit`.
    fn read(&self, path: &Path, limit: u64) -> Option<Vec<u8>>;

    /// Whether `path` is a file (a symbolic link to one counts).
    fn is_file(&self, path: &Path) -> bool;

    /// Whether `path` is a directory (a symbolic link to one counts).
    fn is_dir(&self, path: &Path) -> bool;
}

/// The real file system.
#[derive(Debug, Clone, Copy, Default)]
pub struct FileSystemIconReader;

impl IconThemeReader for FileSystemIconReader {
    fn read(&self, path: &Path, limit: u64) -> Option<Vec<u8>> {
        let file = std::fs::File::open(path).ok()?;
        let mut bytes = Vec::new();
        file.take(limit.saturating_add(1))
            .read_to_end(&mut bytes)
            .ok()?;
        Some(bytes)
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }
}

/// The format of an icon file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IconFileKind {
    /// An SVG document (`.svg`).
    Svg,
    /// A PNG image (`.png`).
    Png,
}

impl IconFileKind {
    /// In the order the specification tries them. XPM is not looked for.
    const SEARCHED: [IconFileKind; 2] = [IconFileKind::Png, IconFileKind::Svg];

    fn extension(self) -> &'static str {
        match self {
            IconFileKind::Svg => "svg",
            IconFileKind::Png => "png",
        }
    }
}

/// An icon file a lookup found.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct IconFile {
    /// Where the file is.
    pub path: PathBuf,
    /// Its format.
    pub kind: IconFileKind,
    /// Whether it is a `-symbolic` icon: a single-colour glyph meant to be
    /// drawn in the colour of the text around it.
    pub symbolic: bool,
}

/// What a lookup is asked for.
///
/// Built with [`IconRequest::new`]; the scale is 1, the plain icon is
/// wanted, and the generic fallback is on until said otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconRequest<'a> {
    name: &'a str,
    size: u32,
    scale: u32,
    symbolic: bool,
    generic_fallback: bool,
}

impl<'a> IconRequest<'a> {
    /// The icon `name` at `size` logical pixels.
    ///
    /// A name ending in `-symbolic` asks for the symbolic variant, as
    /// [`symbolic`](Self::symbolic) does.
    pub fn new(name: &'a str, size: u32) -> Self {
        let (name, symbolic) = match name.strip_suffix(SYMBOLIC_SUFFIX) {
            Some(stem) if !stem.is_empty() => (stem, true),
            _ => (name, false),
        };
        IconRequest {
            name,
            size,
            scale: 1,
            symbolic,
            generic_fallback: true,
        }
    }

    /// The scale the icon is drawn at (2 on a display at 200 %). 0 reads
    /// as 1.
    pub fn scale(mut self, scale: u32) -> Self {
        self.scale = scale.max(1);
        self
    }

    /// Ask for the `-symbolic` variant: `go-down` finds `go-down-symbolic`
    /// first. With the generic fallback on, the plain icon is found when the
    /// theme has no symbolic one; [`IconFile::symbolic`] says which it was.
    pub fn symbolic(mut self, symbolic: bool) -> Self {
        self.symbolic = symbolic;
        self
    }

    /// The Icon Naming Specification's generic fallback, on by default: a
    /// name the themes lack is tried again without its last dash-separated
    /// part (`document-send-mail`, then `document-send`, then `document`),
    /// and a symbolic request ends with the plain names.
    pub fn generic_fallback(mut self, on: bool) -> Self {
        self.generic_fallback = on;
        self
    }

    /// The names tried, in order, each with whether it is symbolic.
    fn candidates(&self) -> Vec<(String, bool)> {
        let mut stems = vec![self.name];
        if self.generic_fallback {
            let mut stem = self.name;
            while let Some((shorter, _)) = stem.rsplit_once('-') {
                if shorter.is_empty() {
                    break;
                }
                stems.push(shorter);
                stem = shorter;
            }
        }
        let plain = stems.iter().map(|stem| ((*stem).to_owned(), false));
        if !self.symbolic {
            return plain.collect();
        }
        let symbolic = stems
            .iter()
            .map(|stem| (format!("{stem}{SYMBOLIC_SUFFIX}"), true));
        if self.generic_fallback {
            symbolic.chain(plain).collect()
        } else {
            symbolic.collect()
        }
    }
}

const SYMBOLIC_SUFFIX: &str = "-symbolic";

/// An icon theme and the themes it inherits from, ready for lookups.
///
/// Loading reads the `index.theme` files only; a lookup checks which icon
/// files exist through the reader it is given. Load once and keep it: the
/// chain does not change until the desktop's theme does.
#[derive(Debug, Clone)]
pub struct IconTheme {
    /// The base directories, in search order.
    base_dirs: Vec<PathBuf>,
    /// The theme asked for, then its ancestors depth first, `hicolor` last.
    chain: Vec<ThemeIndex>,
}

impl IconTheme {
    /// The theme `name` and the themes it inherits from, read from the base
    /// directories `env` gives: `$HOME/.icons`, `$XDG_DATA_HOME/icons`, each
    /// `$XDG_DATA_DIRS/icons`, `/usr/share/pixmaps`.
    ///
    /// `None` when no base directory has an `index.theme` for `name` (or the
    /// first one found is longer than [`MAX_INDEX_LEN`]), or when `name` is
    /// not a plain name. A caller whose theme is missing loads `hicolor`.
    pub fn load(
        name: &str,
        reader: &dyn IconThemeReader,
        env: &dyn Fn(&str) -> Option<String>,
    ) -> Option<IconTheme> {
        if !is_plain_name(name) {
            return None;
        }
        let base_dirs = base_directories(env);
        let mut chain = Vec::new();
        if name != HICOLOR {
            let mut seen = HashSet::new();
            collect_chain(name, 0, reader, &base_dirs, &mut chain, &mut seen);
            if chain.is_empty() {
                return None;
            }
        }
        match ThemeIndex::read(HICOLOR, reader, &base_dirs) {
            Some(hicolor) => chain.push(hicolor),
            None if name == HICOLOR => return None,
            None => {}
        }
        Some(IconTheme { base_dirs, chain })
    }

    /// The theme's name.
    pub fn name(&self) -> &str {
        self.chain
            .first()
            .map_or(HICOLOR, |theme| theme.name.as_str())
    }

    /// The themes searched, in order: the theme, the themes it inherits
    /// from depth first, `hicolor` last when it is installed.
    pub fn chain(&self) -> impl Iterator<Item = &str> {
        self.chain.iter().map(|theme| theme.name.as_str())
    }

    /// The file for `request`, or `None` when no theme of the chain and no
    /// base directory has it, or when the name is not a plain name.
    ///
    /// Each theme is searched for every candidate name before its parent is
    /// (the specification's `FindBestIcon`). Within a theme the first file
    /// in a directory whose size matches wins, else the one whose directory
    /// is closest in size.
    pub fn lookup(
        &self,
        reader: &dyn IconThemeReader,
        request: &IconRequest<'_>,
    ) -> Option<IconFile> {
        if !is_plain_name(request.name) {
            return None;
        }
        let candidates = request.candidates();
        for theme in &self.chain {
            for (name, symbolic) in &candidates {
                if let Some((path, kind)) =
                    theme.lookup_icon(reader, name, request.size, request.scale)
                {
                    return Some(IconFile {
                        path,
                        kind,
                        symbolic: *symbolic,
                    });
                }
            }
        }
        candidates.iter().find_map(|(name, symbolic)| {
            self.lookup_unthemed(reader, name)
                .map(|(path, kind)| IconFile {
                    path,
                    kind,
                    symbolic: *symbolic,
                })
        })
    }

    /// The specification's `LookupFallbackIcon`: `name` directly in a base
    /// directory.
    fn lookup_unthemed(
        &self,
        reader: &dyn IconThemeReader,
        name: &str,
    ) -> Option<(PathBuf, IconFileKind)> {
        self.base_dirs.iter().find_map(|base| {
            IconFileKind::SEARCHED.into_iter().find_map(|kind| {
                let path = base.join(format!("{name}.{}", kind.extension()));
                reader.is_file(&path).then_some((path, kind))
            })
        })
    }
}

/// Push `name` and its ancestors onto `chain`, depth first. A theme already
/// seen, `hicolor` (appended last by the caller), a theme with no readable
/// `index.theme` and anything past the bounds are skipped.
fn collect_chain(
    name: &str,
    depth: usize,
    reader: &dyn IconThemeReader,
    base_dirs: &[PathBuf],
    chain: &mut Vec<ThemeIndex>,
    seen: &mut HashSet<String>,
) {
    if depth > MAX_INHERITANCE_DEPTH
        || chain.len() + 1 >= MAX_THEMES
        || name == HICOLOR
        || !is_plain_name(name)
        || !seen.insert(name.to_owned())
    {
        return;
    }
    let Some(theme) = ThemeIndex::read(name, reader, base_dirs) else {
        return;
    };
    let parents = theme.inherits.clone();
    chain.push(theme);
    for parent in &parents {
        collect_chain(parent, depth + 1, reader, base_dirs, chain, seen);
    }
}

/// One theme: where its directories are and what its `index.theme` says of
/// them.
#[derive(Debug, Clone)]
struct ThemeIndex {
    name: String,
    /// `<base>/<name>` for every base directory that has it: a theme's icons
    /// may be spread over several (the user's own additions to `hicolor`).
    roots: Vec<PathBuf>,
    inherits: Vec<String>,
    directories: Vec<IconDirectory>,
}

impl ThemeIndex {
    /// The first `index.theme` of `name` in the base directories, as the
    /// specification says; `None` when there is none or it is too long.
    fn read(name: &str, reader: &dyn IconThemeReader, base_dirs: &[PathBuf]) -> Option<ThemeIndex> {
        let bytes = base_dirs
            .iter()
            .find_map(|base| reader.read(&base.join(name).join("index.theme"), MAX_INDEX_LEN))?;
        if bytes.len() as u64 > MAX_INDEX_LEN {
            return None;
        }
        let index = parse_index(&String::from_utf8_lossy(&bytes));
        let theme_group = index.get("Icon Theme")?;
        let inherits = theme_group
            .get("Inherits")
            .map(|list| {
                split_list(list)
                    .filter(|n| is_plain_name(n))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let listed = ["Directories", "ScaledDirectories"]
            .into_iter()
            .filter_map(|key| theme_group.get(key))
            .flat_map(|list| split_list(list));
        let mut seen = HashSet::new();
        let directories = listed
            .filter(|dir| is_relative_subdirectory(dir) && seen.insert(*dir))
            .take(MAX_DIRECTORIES)
            .filter_map(|dir| IconDirectory::parse(dir, index.get(dir)?))
            .collect();
        let roots = base_dirs
            .iter()
            .map(|base| base.join(name))
            .filter(|root| reader.is_dir(root))
            .collect();
        Some(ThemeIndex {
            name: name.to_owned(),
            roots,
            inherits,
            directories,
        })
    }

    /// The specification's `LookupIcon`, in one pass: walking the
    /// directories, base directories and extensions in its order, the first
    /// file in a directory matching the size is the one its first loop
    /// returns, and when none matches, the closest seen is the one its
    /// second loop keeps.
    fn lookup_icon(
        &self,
        reader: &dyn IconThemeReader,
        name: &str,
        size: u32,
        scale: u32,
    ) -> Option<(PathBuf, IconFileKind)> {
        let mut closest: Option<(u64, PathBuf, IconFileKind)> = None;
        for directory in &self.directories {
            for root in &self.roots {
                for kind in IconFileKind::SEARCHED {
                    let path = root
                        .join(&directory.path)
                        .join(format!("{name}.{}", kind.extension()));
                    if !reader.is_file(&path) {
                        continue;
                    }
                    if directory.matches_size(size, scale) {
                        return Some((path, kind));
                    }
                    let distance = directory.size_distance(size, scale);
                    if closest.as_ref().is_none_or(|(best, ..)| distance < *best) {
                        closest = Some((distance, path, kind));
                    }
                }
            }
        }
        closest.map(|(_, path, kind)| (path, kind))
    }
}

/// How a directory's icons may be scaled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirectoryType {
    /// Only at their size.
    Fixed,
    /// Between `MinSize` and `MaxSize`.
    Scalable,
    /// Within `Threshold` of their size.
    Threshold,
}

/// One icon directory of a theme (`16x16/actions`, `scalable/apps`).
#[derive(Debug, Clone)]
struct IconDirectory {
    /// Relative to the theme's root.
    path: String,
    size: u32,
    scale: u32,
    kind: DirectoryType,
    min_size: u32,
    max_size: u32,
    threshold: u32,
}

impl IconDirectory {
    /// The directory `path` as its group describes it; `None` without a
    /// `Size`, which the specification requires.
    fn parse(path: &str, group: &HashMap<String, String>) -> Option<IconDirectory> {
        let number = |key: &str| group.get(key).and_then(|v| v.trim().parse::<u32>().ok());
        let size = number("Size")?;
        let kind = match group.get("Type").map(|t| t.trim()) {
            Some("Fixed") => DirectoryType::Fixed,
            Some("Scalable") => DirectoryType::Scalable,
            _ => DirectoryType::Threshold,
        };
        Some(IconDirectory {
            path: path.to_owned(),
            size,
            scale: number("Scale").filter(|s| *s > 0).unwrap_or(1),
            kind,
            min_size: number("MinSize").unwrap_or(size),
            max_size: number("MaxSize").unwrap_or(size),
            threshold: number("Threshold").unwrap_or(2),
        })
    }

    /// The specification's `DirectoryMatchesSize`.
    fn matches_size(&self, size: u32, scale: u32) -> bool {
        if self.scale != scale {
            return false;
        }
        match self.kind {
            DirectoryType::Fixed => self.size == size,
            DirectoryType::Scalable => (self.min_size..=self.max_size).contains(&size),
            DirectoryType::Threshold => {
                let low = self.size.saturating_sub(self.threshold);
                let high = self.size.saturating_add(self.threshold);
                (low..=high).contains(&size)
            }
        }
    }

    /// The specification's `DirectorySizeDistance`, in device pixels. For a
    /// `Threshold` directory outside its range it measures to `MinSize` or
    /// `MaxSize`, as the specification writes it; the absolute value is
    /// taken so a directory whose `MinSize` lies outside its threshold range
    /// cannot come out closer than an exact one.
    fn size_distance(&self, size: u32, scale: u32) -> u64 {
        let wanted = i64::from(size) * i64::from(scale);
        let at = |n: u32| i64::from(n) * i64::from(self.scale);
        let distance = match self.kind {
            DirectoryType::Fixed => at(self.size) - wanted,
            DirectoryType::Scalable => {
                if wanted < at(self.min_size) {
                    at(self.min_size) - wanted
                } else if wanted > at(self.max_size) {
                    wanted - at(self.max_size)
                } else {
                    0
                }
            }
            DirectoryType::Threshold => {
                if wanted < at(self.size.saturating_sub(self.threshold)) {
                    at(self.min_size) - wanted
                } else if wanted > at(self.size.saturating_add(self.threshold)) {
                    wanted - at(self.max_size)
                } else {
                    0
                }
            }
        };
        distance.unsigned_abs()
    }
}

/// The base directories `env` gives, in search order, without duplicates.
/// A relative path in an XDG variable is ignored, as the base directory
/// specification asks.
fn base_directories(env: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let absolute = |value: String| value.starts_with('/').then(|| PathBuf::from(value));
    let home = env("HOME").and_then(absolute);
    let mut dirs = Vec::new();
    if let Some(home) = &home {
        dirs.push(home.join(".icons"));
    }
    let data_home = env("XDG_DATA_HOME")
        .and_then(absolute)
        .or_else(|| home.as_ref().map(|home| home.join(".local/share")));
    if let Some(data_home) = data_home {
        dirs.push(data_home.join("icons"));
    }
    let data_dirs = env("XDG_DATA_DIRS")
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/usr/local/share/:/usr/share/".to_owned());
    dirs.extend(
        data_dirs
            .split(':')
            .filter(|dir| dir.starts_with('/'))
            .map(|dir| Path::new(dir).join("icons")),
    );
    dirs.push(PathBuf::from("/usr/share/pixmaps"));
    let mut seen = HashSet::new();
    dirs.retain(|dir| seen.insert(dir.clone()));
    dirs
}

/// A name that is one path component and nothing more: not empty, no
/// separator, no `..`, no NUL, not too long. Icon and theme names both.
fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && !name.contains(['/', '\\', '\0'])
        && !name.contains("..")
}

/// A directory listed by a theme: relative, and every component a plain
/// name, so `theme/<dir>` stays inside the theme.
fn is_relative_subdirectory(dir: &str) -> bool {
    dir.len() <= MAX_NAME_LEN
        && !dir.starts_with('/')
        && dir
            .split('/')
            .all(|part| part != "." && is_plain_name(part))
}

/// The items of a comma-separated list, trimmed, empty ones dropped.
fn split_list(list: &str) -> impl Iterator<Item = &str> {
    list.split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
}

/// An `index.theme`, which is a desktop entry file: `[Group]` headers and
/// `Key=Value` lines, `#` comments. The first of a repeated group or key
/// counts.
fn parse_index(text: &str) -> HashMap<String, HashMap<String, String>> {
    let mut groups: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut current: Option<String> = None;
    let mut closed: HashSet<String> = HashSet::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            if let Some(previous) = current.take() {
                closed.insert(previous);
            }
            if !closed.contains(name) {
                groups.entry(name.to_owned()).or_default();
                current = Some(name.to_owned());
            }
            continue;
        }
        let (Some(group), Some((key, value))) = (current.as_ref(), line.split_once('=')) else {
            continue;
        };
        if let Some(entries) = groups.get_mut(group) {
            entries
                .entry(key.trim().to_owned())
                .or_insert_with(|| value.trim().to_owned());
        }
    }
    groups
}

/// The icon theme the desktop uses, or `None` when it cannot be told; the
/// caller then loads `hicolor`.
///
/// On a KDE Plasma session it is `kdeglobals [Icons] Theme` (`breeze` when
/// the key is absent); otherwise the desktop portal's
/// `org.gnome.desktop.interface icon-theme`. A name that is not a plain name
/// reads as `None`. Always `None` off Linux.
///
/// The portal read is a blocking D-Bus call, bounded at half a second.
pub fn current_icon_theme_name() -> Option<String> {
    platform::current_icon_theme_name().filter(|name| is_plain_name(name))
}

#[cfg(target_os = "linux")]
mod platform {
    use teksilo_kde_scheme::KdeConfigLocations;

    use crate::linux_helpers::{Desktop, detect_desktop};
    use crate::linux_portal::read_portal_string;
    use crate::os_theme::kde;

    pub(super) fn current_icon_theme_name() -> Option<String> {
        let from_kde = match detect_desktop() {
            Desktop::Kde => {
                let locations = KdeConfigLocations::from_env(kde::process_env);
                kde::kde_icon_theme(&locations, &kde::process_env, kde::read_file)
            }
            _ => None,
        };
        from_kde.or_else(|| read_portal_string("org.gnome.desktop.interface", "icon-theme"))
    }
}

#[cfg(not(target_os = "linux"))]
mod platform {
    pub(super) fn current_icon_theme_name() -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests;
