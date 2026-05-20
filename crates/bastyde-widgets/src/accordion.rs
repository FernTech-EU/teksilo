//! Accordion — a collapsible section with clickable header.
//!
//! Content visibility is animated via `MaxSize::bind_max_height()` with an
//! animated `Signal<f32>`. When collapsed, max_height animates to 0; when
//! expanded, it animates to a large value (content sizes naturally within).
//! V2 attached handlers — no event() override.

use bastyde_canvas::{Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::signal::Signal;
use bastyde_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{BorderRole, Color, TextRole, TextStyle, TextStyleRole};

use crate::animations::collapse::Collapse;
use crate::primitives::{HStack, IconWidget, Spacer, TextWidget, VStack};

// ---------------------------------------------------------------------------
// AccordionRegion — thin wrapper that exposes Role::Region for aria-controls.
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct AccordionRegion {
    name: String,
    child: Option<WidgetId>,
}

impl AccordionRegion {
    fn new(name: String, child: WidgetId) -> Self {
        Self {
            name,
            child: Some(child),
        }
    }
}

impl Widget for AccordionRegion {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        self.child
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
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
            child.origin = bastyde_canvas::Point::new(bounds.x, bounds.y);
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::Region);
        builder.set_name(&self.name);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child.into_iter().collect()
    }
}

// ---------------------------------------------------------------------------
// Accordion widget
// ---------------------------------------------------------------------------

/// Accordion design tokens. Accordion is a group-4 composite
/// with no dedicated `Recipe*Style`, so its layout numbers live
/// alongside the widget that reads them.
pub const ACCORDION_HEADER_HEIGHT: f32 = 28.0;
pub const ACCORDION_HEADER_PADDING_HORIZONTAL: f32 = 8.0;
pub const ACCORDION_INDICATOR_SIZE: f32 = 12.0;
pub const ACCORDION_INDICATOR_GAP: f32 = 6.0;
pub const ACCORDION_CORNER_RADIUS: f32 = 4.0;

/// A collapsible section with a clickable header that toggles content visibility.
///
/// Content must be pre-registered via `content_id(id)`.
pub struct Accordion {
    title: String,
    expanded: Signal<bool>,
    content_id: Option<WidgetId>,
    pending_content: Option<Box<dyn Widget>>,
    root_child_id: Option<WidgetId>,
    /// Region wrapper ID — used for `aria-controls` on the header button.
    region_id: Option<WidgetId>,
    /// Optional override for the header foreground color (title text +
    /// chevron icon). When `None`, the accordion uses
    /// `theme.colors.text_primary`. Set this when the accordion is
    /// embedded inside a surface that uses a non-standard text color
    /// (rich tooltip, dark snackbar, etc.).
    title_color: Option<Color>,
    /// Optional override for the header title's text style. Defaults
    /// to `theme.typography.body` when `None`.
    title_style: Option<TextStyle>,
}

impl Accordion {
    pub fn new(title: impl Into<bastyde_i18n::LocalizedString>, expanded: Signal<bool>) -> Self {
        let ls: bastyde_i18n::LocalizedString = title.into();
        Self {
            title: ls.resolve_now(),
            expanded,
            content_id: None,
            pending_content: None,
            root_child_id: None,
            region_id: None,
            title_color: None,
            title_style: None,
        }
    }

    /// Override the header foreground color used for the title text and
    /// chevron icon. Defaults to `theme.colors.text_primary`.
    pub fn title_color(mut self, color: Color) -> Self {
        self.title_color = Some(color);
        self
    }

    /// Override the header title's `TextStyle`. Use this to make the
    /// disclosure label smaller (e.g. inside a tooltip) or to match
    /// a non-body typography role.
    pub fn title_style(mut self, style: TextStyle) -> Self {
        self.title_style = Some(style);
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) — wraps a raw title in `LocalizedString::literal`.
    #[doc(hidden)]
    pub fn new_literal(title: impl Into<String>, expanded: Signal<bool>) -> Self {
        Self::new(bastyde_i18n::LocalizedString::literal(title), expanded)
    }

    /// Set the content widget by pre-registered ID.
    pub fn content_id(mut self, id: WidgetId) -> Self {
        self.content_id = Some(id);
        self
    }

    /// Set an inline content widget (deferred insertion).
    pub fn content(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_content = Some(Box::new(widget));
        self
    }
}

impl std::fmt::Debug for Accordion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Accordion")
            .field("title", &self.title)
            .finish()
    }
}

impl Widget for Accordion {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Resolve deferred content if provided
        if let Some(pending) = self.pending_content.take() {
            self.content_id = Some(ctx.add_boxed(pending));
        }

        let theme = ctx.theme();
        let accordion_corner_radius = ACCORDION_CORNER_RADIUS;
        let focus_ring_width = theme.shape.focus_ring_width;
        let expanded = self.expanded.clone();

        // Keyboard focus state for focus ring
        let kb_focused = ctx.signal(false);

        // Header foreground: caller override (literal Color) wins, otherwise
        // the Primary text role so the title tracks theme changes.
        let header_fg: ColorProp = match self.title_color {
            Some(c) => c.into(),
            None => TextRole::Primary.into(),
        };

        // Header: title + spacer + chevron icon
        // Use two chevrons with visible_when so the icon updates reactively
        let chevron_down_id = ctx.add(IconWidget::chevron_down(16.0).bind_color(header_fg.clone()));
        let chevron_right_id =
            ctx.add(IconWidget::chevron_right(16.0).bind_color(header_fg.clone()));
        ctx.visible_when(chevron_down_id, expanded.clone());
        ctx.visible_when(chevron_right_id, expanded.map(|v| !*v));

        // Custom override wins; otherwise use the Body role so the title
        // tracks typography changes across themes.
        let title_widget = TextWidget::new_literal(&self.title).bind_color(header_fg);
        let title_widget = if let Some(style) = self.title_style.clone() {
            title_widget.style(style)
        } else {
            title_widget.style(TextStyleRole::Body)
        };
        let title_widget = title_widget.single_line().a11y_hidden();
        let title_id = ctx.add(title_widget);
        let spacer_id = ctx.add(Spacer::new());

        let header = ctx.add(
            HStack::new()
                .spacing(8.0)
                .add_child(title_id)
                .add_child(spacer_id)
                .add_child(chevron_down_id)
                .add_child(chevron_right_id),
        );

        // Int UI focus convention: an accent-colored border
        // appears on the header row itself on keyboard focus
        // instead of a separate ring. Header has no visible
        // rest-state border, so this border is width-zero at
        // rest and snaps to `focus_ring_width` on focus.
        let focus_border_role = kb_focused.map(|f| {
            if *f {
                BorderRole::Focused
            } else {
                BorderRole::Transparent
            }
        });
        let focus_border_width = kb_focused.map(move |f| if *f { focus_ring_width } else { 0.0 });
        let focus_rect_id = ctx.add(
            crate::primitives::RectWidget::new()
                .bind_border_color(focus_border_role)
                .bind_border_width(focus_border_width)
                .corner_radius(bastyde_tokens::CornerRadius::uniform(accordion_corner_radius)),
        );
        let header_with_ring = ctx.add(
            crate::primitives::ZStack::new()
                .add_child(focus_rect_id)
                .add_child(header),
        );

        let mut vstack = VStack::new().spacing(2.0).add_child(header_with_ring);
        if let Some(content_id) = self.content_id {
            // Wrap content in AccordionRegion (Role::Region) so AT can navigate
            // to the content via the header's aria-controls relationship.
            let region_id = ctx.add(AccordionRegion::new(self.title.clone(), content_id));
            self.region_id = Some(region_id);

            // Disclosure animation is handled by `Collapse`, which
            // observes `expanded` and tweens both height and width
            // (width gate prevents the natural-width balloon that
            // would otherwise push tooltip-footer siblings off the row
            // mid-tween).
            let wrapper = ctx.add(Collapse::new(self.expanded.clone()).child_id(region_id));
            vstack = vstack.add_child(wrapper);
        }

        let root = ctx.add(vstack);
        self.root_child_id = Some(root);

        // --- V2 attached handlers ---
        // Handlers just flip `expanded`; the inner `Collapse` widget
        // observes the signal and drives the height/width tween.
        let expanded_tap = self.expanded.clone();
        let expanded_key = self.expanded.clone();
        let kb_focused_focus = kb_focused.clone();

        let handler_set = HandlerSet::new()
            .on_tap({
                move |_pos, _ctx: &mut EventContext| {
                    expanded_tap.set(!expanded_tap.get());
                }
            })
            .on_key({
                move |event: &WidgetEvent, _ctx: &mut EventContext| -> EventResponse {
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::Space | Key::Enter,
                            ..
                        } => EventResponse::Handled,
                        WidgetEvent::KeyUp {
                            key: Key::Space | Key::Enter,
                            ..
                        } => {
                            expanded_key.set(!expanded_key.get());
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            })
            .on_focus({
                move |gained: bool, _ctx: &mut EventContext| {
                    // Only show focus ring for keyboard focus (approximation:
                    // always show on gain, clear on loss — the V1 code checked origin)
                    kb_focused_focus.set(gained);
                }
            })
            .focusable(true)
            .cursor(CursorIcon::Pointer);

        ctx.apply_self_handlers(handler_set);

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
        builder.set_role(bastyde_core::accesskit::Role::Button);
        builder.set_name(&self.title);
        builder.set_expanded(self.expanded.get());
        builder.add_action(bastyde_core::accesskit::Action::Click);
        builder.add_action(bastyde_core::accesskit::Action::Focus);
        if let Some(region_id) = self.region_id {
            builder.push_controlled(bastyde_core::accessibility::widget_id_to_node_id(region_id));
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
    fn accordion_builds_collapsed() {
        let expanded = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let acc = tree.add(Accordion::new_literal("Section", expanded.clone()));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        let b = tree.bounds(acc);
        assert!(b.width > 0.0);
    }

    #[test]
    fn click_toggles_expanded_state() {
        let expanded = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let acc = tree.add(Accordion::new_literal("Section", expanded.clone()));
        tree.layout(SizeProposal::exact(300.0, 200.0));

        tree.click(acc);
        assert!(expanded.get());
        tree.click(acc);
        assert!(!expanded.get());
    }

    #[test]
    fn accordion_with_content() {
        use crate::primitives::TextWidget;
        let expanded = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let content = tree.add(TextWidget::new_literal("Content text"));
        let acc = tree.add(Accordion::new_literal("Details", expanded.clone()).content_id(content));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        let b = tree.bounds(acc);
        assert!(b.height > 0.0);
    }

    #[test]
    fn accessibility() {
        let expanded = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let acc = tree.add(Accordion::new_literal("Details", expanded));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        let info = tree.accessibility_node(acc);
        assert_eq!(info.name(), Some("Details"));
        assert!(info.is_expanded());
    }

    #[test]
    fn external_signal_set_triggers_animation() {
        // Simulates an external mutation: app code sets `expanded` to
        // true without going through the accordion's tap handler. The
        // `Collapse` observer should still kick off the height tween.
        use crate::primitives::TextWidget;
        use std::time::Duration;

        let expanded = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let content = tree.add(TextWidget::new_literal("Some content"));
        let acc = tree.add(Accordion::new_literal("Section", expanded.clone()).content_id(content));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let collapsed = tree.bounds(acc).height;

        expanded.set(true);
        tree.tick_animations(Duration::from_millis(250));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let after = tree.bounds(acc).height;

        assert!(
            after > collapsed,
            "external set must drive expansion: {} > {}",
            after,
            collapsed
        );
    }

    #[test]
    fn double_toggle_round_trips_height() {
        use crate::primitives::TextWidget;
        use std::time::Duration;

        let expanded = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let content = tree.add(TextWidget::new_literal("Some content"));
        let acc = tree.add(Accordion::new_literal("Section", expanded.clone()).content_id(content));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let collapsed_initial = tree.bounds(acc).height;

        // Expand then collapse.
        tree.click(acc);
        tree.tick_animations(Duration::from_millis(250));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let expanded_h = tree.bounds(acc).height;

        tree.click(acc);
        tree.tick_animations(Duration::from_millis(250));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let collapsed_again = tree.bounds(acc).height;

        assert!(expanded_h > collapsed_initial);
        assert!(
            (collapsed_again - collapsed_initial).abs() < 1.0,
            "after collapse round-trip, height should match initial: {} vs {}",
            collapsed_again,
            collapsed_initial
        );
    }

    #[test]
    fn content_dormant_when_collapsed() {
        use crate::primitives::TextWidget;
        use std::time::Duration;

        let expanded = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let content = tree.add(TextWidget::new_literal("Some content text here"));
        let acc = tree.add(Accordion::new_literal("Section", expanded.clone()).content_id(content));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let collapsed_height = tree.bounds(acc).height;

        // Click to expand
        tree.click(acc);
        assert!(expanded.get(), "should be expanded after click");

        // Tick animation to completion (accordion uses 200ms animation)
        tree.tick_animations(Duration::from_millis(250));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let expanded_height = tree.bounds(acc).height;

        assert!(
            expanded_height > collapsed_height,
            "expanded height ({}) should be greater than collapsed height ({})",
            expanded_height,
            collapsed_height
        );
    }
}
