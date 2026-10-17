// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Breeze widget-style settings a desktop configures, carried on a
//! theme as an extension ([`BreezeStyleConfig`]).

use std::time::Duration;

use teksilo_kde_scheme::{BreezeSettings, KdeDesktop};

/// The Breeze widget-style settings of a desktop: `breezerc` and the
/// animation speed.
///
/// A [`teksilo_core::styles::Theme`] extension. [`Default`] is a desktop
/// whose `breezerc` and `[KDE]` group set nothing.
///
/// It holds what `breezerc` sets (read by
/// [`BreezeSettings::from_config`](teksilo_kde_scheme::BreezeSettings::from_config))
/// and the desktop's animation speed, `kdeglobals [KDE]
/// AnimationDurationFactor`. Every theme of this preset carries one:
/// [`crate::light`] and the other stock themes carry the defaults, and
/// [`crate::from_desktop`] the desktop's own. The styles read it from the
/// theme they paint under, so a theme without one paints the defaults.
///
/// What the preset does with each setting:
///
/// | Setting | Effect |
/// | --- | --- |
/// | `[Style] MenuOpacity` | Below 100, a menu's panel is filled at that opacity and outlined in the window text at 25 % ([`crate::styles::popup`]) |
/// | `[Common] ShadowSize`, `ShadowStrength`, `ShadowColor` | The window shadow under every popup, `shadow_lg` and `shadow_inner_lg` ([`crate::shape::breeze_shape_for`]) |
/// | `[Style] ProgressBarAnimated`, `ProgressBarBusyStepDuration` | Whether a busy progress bar's stripes move, and the period they move in ([`crate::styles::progress_bar`]) |
/// | `[KDE] AnimationDurationFactor` | The motion tokens ([`crate::motion::desktop_motion`]); 0 is the desktop asking for reduced motion ([`BreezeStyleConfig::prefers_reduced_motion`]) |
///
/// The other keys are carried for an application to read and change
/// nothing the preset paints, each for its reason:
///
/// - `AnimationsEnabled`, `AnimationsDuration` and `AnimationSteps` time
///   Breeze's own style animations; Teksilo's controls take their timing
///   from the motion tokens, which follow Kirigami's rule for the factor
///   (spec j.2), so a desktop's factor reaches them and these do not.
/// - `MenuItemDrawStrongFocus`: a highlighted menu row is always washed and
///   outlined, Breeze's default. With the key off Breeze marks the row the
///   keyboard is on with a 1 px underline only, a weaker focus indicator
///   than the lifted outline the preset keeps.
/// - `ViewDrawFocusIndicator`: the focus frame of a focused list row is
///   the view's keyboard focus indicator, and the preset keeps it.
/// - `ScrollBarAddLineButtons`, `ScrollBarSubLineButtons`,
///   `SliderDrawTickMarks`, `ViewDrawTreeBranchLines`,
///   `ViewInvertSortIndicator`, `TabBarDrawCenteredTabs`,
///   `ToolBarDrawItemSeparator`, `DockWidgetDrawFrame`,
///   `SidePanelDrawFrame`, `StackedWidgetTransitionsEnabled` and
///   `MnemonicsMode` change parts the widgets build themselves (a scroll
///   bar's buttons, a slider's ticks, a tree's lines, a tab bar's
///   alignment, a toolbar's separators, a stack's transition, the mnemonic
///   underline), which a style is not asked to draw.
///
/// ```
/// use teksilo_theme_kde::{self as kde, BreezeStyleConfig};
/// use teksilo_theme_kde::scheme::{KdeColorScheme, KdeDesktop};
/// use teksilo_tokens::TargetDensity;
///
/// let mut desktop = KdeDesktop::new(KdeColorScheme::breeze_light());
/// desktop.breeze.menu_opacity = 80;
/// let theme = kde::from_desktop(&desktop, TargetDensity::Compact);
/// let config = theme.extension::<BreezeStyleConfig>().unwrap();
/// assert_eq!(config.menu_opacity(), 0.8);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub struct BreezeStyleConfig {
    /// What `breezerc` sets.
    pub settings: BreezeSettings,
    /// `kdeglobals [KDE] AnimationDurationFactor`, `None` when the desktop
    /// does not set it.
    pub animation_factor: Option<f64>,
}

impl BreezeStyleConfig {
    /// The settings `desktop` configures.
    pub fn from_desktop(desktop: &KdeDesktop) -> BreezeStyleConfig {
        BreezeStyleConfig {
            settings: desktop.breeze,
            animation_factor: desktop.animation_duration_factor,
        }
    }

    /// `[S]` The opacity of a menu's panel, `MenuOpacity / 100`: 1 unless
    /// the desktop makes menus translucent.
    pub fn menu_opacity(&self) -> f32 {
        f32::from(self.settings.menu_opacity.min(100)) / 100.0
    }

    /// `[S]` Whether menus are translucent: `MenuOpacity` below 100, which
    /// is when Breeze also outlines the panel in the window text at 25 %.
    pub fn menu_is_translucent(&self) -> bool {
        self.settings.menu_opacity < 100
    }

    /// `[Hp]` The strength of the window shadow, `ShadowStrength / 255`.
    pub fn shadow_strength(&self) -> f32 {
        f32::from(self.settings.shadow_strength) / 255.0
    }

    /// `[S]` The period of a busy progress bar's stripes, or `None` when
    /// they stand still: `ProgressBarAnimated` is off, or
    /// `ProgressBarBusyStepDuration` is not a positive number of
    /// milliseconds (`[unpublished]`: a period of no length cannot move).
    pub fn busy_step(&self) -> Option<Duration> {
        let ms = self.settings.progress_bar_busy_step_duration_ms;
        (self.settings.progress_bar_animated && ms > 0)
            .then(|| Duration::from_millis(u64::try_from(ms).unwrap_or(u64::MAX)))
    }

    /// Whether the desktop asks for reduced motion: its animation duration
    /// factor is exactly 0, as the KDE desktop portal reports it. The
    /// platform reads the same factor into the accessibility preferences,
    /// which is how the framework's transitions learn to snap.
    pub fn prefers_reduced_motion(&self) -> bool {
        self.animation_factor == Some(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_kde_scheme::KdeColorScheme;

    #[test]
    fn the_defaults_are_a_desktop_that_sets_nothing() {
        let c = BreezeStyleConfig::default();
        assert_eq!(c.settings, BreezeSettings::default());
        assert_eq!(c.animation_factor, None);
        assert_eq!(c.menu_opacity(), 1.0);
        assert!(!c.menu_is_translucent());
        assert_eq!(c.shadow_strength(), 1.0);
        assert_eq!(c.busy_step(), Some(Duration::from_millis(800)));
        assert!(!c.prefers_reduced_motion());
        let desktop = KdeDesktop::new(KdeColorScheme::breeze_light());
        assert_eq!(BreezeStyleConfig::from_desktop(&desktop), c);
    }

    #[test]
    fn a_desktop_carries_its_settings_through() {
        let mut desktop = KdeDesktop::new(KdeColorScheme::breeze_dark());
        desktop.breeze.menu_opacity = 75;
        desktop.breeze.shadow_strength = 51;
        desktop.breeze.progress_bar_busy_step_duration_ms = 400;
        desktop.animation_duration_factor = Some(0.0);
        let c = BreezeStyleConfig::from_desktop(&desktop);
        assert_eq!(c.menu_opacity(), 0.75);
        assert!(c.menu_is_translucent());
        assert!((c.shadow_strength() - 0.2).abs() < 1e-6);
        assert_eq!(c.busy_step(), Some(Duration::from_millis(400)));
        assert!(c.prefers_reduced_motion());
    }

    #[test]
    fn a_busy_bar_stands_still_when_its_animation_is_off_or_has_no_period() {
        let mut c = BreezeStyleConfig::default();
        c.settings.progress_bar_animated = false;
        assert_eq!(c.busy_step(), None);
        c.settings.progress_bar_animated = true;
        c.settings.progress_bar_busy_step_duration_ms = 0;
        assert_eq!(c.busy_step(), None);
        c.settings.progress_bar_busy_step_duration_ms = -5;
        assert_eq!(c.busy_step(), None);
    }

    #[test]
    fn only_an_exact_zero_factor_asks_for_reduced_motion() {
        let mut c = BreezeStyleConfig::default();
        for (factor, reduced) in [
            (None, false),
            (Some(1.0), false),
            (Some(0.5), false),
            (Some(1e-9), false),
            (Some(0.0), true),
        ] {
            c.animation_factor = factor;
            assert_eq!(c.prefers_reduced_motion(), reduced, "{factor:?}");
        }
    }
}
