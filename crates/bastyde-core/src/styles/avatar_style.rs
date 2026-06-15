// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `Avatar`. See `docs/styling-system.md`.
//!
//! The style owns everything *around* the inner content: the shape's
//! background fill (with the hash-derived palette pick when no caller
//! override is supplied), the border ring, the keyboard focus ring,
//! and the presence indicator dot. The `Avatar` widget builds the
//! inner content (`InitialsLeaf` or an `ImageWidget`) and passes it
//! in as a pre-built `content` id; the style composes its chrome
//! around that content.
//!
//! Avatar's domain enums (`AvatarShape`, `AvatarSize`, `AvatarPresence`,
//! `AvatarCorner`) live here so the config can carry them and custom
//! `AvatarStyle` implementations can branch on them.

use std::rc::Rc;

use bastyde_tokens::Color;

use crate::build_context::BuildContext;
use crate::color_prop::ColorProp;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

/// Discrete avatar size variants. `Custom(px)` accepts an arbitrary
/// logical-pixel side length. The default size resolution table lives
/// in `bastyde_widgets::styles::recipe_avatar_style::avatar_pixel_size`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum AvatarSize {
    /// Small — list rows, mention chips.
    Small,
    /// Medium — comment threads, sidebars (default).
    #[default]
    Medium,
    /// Large — profile cards.
    Large,
    /// X-large — settings, "your account" headers.
    XLarge,
    /// Arbitrary side length.
    Custom(f32),
}

/// Outer outline.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum AvatarShape {
    #[default]
    Circle,
    RoundedSquare,
    Square,
}

/// Presence indicator dot drawn at one corner of the avatar.
#[derive(Debug, Clone)]
pub enum AvatarPresence {
    Online,
    Offline,
    Away,
    Busy,
    Custom { color: ColorProp, label: String },
}

impl AvatarPresence {
    /// Resolve the dot's fill colour against the active theme.
    pub fn color(&self, theme: &crate::styles::Theme) -> Color {
        match self {
            AvatarPresence::Online => theme.colors.status_success_fg,
            AvatarPresence::Offline => theme.colors.text_disabled,
            AvatarPresence::Away => theme.colors.status_warning_fg,
            AvatarPresence::Busy => theme.colors.status_error_fg,
            // The Avatar widget calls this helper from its paint() but
            // doesn't currently thread `effective_enabled` here. When
            // the Avatar composite migrates (commit 4 of the
            // enabled-state refactor) this signature widens to take
            // `enabled: bool` and the Custom presence respects it.
            AvatarPresence::Custom { color, .. } => color.resolve(theme, true),
        }
    }

    /// Accessible label for screen readers.
    pub fn label(&self) -> String {
        match self {
            AvatarPresence::Online => "Online".to_string(),
            AvatarPresence::Offline => "Offline".to_string(),
            AvatarPresence::Away => "Away".to_string(),
            AvatarPresence::Busy => "Busy".to_string(),
            AvatarPresence::Custom { label, .. } => label.clone(),
        }
    }
}

/// Where the presence dot is rendered relative to the avatar bounds.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum AvatarCorner {
    #[default]
    BottomTrailing,
    BottomLeading,
    TopTrailing,
    TopLeading,
}

impl AvatarCorner {
    /// `(x_factor, y_factor)` in `{-1, 1}` — `-1` = leading/top, `1` =
    /// trailing/bottom. Used by the recipe to position the presence
    /// dot.
    pub fn offset(self) -> (f32, f32) {
        match self {
            AvatarCorner::BottomTrailing => (1.0, 1.0),
            AvatarCorner::BottomLeading => (-1.0, 1.0),
            AvatarCorner::TopTrailing => (1.0, -1.0),
            AvatarCorner::TopLeading => (-1.0, -1.0),
        }
    }
}

#[derive(Clone, Debug)]
pub struct AvatarStyleConfig {
    pub shape: AvatarShape,
    pub size: AvatarSize,
    /// Pre-built content subtree (`InitialsLeaf` or `ImageWidget`).
    pub content: WidgetId,
    /// Current presence (if any). The widget passes the live value
    /// resolved from any bound signal at build time; reactive presence
    /// changes re-run `Avatar::build` so the chrome rebuilds with the
    /// new value.
    pub presence: Option<AvatarPresence>,
    pub presence_corner: AvatarCorner,
    /// `true` while the avatar holds keyboard focus — drives the
    /// outer focus ring.
    pub is_focused: Signal<bool>,
    /// Caller override for the background fill. `None` lets the
    /// recipe pick a colour from the chart palette using `seed`.
    pub background_override: Option<ColorProp>,
    /// Caller override for the border ring colour. `None` lets the
    /// recipe use `theme.colors.surface_main`.
    pub border_color_override: Option<ColorProp>,
    /// Caller override for the border ring width. `None` = no border.
    pub border_width_override: Option<f32>,
    /// Seed string for the hash-derived background palette pick
    /// (typically the avatar's name or initials).
    pub seed: String,
}

pub trait AvatarStyle: 'static {
    fn make_body(&self, cfg: &AvatarStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedAvatarStyle = Rc<dyn AvatarStyle>;
