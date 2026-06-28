// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! SegmentedControl — mutually exclusive segments in a horizontal row.
//!
//! Each segment is a real composed widget — a centered icon + label with
//! a reactive tint — built from a [`Segment`] descriptor. The control
//! binds a `Signal<usize>` index: reading or writing the signal selects
//! the corresponding segment without rebuilding the tree. Per-segment
//! disabling, optional leading icons, and optional hover tooltips are all
//! first-class; the chrome (rounded frame, hover tint, selected-segment
//! surface) is delegated to the active [`SegmentedControlStyle`](bastyde_core::styles::SegmentedControlStyle).
//!
//! ## When to use
//!
//! - Use a `SegmentedControl` when there are 2–5 mutually exclusive modes
//!   that fit in a compact horizontal strip (e.g. view mode, time period).
//! - Prefer a `ComboBox` when there are more than five options or labels
//!   are long.
//! - Prefer `RadioButton` when the options need more vertical space or
//!   detailed descriptions.
//!
//! ## Accessibility
//!
//! `Role::RadioGroup` on the control, `Role::RadioButton` per segment.
//! Arrow keys cycle selection, skipping disabled segments; the entire
//! control is a single tab stop. `Increment`/`Decrement` AT actions
//! mirror arrow-key behavior for switch-access users.
//!
//! ```ignore
//! SegmentedControl::new(selected)
//!     .segment(Segment::new(tr!(list_view())).icon(|| IconWidget::list(14.0)))
//!     .segment(Segment::new(tr!(grid_view())).icon(|| IconWidget::grid(14.0)).tooltip(tr!(grid_hint())))
//!     .segment(Segment::new(tr!(columns())).disabled(true))
//! ```

use std::rc::Rc;

use bastyde_canvas::{Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::focus::FocusOrigin;
use bastyde_core::signal::Signal;
use bastyde_core::styles::{SegmentedControlStyleConfig, SharedSegmentedControlStyle};
use bastyde_core::widget::{CursorIcon, LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use bastyde_core::widget_builder::{HandlerSet, WidgetBuilder};
use bastyde_core::widget_id::WidgetId;
use bastyde_i18n::LocalizedString;
use bastyde_tokens::{TextRole, TextStyleRole};

use crate::primitives::{Center, HStack, IconWidget, TextWidget};
use crate::styles::recipe_segmented_control_style::{
    SEGMENTED_CONTROL_BORDER_WIDTH, SEGMENTED_CONTROL_HEIGHT, SEGMENTED_CONTROL_PADDING_HORIZONTAL,
    SEGMENTED_CONTROL_PADDING_VERTICAL,
};

/// Fallback character width when no text backend is available.
const FALLBACK_CHAR_WIDTH: f32 = 8.0;
const FALLBACK_LINE_HEIGHT: f32 = 16.0;
/// Gap between a segment's icon and its label.
const SEGMENT_ICON_LABEL_SPACING: f32 = 6.0;
/// Rough icon-plus-gap allowance used only for intrinsic-width estimation.
const SEGMENT_ICON_WIDTH_ESTIMATE: f32 = 18.0;

/// Factory that builds a segment's leading icon. `Rc` (not `Box`) so a
/// `Segment` descriptor can be cloned into a fresh cell on every rebuild
/// without consuming it.
type IconFactory = Rc<dyn Fn() -> IconWidget>;

/// One segment descriptor: a localized label with an optional leading
/// icon, hover tooltip, and disabled flag.
#[derive(Clone)]
pub struct Segment {
    label: LocalizedString,
    icon: Option<IconFactory>,
    tooltip: Option<LocalizedString>,
    disabled: bool,
}

impl Segment {
    /// A text segment. The label may come from `tr!(...)` (translated —
    /// follows a live locale switch) or `lit!(...)` (untranslated).
    pub fn new(label: impl Into<LocalizedString>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            tooltip: None,
            disabled: false,
        }
    }

    /// Add a leading icon. The factory is invoked at build time (and on
    /// rebuild); the icon's tint is bound reactively to the segment's
    /// selected / focus / enabled state so it matches the label.
    pub fn icon(mut self, factory: impl Fn() -> IconWidget + 'static) -> Self {
        self.icon = Some(Rc::new(factory));
        self
    }

    /// Hover tooltip — most useful for icon-only segments.
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    /// Disable this segment: not selectable via click or keyboard,
    /// dimmed, and announced disabled to assistive tech.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// Label-only convenience: `tr!(day())` / `lit!("Off")` flow straight
/// into `.segment(...)` / `.segments([...])` without `Segment::new`.
impl From<LocalizedString> for Segment {
    fn from(label: LocalizedString) -> Self {
        Segment::new(label)
    }
}

impl std::fmt::Debug for Segment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Segment")
            .field("label", &self.label)
            .field("has_icon", &self.icon.is_some())
            .field("disabled", &self.disabled)
            .finish()
    }
}

/// Internal composed widget for one segment: a centered icon + label
/// with reactive tint, owning the per-segment click / hover / tooltip /
/// a11y. Paints no chrome — the SegmentedControl's style leaf paints the
/// frame + selection / hover background + focus ring behind these cells.
struct SegmentCell {
    label: LocalizedString,
    icon: Option<IconFactory>,
    tooltip: Option<LocalizedString>,
    label_style: Option<bastyde_core::color_prop::TextStyleProp>,
    seg_disabled: bool,
    index: usize,
    selected: Signal<usize>,
    hovered_segment: Signal<Option<usize>>,
    focus_origin: Signal<Option<FocusOrigin>>,
    content_id: Option<WidgetId>,
}

impl std::fmt::Debug for SegmentCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SegmentCell")
            .field("index", &self.index)
            .field("disabled", &self.seg_disabled)
            .finish()
    }
}

impl Widget for SegmentCell {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        // Statically-disabled segments are disabled in the arena: clicks
        // and hovers are gated and the AT node is announced disabled
        // automatically by the framework walker.
        if self.seg_disabled {
            ctx.enabled_when(self_id, false);
        }
        let enabled = ctx.effective_enabled_signal(self_id);

        // Label / icon tint follows (selected, focus, enabled):
        //   !enabled            -> Disabled
        //   selected + focused  -> OnAccent (the chrome fills it accent)
        //   otherwise           -> Primary
        // `Signal<TextRole>` resolves against the live theme at paint, so
        // this is theme-reactive too.
        let idx = self.index;
        let color = self
            .selected
            .zip3(&self.focus_origin, &enabled)
            .map(move |(sel, foc, en)| {
                if !*en {
                    TextRole::Disabled
                } else if *sel == idx && foc.is_some() {
                    TextRole::OnAccent
                } else {
                    TextRole::Primary
                }
            });

        // Borrow (don't consume) the icon factory / tooltip so the cell
        // stays rebuild-safe.
        let mut row = HStack::new().spacing(SEGMENT_ICON_LABEL_SPACING);
        if let Some(icon_factory) = &self.icon {
            row = row.child(icon_factory().color(color.clone()));
        }
        let label_widget = match &self.label_style {
            Some(style) => TextWidget::new(self.label.clone()).style(style.clone()),
            None => TextWidget::new(self.label.clone()).style(TextStyleRole::Small),
        };
        row = row.child(label_widget.bind_color(color).single_line());

        // The cell node owns the AT RadioButton + name; exclude the inner
        // content subtree so a screen reader doesn't double-announce the
        // label (an `access_hidden` flag alone would not prune the
        // descendant `TextWidget`/icon nodes).
        let content_id = ctx.add(Center::new().child(row).access_exclude_subtree());
        self.content_id = Some(content_id);

        // Optional hover tooltip (icon-only segments especially).
        if let Some(tt) = &self.tooltip {
            let tip = ctx.add(crate::tooltip::TooltipWidget::new(tt.clone()));
            let delay = ctx.theme().motion.tooltip_delay;
            ctx.attach_tooltip(self_id, tip, delay);
        }

        // Click selects (arena gates disabled cells, so no per-cell guard
        // needed); hover drives the chrome's hover tint. Focus stays on
        // the parent SegmentedControl (single tab stop).
        let selected = self.selected.clone();
        let hovered = self.hovered_segment.clone();
        let handlers = HandlerSet::new()
            .cursor(CursorIcon::Pointer)
            .focusable(false)
            .on_tap(move |_pos, _ctx| {
                selected.set(idx);
            })
            .on_hover(move |entered, _ctx| {
                if entered {
                    hovered.set(Some(idx));
                } else if hovered.get() == Some(idx) {
                    hovered.set(None);
                }
            });
        ctx.apply_self_handlers(handlers);

        vec![content_id]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        // The parent SegmentedControl assigns exact bounds in
        // `place_children`; just claim the proposed envelope.
        Size::new(
            proposal.width.unwrap_or(0.0),
            proposal.height.unwrap_or(0.0),
        )
        .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        if let Some(c) = children.first_mut() {
            c.origin = bounds.origin();
            c.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::RadioButton);
        builder.set_name(self.label.resolve_now());
        builder.set_selected(self.selected.get() == self.index);
        // Framework a11y walker sets `set_disabled` from arena state.
        builder.add_action(bastyde_core::accesskit::Action::Click);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.content_id.into_iter().collect()
    }
}

/// A segmented control that binds a `Signal<usize>` index to a row of
/// mutually exclusive segments. Build the segment list with
/// [`segment`](Self::segment) or [`segments`](Self::segments).
pub struct SegmentedControl {
    /// Segment descriptors. Retained (cloned, not consumed, into cells on
    /// each build) so the control is rebuild-safe and so `layout_response`
    /// / `accessibility` can read labels even when measured while dormant.
    segments: Vec<Segment>,
    selected: Signal<usize>,
    /// Initial enabled-state; forwarded to the arena at build time.
    initial_enabled: bool,
    hovered_segment: Signal<Option<usize>>,
    /// Raw keyboard/pointer focus (any modality). The keyboard-only focus
    /// ring and the focus-driven selected-segment accent fill are derived
    /// live from this × the input-modality signal in `build()`
    /// (`:focus-visible`).
    focused: Signal<bool>,
    /// Per-call override for the chrome.
    style_override: Option<SharedSegmentedControlStyle>,
    /// Per-call override for every segment's label text style (font, size,
    /// weight). `None` ⇒ the default `TextStyleRole::Small`. Text *color*
    /// stays state-driven (selected → `OnAccent`, disabled → `Disabled`)
    /// and is intentionally not overridable.
    label_style: Option<bastyde_core::color_prop::TextStyleProp>,
    /// Build-time children — chrome first (back), then one
    /// `SegmentCell` per segment.
    children: Vec<WidgetId>,
}

impl SegmentedControl {
    /// Create an empty segmented control bound to `selected`. Add segments
    /// with [`segment`](Self::segment) or [`segments`](Self::segments).
    pub fn new(selected: Signal<usize>) -> Self {
        Self {
            segments: Vec::new(),
            selected,
            initial_enabled: true,
            hovered_segment: Signal::new(None),
            focused: Signal::new(false),
            style_override: None,
            label_style: None,
            children: Vec::new(),
        }
    }

    /// Append one segment. Accepts a [`Segment`] or, via
    /// `From<LocalizedString>`, a bare `tr!(...)` / `lit!(...)` label.
    pub fn segment(mut self, segment: impl Into<Segment>) -> Self {
        self.segments.push(segment.into());
        self
    }

    /// Append several segments. Label-only:
    /// `.segments([tr!(day()), tr!(week())])`; rich:
    /// `.segments([Segment::new(...).icon(...), ...])`.
    pub fn segments(mut self, segments: impl IntoIterator<Item = impl Into<Segment>>) -> Self {
        self.segments.extend(segments.into_iter().map(Into::into));
        self
    }

    /// Set the initial enabled state. Forwarded to the arena at build
    /// time. For reactive enable/disable use
    /// `ctx.enabled_when(segmented_control_id, signal)`.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.initial_enabled = enabled;
        self
    }

    /// Per-call override for the segmented-control chrome.
    pub fn style(mut self, style: impl bastyde_core::styles::SegmentedControlStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Override every segment's label text style (font, size, weight).
    /// Accepts a `TextStyleRole`, a `TextStyle`, or a `Signal` of either.
    /// Default (unset) is `TextStyleRole::Small`. Text color stays
    /// state-driven and is intentionally not overridable here.
    pub fn text_style(mut self, style: impl Into<bastyde_core::color_prop::TextStyleProp>) -> Self {
        self.label_style = Some(style.into());
        self
    }

    /// Estimate the intrinsic width of all segments — used in
    /// `layout_response` when no width is proposed.
    fn estimate_width(&self) -> f32 {
        let n = self.segments.len();
        if n == 0 {
            return 0.0;
        }
        let max_content = self
            .segments
            .iter()
            .map(|s| {
                let label_w = s.label.resolve_now().chars().count() as f32 * FALLBACK_CHAR_WIDTH;
                let icon_w = if s.icon.is_some() {
                    SEGMENT_ICON_WIDTH_ESTIMATE + SEGMENT_ICON_LABEL_SPACING
                } else {
                    0.0
                };
                label_w + icon_w
            })
            .fold(0.0_f32, f32::max);
        (max_content + SEGMENTED_CONTROL_PADDING_HORIZONTAL * 2.0) * n as f32
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

    /// Next selectable index in `dir` (true = forward), wrapping and
    /// skipping disabled segments. Returns `current` if no other
    /// segment is enabled.
    fn step_selection(current: usize, forward: bool, disabled: &[bool]) -> usize {
        let n = disabled.len();
        if n == 0 {
            return current;
        }
        let mut i = current;
        for _ in 0..n {
            i = if forward {
                (i + 1) % n
            } else {
                (i + n - 1) % n
            };
            if !disabled[i] {
                return i;
            }
        }
        current
    }
}

impl std::fmt::Debug for SegmentedControl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SegmentedControl")
            .field("segments", &self.segments.len())
            .field("initial_enabled", &self.initial_enabled)
            .finish()
    }
}

impl Widget for SegmentedControl {
    fn build(
        &mut self,
        ctx: &mut bastyde_core::build_context::BuildContext,
    ) -> Vec<bastyde_core::widget_id::WidgetId> {
        let self_id = ctx.self_id();
        // Forward initial-enabled to the arena; see IconButton.
        if !self.initial_enabled {
            ctx.enabled_when(self_id, false);
        }
        let effective_enabled = ctx.effective_enabled_signal(self_id);

        let registry = ctx.binding_registry();
        self.selected.bind_to(
            self_id,
            registry,
            bastyde_core::binding::BindingLevel::RepaintOnly,
        );

        let selected = self.selected.clone();
        let hovered_segment = self.hovered_segment.clone();
        // `:focus-visible`: derive the keyboard/pointer origin live from the
        // input-modality signal (true after a key event, false after
        // pointer-down) rather than snapshotting hover at focus time. The
        // chrome reads `Some(_)` for the selected-segment accent fill (any
        // focus) and `Some(Keyboard)` for the focus ring, so this keeps the
        // fill on a click while making the ring keyboard-only.
        let focused = self.focused.clone();
        let focus_origin = self.focused.zip(&ctx.focus_visible()).map(|(f, v)| {
            if !*f {
                None
            } else if *v {
                Some(FocusOrigin::Keyboard)
            } else {
                Some(FocusOrigin::Pointer)
            }
        });

        let n = self.segments.len();
        let disabled_flags: Rc<Vec<bool>> =
            Rc::new(self.segments.iter().map(|s| s.disabled).collect());

        // Build chrome leaf first (so it sits at index 0 in `children`
        // and paints behind the segment cells).
        let style: SharedSegmentedControlStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.segmented_control.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeSegmentedControlStyle::default()));
        let chrome_id = style.make_body(
            &SegmentedControlStyleConfig {
                segment_count: n,
                selected: selected.clone(),
                hovered_segment: hovered_segment.clone(),
                focus_origin: focus_origin.clone(),
                is_enabled: effective_enabled.clone(),
            },
            ctx,
        );

        // Build one composed `SegmentCell` per descriptor, grid-placed
        // over the chrome. Descriptors are cloned (not drained) so a
        // rebuild reproduces every cell. Each cell disables itself in the
        // arena when its segment is disabled.
        self.children.clear();
        self.children.push(chrome_id);
        for index in 0..n {
            let id = ctx.add(SegmentCell {
                label: self.segments[index].label.clone(),
                icon: self.segments[index].icon.clone(),
                tooltip: self.segments[index].tooltip.clone(),
                label_style: self.label_style.clone(),
                seg_disabled: self.segments[index].disabled,
                index,
                selected: selected.clone(),
                hovered_segment: hovered_segment.clone(),
                focus_origin: focus_origin.clone(),
                content_id: None,
            });
            self.children.push(id);
        }

        // Framework gates events on `arena.is_enabled`; focus walker
        // skips disabled subtrees.
        let mut handlers = HandlerSet::new()
            .focusable(true)
            .cursor(CursorIcon::Pointer);

        // Hover-out on the parent clears the segment highlight when the
        // pointer leaves the control entirely.
        {
            let hovered_segment = hovered_segment.clone();
            handlers = handlers.on_hover(move |entered, _ctx| {
                if !entered {
                    hovered_segment.set(None);
                }
            });
        }

        // Arrow keys cycle selection, skipping disabled segments. Focus
        // stays on the parent.
        {
            let selected = selected.clone();
            let disabled = disabled_flags.clone();
            handlers = handlers.on_key(move |event, _ctx| {
                if n == 0 {
                    return EventResponse::Ignored;
                }
                match event {
                    WidgetEvent::KeyDown {
                        key: Key::ArrowRight,
                        ..
                    } => {
                        selected.set(Self::step_selection(selected.get(), true, &disabled));
                        EventResponse::Handled
                    }
                    WidgetEvent::KeyDown {
                        key: Key::ArrowLeft,
                        ..
                    } => {
                        selected.set(Self::step_selection(selected.get(), false, &disabled));
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }
            });
        }

        // Focus handler. Track raw focus only; the keyboard/pointer
        // distinction (for the ring and the selected-segment accent fill) is
        // derived live from the input-modality signal in `build()`
        // (`:focus-visible`), so clicking to focus then pressing a key
        // reveals the ring.
        {
            let focused = focused.clone();
            handlers = handlers.on_focus(move |gained, _ctx| {
                focused.set(gained);
            });
        }

        // Access actions — increment/decrement cycle selection (skipping
        // disabled segments).
        {
            let selected = selected.clone();
            let disabled = disabled_flags.clone();
            handlers = handlers.on_access_action(move |action, _ctx| {
                if n == 0 {
                    return EventResponse::Ignored;
                }
                if action == bastyde_core::accesskit::Action::Increment {
                    selected.set(Self::step_selection(selected.get(), true, &disabled));
                    EventResponse::Handled
                } else if action == bastyde_core::accesskit::Action::Decrement {
                    selected.set(Self::step_selection(selected.get(), false, &disabled));
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
        // children[0] is the chrome leaf — fills the full control bounds
        // so it paints frame / selection / hover / focus-ring envelope.
        // children[1..] are the SegmentCells, grid-placed in the inner
        // (visual minus border) rect.
        if children.is_empty() {
            return;
        }
        children[0].origin = bounds.origin();
        children[0].size = bounds.size();

        let n = self.segments.len();
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
            placement.origin = Point::new(inner.x + i as f32 * seg_w, inner.y);
            placement.size = Size::new(seg_w, inner.height);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::RadioGroup);
        let selected_index = self.selected.get();
        if let Some(seg) = self.segments.get(selected_index) {
            builder.set_value(seg.label.resolve_now());
        }
        // Framework a11y walker sets `set_disabled` from arena state.
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
    use bastyde_i18n::lit;

    fn abc(selected: Signal<usize>) -> SegmentedControl {
        SegmentedControl::new(selected).segments([lit!("A"), lit!("B"), lit!("C")])
    }

    #[test]
    fn click_selects_segment_by_position() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let sc = tree.add(abc(selected.clone()));
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
        let sc = tree.add(abc(selected.clone()));
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
        let sc = tree.add(abc(selected.clone()));
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
        let sc = tree.add(abc(selected.clone()));
        tree.layout(SizeProposal::exact(300.0, 60.0));

        tree.focus(sc);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(selected.get(), 0, "should wrap from last to first");
        tree.press_key(Key::ArrowLeft, Modifiers::NONE);
        assert_eq!(selected.get(), 2, "should wrap from first to last");
    }

    #[test]
    fn keyboard_skips_disabled_segments() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let sc = tree.add(SegmentedControl::new(selected.clone()).segments([
            Segment::new(lit!("A")),
            Segment::new(lit!("B")).disabled(true),
            Segment::new(lit!("C")),
        ]));
        tree.layout(SizeProposal::exact(300.0, 60.0));
        tree.focus(sc);
        // 0 -> (skip disabled 1) -> 2
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(
            selected.get(),
            2,
            "ArrowRight should skip the disabled middle segment"
        );
        // 2 -> wrap -> 0
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(selected.get(), 0);
    }

    #[test]
    fn rebuild_preserves_segments() {
        // Regression guard: segments are cloned (not drained) into cells,
        // so a rebuild must reproduce every cell.
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let sc = tree.add(abc(selected.clone()));
        tree.layout(SizeProposal::exact(300.0, 60.0));
        assert_eq!(tree.children(sc).len(), 4, "chrome + 3 segments");
        // Force a rebuild of the composite and re-layout.
        tree.arena_mark_needs_rebuild_for_testing(sc);
        tree.layout(SizeProposal::exact(300.0, 60.0));
        assert_eq!(
            tree.children(sc).len(),
            4,
            "rebuild must reproduce chrome + 3 segments, not drop them"
        );
    }

    #[test]
    fn accessibility() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let sc = tree.add(SegmentedControl::new(selected).segments([lit!("A"), lit!("B")]));
        tree.layout(SizeProposal::exact(300.0, 60.0));
        let info = tree.accessibility_node(sc);
        assert_eq!(info.role(), bastyde_core::accesskit::Role::RadioGroup);
    }

    #[test]
    fn paints_selected_segment_with_accent_when_focused() {
        let selected = Signal::new(1_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let sc = tree.add(abc(selected));
        tree.layout(SizeProposal::exact(300.0, 60.0));
        tree.focus(sc);
        let frame = tree.render();
        let accent = bastyde_core::presets::intui::light()
            .colors
            .accent
            .to_array();
        assert!(
            frame.shapes.iter().any(|s| s.color == accent),
            "focused selected segment should render with accent color"
        );
    }

    #[test]
    fn unfocused_selected_segment_uses_inactive_surface() {
        let selected = Signal::new(1_usize);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(abc(selected));
        tree.layout(SizeProposal::exact(300.0, 60.0));
        let frame = tree.render();
        let accent = bastyde_core::presets::intui::light()
            .colors
            .accent
            .to_array();
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
        let sc = tree.add(SegmentedControl::new(selected).segments([lit!("A"), lit!("B")]));
        tree.layout(SizeProposal::exact(300.0, 60.0));
        let info = tree.accessibility_node(sc);
        assert!(
            info.actions()
                .contains(&bastyde_core::accesskit::Action::Increment)
        );
    }
}
