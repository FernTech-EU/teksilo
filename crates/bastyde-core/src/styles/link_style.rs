// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `Link`. See `docs/styling-system.md`.
//!
//! The style owns the full visual: the per-state text colour (idle /
//! hover / pressed / visited / disabled), the underline policy, the
//! corner radius, and the focus-ring border. The `Link` widget owns
//! only the interaction state signals and dispatches events — it
//! passes the resolved text *string* (not a pre-built `TextWidget`)
//! through the config so the style can build the label with its own
//! colour binding.

use std::rc::Rc;

use crate::build_context::BuildContext;
use crate::signal::{Prop, Signal};
use crate::widget_id::WidgetId;

#[derive(Clone, Debug)]
pub struct LinkStyleConfig {
    /// Label text as a reactive `Prop<String>`. The widget converts its
    /// `LocalizedString` via `.into()` (a `tr!` source yields a bound,
    /// locale-reactive prop); the style binds it to the label widget.
    /// `bastyde-core` can't name `LocalizedString` (i18n depends on core,
    /// not the reverse), so the i18n type is erased to `Prop<String>` here.
    pub text: Prop<String>,
    /// `true` while the pointer is over the link.
    pub is_hovered: Signal<bool>,
    /// `true` while the link is being pressed (mouse-down or Space/Enter).
    pub is_pressed: Signal<bool>,
    /// `true` while the link holds keyboard focus.
    pub is_focused: Signal<bool>,
    /// `true` if the link's target has previously been visited. The
    /// app owns visited-tracking; default `Signal::new(false)` is
    /// fine for links that don't represent URLs.
    pub is_visited: Signal<bool>,
    /// `true` when the widget is effectively disabled (this node or
    /// any ancestor has its arena `enabled_state` resolved to false).
    /// Reactive: re-emits when `enabled_when(..., signal)` flips.
    pub is_disabled: Signal<bool>,
}

pub trait LinkStyle: 'static {
    fn make_body(&self, cfg: &LinkStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedLinkStyle = Rc<dyn LinkStyle>;
