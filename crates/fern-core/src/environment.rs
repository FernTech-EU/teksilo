use fern_tokens::Theme;

/// Layout direction for RTL/LTR support.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutDirection {
    LeftToRight,
    RightToLeft,
}

impl Default for LayoutDirection {
    fn default() -> Self {
        Self::LeftToRight
    }
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
        Self::new(Theme::light_default())
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
