// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The desktop settings a KDE session gives an application, read from
//! configuration layers the caller has already loaded.
//!
//! [`KdeDesktop::from_layers`] is pure: the caller finds the files with
//! [`KdeConfigLocations`](crate::KdeConfigLocations), parses the ones that
//! exist, and hands the layers over.

use crate::color::KdeColor;
use crate::config::{ExpansionEnv, KConfig, LayerKind};
use crate::fonts::{KdeFont, STYLE_HINT_MONOSPACE};
use crate::scheme::KdeColorScheme;

/// Where a desktop's colour scheme came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SchemeOrigin {
    /// The colour groups of `kdeglobals` itself, which has a
    /// `[Colors:View]` group. Any accent the user chose is already in them.
    Kdeglobals,
    /// A scheme file read because `kdeglobals` has no `[Colors:View]`
    /// group: the look-and-feel package's colours or the named scheme.
    SchemeFile,
    /// Neither: `kdeglobals` has no colours and no scheme file was found,
    /// so the built-in Breeze Light scheme was used.
    BuiltIn,
}

/// The five fonts a KDE desktop configures.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct KdeFonts {
    /// `[General] font`, default Noto Sans 10 pt.
    pub general: KdeFont,
    /// `[General] fixed`, default Hack 10 pt with the monospace hint.
    pub fixed: KdeFont,
    /// `[General] smallestReadableFont`, default Noto Sans 8 pt.
    pub small: KdeFont,
    /// `[General] menuFont`, default Noto Sans 10 pt.
    pub menu: KdeFont,
    /// `[General] toolBarFont`, default Noto Sans 10 pt.
    pub toolbar: KdeFont,
}

impl Default for KdeFonts {
    fn default() -> Self {
        let regular = |family: &str, points: f64| {
            let mut font = KdeFont::new(family, points);
            font.style_name = Some("Regular".to_owned());
            font
        };
        let mut fixed = regular("Hack", 10.0);
        fixed.style_hint = STYLE_HINT_MONOSPACE;
        KdeFonts {
            general: regular("Noto Sans", 10.0),
            fixed,
            small: regular("Noto Sans", 8.0),
            menu: regular("Noto Sans", 10.0),
            toolbar: regular("Noto Sans", 10.0),
        }
    }
}

impl KdeFonts {
    fn read(config: &KConfig, env: &ExpansionEnv<'_>) -> KdeFonts {
        let defaults = KdeFonts::default();
        let font = |key: &str, default: KdeFont| {
            config
                .read_string(&["General"], key, env)
                .filter(|v| !v.is_empty())
                .and_then(|v| KdeFont::parse_qfont(&v))
                .unwrap_or(default)
        };
        KdeFonts {
            general: font("font", defaults.general),
            fixed: font("fixed", defaults.fixed),
            small: font("smallestReadableFont", defaults.small),
            menu: font("menuFont", defaults.menu),
            toolbar: font("toolBarFont", defaults.toolbar),
        }
    }
}

/// How large a Breeze window shadow is (`[Common] ShadowSize`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ShadowSize {
    /// `ShadowNone`, 0.
    None,
    /// `ShadowSmall`, 1.
    Small,
    /// `ShadowMedium`, 2.
    Medium,
    /// `ShadowLarge`, 3, the default.
    #[default]
    Large,
    /// `ShadowVeryLarge`, 4.
    VeryLarge,
}

/// When Breeze underlines mnemonics (`[Style] MnemonicsMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MnemonicsMode {
    /// `MN_NEVER`, 0.
    Never,
    /// `MN_AUTO`, 1, the default: while Alt is held.
    #[default]
    Auto,
    /// `MN_ALWAYS`, 2.
    Always,
}

/// How many arrow buttons a Breeze scroll bar has at one end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ScrollBarButtons {
    /// 0, the default.
    #[default]
    None,
    /// 1.
    Single,
    /// Any other value.
    Double,
}

impl ScrollBarButtons {
    fn from_code(code: i64) -> ScrollBarButtons {
        match code {
            0 => ScrollBarButtons::None,
            1 => ScrollBarButtons::Single,
            _ => ScrollBarButtons::Double,
        }
    }
}

/// The Breeze widget-style settings of `breezerc` that change how widgets
/// look. Keys that only concern the window decoration, window dragging,
/// splitter proxies or debugging are not read.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct BreezeSettings {
    /// `[Common] ShadowStrength`, clamped to 25..=255 (the strength is the
    /// value divided by 255). Default 255.
    pub shadow_strength: u8,
    /// `[Common] ShadowSize`. Default large.
    pub shadow_size: ShadowSize,
    /// `[Common] ShadowColor`. Default black.
    pub shadow_color: KdeColor,
    /// `[Style] AnimationsEnabled`. Default true.
    pub animations_enabled: bool,
    /// `[Style] AnimationSteps`: animation progress is quantised to
    /// multiples of one over this many steps. Default 10.
    pub animation_steps: i64,
    /// `[Style] AnimationsDuration` in milliseconds. Default 100. A
    /// `kdeglobals [KDE] AnimationDurationFactor` overrides it.
    pub animations_duration_ms: i64,
    /// `[Style] StackedWidgetTransitionsEnabled`. Default false.
    pub stacked_widget_transitions_enabled: bool,
    /// `[Style] ProgressBarAnimated`: the busy animation. Default true.
    pub progress_bar_animated: bool,
    /// `[Style] ProgressBarBusyStepDuration` in milliseconds. Default 800.
    pub progress_bar_busy_step_duration_ms: i64,
    /// `[Style] ScrollBarAddLineButtons`. Default none.
    pub scroll_bar_add_line_buttons: ScrollBarButtons,
    /// `[Style] ScrollBarSubLineButtons`. Default none.
    pub scroll_bar_sub_line_buttons: ScrollBarButtons,
    /// `[Style] MnemonicsMode`. Default automatic.
    pub mnemonics_mode: MnemonicsMode,
    /// `[Style] ToolBarDrawItemSeparator`. Default true.
    pub tool_bar_draw_item_separator: bool,
    /// `[Style] ViewDrawFocusIndicator`: the item-view focus rectangle.
    /// Default true.
    pub view_draw_focus_indicator: bool,
    /// `[Style] SliderDrawTickMarks`. Default true.
    pub slider_draw_tick_marks: bool,
    /// `[Style] ViewDrawTreeBranchLines`. Default true.
    pub view_draw_tree_branch_lines: bool,
    /// `[Style] ViewInvertSortIndicator`. Default true.
    pub view_invert_sort_indicator: bool,
    /// `[Style] TabBarDrawCenteredTabs`. Default false.
    pub tab_bar_draw_centered_tabs: bool,
    /// `[Style] DockWidgetDrawFrame`. Default false.
    pub dock_widget_draw_frame: bool,
    /// `[Style] SidePanelDrawFrame`. Default false.
    pub side_panel_draw_frame: bool,
    /// `[Style] MenuItemDrawStrongFocus`. Default true.
    pub menu_item_draw_strong_focus: bool,
    /// `[Style] MenuOpacity` in percent, clamped to 0..=100. Default 100.
    pub menu_opacity: u8,
}

impl Default for BreezeSettings {
    fn default() -> Self {
        BreezeSettings {
            shadow_strength: 255,
            shadow_size: ShadowSize::Large,
            shadow_color: KdeColor::BLACK,
            animations_enabled: true,
            animation_steps: 10,
            animations_duration_ms: 100,
            stacked_widget_transitions_enabled: false,
            progress_bar_animated: true,
            progress_bar_busy_step_duration_ms: 800,
            scroll_bar_add_line_buttons: ScrollBarButtons::None,
            scroll_bar_sub_line_buttons: ScrollBarButtons::None,
            mnemonics_mode: MnemonicsMode::Auto,
            tool_bar_draw_item_separator: true,
            view_draw_focus_indicator: true,
            slider_draw_tick_marks: true,
            view_draw_tree_branch_lines: true,
            view_invert_sort_indicator: true,
            tab_bar_draw_centered_tabs: false,
            dock_widget_draw_frame: false,
            side_panel_draw_frame: false,
            menu_item_draw_strong_focus: true,
            menu_opacity: 100,
        }
    }
}

impl BreezeSettings {
    /// Reads the settings from a merged `breezerc`.
    pub fn from_config(config: &KConfig) -> BreezeSettings {
        let d = BreezeSettings::default();
        let common = &["Common"];
        let style = &["Style"];
        let flag = |key: &str, default: bool| config.read_bool(style, key).unwrap_or(default);
        let int = |key: &str, default: i64| config.read_int(style, key, default);
        BreezeSettings {
            shadow_strength: config
                .read_int(common, "ShadowStrength", i64::from(d.shadow_strength))
                .clamp(25, 255) as u8,
            shadow_size: config
                .get(common, "ShadowSize")
                .and_then(|v| {
                    enum_value(
                        v,
                        &[
                            "ShadowNone",
                            "ShadowSmall",
                            "ShadowMedium",
                            "ShadowLarge",
                            "ShadowVeryLarge",
                        ],
                    )
                })
                .map_or(d.shadow_size, |i| {
                    [
                        ShadowSize::None,
                        ShadowSize::Small,
                        ShadowSize::Medium,
                        ShadowSize::Large,
                        ShadowSize::VeryLarge,
                    ][i]
                }),
            shadow_color: config
                .read_color(common, "ShadowColor")
                .unwrap_or(d.shadow_color),
            animations_enabled: flag("AnimationsEnabled", d.animations_enabled),
            animation_steps: int("AnimationSteps", d.animation_steps),
            animations_duration_ms: int("AnimationsDuration", d.animations_duration_ms),
            stacked_widget_transitions_enabled: flag(
                "StackedWidgetTransitionsEnabled",
                d.stacked_widget_transitions_enabled,
            ),
            progress_bar_animated: flag("ProgressBarAnimated", d.progress_bar_animated),
            progress_bar_busy_step_duration_ms: int(
                "ProgressBarBusyStepDuration",
                d.progress_bar_busy_step_duration_ms,
            ),
            scroll_bar_add_line_buttons: ScrollBarButtons::from_code(int(
                "ScrollBarAddLineButtons",
                0,
            )),
            scroll_bar_sub_line_buttons: ScrollBarButtons::from_code(int(
                "ScrollBarSubLineButtons",
                0,
            )),
            mnemonics_mode: config
                .get(style, "MnemonicsMode")
                .and_then(|v| enum_value(v, &["MN_NEVER", "MN_AUTO", "MN_ALWAYS"]))
                .map_or(d.mnemonics_mode, |i| {
                    [
                        MnemonicsMode::Never,
                        MnemonicsMode::Auto,
                        MnemonicsMode::Always,
                    ][i]
                }),
            tool_bar_draw_item_separator: flag(
                "ToolBarDrawItemSeparator",
                d.tool_bar_draw_item_separator,
            ),
            view_draw_focus_indicator: flag("ViewDrawFocusIndicator", d.view_draw_focus_indicator),
            slider_draw_tick_marks: flag("SliderDrawTickMarks", d.slider_draw_tick_marks),
            view_draw_tree_branch_lines: flag(
                "ViewDrawTreeBranchLines",
                d.view_draw_tree_branch_lines,
            ),
            view_invert_sort_indicator: flag(
                "ViewInvertSortIndicator",
                d.view_invert_sort_indicator,
            ),
            tab_bar_draw_centered_tabs: flag(
                "TabBarDrawCenteredTabs",
                d.tab_bar_draw_centered_tabs,
            ),
            dock_widget_draw_frame: flag("DockWidgetDrawFrame", d.dock_widget_draw_frame),
            side_panel_draw_frame: flag("SidePanelDrawFrame", d.side_panel_draw_frame),
            menu_item_draw_strong_focus: flag(
                "MenuItemDrawStrongFocus",
                d.menu_item_draw_strong_focus,
            ),
            menu_opacity: int("MenuOpacity", i64::from(d.menu_opacity)).clamp(0, 100) as u8,
        }
    }
}

/// An enumeration value stored as one of `names` (ignoring ASCII case) or
/// as its index.
fn enum_value(value: &str, names: &[&str]) -> Option<usize> {
    names
        .iter()
        .position(|n| n.eq_ignore_ascii_case(value))
        .or_else(|| value.parse::<usize>().ok().filter(|&i| i < names.len()))
}

/// The configuration layers a [`KdeDesktop`] is built from, each list
/// lowest priority first, as
/// [`KdeConfigLocations`](crate::KdeConfigLocations) orders them.
#[derive(Debug, Clone, Copy, Default)]
pub struct KdeDesktopLayers<'a> {
    /// The `kdeglobals` cascade.
    pub kdeglobals: &'a [KConfig],
    /// The `breezerc` cascade.
    pub breezerc: &'a [KConfig],
    /// The colour-scheme file to read when `kdeglobals` has no
    /// `[Colors:View]` group: the first existing candidate of
    /// [`KdeConfigLocations::scheme_fallback_candidates`](crate::KdeConfigLocations::scheme_fallback_candidates).
    pub scheme_file: Option<&'a KConfig>,
}

/// What a KDE desktop configures for an application.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct KdeDesktop {
    /// The colour scheme in effect.
    pub scheme: KdeColorScheme,
    /// Where the scheme came from.
    pub scheme_origin: SchemeOrigin,
    /// `kdeglobals [General] AccentColor`, the accent the user chose, when
    /// present, a colour, and not fully transparent black.
    pub user_accent: Option<KdeColor>,
    /// The configured fonts.
    pub fonts: KdeFonts,
    /// `kdeglobals [KDE] AnimationDurationFactor`, `None` when absent (a
    /// present value that does not parse reads as 1.0).
    pub animation_duration_factor: Option<f64>,
    /// `kdeglobals [KDE] widgetStyle` as stored, `None` when absent.
    pub widget_style: Option<String>,
    /// The Breeze widget-style settings.
    pub breeze: BreezeSettings,
}

impl KdeDesktop {
    /// A desktop with `scheme` and every other setting at its default, as a
    /// KDE session with empty configuration files gives.
    pub fn new(scheme: KdeColorScheme) -> KdeDesktop {
        KdeDesktop {
            scheme,
            scheme_origin: SchemeOrigin::Kdeglobals,
            user_accent: None,
            fonts: KdeFonts::default(),
            animation_duration_factor: None,
            widget_style: None,
            breeze: BreezeSettings::default(),
        }
    }

    /// Builds the desktop from already-read layers.
    ///
    /// When the merged `kdeglobals` has a `[Colors:View]` group the scheme
    /// is read from it. Otherwise it is read from `layers.scheme_file`, or
    /// from the built-in Breeze Light when there is none, with the
    /// `kdeglobals` cascade below it, so a key the scheme file lacks still
    /// comes from `kdeglobals`. String settings expand `[$e]` entries
    /// through `env`.
    pub fn from_layers(layers: &KdeDesktopLayers<'_>, env: &ExpansionEnv<'_>) -> KdeDesktop {
        let globals = KConfig::cascade(layers.kdeglobals);
        let (scheme, scheme_origin) = if globals.group_exists(&["Colors:View"]) {
            (
                KdeColorScheme::from_config(&globals),
                SchemeOrigin::Kdeglobals,
            )
        } else {
            let (file, origin) = match layers.scheme_file {
                Some(file) => (file.clone(), SchemeOrigin::SchemeFile),
                None => (
                    KConfig::parse(&KdeColorScheme::breeze_light().to_kconfig_text()).0,
                    SchemeOrigin::BuiltIn,
                ),
            };
            let merged = KConfig::cascade(&[globals.clone(), file.with_kind(LayerKind::Default)]);
            (KdeColorScheme::from_config(&merged), origin)
        };

        let kde = &["KDE"];
        let animation_duration_factor = globals
            .has_key(kde, "AnimationDurationFactor")
            .then(|| globals.read_real(kde, "AnimationDurationFactor", 1.0));

        KdeDesktop {
            scheme,
            scheme_origin,
            user_accent: globals
                .read_color(&["General"], "AccentColor")
                .filter(|c| !c.is_transparent_black()),
            fonts: KdeFonts::read(&globals, env),
            animation_duration_factor,
            widget_style: globals.read_string(kde, "widgetStyle", env),
            breeze: BreezeSettings::from_config(&KConfig::cascade(layers.breezerc)),
        }
    }

    /// The animation duration factor, 1.0 when `kdeglobals` does not set
    /// one.
    pub fn animation_factor(&self) -> f64 {
        self.animation_duration_factor.unwrap_or(1.0)
    }

    /// Whether the desktop asks for reduced motion: the animation duration
    /// factor is exactly 0, as the KDE desktop portal reports it.
    pub fn prefers_reduced_motion(&self) -> bool {
        self.animation_duration_factor == Some(0.0)
    }

    /// Whether the widget style is Breeze: `widgetStyle` absent, empty, or
    /// `breeze` ignoring ASCII case.
    pub fn uses_breeze_style(&self) -> bool {
        self.widget_style
            .as_deref()
            .is_none_or(|s| s.is_empty() || s.eq_ignore_ascii_case("breeze"))
    }
}
