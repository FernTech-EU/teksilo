//! Navigator pane (left).
//!
//! Lists registered widget catalog entries grouped by `entry.group()`,
//! each expandable to its variants. Clicking a variant updates the
//! shared [`AppState`] selection signals; the canvas rebuilds.
//!
//! The list is implemented as a flat scrollable VStack rather than as
//! a `TreeView`. `TreeView` requires a `TreeModel<T>` of homogeneous
//! `T`; mapping the registry — which is heterogeneous (groups vs
//! widgets vs variants) — into a single `T` would either lose type
//! information or push complexity into the delegate. A flat scroller
//! with hand-authored rows is simpler and gives full control over
//! styling.

use bastyde_core::build_context::BuildContext;
use bastyde_core::widget::CursorIcon;
use bastyde_core::widget_builder::WidgetBuilder;
use bastyde_core::widget_id::WidgetId;
use bastyde_i18n::lit;
use bastyde_tokens::{BorderRole, SurfaceRole, TextRole, TextStyleRole};
use bastyde_widgets::primitives::{Padding, ZStack};
use bastyde_widgets::{
    Expand, HStack, MaxSize, MinSize, RectWidget, ScrollArea, TextWidget, VStack,
};

use crate::app_state::AppState;

pub fn build_navigator(ctx: &mut BuildContext, state: &AppState) -> WidgetId {
    let header = build_header(ctx, state);
    let list = build_list(ctx, state);
    let scroll = ScrollArea::from_id(list);
    let scroll_id = ctx.add(scroll);
    // The list scroll area must fill the remaining height below the
    // header — otherwise the VStack collapses to header height.
    let scroll_expanded = ctx.add(Expand::vertical().child_id(scroll_id));

    let column = VStack::new().add_child(header).add_child(scroll_expanded);
    let bg = RectWidget::new()
        .background(SurfaceRole::Sunken)
        .border_color(BorderRole::Default)
        .border_width(1.0);
    let bg_id = ctx.add(bg);
    let column_id = ctx.add(column);
    let stack = ZStack::new().add_child(bg_id).add_child(column_id);
    ctx.add(stack)
}

fn build_header(ctx: &mut BuildContext, state: &AppState) -> WidgetId {
    let title = TextWidget::new(lit!("Catalog"))
        .style(TextStyleRole::SmallBold)
        .color(TextRole::Primary);
    let count = TextWidget::new(lit!(""))
        .style(TextStyleRole::Tiny)
        .color(TextRole::Secondary)
        .single_line()
        .bind_text(state.navigator_filter.clone().map({
            move |q| {
                let total = bastyde_preview::iter_entries().count();
                if q.is_empty() {
                    format!("{} widget{}", total, if total == 1 { "" } else { "s" })
                } else {
                    let n = filter_count(q);
                    format!("{}/{}", n, total)
                }
            }
        }));
    let row = HStack::new()
        .spacing(8.0)
        .child(title)
        .child(bastyde_widgets::Spacer::new())
        .child(count);
    let header = Padding::symmetric(8.0, 12.0).child(row);
    ctx.add(header)
}

fn filter_count(q: &str) -> usize {
    let q = q.to_lowercase();
    bastyde_preview::iter_entries()
        .filter(|e| {
            e.id().to_lowercase().contains(&q)
                || e.display_name().to_lowercase().contains(&q)
                || e.variants()
                    .iter()
                    .any(|v| v.name().to_lowercase().contains(&q))
        })
        .count()
}

fn build_list(ctx: &mut BuildContext, state: &AppState) -> WidgetId {
    let mut column = VStack::new().spacing(4.0);
    let entries: Vec<_> = bastyde_preview::entries_by_group();
    for (group_name, group_entries) in entries {
        let group_label = TextWidget::new(lit!(group_name))
            .style(TextStyleRole::Tiny)
            .color(TextRole::Secondary);
        let group_padding = Padding::symmetric(4.0, 12.0).child(group_label);
        column = column.child(group_padding);

        for entry in group_entries {
            let row = build_entry_row(ctx, state, entry);
            column = column.add_child(row);
        }
    }
    ctx.add(column)
}

fn build_entry_row(
    ctx: &mut BuildContext,
    state: &AppState,
    entry: &'static dyn bastyde_preview::CatalogEntry,
) -> WidgetId {
    let display_name = entry.display_name();
    let widget_id = entry.id();

    let widget_label = TextWidget::new(lit!(display_name))
        .style(TextStyleRole::Body)
        .color(TextRole::Primary)
        .single_line();
    let header_row = Padding::new(4.0, 12.0, 4.0, 16.0).child(widget_label);

    // Highlight when this widget is the selected one.
    let widget_id_sig = state.selected_widget.clone();
    let bg_role = widget_id_sig.map(move |sel| {
        if *sel == Some(widget_id) {
            SurfaceRole::Selected
        } else {
            SurfaceRole::Transparent
        }
    });
    let bg = RectWidget::new().bind_background(bg_role);
    let bg_id = ctx.add(bg);
    let header_id = ctx.add(header_row);

    let stack = ZStack::new().add_child(bg_id).add_child(header_id);
    let stack_w = MinSize::new(0.0, 28.0).child(stack);
    let stack_id = ctx.add(stack_w);

    // Click to select this widget.
    let st = state.clone();
    let row_clickable = MaxSize::new(f32::INFINITY, f32::INFINITY)
        .child_id(stack_id)
        .on_tap(move |_pos, _ctx| {
            st.select_widget(widget_id);
        })
        .focusable(true)
        .cursor(CursorIcon::Pointer);
    let row_id = ctx.add(row_clickable);

    // Variants underneath.
    let mut variants_col = VStack::new().spacing(0.0);
    for variant in entry.variants() {
        let variant_name = variant.name();
        let variant_row = build_variant_row(ctx, state, widget_id, variant_name);
        variants_col = variants_col.add_child(variant_row);
    }
    let variants_id = ctx.add(variants_col);

    let combined = VStack::new().add_child(row_id).add_child(variants_id);
    ctx.add(combined)
}

fn build_variant_row(
    ctx: &mut BuildContext,
    state: &AppState,
    widget_id: &'static str,
    variant_name: &'static str,
) -> WidgetId {
    let variant_label = TextWidget::new(lit!(variant_name))
        .style(TextStyleRole::Small)
        .color(TextRole::Secondary)
        .single_line();
    let row_padding = Padding::new(2.0, 12.0, 2.0, 28.0).child(variant_label);

    let widget_sig = state.selected_widget.clone();
    let variant_sig = state.selected_variant.clone();
    let bg_role = widget_sig.zip(&variant_sig).map(move |t| {
        let (w, v) = *t;
        if w == Some(widget_id) && v == Some(variant_name) {
            SurfaceRole::Selected
        } else {
            SurfaceRole::Transparent
        }
    });
    let bg = RectWidget::new().bind_background(bg_role);
    let bg_id = ctx.add(bg);
    let label_id = ctx.add(row_padding);

    let stack = ZStack::new().add_child(bg_id).add_child(label_id);
    let stack_widget = MinSize::new(0.0, 22.0).child(stack);
    let stack_id = ctx.add(stack_widget);

    let st = state.clone();
    let clickable = MaxSize::new(f32::INFINITY, f32::INFINITY)
        .child_id(stack_id)
        .on_tap(move |_pos, _ctx| {
            st.select_widget(widget_id);
            st.select_variant(variant_name);
        })
        .focusable(true)
        .cursor(CursorIcon::Pointer);
    ctx.add(clickable)
}
