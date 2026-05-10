//! SegmentedControl — mutually exclusive segments in a horizontal row.
//!
//! Level 2 widget that paints segments directly and uses cached bounds
//! for position-based click-to-select.

use std::cell::Cell;
use std::rc::Rc;

use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::focus::FocusOrigin;
use fern_core::signal::Signal;
use fern_core::widget::{CursorIcon, LayoutContext, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::CornerRadius;

/// Fallback character width when no text backend is available.
const FALLBACK_CHAR_WIDTH: f32 = 8.0;
const FALLBACK_LINE_HEIGHT: f32 = 16.0;

/// Internal leaf widget — one per visual segment. Carries the
/// per-segment accessibility node (`Role::RadioButton` + selected
/// state + label) and owns the click / hover handlers. It renders
/// nothing; the parent `SegmentedControl` paints the entire control
/// from `self.labels`, `self.selected`, and `self.hovered_segment`
/// in a single pass so visuals stay consistent.
#[derive(Debug)]
struct SegmentButton {
    label: String,
    index: usize,
    selected: Signal<usize>,
    enabled: bool,
    /// Shared with the parent's paint — each SegmentButton
    /// updates it from its own hover handler so the parent
    /// can render the hover highlight.
    hovered_segment: Rc<Cell<Option<usize>>>,
}

impl Widget for SegmentButton {
    fn build(&mut self, _ctx: &mut fern_core::build_context::BuildContext) -> Vec<WidgetId> {
        let enabled = self.enabled;
        let index = self.index;
        let selected = self.selected.clone();
        let hovered = self.hovered_segment.clone();

        let on_tap_selected = selected.clone();
        let handlers = HandlerSet::new()
            .cursor(CursorIcon::Pointer)
            // Not focusable on its own — focus stays on the parent
            // SegmentedControl so the keyboard arrow-key navigation
            // continues to work as one tab stop. The child nodes
            // exist purely for ATs to enumerate in browse mode.
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

        _ctx.apply_self_handlers(handlers);
        Vec::new()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        // The parent SegmentedControl assigns exact bounds in
        // `place_children`; `size_that_fits` is only consulted when
        // the parent uses `child_size`, which we don't. Return
        // whatever the proposal resolves to.
        proposal.resolve(0.0, 0.0).into()
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut Canvas, _ctx: &PaintContext) {
        // Parent paints. Empty by design.
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::RadioButton);
        builder.set_name(&self.label);
        builder.set_selected(self.selected.get() == self.index);
        if !self.enabled {
            builder.set_disabled();
        } else {
            builder.add_action(fern_core::accesskit::Action::Click);
        }
    }
}

/// A segmented control with mutually exclusive segments.
pub struct SegmentedControl {
    labels: Vec<String>,
    selected: Signal<usize>,
    enabled: bool,
    hovered_segment: Rc<Cell<Option<usize>>>,
    focus_origin: Rc<Cell<Option<FocusOrigin>>>,
    /// Child `SegmentButton` widget ids, one per label. Created in
    /// `build()`, positioned in `place_children()`. Each child owns
    /// its own a11y node (`Role::RadioButton`) and click/hover
    /// handlers so screen readers see per-segment nodes rather
    /// than a single opaque container.
    segment_ids: Vec<WidgetId>,
}

impl SegmentedControl {
    pub fn new(labels: Vec<String>, selected: Signal<usize>) -> Self {
        Self {
            labels,
            selected,
            enabled: true,
            hovered_segment: Rc::new(Cell::new(None)),
            focus_origin: Rc::new(Cell::new(None)),
            segment_ids: Vec::new(),
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    fn segment_count(&self) -> usize {
        self.labels.len()
    }

    /// The inset-by-focus-ring-envelope bounds where the frame and
    /// segment row actually paint. Computed identically in
    /// `paint` and `place_children` so geometry never drifts.
    fn compute_visual(&self, bounds: Rect, theme: &fern_core::Theme) -> Rect {
        let envelope = theme.shape.focus_ring_offset + theme.shape.focus_ring_width;
        Rect::new(
            bounds.x + envelope,
            bounds.y + envelope,
            (bounds.width - envelope * 2.0).max(0.0),
            (bounds.height - envelope * 2.0).max(0.0),
        )
    }

    /// The inner row — `visual` inset by the frame's border width.
    /// All segment-grid math (child placement, non-selected paint,
    /// selected paint anchor) uses this as its coordinate space.
    fn compute_inner(&self, visual: Rect, bw: f32) -> Rect {
        Rect::new(
            visual.x + bw,
            visual.y + bw,
            (visual.width - bw * 2.0).max(0.0),
            (visual.height - bw * 2.0).max(0.0),
        )
    }

    fn segment_width(&self, inner: Rect) -> f32 {
        let n = self.segment_count();
        if n == 0 {
            return 0.0;
        }
        inner.width / n as f32
    }

    fn segment_rect(&self, index: usize, inner: Rect) -> Rect {
        let w = self.segment_width(inner);
        Rect::new(inner.x + index as f32 * w, inner.y, w, inner.height)
    }

    /// Estimate the intrinsic width of all segments.
    fn estimate_width(&self, padding_h: f32) -> f32 {
        let n = self.segment_count();
        if n == 0 {
            return 0.0;
        }
        let max_label_width = self
            .labels
            .iter()
            .map(|l| l.len() as f32 * FALLBACK_CHAR_WIDTH)
            .fold(0.0_f32, f32::max);
        (max_label_width + padding_h * 2.0) * n as f32
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
        ctx: &mut fern_core::build_context::BuildContext,
    ) -> Vec<fern_core::widget_id::WidgetId> {
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.selected.bind_to(
            self_id,
            registry,
            fern_core::binding::BindingLevel::RepaintOnly,
        );

        let selected = self.selected.clone();
        let enabled = self.enabled;
        let n = self.segment_count();
        let hovered_segment = self.hovered_segment.clone();
        let focus_origin = self.focus_origin.clone();

        // Create one SegmentButton child per label. Each child owns
        // its own a11y node, tap handler, and hover handler. The
        // parent still paints the whole control — children have
        // empty paint() — and positions them by segment rect in
        // place_children().
        self.segment_ids.clear();
        for (index, label) in self.labels.iter().enumerate() {
            let id = ctx.add(SegmentButton {
                label: label.clone(),
                index,
                selected: selected.clone(),
                enabled,
                hovered_segment: hovered_segment.clone(),
            });
            self.segment_ids.push(id);
        }

        let mut handlers = HandlerSet::new()
            .focusable(enabled)
            .cursor(CursorIcon::Pointer);

        // Hover handler on the parent — only used to clear the
        // highlight when the pointer leaves the control bounds
        // entirely (individual segment entries/exits are handled
        // by the child SegmentButtons).
        {
            let hovered_segment = hovered_segment.clone();
            handlers = handlers.on_hover(move |entered, _ctx| {
                if !entered {
                    hovered_segment.set(None);
                }
            });
        }

        // Key handler — arrow keys cycle the selection. Focus
        // stays on the parent (single tab stop) matching the
        // standard ARIA RadioGroup keyboard model.
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

        // Focus handler
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

        // Access action handler
        {
            let selected = selected.clone();
            handlers = handlers.on_access_action(move |action, _ctx| {
                if n == 0 {
                    return EventResponse::Ignored;
                }
                if action == fern_core::accesskit::Action::Increment {
                    let current = selected.get();
                    selected.set((current + 1) % n);
                    EventResponse::Handled
                } else if action == fern_core::accesskit::Action::Decrement {
                    let current = selected.get();
                    selected.set(if current == 0 { n - 1 } else { current - 1 });
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            });
        }

        ctx.apply_self_handlers(handlers);

        self.segment_ids.clone()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        let envelope = ctx.theme.shape.focus_ring_offset + ctx.theme.shape.focus_ring_width;
        let sc_style = ctx.theme.components.segmented_control;
        let width = proposal
            .width
            .unwrap_or_else(|| self.estimate_width(sc_style.padding_horizontal) + envelope * 2.0);
        // Reserve the focus-ring envelope on top and bottom.
        let visual_h =
            (FALLBACK_LINE_HEIGHT + sc_style.padding_vertical * 2.0).max(sc_style.height);
        Size::new(width, visual_h + envelope * 2.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        // Children occupy the inner row (visual inset by the frame
        // border width) and are split into equal slices. `paint()`
        // uses the exact same `inner` grid for non-selected segments,
        // so hit-test rects line up with painted rects to the pixel.
        let n = self.segment_count();
        if n == 0 {
            return;
        }
        let sc_style = ctx.theme.components.segmented_control;
        let visual = self.compute_visual(bounds, ctx.theme);
        let inner = self.compute_inner(visual, sc_style.border_width);
        let seg_w = self.segment_width(inner);
        for (i, placement) in children.iter_mut().enumerate() {
            placement.origin = fern_canvas::Point::new(inner.x + i as f32 * seg_w, inner.y);
            placement.size = Size::new(seg_w, inner.height);
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let colors = &ctx.theme.colors;
        let shape = &ctx.theme.shape;
        let sc_style = ctx.theme.components.segmented_control;
        let n = self.segment_count();
        if n == 0 {
            return;
        }

        let visual = self.compute_visual(bounds, ctx.theme);
        let bw = sc_style.border_width;
        let inner = self.compute_inner(visual, bw);

        let selected = self.selected.get();
        let hovered = self.hovered_segment.get();
        let focused = self.focus_origin.get().is_some();
        let frame_cr = CornerRadius::uniform(sc_style.corner_radius);

        // 1. Outer frame — one rounded rect wrapping the whole control.
        let frame_border = if !self.enabled {
            colors.border
        } else {
            colors.border_strong
        };
        canvas.stroke_rounded_rect(visual, frame_cr, frame_border, bw);

        // 2. Non-selected segments: hover tint + label. No border.
        //    `segment_rect` steps along the `inner` grid — exactly the
        //    coordinate space `place_children` uses for hit-testing.
        for i in 0..n {
            if i == selected {
                continue;
            }
            let rect = self.segment_rect(i, inner);
            if self.enabled && hovered == Some(i) {
                canvas.fill_rounded_rect(rect, frame_cr, colors.surface_hover);
            }
            let text_color = if !self.enabled {
                colors.text_disabled
            } else {
                colors.text_primary
            };
            let text_rect = Rect::new(
                rect.x + sc_style.padding_horizontal,
                rect.y + sc_style.padding_vertical,
                (rect.width - sc_style.padding_horizontal * 2.0).max(0.0),
                (rect.height - sc_style.padding_vertical * 2.0).max(0.0),
            );
            canvas.draw_text(
                &self.labels[i],
                text_rect,
                &ctx.theme.typography.small,
                text_color,
            );
        }

        // 3. Selected segment — painted last so its border overlays
        //    the frame border AND any adjacent hover tint. The rect is
        //    the inner-grid slot extended uniformly by `bw` on all four
        //    sides, so its stroke exactly covers the frame edge on the
        //    outside edges and covers the neighbors' edge pixels on
        //    middle segments.
        if selected < n {
            let sel_base = self.segment_rect(selected, inner);
            let sel = Rect::new(
                sel_base.x - bw,
                sel_base.y - bw,
                sel_base.width + bw * 2.0,
                sel_base.height + bw * 2.0,
            );
            let (sel_bg, sel_border, sel_text) = if !self.enabled {
                (
                    colors.surface_selected_inactive,
                    colors.border,
                    colors.text_disabled,
                )
            } else if focused {
                (colors.accent, colors.accent, colors.text_on_accent)
            } else {
                (
                    colors.surface_selected_inactive,
                    colors.border_strong,
                    colors.text_primary,
                )
            };
            canvas.fill_rounded_rect(sel, frame_cr, sel_bg);
            canvas.stroke_rounded_rect(sel, frame_cr, sel_border, bw);

            let text_rect = Rect::new(
                sel.x + sc_style.padding_horizontal,
                sel.y + sc_style.padding_vertical,
                (sel.width - sc_style.padding_horizontal * 2.0).max(0.0),
                (sel.height - sc_style.padding_vertical * 2.0).max(0.0),
            );
            canvas.draw_text(
                &self.labels[selected],
                text_rect,
                &ctx.theme.typography.small,
                sel_text,
            );
        }

        // 4. Focus ring — drawn OUTSIDE the visual, inside the reserved envelope.
        if self.focus_origin.get() == Some(FocusOrigin::Keyboard) {
            let half_stroke = shape.focus_ring_width * 0.5;
            let ring_rect = Rect::new(
                bounds.x + half_stroke,
                bounds.y + half_stroke,
                (bounds.width - half_stroke * 2.0).max(0.0),
                (bounds.height - half_stroke * 2.0).max(0.0),
            );
            let ring_radius = sc_style.corner_radius + shape.focus_ring_offset + half_stroke;
            canvas.stroke_rounded_rect(
                ring_rect,
                CornerRadius::uniform(ring_radius),
                colors.focus_ring,
                shape.focus_ring_width,
            );
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // SegmentedControl is mutually exclusive — ARIA's closest
        // match is `RadioGroup`. Each visual segment is exposed as
        // a `Role::RadioButton` child node (see `SegmentButton`),
        // so screen readers enumerate the segments and their
        // selected state individually.
        builder.set_role(fern_core::accesskit::Role::RadioGroup);
        // Expose the current selection's label as the group's
        // value too, so when a user focuses the whole control the
        // SR immediately announces the active segment without
        // needing to drill into the children.
        let selected_index = self.selected.get();
        if let Some(label) = self.labels.get(selected_index) {
            builder.set_value(label);
        }
        if !self.enabled {
            builder.set_disabled();
        }
        builder.add_action(fern_core::accesskit::Action::Focus);
        builder.add_action(fern_core::accesskit::Action::Increment);
        builder.add_action(fern_core::accesskit::Action::Decrement);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.segment_ids.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::event::Modifiers;
    use fern_core::widget_tree::WidgetTree;
    use fern_core::Theme;

    #[test]
    fn click_selects_segment_by_position() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let sc = tree.add(SegmentedControl::new(
            vec!["A".into(), "B".into(), "C".into()],
            selected.clone(),
        ));
        tree.layout(SizeProposal::exact(300.0, 60.0));
        tree.render(); // cache bounds

        // Click at center of 300px-wide control with 3 segments (100px each).
        // Center is x=150 → segment 1 (the middle one).
        tree.click(sc);
        assert_eq!(selected.get(), 1, "click at center should select segment 1");
    }

    #[test]
    fn click_on_each_segment_lands_correctly() {
        // Regression for the inner-vs-visual-grid drift: the child
        // `SegmentButton` hit-test rects must match the painted
        // segment rects to the pixel. With 3 segments, clicking
        // the center of each slice in the unified `inner` grid
        // must select that slice.
        use fern_core::event::PointerButton;
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let sc = tree.add(SegmentedControl::new(
            vec!["A".into(), "B".into(), "C".into()],
            selected.clone(),
        ));
        tree.layout(SizeProposal::exact(300.0, 60.0));

        // The control's first child is SegmentButton 0 — use
        // child_bounds to get each painted segment rect and click
        // its center. If hit-test and paint geometry drift, the
        // click will fall outside the right child.
        for i in 0..3 {
            let rect = tree.child_bounds(sc, i);
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
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let sc = tree.add(SegmentedControl::new(
            vec!["A".into(), "B".into()],
            selected,
        ));
        tree.layout(SizeProposal::exact(300.0, 60.0));
        let info = tree.accessibility_node(sc);
        assert_eq!(info.role(), fern_core::accesskit::Role::RadioGroup);
    }

    #[test]
    fn paints_selected_segment_with_accent_when_focused() {
        let selected = Signal::new(1_usize);
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let sc = tree.add(SegmentedControl::new(
            vec!["A".into(), "B".into(), "C".into()],
            selected,
        ));
        tree.layout(SizeProposal::exact(300.0, 60.0));
        tree.focus(sc);
        let frame = tree.render();
        let accent = fern_core::presets::intui::light().colors.accent.to_array();
        assert!(
            frame.shapes.iter().any(|s| s.color == accent),
            "focused selected segment should render with accent color"
        );
    }

    #[test]
    fn unfocused_selected_segment_uses_inactive_surface() {
        let selected = Signal::new(1_usize);
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        tree.add(SegmentedControl::new(
            vec!["A".into(), "B".into(), "C".into()],
            selected,
        ));
        tree.layout(SizeProposal::exact(300.0, 60.0));
        let frame = tree.render();
        let accent = fern_core::presets::intui::light().colors.accent.to_array();
        let inactive = fern_core::presets::intui::light()
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
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let sc = tree.add(SegmentedControl::new(
            vec!["A".into(), "B".into()],
            selected,
        ));
        tree.layout(SizeProposal::exact(300.0, 60.0));
        let info = tree.accessibility_node(sc);
        assert!(
            info.actions()
                .contains(&fern_core::accesskit::Action::Increment)
        );
        assert!(
            info.actions()
                .contains(&fern_core::accesskit::Action::Decrement)
        );
    }
}
