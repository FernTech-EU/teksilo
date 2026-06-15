// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Properties tab — key/value rows for the selected widget plus a
//! Copy button that writes the formatted dump to the clipboard.
//!
//! Right-click on a row opens a context menu with `Copy value` that
//! copies just that row's value. Wired through the framework's
//! `.context_menu(|pos, ctx| …)` builder: the closure uses `pos.y` to
//! identify the row, builds a fresh menu with the row's value
//! captured directly into the Copy item's activate closure, and
//! returns `Some(menu)`. Returning `None` (e.g. when the click missed
//! the row strip) falls through to the parent factory.

use bastyde_i18n::lit;
use std::cell::RefCell;
use std::rc::Rc;

use bastyde_canvas::{Canvas, Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::arena::WidgetArena;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_platform::ClipboardHandle;
use bastyde_tokens::TextRole;
use bastyde_widgets::primitives::{HStack, Padding, VStack};
use bastyde_widgets::{Button, MenuItem, MenuList};

use crate::state::InspectorState;
use crate::tabs::{ROW_HEIGHT, ROW_PADDING_X, last_segment};

const KEY_COLUMN_WIDTH: f32 = 140.0;
/// Single-line cap for the Debug repr row's *displayed* value. The
/// full repr always lands in the clipboard dump.
const DEBUG_REPR_DISPLAY_CAP: usize = 200;

#[derive(Clone, Debug)]
struct KvRow {
    key: String,
    value: String,
}

pub(crate) struct PropertiesTab {
    state: InspectorState,
    root_child_id: Option<WidgetId>,
}

impl PropertiesTab {
    pub fn new(state: InspectorState) -> Self {
        Self {
            state,
            root_child_id: None,
        }
    }
}

impl std::fmt::Debug for PropertiesTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PropertiesTab").finish()
    }
}

impl Widget for PropertiesTab {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let dump = self.state.properties_dump.clone();
        let copy_button = Button::new(lit!("Copy")).on_activate_fn(move |ctx| {
            if let Some(cb) = ctx.app_state::<ClipboardHandle>() {
                let _ = cb.set_text(&dump.get());
            }
        });
        let toolbar =
            Padding::symmetric(2.0, 4.0).child(HStack::new().spacing(6.0).child(copy_button));
        let rows = PropertiesRows::new(self.state.clone());
        let root = ctx.add(VStack::new().spacing(2.0).child(toolbar).child(rows));
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .map(LayoutResponse::from)
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0).into())
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for c in children.iter_mut() {
            c.origin = bastyde_canvas::Point::new(bounds.x, bounds.y);
            c.size = bastyde_canvas::Size::new(bounds.width, bounds.height);
        }
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}

struct PropertiesRows {
    state: InspectorState,
    /// Shared rows snapshot — RefCell inside Rc so the right-click
    /// `.context_menu(...)` closure (captures by move) and the
    /// `paint` / `layout_response` methods (read via `&self`) can
    /// share the same data without ownership gymnastics.
    rows: Rc<RefCell<Vec<KvRow>>>,
}

impl PropertiesRows {
    fn new(state: InspectorState) -> Self {
        Self {
            state,
            rows: Rc::new(RefCell::new(Vec::new())),
        }
    }
}

impl std::fmt::Debug for PropertiesRows {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PropertiesRows").finish()
    }
}

impl Widget for PropertiesRows {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        // Re-layout (and therefore re-snapshot + refresh dump) on
        // selection change.
        self.state
            .selected_id
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Relayout);

        // Right-click on a row → fresh menu carrying that row's value.
        // The factory uses `pos.y` to pick the row (rows are uniform
        // ROW_HEIGHT). Returning `None` when the click misses the row
        // strip lets the framework fall through to a parent factory
        // (e.g. an outer panel's debug menu, if one is ever wired).
        let rows_for_factory = self.rows.clone();
        let key_sig = self.state.properties_context_key.clone();
        let value_sig = self.state.properties_context_value.clone();
        let handlers = HandlerSet::new().context_menu(move |position, _ctx| {
            let idx = (position.y / ROW_HEIGHT).floor() as usize;
            let row = {
                let rows = rows_for_factory.borrow();
                rows.get(idx).cloned()?
            };
            // Stash the row context for the toolbar / Copy-button
            // path, which still reads these signals.
            key_sig.set(row.key.clone());
            value_sig.set(row.value.clone());
            // Capture the row's value directly into the Copy item's
            // activate closure — no need to thread through Signals
            // for menu actions.
            let value = row.value.clone();
            let copy_item = MenuItem::new(lit!("Copy value")).on_activate_fn(move |c| {
                if let Some(cb) = c.app_state::<ClipboardHandle>() {
                    let _ = cb.set_text(&value);
                }
            });
            Some(Box::new(MenuList::new().item(copy_item)) as Box<dyn Widget>)
        });
        ctx.apply_self_handlers(handlers);
        let _ = self_id;
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let mut rows: Vec<KvRow> = Vec::new();
        let mut full_debug = String::new();
        if let (Some(arena), Some(id)) = (ctx.arena(), self.state.selected_id.get()) {
            collect_properties(arena, id, &mut rows, &mut full_debug);
        }

        // Refresh the clipboard-bound dump so the toolbar's Copy
        // button has up-to-date text. Includes the full multi-line
        // Debug repr (untruncated) at the bottom.
        let mut dump = String::new();
        for row in &rows {
            dump.push_str(&row.key);
            dump.push_str(":\t");
            dump.push_str(&row.value);
            dump.push('\n');
        }
        if !full_debug.is_empty() {
            dump.push_str("\ndebug_repr:\n");
            dump.push_str(&full_debug);
            if !full_debug.ends_with('\n') {
                dump.push('\n');
            }
        }
        if self.state.properties_dump.get() != dump {
            self.state.properties_dump.set(dump);
        }

        let height = rows.len() as f32 * ROW_HEIGHT;
        *self.rows.borrow_mut() = rows;
        proposal.resolve(0.0, height).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let theme = ctx.theme;
        let style = &theme.typography.body;
        let key_color = TextRole::Secondary.resolve(&theme.colors);
        let value_color = TextRole::Primary.resolve(&theme.colors);

        for (i, row) in self.rows.borrow().iter().enumerate() {
            let y = bounds.y + (i as f32) * ROW_HEIGHT + 2.0;
            let key_rect = Rect::new(bounds.x + ROW_PADDING_X, y, KEY_COLUMN_WIDTH, ROW_HEIGHT);
            let value_x = bounds.x + ROW_PADDING_X + KEY_COLUMN_WIDTH + ROW_PADDING_X;
            let value_rect = Rect::new(
                value_x,
                y,
                (bounds.x + bounds.width - value_x).max(0.0),
                ROW_HEIGHT,
            );
            canvas.draw_text(&row.key, key_rect, style, key_color);
            canvas.draw_text(&row.value, value_rect, style, value_color);
        }
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}

fn collect_properties(
    arena: &WidgetArena,
    id: WidgetId,
    out: &mut Vec<KvRow>,
    full_debug: &mut String,
) {
    let Some(node) = arena.get(id) else {
        return;
    };
    let push = |key: &str, value: String, out: &mut Vec<KvRow>| {
        out.push(KvRow {
            key: key.to_string(),
            value,
        });
    };
    push("type", node.widget.type_name().to_string(), out);
    push(
        "short_type",
        last_segment(node.widget.type_name()).to_string(),
        out,
    );
    push("widget_id", format!("{:?}", id), out);
    let bounds = arena.bounds(id);
    push(
        "bounds",
        format!(
            "x={:.1} y={:.1} w={:.1} h={:.1}",
            bounds.x, bounds.y, bounds.width, bounds.height
        ),
        out,
    );
    push("clips_children", node.clips_children.to_string(), out);
    push(
        "event_pass_through",
        node.event_pass_through.to_string(),
        out,
    );
    if let Some(parent) = node.parent {
        push("parent", format!("{:?}", parent), out);
    }
    push("children", format!("{}", node.children.len()), out);
    push("needs_layout", node.dirty.needs_layout.to_string(), out);
    push("needs_paint", node.dirty.needs_paint.to_string(), out);
    push("needs_rebuild", node.dirty.needs_rebuild.to_string(), out);
    push("activation", format!("{:?}", node.activation), out);

    // Debug repr — single-line truncated for the visible row, full
    // text written into `full_debug` for the clipboard dump.
    let repr = format!("{:?}", node.widget);
    let one_line: String = repr.chars().filter(|c| *c != '\n').collect();
    let display = if one_line.chars().count() > DEBUG_REPR_DISPLAY_CAP {
        let mut s: String = one_line.chars().take(DEBUG_REPR_DISPLAY_CAP).collect();
        s.push('…');
        s
    } else {
        one_line
    };
    push("debug_repr", display, out);
    *full_debug = repr;
}
