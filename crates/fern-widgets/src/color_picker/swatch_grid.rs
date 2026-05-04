//! `SwatchGrid` — Role::Grid container of color swatches with arrow-key
//! roving focus, mirroring the Calendar widget's grid pattern.
//!
//! Lays out its children in fixed-column rows using the existing
//! [`Grid`](crate::primitives::Grid) primitive. Click / Enter / Space
//! on any cell calls `on_select(color, ctx)`. Tab moves focus into the
//! first cell; arrow keys move within the grid (Left/Right by 1,
//! Up/Down by `columns`, Home/End to row bounds, Ctrl+Home/End to grid
//! bounds); Tab again leaves the grid.

use std::rc::Rc;

use fern_canvas::{Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::accesskit::Role;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::widget::{
    EventContext, LayoutContext, LayoutResponse, Widget, WidgetPlacement,
};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_i18n::resolve_message_widget;
use fern_tokens::Color;

use super::swatch::ColorSwatch;
use crate::primitives::{Grid, TrackSize};

pub(crate) struct SwatchGrid {
    swatches: Signal<Vec<Color>>,
    selected: Signal<Color>,
    columns: usize,
    on_select: Rc<dyn Fn(Color, &mut EventContext)>,
    /// Currently focused cell index inside the grid. Used for the
    /// roving-focus pattern (only one cell takes focus; arrow keys
    /// move between cells).
    focused_index: Signal<usize>,
    enabled: bool,
    root_child_id: Option<WidgetId>,
}

impl SwatchGrid {
    pub(crate) fn new(
        swatches: Signal<Vec<Color>>,
        selected: Signal<Color>,
        columns: usize,
        on_select: Rc<dyn Fn(Color, &mut EventContext)>,
    ) -> Self {
        Self {
            swatches,
            selected,
            columns: columns.max(1),
            on_select,
            focused_index: Signal::new(0),
            enabled: true,
            root_child_id: None,
        }
    }

    pub(crate) fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

impl std::fmt::Debug for SwatchGrid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SwatchGrid")
            .field("columns", &self.columns)
            .finish_non_exhaustive()
    }
}

impl Widget for SwatchGrid {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        // Re-layout when the swatches list changes; repaint when
        // selection moves (children re-render with the new ring).
        self.swatches.bind_to(self_id, registry, fern_core::binding::BindingLevel::Relayout);
        self.selected.bind_to(self_id, registry, fern_core::binding::BindingLevel::RepaintOnly);

        let swatches = self.swatches.get();
        let columns = self.columns;
        let style = ctx.theme_signal().get().components.color_picker;

        // Build one ColorSwatch per color.
        let on_select = self.on_select.clone();
        let selected = self.selected.clone();
        let mut grid = Grid::new()
            .columns(vec![TrackSize::Auto; columns])
            .row_gap(style.swatch_spacing)
            .column_gap(style.swatch_spacing);
        for color in &swatches {
            let color = *color;
            let on_select = on_select.clone();
            let is_selected = selected.get() == color;
            let cell = ColorSwatch::new(color)
                .selected(is_selected)
                .enabled(self.enabled)
                .on_activate_fn(move |ctx_evt| {
                    (on_select)(color, ctx_evt);
                });
            grid = grid.child(cell);
        }
        let root = ctx.add(grid);
        self.root_child_id = Some(root);

        // Self handlers — Tab brings focus in, arrow keys move
        // focused_index across the grid. Roving focus pattern.
        let count = swatches.len();
        let columns_for_keys = self.columns;
        let focused_index = self.focused_index.clone();
        let enabled = self.enabled;
        let on_select_keys = self.on_select.clone();
        let swatches_for_keys = self.swatches.clone();
        let handlers = HandlerSet::new()
            .focusable(enabled && count > 0)
            .on_key(move |event, ctx_evt| {
                if !enabled || count == 0 {
                    return EventResponse::Ignored;
                }
                let WidgetEvent::KeyDown { key, modifiers, .. } = event else {
                    return EventResponse::Ignored;
                };
                let mut idx = focused_index.get().min(count.saturating_sub(1));
                let last = count.saturating_sub(1);
                let row = idx / columns_for_keys;
                let col = idx % columns_for_keys;
                let row_start = row * columns_for_keys;
                let row_end = ((row + 1) * columns_for_keys - 1).min(last);
                match key {
                    Key::ArrowLeft => {
                        if idx > 0 {
                            idx -= 1;
                        }
                    }
                    Key::ArrowRight => {
                        if idx < last {
                            idx += 1;
                        }
                    }
                    Key::ArrowUp => {
                        if idx >= columns_for_keys {
                            idx -= columns_for_keys;
                        }
                    }
                    Key::ArrowDown => {
                        if idx + columns_for_keys <= last {
                            idx += columns_for_keys;
                        }
                    }
                    Key::Home => {
                        idx = if modifiers.ctrl() { 0 } else { row_start };
                    }
                    Key::End => {
                        idx = if modifiers.ctrl() { last } else { row_end };
                    }
                    Key::Enter | Key::Space => {
                        let list = swatches_for_keys.get();
                        if let Some(c) = list.get(idx) {
                            (on_select_keys)(*c, ctx_evt);
                        }
                        return EventResponse::Handled;
                    }
                    _ => return EventResponse::Ignored,
                }
                let _ = (row, col);
                focused_index.set(idx);
                EventResponse::Handled
            });
        ctx.apply_self_handlers(handlers);

        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        match self.root_child_id {
            Some(id) => ctx
                .child_layout_response(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0).into()),
            None => proposal.resolve(0.0, 0.0).into(),
        }
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

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(Role::Grid);
        builder.set_name(resolve_message_widget("color-picker-swatches-name", &[]));
        if !self.enabled {
            builder.set_disabled();
        }
    }
}
