// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Plasma's faces, Noto Sans and Hack: named, embedded, or neither.
//!
//! Plasma writes its text in Noto Sans and its fixed-width text in Hack.
//! Which family the preset's typography names depends on two Cargo
//! features, both off by default:
//!
//! | Features | Text family | Monospace family | Embedded |
//! | --- | --- | --- | --- |
//! | neither | Inter | JetBrains Mono | nothing; Teksilo's own bundled faces draw |
//! | `system-fonts` | Noto Sans | Hack | nothing; the installed faces draw |
//! | `bundled-fonts` | Noto Sans | Hack | the six faces below |
//!
//! The sizes and line boxes are the same in every row: they are Noto Sans
//! and Hack's at Plasma's default fonts ([`crate::typography`]), so turning
//! a feature on changes the glyphs, never a control's height.
//!
//! ## `bundled-fonts`
//!
//! Embeds the faces in the binary, about 5.6 MB, and exposes
//! `font_registrar()`, which an app hands to the app builder so the families
//! resolve to them before any text is shaped:
//!
//! ```ignore
//! TeksiloAppBuilder::new()
//!     .theme(kde::light())
//!     .register_fonts(kde::font_registrar())
//!     .run();
//! ```
//!
//! | File | Face | Upstream |
//! | --- | --- | --- |
//! | `NotoSans-Variable.ttf` | Noto Sans, every weight, upright | Noto Sans v2.015, `NotoSans/full/variable-ttf/NotoSans[wdth,wght].ttf` |
//! | `NotoSans-Italic-Variable.ttf` | Noto Sans, every weight, italic | the same release, `NotoSans-Italic[wdth,wght].ttf` |
//! | `Hack-Regular.ttf`, `Hack-Bold.ttf` | Hack 400 and 700, upright | Hack v3.003, `Hack-v3.003-ttf.zip` |
//! | `Hack-Italic.ttf`, `Hack-BoldItalic.ttf` | Hack 400 and 700, italic | the same release |
//!
//! Each is redistributed unmodified, with its licence text beside it in
//! `fonts/`: the SIL Open Font License 1.1 for Noto Sans, the MIT licence
//! and the Bitstream Vera licence for Hack. Noto Sans is the variable
//! font, so the 600 weight of the bold text styles is drawn at that weight,
//! not synthesised or rounded to a bold. The italics are there for text an
//! app sets in italic: no text style of the preset is italic.
//!
//! The registrar registers the faces under the families "Noto Sans" and
//! "Hack" whatever their files' name tables say, and makes upright Noto
//! Sans at the body size the default face, the one text naming no family
//! is drawn in.
//!
//! ## `system-fonts`
//!
//! Names the families without embedding anything, for a desktop that has
//! them installed, as every Plasma desktop does: the text engine finds them
//! among the installed fonts, and on a machine without them falls back to
//! the bundled default, so the feature is never fatal. With both features
//! the bundled faces are registered and the installed ones are found too.
//!
//! [`crate::from_desktop`] names the desktop's own families with or without
//! either feature.

/// `[measured-qt]` The family of Plasma's general, smallest readable, menu
/// and toolbar fonts, and the text styles' family under either font
/// feature.
pub const NOTO_SANS_FAMILY: &str = "Noto Sans";

/// `[measured-qt]` The family of Plasma's fixed font, and the monospace
/// style's family under either font feature.
pub const HACK_FAMILY: &str = "Hack";

#[cfg(feature = "bundled-fonts")]
mod bundled {
    use std::sync::Arc;

    use teksilo_text::{FontFaceId, FontRegistrar, SharedFontData, TextFontService};

    use super::{HACK_FAMILY, NOTO_SANS_FAMILY};
    use crate::typography::{BREEZE_BODY_PT, face_px};

    /// Noto Sans v2.015, the variable upright face.
    static NOTO_SANS: &[u8] = include_bytes!("../fonts/NotoSans-Variable.ttf");
    /// Noto Sans v2.015, the variable italic face.
    static NOTO_SANS_ITALIC: &[u8] = include_bytes!("../fonts/NotoSans-Italic-Variable.ttf");
    /// Hack v3.003 Regular.
    static HACK_REGULAR: &[u8] = include_bytes!("../fonts/Hack-Regular.ttf");
    /// Hack v3.003 Bold.
    static HACK_BOLD: &[u8] = include_bytes!("../fonts/Hack-Bold.ttf");
    /// Hack v3.003 Italic.
    static HACK_ITALIC: &[u8] = include_bytes!("../fonts/Hack-Italic.ttf");
    /// Hack v3.003 Bold Italic.
    static HACK_BOLD_ITALIC: &[u8] = include_bytes!("../fonts/Hack-BoldItalic.ttf");

    /// `[derived]` The weight each face is registered at: its file's OS/2
    /// weight class, 400 for Noto Sans (a variable face, drawn at every
    /// weight from there) and Hack Regular and Italic, 700 for Hack Bold
    /// and Bold Italic.
    const REGULAR: u16 = 400;
    /// See [`REGULAR`].
    const BOLD: u16 = 700;

    /// Registers the embedded faces under the families the typography
    /// names.
    pub(super) struct BreezeFontRegistrar;

    /// Shares a face compiled into the binary rather than copying it onto
    /// the heap for the life of the process.
    fn shared(bytes: &'static [u8]) -> SharedFontData {
        Arc::new(bytes)
    }

    impl FontRegistrar for BreezeFontRegistrar {
        fn register_on_service(&self, service: &mut TextFontService) -> Option<FontFaceId> {
            let body = service.register_font_shared_as(
                shared(NOTO_SANS),
                NOTO_SANS_FAMILY,
                REGULAR,
                false,
            );
            service.register_font_shared_as(
                shared(NOTO_SANS_ITALIC),
                NOTO_SANS_FAMILY,
                REGULAR,
                true,
            );
            for (bytes, weight, italic) in [
                (HACK_REGULAR, REGULAR, false),
                (HACK_BOLD, BOLD, false),
                (HACK_ITALIC, REGULAR, true),
                (HACK_BOLD_ITALIC, BOLD, true),
            ] {
                service.register_font_shared_as(shared(bytes), HACK_FAMILY, weight, italic);
            }
            service.set_default_font(body, face_px(BREEZE_BODY_PT) as f32);
            Some(body)
        }
    }
}

/// A [`FontRegistrar`](teksilo_text::FontRegistrar) that embeds Noto Sans
/// and Hack and registers them under the families "Noto Sans" and "Hack",
/// upright Noto Sans at the body size as the default face. Pass it to the
/// app builder so the typography, which names those families under this
/// feature, resolves to them:
///
/// ```ignore
/// TeksiloAppBuilder::new()
///     .theme(kde::light())
///     .register_fonts(kde::font_registrar())
///     .run();
/// ```
///
/// Available only under the `bundled-fonts` Cargo feature. See [the module
/// documentation](crate::fonts) for the faces.
#[cfg(feature = "bundled-fonts")]
pub fn font_registrar() -> impl teksilo_text::FontRegistrar {
    bundled::BreezeFontRegistrar
}
