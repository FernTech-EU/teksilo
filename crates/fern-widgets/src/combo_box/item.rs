//! `DropdownItem` — the row widget for a single entry in the dropdown
//! panel, plus the `build_default_item` helper used when the caller
//! hasn't supplied a custom `render_item` closure.

use std::rc::Rc;

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::signal::Signal;
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, SurfaceRole, TextStyleRole};

use crate::primitives::{HStack, Padding, RectWidget, Spacer, TextWidget, ZStack};

/// Add the default label-plus-padding subtree into the arena and return
/// its root id. Used when `render_item` is not provided.
///
/// The label is wrapped in an `HStack` with a trailing `Spacer` so the
/// inner content stretches to the full item width — without that
/// stretch, the `ZStack` in `DropdownItem` (which defaults to
/// `Alignment::CENTER`) would center the narrow text inside the wide
/// row, producing visibly centered labels instead of left-aligned
/// ones. This mirrors the pattern used by `MenuItem`'s row.
pub(super) fn build_default_item(
    ctx: &mut BuildContext,
    label: &str,
    theme: &fern_tokens::Theme,
) -> WidgetId {
    let text = TextWidget::new_literal(label)
        .style(TextStyleRole::Body)
        .bind_color(ctx.theme_signal().map(|t| t.colors.text_primary))
        .single_line()
        .a11y_hidden();
    let text_id = ctx.add(text);

    // HStack { label | Spacer } fills the available width, which forces
    // the enclosing `Padding` to stretch to its full proposal rather
    // than shrinking to the label's intrinsic width.
    let row = HStack::new()
        .spacing(0.0)
        .add_child(text_id)
        .child(Spacer::new());
    let row_id = ctx.add(row);

    let menu_style = theme.components.menu;
    let pad_v =
        ((menu_style.item_height - theme.typography.body.size).max(0.0) * 0.5).max(0.0);
    let padding =
        Padding::symmetric(pad_v, menu_style.item_padding_horizontal).child_id(row_id);
    ctx.add(padding)
}

/// A single row in the dropdown. Wraps the user-rendered (or default)
/// subtree with the `Role::ListBoxOption` accessibility role, a
/// tap-to-select handler, and a selection-driven highlight background.
pub(super) struct DropdownItem<T: Clone + PartialEq + 'static> {
    pub(super) value: T,
    pub(super) label: String,
    /// 1-based index for `position_in_set`.
    pub(super) position: usize,
    pub(super) total: usize,
    pub(super) selected_signal: Signal<Option<T>>,
    pub(super) render: Option<Rc<dyn Fn(&T, bool) -> Box<dyn Widget>>>,
    pub(super) root_child_id: Option<WidgetId>,
}

impl<T: Clone + PartialEq + 'static> std::fmt::Debug for DropdownItem<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DropdownItem")
            .field("label", &self.label)
            .field("position", &self.position)
            .finish()
    }
}

impl<T: Clone + PartialEq + 'static> Widget for DropdownItem<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme_signal = ctx.theme_signal();
        let theme = theme_signal.get();
        let selected_signal = self.selected_signal.clone();
        let value_for_tap = self.value.clone();

        // Track whether this item is highlighted (hovered or selected).
        let highlighted = ctx.signal(false);

        // Sync highlight with the currently-selected value.
        {
            let highlighted = highlighted.clone();
            let value = self.value.clone();
            ctx.effect(&self.selected_signal, move |sel| {
                highlighted.set(sel.as_ref() == Some(&value));
            });
        }

        // Highlight uses the accent-subtle background token (designed for
        // this exact "hinted selection" purpose); falls back to transparent
        // when not highlighted. Role-based so paint resolves against theme.
        let bg_role = highlighted.map(|h| {
            if *h {
                SurfaceRole::AccentSubtle
            } else {
                SurfaceRole::Transparent
            }
        });

        // Build the inner content — either the user's render_item or the
        // default label row. The default path adds widgets directly via
        // `ctx.add` so every child is in the arena at layout time.
        let is_currently_selected = self.selected_signal.get().as_ref() == Some(&self.value);
        let inner_id = match &self.render {
            Some(r) => {
                let widget = (r)(&self.value, is_currently_selected);
                ctx.add_boxed(widget)
            }
            None => build_default_item(ctx, &self.label, &theme),
        };

        let bg = RectWidget::new().background(bg_role);
        let bg_id = ctx.add(bg);

        let zstack = ZStack::new().add_child(bg_id).add_child(inner_id);
        let root_id = ctx.add(zstack);
        self.root_child_id = Some(root_id);

        let handler_set = HandlerSet::new()
            .on_tap(move |_pos, ctx: &mut EventContext| {
                selected_signal.set(Some(value_for_tap.clone()));
                ctx.dismiss_all_overlays();
            })
            .on_hover({
                let highlighted = highlighted.clone();
                move |entered: bool, _ctx: &mut EventContext| {
                    highlighted.set(entered);
                }
            })
            .cursor(CursorIcon::Pointer);

        ctx.apply_self_handlers(handler_set);

        vec![root_id]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        let min_h = ctx.theme.components.menu.item_height;
        // Forward the width proposal so each row stretches the full panel
        // width instead of collapsing to its text's intrinsic width —
        // ZStack::size_that_fits queries children with `unspecified`,
        // stripping the proposed width, so we can't just delegate to the
        // root ZStack. Same pattern as `menu_list::KeyboardHighlightWrapper`.
        let child_size = self
            .root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, min_h));
        let width = proposal.width.unwrap_or(child_size.width.max(120.0));
        let height = child_size.height.max(min_h);
        Size::new(width, height)
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
        builder.set_role(fern_core::accesskit::Role::ListBoxOption);
        builder.set_name(&self.label);
        // A11y gap #1: announce selection state so screen readers can
        // say "selected, Apple" instead of just "Apple".
        let is_selected = self.selected_signal.get().as_ref() == Some(&self.value);
        builder.set_selected(is_selected);
        builder.inner_mut().set_position_in_set(self.position);
        builder.inner_mut().set_size_of_set(self.total);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}
