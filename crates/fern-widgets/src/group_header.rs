//! GroupHeader — a horizontal section header: label followed by a trailing
//! rule line that fills the remaining width.
//!
//! Used to segment settings pages, preference sheets, and forms into labelled
//! regions without the heavier chrome of a [`GroupBox`](crate::group_box::GroupBox).
//! Int UI and Jewel use this pattern as a lightweight "soft divider with a
//! caption" between groups of related controls.
//!
//! ```ignore
//! GroupHeader::new_literal("Appearance")
//! ```
//!
//! Trivially composed from existing primitives:
//! `HStack → TextWidget + Expand(Divider)`.

use fern_canvas::{Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::color_prop::ColorProp;
use fern_core::widget::{LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, TextRole, TextStyle};

use crate::primitives::{Divider, Expand, HStack, TextWidget};

/// A labelled section header with a trailing rule line.
pub struct GroupHeader {
    label: String,
    /// Optional text-style override for the label. Defaults to
    /// `theme.typography.body` — IntelliJ/Jewel group headers render at
    /// normal body size, not as a smaller caption.
    style: Option<TextStyle>,
    /// Optional label-color override. Defaults to
    /// `theme.colors.text_primary` (no dimming).
    color: Option<Color>,
    /// Horizontal gap between the label and the rule line.
    gap: f32,
    // Build state
    root_child_id: Option<WidgetId>,
}

impl GroupHeader {
    pub fn new(label: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        Self {
            label: ls.resolve_now(),
            style: None,
            color: None,
            gap: 8.0,
            root_child_id: None,
        }
    }

    /// Shim (permanent, `#[doc(hidden)]`) — wraps a raw label in
    /// `LocalizedString::literal` for tests and scaffolding where
    /// translation is overkill.
    #[doc(hidden)]
    pub fn new_literal(label: impl Into<String>) -> Self {
        Self::new(fern_i18n::LocalizedString::literal(label))
    }

    /// Override the label's text style (font, size, weight, …).
    pub fn style(mut self, style: TextStyle) -> Self {
        self.style = Some(style);
        self
    }

    /// Override the label's color. Useful when a consumer wants to
    /// emphasise a header with an accent.
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Horizontal gap between the label and the rule line. Defaults to 8 dp.
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }
}

impl std::fmt::Debug for GroupHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GroupHeader")
            .field("label", &self.label)
            .field("gap", &self.gap)
            .finish()
    }
}

impl Widget for GroupHeader {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let style = self
            .style
            .clone()
            .unwrap_or_else(|| ctx.theme().typography.body.clone());
        // Caller override (literal Color) wins, otherwise the Primary
        // text role so the label tracks runtime theme changes.
        let color: ColorProp = match self.color {
            Some(c) => c.into(),
            None => TextRole::Primary.into(),
        };

        let label = TextWidget::new_literal(&self.label)
            .style(style)
            .bind_color(color)
            .single_line()
            .a11y_hidden();
        let label_id = ctx.add(label);

        // Fill the remaining horizontal space with a horizontal Divider.
        // `Expand::horizontal()` defaults to flex=1, claiming leftover slack
        // from the parent HStack and stretching the divider to its bounds.
        let rule_id = ctx.add(Expand::horizontal().child(Divider::horizontal()));

        let row_id = ctx.add(
            HStack::new()
                .spacing(self.gap)
                .add_child(label_id)
                .add_child(rule_id),
        );
        self.root_child_id = Some(row_id);

        vec![row_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        match self.root_child_id {
            Some(id) => ctx
                .child_size(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0)),
            None => proposal.resolve(0.0, 0.0),
        }
        .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // A GroupHeader is a section caption: it names the region that
        // follows it without consuming focus or firing actions. `Label`
        // is the closest accesskit role — screen readers read it as a
        // non-interactive caption.
        builder.set_role(fern_core::accesskit::Role::Label);
        builder.set_name(&self.label);
    }

    fn children(&self) -> Vec<WidgetId> {
        match self.root_child_id {
            Some(id) => vec![id],
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;

    #[test]
    fn builds_and_lays_out_with_proposed_width() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let header = tree.add(GroupHeader::new_literal("Appearance"));
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: None,
        });
        let b = tree.bounds(header);
        // Header claims the full proposed width (label + spacer + rule).
        assert!(
            (b.width - 400.0).abs() < 0.01,
            "expected header width 400, got {}",
            b.width
        );
        // Height is driven by the label (single line of `small` text),
        // which is taller than the 1 dp divider, so the HStack height
        // equals the label height — strictly positive.
        assert!(b.height > 0.0);
    }

    #[test]
    fn rule_line_absorbs_remaining_width() {
        // The header's root HStack child is `[label, expand(divider)]`.
        // Walk the tree to the Expand and verify its bounds consume the
        // remaining width, not the natural 0-width of a bare Divider.
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let header = tree.add(GroupHeader::new_literal("X"));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });

        // Collect descendants and find the Divider (role=Splitter).
        let mut queue = vec![header];
        let mut divider_bounds = None;
        while let Some(id) = queue.pop() {
            let info = tree.accessibility_node(id);
            if info.role() == fern_core::accesskit::Role::Splitter {
                divider_bounds = Some(tree.bounds(id));
                break;
            }
            queue.extend(tree.children(id));
        }
        let db = divider_bounds.expect("GroupHeader should contain a Divider");
        // The divider should be substantially wider than zero — it fills
        // whatever the label didn't claim.
        assert!(
            db.width > 100.0,
            "rule line should absorb remaining width (got {})",
            db.width
        );
    }

    #[test]
    fn accessibility_role_and_name() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let header = tree.add(GroupHeader::new_literal("Appearance"));
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: None,
        });
        let info = tree.accessibility_node(header);
        assert_eq!(info.role(), fern_core::accesskit::Role::Label);
        assert_eq!(info.name(), Some("Appearance"));
    }

    #[test]
    fn custom_gap_respected() {
        // A large gap should push the rule line start further right,
        // so the rule line width should be smaller than with gap=0.
        fn divider_width(tree: &WidgetTree, root: WidgetId) -> f32 {
            let mut queue = vec![root];
            while let Some(id) = queue.pop() {
                let info = tree.accessibility_node(id);
                if info.role() == fern_core::accesskit::Role::Splitter {
                    return tree.bounds(id).width;
                }
                queue.extend(tree.children(id));
            }
            panic!("no divider found");
        }

        let mut tree_default = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let h0 = tree_default.add(GroupHeader::new_literal("Section").gap(0.0));
        tree_default.layout(SizeProposal {
            width: Some(400.0),
            height: None,
        });

        let mut tree_wide = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let h60 = tree_wide.add(GroupHeader::new_literal("Section").gap(60.0));
        tree_wide.layout(SizeProposal {
            width: Some(400.0),
            height: None,
        });

        let w0 = divider_width(&tree_default, h0);
        let w60 = divider_width(&tree_wide, h60);
        assert!(
            w60 < w0,
            "wider gap should shrink the rule line (gap=0 -> {w0}, gap=60 -> {w60})"
        );
    }
}
