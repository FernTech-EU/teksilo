// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The KDE colour-scheme model: what a `.colors` file, or the colour groups
//! of `kdeglobals`, says.
//!
//! A scheme has seven colour sets ([`ColorSet`]), each with twelve stored
//! roles ([`ColorRole`], gathered in [`SetColors`]), an optional
//! replacement of each set for inactive windows (an `[Inactive]` subgroup),
//! the settings of the Disabled and Inactive state effects
//! ([`StateEffect`]), the title-bar colours ([`WmColors`]) and a few
//! scalars.
//!
//! [`KdeColorScheme::from_config`] fills every key a configuration lacks
//! from KDE's compiled defaults, so a scheme is always complete:
//!
//! - every set falls back to its own default table;
//! - the Header set falls back, key by key, to the same key of the Window
//!   group when that group has it, and to the Header table otherwise;
//! - an `[Inactive]` subgroup replaces its set as a whole: a key it lacks
//!   takes the same default the set would, never the parent group's value;
//! - `[KDE] contrast` defaults to 7 and `[KDE] frameContrast` to 0.2,
//!   clamped to `0..=1`.

use std::fmt;
use std::sync::Arc;

use crate::color::KdeColor;
use crate::config::{KConfig, ParseDiagnostic, escape_value};

/// One of the seven colour sets of a scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ColorSet {
    /// Content views: lists, text fields, documents.
    View,
    /// Window backgrounds and the controls drawn on them.
    Window,
    /// Push buttons.
    Button,
    /// Selected items and the highlight.
    Selection,
    /// Tooltips.
    Tooltip,
    /// Areas drawn in the opposite light/dark sense, such as a dark panel
    /// in a light scheme.
    Complementary,
    /// Headers: title bars, toolbars, column headers.
    Header,
}

impl ColorSet {
    /// Every set, in the order the model stores them.
    pub const ALL: [ColorSet; 7] = [
        ColorSet::View,
        ColorSet::Window,
        ColorSet::Button,
        ColorSet::Selection,
        ColorSet::Tooltip,
        ColorSet::Complementary,
        ColorSet::Header,
    ];

    /// The configuration group the set is stored in, such as
    /// `Colors:View`.
    pub const fn group_name(self) -> &'static str {
        match self {
            ColorSet::View => "Colors:View",
            ColorSet::Window => "Colors:Window",
            ColorSet::Button => "Colors:Button",
            ColorSet::Selection => "Colors:Selection",
            ColorSet::Tooltip => "Colors:Tooltip",
            ColorSet::Complementary => "Colors:Complementary",
            ColorSet::Header => "Colors:Header",
        }
    }

    /// The set's position in [`ColorSet::ALL`].
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// One of the three states (colour groups) a palette is resolved for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ColorState {
    /// A focused window's normal controls.
    Active,
    /// Controls in a window that does not have focus.
    Inactive,
    /// Disabled controls.
    Disabled,
}

impl ColorState {
    /// Every state.
    pub const ALL: [ColorState; 3] = [
        ColorState::Active,
        ColorState::Inactive,
        ColorState::Disabled,
    ];

    /// The state's position in [`ColorState::ALL`].
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// One of the twelve colours a set stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ColorRole {
    /// `BackgroundNormal`.
    BackgroundNormal,
    /// `BackgroundAlternate`, alternate rows.
    BackgroundAlternate,
    /// `ForegroundNormal`, ordinary text.
    ForegroundNormal,
    /// `ForegroundInactive`, secondary text.
    ForegroundInactive,
    /// `ForegroundActive`.
    ForegroundActive,
    /// `ForegroundLink`.
    ForegroundLink,
    /// `ForegroundVisited`.
    ForegroundVisited,
    /// `ForegroundNegative`, errors.
    ForegroundNegative,
    /// `ForegroundNeutral`, warnings.
    ForegroundNeutral,
    /// `ForegroundPositive`, success.
    ForegroundPositive,
    /// `DecorationFocus`, the focus indicator.
    DecorationFocus,
    /// `DecorationHover`, the hover indicator.
    DecorationHover,
}

impl ColorRole {
    /// Every role, in the order [`SetColors`] declares them.
    pub const ALL: [ColorRole; 12] = [
        ColorRole::BackgroundNormal,
        ColorRole::BackgroundAlternate,
        ColorRole::ForegroundNormal,
        ColorRole::ForegroundInactive,
        ColorRole::ForegroundActive,
        ColorRole::ForegroundLink,
        ColorRole::ForegroundVisited,
        ColorRole::ForegroundNegative,
        ColorRole::ForegroundNeutral,
        ColorRole::ForegroundPositive,
        ColorRole::DecorationFocus,
        ColorRole::DecorationHover,
    ];

    /// The configuration key, such as `BackgroundNormal`.
    pub const fn key(self) -> &'static str {
        match self {
            ColorRole::BackgroundNormal => "BackgroundNormal",
            ColorRole::BackgroundAlternate => "BackgroundAlternate",
            ColorRole::ForegroundNormal => "ForegroundNormal",
            ColorRole::ForegroundInactive => "ForegroundInactive",
            ColorRole::ForegroundActive => "ForegroundActive",
            ColorRole::ForegroundLink => "ForegroundLink",
            ColorRole::ForegroundVisited => "ForegroundVisited",
            ColorRole::ForegroundNegative => "ForegroundNegative",
            ColorRole::ForegroundNeutral => "ForegroundNeutral",
            ColorRole::ForegroundPositive => "ForegroundPositive",
            ColorRole::DecorationFocus => "DecorationFocus",
            ColorRole::DecorationHover => "DecorationHover",
        }
    }
}

/// The twelve stored colours of one set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SetColors {
    /// `BackgroundNormal`.
    pub background_normal: KdeColor,
    /// `BackgroundAlternate`.
    pub background_alternate: KdeColor,
    /// `ForegroundNormal`.
    pub foreground_normal: KdeColor,
    /// `ForegroundInactive`.
    pub foreground_inactive: KdeColor,
    /// `ForegroundActive`.
    pub foreground_active: KdeColor,
    /// `ForegroundLink`.
    pub foreground_link: KdeColor,
    /// `ForegroundVisited`.
    pub foreground_visited: KdeColor,
    /// `ForegroundNegative`.
    pub foreground_negative: KdeColor,
    /// `ForegroundNeutral`.
    pub foreground_neutral: KdeColor,
    /// `ForegroundPositive`.
    pub foreground_positive: KdeColor,
    /// `DecorationFocus`.
    pub decoration_focus: KdeColor,
    /// `DecorationHover`.
    pub decoration_hover: KdeColor,
}

impl SetColors {
    /// A set from its twelve colours as 8-bit triples, in
    /// [`ColorRole::ALL`] order.
    pub const fn from_rgb8(rows: [[u8; 3]; 12]) -> SetColors {
        const fn c(rgb: [u8; 3]) -> KdeColor {
            KdeColor::rgb8(rgb[0], rgb[1], rgb[2])
        }
        SetColors {
            background_normal: c(rows[0]),
            background_alternate: c(rows[1]),
            foreground_normal: c(rows[2]),
            foreground_inactive: c(rows[3]),
            foreground_active: c(rows[4]),
            foreground_link: c(rows[5]),
            foreground_visited: c(rows[6]),
            foreground_negative: c(rows[7]),
            foreground_neutral: c(rows[8]),
            foreground_positive: c(rows[9]),
            decoration_focus: c(rows[10]),
            decoration_hover: c(rows[11]),
        }
    }

    /// The colour of `role`.
    pub const fn get(&self, role: ColorRole) -> KdeColor {
        match role {
            ColorRole::BackgroundNormal => self.background_normal,
            ColorRole::BackgroundAlternate => self.background_alternate,
            ColorRole::ForegroundNormal => self.foreground_normal,
            ColorRole::ForegroundInactive => self.foreground_inactive,
            ColorRole::ForegroundActive => self.foreground_active,
            ColorRole::ForegroundLink => self.foreground_link,
            ColorRole::ForegroundVisited => self.foreground_visited,
            ColorRole::ForegroundNegative => self.foreground_negative,
            ColorRole::ForegroundNeutral => self.foreground_neutral,
            ColorRole::ForegroundPositive => self.foreground_positive,
            ColorRole::DecorationFocus => self.decoration_focus,
            ColorRole::DecorationHover => self.decoration_hover,
        }
    }

    /// Replaces the colour of `role`.
    pub fn set(&mut self, role: ColorRole, color: KdeColor) {
        let slot = match role {
            ColorRole::BackgroundNormal => &mut self.background_normal,
            ColorRole::BackgroundAlternate => &mut self.background_alternate,
            ColorRole::ForegroundNormal => &mut self.foreground_normal,
            ColorRole::ForegroundInactive => &mut self.foreground_inactive,
            ColorRole::ForegroundActive => &mut self.foreground_active,
            ColorRole::ForegroundLink => &mut self.foreground_link,
            ColorRole::ForegroundVisited => &mut self.foreground_visited,
            ColorRole::ForegroundNegative => &mut self.foreground_negative,
            ColorRole::ForegroundNeutral => &mut self.foreground_neutral,
            ColorRole::ForegroundPositive => &mut self.foreground_positive,
            ColorRole::DecorationFocus => &mut self.decoration_focus,
            ColorRole::DecorationHover => &mut self.decoration_hover,
        };
        *slot = color;
    }

    /// The same set with both decoration colours replaced.
    const fn with_decorations(mut self, focus: KdeColor, hover: KdeColor) -> SetColors {
        self.decoration_focus = focus;
        self.decoration_hover = hover;
        self
    }
}

/// The intensity part of a state effect (`IntensityEffect`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntensityEffect {
    /// 0, or any value without a meaning.
    None,
    /// 1.
    Shade,
    /// 2.
    Darken,
    /// 3.
    Lighten,
}

impl IntensityEffect {
    /// The effect a stored integer selects; an unknown value is `None`.
    pub const fn from_code(code: i64) -> IntensityEffect {
        match code {
            1 => IntensityEffect::Shade,
            2 => IntensityEffect::Darken,
            3 => IntensityEffect::Lighten,
            _ => IntensityEffect::None,
        }
    }

    /// The integer stored for the effect.
    pub const fn code(self) -> i64 {
        match self {
            IntensityEffect::None => 0,
            IntensityEffect::Shade => 1,
            IntensityEffect::Darken => 2,
            IntensityEffect::Lighten => 3,
        }
    }
}

/// The colour part of a state effect (`ColorEffect`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorEffect {
    /// 0, or any value without a meaning.
    None,
    /// 1.
    Desaturate,
    /// 2.
    Fade,
    /// 3.
    Tint,
}

impl ColorEffect {
    /// The effect a stored integer selects; an unknown value is `None`.
    pub const fn from_code(code: i64) -> ColorEffect {
        match code {
            1 => ColorEffect::Desaturate,
            2 => ColorEffect::Fade,
            3 => ColorEffect::Tint,
            _ => ColorEffect::None,
        }
    }

    /// The integer stored for the effect.
    pub const fn code(self) -> i64 {
        match self {
            ColorEffect::None => 0,
            ColorEffect::Desaturate => 1,
            ColorEffect::Fade => 2,
            ColorEffect::Tint => 3,
        }
    }
}

/// The contrast part of a state effect (`ContrastEffect`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContrastEffect {
    /// 0, or any value without a meaning.
    None,
    /// 1.
    Fade,
    /// 2.
    Tint,
}

impl ContrastEffect {
    /// The effect a stored integer selects; an unknown value is `None`.
    pub const fn from_code(code: i64) -> ContrastEffect {
        match code {
            1 => ContrastEffect::Fade,
            2 => ContrastEffect::Tint,
            _ => ContrastEffect::None,
        }
    }

    /// The integer stored for the effect.
    pub const fn code(self) -> i64 {
        match self {
            ContrastEffect::None => 0,
            ContrastEffect::Fade => 1,
            ContrastEffect::Tint => 2,
        }
    }
}

/// The settings of one state's effects, `[ColorEffects:Disabled]` or
/// `[ColorEffects:Inactive]`.
///
/// When `enabled` is false the other fields are kept as read but have no
/// effect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StateEffect {
    /// `Enable`.
    pub enabled: bool,
    /// `IntensityEffect`.
    pub intensity_effect: IntensityEffect,
    /// `IntensityAmount`.
    pub intensity_amount: f64,
    /// `ColorEffect`.
    pub color_effect: ColorEffect,
    /// `ColorAmount`.
    pub color_amount: f64,
    /// `Color`, the colour the colour effect uses. KDE reads it only when
    /// a colour effect is set, so with [`ColorEffect::None`] it is the
    /// state's default colour.
    pub color: KdeColor,
    /// `ContrastEffect`.
    pub contrast_effect: ContrastEffect,
    /// `ContrastAmount`.
    pub contrast_amount: f64,
}

impl StateEffect {
    /// The Disabled state's settings when its group is absent: enabled,
    /// darken by 0.1, no colour effect, contrast fade 0.65, colour
    /// 56,56,56.
    pub const DISABLED_DEFAULT: StateEffect = StateEffect {
        enabled: true,
        intensity_effect: IntensityEffect::Darken,
        intensity_amount: 0.1,
        color_effect: ColorEffect::None,
        color_amount: 0.0,
        color: KdeColor::rgb8(56, 56, 56),
        contrast_effect: ContrastEffect::Fade,
        contrast_amount: 0.65,
    };

    /// The Inactive state's settings when its group is absent: disabled,
    /// no intensity effect, desaturate by -0.9, contrast tint 0.25, colour
    /// 112,111,110.
    pub const INACTIVE_DEFAULT: StateEffect = StateEffect {
        enabled: false,
        intensity_effect: IntensityEffect::None,
        intensity_amount: 0.0,
        color_effect: ColorEffect::Desaturate,
        color_amount: -0.9,
        color: KdeColor::rgb8(112, 111, 110),
        contrast_effect: ContrastEffect::Tint,
        contrast_amount: 0.25,
    };

    fn read(config: &KConfig, group: &str, default: StateEffect) -> StateEffect {
        let g = &[group];
        let color_effect =
            ColorEffect::from_code(config.read_int(g, "ColorEffect", default.color_effect.code()));
        let color = if color_effect == ColorEffect::None {
            default.color
        } else {
            config.read_color(g, "Color").unwrap_or(default.color)
        };
        StateEffect {
            enabled: config.read_bool(g, "Enable").unwrap_or(default.enabled),
            intensity_effect: IntensityEffect::from_code(config.read_int(
                g,
                "IntensityEffect",
                default.intensity_effect.code(),
            )),
            intensity_amount: config.read_real(g, "IntensityAmount", default.intensity_amount),
            color_effect,
            color_amount: config.read_real(g, "ColorAmount", default.color_amount),
            color,
            contrast_effect: ContrastEffect::from_code(config.read_int(
                g,
                "ContrastEffect",
                default.contrast_effect.code(),
            )),
            contrast_amount: config.read_real(g, "ContrastAmount", default.contrast_amount),
        }
    }

    fn write(&self, out: &mut String) {
        push_entry(out, "Color", &self.color.to_kconfig_text());
        push_entry(out, "ColorAmount", &self.color_amount.to_string());
        push_entry(out, "ColorEffect", &self.color_effect.code().to_string());
        push_entry(out, "ContrastAmount", &self.contrast_amount.to_string());
        push_entry(
            out,
            "ContrastEffect",
            &self.contrast_effect.code().to_string(),
        );
        push_entry(out, "Enable", bool_text(self.enabled));
        push_entry(out, "IntensityAmount", &self.intensity_amount.to_string());
        push_entry(
            out,
            "IntensityEffect",
            &self.intensity_effect.code().to_string(),
        );
    }
}

/// The title-bar colours of `[WM]`. Each is `None` when the scheme does not
/// set it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct WmColors {
    /// `activeBackground`.
    pub active_background: Option<KdeColor>,
    /// `activeForeground`.
    pub active_foreground: Option<KdeColor>,
    /// `inactiveBackground`.
    pub inactive_background: Option<KdeColor>,
    /// `inactiveForeground`.
    pub inactive_foreground: Option<KdeColor>,
    /// `activeBlend`.
    pub active_blend: Option<KdeColor>,
    /// `inactiveBlend`.
    pub inactive_blend: Option<KdeColor>,
}

impl WmColors {
    /// The six keys with a reference to their field, in key order.
    fn fields(&self) -> [(&'static str, Option<KdeColor>); 6] {
        [
            ("activeBackground", self.active_background),
            ("activeBlend", self.active_blend),
            ("activeForeground", self.active_foreground),
            ("inactiveBackground", self.inactive_background),
            ("inactiveBlend", self.inactive_blend),
            ("inactiveForeground", self.inactive_foreground),
        ]
    }

    fn read(config: &KConfig) -> WmColors {
        let c = |key| config.read_color(&["WM"], key);
        WmColors {
            active_background: c("activeBackground"),
            active_foreground: c("activeForeground"),
            inactive_background: c("inactiveBackground"),
            inactive_foreground: c("inactiveForeground"),
            active_blend: c("activeBlend"),
            inactive_blend: c("inactiveBlend"),
        }
    }
}

/// The default `[KDE] contrast`.
pub const DEFAULT_CONTRAST: i32 = 7;

/// The default `[KDE] frameContrast`.
pub const DEFAULT_FRAME_CONTRAST: f64 = 0.2;

/// The default value of `[General] TintFactor` when the key is present but
/// does not parse.
pub const DEFAULT_TINT_FACTOR: f64 = 0.15;

/// A complete KDE colour scheme.
///
/// Every field holds a value: keys a configuration lacks take KDE's
/// compiled defaults (see the module documentation).
///
/// A scheme read with [`KdeColorScheme::from_config`] also keeps the
/// configuration it was read from ([`KdeColorScheme::source`]), for an
/// operation that must reproduce what the file said rather than what it
/// means. Equality compares the scheme's fields only, never that source.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct KdeColorScheme {
    /// `[General] Name`, the display name (localised variants are
    /// ignored).
    pub name: Option<String>,
    /// `[General] ColorScheme`, the scheme's file name without extension.
    pub scheme_id: Option<String>,
    /// The seven sets, indexed by [`ColorSet::index`].
    pub sets: [SetColors; 7],
    /// The `[Inactive]` subgroup of each set when it has one, indexed by
    /// [`ColorSet::index`]. It replaces the set for inactive windows.
    pub inactive_sets: [Option<SetColors>; 7],
    /// Whether the configuration has a `[Colors:Header]` group (or a group
    /// nested in it). Without one, the Header set is the Window group's
    /// keys, each falling back to the Header table where Window lacks it.
    pub header_group_present: bool,
    /// `[ColorEffects:Disabled]`.
    pub disabled_effect: StateEffect,
    /// `[ColorEffects:Inactive]`.
    pub inactive_effect: StateEffect,
    /// `[ColorEffects:Inactive] ChangeSelectionColor`: whether the
    /// selection of an inactive window is drawn from the Window set tinted
    /// toward the selection colour. Defaults to the Inactive group's
    /// `Enable`, itself read with a default of true here.
    pub change_selection_color: bool,
    /// `[WM]`.
    pub wm: WmColors,
    /// `[KDE] contrast`, not clamped.
    pub contrast: i32,
    /// `[KDE] frameContrast`, clamped to `0..=1`.
    pub frame_contrast: f64,
    /// `[General] AccentColor`, when present and a colour.
    pub accent: Option<KdeColor>,
    /// `[General] TintFactor`: `Some` when the key is present (0.15 when it
    /// does not parse), which turns accent tinting on.
    pub tint_factor: Option<f64>,
    /// `[General] TitlebarIsAccentColored`, falling back to the legacy
    /// `accentActiveTitlebar`, then false.
    pub titlebar_is_accent_colored: bool,
    source: SchemeSource,
}

/// The configuration a scheme was read from. It takes no part in equality:
/// two schemes with the same fields are equal whatever text they came from.
#[derive(Clone, Default)]
struct SchemeSource(Option<Arc<KConfig>>);

impl PartialEq for SchemeSource {
    fn eq(&self, _: &SchemeSource) -> bool {
        true
    }
}

impl fmt::Debug for SchemeSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Some(config) => write!(f, "SchemeSource({} groups)", config.groups().count()),
            None => f.write_str("SchemeSource(none)"),
        }
    }
}

const FOCUS: KdeColor = KdeColor::rgb8(61, 174, 233);

/// The compiled default tables, in [`ColorSet::ALL`] order. The last one is
/// the Header table, used only where `[Colors:Window]` lacks the key.
const DEFAULT_SETS: [SetColors; 7] = [
    // View
    SetColors::from_rgb8([
        [255, 255, 255],
        [247, 247, 247],
        [35, 38, 41],
        [112, 125, 138],
        [61, 174, 233],
        [41, 128, 185],
        [155, 89, 182],
        [218, 68, 83],
        [246, 116, 0],
        [39, 174, 96],
        [61, 174, 233],
        [147, 206, 233],
    ]),
    // Window
    SetColors::from_rgb8([
        [239, 240, 241],
        [227, 229, 231],
        [35, 38, 41],
        [112, 125, 138],
        [61, 174, 233],
        [41, 128, 185],
        [155, 89, 182],
        [218, 68, 83],
        [246, 116, 0],
        [39, 174, 96],
        [61, 174, 233],
        [147, 206, 233],
    ]),
    // Button
    SetColors::from_rgb8([
        [252, 252, 252],
        [163, 212, 250],
        [35, 38, 41],
        [112, 125, 138],
        [61, 174, 233],
        [41, 128, 185],
        [155, 89, 182],
        [218, 68, 83],
        [246, 116, 0],
        [39, 174, 96],
        [61, 174, 233],
        [147, 206, 233],
    ]),
    // Selection
    SetColors::from_rgb8([
        [61, 174, 233],
        [163, 212, 250],
        [255, 255, 255],
        [112, 125, 138],
        [255, 255, 255],
        [253, 188, 75],
        [155, 89, 182],
        [176, 55, 69],
        [198, 92, 0],
        [23, 104, 57],
        [61, 174, 233],
        [147, 206, 233],
    ]),
    // Tooltip
    SetColors::from_rgb8([
        [247, 247, 247],
        [239, 240, 241],
        [35, 38, 41],
        [112, 125, 138],
        [61, 174, 233],
        [41, 128, 185],
        [155, 89, 182],
        [218, 68, 83],
        [246, 116, 0],
        [39, 174, 96],
        [61, 174, 233],
        [147, 206, 233],
    ]),
    // Complementary
    SetColors::from_rgb8([
        [42, 46, 50],
        [27, 30, 32],
        [252, 252, 252],
        [161, 169, 177],
        [61, 174, 233],
        [29, 153, 243],
        [155, 89, 182],
        [218, 68, 83],
        [246, 116, 0],
        [39, 174, 96],
        [61, 174, 233],
        [147, 206, 233],
    ]),
    // Header
    SetColors::from_rgb8([
        [222, 224, 226],
        [239, 240, 241],
        [35, 38, 41],
        [112, 125, 138],
        [61, 174, 233],
        [41, 128, 185],
        [155, 89, 182],
        [218, 68, 83],
        [246, 116, 0],
        [39, 174, 96],
        [61, 174, 233],
        [147, 206, 233],
    ]),
];

/// Breeze Light's sets: the compiled defaults with every hover decoration
/// set to the focus colour.
const BREEZE_LIGHT_SETS: [SetColors; 7] = [
    DEFAULT_SETS[0].with_decorations(FOCUS, FOCUS),
    DEFAULT_SETS[1].with_decorations(FOCUS, FOCUS),
    DEFAULT_SETS[2].with_decorations(FOCUS, FOCUS),
    DEFAULT_SETS[3].with_decorations(FOCUS, FOCUS),
    DEFAULT_SETS[4].with_decorations(FOCUS, FOCUS),
    DEFAULT_SETS[5].with_decorations(FOCUS, FOCUS),
    DEFAULT_SETS[6].with_decorations(FOCUS, FOCUS),
];

/// Breeze Light's `[Colors:Header][Inactive]`.
const BREEZE_LIGHT_HEADER_INACTIVE: SetColors = SetColors::from_rgb8([
    [239, 240, 241],
    [227, 229, 231],
    [35, 38, 41],
    [112, 125, 138],
    [61, 174, 233],
    [41, 128, 185],
    [155, 89, 182],
    [218, 68, 83],
    [246, 116, 0],
    [39, 174, 96],
    [61, 174, 233],
    [61, 174, 233],
]);

/// A Breeze Dark set from its two backgrounds; every Breeze Dark set except
/// Selection shares the same foregrounds and decorations.
const fn dark_set(normal: [u8; 3], alternate: [u8; 3]) -> SetColors {
    SetColors::from_rgb8([
        normal,
        alternate,
        [252, 252, 252],
        [161, 169, 177],
        [61, 174, 233],
        [29, 153, 243],
        [155, 89, 182],
        [218, 68, 83],
        [246, 116, 0],
        [39, 174, 96],
        [61, 174, 233],
        [61, 174, 233],
    ])
}

const BREEZE_DARK_SETS: [SetColors; 7] = [
    dark_set([20, 22, 24], [29, 31, 34]),
    dark_set([32, 35, 38], [41, 44, 48]),
    dark_set([41, 44, 48], [30, 87, 116]),
    SetColors::from_rgb8([
        [61, 174, 233],
        [30, 87, 116],
        [252, 252, 252],
        [161, 169, 177],
        [252, 252, 252],
        [253, 188, 75],
        [155, 89, 182],
        [176, 55, 69],
        [198, 92, 0],
        [23, 104, 57],
        [61, 174, 233],
        [61, 174, 233],
    ]),
    dark_set([41, 44, 48], [32, 35, 38]),
    dark_set([32, 35, 38], [30, 87, 116]),
    dark_set([41, 44, 48], [32, 35, 38]),
];

const BREEZE_DARK_HEADER_INACTIVE: SetColors = dark_set([32, 35, 38], [41, 44, 48]);

/// Breeze Classic's Window set, which its Header set follows key by key.
const BREEZE_CLASSIC_WINDOW: SetColors = SetColors::from_rgb8([
    [239, 240, 241],
    [189, 195, 199],
    [35, 38, 39],
    [127, 140, 141],
    [61, 174, 233],
    [41, 128, 185],
    [127, 140, 141],
    [218, 68, 83],
    [246, 116, 0],
    [39, 174, 96],
    [61, 174, 233],
    [147, 206, 233],
]);

const BREEZE_CLASSIC_SETS: [SetColors; 7] = [
    SetColors::from_rgb8([
        [252, 252, 252],
        [239, 240, 241],
        [35, 38, 39],
        [127, 140, 141],
        [61, 174, 233],
        [41, 128, 185],
        [127, 140, 141],
        [218, 68, 83],
        [246, 116, 0],
        [39, 174, 96],
        [61, 174, 233],
        [147, 206, 233],
    ]),
    BREEZE_CLASSIC_WINDOW,
    SetColors::from_rgb8([
        [239, 240, 241],
        [189, 195, 199],
        [35, 38, 39],
        [127, 140, 141],
        [61, 174, 233],
        [41, 128, 185],
        [127, 140, 141],
        [218, 68, 83],
        [198, 92, 0],
        [39, 174, 96],
        [61, 174, 233],
        [147, 206, 233],
    ]),
    SetColors::from_rgb8([
        [61, 174, 233],
        [29, 153, 243],
        [252, 252, 252],
        [239, 240, 241],
        [252, 252, 252],
        [253, 188, 75],
        [189, 195, 199],
        [176, 55, 69],
        [246, 116, 0],
        [23, 104, 57],
        [61, 174, 233],
        [147, 206, 233],
    ]),
    SetColors::from_rgb8([
        [35, 38, 39],
        [77, 77, 77],
        [252, 252, 252],
        [189, 195, 199],
        [61, 174, 233],
        [41, 128, 185],
        [127, 140, 141],
        [218, 68, 83],
        [246, 116, 0],
        [39, 174, 96],
        [61, 174, 233],
        [147, 206, 233],
    ]),
    SetColors::from_rgb8([
        [49, 54, 59],
        [59, 64, 69],
        [239, 240, 241],
        [175, 176, 179],
        [147, 206, 233],
        [61, 174, 230],
        [61, 174, 230],
        [231, 76, 60],
        [253, 188, 75],
        [46, 204, 113],
        [30, 146, 255],
        [61, 174, 230],
    ]),
    BREEZE_CLASSIC_WINDOW,
];

/// The `[ColorEffects:Inactive]` group all three stock Breeze schemes
/// share. Their `[ColorEffects:Disabled]` group equals
/// [`StateEffect::DISABLED_DEFAULT`].
const BREEZE_INACTIVE_EFFECT: StateEffect = StateEffect {
    enabled: false,
    intensity_effect: IntensityEffect::None,
    intensity_amount: 0.0,
    color_effect: ColorEffect::Fade,
    color_amount: 0.025,
    color: KdeColor::rgb8(112, 111, 110),
    contrast_effect: ContrastEffect::Tint,
    contrast_amount: 0.1,
};

const fn wm(colors: [[u8; 3]; 6]) -> WmColors {
    const fn c(rgb: [u8; 3]) -> Option<KdeColor> {
        Some(KdeColor::rgb8(rgb[0], rgb[1], rgb[2]))
    }
    WmColors {
        active_background: c(colors[0]),
        active_blend: c(colors[1]),
        active_foreground: c(colors[2]),
        inactive_background: c(colors[3]),
        inactive_blend: c(colors[4]),
        inactive_foreground: c(colors[5]),
    }
}

impl Default for KdeColorScheme {
    fn default() -> Self {
        KdeColorScheme::kcolorscheme_defaults()
    }
}

impl KdeColorScheme {
    /// The scheme a configuration with no colour keys at all describes:
    /// KDE's compiled defaults. They equal Breeze Light except that every
    /// `DecorationHover` is 147,206,233, there is no `[Inactive]` subgroup,
    /// `contrast` is 7 and there is no `[WM]` colour.
    pub fn kcolorscheme_defaults() -> KdeColorScheme {
        KdeColorScheme {
            name: None,
            scheme_id: None,
            sets: DEFAULT_SETS,
            inactive_sets: [None; 7],
            header_group_present: false,
            disabled_effect: StateEffect::DISABLED_DEFAULT,
            inactive_effect: StateEffect::INACTIVE_DEFAULT,
            change_selection_color: true,
            wm: WmColors::default(),
            contrast: DEFAULT_CONTRAST,
            frame_contrast: DEFAULT_FRAME_CONTRAST,
            accent: None,
            tint_factor: None,
            titlebar_is_accent_colored: false,
            source: SchemeSource::default(),
        }
    }

    /// The stock Breeze Light scheme (`BreezeLight.colors`, breeze 6.8.0).
    pub fn breeze_light() -> KdeColorScheme {
        let mut inactive_sets = [None; 7];
        inactive_sets[ColorSet::Header.index()] = Some(BREEZE_LIGHT_HEADER_INACTIVE);
        KdeColorScheme {
            name: Some("Breeze Light".to_owned()),
            scheme_id: Some("BreezeLight".to_owned()),
            sets: BREEZE_LIGHT_SETS,
            inactive_sets,
            header_group_present: true,
            inactive_effect: BREEZE_INACTIVE_EFFECT,
            wm: wm([
                [227, 229, 231],
                [227, 229, 231],
                [35, 38, 41],
                [239, 240, 241],
                [239, 240, 241],
                [112, 125, 138],
            ]),
            contrast: 4,
            ..KdeColorScheme::kcolorscheme_defaults()
        }
    }

    /// The stock Breeze Dark scheme (`BreezeDark.colors`, breeze 6.8.0).
    pub fn breeze_dark() -> KdeColorScheme {
        let mut inactive_sets = [None; 7];
        inactive_sets[ColorSet::Header.index()] = Some(BREEZE_DARK_HEADER_INACTIVE);
        KdeColorScheme {
            name: Some("Breeze Dark".to_owned()),
            scheme_id: Some("BreezeDark".to_owned()),
            sets: BREEZE_DARK_SETS,
            inactive_sets,
            header_group_present: true,
            inactive_effect: BREEZE_INACTIVE_EFFECT,
            wm: wm([
                [39, 44, 49],
                [252, 252, 252],
                [252, 252, 252],
                [32, 36, 40],
                [161, 169, 177],
                [161, 169, 177],
            ]),
            contrast: 4,
            ..KdeColorScheme::kcolorscheme_defaults()
        }
    }

    /// The stock Breeze Classic scheme (`BreezeClassic.colors`, breeze
    /// 6.8.0). It has no Header group, so its Header set is its Window set,
    /// and it colours the active title bar with the accent.
    pub fn breeze_classic() -> KdeColorScheme {
        KdeColorScheme {
            name: Some("Breeze Classic".to_owned()),
            scheme_id: Some("BreezeClassic".to_owned()),
            sets: BREEZE_CLASSIC_SETS,
            inactive_effect: BREEZE_INACTIVE_EFFECT,
            wm: wm([
                [71, 80, 87],
                [252, 252, 252],
                [252, 252, 252],
                [239, 240, 241],
                [75, 71, 67],
                [189, 195, 199],
            ]),
            contrast: 4,
            titlebar_is_accent_colored: true,
            ..KdeColorScheme::kcolorscheme_defaults()
        }
    }

    /// The stored colours of `set`.
    pub fn set(&self, set: ColorSet) -> &SetColors {
        &self.sets[set.index()]
    }

    /// The `[Inactive]` replacement of `set`, when the scheme has one.
    pub fn inactive_set(&self, set: ColorSet) -> Option<&SetColors> {
        self.inactive_sets[set.index()].as_ref()
    }

    /// Parses a `.colors` file (or any configuration text) and reads the
    /// scheme from it.
    pub fn parse(text: &str) -> (KdeColorScheme, Vec<ParseDiagnostic>) {
        let (config, diagnostics) = KConfig::parse(text);
        (KdeColorScheme::from_config(&config), diagnostics)
    }

    /// Reads the scheme a configuration describes, a single file or a
    /// merged cascade. Names are read as stored, without `[$e]` expansion.
    pub fn from_config(config: &KConfig) -> KdeColorScheme {
        let header_defaults = {
            let mut defaults = DEFAULT_SETS[ColorSet::Header.index()];
            for role in ColorRole::ALL {
                if let Some(c) = config.read_color(&[ColorSet::Window.group_name()], role.key()) {
                    defaults.set(role, c);
                }
            }
            defaults
        };

        let mut sets = DEFAULT_SETS;
        let mut inactive_sets = [None; 7];
        for set in ColorSet::ALL {
            let defaults = if set == ColorSet::Header {
                header_defaults
            } else {
                DEFAULT_SETS[set.index()]
            };
            let group = set.group_name();
            sets[set.index()] = read_set(config, &[group], defaults);
            if config.group_exists(&[group, "Inactive"]) {
                inactive_sets[set.index()] = Some(read_set(config, &[group, "Inactive"], defaults));
            }
        }

        let inactive = "ColorEffects:Inactive";
        let change_selection_color = config
            .read_bool(&[inactive], "ChangeSelectionColor")
            .unwrap_or_else(|| config.read_bool(&[inactive], "Enable").unwrap_or(true));

        let general = &["General"];
        let titlebar_is_accent_colored = config
            .read_bool(general, "TitlebarIsAccentColored")
            .or_else(|| config.read_bool(general, "accentActiveTitlebar"))
            .unwrap_or(false);
        let tint_factor = config
            .has_key(general, "TintFactor")
            .then(|| config.read_real(general, "TintFactor", DEFAULT_TINT_FACTOR));

        let contrast = config.read_int(&["KDE"], "contrast", i64::from(DEFAULT_CONTRAST));

        KdeColorScheme {
            name: config.get(general, "Name").map(str::to_owned),
            scheme_id: config.get(general, "ColorScheme").map(str::to_owned),
            sets,
            inactive_sets,
            header_group_present: config.group_exists(&[ColorSet::Header.group_name()]),
            disabled_effect: StateEffect::read(
                config,
                "ColorEffects:Disabled",
                StateEffect::DISABLED_DEFAULT,
            ),
            inactive_effect: StateEffect::read(config, inactive, StateEffect::INACTIVE_DEFAULT),
            change_selection_color,
            wm: WmColors::read(config),
            contrast: i32::try_from(contrast).unwrap_or(DEFAULT_CONTRAST),
            frame_contrast: config
                .read_real(&["KDE"], "frameContrast", DEFAULT_FRAME_CONTRAST)
                .clamp(0.0, 1.0),
            accent: config.read_color(general, "AccentColor"),
            tint_factor,
            titlebar_is_accent_colored,
            source: SchemeSource(Some(Arc::new(config.clone()))),
        }
    }

    /// The configuration the scheme was read from, `None` for a built-in
    /// scheme. It holds what the fields do not: which keys were present,
    /// each value as written, `frameContrast` before clamping.
    pub fn source(&self) -> Option<&KConfig> {
        self.source.0.as_deref()
    }

    /// The scheme as configuration text that [`KdeColorScheme::parse`]
    /// reads back to an equal scheme.
    ///
    /// Groups and keys are written in sorted order, colours as KDE writes
    /// them, `r,g,b` (`r,g,b,a` when translucent) in 8 bits, so a colour
    /// read from a 9- or 12-digit `#` form comes back rounded to 8 bits. `[KDE] contrast` and `frameContrast`
    /// are written only when they differ from their defaults, so a key the
    /// scheme leaves at its default still falls back to `kdeglobals` when
    /// the text is cascaded over it, as a stock `.colors` file does.
    ///
    /// A scheme without a Header group is written without one. Its Header
    /// colours follow the Window group key by key, falling back to the
    /// Header table where Window lacks a key; to keep that, a Window key
    /// whose Header colour differs from the Window colour is left out, so it
    /// reads back as absent from both.
    pub fn to_kconfig_text(&self) -> String {
        let mut out = String::new();

        push_group(&mut out, &["ColorEffects:Disabled"]);
        self.disabled_effect.write(&mut out);
        push_group(&mut out, &["ColorEffects:Inactive"]);
        push_entry(
            &mut out,
            "ChangeSelectionColor",
            bool_text(self.change_selection_color),
        );
        self.inactive_effect.write(&mut out);

        let mut ordered = ColorSet::ALL;
        ordered.sort_by_key(|s| s.group_name());
        for set in ordered {
            if set == ColorSet::Header && !self.header_group_present {
                continue;
            }
            let colors = self.set(set);
            push_group(&mut out, &[set.group_name()]);
            for (key, role) in sorted_roles() {
                let follows_window_default = set == ColorSet::Window
                    && !self.header_group_present
                    && self.set(ColorSet::Header).get(role) != colors.get(role);
                if !follows_window_default {
                    push_entry(&mut out, key, &colors.get(role).to_kconfig_text());
                }
            }
            if let Some(inactive) = self.inactive_set(set) {
                push_group(&mut out, &[set.group_name(), "Inactive"]);
                for (key, role) in sorted_roles() {
                    push_entry(&mut out, key, &inactive.get(role).to_kconfig_text());
                }
            }
        }

        let general: Vec<(&str, String)> = [
            ("AccentColor", self.accent.map(KdeColor::to_kconfig_text)),
            ("ColorScheme", self.scheme_id.as_deref().map(escape_value)),
            ("Name", self.name.as_deref().map(escape_value)),
            ("TintFactor", self.tint_factor.map(|f| f.to_string())),
            (
                "TitlebarIsAccentColored",
                self.titlebar_is_accent_colored.then(|| "true".to_owned()),
            ),
        ]
        .into_iter()
        .filter_map(|(k, v)| v.map(|v| (k, v)))
        .collect();
        if !general.is_empty() {
            push_group(&mut out, &["General"]);
            for (key, value) in &general {
                push_entry(&mut out, key, value);
            }
        }

        let contrast = (self.contrast != DEFAULT_CONTRAST).then(|| self.contrast.to_string());
        let frame = (self.frame_contrast != DEFAULT_FRAME_CONTRAST)
            .then(|| self.frame_contrast.to_string());
        if contrast.is_some() || frame.is_some() {
            push_group(&mut out, &["KDE"]);
            if let Some(contrast) = contrast {
                push_entry(&mut out, "contrast", &contrast);
            }
            if let Some(frame) = frame {
                push_entry(&mut out, "frameContrast", &frame);
            }
        }

        let wm: Vec<_> = self
            .wm
            .fields()
            .into_iter()
            .filter_map(|(k, v)| v.map(|v| (k, v)))
            .collect();
        if !wm.is_empty() {
            push_group(&mut out, &["WM"]);
            for (key, color) in wm {
                push_entry(&mut out, key, &color.to_kconfig_text());
            }
        }
        out
    }
}

fn read_set(config: &KConfig, group: &[&str], defaults: SetColors) -> SetColors {
    let mut colors = defaults;
    for role in ColorRole::ALL {
        if let Some(c) = config.read_color(group, role.key()) {
            colors.set(role, c);
        }
    }
    colors
}

fn sorted_roles() -> impl Iterator<Item = (&'static str, ColorRole)> {
    let mut roles: Vec<_> = ColorRole::ALL.iter().map(|r| (r.key(), *r)).collect();
    roles.sort_by_key(|(k, _)| *k);
    roles.into_iter()
}

fn push_group(out: &mut String, path: &[&str]) {
    if !out.is_empty() {
        out.push('\n');
    }
    for segment in path {
        out.push('[');
        out.push_str(segment);
        out.push(']');
    }
    out.push('\n');
}

fn push_entry(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push('=');
    out.push_str(value);
    out.push('\n');
}

fn bool_text(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}
