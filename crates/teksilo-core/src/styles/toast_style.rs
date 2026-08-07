// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `Toast`. See `docs/styling-system.md`.
//!
//! Themes the floating toast surface — the per-severity card the
//! `ToastHost` stacks at a viewport corner. The `Toast` widget keeps
//! its accessibility node (`Role::Alert` for Warning/Error,
//! `Role::Status` for Info/Success, plus the matching `Live` setting);
//! `ToastStyle` only owns the surface chrome.
//!
//! `ToastStyleConfig` exposes both severity AND priority so the recipe
//! can give Urgent/High toasts a slightly heavier shadow or a more
//! emphatic border without changing the severity tint. Pre-built child
//! ids (`content`, `leading_glyph`, `trailing_close`) follow the same
//! pattern as `BannerStyleConfig` — the widget assembles the functional
//! subtrees, the recipe arranges them.

use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::build_context::BuildContext;
use crate::styles::BannerSeverity;
use crate::widget_id::WidgetId;

/// Queue / assertiveness level — shared with the public `Toast` builder
/// API. Lives here in teksilo-core so the style trait can take it in its
/// config without depending on teksilo-widgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum ToastPriority {
    /// Queues if `max_visible` reached; admits in order. Default.
    #[default]
    Normal,
    /// Shown immediately; bumps the oldest Normal entry if needed.
    /// Doesn't interrupt screen-reader speech.
    High,
    /// Shown immediately; forces `Live::Assertive` regardless of severity.
    Urgent,
}

#[derive(Clone, Debug)]
pub struct ToastStyleConfig {
    /// Severity hint — drives the recipe's surface tint (same mapping
    /// as `BannerSeverity::surface()`).
    pub severity: BannerSeverity,
    /// Priority hint — recipes can use this to differentiate Urgent
    /// toasts (e.g., heavier shadow). Most recipes ignore it.
    pub priority: ToastPriority,
    /// Pre-built body subtree (title + body + optional action row +
    /// optional progress bar). The recipe arranges it next to the
    /// leading glyph.
    pub content: WidgetId,
    /// Pre-built `SeverityGlyph` (or app-supplied custom leading
    /// widget). The recipe places it at the leading edge.
    pub leading_glyph: WidgetId,
    /// Optional pre-built close `IconButton`. `None` when the toast
    /// was constructed with `.show_close_button(false)`.
    pub trailing_close: Option<WidgetId>,
}

pub trait ToastStyle: 'static {
    fn make_body(&self, cfg: &ToastStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedToastStyle = Rc<dyn ToastStyle>;
