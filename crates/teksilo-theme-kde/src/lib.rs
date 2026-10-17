// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! KDE Plasma theme preset for Teksilo: the Breeze design language.
//!
//! The colours come from a KDE colour scheme, the stock Breeze Light or
//! Breeze Dark or any other, read and resolved by [`scheme`]
//! (`teksilo-kde-scheme`) exactly as KDE resolves it, including Plasma's
//! accent colours. The geometry, typography and motion are Breeze's, as
//! Plasma 6.8 ships it ([`BREEZE_VERSION`]).
//!
//! ```
//! use teksilo_theme_kde::{self as kde, PlasmaAccent};
//!
//! let light = kde::light();
//! let dark = kde::dark();
//! let purple = kde::light_with_accent(PlasmaAccent::Purple.color());
//! assert_eq!(light.id.as_str(), "kde.light");
//! assert_eq!(dark.id.as_str(), "kde.dark");
//! assert_ne!(purple.colors.accent, light.colors.accent);
//! ```
//!
//! A scheme of the user's own, read from its `.colors` file, gives its own
//! theme, light or dark as KDE judges it:
//!
//! ```
//! use teksilo_theme_kde::{self as kde, scheme::KdeColorScheme};
//!
//! let scheme = KdeColorScheme::breeze_dark();
//! let theme = kde::from_scheme(&scheme);
//! assert!(theme.is_dark());
//! ```
//!
//! ## What makes it read as Breeze
//!
//! - **One radius.** Controls and floating surfaces share Breeze's 5 px
//!   frame radius (4.5 where a 1 px outline is stroked), between Fluent's 4
//!   and macOS's 6 and 10.
//! - **Breeze's buttons.** A push button is a flat fill outlined in the
//!   button colour moved a fifth of the way to its text (the frame
//!   contrast), with an outline that turns to the highlight under the
//!   pointer or keyboard focus and a 1 px shadow below it in an active
//!   window; the default button's fill moves toward the highlight, and a
//!   tool button shows nothing until it is hovered ([`styles`]).
//! - **Breeze's indicators and fields.** A check box or radio button is a
//!   15 px box or disc in the button colour, filled toward the highlight and
//!   marked in the view's text when checked, its label 24 px from its
//!   leading edge; a switch is a handle as tall as its 36 x 18 indicator
//!   riding a 12 px track; a text field is a flat view-coloured frame whose
//!   1 px outline turns to the focus colour while it has focus. A combo box
//!   is the push button with a chevron 2 px inside its trailing edge, a
//!   spin box the line edit with a column of two chevrons and no buttons
//!   drawn, and a slider a 6 px groove filled toward the highlight up to a
//!   17 px round handle in the button colour. Other
//!   controls are outlined in the window colour moved the same way toward
//!   the text, the `border` token; the highlight-based outlines are in
//!   [`BreezeColors`].
//! - **Breeze's menus and popups.** A menu row is 29 px at Plasma's default
//!   font, washed with the focus colour at 30 % and outlined while it is
//!   highlighted, and filled with the accent while it is held down; a menu,
//!   a popover and a tooltip are framed at the 5 px radius in a 1 px
//!   outline over Breeze's two-layer window shadow, and a tooltip's text
//!   lies 4 px inside its frame.
//! - **Breeze's rows, tabs and bars.** A list or tree row is 24 px at
//!   Plasma's default font, 26 with an icon or a check box, its selection a
//!   rounded rectangle inside the row's 2 px and 1 px margins, filled with
//!   the highlight and written in the text on it, and muted only when the
//!   window goes inactive; a table row is 24 px under a 30 px header of
//!   button-coloured sections, the column sorted in ascending order shown
//!   pressed. A selected tab is the window colour under a
//!   3 px highlight bar, an unselected one the window colour darkened. A
//!   scroll bar is an 8 px handle over no track beside a 1 px separator,
//!   and a progress bar a 6 px groove filled toward the highlight, its busy
//!   state moving stripes.
//! - **Breeze's focus ring geometry.** Keyboard focus is a 2 px band, half
//!   inside the control and half outside, in the scheme's focus colour.
//!   Breeze paints it at 30 % ([`shape::BREEZE_FOCUS_RING_ALPHA`]); the
//!   `focus_ring` token is opaque, lifted to 3:1, and the styles draw it as
//!   it is.
//! - **Linear, quick motion.** Style transitions run 100 ms, linearly.
//! - **Breeze's frames and margins.** A card, a panel and a banner are
//!   framed at the 5 px radius in the 1 px pen with their content 6 px
//!   inside, the margin of a nested layout; a dialog's content lies 10 px
//!   inside it, Breeze's dialog margin; a suggestion list lays its rows out
//!   as a menu without icons ([`styles::recipes`]).
//! - **Text-driven heights.** A push button is the text line plus 16 px, a
//!   field the line plus 14: 34 and 32 px at Plasma's default font, and
//!   they grow with it ([`geometry`]).
//!
//! ## The live desktop
//!
//! [`from_desktop`] builds the theme of a KDE desktop as it is configured,
//! read by the platform or by [`scheme`] ([`scheme::KdeDesktop`]): its
//! colour scheme with the user's accent, its fonts, its animation speed and
//! its `breezerc`. [`native_theme`] is the same theme for a desktop styled
//! with Breeze and `None` for one styled otherwise, the shape
//! `teksilo-app`'s native-theme factory takes:
//!
//! ```
//! use teksilo_theme_kde::{self as kde, scheme::KdeColorScheme, scheme::KdeDesktop};
//! use teksilo_tokens::TargetDensity;
//!
//! let mut desktop = KdeDesktop::new(KdeColorScheme::breeze_light());
//! assert!(kde::native_theme(&desktop, TargetDensity::Compact).is_some());
//! desktop.widget_style = Some("Fusion".to_owned());
//! assert!(kde::native_theme(&desktop, TargetDensity::Compact).is_none());
//! ```
//!
//! Every theme carries the desktop's `breezerc` settings as a
//! [`BreezeStyleConfig`] and its menu and toolbar fonts as
//! [`KdeFontRoles`], at Plasma's defaults for the stock themes.
//!
//! ## Fonts
//!
//! Plasma writes in Noto Sans and Hack. By default the preset draws
//! Teksilo's bundled Inter and JetBrains Mono at Noto Sans and Hack's sizes
//! and line boxes; two Cargo features, both off by default, name Plasma's
//! faces instead ([`fonts`]):
//!
//! - `bundled-fonts` embeds Noto Sans and Hack, about 5.6 MB, and
//!   `font_registrar()` registers them for the app builder:
//!
//!   ```ignore
//!   TeksiloAppBuilder::new()
//!       .theme(kde::light())
//!       .register_fonts(kde::font_registrar())
//!       .run();
//!   ```
//! - `system-fonts` names them without embedding them, for a desktop that
//!   has them installed.
//!
//! [`from_desktop`] names the desktop's own fonts either way.
//!
//! ## Accent colour
//!
//! [`light_with_accent`] and [`dark_with_accent`] apply an accent the way
//! Plasma's colour settings apply it to the stock scheme: links, focus and
//! hover take the accent (a link darkened or lightened once when it would be
//! hard to read), and the selection becomes the accent at 70 % over the
//! view with black or white text. So even Breeze's own blue,
//! [`PlasmaAccent::Blue`], gives a lighter selection than [`light`], as it
//! does on a Plasma desktop. [`PlasmaAccent`] lists the ten swatches Plasma
//! offers.
//!
//! ## Colours and WCAG
//!
//! KDE never adjusts a colour for readability, and some stock Breeze colours
//! fall short of WCAG 2.x (the light theme's secondary text, links, status
//! colours, focus colour, frame outlines, scroll-bar thumb and selection
//! text). The theme's colour tokens lift each of them by rule, for any
//! scheme, keeping its hue; the [`KdePalette`] extension keeps the scheme's
//! own colours. The rules, the inactive-window and high-contrast colours and
//! the tokens kept from Int UI are listed in the colour mapping's
//! documentation (the private `color` module); every lift is tested to pass
//! and its premise, that the scheme's colour fails, is tested too.
//!
//! An inactive window paints the scheme's Inactive colours: for Breeze, a
//! selection and accent muted to the window colour tinted toward the
//! selection colour, as Plasma paints a backdrop window, with the focus ring
//! unchanged. Under the OS high-contrast preference the theme paints its own
//! high-contrast colours, with 7:1 text and 4.5:1 boundaries.
//!
//! ## Trademarks
//!
//! KDE® and the K Desktop Environment® logo are registered trademarks of
//! KDE e.V. This preset is not affiliated with, endorsed by or sponsored by
//! KDE e.V.; the names KDE, Plasma and Breeze are used only to say which
//! design language it reproduces. It contains no KDE code, artwork, icon or
//! font; the Noto Sans and Hack faces the `bundled-fonts` feature embeds
//! are the Noto project's and Source Foundry's, under their own licences.
//! See `NOTICE` and `TRADEMARKS.md` in the Teksilo repository.
//!
//! ## Known limitations
//!
//! - **Some widget chrome is the framework's.** This version installs
//!   Breeze styles for `Button`, `IconButton`, `SplitButton`, `Toggle`,
//!   `Checkbox`, `RadioButton`, `TextInput`, `ComboBox`, `SpinBox`,
//!   `Slider`, `MenuItem`, the popup frame, the tooltip, `StandardListItem`
//!   and `StandardTreeItem`, `TableView` and `TreeTableView`, `TabBar` and
//!   `TabWidget`, `ScrollBar` and `ProgressBar`. `Card`, `Panel`, `Dialog`,
//!   `Snackbar`, `Toast`, `Banner`, `Link`, `Badge`, `Calendar`,
//!   `SearchField`, `Avatar`, `Splitter`, the list containers and
//!   `SegmentedControl` keep Teksilo's recipe style built with Breeze's
//!   radius, pen and margins, and the dimensions Breeze has no number for,
//!   or that the widget does not take from its style (a segmented control's
//!   height, a calendar's cell size), stay the framework's
//!   ([`styles::recipes`]). Every other widget keeps Teksilo's recipe style
//!   as it ships, painted in the Breeze colours, radii, focus ring,
//!   typography and motion.
//! - **A menu's shortcuts, separators, section titles and check marks are
//!   the framework's.** `MenuItem` writes a shortcut in the shortcut text
//!   role, where Breeze writes the window text at 70 % opacity, and gives
//!   its style no say over it; a `MenuSeparator` runs the width of the
//!   rows, where Breeze stops its line 4 px short of either end; a section
//!   title is the widget an app hands `MenuList::header`, which
//!   [`styles::metrics::BreezeMenuMetrics::section_height`] can size; and
//!   a checkable row shows the framework's check mark in its icon column,
//!   not Breeze's check box ([`styles::menu_item`]).
//! - **Every menu is laid out as one with icons.** `MenuItem` reserves its
//!   icon column on every row, so a menu without icons keeps the column,
//!   where Breeze moves the text 4 px past its 8 px margin instead
//!   ([`styles::menu_item`]).
//! - **A pressed menu row is filled with the accent**, not the focus
//!   colour, so the text on it is readable (D1); the two are the same colour
//!   in the stock Breeze schemes. A highlighted row's outline is lifted to
//!   3:1 (D5, [`styles::menu_item`]).
//! - **A popup has no caret and a shadow on every side**, and is rounded
//!   whatever the desktop's compositing; a menu translucent under
//!   `breezerc`'s `MenuOpacity` has nothing blurred behind it, where Plasma's
//!   compositor blurs what lies under it ([`styles::popup`]).
//! - **A plain tooltip is shorter than Plasma's.** `TooltipWidget` writes its
//!   text in the small text style, where Breeze uses the body font, so a
//!   one-line tooltip is the small line box plus 8 px rather than 26 px
//!   ([`styles::tooltip`]).
//! - **A spin box's arrows keep their colour at the limit**, and an empty
//!   combo box's arrow is not greyed: the styles are not told the value's
//!   limits or the item count ([`styles::spin_box`],
//!   [`styles::combo_box`]).
//! - **A spin box's two arrow halves are 20 x 10**, Breeze's geometry,
//!   under WCAG 2.2 SC 2.5.8's 24 px at every density; the value is also
//!   reachable with the arrow and page keys, the mouse wheel and the
//!   assistive Increment and Decrement actions ([`styles::spin_box`]).
//! - **A slider's tick marks are on a fixed side**, below a horizontal
//!   slider and to the right of a vertical one: Breeze draws them on the
//!   side the application asks for, and a `Slider` names none
//!   ([`styles::slider`]).
//! - **A slider's handle is outlined in the hover colour while the pointer
//!   is anywhere on the slider**, not only over the handle, and is
//!   outlined at 3:1 at rest (D6, [`styles::slider`]).
//! - **A check box or radio button row is 24 px, not 22.** Breeze's row is
//!   the label's line box plus 4 px; the widgets keep their 24 px minimum,
//!   WCAG 2.2's target size, and paint the 20 px indicator unchanged in it
//!   (D8, [`styles::checkbox`]).
//! - **The switch is not exactly the desktop style's.** Its label is the
//!   `Toggle`'s, 6 px after the node, where the desktop style spaces it 4 px
//!   after the indicator; keyboard focus adds the 2 px focus frame Breeze
//!   draws around a check box, which the desktop style's switch has not;
//!   the node is raised to the density's target on both axes, the 36 x 18
//!   indicator painted unchanged in it; and a checked track is outlined in
//!   the highlight lifted to 3:1 (D6) ([`styles::toggle`]).
//! - **A table's selection band, alternate rows, grid lines and sort arrow
//!   are the view's.** `TableView` and `TreeTableView` paint them
//!   themselves, from the theme's tokens: the band is square and the
//!   highlight, as Breeze's, but it mutes when the table loses keyboard
//!   focus as well as when the window goes inactive, and a hovered row is
//!   not washed. The style's own row background and sort arrow are
//!   Breeze's, for a style that composes them ([`styles::table`]).
//! - **A tab is at least 96 px wide, and always 30 px tall.** The bar
//!   applies its own minimum tab width, where Breeze's is 80, and a
//!   `TabStyle` is not told whether its bar is one Breeze makes 34 px tall
//!   (a document-mode, static bar); both numbers are in
//!   [`styles::metrics::BreezeTabMetrics`] ([`styles::tab`]).
//! - **A scroll bar is as thick as the widget lays it out.** A
//!   `ScrollArea`'s bars are 12 px and a `ScrollBar`'s 8 px unless the app
//!   says otherwise, where Breeze's is 21; the handle keeps its 8 px width
//!   and a permanent bar its 1 px separator in whatever thickness it is
//!   given. The handle takes the hover colour while the pointer is over the
//!   bar, not only over the handle, and is filled in the lifted thumb
//!   colours (D9). It runs to the bar's ends, where Breeze keeps 3 px free
//!   at each end of a bar without buttons: the widget gives its style no say
//!   over the track ([`styles::scroll_bar`]).
//! - **A translucent paint over what lies behind a control is not Plasma's
//!   shade.** The renderer composites in linear light and Qt's painter in
//!   sRGB, so the push button's 1 px shadow, the scroll bar's separator, a
//!   hovered row's outline, a highlighted menu row's wash and every other
//!   paint laid at an alpha over a backdrop the style does not paint come
//!   out a different shade of the same colour (Breeze Light's button shadow
//!   is #e1e2e3 where Plasma's is #d1d2d3), and antialiased edges blend the
//!   same way. Where the style paints the backdrop itself, as a check box's
//!   wash over its face, it paints Plasma's composite.
//! - **A list row does not mark the current row apart from the selected
//!   one.** Keyboard focus on a focused view adds the focus frame around the
//!   selected row; Breeze outlines an unselected current row instead, which
//!   a row is not told it is ([`styles::standard_item`]).
//! - **A progress bar shows no text.** `ProgressBar` has none to show; the
//!   bar is still Qt's 26 px, room for it, around the 6 px groove
//!   ([`styles::progress_bar`]).
//! - **A short text field keeps its frame.** Breeze paints only the fill of
//!   a line edit shorter than the line box plus 12 px; the field keeps its
//!   outline at every height, the field's only boundary (D6,
//!   [`styles::text_input`]).
//! - **A filled `SplitButton` is filled with the accent.** The widget labels
//!   its `Filled` and `Destructive` variants in the on-accent text and gives
//!   its style no say, so they are framed in the accent rather than as
//!   Breeze's default button, whose light fill would not carry that text in
//!   a dark scheme ([`styles::split_button`]).
//! - **Without a font feature, the stock themes' fonts are not Plasma's.**
//!   The text sizes and line boxes are Noto Sans and Hack's at Plasma's
//!   default sizes, drawn in Teksilo's bundled Inter and JetBrains Mono
//!   unless `bundled-fonts` or `system-fonts` is on ([`fonts`]).
//!   [`from_desktop`] names the desktop's own families instead, which the
//!   text engine finds among the installed fonts or replaces with the
//!   bundled ones.
//! - **A desktop's fonts keep the text styles' weights**, 400 and 600, and
//!   a family other than Noto Sans and Hack gets a line box from Noto
//!   Sans's proportions, which can be a pixel off: the preset measures no
//!   font ([`typography`]). A menu's label is written in the general font,
//!   its row sized for the taller of the general and menu fonts, and a
//!   toolbar's buttons in the general font: Teksilo has no text style for
//!   either, and [`KdeFontRoles`] carries them for an app to apply.
//! - **Some `breezerc` settings change nothing.** The menu opacity, the
//!   window shadow and the busy progress bar follow `breezerc`; the keys
//!   that time Breeze's own style animations, draw its other focus
//!   indicators, or change parts the widgets build themselves (scroll-bar
//!   buttons, tick marks, tree lines, centred tabs, toolbar separators,
//!   mnemonic underlines) are carried and not drawn ([`BreezeStyleConfig`]
//!   lists each).
//! - **The high-contrast selection is light on a light window.** Under the
//!   OS high-contrast preference the selection fill carries the window's
//!   text at 7:1, which keeps it below 4.5:1 on the window (about 1.9:1 for
//!   Breeze Light, 2.2:1 for Breeze Dark): no colour reaches both.
//! - **Widget shapes are chosen at build time.** Switching between this and
//!   another preset at runtime re-tints instantly but keeps the shapes the
//!   tree was built with, a property of the styling system.

mod breeze_config;
mod color;
pub mod fonts;
pub mod geometry;
pub mod motion;
pub mod palette;
pub mod shape;
pub mod styles;
pub mod typography;

pub use breeze_config::BreezeStyleConfig;
#[cfg(feature = "bundled-fonts")]
pub use fonts::font_registrar;
pub use palette::{BreezeColors, KdePalette, PlasmaAccent, UnknownPlasmaAccent};
pub use teksilo_kde_scheme as scheme;
pub use typography::KdeFontRoles;

use std::rc::Rc;
use std::time::Duration;

use teksilo_core::presets::intui;
use teksilo_core::styles::{DensityProjection, PaintProjection, PaintState, Theme};
use teksilo_kde_scheme::{KdeColorScheme, KdeDesktop};
use teksilo_tokens::{
    Color, InputTokens, LayoutTokens, MotionTokens, ShapeTokens, TargetDensity, TypographyTokens,
};

/// The Plasma release whose Breeze style this preset transcribes.
pub const BREEZE_VERSION: &str = "6.8.0";

/// `[M]` The spacing between sibling controls: Breeze's layout spacing.
pub const BREEZE_CONTROL_GAP: f32 = 6.0;
/// `[K]` The spacing between sections: Kirigami's large spacing.
pub const BREEZE_SECTION_GAP: f32 = 8.0;

/// **Breeze Light**, Plasma's default scheme.
pub fn light() -> Theme {
    from_scheme(&KdeColorScheme::breeze_light())
}

/// **Breeze Dark**.
pub fn dark() -> Theme {
    from_scheme(&KdeColorScheme::breeze_dark())
}

/// Breeze Light with `accent` applied as Plasma's colour settings apply it.
///
/// ```
/// use teksilo_theme_kde::{self as kde, PlasmaAccent};
/// let theme = kde::light_with_accent(PlasmaAccent::Green.color());
/// assert!(!theme.is_dark());
/// ```
pub fn light_with_accent(accent: Color) -> Theme {
    from_scheme(&KdeColorScheme::breeze_light().with_accent(accent))
}

/// Breeze Dark with `accent` applied as Plasma's colour settings apply it.
pub fn dark_with_accent(accent: Color) -> Theme {
    from_scheme(&KdeColorScheme::breeze_dark().with_accent(accent))
}

/// The Breeze theme of any KDE colour scheme.
///
/// Light or dark, and the id `kde.light` or `kde.dark`, as KDE judges the
/// scheme ([`KdeColorScheme::is_dark`]). The scheme's colours, frame
/// contrast and inactive and disabled effects all come through; geometry,
/// typography and motion are Breeze's.
pub fn from_scheme(scheme: &KdeColorScheme) -> Theme {
    build(KdePalette::from_scheme(scheme), Desktop::breeze_default())
}

/// The Breeze theme of a KDE desktop as it is configured, at `density`.
///
/// Everything the desktop sets that the preset can follow comes through:
///
/// - **Colours**: the desktop's scheme, as [`from_scheme`] takes it, with
///   the accent the user chose already applied ([`KdeDesktop::scheme`]).
/// - **Fonts**: the general, smallest readable and fixed fonts become the
///   six text styles, in the desktop's own families, at the whole pixel
///   sizes Qt draws them at and on Qt's line boxes
///   ([`typography::desktop_typography`]); the control heights follow the
///   general font's line box ([`geometry`]). The menu and toolbar fonts
///   are carried as [`KdeFontRoles`], and the menu rows are sized for the
///   menu font.
/// - **Animation speed**: `AnimationDurationFactor` scales the motion
///   tokens as Kirigami scales its units, with the tooltip delays and the
///   busy progress period left alone ([`motion::desktop_motion`]). A factor
///   of 0 is the desktop asking for reduced motion: the transitions are
///   then 0 or 1 ms long, and [`BreezeStyleConfig::prefers_reduced_motion`]
///   reports it, as the platform does from the same factor.
/// - **`breezerc`**: the menu opacity, the window shadow's size, colour and
///   strength and the busy progress bar's animation; the theme carries all
///   of it as a [`BreezeStyleConfig`], which lists what each key does.
///
/// The theme's id is `kde.light` or `kde.dark`, as KDE judges the scheme,
/// whatever the desktop's widget style; [`native_theme`] is the form that
/// answers only for a Breeze desktop.
///
/// ```
/// use teksilo_theme_kde::{self as kde, scheme::KdeColorScheme, scheme::KdeDesktop};
/// use teksilo_tokens::TargetDensity;
///
/// let mut desktop = KdeDesktop::new(KdeColorScheme::breeze_dark());
/// desktop.animation_duration_factor = Some(2.0);
/// let theme = kde::from_desktop(&desktop, TargetDensity::Touch);
/// assert!(theme.is_dark());
/// assert_eq!(theme.motion.duration_normal.as_millis(), 400);
/// assert_eq!(theme.input.density, TargetDensity::Touch);
/// ```
pub fn from_desktop(desktop: &KdeDesktop, density: TargetDensity) -> Theme {
    build(
        KdePalette::from_scheme(&desktop.scheme),
        Desktop::from_desktop(desktop),
    )
    .with_density(density)
}

/// The Breeze theme of a KDE desktop for `teksilo-app`'s native-theme
/// seam, or `None` when the desktop does not use the Breeze widget style.
///
/// `Some(`[`from_desktop`]`)` when `kdeglobals [KDE] widgetStyle` is absent,
/// empty or `Breeze` in any ASCII case ([`KdeDesktop::uses_breeze_style`]).
/// A desktop styled with Fusion, Oxygen, Kvantum or any other style gets
/// `None`: its applications do not draw Breeze's shapes, so a Teksilo
/// application keeps the framework's default geometry, coloured by the
/// desktop.
///
/// Meant as `TeksiloAppBuilder::native_theme`'s factory, which is asked
/// with the desktop's report and a density:
///
/// ```no_run
/// use teksilo_app::{TeksiloAppBuilder, ThemeMode};
///
/// TeksiloAppBuilder::new()
///     .theme_mode(ThemeMode::Native)
///     .native_theme(|request| {
///         let desktop = request.desktop.kde.as_ref()?;
///         teksilo_theme_kde::native_theme(desktop, request.density)
///     })
///     .run();
/// ```
///
/// The seam's contract is that a factory's theme is used as it comes:
/// `resolve_native_theme` sets no id and applies no density, and the window
/// manager, which resolves every theme that follows the OS, then gives it
/// the id `system` and projects it onto the density of the window it is
/// for. So the theme this returns carries `kde.light` or `kde.dark` and
/// `density`, and a window in Native mode shows it as `system`. A `None`
/// makes the seam fall back to the framework's IntUI coloured by the
/// desktop's report, which on a KDE session is the same scheme's colours.
pub fn native_theme(desktop: &KdeDesktop, density: TargetDensity) -> Option<Theme> {
    desktop
        .uses_breeze_style()
        .then(|| from_desktop(desktop, density))
}

/// What a theme takes from a desktop besides its colours.
struct Desktop {
    typography: TypographyTokens,
    fonts: KdeFontRoles,
    shape: ShapeTokens,
    motion: MotionTokens,
    config: BreezeStyleConfig,
}

impl Desktop {
    /// A desktop at Plasma's defaults, drawn in the bundled faces.
    fn breeze_default() -> Desktop {
        Desktop {
            typography: typography::breeze_typography(),
            fonts: KdeFontRoles::breeze_default(),
            shape: shape::breeze_shape(),
            motion: motion::breeze_motion(),
            config: BreezeStyleConfig::default(),
        }
    }

    /// What `desktop` configures.
    fn from_desktop(desktop: &KdeDesktop) -> Desktop {
        let config = BreezeStyleConfig::from_desktop(desktop);
        let breeze = &config.settings;
        let busy = match breeze.progress_bar_busy_step_duration_ms {
            ms if ms > 0 => Duration::from_millis(u64::try_from(ms).unwrap_or(u64::MAX)),
            // `[unpublished]` A period of no length cannot move; the bar
            // stands still and the token keeps Breeze's default.
            _ => Duration::from_millis(motion::BREEZE_BUSY_STEP_MS),
        };
        Desktop {
            typography: typography::desktop_typography(&desktop.fonts),
            fonts: KdeFontRoles::from_fonts(&desktop.fonts),
            shape: shape::breeze_shape_for(
                breeze.shadow_size,
                breeze.shadow_color.to_color(),
                config.shadow_strength(),
            ),
            motion: motion::desktop_motion(desktop.animation_factor(), busy),
            config,
        }
    }
}

fn build(palette: KdePalette, desktop: Desktop) -> Theme {
    // Starting from Int UI keeps the input tokens, the density ladder, at
    // the framework's Compact baseline; every other token group is replaced.
    let mut theme = if palette.is_dark() {
        intui::dark().with_id("kde.dark")
    } else {
        intui::light().with_id("kde.light")
    };
    theme.colors = palette.tokens(ACTIVE).clone();
    theme.layout = LayoutTokens {
        control_gap: BREEZE_CONTROL_GAP,
        section_gap: BREEZE_SECTION_GAP,
    };
    theme.typography = desktop.typography;
    theme.shape = desktop.shape;
    theme.motion = desktop.motion;
    theme.extensions.insert(palette);
    theme.extensions.insert(desktop.fonts);
    theme.extensions.insert(desktop.config);
    install_styles(&mut theme);
    theme.extensions.insert(DensityProjection(reproject));
    theme.extensions.insert(PaintProjection(project));
    theme
}

/// The paint state of an active window without the high-contrast
/// preference.
const ACTIVE: PaintState = PaintState {
    window_active: true,
    high_contrast: false,
};

/// Re-derive a Breeze theme for another density.
///
/// Registered as the theme's [`DensityProjection`]. `[unpublished]` KDE
/// publishes no touch ladder, so the input tokens are the framework's
/// generic [`InputTokens::for_density`] ladder; colours, id, the
/// [`KdePalette`] and both projections ride across untouched, and the
/// slots this preset installs are rebuilt for the new tokens.
fn reproject(base: &Theme, density: TargetDensity) -> Theme {
    let mut theme = base.clone();
    theme.input = InputTokens::for_density(density);
    install_styles(&mut theme);
    theme
}

/// Re-colour a Breeze theme for a window state and the high-contrast
/// preference.
///
/// Registered as the theme's [`PaintProjection`]. While the theme's colours
/// are still the active tokens of its [`KdePalette`], the state's own tokens
/// are painted ([`KdePalette::tokens`]). A theme whose colours an app has
/// changed since, or one without the palette, gets the framework's generic
/// inactive and high-contrast projection of its own colours instead, so a
/// change the app made is never painted over.
fn project(theme: &Theme, state: PaintState) -> Theme {
    if state == ACTIVE {
        return theme.clone();
    }
    match theme.extension::<KdePalette>() {
        Some(palette) if theme.colors == *palette.tokens(ACTIVE) => Theme {
            colors: palette.tokens(state).clone(),
            ..theme.clone()
        },
        _ => {
            let mut colors = theme.colors.clone();
            if !state.window_active {
                colors = colors.for_inactive_window();
            }
            if state.high_contrast {
                colors = colors.for_high_contrast();
            }
            Theme {
                colors,
                ..theme.clone()
            }
        }
    }
}

/// Install the Breeze Tier-3 chrome.
///
/// The buttons, the check box, the radio button, the switch, the line edit,
/// the combo box, the spin box, the slider, the menu item, the popup frame,
/// the tooltip, the list row, the table, the tab, the scroll bar and the
/// progress bar are Breeze's own ([`styles`]); the card, the panel, the
/// dialog, the snackbar, the toast, the banner, the link, the badge, the
/// calendar, the search field, the avatar, the splitter, the list container
/// and the segmented control are the stock recipe styles built with Breeze's
/// metrics ([`styles::recipes`]); every other slot is left to the widget's
/// recipe style, which paints from the Breeze tokens, and the crate's tests
/// list the reason for each. Every style installed here reads
/// its metrics from the theme it builds under, `theme.input` included, so
/// [`reproject`] can rerun this alone.
fn install_styles(theme: &mut Theme) {
    let menu_item = styles::menu_item::BreezeMenuItemStyle::for_theme(theme);
    let popover = styles::popover::BreezePopoverStyle::for_tokens(&theme.input);
    let tooltip = styles::tooltip::BreezeTooltipStyle::for_tokens(&theme.input);
    let standard_item = styles::standard_item::BreezeStandardItemStyle::for_theme(theme);
    let table = styles::table::BreezeTableStyle::for_theme(theme);
    let tab = styles::tab::BreezeTabStyle::for_theme(theme);
    let scroll_bar = styles::scroll_bar::BreezeScrollBarStyle::for_tokens(&theme.input);
    let progress_bar = styles::progress_bar::BreezeProgressBarStyle::for_theme(theme);
    let slots = &mut theme.style_slots;
    slots.button = Some(Rc::new(styles::button::BreezeButtonStyle));
    slots.icon_button = Some(Rc::new(styles::icon_button::BreezeIconButtonStyle));
    slots.split_button = Some(Rc::new(styles::split_button::BreezeSplitButtonStyle));
    slots.toggle = Some(Rc::new(styles::toggle::BreezeToggleStyle));
    slots.checkbox = Some(Rc::new(styles::checkbox::BreezeCheckboxStyle));
    slots.radio = Some(Rc::new(styles::radio::BreezeRadioStyle));
    slots.text_input = Some(Rc::new(styles::text_input::BreezeTextInputStyle));
    slots.combo_box = Some(Rc::new(styles::combo_box::BreezeComboBoxStyle));
    slots.spin_box = Some(Rc::new(styles::spin_box::BreezeSpinBoxStyle));
    slots.slider = Some(Rc::new(styles::slider::BreezeSliderStyle));
    slots.menu_item = Some(Rc::new(menu_item));
    slots.popover = Some(Rc::new(popover));
    slots.tooltip = Some(Rc::new(tooltip));
    slots.standard_item = Some(Rc::new(standard_item));
    slots.table = Some(Rc::new(table));
    slots.tab = Some(Rc::new(tab));
    slots.scroll_bar = Some(Rc::new(scroll_bar));
    slots.progress_bar = Some(Rc::new(progress_bar));
    let t = &theme.input;
    slots.card = Some(Rc::new(styles::recipes::breeze_card_style_for(t)));
    slots.panel = Some(Rc::new(styles::recipes::breeze_panel_style_for(t)));
    slots.dialog = Some(Rc::new(styles::recipes::breeze_dialog_style_for(t)));
    slots.snackbar = Some(Rc::new(styles::recipes::breeze_snackbar_style_for(t)));
    slots.toast = Some(Rc::new(styles::recipes::breeze_toast_style_for(t)));
    slots.banner = Some(Rc::new(styles::recipes::breeze_banner_style_for(t)));
    slots.link = Some(Rc::new(styles::recipes::breeze_link_style_for(t)));
    slots.badge = Some(Rc::new(styles::recipes::breeze_badge_style_for(t)));
    slots.calendar = Some(Rc::new(styles::recipes::breeze_calendar_style_for(t)));
    slots.search_field = Some(Rc::new(styles::recipes::breeze_search_field_style_for(t)));
    slots.avatar = Some(Rc::new(styles::recipes::breeze_avatar_style_for(t)));
    slots.splitter = Some(Rc::new(styles::recipes::breeze_splitter_style_for(t)));
    slots.list_container = Some(Rc::new(styles::recipes::breeze_list_container_style_for(t)));
    slots.segmented_control = Some(Rc::new(
        styles::recipes::breeze_segmented_control_style_for(t),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::Path;
    use std::time::Duration;
    use teksilo_core::styles::{ComponentStyleSlots, ThemeAppearance};
    use teksilo_kde_scheme::math::{hsv_darker, hsv_lighter, mix};
    use teksilo_kde_scheme::{
        BackgroundRole, ColorSet, ColorState, DecorationRole, ForegroundRole, KdeColor, PaletteRole,
    };
    use teksilo_tokens::contrast::composite_contrast;
    use teksilo_tokens::{ColorTokens, Easing};

    /// Whether two themes are the same by content.
    ///
    /// `Theme`'s own equality compares style slots by pointer and extensions
    /// by type only, so it cannot tell two independently built themes
    /// apart. The destructuring has no `..`: a field added to `Theme` must
    /// be compared here before this compiles.
    fn assert_same_theme(a: &Theme, b: &Theme) {
        let Theme {
            id,
            appearance,
            colors,
            layout,
            typography,
            shape,
            motion,
            input,
            style_slots,
            extensions,
        } = a;
        assert_eq!(*id, b.id, "id");
        assert_eq!(*appearance, b.appearance, "appearance");
        assert_eq!(*colors, b.colors, "colors");
        assert_eq!(*layout, b.layout, "layout");
        assert_eq!(*typography, b.typography, "typography");
        assert_eq!(*shape, b.shape, "shape");
        assert_eq!(*motion, b.motion, "motion");
        assert_eq!(*input, b.input, "input");
        assert_eq!(
            style_slots.installed(),
            b.style_slots.installed(),
            "installed slots"
        );
        assert_eq!(
            extensions.get::<KdePalette>(),
            b.extension::<KdePalette>(),
            "KDE palette"
        );
        assert_eq!(
            extensions.get::<KdeFontRoles>(),
            b.extension::<KdeFontRoles>(),
            "KDE font roles"
        );
        assert_eq!(
            extensions.get::<BreezeStyleConfig>(),
            b.extension::<BreezeStyleConfig>(),
            "Breeze style config"
        );
    }

    fn paint(theme: &Theme, window_active: bool, high_contrast: bool) -> Theme {
        theme.for_paint_state(PaintState {
            window_active,
            high_contrast,
        })
    }

    fn hex8(c: Color) -> [u8; 4] {
        KdeColor::from_color(c).to_rgba8()
    }

    #[test]
    fn light_reports_light_appearance() {
        assert_eq!(light().appearance, ThemeAppearance::Light);
        assert!(!light().is_dark());
    }

    #[test]
    fn dark_reports_dark_appearance() {
        assert_eq!(dark().appearance, ThemeAppearance::Dark);
        assert!(dark().is_dark());
    }

    #[test]
    fn ids_are_namespaced_and_follow_the_appearance() {
        assert_eq!(light().id.as_str(), "kde.light");
        assert_eq!(dark().id.as_str(), "kde.dark");
        // The widget catalog persists themes by this id.
        assert_eq!(
            light_with_accent(PlasmaAccent::Red.color()).id.as_str(),
            "kde.light"
        );
        assert_eq!(
            dark_with_accent(PlasmaAccent::Red.color()).id.as_str(),
            "kde.dark"
        );
        assert_eq!(
            from_scheme(&KdeColorScheme::breeze_classic()).id.as_str(),
            "kde.light"
        );
    }

    #[test]
    fn from_scheme_of_breeze_light_is_light() {
        assert_same_theme(&from_scheme(&KdeColorScheme::breeze_light()), &light());
        assert_same_theme(&from_scheme(&KdeColorScheme::breeze_dark()), &dark());
    }

    #[test]
    fn every_theme_carries_the_desktop_extensions_at_their_defaults() {
        for t in [
            light(),
            dark(),
            light_with_accent(PlasmaAccent::Red.color()),
        ] {
            assert_eq!(
                t.extension::<BreezeStyleConfig>(),
                Some(&BreezeStyleConfig::default())
            );
            assert_eq!(
                t.extension::<KdeFontRoles>(),
                Some(&KdeFontRoles::breeze_default())
            );
        }
    }

    /// The default desktop is the stock theme but for the families, which
    /// a desktop names and the stock theme leaves to the bundled faces.
    #[test]
    fn a_desktop_at_plasmas_defaults_is_the_stock_theme_in_its_own_faces() {
        let desktop = from_desktop(
            &KdeDesktop::new(KdeColorScheme::breeze_light()),
            TargetDensity::Compact,
        );
        let stock = light();
        let mut renamed = stock.clone();
        renamed.typography = desktop.typography.clone();
        renamed
            .extensions
            .insert(desktop.extension::<KdeFontRoles>().unwrap().clone());
        assert_same_theme(&desktop, &renamed);
        assert_eq!(desktop.typography.body.family, "Noto Sans");
        assert_eq!(desktop.typography.mono.family, "Hack");
        for (a, b) in [
            (&desktop.typography.body, &stock.typography.body),
            (&desktop.typography.small, &stock.typography.small),
            (&desktop.typography.mono, &stock.typography.mono),
        ] {
            assert_eq!(
                (a.size, a.line_height, a.weight),
                (b.size, b.line_height, b.weight)
            );
        }
    }

    #[test]
    fn breezes_own_blue_as_an_accent_still_changes_the_colours() {
        // Plasma's accent application blends the selection from the accent
        // over the view, so applying Breeze's own blue lightens it.
        let accented = light_with_accent(Color::from_hex("#3daee9"));
        let base = light();
        assert_ne!(accented.colors, base.colors);
        assert_eq!(hex8(accented.colors.accent), [119, 198, 239, 255]);
        assert_eq!(hex8(base.colors.accent), [61, 174, 233, 255]);
        // Everything that is not a colour is the same.
        let Theme {
            id,
            appearance,
            layout,
            typography,
            shape,
            motion,
            input,
            ..
        } = &accented;
        assert_eq!(*id, base.id);
        assert_eq!(*appearance, base.appearance);
        assert_eq!(*layout, base.layout);
        assert_eq!(*typography, base.typography);
        assert_eq!(*shape, base.shape);
        assert_eq!(*motion, base.motion);
        assert_eq!(*input, base.input);
    }

    #[test]
    fn surfaces_are_the_breeze_window_backgrounds() {
        assert_eq!(hex8(light().colors.surface_main), [239, 240, 241, 255]);
        assert_eq!(hex8(dark().colors.surface_main), [32, 35, 38, 255]);
        assert_eq!(hex8(light().colors.surface_content), [255, 255, 255, 255]);
        assert_eq!(hex8(dark().colors.surface_content), [20, 22, 24, 255]);
    }

    #[test]
    fn the_accent_is_the_highlight_with_plasmas_black_text() {
        for t in [light(), dark()] {
            assert_eq!(hex8(t.colors.accent), [61, 174, 233, 255]);
            assert_eq!(t.colors.text_on_accent, Color::BLACK);
        }
    }

    #[test]
    fn geometry_is_the_breeze_frame_radius_and_ring() {
        for t in [light(), dark()] {
            assert_eq!(t.shape.radius_control, 5.0);
            assert_eq!(t.shape.radius_popup, 5.0);
            assert_eq!(t.shape.focus_ring_width, 2.0);
            assert_eq!(t.shape.focus_ring_offset, -1.0);
            assert_eq!(t.layout.control_gap, 6.0);
            assert_eq!(t.layout.section_gap, 8.0);
        }
    }

    #[test]
    fn body_is_qts_thirteen_pixel_face_on_an_eighteen_pixel_line() {
        let t = light();
        assert_eq!(t.typography.body.size, 13.0);
        assert!((t.typography.body.size * t.typography.body.line_height - 18.0).abs() < 1e-4);
        assert_eq!(t.typography.body.letter_spacing, 0.0);
    }

    #[test]
    fn motion_is_breezes_linear_hundred_milliseconds() {
        for t in [light(), dark()] {
            assert_eq!(t.motion.duration_fast, Duration::from_millis(100));
            assert_eq!(t.motion.duration_normal, Duration::from_millis(200));
            assert_eq!(t.motion.easing_standard, Easing::Linear);
        }
    }

    #[test]
    fn the_palette_extension_is_present_and_matches_the_appearance() {
        let l = light();
        let d = dark();
        let lp = l.extension::<KdePalette>().expect("light palette");
        let dp = d.extension::<KdePalette>().expect("dark palette");
        assert!(!lp.is_dark());
        assert!(dp.is_dark());
        assert_eq!(
            lp.role(ColorState::Active, PaletteRole::Window),
            l.colors.surface_main
        );
        assert!((lp.frame_contrast() - 0.2).abs() < 1e-12);
    }

    /// Every slot the preset deliberately leaves to the widget's own recipe
    /// style, with the reason it is inherited rather than filled. Each is a
    /// widget Breeze has no counterpart or no metric for, or one whose
    /// dimensions the style cannot set; its recipe paints from the Breeze
    /// tokens all the same.
    const KDE_UNFILLED: &[(&str, &str)] = &[
        (
            "radio_tile",
            "Breeze has no selectable card, so there is no Breeze metric to build it with; \
             the recipe frame paints the Breeze surface, accent, border and focus roles",
        ),
        (
            "chart",
            "Breeze has no chart chrome, and the default ChartStyle lives in teksilo-charts, \
             which this preset does not depend on; it reads the series colours and the Breeze border role",
        ),
        (
            "color_picker",
            "Breeze specifies no colour-picker geometry; the canvas, strips and swatches are \
             colour-space renderers that read the recipe's own dimensions, drawn in the theme roles",
        ),
        (
            "date_edit",
            "the recipe is a passthrough around the TextInput, which this preset frames as Breeze's line edit",
        ),
        (
            "rich_text_editor",
            "a read-only viewer is frameless, and an editable one takes its frame's radius, pen and \
             insets from the stock line-edit recipe rather than the slot, painted in the Breeze roles",
        ),
        (
            "drop_zone",
            "Breeze has no drop-zone control, so there is no Breeze metric to build it with; \
             the recipe tints its region from the theme roles",
        ),
        (
            "drop_target",
            "Breeze has no drop-target control, so there is no Breeze metric to build it with; \
             the recipe border reads the theme roles",
        ),
        (
            "grid_view",
            "Breeze specifies no grid view; the recipe's focus ring, marquee, insertion bar and \
             pinned header are the trait's default decorations, drawn in the roles this preset maps",
        ),
        (
            "web_view",
            "the page paints itself; the recipe only tints the overlay shown over it, from theme roles",
        ),
        (
            "text_selection",
            "Breeze defines no touch selection handles or magnifier, so there is no Breeze \
             metric to build it with; the recipe paints them in the theme roles",
        ),
    ];

    /// Whether the installed slots and [`KDE_UNFILLED`] are exactly
    /// [`ComponentStyleSlots::NAMES`], with no slot in both.
    #[test]
    fn installed_and_unfilled_slots_partition_the_roster() {
        let unfilled: BTreeSet<&str> = KDE_UNFILLED.iter().map(|(name, _)| *name).collect();
        assert_eq!(unfilled.len(), KDE_UNFILLED.len(), "a slot is listed twice");
        for (name, reason) in KDE_UNFILLED {
            assert!(
                !reason.trim().is_empty(),
                "{name} is listed without a reason"
            );
        }
        let names: BTreeSet<&str> = ComponentStyleSlots::NAMES.iter().copied().collect();
        for t in [light(), dark(), light().with_density(TargetDensity::Touch)] {
            let installed: BTreeSet<&str> = t.style_slots.installed().into_iter().collect();
            let both: Vec<_> = installed.intersection(&unfilled).collect();
            assert!(
                both.is_empty(),
                "installed yet listed as unfilled: {both:?}"
            );
            let covered: BTreeSet<&str> = installed.union(&unfilled).copied().collect();
            let missing: Vec<_> = names.difference(&covered).collect();
            let unknown: Vec<_> = covered.difference(&names).collect();
            assert!(
                missing.is_empty() && unknown.is_empty(),
                "neither installed nor listed: {missing:?}; not a slot: {unknown:?}"
            );
        }
    }

    #[test]
    fn an_accent_moves_the_accent_family_and_the_links_but_no_neutral() {
        let t = light_with_accent(PlasmaAccent::Purple.color());
        let base = light();
        assert_ne!(t.colors.accent, base.colors.accent);
        assert_ne!(t.colors.focus_ring, base.colors.focus_ring);
        assert_ne!(t.colors.surface_selected, base.colors.surface_selected);
        // Plasma writes the accent into the link colour too, unlike macOS.
        assert_ne!(t.colors.text_link, base.colors.text_link);
        assert_eq!(t.colors.surface_main, base.colors.surface_main);
        assert_eq!(t.colors.surface_content, base.colors.surface_content);
        assert_eq!(t.colors.text_primary, base.colors.text_primary);
        let palette = t.extension::<KdePalette>().unwrap();
        let [r, g, b, _] = palette
            .resolved()
            .set(ColorState::Active, ColorSet::View)
            .decoration(DecorationRole::Focus)
            .to_rgba8();
        assert_eq!([r, g, b], [184, 117, 220]);
    }

    #[test]
    fn every_swatch_builds_a_usable_theme() {
        for accent in PlasmaAccent::ALL {
            for t in [
                light_with_accent(accent.color()),
                dark_with_accent(accent.color()),
            ] {
                let c = &t.colors;
                for fill in [c.accent, c.accent_hover, c.accent_pressed] {
                    assert!(
                        composite_contrast(c.text_on_accent, fill) >= 4.5,
                        "{accent:?}: the accent label is {:.2}:1",
                        composite_contrast(c.text_on_accent, fill)
                    );
                }
                assert!(t.extension::<KdePalette>().is_some());
            }
        }
    }

    #[test]
    fn the_inactive_projection_paints_the_schemes_muted_selection() {
        for t in [light(), dark()] {
            let palette = t.extension::<KdePalette>().unwrap();
            let inactive = paint(&t, false, false);
            let pi = palette.role(ColorState::Inactive, PaletteRole::Highlight);
            // KDE's Inactive state: the window colour tinted toward the
            // selection colour.
            assert_eq!(inactive.colors.accent, pi);
            assert_ne!(inactive.colors.accent, t.colors.accent);
            let chroma = |c: Color| {
                let (_, s, _) = c.to_hsv();
                s
            };
            assert!(chroma(inactive.colors.accent) < chroma(t.colors.accent));
            // Breeze's inactive effects are off: the neutrals pass through
            // and the focus ring keeps the focus decoration.
            assert_eq!(inactive.colors.surface_main, t.colors.surface_main);
            assert_eq!(inactive.colors.text_primary, t.colors.text_primary);
            assert_eq!(inactive.colors.focus_ring, t.colors.focus_ring);
            assert_eq!(
                inactive.colors,
                *palette.tokens(PaintState {
                    window_active: false,
                    high_contrast: false,
                })
            );
        }
        // The Breeze Light example of the specification: 194,224,245.
        assert_eq!(
            hex8(paint(&light(), false, false).colors.accent),
            [194, 224, 245, 255]
        );
    }

    #[test]
    fn a_scheme_with_inactive_effects_is_honoured() {
        let mut scheme = KdeColorScheme::breeze_light();
        scheme.inactive_effect.enabled = true;
        scheme.inactive_effect.intensity_effect = teksilo_kde_scheme::IntensityEffect::Darken;
        scheme.inactive_effect.intensity_amount = 0.2;
        let t = from_scheme(&scheme);
        let inactive = paint(&t, false, false);
        assert_ne!(inactive.colors.surface_main, t.colors.surface_main);
        let palette = t.extension::<KdePalette>().unwrap();
        assert_eq!(
            inactive.colors.surface_main,
            palette.role(ColorState::Inactive, PaletteRole::Window)
        );
    }

    #[test]
    fn a_colour_the_app_changed_is_never_painted_over() {
        let mut t = light();
        t.colors.accent = Color::from_hex("#FF0000");
        let inactive = paint(&t, false, false);
        assert_eq!(inactive.colors, t.colors.for_inactive_window());
        assert_eq!(paint(&t, true, false).colors, t.colors);
        let mut bare = light();
        bare.extensions.remove::<KdePalette>();
        assert_eq!(
            paint(&bare, true, true).colors,
            bare.colors.for_high_contrast()
        );
    }

    #[test]
    fn the_active_paint_state_is_the_theme_itself() {
        let t = light();
        assert_same_theme(&paint(&t, true, false), &t);
    }

    #[test]
    fn a_density_switch_keeps_the_colours_and_both_projections() {
        let base = light();
        let touch = base.with_density(TargetDensity::Touch);
        assert_eq!(touch.input, InputTokens::for_density(TargetDensity::Touch));
        assert_eq!(touch.input.density, TargetDensity::Touch);
        assert_eq!(touch.id, base.id);
        assert_eq!(touch.colors, base.colors);
        assert_eq!(
            touch.extension::<KdePalette>(),
            base.extension::<KdePalette>()
        );
        assert!(touch.extension::<DensityProjection>().is_some());
        assert!(touch.extension::<PaintProjection>().is_some());
        // The projection still works on the re-derived theme.
        assert_eq!(
            paint(&touch, false, false).colors,
            paint(&base, false, false).colors
        );
    }

    /// The menu metrics `MenuItem`, `MenuSeparator` and `MenuList` read
    /// follow the density a theme is projected to: the installed style is
    /// rebuilt from the projected input tokens.
    #[test]
    fn a_density_switch_reprojects_the_menu_metrics() {
        use teksilo_core::styles::MenuItemStyle;
        let base = light();
        for (density, item, separator) in [
            (TargetDensity::Compact, 29.0, 5.0),
            (TargetDensity::Comfortable, 32.0, 5.75),
            (TargetDensity::Touch, 44.0, 6.5),
        ] {
            let t = base.with_density(density);
            let m = t
                .style_slots
                .menu_item
                .as_ref()
                .expect("a menu style")
                .metrics();
            assert_eq!(m.item_height, item, "{density:?}");
            assert!((m.separator_height - separator).abs() < 1e-4, "{density:?}");
            assert_eq!(m.icon_column_width, 16.0, "{density:?}");
            assert_eq!(
                m,
                styles::menu_item::BreezeMenuItemStyle::for_theme(&t).metrics(),
                "{density:?}"
            );
        }
        // Projecting back to Compact gives Breeze's own numbers again.
        let back = base
            .with_density(TargetDensity::Touch)
            .with_density(TargetDensity::Compact);
        let m = back.style_slots.menu_item.as_ref().unwrap().metrics();
        assert_eq!(m.item_height, 29.0);
    }

    /// The rows a `StandardListItem` lays out follow the density a theme is
    /// projected to: the installed row, table, tab, scroll-bar and
    /// progress-bar styles are rebuilt from the projected input tokens, and
    /// a row measured in a tree is the one the style reports.
    #[test]
    fn a_density_switch_reprojects_the_rows_and_bars() {
        use std::cell::RefCell;
        use teksilo_canvas::{MockTextBackend, SizeProposal};
        use teksilo_core::widget_tree::WidgetTree;
        let base = light();
        for (density, row, table, tab) in [
            (TargetDensity::Compact, 24.0, 24.0, 30.0),
            (TargetDensity::Comfortable, 32.0, 32.0, 32.0),
            (TargetDensity::Touch, 44.0, 44.0, 44.0),
        ] {
            let t = base.with_density(density);
            let slots = &t.style_slots;
            let input = &t.input;
            let rows = slots.standard_item.as_ref().expect("a row style");
            assert_eq!(rows.min_height_single_line(input), row, "{density:?}");
            assert_eq!(
                slots
                    .table
                    .as_ref()
                    .expect("a table style")
                    .row_height(input),
                table,
                "{density:?}"
            );
            assert_eq!(
                slots
                    .tab
                    .as_ref()
                    .expect("a tab style")
                    .editor_height(input),
                tab,
                "{density:?}"
            );
            assert_eq!(
                slots
                    .scroll_bar
                    .as_ref()
                    .expect("a scroll-bar style")
                    .min_thumb_length(input),
                20.0
            );
            assert_eq!(
                slots
                    .progress_bar
                    .as_ref()
                    .expect("a progress-bar style")
                    .thickness(input),
                Some(26.0)
            );
            let mut tree = WidgetTree::new()
                .with_theme(t.clone())
                .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
            let id = tree.add(teksilo_widgets::StandardListItem::new(teksilo_i18n::lit!(
                "Row"
            )));
            tree.layout(SizeProposal {
                width: Some(300.0),
                height: None,
            });
            assert_eq!(tree.bounds(id).height, row, "{density:?}: the rendered row");
        }
    }

    /// The recipe slots built with Breeze's metrics are rebuilt for the
    /// density a theme is projected to: what a widget asking the installed
    /// style is told follows the projected input tokens.
    #[test]
    fn a_density_switch_reprojects_the_recipe_slots() {
        let base = light();
        for (density, factor, arrow) in [
            (TargetDensity::Compact, 1.0, 24.0),
            (TargetDensity::Comfortable, 1.15, 32.0),
            (TargetDensity::Touch, 1.3, 44.0),
        ] {
            let t = base.with_density(density);
            let slots = &t.style_slots;
            let search = slots.search_field.as_ref().expect("a search-field style");
            assert!(
                (search.row_padding_horizontal(&t.input) - 16.0 * factor).abs() < 1e-4,
                "{density:?}"
            );
            assert!(
                (search.row_padding_vertical(&t.input) - 4.0 * factor).abs() < 1e-4,
                "{density:?}"
            );
            let calendar = slots.calendar.as_ref().expect("a calendar style");
            assert_eq!(calendar.nav_arrow_size(&t.input), arrow, "{density:?}");
            for (name, installed) in [
                ("card", slots.card.is_some()),
                ("panel", slots.panel.is_some()),
                ("dialog", slots.dialog.is_some()),
                ("snackbar", slots.snackbar.is_some()),
                ("toast", slots.toast.is_some()),
                ("banner", slots.banner.is_some()),
                ("link", slots.link.is_some()),
                ("badge", slots.badge.is_some()),
                ("avatar", slots.avatar.is_some()),
                ("splitter", slots.splitter.is_some()),
                ("list_container", slots.list_container.is_some()),
                ("segmented_control", slots.segmented_control.is_some()),
            ] {
                assert!(installed, "{density:?}: no {name} style");
            }
        }
    }

    /// Every colour token, pinned to its source in Breeze Light's resolved
    /// palette. The destructuring has no `..`: a field added to
    /// `ColorTokens` must be pinned here before this compiles.
    ///
    /// A lifted token is pinned by its rule: it keeps its source's hue,
    /// reaches its floor, and equals the source where the source already
    /// does (checked in Breeze Dark, where most of them pass).
    #[test]
    fn every_colour_token_comes_from_its_documented_source() {
        let t = light();
        let p = t.extension::<KdePalette>().unwrap().clone();
        let r = |role| p.resolved().role(ColorState::Active, role);
        let d = |role| p.resolved().role(ColorState::Disabled, role);
        let i = |role| p.resolved().role(ColorState::Inactive, role);
        let view = *p.resolved().set(ColorState::Active, ColorSet::View);
        let win = *p.resolved().set(ColorState::Active, ColorSet::Window);
        let tip = *p.resolved().set(ColorState::Active, ColorSet::Tooltip);
        let c = |k: KdeColor| k.to_color();
        let window = r(PaletteRole::Window);
        let base = r(PaletteRole::Base);
        let frame_background = mix(window, base, 0.3);
        let outline = mix(window, r(PaletteRole::WindowText), 0.2);
        let hover_wash = mix(base, r(PaletteRole::Highlight), 0.3);
        let readable = |token: Color, source: KdeColor, floor: f32, on: &[Color]| {
            let (h1, s1, _) = token.to_hsv();
            let (h2, s2, _) = c(source).to_hsv();
            if s1 > 0.05 && s2 > 0.05 {
                let dh = (h1 - h2).abs().min(360.0 - (h1 - h2).abs());
                assert!(dh < 6.0, "{token:?} lost the hue of {source:?}");
            }
            for bg in on {
                assert!(
                    composite_contrast(token, *bg) >= floor,
                    "{token:?} from {source:?} is {:.2}:1 on {bg:?}",
                    composite_contrast(token, *bg)
                );
            }
        };
        let surfaces = [t.colors.surface_main, t.colors.surface_content];
        let kept = ColorTokens::light_default();

        let ColorTokens {
            surface_main,
            surface_content,
            surface_raised,
            surface_sunken,
            surface_hover,
            surface_pressed,
            surface_selected,
            surface_selected_inactive,
            surface_alt_row,
            surface_disabled,
            text_primary,
            text_secondary,
            text_disabled,
            text_on_accent,
            text_link,
            text_link_hover,
            text_link_visited,
            text_error,
            text_warning,
            text_success,
            accent,
            accent_hover,
            accent_pressed,
            accent_disabled,
            accent_subtle_bg,
            border,
            border_strong,
            border_focused,
            border_error,
            border_warning,
            border_disabled,
            divider,
            divider_strong,
            status_info_fg,
            status_info_bg,
            status_success_fg,
            status_success_bg,
            status_warning_fg,
            status_warning_bg,
            status_error_fg,
            status_error_bg,
            selection_bg_active,
            selection_text_active,
            selection_bg_inactive,
            selection_text_inactive,
            search_match_bg,
            search_match_current_bg,
            scrollbar_track,
            scrollbar_track_hover,
            scrollbar_thumb,
            scrollbar_thumb_hover,
            scrollbar_thumb_pressed,
            tooltip_bg,
            tooltip_text,
            tooltip_border,
            tooltip_shortcut,
            editor_bg,
            editor_fg,
            editor_caret,
            editor_current_line_bg,
            editor_gutter_fg,
            editor_selection_bg,
            editor_code_block_bg,
            editor_code_block_fg,
            focus_ring,
            focus_ring_error,
            scrim,
            text_on_error,
            text_on_error_container,
            surface_error_container,
            surface_container,
            surface_container_raised,
            surface_container_sunken,
            chart_palette,
        } = t.colors.clone();

        // Surfaces.
        assert_eq!(surface_main, c(window));
        assert_eq!(surface_content, c(base));
        assert_eq!(surface_raised, c(frame_background));
        assert_eq!(surface_sunken, c(win.background(BackgroundRole::Alternate)));
        assert_eq!(surface_hover, c(hover_wash));
        assert_eq!(
            surface_pressed,
            c(mix(r(PaletteRole::Button), r(PaletteRole::Highlight), 0.3))
        );
        assert_eq!(surface_selected, c(r(PaletteRole::Highlight)));
        assert_eq!(surface_selected_inactive, c(i(PaletteRole::Highlight)));
        assert_eq!(surface_alt_row, c(r(PaletteRole::AlternateBase)));
        assert_eq!(surface_disabled, c(d(PaletteRole::Button)));
        assert_eq!(surface_container, c(frame_background));
        assert_eq!(surface_container_raised, c(base));
        assert_eq!(surface_container_sunken, surface_sunken);
        assert_eq!(
            surface_error_container,
            c(view.background(BackgroundRole::Negative))
        );

        // Text.
        assert_eq!(text_primary, c(r(PaletteRole::WindowText)));
        readable(
            text_secondary,
            win.foreground(ForegroundRole::Inactive),
            4.5,
            &surfaces,
        );
        assert_eq!(text_disabled, c(d(PaletteRole::WindowText)));
        assert_eq!(text_on_accent, Color::BLACK);
        readable(
            text_link,
            view.foreground(ForegroundRole::Link),
            4.5,
            &surfaces,
        );
        readable(
            text_link_hover,
            view.foreground(ForegroundRole::Active),
            4.5,
            &surfaces,
        );
        readable(
            text_link_visited,
            view.foreground(ForegroundRole::Visited),
            4.5,
            &surfaces,
        );
        readable(
            text_error,
            view.foreground(ForegroundRole::Negative),
            4.5,
            &surfaces,
        );
        readable(
            text_warning,
            view.foreground(ForegroundRole::Neutral),
            4.5,
            &surfaces,
        );
        readable(
            text_success,
            view.foreground(ForegroundRole::Positive),
            4.5,
            &surfaces,
        );
        assert_eq!(text_on_error, Color::WHITE);
        assert_eq!(text_on_error_container, text_error);

        // Accent.
        assert_eq!(accent, c(r(PaletteRole::Highlight)));
        assert_eq!(accent_hover, c(hsv_lighter(r(PaletteRole::Highlight), 110)));
        assert_eq!(
            accent_pressed,
            c(hsv_darker(r(PaletteRole::Highlight), 110))
        );
        assert_eq!(accent_disabled, c(d(PaletteRole::Highlight)));
        assert_eq!(accent_subtle_bg, c(hover_wash));

        // Borders.
        assert_eq!(border, c(outline));
        readable(border_strong, outline, 3.0, &surfaces);
        assert_eq!(divider, c(outline));
        assert_eq!(divider_strong, border_strong);
        readable(
            focus_ring,
            view.decoration(DecorationRole::Focus),
            3.0,
            &surfaces,
        );
        assert_eq!(border_focused, focus_ring);
        readable(
            border_error,
            view.foreground(ForegroundRole::Negative),
            3.0,
            &surfaces,
        );
        readable(
            border_warning,
            view.foreground(ForegroundRole::Neutral),
            3.0,
            &surfaces,
        );
        assert_eq!(focus_ring_error, border_error);
        assert_eq!(
            border_disabled,
            c(mix(d(PaletteRole::Window), d(PaletteRole::WindowText), 0.2))
        );

        // Status.
        assert_eq!(status_error_fg, text_error);
        assert_eq!(status_warning_fg, text_warning);
        assert_eq!(status_success_fg, text_success);
        readable(
            status_info_fg,
            view.foreground(ForegroundRole::Active),
            4.5,
            &surfaces,
        );
        assert_eq!(status_info_bg, c(view.background(BackgroundRole::Active)));
        assert_eq!(
            status_success_bg,
            c(view.background(BackgroundRole::Positive))
        );
        assert_eq!(
            status_warning_bg,
            c(view.background(BackgroundRole::Neutral))
        );
        assert_eq!(
            status_error_bg,
            c(view.background(BackgroundRole::Negative))
        );

        // Selection.
        assert_eq!(selection_bg_active, surface_selected);
        assert_eq!(selection_text_active, Color::BLACK);
        assert_eq!(selection_bg_inactive, surface_selected_inactive);
        assert_eq!(selection_text_inactive, c(i(PaletteRole::HighlightedText)));
        assert_eq!(search_match_bg, kept.search_match_bg);
        assert_eq!(search_match_current_bg, kept.search_match_current_bg);

        // Scroll bar.
        assert_eq!(scrollbar_track, Color::TRANSPARENT);
        assert_eq!(scrollbar_track_hover, Color::TRANSPARENT);
        readable(
            scrollbar_thumb,
            mix(window, r(PaletteRole::WindowText), 0.175),
            3.0,
            &surfaces,
        );
        readable(
            scrollbar_thumb_hover,
            mix(window, view.decoration(DecorationRole::Hover), 0.5),
            3.0,
            &surfaces,
        );
        assert_eq!(scrollbar_thumb_pressed, scrollbar_thumb_hover);

        // Tooltip.
        let tip_bg = tip.background(BackgroundRole::Normal);
        let tip_fg = tip.foreground(ForegroundRole::Normal);
        assert_eq!(tooltip_bg, c(tip_bg));
        assert_eq!(tooltip_text, c(tip_fg));
        assert_eq!(tooltip_border, c(mix(tip_bg, tip_fg, 0.2)));
        readable(
            tooltip_shortcut,
            tip.foreground(ForegroundRole::Inactive),
            4.5,
            &[tooltip_bg],
        );

        // Editor.
        assert_eq!(editor_bg, c(base));
        assert_eq!(editor_fg, c(r(PaletteRole::Text)));
        assert_eq!(editor_caret, editor_fg);
        assert_eq!(editor_current_line_bg, c(r(PaletteRole::AlternateBase)));
        readable(
            editor_gutter_fg,
            view.foreground(ForegroundRole::Inactive),
            4.5,
            &[editor_bg, editor_current_line_bg],
        );
        assert_eq!(editor_selection_bg, surface_selected);
        assert_eq!(editor_code_block_bg, c(mix(base, window, 0.5)));
        assert_eq!(editor_code_block_fg, editor_fg);

        // Kept from Int UI.
        assert_eq!(scrim, kept.scrim);
        assert_eq!(chart_palette, kept.chart_palette);

        // Where the source already reaches its floor the lift leaves it
        // alone: Breeze Dark's tooltip shortcut text is readable as it is.
        let dark = super::dark();
        let dp = dark.extension::<KdePalette>().unwrap();
        let dark_shortcut = dp
            .resolved()
            .set(ColorState::Active, ColorSet::Tooltip)
            .foreground(ForegroundRole::Inactive);
        assert!(composite_contrast(dark_shortcut.to_color(), dark.colors.tooltip_bg) >= 4.5);
        assert_eq!(dark.colors.tooltip_shortcut, dark_shortcut.to_color());
    }

    /// No assignment may stay tagged as unverified: the tag means a value
    /// the specification has not settled.
    #[test]
    fn no_unverified_tags() {
        let needle = concat!("[", "unverified", "]");
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut found = Vec::new();
        let mut scanned = 0;
        let mut stack = vec![root.join("src"), root.join("tests")];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries {
                let path = entry.expect("a directory entry").path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    scanned += 1;
                    let text = std::fs::read_to_string(&path).expect("readable source");
                    for (n, line) in text.lines().enumerate() {
                        if line.contains(needle) {
                            found.push(format!("{}:{}", path.display(), n + 1));
                        }
                    }
                }
            }
        }
        assert!(scanned > 5, "only {scanned} source files were scanned");
        assert!(
            found.is_empty(),
            "unverified values remain:\n{}",
            found.join("\n")
        );
    }
}
