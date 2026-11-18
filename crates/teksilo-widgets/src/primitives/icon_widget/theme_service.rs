// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`IconThemeService`]: the desktop's icon theme as
//! [`IconWidget::themed`](super::IconWidget::themed) reaches it, with the
//! files it found decoded and kept.

use std::cell::RefCell;
use std::collections::HashMap;

use teksilo_canvas::RasterIcon;
use teksilo_canvas::svg::SvgIcon;
use teksilo_core::signal::Signal;
use teksilo_platform::icon_theme::{
    FileSystemIconReader, IconFile, IconFileKind, IconRequest, IconTheme, IconThemeReader,
    current_icon_theme_name,
};

/// The longest icon file read, in bytes. Theme icons are a few kilobytes.
const MAX_ICON_FILE_LEN: u64 = 1024 * 1024;

/// The widest or tallest PNG decoded, in pixels: a theme's largest icons are
/// 512 px, and the bound keeps a small file declaring a huge image from
/// being decoded.
const MAX_RASTER_SIDE: u32 = 1024;

/// How many lookups are kept, found or not.
const CACHE_CAPACITY: usize = 256;

/// The icon theme of the desktop, for [`IconWidget::themed`](super::IconWidget::themed).
///
/// An application gets one in its app-state from `TeksiloAppBuilder::run`,
/// reading the desktop through [`new`](Self::new); register another with
/// `.app_state(...)` before `run` to replace it (a fixed theme, a test
/// fixture). A tree with none draws every themed icon's fallback.
///
/// Nothing is read until the first themed icon is built. Then the theme's
/// name is asked of the desktop (on a GNOME session, a D-Bus call to the
/// portal bounded at half a second), its `index.theme` files are read, and
/// each icon found is read and decoded once, on the UI thread: an icon file
/// is a few kilobytes, so this costs well under a millisecond per icon after
/// the first. The last 256 lookups are kept, misses included.
///
/// [`reload`](Self::reload) forgets all of it and rebuilds every themed
/// icon; the window manager calls it when it hears the desktop's appearance
/// change.
pub struct IconThemeService {
    reader: Box<dyn IconThemeReader>,
    env: Box<dyn Fn(&str) -> Option<String>>,
    theme_name: Box<dyn Fn() -> Option<String>>,
    state: RefCell<State>,
    generation: Signal<u64>,
}

impl std::fmt::Debug for IconThemeService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.borrow();
        f.debug_struct("IconThemeService")
            .field(
                "theme",
                &state
                    .theme
                    .as_ref()
                    .map(|theme| theme.as_ref().map(IconTheme::name)),
            )
            .field("cached", &state.pictures.len())
            .finish()
    }
}

#[derive(Default)]
struct State {
    /// `None` until first used; then the theme loaded, or `None` when not
    /// even `hicolor` could be.
    theme: Option<Option<IconTheme>>,
    /// Lookup key to what was found and when it was last used.
    pictures: HashMap<(String, u32, u32), (Option<ThemedPicture>, u64)>,
    clock: u64,
}

/// A themed icon's picture, decoded.
#[derive(Debug, Clone)]
pub(crate) struct ThemedPicture {
    pub(crate) image: ThemedImage,
    /// A `-symbolic` icon, drawn in the widget's colour.
    pub(crate) symbolic: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum ThemedImage {
    Svg(SvgIcon),
    Raster(RasterIcon),
}

impl Default for IconThemeService {
    fn default() -> Self {
        Self::new()
    }
}

impl IconThemeService {
    /// The desktop's icon theme
    /// ([`current_icon_theme_name`], `hicolor` when it names none or its
    /// theme is not installed), read from the real file system and the
    /// process environment. Cheap: nothing is read until first used.
    pub fn new() -> Self {
        Self::build(
            Box::new(FileSystemIconReader),
            Box::new(|name| std::env::var(name).ok()),
            Box::new(current_icon_theme_name),
        )
    }

    /// The icon theme `theme` (`hicolor` when `None` or not installed),
    /// read through `reader` from the base directories `env` gives (`HOME`,
    /// `XDG_DATA_HOME`, `XDG_DATA_DIRS`).
    pub fn with_source(
        reader: impl IconThemeReader + 'static,
        env: impl Fn(&str) -> Option<String> + 'static,
        theme: Option<String>,
    ) -> Self {
        Self::build(
            Box::new(reader),
            Box::new(env),
            Box::new(move || theme.clone()),
        )
    }

    fn build(
        reader: Box<dyn IconThemeReader>,
        env: Box<dyn Fn(&str) -> Option<String>>,
        theme_name: Box<dyn Fn() -> Option<String>>,
    ) -> Self {
        IconThemeService {
            reader,
            env,
            theme_name,
            state: RefCell::new(State::default()),
            generation: Signal::new(0),
        }
    }

    /// Forget the theme and every icon found, and rebuild the themed icons
    /// of every window, which then look their icons up again.
    pub fn reload(&self) {
        *self.state.borrow_mut() = State::default();
        let generation = self.generation.get();
        self.generation.set(generation.wrapping_add(1));
    }

    /// Bumped by [`reload`](Self::reload); a themed icon binds it to be
    /// rebuilt.
    pub(crate) fn generation(&self) -> Signal<u64> {
        self.generation.clone()
    }

    /// The picture for the icon `name` at `size` logical pixels and `scale`,
    /// `None` when the theme has no usable file for it.
    pub(crate) fn picture(&self, name: &str, size: u32, scale: u32) -> Option<ThemedPicture> {
        let key = (name.to_owned(), size, scale);
        let mut state = self.state.borrow_mut();
        state.clock += 1;
        let now = state.clock;
        if let Some((picture, used)) = state.pictures.get_mut(&key) {
            *used = now;
            return picture.clone();
        }
        let theme = state.theme.get_or_insert_with(|| self.load_theme());
        let picture = theme
            .as_ref()
            .and_then(|theme| {
                theme.lookup(&*self.reader, &IconRequest::new(name, size).scale(scale))
            })
            .and_then(|file| self.decode(&file));
        if state.pictures.len() >= CACHE_CAPACITY
            && let Some(oldest) = state
                .pictures
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(key, _)| key.clone())
        {
            state.pictures.remove(&oldest);
        }
        state.pictures.insert(key, (picture.clone(), now));
        picture
    }

    fn load_theme(&self) -> Option<IconTheme> {
        let named =
            (self.theme_name)().and_then(|name| IconTheme::load(&name, &*self.reader, &*self.env));
        named.or_else(|| IconTheme::load("hicolor", &*self.reader, &*self.env))
    }

    fn decode(&self, file: &IconFile) -> Option<ThemedPicture> {
        let bytes = self.reader.read(&file.path, MAX_ICON_FILE_LEN)?;
        if bytes.len() as u64 > MAX_ICON_FILE_LEN {
            return None;
        }
        let image = match file.kind {
            IconFileKind::Svg => {
                ThemedImage::Svg(SvgIcon::parse(std::str::from_utf8(&bytes).ok()?).ok()?)
            }
            IconFileKind::Png => {
                let (width, height) = png_dimensions(&bytes)?;
                if width > MAX_RASTER_SIDE || height > MAX_RASTER_SIDE {
                    return None;
                }
                ThemedImage::Raster(RasterIcon::decode_png(&bytes).ok()?)
            }
        };
        Some(ThemedPicture {
            image,
            symbolic: file.symbolic,
        })
    }
}

/// A PNG's width and height from its header, before decoding it.
fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    if bytes.get(..8)? != SIGNATURE || bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    let number = |at: usize| Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
    Some((number(16)?, number(20)?))
}

#[cfg(test)]
mod tests {
    use super::png_dimensions;

    #[test]
    fn a_png_header_gives_its_dimensions_and_anything_else_none() {
        let mut header = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        header.extend_from_slice(&5000u32.to_be_bytes());
        header.extend_from_slice(&3u32.to_be_bytes());
        assert_eq!(png_dimensions(&header), Some((5000, 3)));
        assert_eq!(png_dimensions(&header[..20]), None);
        assert_eq!(png_dimensions(b"<svg/>"), None);
    }
}
