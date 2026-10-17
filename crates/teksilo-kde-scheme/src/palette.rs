// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A scheme resolved into the colours an application draws with.
//!
//! Implemented from sections f and i of Teksilo's specification
//! (`engineering/docs/kde-breeze-spec.md`).
//!
//! [`KdeColorScheme::resolve`] gives a [`ResolvedPalette`]: for each of the
//! three states and seven sets, a [`ResolvedSet`] of
//!
//! - 8 foregrounds ([`ForegroundRole`]), as stored and after the state's
//!   effects ([`crate::effects`]);
//! - 8 backgrounds ([`BackgroundRole`]): the two stored ones after the
//!   effects, and six computed ones, each the `Normal` background tinted
//!   toward one of the foregrounds by [`COMPUTED_BACKGROUND_TINT`];
//! - 2 decorations ([`DecorationRole`]);
//! - 5 shades ([`ShadeRole`]) of the `Normal` background, at the scheme's
//!   contrast value ([`KdeColorScheme::contrast_value`]).
//!
//! [`PaletteRole`] names the twenty colours an application palette takes
//! from those sets, read with [`ResolvedPalette::role`].
//!
//! [`KdeColorScheme::is_dark`] is the light or dark decision KDE reports to
//! applications and portals, and [`KdeColorScheme::os_theme_colors`] the
//! report Teksilo's native theme builds its tokens from.

use teksilo_tokens::{ColorSchemePreference, OsThemeColors};

use crate::color::KdeColor;
use crate::math::{ShadeRole, shade_role, tint};
use crate::scheme::{ColorRole, ColorSet, ColorState, KdeColorScheme, SetColors};

/// How far each computed background is tinted from the `Normal` background
/// toward its foreground.
pub const COMPUTED_BACKGROUND_TINT: f64 = 0.3;

/// One of the eight foregrounds of a resolved set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ForegroundRole {
    /// Ordinary text, `ForegroundNormal`.
    Normal,
    /// Secondary text, `ForegroundInactive`.
    Inactive,
    /// Active or hot text, `ForegroundActive`.
    Active,
    /// An unvisited link, `ForegroundLink`.
    Link,
    /// A visited link, `ForegroundVisited`.
    Visited,
    /// Errors, `ForegroundNegative`.
    Negative,
    /// Warnings, `ForegroundNeutral`.
    Neutral,
    /// Success, `ForegroundPositive`.
    Positive,
}

impl ForegroundRole {
    /// The eight foregrounds, in declaration order.
    pub const ALL: [ForegroundRole; 8] = [
        ForegroundRole::Normal,
        ForegroundRole::Inactive,
        ForegroundRole::Active,
        ForegroundRole::Link,
        ForegroundRole::Visited,
        ForegroundRole::Negative,
        ForegroundRole::Neutral,
        ForegroundRole::Positive,
    ];

    /// The stored key the foreground comes from.
    pub const fn stored_role(self) -> ColorRole {
        match self {
            ForegroundRole::Normal => ColorRole::ForegroundNormal,
            ForegroundRole::Inactive => ColorRole::ForegroundInactive,
            ForegroundRole::Active => ColorRole::ForegroundActive,
            ForegroundRole::Link => ColorRole::ForegroundLink,
            ForegroundRole::Visited => ColorRole::ForegroundVisited,
            ForegroundRole::Negative => ColorRole::ForegroundNegative,
            ForegroundRole::Neutral => ColorRole::ForegroundNeutral,
            ForegroundRole::Positive => ColorRole::ForegroundPositive,
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

/// One of the eight backgrounds of a resolved set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BackgroundRole {
    /// The ordinary background, `BackgroundNormal`.
    Normal,
    /// Alternate rows, `BackgroundAlternate`.
    Alternate,
    /// Computed: `Normal` tinted toward [`ForegroundRole::Active`].
    Active,
    /// Computed: `Normal` tinted toward [`ForegroundRole::Link`].
    Link,
    /// Computed: `Normal` tinted toward [`ForegroundRole::Visited`].
    Visited,
    /// Computed: `Normal` tinted toward [`ForegroundRole::Negative`].
    Negative,
    /// Computed: `Normal` tinted toward [`ForegroundRole::Neutral`].
    Neutral,
    /// Computed: `Normal` tinted toward [`ForegroundRole::Positive`].
    Positive,
}

impl BackgroundRole {
    /// The eight backgrounds, in declaration order.
    pub const ALL: [BackgroundRole; 8] = [
        BackgroundRole::Normal,
        BackgroundRole::Alternate,
        BackgroundRole::Active,
        BackgroundRole::Link,
        BackgroundRole::Visited,
        BackgroundRole::Negative,
        BackgroundRole::Neutral,
        BackgroundRole::Positive,
    ];

    /// The foreground a computed background is tinted toward, `None` for
    /// the two stored backgrounds.
    pub const fn tinted_toward(self) -> Option<ForegroundRole> {
        match self {
            BackgroundRole::Normal | BackgroundRole::Alternate => None,
            BackgroundRole::Active => Some(ForegroundRole::Active),
            BackgroundRole::Link => Some(ForegroundRole::Link),
            BackgroundRole::Visited => Some(ForegroundRole::Visited),
            BackgroundRole::Negative => Some(ForegroundRole::Negative),
            BackgroundRole::Neutral => Some(ForegroundRole::Neutral),
            BackgroundRole::Positive => Some(ForegroundRole::Positive),
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

/// One of the two decorations of a resolved set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DecorationRole {
    /// The keyboard focus indicator, `DecorationFocus`.
    Focus,
    /// The hover indicator, `DecorationHover`.
    Hover,
}

impl DecorationRole {
    /// Both decorations.
    pub const ALL: [DecorationRole; 2] = [DecorationRole::Focus, DecorationRole::Hover];

    const fn index(self) -> usize {
        self as usize
    }
}

const fn shade_index(role: ShadeRole) -> usize {
    match role {
        ShadeRole::Light => 0,
        ShadeRole::Midlight => 1,
        ShadeRole::Mid => 2,
        ShadeRole::Dark => 3,
        ShadeRole::Shadow => 4,
    }
}

/// The colours of one set in one state, every effect applied and every
/// derived colour computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResolvedSet {
    foregrounds: [KdeColor; 8],
    backgrounds: [KdeColor; 8],
    decorations: [KdeColor; 2],
    shades: [KdeColor; 5],
}

impl ResolvedSet {
    /// Completes a set's twelve colours, already through the state's
    /// effects ([`KdeColorScheme::state_colors`]): the six computed
    /// backgrounds and the five shades of `BackgroundNormal` at
    /// `contrast_value` (the scheme's `contrast` divided by 10).
    pub fn from_state_colors(colors: &SetColors, contrast_value: f64) -> ResolvedSet {
        let foregrounds = ForegroundRole::ALL.map(|role| colors.get(role.stored_role()));
        let normal = colors.background_normal;
        let backgrounds = BackgroundRole::ALL.map(|role| match (role, role.tinted_toward()) {
            (BackgroundRole::Normal, _) => normal,
            (_, Some(foreground)) => tint(
                normal,
                foregrounds[foreground.index()],
                COMPUTED_BACKGROUND_TINT,
            ),
            (_, None) => colors.background_alternate,
        });
        ResolvedSet {
            foregrounds,
            backgrounds,
            decorations: [colors.decoration_focus, colors.decoration_hover],
            shades: ShadeRole::ALL.map(|role| shade_role(normal, role, contrast_value, 0.0)),
        }
    }

    /// The foreground of `role`.
    pub const fn foreground(&self, role: ForegroundRole) -> KdeColor {
        self.foregrounds[role.index()]
    }

    /// The background of `role`.
    pub const fn background(&self, role: BackgroundRole) -> KdeColor {
        self.backgrounds[role.index()]
    }

    /// The decoration of `role`.
    pub const fn decoration(&self, role: DecorationRole) -> KdeColor {
        self.decorations[role.index()]
    }

    /// The shade of `BackgroundNormal` for `role`.
    pub const fn shade(&self, role: ShadeRole) -> KdeColor {
        self.shades[shade_index(role)]
    }
}

/// A colour of an application palette, and the resolved set it is read
/// from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PaletteRole {
    /// Window background: Window `BackgroundNormal`.
    Window,
    /// Text on the window: Window `ForegroundNormal`.
    WindowText,
    /// Content-view background: View `BackgroundNormal`.
    Base,
    /// Content-view text: View `ForegroundNormal`.
    Text,
    /// Alternate rows: View `BackgroundAlternate`.
    AlternateBase,
    /// Unvisited links: View `ForegroundLink`.
    Link,
    /// Visited links: View `ForegroundVisited`.
    LinkVisited,
    /// Placeholder text in an empty field: View `ForegroundInactive`.
    PlaceholderText,
    /// Button background: Button `BackgroundNormal`.
    Button,
    /// Button text: Button `ForegroundNormal`.
    ButtonText,
    /// Selection background: Selection `BackgroundNormal`.
    Highlight,
    /// Selected text: Selection `ForegroundNormal`.
    HighlightedText,
    /// The accent colour, the same as [`PaletteRole::Highlight`].
    Accent,
    /// Tooltip background: Tooltip `BackgroundNormal` of the Active state,
    /// whatever state is asked for.
    TooltipBase,
    /// Tooltip text: Tooltip `ForegroundNormal` of the Active state,
    /// whatever state is asked for.
    TooltipText,
    /// The light shade of the Window `BackgroundNormal`.
    Light,
    /// The midlight shade of the Window `BackgroundNormal`.
    Midlight,
    /// The mid shade of the Window `BackgroundNormal`.
    Mid,
    /// The dark shade of the Window `BackgroundNormal`.
    Dark,
    /// The shadow shade of the Window `BackgroundNormal`.
    Shadow,
}

impl PaletteRole {
    /// Every palette role, in declaration order.
    pub const ALL: [PaletteRole; 20] = [
        PaletteRole::Window,
        PaletteRole::WindowText,
        PaletteRole::Base,
        PaletteRole::Text,
        PaletteRole::AlternateBase,
        PaletteRole::Link,
        PaletteRole::LinkVisited,
        PaletteRole::PlaceholderText,
        PaletteRole::Button,
        PaletteRole::ButtonText,
        PaletteRole::Highlight,
        PaletteRole::HighlightedText,
        PaletteRole::Accent,
        PaletteRole::TooltipBase,
        PaletteRole::TooltipText,
        PaletteRole::Light,
        PaletteRole::Midlight,
        PaletteRole::Mid,
        PaletteRole::Dark,
        PaletteRole::Shadow,
    ];
}

/// Every colour of a scheme, resolved for the three states and seven sets.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolvedPalette {
    /// Indexed by [`ColorState::index`], then [`ColorSet::index`].
    sets: [[ResolvedSet; 7]; 3],
}

impl ResolvedPalette {
    /// The resolved colours of `set` in `state`.
    pub const fn set(&self, state: ColorState, set: ColorSet) -> &ResolvedSet {
        &self.sets[state.index()][set.index()]
    }

    /// The palette colour of `role` in `state`.
    pub const fn role(&self, state: ColorState, role: PaletteRole) -> KdeColor {
        let window = self.set(state, ColorSet::Window);
        let view = self.set(state, ColorSet::View);
        let button = self.set(state, ColorSet::Button);
        let selection = self.set(state, ColorSet::Selection);
        let tooltip = self.set(ColorState::Active, ColorSet::Tooltip);
        match role {
            PaletteRole::Window => window.background(BackgroundRole::Normal),
            PaletteRole::WindowText => window.foreground(ForegroundRole::Normal),
            PaletteRole::Base => view.background(BackgroundRole::Normal),
            PaletteRole::Text => view.foreground(ForegroundRole::Normal),
            PaletteRole::AlternateBase => view.background(BackgroundRole::Alternate),
            PaletteRole::Link => view.foreground(ForegroundRole::Link),
            PaletteRole::LinkVisited => view.foreground(ForegroundRole::Visited),
            PaletteRole::PlaceholderText => view.foreground(ForegroundRole::Inactive),
            PaletteRole::Button => button.background(BackgroundRole::Normal),
            PaletteRole::ButtonText => button.foreground(ForegroundRole::Normal),
            PaletteRole::Highlight | PaletteRole::Accent => {
                selection.background(BackgroundRole::Normal)
            }
            PaletteRole::HighlightedText => selection.foreground(ForegroundRole::Normal),
            PaletteRole::TooltipBase => tooltip.background(BackgroundRole::Normal),
            PaletteRole::TooltipText => tooltip.foreground(ForegroundRole::Normal),
            PaletteRole::Light => window.shade(ShadeRole::Light),
            PaletteRole::Midlight => window.shade(ShadeRole::Midlight),
            PaletteRole::Mid => window.shade(ShadeRole::Mid),
            PaletteRole::Dark => window.shade(ShadeRole::Dark),
            PaletteRole::Shadow => window.shade(ShadeRole::Shadow),
        }
    }
}

/// The 8-bit grey level below which a window background counts as dark.
pub const DARK_GRAY_THRESHOLD: u32 = 192;

impl KdeColorScheme {
    /// The contrast value the shades are computed with: `[KDE] contrast`
    /// times 0.1, unclamped here (0.4 for Breeze, 0.7 by default).
    pub fn contrast_value(&self) -> f64 {
        0.1 * f64::from(self.contrast)
    }

    /// Resolves every set in every state: the state's colour source and
    /// effects ([`KdeColorScheme::state_colors`]), then the computed
    /// backgrounds and shades ([`ResolvedSet::from_state_colors`]).
    ///
    /// The Tooltip set is resolved in every state too, although the palette
    /// roles read only its Active colours.
    pub fn resolve(&self) -> ResolvedPalette {
        let k = self.contrast_value();
        ResolvedPalette {
            sets: ColorState::ALL.map(|state| {
                ColorSet::ALL
                    .map(|set| ResolvedSet::from_state_colors(&self.state_colors(state, set), k))
            }),
        }
    }

    /// Whether the scheme is dark, as KDE reports it to applications and
    /// to the desktop portal.
    ///
    /// The Window `BackgroundNormal` of the Active state (which no effect
    /// changes) is reduced to 8-bit channels `r`, `g`, `b`, and its grey
    /// level `(11 r + 16 g + 5 b) / 32` (integer division) is compared with
    /// [`DARK_GRAY_THRESHOLD`]: below it, the scheme is dark. Only the
    /// window decides: a dark window with light content views is dark.
    pub fn is_dark(&self) -> bool {
        let [r, g, b, _] = self.set(ColorSet::Window).background_normal.to_rgba8();
        let gray = (11 * u32::from(r) + 16 * u32::from(g) + 5 * u32::from(b)) / 32;
        gray < DARK_GRAY_THRESHOLD
    }

    /// The scheme as the colour report Teksilo's native theme is built
    /// from, every colour taken from the Active state:
    ///
    /// | Field | Source |
    /// |---|---|
    /// | `color_scheme` | [`KdeColorScheme::is_dark`] |
    /// | `accent`, `selection_bg` | Selection `BackgroundNormal` |
    /// | `selection_fg` | Selection `ForegroundNormal` |
    /// | `window_bg`, `window_fg` | Window `BackgroundNormal`, `ForegroundNormal` |
    /// | `button_bg`, `button_fg` | Button `BackgroundNormal`, `ForegroundNormal` |
    /// | `tooltip_bg`, `tooltip_fg` | Tooltip `BackgroundNormal`, `ForegroundNormal` |
    /// | `view_bg`, `view_fg` | View `BackgroundNormal`, `ForegroundNormal` |
    /// | `alternate_bg` | View `BackgroundAlternate` |
    /// | `inactive_fg` | View `ForegroundInactive` |
    /// | `link`, `link_visited` | View `ForegroundLink`, `ForegroundVisited` |
    /// | `error`, `warning`, `success` | View `ForegroundNegative`, `ForegroundNeutral`, `ForegroundPositive` |
    /// | `focus` | View `DecorationFocus` |
    ///
    /// The accent is the selection colour, as KDE's application palette
    /// and the portal's accent colour both report it.
    pub fn os_theme_colors(&self) -> OsThemeColors {
        let scheme = if self.is_dark() {
            ColorSchemePreference::Dark
        } else {
            ColorSchemePreference::Light
        };
        let some = |color: KdeColor| Some(color.to_color());
        let view = self.set(ColorSet::View);
        let window = self.set(ColorSet::Window);
        let button = self.set(ColorSet::Button);
        let selection = self.set(ColorSet::Selection);
        let tooltip = self.set(ColorSet::Tooltip);

        let mut os = OsThemeColors::new(scheme);
        os.accent = some(selection.background_normal);
        os.window_bg = some(window.background_normal);
        os.window_fg = some(window.foreground_normal);
        os.button_bg = some(button.background_normal);
        os.button_fg = some(button.foreground_normal);
        os.selection_bg = some(selection.background_normal);
        os.selection_fg = some(selection.foreground_normal);
        os.tooltip_bg = some(tooltip.background_normal);
        os.tooltip_fg = some(tooltip.foreground_normal);
        os.view_bg = some(view.background_normal);
        os.view_fg = some(view.foreground_normal);
        os.alternate_bg = some(view.background_alternate);
        os.inactive_fg = some(view.foreground_inactive);
        os.link = some(view.foreground_link);
        os.link_visited = some(view.foreground_visited);
        os.error = some(view.foreground_negative);
        os.warning = some(view.foreground_neutral);
        os.success = some(view.foreground_positive);
        os.focus = some(view.decoration_focus);
        os
    }
}
