// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! StatusBar — a horizontal chrome bar at the bottom of a window for status
//! information.
//!
//! The bar publishes `Role::Status` so assistive technology can discover it as
//! a status landmark. It is **not** a live region by default — use
//! [`announce_changes(true)`](StatusBar::announce_changes) only for bars that
//! surface transient messages worth reading aloud (e.g. "Saved"), not for bars
//! showing continuous data like cursor position or zoom level that would flood
//! the screen reader. Visual chrome (background, border, corner radius) is
//! delegated to an inner [`Panel`].
//!
//! ```rust
//! # use bastyde_widgets::StatusBar;
//! # use bastyde_widgets::primitives::TextWidget;
//! # use bastyde_i18n::lit;
//! let _bar = StatusBar::new()
//!     .child(TextWidget::new(lit!("Ln 1, Col 1")))
//!     .announce_changes(false);
//! ```

use bastyde_canvas::{Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::signal::Prop;
use bastyde_core::widget::{LayoutContext, PendingChild, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;

use crate::Panel;
use crate::primitives::HStack;
use bastyde_tokens::SurfaceRole;

/// StatusBar design tokens.
pub const STATUS_BAR_HEIGHT: f32 = 22.0;
pub const STATUS_BAR_PADDING_HORIZONTAL: f32 = 8.0;
pub const STATUS_BAR_ITEM_GAP: f32 = 2.0;

/// A status bar for displaying information at the bottom of a window.
///
/// Visual chrome is delegated to an inner [`Panel`]. By default the bar
/// uses the `SurfaceRole::Sunken` surface with **square corners** (a bar
/// spanning the window edge shouldn't be rounded); override the surface
/// with [`background`](Self::background), the corners with
/// [`corner_radius`](Self::corner_radius), or add a frame with
/// [`border_color`](Self::border_color) / [`border_width`](Self::border_width).
///
/// Accessibility: the bar publishes `Role::Status` (→ AT-SPI `StatusBar`,
/// macOS `AXApplicationStatus`, Windows `UIA_StatusBarControlTypeId`) so
/// it is discoverable as a status landmark. It is **not** a live region
/// by default — a status bar showing continuously-changing data (cursor
/// position, zoom level, word count) would otherwise flood the screen
/// reader. Call [`announce_changes(true)`](Self::announce_changes) for a
/// bar that surfaces transient messages worth reading aloud ("Saved").
pub struct StatusBar {
    pending: Vec<PendingChild>,
    child_ids: Vec<WidgetId>,
    root_child_id: Option<WidgetId>,
    background: Option<ColorProp>,
    corner_radius: Option<Prop<f32>>,
    border_color: Option<ColorProp>,
    border_width: Option<Prop<f32>>,
    name: Option<Prop<String>>,
    announce_changes: bool,
}

impl StatusBar {
    /// Create an empty status bar with default styling (`SurfaceRole::Sunken`,
    /// square corners, no live region).
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            child_ids: Vec::new(),
            root_child_id: None,
            background: None,
            corner_radius: None,
            border_color: None,
            border_width: None,
            name: None,
            announce_changes: false,
        }
    }

    /// Add an inline child widget (deferred insertion).
    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.pending.push(PendingChild::Deferred(Box::new(widget)));
        self
    }

    /// Add a pre-registered child widget by ID.
    pub fn add_child(mut self, id: WidgetId) -> Self {
        self.pending.push(PendingChild::Id(id));
        self
    }

    /// Override the background surface. Accepts `Color`, a
    /// [`SurfaceRole`], or a `Signal<Color>`.
    /// Default (unset) is `SurfaceRole::Sunken`.
    pub fn background(mut self, color: impl Into<ColorProp>) -> Self {
        self.background = Some(color.into());
        self
    }

    /// Override the corner radius. Accepts a static `f32` or a reactive
    /// `Signal<f32>`. Default (unset) is `0.0` — square corners.
    pub fn corner_radius(mut self, radius: impl Into<Prop<f32>>) -> Self {
        self.corner_radius = Some(radius.into());
        self
    }

    /// Override the border color. Accepts `Color`, a
    /// [`BorderRole`](bastyde_tokens::BorderRole), or a `Signal<Color>`.
    /// Only painted when [`border_width`](Self::border_width) > 0.
    pub fn border_color(mut self, color: impl Into<ColorProp>) -> Self {
        self.border_color = Some(color.into());
        self
    }

    /// Override the border width. Accepts a static `f32` or a reactive
    /// `Signal<f32>`. Default (unset) is `0.0` — no border.
    pub fn border_width(mut self, width: impl Into<Prop<f32>>) -> Self {
        self.border_width = Some(width.into());
        self
    }

    /// Override the accessible name announced for the bar. Accepts a
    /// static string, a `Signal<String>`, or a `tr!(...)`
    /// [`LocalizedString`](bastyde_i18n::LocalizedString) (locale-reactive).
    /// Default (unset) is the localized "Status".
    pub fn name(mut self, name: impl Into<Prop<String>>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Control whether content changes are announced by assistive tech.
    ///
    /// Default `false`: the `Role::Status` landmark is published (still
    /// navigable) but the bar is not a live region, so continuously-changing
    /// data (cursor position, zoom, word count) doesn't flood the screen
    /// reader. Set `true` to make it a `Live::Polite` region for bars that
    /// surface transient messages worth reading aloud ("Saved").
    pub fn announce_changes(mut self, announce: bool) -> Self {
        self.announce_changes = announce;
        self
    }
}

impl Default for StatusBar {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for StatusBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatusBar").finish()
    }
}

impl Widget for StatusBar {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let _ = ctx.theme_signal();
        let spacing = STATUS_BAR_ITEM_GAP;

        // Register a bound `name` prop on the StatusBar itself at
        // AccessibilityOnly so a change to the status text re-walks the AT tree
        // and re-announces it (WCAG 4.1.3) — particularly when
        // `announce_changes(true)` makes this a `Live::Polite` region.
        // Static names are ignored by `register_if_bound`.
        if let Some(name) = self.name.as_ref() {
            name.register_if_bound(
                ctx.self_id(),
                ctx.binding_registry(),
                bastyde_core::binding::BindingLevel::AccessibilityOnly,
            );
        }

        // Resolve pending children
        let pending = std::mem::take(&mut self.pending);
        if !pending.is_empty() {
            self.child_ids = pending
                .into_iter()
                .map(|child| match child {
                    PendingChild::Id(id) => id,
                    PendingChild::Deferred(w) => ctx.add_boxed(w),
                })
                .collect();
        }

        let mut row = HStack::new().spacing(spacing);
        for &id in &self.child_ids {
            row = row.add_child(id);
        }

        let row_id = ctx.add(row);
        let mut panel = Panel::new()
            .background(
                self.background
                    .take()
                    .unwrap_or_else(|| SurfaceRole::Sunken.into()),
            )
            .corner_radius(self.corner_radius.take().unwrap_or(Prop::Static(0.0)))
            .padding(spacing)
            .a11y_presentational()
            .child_id(row_id);
        if let Some(border_color) = self.border_color.take() {
            panel = panel.border_color(border_color);
        }
        if let Some(border_width) = self.border_width.take() {
            panel = panel.border_width(border_width);
        }
        let root = ctx.add(panel);
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        if let Some(root) = self.root_child_id
            && let Some(size) = ctx.child_size(root, proposal)
        {
            return (size).into();
        }
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bastyde_canvas::Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::Status);
        let name = match &self.name {
            Some(prop) => prop.get(),
            None => bastyde_i18n::tr_widget!(a11y_status_bar_name()).resolve_now(),
        };
        builder.set_name(name);
        if self.announce_changes {
            builder.set_live(bastyde_core::accesskit::Live::Polite);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::widget_tree::WidgetTree;

    #[test]
    fn status_bar_builds() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let sb = tree.add(StatusBar::new());
        tree.layout(SizeProposal::exact(400.0, 50.0));
        let b = tree.bounds(sb);
        assert!(b.width > 0.0);
    }

    #[test]
    fn status_bar_accessibility() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let sb = tree.add(StatusBar::new());
        tree.layout(SizeProposal::exact(400.0, 50.0));
        let info = tree.accessibility_node(sb);
        assert_eq!(info.role(), bastyde_core::accesskit::Role::Status);
        assert_eq!(info.name(), Some("Status"));
    }

    #[test]
    fn status_bar_default_has_no_live_region() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let sb = tree.add(StatusBar::new());
        tree.layout(SizeProposal::exact(400.0, 50.0));
        let update = tree.sync_accessibility();
        let sb_nid = bastyde_core::accessibility::widget_id_to_node_id(sb);
        let sb_node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == sb_nid)
            .map(|(_, n)| n)
            .expect("status bar node in tree");
        // Role stays discoverable, but no auto-announce by default.
        assert_eq!(sb_node.role(), bastyde_core::accesskit::Role::Status);
        assert_eq!(sb_node.live(), None);
    }

    #[test]
    fn status_bar_name_override() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let sb = tree.add(StatusBar::new().name("Editor status".to_string()));
        tree.layout(SizeProposal::exact(400.0, 50.0));
        let info = tree.accessibility_node(sb);
        assert_eq!(info.name(), Some("Editor status"));
    }

    #[test]
    fn status_bar_announce_changes_enables_polite_live_region_and_no_group_wrapper() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let sb = tree.add(StatusBar::new().announce_changes(true));
        tree.layout(SizeProposal::exact(400.0, 50.0));
        let update = tree.sync_accessibility();
        let sb_nid = bastyde_core::accessibility::widget_id_to_node_id(sb);
        let sb_node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == sb_nid)
            .map(|(_, n)| n)
            .expect("status bar node in tree");
        assert_eq!(sb_node.live(), Some(bastyde_core::accesskit::Live::Polite));
        // Panel wrapper should be hidden so StatusBar → HStack directly,
        // no intermediate Role::Group node.
        let groups: Vec<_> = update
            .nodes
            .iter()
            .filter(|(_, n)| n.role() == bastyde_core::accesskit::Role::Group)
            .collect();
        assert!(
            groups.is_empty(),
            "expected no Role::Group wrapper under StatusBar, got {}",
            groups.len()
        );
    }
}
