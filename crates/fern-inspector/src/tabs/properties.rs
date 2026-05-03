//! Properties tab — key/value rows for the selected widget plus a
//! Copy button that writes the formatted dump to the clipboard.
//!
//! Right-click on a row opens a context menu with `Copy value` that
//! copies just that row's value. Implemented with a manually-managed
//! overlay (the framework's `.context_menu()` builder consumes the
//! secondary click before our `on_pointer_event` could capture the
//! row position).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use fern_canvas::{Canvas, Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::arena::WidgetArena;
use fern_core::binding::BindingLevel;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, PointerButton, WidgetEvent};
use fern_core::overlay::{
    DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest,
};
use fern_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_platform::ClipboardHandle;
use fern_tokens::TextRole;
use fern_widgets::primitives::{HStack, Padding, VStack};
use fern_widgets::{Button, MenuItem, MenuList};

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
        let copy_button = Button::new_literal("Copy").on_activate_fn(move |ctx| {
            if let Some(cb) = ctx.app_state::<ClipboardHandle>() {
                let _ = cb.set_text(&dump.get());
            }
        });
        let toolbar = Padding::symmetric(2.0, 4.0).child(HStack::new().spacing(6.0).child(copy_button));
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
            c.origin = fern_canvas::Point::new(bounds.x, bounds.y);
            c.size = fern_canvas::Size::new(bounds.width, bounds.height);
        }
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}

struct PropertiesRows {
    state: InspectorState,
    /// Shared rows snapshot — RefCell inside Rc so the
    /// `on_pointer_event` handler closure (which captures by move) and
    /// the `paint` / `layout_response` methods (which read via
    /// `&self`) can share the same data without ownership gymnastics.
    rows: Rc<RefCell<Vec<KvRow>>>,
    /// Overlay content id for the right-click context menu — set
    /// during build, consumed by the secondary-click handler.
    context_menu_id: Rc<Cell<Option<WidgetId>>>,
}

impl PropertiesRows {
    fn new(state: InspectorState) -> Self {
        Self {
            state,
            rows: Rc::new(RefCell::new(Vec::new())),
            context_menu_id: Rc::new(Cell::new(None)),
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
        self.state.selected_id.bind_to(
            self_id,
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );

        // Pre-register the context-menu MenuList as an orphan child.
        // The framework activates it when `show_overlay` is called from
        // the right-click handler. The menu's "Copy value" action
        // reads `state.properties_context_value` — set just before the
        // overlay opens — so a single static menu serves every row.
        let value_sig = self.state.properties_context_value.clone();
        let copy_item = MenuItem::new_literal("Copy value")
            .on_activate_fn(move |c| {
                if let Some(cb) = c.app_state::<ClipboardHandle>() {
                    let _ = cb.set_text(&value_sig.get());
                }
            });
        let menu = MenuList::new().item(copy_item);
        let menu_id = ctx.add(menu);
        self.context_menu_id.set(Some(menu_id));

        // Right-click handler: stash the row's value into the shared
        // signals, then activate + show the menu at the click point.
        let rows_for_handler = self.rows.clone();
        let key_sig = self.state.properties_context_key.clone();
        let value_sig = self.state.properties_context_value.clone();
        let menu_slot = self.context_menu_id.clone();
        let handlers = HandlerSet::new().on_pointer_event(move |event, ctx| match event {
            WidgetEvent::PointerDown {
                position,
                button: PointerButton::Secondary,
                ..
            } => {
                let idx = (position.y / ROW_HEIGHT).floor() as usize;
                let rows = rows_for_handler.borrow();
                let Some(row) = rows.get(idx) else {
                    return EventResponse::Ignored;
                };
                key_sig.set(row.key.clone());
                value_sig.set(row.value.clone());
                drop(rows);
                if let Some(menu_id) = menu_slot.get() {
                    ctx.activate(menu_id);
                    ctx.show_overlay(OverlayRequest {
                        content_id: menu_id,
                        anchor: self_id,
                        placement: OverlayPlacement::AtPointer(*position),
                        dismiss: DismissBehavior::EscapeOrClickOutside,
                        layer: OverlayLayer::InTree,
                        parent_overlay: None,
                        on_dismiss: None,
                        fade_duration: None,
                    });
                }
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        });
        ctx.apply_self_handlers(handlers);
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
    push("short_type", last_segment(node.widget.type_name()).to_string(), out);
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
    push("event_pass_through", node.event_pass_through.to_string(), out);
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
