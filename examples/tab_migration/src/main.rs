//! Tab migration — drag a tab from one `TabWidget` into another.
//!
//! Two small tabbed groups sit side by side, each backed by its own
//! `ListModel<TabHandle>`. Both opt into cross-bar transfer with a
//! single `.accept_external_tabs(true)` call, so a tab dragged out of
//! one group and dropped between the tabs of the other **migrates**:
//! it disappears from the source group and appears at the drop
//! position in the target group.
//!
//! The demo proves two invariants:
//!
//! 1. **The tab moves, it isn't copied.** After a migration the
//!    dragged tab is gone from the source `ListModel` and present in
//!    the target `ListModel` — each group mutates only its own model
//!    (the framework's "split" transfer model: the target inserts via
//!    its `on_tab_received`, the source removes via its
//!    `on_transfer_out`; here both use the built-in defaults that
//!    mutate the connected `dynamic_model`).
//!
//! 2. **State survives the move.** Each tab's heavy state lives on its
//!    `TabHandle::payload` (`Rc<dyn Any>` — here a `DocState` with an
//!    `edits: Signal<usize>` counter). The migrated `TabHandle` carries
//!    that same `Rc`, so the edit count you ran up in group A is intact
//!    when the tab lands in group B. It's a real move, not a rebuild
//!    from scratch.
//!
//! Intra-group reordering still works (drag within one group), and a
//! drag dropped on empty space outside both bars is cancelled (the tab
//! stays put).
//!
//! Run with: `cargo run -p tab-migration`.

use bastyde::core::widget::WidgetPlacement;
use bastyde::data::ListModel;
use bastyde::prelude::*;
use bastyde::widgets::{
    Button, ButtonVariant, Card, Divider, Expand, HStack, Panel, TabHandle, TabId, TabInfo,
    TabWidget, TextWidget, VStack,
};

/// Per-document state, kept on the `TabHandle::payload` (`Rc<dyn Any>`)
/// so it travels with the tab across a migration. The `edits` counter
/// is the visible proof that the same `Rc` moved (not a fresh handle).
#[derive(Debug)]
struct DocState {
    title: String,
    edits: Signal<usize>,
}

/// Build one document tab. `kind = "doc"` selects the content factory
/// both groups register, so a tab from either group renders in the
/// other.
fn new_doc(title: &str) -> TabHandle {
    TabHandle::dynamic(
        TabId::fresh(),
        "doc",
        TabInfo::new()
            .title(LocalizedString::literal(title.to_string()))
            .closable(true),
        DocState {
            title: title.to_string(),
            edits: Signal::new(0),
        },
    )
}

/// Content pane for a document tab — reads its `DocState` from the
/// handle payload. The "Make an edit" button bumps the shared
/// `edits` signal; that count persists through a migration.
fn doc_pane(state: &DocState) -> impl Widget + 'static {
    let edits = state.edits.clone();
    let title = state.title.clone();

    Card::new()
        .header(
            TextWidget::new(lit!(title))
                .style(TextStyleRole::BodyBold)
                .color(TextRole::Primary),
        )
        .content(
            VStack::new()
                .spacing(12.0)
                .child(
                    TextWidget::new(lit!(
                        "Drag this tab's header into the other group. The tab — and \
                         this edit count — move across intact."
                    ))
                    .style(TextStyleRole::Body)
                    .color(TextRole::Secondary),
                )
                .child(
                    HStack::new()
                        .spacing(8.0)
                        .child(
                            TextWidget::new(lit!("Edits:"))
                                .style(TextStyleRole::Small)
                                .color(TextRole::Secondary),
                        )
                        .child(
                            TextWidget::new(lit!(""))
                                .bind_text(edits.map(|n| n.to_string()))
                                .style(TextStyleRole::BodyBold)
                                .color(TextRole::Accent),
                        ),
                )
                .child(
                    Button::new(lit!("Make an edit"))
                        .variant(ButtonVariant::Filled)
                        .on_activate_fn({
                            let edits = edits.clone();
                            move |_ctx: &mut EventContext| edits.set(edits.get() + 1)
                        }),
                ),
        )
}

#[derive(Debug)]
struct Root {
    model_a: ListModel<TabHandle>,
    model_b: ListModel<TabHandle>,
    selected_a: Signal<Option<TabId>>,
    selected_b: Signal<Option<TabId>>,
    root_child_id: Option<WidgetId>,
}

impl Root {
    fn new() -> Self {
        let model_a =
            ListModel::from_vec(vec![new_doc("Alpha"), new_doc("Bravo"), new_doc("Charlie")]);
        let model_b = ListModel::from_vec(vec![new_doc("Xeno"), new_doc("Yotta")]);
        let selected_a = Signal::new(model_a.with_item(0, |h| h.id));
        let selected_b = Signal::new(model_b.with_item(0, |h| h.id));
        Self {
            model_a,
            model_b,
            selected_a,
            selected_b,
            root_child_id: None,
        }
    }

    /// One tabbed group. `accept_external_tabs(true)` makes it both a
    /// transfer source and a target; the built-in defaults insert into
    /// / remove from its own `dynamic_model`, so no explicit
    /// `on_tab_received` / `on_transfer_out` is needed here.
    fn group(
        ctx: &mut BuildContext,
        label: &str,
        model: &ListModel<TabHandle>,
        selected: &Signal<Option<TabId>>,
    ) -> WidgetId {
        let tabs = TabWidget::new(selected.clone())
            .dynamic_model(model.clone())
            .dynamic_tab::<DocState>("doc", |_h, state| Box::new(doc_pane(state)))
            .reorderable(true)
            .accept_external_tabs(true);

        let panel = Panel::new().child(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new(lit!(label.to_string()))
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(Expand::new().respect_intrinsic().child(tabs)),
        );
        ctx.add(panel)
    }
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let group_a = Self::group(ctx, "Group A", &self.model_a, &self.selected_a);
        let group_b = Self::group(ctx, "Group B", &self.model_b, &self.selected_b);

        let root = ctx.add(
            HStack::new()
                .spacing(12.0)
                .child(Expand::new().flex(1.0).child_id(group_a))
                .child(Divider::vertical())
                .child(Expand::new().flex(1.0).child_id(group_b)),
        );
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_child_id
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
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

fn main() {
    BastydeAppBuilder::new()
        .install_inspector_in_debug()
        .theme(bastyde::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Tab Migration — drag tabs between groups")
                .size(960, 640)
                .root(|tree, _state| tree.add(Root::new())),
        )
        .run();
}
