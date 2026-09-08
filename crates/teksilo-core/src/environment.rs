// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use crate::styles::Theme;

/// Layout direction for RTL/LTR support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayoutDirection {
    #[default]
    LeftToRight,
    RightToLeft,
}

/// Whether an assistive technology is driving the app, as reported by the
/// platform.
///
/// Lives here rather than in `teksilo-platform` because `Environment` is a
/// `teksilo-core` type and core cannot name platform's
/// `AccessibilityPreferences`; the platform layer pushes the value in exactly
/// as it pushes [`Environment::prefers_reduced_motion`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ScreenReaderState {
    /// The platform does not expose the state, or has not been asked yet. The
    /// default — behaviour is unchanged from before the field existed.
    #[default]
    Unknown,
    /// No assistive technology is attached.
    Inactive,
    /// An assistive technology is attached and reading the tree.
    Active,
}

/// Whether the platform's touch *exploration* mode is on — VoiceOver on iOS,
/// TalkBack's "Explore by touch" on Android, Narrator touch mode on Windows.
///
/// While it is on, a touch is a *probe*: the first tap announces what is under
/// the finger and a second tap activates it. Gesture recognition must step
/// aside for it, which is why this is an environment flag and not a preference
/// a widget reads case by case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ExploreByTouch {
    /// Explore-by-touch is off, or the platform does not report it. The
    /// default, and today's behaviour.
    #[default]
    Off,
    /// Follow [`ScreenReaderState`]: on when a screen reader is active.
    Auto,
    /// Explore-by-touch is on regardless of the screen-reader state.
    On,
}

/// Environment data that flows down the widget tree.
/// Subtrees can override parts of the environment.
#[derive(Debug, Clone)]
pub struct Environment {
    pub theme: Theme,
    pub layout_direction: LayoutDirection,
    pub scale_factor: f32,
    pub prefers_high_contrast: bool,
    pub prefers_reduced_motion: bool,
    pub prefers_large_text: bool,
    /// The OS's stated preference for a touch-first UI (Windows tablet mode,
    /// a convertible in slate posture), or `None` when the platform does not
    /// report one.
    ///
    /// Nothing writes it and nothing reads it yet: it is the input to a
    /// density policy the framework does not act on — see
    /// [`WidgetTree::set_density_policy`](crate::WidgetTree::set_density_policy).
    pub prefers_touch: Option<bool>,
    /// Whether an assistive technology is attached. See [`ScreenReaderState`].
    pub screen_reader: ScreenReaderState,
    /// Whether the platform's touch-exploration mode is on. See
    /// [`ExploreByTouch`].
    pub explore_by_touch: ExploreByTouch,
}

impl Environment {
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
            layout_direction: LayoutDirection::default(),
            scale_factor: 1.0,
            prefers_high_contrast: false,
            prefers_reduced_motion: false,
            prefers_large_text: false,
            prefers_touch: None,
            screen_reader: ScreenReaderState::default(),
            explore_by_touch: ExploreByTouch::default(),
        }
    }

    /// Apply a theme override function, returning a new Environment with the
    /// modified theme while preserving all other fields.
    pub fn with_theme_override(&self, f: &dyn Fn(&mut Theme)) -> Self {
        let mut env = self.clone();
        f(&mut env.theme);
        env
    }
}

impl Default for Environment {
    fn default() -> Self {
        Self::new(crate::presets::intui::light())
    }
}

/// A stored theme override closure for a widget node.
/// When present on a node, its subtree sees a modified theme.
pub(crate) struct ThemeOverride {
    pub func: Box<dyn Fn(&mut Theme)>,
}

impl std::fmt::Debug for ThemeOverride {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ThemeOverride(..)")
    }
}
