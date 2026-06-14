//! `DockingLayout` showcase — a VS Code-style IDE shell.
//!
//! Demonstrates the full docking stack:
//!   * a fixed **centre** editor surrounded by four dockable sides,
//!   * a **leading** side in **Rail** presentation (an activity bar — click an
//!     item to switch / re-click the active one to hide the side) whose active
//!     tab holds two DockWidgets, each in its own single-item **ToolBox**,
//!     separated by a **Splitter**,
//!   * a **bottom** side in **Strip** presentation with **two tabs**
//!     (Terminal | Problems) — drag a panel's ToolBox header onto another
//!     pane's edge to split, or onto its centre to stack,
//!   * a **trailing** Properties panel,
//!   * drag a panel out / between sides; hide / show sides from the rail or the
//!     toolbar; flip a corner owner live; export → restore the layout.

use bastyde::core::Signal;
use bastyde::prelude::*;
use bastyde::widgets::{
    Badge, Button, DockCorner, DockOpenLocation, DockRail, DockSide, DockWidget, DockWidgetId,
    DockingLayout, DockingModel, Expand, HStack, IconButton, IconButtonSize, IconWidget, Panel,
    ScrollArea, Spacer, TextWidget, Toolbar, VStack,
};

#[derive(Debug)]
struct DockingDemo {
    model: DockingModel,
    saved: std::rc::Rc<std::cell::RefCell<Option<bastyde::widgets::DockLayoutState>>>,
    status: Signal<String>,
    bottom_corner_to_leading: Signal<bool>,
    root_child_id: Option<WidgetId>,
    // Dock ids held so the toolbar can address sides.
    ids: DemoIds,
}

#[derive(Debug, Clone, Copy)]
struct DemoIds {
    explorer: DockWidgetId,
    search: DockWidgetId,
    terminal: DockWidgetId,
    problems: DockWidgetId,
    properties: DockWidgetId,
}

fn list_panel(title: &str, items: &[&str]) -> impl Widget {
    let mut stack = VStack::new().spacing(8.0).child(
        TextWidget::new(lit!(title))
            .style(TextStyleRole::BodyBold)
            .color(TextRole::Primary),
    );
    for item in items {
        stack = stack.child(Badge::new(lit!(*item)));
    }
    Panel::new()
        .background(SurfaceRole::Sunken)
        .padding(12.0)
        .child(ScrollArea::new().child(stack))
}

fn text_panel(title: &str, body: &str) -> impl Widget {
    Panel::new().background(SurfaceRole::Raised).padding(14.0).child(
        VStack::new()
            .spacing(8.0)
            .child(
                TextWidget::new(lit!(title))
                    .style(TextStyleRole::BodyBold)
                    .color(TextRole::Primary),
            )
            .child(
                TextWidget::new(lit!(body))
                    .style(TextStyleRole::Body)
                    .color(TextRole::Secondary),
            ),
    )
}

impl DockingDemo {
    fn new() -> Self {
        let model = DockingModel::new();
        let ids = DemoIds {
            explorer: DockWidgetId::fresh(),
            search: DockWidgetId::fresh(),
            terminal: DockWidgetId::fresh(),
            problems: DockWidgetId::fresh(),
            properties: DockWidgetId::fresh(),
        };
        // The leading side is an activity rail; the trailing/bottom use strips.
        model.set_side_rail(DockSide::Leading, 48.0);
        Self {
            model,
            saved: std::rc::Rc::new(std::cell::RefCell::new(None)),
            status: Signal::new(String::from(
                "Drag a panel title bar onto a pane edge to split, or to another side to move it.",
            )),
            bottom_corner_to_leading: Signal::new(false),
            root_child_id: None,
            ids,
        }
    }
}

impl Widget for DockingDemo {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let ids = self.ids;

        let editor = text_panel(
            "Editor",
            "The centre is your app's main content — always present. The four \
             sides hold dockable panels. Resize a side by dragging its divider; \
             collapse it past its minimum to hide it (reopen from the leading \
             activity rail).",
        );

        let layout = DockingLayout::new(self.model.clone())
            .center(editor)
            // Style the leading activity rail: Large items, a logo on top, a
            // settings glyph at the bottom, and an overflow trigger that opens
            // a list of any items that don't fit.
            .rail(
                DockRail::new(DockSide::Leading)
                    .size(IconButtonSize::Large)
                    .top_slot(|| TextWidget::new(lit!("◆")).style(TextStyleRole::BodyBold))
                    .bottom_slot(|| {
                        IconButton::new(IconWidget::chevron_up(20.0))
                            .size(IconButtonSize::Large)
                            .tooltip(lit!("Settings"))
                    })
                    .overflow_icon(|| IconWidget::chevron_down(18.0)),
            )
            .dock(
                DockWidget::new(ids.explorer, lit!("Explorer"), |_| {
                    list_panel("Explorer", &["src", "crates", "examples", "docs", "Cargo.toml"])
                })
                .default_location(DockOpenLocation::side(DockSide::Leading)),
            )
            .dock(
                DockWidget::new(ids.search, lit!("Search"), |_| {
                    list_panel("Search results", &["main.rs:42", "model.rs:17", "geometry.rs:9"])
                })
                .default_location(DockOpenLocation::side(DockSide::Leading)),
            )
            .dock(
                DockWidget::new(ids.terminal, lit!("Terminal"), |_| {
                    text_panel("Terminal", "$ cargo run -p docking\n   Compiling …")
                })
                .default_location(DockOpenLocation::side(DockSide::Bottom)),
            )
            .dock(
                DockWidget::new(ids.problems, lit!("Problems"), |_| {
                    list_panel("Problems", &["warning: unused import", "note: 0 errors"])
                })
                .default_location(DockOpenLocation::side(DockSide::Bottom)),
            )
            .dock(
                DockWidget::new(ids.properties, lit!("Properties"), |_| {
                    list_panel("Properties", &["Name", "Type", "Layout", "Accessibility"])
                })
                .default_location(DockOpenLocation::side(DockSide::Trailing)),
            );

        // Initial layout: leading rail with Explorer + Search stacked (a
        // ToolBox); bottom strip with Terminal + Problems as two tabs;
        // trailing Properties.
        self.model
            .open_dock(ids.explorer, DockOpenLocation::side(DockSide::Leading));
        self.model
            .open_dock(ids.search, DockOpenLocation::side(DockSide::Leading).stack());
        self.model
            .open_dock(ids.terminal, DockOpenLocation::side(DockSide::Bottom));
        self.model
            .open_dock(ids.problems, DockOpenLocation::side(DockSide::Bottom).new_tab());
        self.model
            .open_dock(ids.properties, DockOpenLocation::side(DockSide::Trailing));

        let toolbar = self.build_toolbar();

        let status_line = TextWidget::new(lit!(""))
            .bind_text(self.status.clone())
            .style(TextStyleRole::Small)
            .color(TextRole::Secondary);

        let root = ctx.add(
            VStack::new()
                .child(toolbar)
                .child(Panel::new().padding(6.0).child(status_line))
                .child(Expand::new().flex(1.0).child(layout)),
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
        children: &mut [bastyde::core::WidgetPlacement],
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

impl DockingDemo {
    fn build_toolbar(&self) -> Toolbar {
        let model_b = self.model.clone();
        let model_t = self.model.clone();
        let model_p = self.model.clone();
        let model_corner = self.model.clone();
        let model_x = self.model.clone();
        let model_i = self.model.clone();
        let saved_x = self.saved.clone();
        let saved_i = self.saved.clone();
        let status_x = self.status.clone();
        let status_i = self.status.clone();
        let corner_flag = self.bottom_corner_to_leading.clone();
        let is_dark = Signal::new(false);

        Toolbar::new().child(
            HStack::new()
                .spacing(8.0)
                .child(
                    Button::new(lit!("Toggle Sidebar"))
                        .on_activate_fn(move |_| model_b.toggle_side_visible(DockSide::Leading)),
                )
                .child(
                    Button::new(lit!("Toggle Panel"))
                        .on_activate_fn(move |_| model_p.toggle_side_visible(DockSide::Bottom)),
                )
                .child(
                    Button::new(lit!("Toggle Inspector"))
                        .on_activate_fn(move |_| model_t.toggle_side_visible(DockSide::Trailing)),
                )
                .child(
                    // Flip whether the bottom panel or the leading sidebar owns
                    // the bottom-leading corner.
                    Button::new(lit!("Flip Corner")).on_activate_fn(move |_| {
                        let to_leading = !corner_flag.get();
                        corner_flag.set(to_leading);
                        model_corner.set_corner(
                            DockCorner::BottomLeading,
                            if to_leading {
                                DockSide::Leading
                            } else {
                                DockSide::Bottom
                            },
                        );
                    }),
                )
                .child(Spacer::new())
                .child(Button::new(lit!("Export")).on_activate_fn(move |_| {
                    let state = model_x.export_state();
                    status_x.set(String::from("Exported layout."));
                    *saved_x.borrow_mut() = Some(state);
                }))
                .child(Button::new(lit!("Restore")).on_activate_fn(move |_| {
                    if let Some(state) = saved_i.borrow().clone() {
                        model_i.import_state(&state);
                        status_i.set(String::from("Restored layout."));
                    } else {
                        status_i.set(String::from("Nothing exported yet."));
                    }
                }))
                .child(Spacer::new())
                .child(
                    Button::new(lit!("Toggle Dark Mode")).on_activate_fn(move |ctx| {
                        let next = !is_dark.get();
                        is_dark.set(next);
                        ctx.set_theme(if next {
                            bastyde::presets::intui::dark()
                        } else {
                            bastyde::presets::intui::light()
                        });
                    }),
                ),
        )
    }
}

fn main() {
    BastydeAppBuilder::new()
        .install_inspector_in_debug()
        .theme(bastyde::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Docking")
                .size(1280, 860)
                .root(|tree, _state| tree.add(DockingDemo::new())),
        )
        .run();
}
