// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `Banner`. See `docs/styling-system.md`.
//!
//! Themes the persistent inline status strip — the per-severity
//! surface tint, corner radius, padding, and the arrangement of the
//! leading severity glyph next to the message/action content. The
//! `Banner` widget keeps its `Role::Status` / `Live::Polite`
//! accessibility node and builds the functional `SeverityGlyph`
//! painter itself (principle 6: a domain renderer is not chrome).

use std::rc::Rc;

use bastyde_tokens::SurfaceRole;
use serde::{Deserialize, Serialize};

use crate::build_context::BuildContext;
use crate::widget_id::WidgetId;

/// Banner severity level. Drives the surface tint, glyph color, and
/// glyph shape. Apps with a "neutral" callout requirement should use
/// a `Card` instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BannerSeverity {
    /// Informational notice — accent-tinted background, circle glyph.
    Info,
    /// Success / confirmation — green-tinted background, circle glyph.
    Success,
    /// Non-fatal warning — amber-tinted background, triangle glyph.
    Warning,
    /// Error / critical condition — red-tinted background, circle glyph.
    Error,
}

impl BannerSeverity {
    /// Surface-tint role for the banner strip background.
    pub fn surface(self) -> SurfaceRole {
        match self {
            Self::Info => SurfaceRole::StatusInfo,
            Self::Success => SurfaceRole::StatusSuccess,
            Self::Warning => SurfaceRole::StatusWarning,
            Self::Error => SurfaceRole::StatusError,
        }
    }

    /// Foreground color for the leading severity glyph.
    pub fn glyph_color(self, theme: &crate::styles::Theme) -> bastyde_tokens::Color {
        match self {
            Self::Info => theme.colors.status_info_fg,
            Self::Success => theme.colors.status_success_fg,
            Self::Warning => theme.colors.status_warning_fg,
            Self::Error => theme.colors.status_error_fg,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BannerStyleConfig {
    /// Severity hint — drives the recipe's surface tint.
    pub severity: BannerSeverity,
    /// Pre-built message + action content (everything but the leading
    /// glyph) the strip arranges to the right of the glyph.
    pub content: WidgetId,
    /// Pre-built `SeverityGlyph` subtree — placed at the leading edge.
    pub leading_glyph: WidgetId,
}

pub trait BannerStyle: 'static {
    fn make_body(&self, cfg: &BannerStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedBannerStyle = Rc<dyn BannerStyle>;
