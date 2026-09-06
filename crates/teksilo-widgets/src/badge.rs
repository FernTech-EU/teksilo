// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Badge — a pill-shaped label for tags, status indicators, and counts.
//!
//! `Badge` renders a short piece of text inside a rounded-pill container.
//! Common uses include tag chips on list items, unread-count bubbles in
//! navigation rails, and severity labels in alert rows. The pill chrome
//! (corner radius, padding, surface tint) is driven by the active
//! `BadgeStyle`; callers may swap it per-instance (`.style(...)`) or
//! theme-wide via `theme.style_slots.badge`.
//!
//! ## When to use
//!
//! - Inline chip that annotates another widget (version tag, "NEW" label).
//! - Standalone count indicator; pair with `SeverityBadge` for icon-backed
//!   status glyphs.
//!
//! ## Accessibility
//!
//! Announces as `Role::Label` with its resolved text as the AT name.
//! The inner `TextWidget` is hidden from AT to avoid double-announcement;
//! the badge emits that label's text runs from its own node instead, so a
//! reader can review the text by character, word and line rather than only
//! hear it.
//!
//! ```rust
//! # use teksilo_widgets::Badge;
//! # use teksilo_i18n::lit;
//! # use teksilo_tokens::Color;
//! let _badge = Badge::new(lit!("NEW"))
//!     .background(Color::new(0.2, 0.6, 1.0, 1.0));
//! ```

use std::rc::Rc;

use teksilo_canvas::{Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::accessibility::text_runs::{TextGeometryHandle, TextRunSource, push_text_runs};
use teksilo_core::build_context::BuildContext;
use teksilo_core::color_prop::ColorProp;
use teksilo_core::styles::{BadgeStyleConfig, SharedBadgeStyle};
use teksilo_core::widget::{LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::TextStyleRole;

use crate::primitives::TextWidget;
use teksilo_i18n::LocalizedString;

/// A pill-shaped label for displaying tags, counts, or status.
pub struct Badge {
    label: LocalizedString,
    background: Option<ColorProp>,
    text_role: Option<ColorProp>,
    /// Per-call override for the label's text style (font, size, weight).
    /// `None` ⇒ the default `TextStyleRole::Tiny`.
    text_style: Option<teksilo_core::color_prop::TextStyleProp>,
    /// Per-call override for the pill chrome.
    style_override: Option<SharedBadgeStyle>,
    root_child_id: Option<WidgetId>,
    /// What the hidden label last measured. The badge owns its accessible
    /// name, so it also owns the text runs behind it; the label lends its
    /// layout through this handle. Empty until the label has been placed.
    label_geometry: Option<TextGeometryHandle>,
    /// Optional plain tooltip text shown after a hover delay. Mutually exclusive
    /// with the rich / composite slots — every setter clears the other two so
    /// the last call wins.
    tooltip_text: Option<LocalizedString>,
    /// Optional rich tooltip source (registry key or inline content).
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    /// Optional composite tooltip body (arbitrary widget tree).
    composite_tooltip_content: Option<Box<dyn Widget>>,
}

impl Badge {
    /// Construct a badge with the given label text.
    pub fn new(label: impl Into<LocalizedString>) -> Self {
        Self {
            label: label.into(),
            background: None,
            text_role: None,
            text_style: None,
            style_override: None,
            root_child_id: None,
            label_geometry: None,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
        }
    }

    /// Per-call style override for the badge pill chrome. Replaces the
    /// theme-wide default `BadgeStyle` for just this instance.
    pub fn style(mut self, style: impl teksilo_core::styles::BadgeStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Override the badge background. Accepts `Color`, a
    /// [`SurfaceRole`](teksilo_tokens::SurfaceRole) / [`TextRole`](teksilo_tokens::TextRole),
    /// or a `Signal<Color>`. Default (unset) is `SurfaceRole::AccentSubtle`.
    pub fn background(mut self, color: impl Into<ColorProp>) -> Self {
        self.background = Some(color.into());
        self
    }

    /// Override the badge text color. Accepts `Color`, a role, or a signal.
    /// Default (unset) is the theme's `status_info_fg`.
    pub fn text_role(mut self, color: impl Into<ColorProp>) -> Self {
        self.text_role = Some(color.into());
        self
    }

    /// Override the label's text style (font, size, weight). Accepts a
    /// `TextStyleRole`, a `TextStyle`, or a `Signal` of either. Default
    /// (unset) is `TextStyleRole::Tiny`.
    pub fn text_style(mut self, style: impl Into<teksilo_core::color_prop::TextStyleProp>) -> Self {
        self.text_style = Some(style.into());
        self
    }

    /// Attach a plain single-line tooltip shown after a hover delay.
    ///
    /// Mutually exclusive with [`rich_tooltip`](Self::rich_tooltip),
    /// [`rich_tooltip_content`](Self::rich_tooltip_content), and
    /// [`composite_tooltip`](Self::composite_tooltip) — the last setter called wins.
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip identified by a registry key.
    ///
    /// Mutually exclusive with [`tooltip`](Self::tooltip),
    /// [`rich_tooltip_content`](Self::rich_tooltip_content), and
    /// [`composite_tooltip`](Self::composite_tooltip) — the last setter called wins.
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip from inline [`TooltipContent`](crate::tooltip::TooltipContent).
    ///
    /// Mutually exclusive with [`tooltip`](Self::tooltip),
    /// [`rich_tooltip`](Self::rich_tooltip), and
    /// [`composite_tooltip`](Self::composite_tooltip) — the last setter called wins.
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a composite tooltip with an arbitrary widget tree body.
    ///
    /// Mutually exclusive with [`tooltip`](Self::tooltip),
    /// [`rich_tooltip`](Self::rich_tooltip), and
    /// [`rich_tooltip_content`](Self::rich_tooltip_content) — the last setter called wins.
    pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
        self
    }
}

impl std::fmt::Debug for Badge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Badge").field("label", &self.label).finish()
    }
}

impl Widget for Badge {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme_signal = ctx.theme_signal();

        // Default text color: `status_info_fg` via a derived signal so
        // theme changes still propagate. Callers override with
        // `.text_role(...)`. The pill background default
        // (`AccentSubtle`) lives in the recipe; `.background(...)` reaches
        // the style as `background_override`.
        let text: ColorProp = self
            .text_role
            .take()
            .unwrap_or_else(|| ColorProp::Bound(theme_signal.map(|t| t.colors.status_info_fg)));

        let mut text_widget = TextWidget::new(self.label.clone())
            .color(text)
            .single_line()
            .a11y_hidden();
        text_widget = match &self.text_style {
            Some(style) => text_widget.style(style.clone()),
            None => text_widget.style(TextStyleRole::Tiny),
        };
        self.label_geometry = Some(text_widget.geometry_handle());
        let content = ctx.add(text_widget);

        // The pill chrome (rounded background + padding inset) is owned
        // by the active `BadgeStyle`.
        let style: SharedBadgeStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.badge.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeBadgeStyle::default()));
        let root = style.make_body(
            &BadgeStyleConfig {
                content,
                background_override: self.background.take(),
            },
            ctx,
        );
        self.root_child_id = Some(root);

        if let Some(content) = self.composite_tooltip_content.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed(ctx, root, content, delay);
        } else if let Some(source) = self.rich_tooltip_source.clone() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_rich_tooltip_source(ctx, root, source, delay);
        } else if let Some(text) = self.tooltip_text.clone() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_plain_tooltip(ctx, root, text, delay);
        }

        vec![root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        // Rigid: size to content, no shrink (see Button's note).
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
            child.origin = teksilo_canvas::Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::Label);

        // The badge owns the accessible name, so its label is hidden from
        // AT — which would leave the text announceable but not reviewable,
        // since `supports_text_ranges` needs `Role::TextRun` children.
        // Borrowing the hidden label's layout puts them on this node
        // instead, one level up from where they were measured.
        //
        // Those rects are in window space, which is sound only while the
        // label sits rigidly inside the badge: `place_children` gives the
        // whole subtree the badge's own bounds on every pass, so a move
        // re-places the label too. A composite that positioned its donor
        // independently would have to register local rects instead.
        let unplaced;
        let source = match self
            .label_geometry
            .as_ref()
            .and_then(|handle| TextRunSource::from_handle(handle, 0))
        {
            Some(source) => source,
            // Never placed — a name probe, or a tree asked for its
            // accessibility before its first layout.
            None => {
                unplaced = self.label.resolve_now();
                TextRunSource::flat(&unplaced, 0)
            }
        };
        let emission = push_text_runs(builder, None, &source);
        // The name must be byte-identical to the runs' concatenation: the
        // consumer derives the document text from the runs, and every
        // divergence is a place where what a reader reviews and what it
        // announces disagree.
        builder.set_name(&emission.value);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_i18n::lit;

    #[test]
    fn badge_builds_and_renders() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let badge = tree.add(Badge::new(lit!("New")));
        tree.layout(SizeProposal::exact(200.0, 50.0));
        let b = tree.bounds(badge);
        assert!(b.width > 0.0);
        assert!(b.height > 0.0);
    }

    /// The `Role::TextRun` children one node published, in tree order,
    /// as `(value, window-space bounds)`.
    fn runs_of(
        update: &teksilo_core::accesskit::TreeUpdate,
        owner: WidgetId,
    ) -> Vec<(String, teksilo_core::accesskit::Rect)> {
        let node_id = teksilo_core::accessibility::widget_id_to_node_id(owner);
        let node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == node_id)
            .map(|(_, node)| node)
            .expect("the badge is absent from the emitted tree");
        node.children()
            .iter()
            .filter_map(|child| update.nodes.iter().find(|(id, _)| id == child))
            .filter(|(_, node)| node.role() == teksilo_core::accesskit::Role::TextRun)
            .map(|(_, node)| {
                (
                    node.value().unwrap_or_default().to_string(),
                    node.bounds().expect("a run without bounds"),
                )
            })
            .collect()
    }

    /// A tree whose text is measured, so the runs carry real extents
    /// rather than the degenerate boxes an unmeasured label falls back to.
    fn measured_tree() -> WidgetTree {
        WidgetTree::new()
            .with_theme(teksilo_core::presets::intui::light())
            .with_text_backend(std::rc::Rc::new(std::cell::RefCell::new(
                teksilo_canvas::MockTextBackend::new(),
            )))
    }

    #[test]
    fn badge_accessibility() {
        let mut tree = measured_tree();
        let badge = tree.add(Badge::new(lit!("3")));
        tree.layout(SizeProposal::exact(200.0, 50.0));
        let info = tree.accessibility_node(badge);
        assert_eq!(info.role(), teksilo_core::accesskit::Role::Label);
        assert_eq!(info.name(), Some("3"));

        // The label is hidden from AT, so without borrowed runs the badge
        // would be announceable but not reviewable: `supports_text_ranges`
        // is false for a `Role::Label` with no `Role::TextRun` children.
        let update = tree.sync_accessibility();
        let runs = runs_of(&update, badge);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].0, "3");
        // Borrowed rects are absolute, so the run sits where the label was
        // painted — inside the pill, not at the window origin.
        assert!(
            runs[0].1.x0 > 0.0,
            "the run was not placed: {:?}",
            runs[0].1
        );
    }

    #[test]
    fn a_badge_is_reviewable_by_character() {
        // `supports_text_ranges` is a consumer method and the gate every
        // platform's text API sits behind, and `bounding_boxes` is where a
        // single geometry-less run silently empties a whole label. Neither
        // can be asserted off the emitted node.
        let mut tree = measured_tree();
        let badge = tree.add(Badge::new(lit!("NEW")));
        tree.layout(SizeProposal::exact(200.0, 50.0));
        let update = tree.sync_accessibility();
        let node_id = teksilo_core::accessibility::widget_id_to_node_id(badge);

        let consumer = accesskit_consumer::Tree::new(update, false);
        let state = consumer.state();
        let mut stack = vec![state.root()];
        let node = loop {
            let node = stack
                .pop()
                .expect("the badge is absent from the emitted tree");
            if node.locate().0 == node_id {
                break node;
            }
            stack.extend(node.children());
        };
        assert!(node.supports_text_ranges());
        assert_eq!(node.document_range().text(), "NEW");
        assert!(
            !node.document_range().bounding_boxes().is_empty(),
            "the badge reports no geometry, so a magnifier cannot follow it"
        );
    }

    #[test]
    fn a_translated_badge_moves_its_runs_by_the_same_delta() {
        // The borrowed rects are in window space, so they are only right
        // while the label is re-placed with the badge. A leading spacer
        // that grows moves both.
        use crate::primitives::{FixedSize, HStack};
        use teksilo_core::signal::Signal;

        let offset = Signal::new(0.0f32);
        let mut tree = measured_tree();
        let spacer = tree.add(FixedSize::new().width(offset.clone()).height(1.0));
        let badge = tree.add(Badge::new(lit!("NEW")));
        let _row = tree.add(HStack::new().add_child(spacer).add_child(badge));
        offset.bind_to(
            spacer,
            tree.binding_registry(),
            teksilo_core::binding::BindingLevel::Relayout,
        );

        tree.layout(SizeProposal::exact(400.0, 50.0));
        let before = runs_of(&tree.sync_accessibility(), badge);
        let x_before = tree.bounds(badge).x;

        offset.set(60.0);
        tree.layout(SizeProposal::exact(400.0, 50.0));
        let after = runs_of(&tree.sync_accessibility(), badge);
        let delta = f64::from(tree.bounds(badge).x - x_before);

        assert_eq!(before.len(), after.len());
        assert!(delta > 0.0, "the badge did not move");
        for ((text, before), (_, after)) in before.iter().zip(after.iter()) {
            assert!(
                (after.x0 - before.x0 - delta).abs() < 0.01,
                "run {text:?} moved by {} rather than {delta}",
                after.x0 - before.x0
            );
        }
    }

    #[test]
    fn tooltip_appears_on_hover() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(Badge::new(lit!("New")).tooltip(lit!("Tip")));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        tree.pointer_move(tree.bounds(id).center());
        tree.advance_time(std::time::Duration::from_secs(1));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "tooltip should appear on hover"
        );
        assert!(tree.find_by_label("Tip").is_some());
    }
}
