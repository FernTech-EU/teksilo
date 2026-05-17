//! SegmentedControl — mutually exclusive segments in a horizontal row.
//!
//! The control's visual chrome (frame, hover tints, selected
//! highlight, labels, focus ring) is owned by `SegmentedControlStyle`.
//! `SegmentedControl::build` composes:
//!
//! - One `SegmentButton` per label — invisible-but-focusable a11y
//!   stubs (`Role::RadioButton`) that own per-segment click + hover
//!   handlers.
//! - One `SegmentedControlChrome` leaf built by the style — paints
//!   everything visible, driven by `selected` / `hovered_segment` /
//!   `focus_origin` signals.
//!
//! The widget itself has no `paint()`; it stays pure composition.

use std::rc::Rc;

use bastyde_canvas::{Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::focus::FocusOrigin;
use bastyde_core::signal::Signal;
use bastyde_core::styles::{SegmentedControlStyleConfig, SharedSegmentedControlStyle};
use bastyde_core::widget::{CursorIcon, LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;

use crate::styles::recipe_segmented_control_style::{
    SEGMENTED_CONTROL_BORDER_WIDTH, SEGMENTED_CONTROL_HEIGHT, SEGMENTED_CONTROL_PADDING_HORIZONTAL,
    SEGMENTED_CONTROL_PADDING_VERTICAL,
};

/// Fallback character width when no text backend is available.
const FALLBACK_CHAR_WIDTH: f32 = 8.0;
const FALLBACK_LINE_HEIGHT: f32 = 16.0;

/// Internal a11y / interaction stub — one per visual segment. Carries
/// the `Role::RadioButton` accessibility node and owns the per-segment
/// click + hover handlers. Paints nothing; the parent chrome leaf
/// paints the whole control.
#[derive(Debug)]
struct SegmentButton {
    label: String,
    index: usize,
    selected: Signal<usize>,
    enabled: bool,
    hovered_segment: Signal<Option<usize>>,
}

impl Widget for SegmentButton {
    fn build(&mut self, ctx: &mut bastyde_core::build_context::BuildContext) -> Vec<WidgetId> {
        let enabled = self.enabled;
        let index = self.index;
        let selected = self.selected.clone();
        let hovered = self.hovered_segment.clone();

        let on_tap_selected = selected.clone();
        let handlers = HandlerSet::new()
            .cursor(CursorIcon::Pointer)
            // Focus stays on the parent SegmentedControl (single tab
            // stop), matching the standard ARIA RadioGroup model.
            .focusable(false)
            .on_tap(move |_pos, _ctx| {
                if !enabled {
                    return;
                }
                on_tap_selected.set(index);
            })
            .on_hover({
                let hovered = hovered.clone();
                move |entered, _ctx| {
                    if !enabled {
                        if !entered && hovered.get() == Some(index) {
                            hovered.set(None);
                        }
                        return;
                    }
                    if entered {
                        hovered.set(Some(index));
                    } else if hovered.get() == Some(index) {
                        hovered.set(None);
                    }
                }
            });

        ctx.apply_self_handlers(handlers);
        Vec::new()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        // The parent SegmentedControl assigns exact bounds in
        // `place_children`; size_that_fits is irrelevant here.
        proposal.resolve(0.0, 0.0).into()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::RadioButton);
        builder.set_name(&self.label);
        builder.set_selected(self.selected.get() == self.index);
        if !self.enabled {
            builder.set_disabled();
        } else {
            builder.add_action(bastyde_core::accesskit::Action::Click);
        }
    }
}

/// A segmented control with mutually exclusive segments.
pub struct SegmentedControl {
    labels: Vec<String>,
    selected: Signal<usize>,
    enabled: bool,
    hovered_segment: Signal<Option<usize>>,
    focus_origin: Signal<Option<FocusOrigin>>,
    /// Per-call override for the chrome.
    style_override: Option<SharedSegmentedControlStyle>,
    /// Build-time children — chrome first (back), then one
    /// `SegmentButton` per label.
    children: Vec<WidgetId>,
    /// Number of segment children (excludes the leading chrome leaf
    /// in `children`). Used by `place_children` to grid-place the
    /// trailing entries.
    segment_count: usize,
}

impl SegmentedControl {
    pub fn new(labels: Vec<String>, selected: Signal<usize>) -> Self {
        Self {
            labels,
            selected,
            enabled: true,
            hovered_segment: Signal::new(None),
            focus_origin: Signal::new(None),
            style_override: None,
            children: Vec::new(),
            segment_count: 0,
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Per-call override for the segmented-control chrome.
    pub fn style(mut self, style: impl bastyde_core::styles::SegmentedControlStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Estimate the intrinsic width of all segments — used in
    /// `layout_response` when no width is proposed.
    fn estimate_width(&self) -> f32 {
        let n = self.labels.len();
        if n == 0 {
            return 0.0;
        }
        let max_label_width = self
            .labels
            .iter()
            .map(|l| l.len() as f32 * FALLBACK_CHAR_WIDTH)
            .fold(0.0_f32, f32::max);
        (max_label_width + SEGMENTED_CONTROL_PADDING_HORIZONTAL * 2.0) * n as f32
    }

    /// Inset-by-focus-ring-envelope bounds — the actual frame /
    /// segment-grid area. Mirrors the recipe's compute_visual so
    /// children land where the chrome paints.
    fn compute_visual(&self, bounds: Rect, theme: &bastyde_core::Theme) -> Rect {
        let envelope = theme.shape.focus_ring_offset + theme.shape.focus_ring_width;
        Rect::new(
            bounds.x + envelope,
            bounds.y + envelope,
            (bounds.width - envelope * 2.0).max(0.0),
            (bounds.height - envelope * 2.0).max(0.0),
        )
    }
}

impl std::fmt::Debug for SegmentedControl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SegmentedControl")
            .field("labels", &self.labels)
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl Widget for SegmentedControl {
    fn build(
        &mut self,
        ctx: &mut bastyde_core::build_context::BuildContext,
    ) -> Vec<bastyde_core::widget_id::WidgetId> {
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.selected.bind_to(
            self_id,
            registry,
            bastyde_core::binding::BindingLevel::RepaintOnly,
        );

        let selected = self.selected.clone();
        let enabled = self.enabled;
        let n = self.labels.len();
        let hovered_segment = self.hovered_segment.clone();
        let focus_origin = self.focus_origin.clone();

        // Build chrome leaf first (so it sits at index 0 in `children`
        // and paints behind the SegmentButton stubs).
        let style: SharedSegmentedControlStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.segmented_control.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeSegmentedControlStyle));
        let chrome_id = style.make_body(
            &SegmentedControlStyleConfig {
                labels: self.labels.clone(),
                selected: selected.clone(),
                hovered_segment: hovered_segment.clone(),
                focus_origin: focus_origin.clone(),
                is_enabled: enabled,
            },
            ctx,
        );

        // Build one SegmentButton per label — invisible a11y +
        // click/hover stubs sitting at the segment grid positions.
        self.children.clear();
        self.children.push(chrome_id);
        for (index, label) in self.labels.iter().enumerate() {
            let id = ctx.add(SegmentButton {
                label: label.clone(),
                index,
                selected: selected.clone(),
                enabled,
                hovered_segment: hovered_segment.clone(),
            });
            self.children.push(id);
        }
        self.segment_count = n;

        // Parent handlers — arrow keys, focus tracking, access actions.
        let mut handlers = HandlerSet::new()
            .focusable(enabled)
            .cursor(CursorIcon::Pointer);

        // Hover-out on the parent clears the segment highlight when
        // the pointer leaves the control entirely.
        {
            let hovered_segment = hovered_segment.clone();
            handlers = handlers.on_hover(move |entered, _ctx| {
                if !entered {
                    hovered_segment.set(None);
                }
            });
        }

        // Arrow keys cycle selection. Focus stays on the parent.
        {
            let selected = selected.clone();
            handlers = handlers.on_key(move |event, _ctx| {
                if !enabled || n == 0 {
                    return EventResponse::Ignored;
                }
                match event {
                    WidgetEvent::KeyDown {
                        key: Key::ArrowRight,
                        ..
                    } => {
                        let current = selected.get();
                        selected.set((current + 1) % n);
                        EventResponse::Handled
                    }
                    WidgetEvent::KeyDown {
                        key: Key::ArrowLeft,
                        ..
                    } => {
                        let current = selected.get();
                        selected.set(if current == 0 { n - 1 } else { current - 1 });
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }
            });
        }

        // Focus handler — track origin so the chrome can decide
        // accent-vs-inactive selected appearance and keyboard-vs-no
        // focus ring.
        {
            let focus_origin = focus_origin.clone();
            let hovered_segment = hovered_segment.clone();
            handlers = handlers.on_focus(move |gained, _ctx| {
                if gained {
                    let origin = if hovered_segment.get().is_some() {
                        FocusOrigin::Pointer
                    } else {
                        FocusOrigin::Keyboard
                    };
                    focus_origin.set(Some(origin));
                } else {
                    focus_origin.set(None);
                }
            });
        }

        // Access actions — increment/decrement cycle selection.
        {
            let selected = selected.clone();
            handlers = handlers.on_access_action(move |action, _ctx| {
                if n == 0 {
                    return EventResponse::Ignored;
                }
                if action == bastyde_core::accesskit::Action::Increment {
                    let current = selected.get();
                    selected.set((current + 1) % n);
                    EventResponse::Handled
                } else if action == bastyde_core::accesskit::Action::Decrement {
                    let current = selected.get();
                    selected.set(if current == 0 { n - 1 } else { current - 1 });
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            });
        }

        ctx.apply_self_handlers(handlers);

        self.children.clone()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        let envelope = ctx.theme.shape.focus_ring_offset + ctx.theme.shape.focus_ring_width;
        let width = proposal
            .width
            .unwrap_or_else(|| self.estimate_width() + envelope * 2.0);
        let visual_h = (FALLBACK_LINE_HEIGHT + SEGMENTED_CONTROL_PADDING_VERTICAL * 2.0)
            .max(SEGMENTED_CONTROL_HEIGHT);
        Size::new(width, visual_h + envelope * 2.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        // children[0] is the chrome leaf — fills the full control
        // bounds so it paints frame / labels / focus ring envelope.
        // children[1..] are the SegmentButton stubs, grid-placed in
        // the inner (visual minus border) rect so hit-tests land on
        // the painted segment cells.
        if children.is_empty() {
            return;
        }
        children[0].origin = bounds.origin();
        children[0].size = bounds.size();

        let n = self.segment_count;
        if n == 0 || children.len() < n + 1 {
            return;
        }
        let visual = self.compute_visual(bounds, ctx.theme);
        let bw = SEGMENTED_CONTROL_BORDER_WIDTH;
        let inner = Rect::new(
            visual.x + bw,
            visual.y + bw,
            (visual.width - bw * 2.0).max(0.0),
            (visual.height - bw * 2.0).max(0.0),
        );
        let seg_w = inner.width / n as f32;
        for (i, placement) in children.iter_mut().skip(1).take(n).enumerate() {
            placement.origin = bastyde_canvas::Point::new(inner.x + i as f32 * seg_w, inner.y);
            placement.size = Size::new(seg_w, inner.height);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::RadioGroup);
        let selected_index = self.selected.get();
        if let Some(label) = self.labels.get(selected_index) {
            builder.set_value(label);
        }
        if !self.enabled {
            builder.set_disabled();
        }
        builder.add_action(bastyde_core::accesskit::Action::Focus);
        builder.add_action(bastyde_core::accesskit::Action::Increment);
        builder.add_action(bastyde_core::accesskit::Action::Decrement);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.children.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::event::Modifiers;
    use bastyde_core::widget_tree::WidgetTree;

    #[test]
    fn click_selects_segment_by_position() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let sc = tree.add(SegmentedControl::new(
            vec!["A".into(), "B".into(), "C".into()],
            selected.clone(),
        ));
        tree.layout(SizeProposal::exact(300.0, 60.0));
        tree.render();
        tree.click(sc);
        assert_eq!(selected.get(), 1, "click at center should select segment 1");
    }

    #[test]
    fn click_on_each_segment_lands_correctly() {
        use bastyde_core::event::PointerButton;
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let sc = tree.add(SegmentedControl::new(
            vec!["A".into(), "B".into(), "C".into()],
            selected.clone(),
        ));
        tree.layout(SizeProposal::exact(300.0, 60.0));

        // The control's children are [chrome, segment_0, segment_1,
        // segment_2]; iterate the segment indices (1..=3).
        for i in 0..3 {
            let rect = tree.child_bounds(sc, i + 1);
            let center = rect.center();
            tree.pointer_down_button(center, PointerButton::Primary);
            tree.pointer_up_button(center, PointerButton::Primary);
            assert_eq!(
                selected.get(),
                i,
                "click on center of segment {} must select it",
                i
            );
        }
    }

    #[test]
    fn keyboard_navigation() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let sc = tree.add(SegmentedControl::new(
            vec!["A".into(), "B".into(), "C".into()],
            selected.clone(),
        ));
        tree.layout(SizeProposal::exact(300.0, 60.0));

        tree.focus(sc);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(selected.get(), 1);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(selected.get(), 2);
        tree.press_key(Key::ArrowLeft, Modifiers::NONE);
        assert_eq!(selected.get(), 1);
    }

    #[test]
    fn keyboard_wraps_around() {
        let selected = Signal::new(2_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let sc = tree.add(SegmentedControl::new(
            vec!["A".into(), "B".into(), "C".into()],
            selected.clone(),
        ));
        tree.layout(SizeProposal::exact(300.0, 60.0));

        tree.focus(sc);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(selected.get(), 0, "should wrap from last to first");
        tree.press_key(Key::ArrowLeft, Modifiers::NONE);
        assert_eq!(selected.get(), 2, "should wrap from first to last");
    }

    #[test]
    fn accessibility() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let sc = tree.add(SegmentedControl::new(
            vec!["A".into(), "B".into()],
            selected,
        ));
        tree.layout(SizeProposal::exact(300.0, 60.0));
        let info = tree.accessibility_node(sc);
        assert_eq!(info.role(), bastyde_core::accesskit::Role::RadioGroup);
    }

    #[test]
    fn paints_selected_segment_with_accent_when_focused() {
        let selected = Signal::new(1_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let sc = tree.add(SegmentedControl::new(
            vec!["A".into(), "B".into(), "C".into()],
            selected,
        ));
        tree.layout(SizeProposal::exact(300.0, 60.0));
        tree.focus(sc);
        let frame = tree.render();
        let accent = bastyde_core::presets::intui::light().colors.accent.to_array();
        assert!(
            frame.shapes.iter().any(|s| s.color == accent),
            "focused selected segment should render with accent color"
        );
    }

    #[test]
    fn unfocused_selected_segment_uses_inactive_surface() {
        let selected = Signal::new(1_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(SegmentedControl::new(
            vec!["A".into(), "B".into(), "C".into()],
            selected,
        ));
        tree.layout(SizeProposal::exact(300.0, 60.0));
        let frame = tree.render();
        let accent = bastyde_core::presets::intui::light().colors.accent.to_array();
        let inactive = bastyde_core::presets::intui::light()
            .colors
            .surface_selected_inactive
            .to_array();
        assert!(
            !frame.shapes.iter().any(|s| s.color == accent),
            "unfocused selected segment must not use accent color"
        );
        assert!(
            frame.shapes.iter().any(|s| s.color == inactive),
            "unfocused selected segment should render with surface_selected_inactive"
        );
    }

    #[test]
    fn accessibility_has_actions() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let sc = tree.add(SegmentedControl::new(
            vec!["A".into(), "B".into()],
            selected,
        ));
        tree.layout(SizeProposal::exact(300.0, 60.0));
        let info = tree.accessibility_node(sc);
        assert!(
            info.actions()
                .contains(&bastyde_core::accesskit::Action::Increment)
        );
        assert!(
            info.actions()
                .contains(&bastyde_core::accesskit::Action::Decrement)
        );
    }
}
