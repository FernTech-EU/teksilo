//! Semantic color roles.
//!
//! Role enums let widgets name *what* a color represents rather than *which
//! exact hex value* to use, so the theme can change underneath without
//! cooperation from every callsite. The widget stores the role and resolves
//! it against the current theme at paint time, which is reactive by
//! construction: `WidgetTree::set_theme` dirty-marks every node and the next
//! paint pass reads the new theme.
//!
//! The escape hatches still exist for cases the roles don't cover:
//! - `.color(Color::RED)` for a genuinely static color.
//! - `.color(signal)` for a reactive source unrelated to the theme.
//!
//! Keep role sets small; add a new variant only when a widget repeatedly
//! needs a color that doesn't fit an existing role.
//!
//! When a widget accepts `impl Into<ColorProp>`, any of `Color`,
//! `TextRole`, `SurfaceRole`, `BorderRole`, or a `Signal<Color>` slot in.

use crate::color::Color;
use crate::theme::ColorTokens;

/// Semantic text-foreground role. Resolved against `ColorTokens` at paint time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextRole {
    /// Default body / primary text.
    #[default]
    Primary,
    /// Secondary / dimmed text — captions, helper text.
    Secondary,
    /// Disabled text.
    Disabled,
    /// Text painted on top of an accent surface.
    OnAccent,
    /// Accent-colored text (non-link emphasis).
    Accent,
    /// Error message text.
    Error,
    /// Warning message text.
    Warning,
    /// Success message text.
    Success,
    /// Inline hyperlink (idle).
    Link,
    /// Inline hyperlink on hover.
    LinkHover,
    /// Tooltip body text (dark surface in both themes).
    TooltipText,
    /// Tooltip shortcut chip text.
    TooltipShortcut,
    /// Editor foreground (code / prose editor pane).
    EditorFg,
    /// Editor gutter foreground (line numbers).
    EditorGutterFg,
}

impl TextRole {
    pub fn resolve(self, colors: &ColorTokens) -> Color {
        match self {
            TextRole::Primary => colors.text_primary,
            TextRole::Secondary => colors.text_secondary,
            TextRole::Disabled => colors.text_disabled,
            TextRole::OnAccent => colors.text_on_accent,
            TextRole::Accent => colors.accent,
            TextRole::Error => colors.text_error,
            TextRole::Warning => colors.text_warning,
            TextRole::Success => colors.text_success,
            TextRole::Link => colors.text_link,
            TextRole::LinkHover => colors.text_link_hover,
            TextRole::TooltipText => colors.tooltip_text,
            TextRole::TooltipShortcut => colors.tooltip_shortcut,
            TextRole::EditorFg => colors.editor_fg,
            TextRole::EditorGutterFg => colors.editor_gutter_fg,
        }
    }
}

/// Semantic surface / background role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SurfaceRole {
    /// Main window / panel background.
    #[default]
    Main,
    /// Content area (input fields, editors).
    Content,
    /// Raised (popups, menus, dialogs).
    Raised,
    /// Sunken (scrollable containers, code blocks).
    Sunken,
    /// Hover feedback surface.
    Hover,
    /// Pressed feedback surface.
    Pressed,
    /// Selected row / item background.
    Selected,
    /// Inactive selected (when the widget isn't focused).
    SelectedInactive,
    /// Accent fill (primary buttons, toggled checkboxes).
    Accent,
    /// Accent hover.
    AccentHover,
    /// Accent pressed.
    AccentPressed,
    /// Accent disabled.
    AccentDisabled,
    /// Subtle accent tint (badges, info backgrounds).
    AccentSubtle,
    /// Status info background.
    StatusInfo,
    /// Status success background.
    StatusSuccess,
    /// Status warning background.
    StatusWarning,
    /// Status error background.
    StatusError,
    /// Tooltip body background (dark).
    TooltipBg,
    /// Editor pane background.
    EditorBg,
    /// Editor caret color (used as the caret's fill rect in text editors).
    EditorCaret,
    /// Editor current-line highlight background.
    EditorCurrentLineBg,
    /// Editor selection range background.
    EditorSelectionBg,
    /// Modal scrim (overlay dim).
    Scrim,
}

impl SurfaceRole {
    pub fn resolve(self, colors: &ColorTokens) -> Color {
        match self {
            SurfaceRole::Main => colors.surface_main,
            SurfaceRole::Content => colors.surface_content,
            SurfaceRole::Raised => colors.surface_raised,
            SurfaceRole::Sunken => colors.surface_sunken,
            SurfaceRole::Hover => colors.surface_hover,
            SurfaceRole::Pressed => colors.surface_pressed,
            SurfaceRole::Selected => colors.surface_selected,
            SurfaceRole::SelectedInactive => colors.surface_selected_inactive,
            SurfaceRole::Accent => colors.accent,
            SurfaceRole::AccentHover => colors.accent_hover,
            SurfaceRole::AccentPressed => colors.accent_pressed,
            SurfaceRole::AccentDisabled => colors.accent_disabled,
            SurfaceRole::AccentSubtle => colors.accent_subtle_bg,
            SurfaceRole::StatusInfo => colors.status_info_bg,
            SurfaceRole::StatusSuccess => colors.status_success_bg,
            SurfaceRole::StatusWarning => colors.status_warning_bg,
            SurfaceRole::StatusError => colors.status_error_bg,
            SurfaceRole::TooltipBg => colors.tooltip_bg,
            SurfaceRole::EditorBg => colors.editor_bg,
            SurfaceRole::EditorCaret => colors.editor_caret,
            SurfaceRole::EditorCurrentLineBg => colors.editor_current_line_bg,
            SurfaceRole::EditorSelectionBg => colors.editor_selection_bg,
            SurfaceRole::Scrim => colors.scrim,
        }
    }
}

/// Semantic border / divider role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BorderRole {
    /// Default 1 dp border.
    #[default]
    Default,
    /// Stronger border (hover, focused rest).
    Strong,
    /// Focus-ring border.
    Focused,
    /// Error border.
    Error,
    /// Warning border.
    Warning,
    /// Divider — between list rows, panels.
    Divider,
    /// Strong divider — between content regions.
    DividerStrong,
    /// Tooltip border.
    TooltipBorder,
}

impl BorderRole {
    pub fn resolve(self, colors: &ColorTokens) -> Color {
        match self {
            BorderRole::Default => colors.border,
            BorderRole::Strong => colors.border_strong,
            BorderRole::Focused => colors.border_focused,
            BorderRole::Error => colors.border_error,
            BorderRole::Warning => colors.border_warning,
            BorderRole::Divider => colors.divider,
            BorderRole::DividerStrong => colors.divider_strong,
            BorderRole::TooltipBorder => colors.tooltip_border,
        }
    }
}
