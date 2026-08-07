// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `Splitter` showcase — the N-pane, collapsible, serializable split
//! container that replaces the old two-pane `SplitView`.
//!
//! Demonstrates:
//!   * a 3-pane IDE layout (collapsible sidebar | editor | collapsible
//!     inspector) with per-pane stretch (only the editor absorbs window
//!     resize),
//!   * all collapse triggers — buttons (programmatic), double-click a
//!     divider, drag a side pane past its min to snap it shut, or focus a
//!     divider and press Enter,
//!   * collapse (folds the pane, keeps its gutter) vs. hide (removes the
//!     pane AND its gutter — the reactive "add / remove a pane" trick),
//!   * keyboard / drag resize,
//!   * export → restore of the layout state (the persistence surface a
//!     real app would route through `SettingsFile<SplitterState>`),
//!   * a vertical 2-pane split.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo::core::{Signal, WidgetPlacement};
use teksilo::prelude::*;
use teksilo::tokens::Orientation;
use teksilo::widgets::{
    Badge, Button, Expand, FixedSize, HStack, PaneDescriptor, Panel, ScrollArea, Spacer, Splitter,
    SplitterModel, SplitterState, TextWidget, Toolbar, VStack,
};

#[derive(Debug)]
struct SplitterDemo {
    /// The 3-pane horizontal layout — shared with the toolbar buttons.
    layout: SplitterModel,
    /// The vertical 2-pane layout (console over output list).
    vsplit: SplitterModel,
    /// The last exported snapshot, restored by the "Restore" button.
    saved: Rc<RefCell<Option<SplitterState>>>,
    /// Status line reflecting export/restore actions.
    status: Signal<String>,
    root_child_id: Option<WidgetId>,
}

impl SplitterDemo {
    fn new() -> Self {
        let layout = SplitterModel::from_panes(
            vec![
                // Sidebar: fixed-ish, collapsible.
                PaneDescriptor::new()
                    .size(220.0)
                    .min_size(160.0)
                    .stretch(0.0)
                    .collapsible(true),
                // Editor: absorbs all window-resize slack.
                PaneDescriptor::new().min_size(320.0).stretch(1.0),
                // Inspector: fixed-ish, collapsible.
                PaneDescriptor::new()
                    .size(280.0)
                    .min_size(200.0)
                    .stretch(0.0)
                    .collapsible(true),
            ],
            Orientation::Horizontal,
        );
        let vsplit = SplitterModel::new(2, Orientation::Vertical);
        Self {
            layout,
            vsplit,
            saved: Rc::new(RefCell::new(None)),
            status: Signal::new(String::from(
                "Drag a divider, double-click it, or use the buttons.",
            )),
            root_child_id: None,
        }
    }
}

fn describe(state: &SplitterState) -> String {
    let parts: Vec<String> = state
        .panes
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if p.collapsed {
                format!("#{i}=collapsed")
            } else {
                format!("#{i}={:.0}px", p.stored_size)
            }
        })
        .collect();
    format!("v{} · {}", state.version, parts.join("  "))
}

impl Widget for SplitterDemo {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let toolbar = {
            let layout_a = self.layout.clone();
            let layout_b = self.layout.clone();
            let layout_x = self.layout.clone();
            let layout_i = self.layout.clone();
            let saved_x = self.saved.clone();
            let saved_i = self.saved.clone();
            let status_x = self.status.clone();
            let status_i = self.status.clone();
            Toolbar::new().child(
                HStack::new()
                    .spacing(8.0)
                    .child(
                        // Collapse folds the pane but keeps its grabbable gutter.
                        Button::new(lit!("Collapse Sidebar"))
                            .on_activate_fn(move |_| layout_a.toggle_collapsed(0)),
                    )
                    .child(
                        // Hide removes the pane AND its gutter — it reads as
                        // added/removed (the "trick"), no rebuild.
                        Button::new(lit!("Add / Remove Inspector")).on_activate_fn(move |_| {
                            let shown = layout_b.is_pane_visible(2);
                            layout_b.set_pane_visible(2, !shown);
                        }),
                    )
                    .child(Spacer::new())
                    .child(Button::new(lit!("Export layout")).on_activate_fn(move |_| {
                        let state = layout_x.export_state();
                        status_x.set(format!("Exported  {}", describe(&state)));
                        *saved_x.borrow_mut() = Some(state);
                    }))
                    .child(
                        Button::new(lit!("Restore layout")).on_activate_fn(move |_| {
                            if let Some(state) = saved_i.borrow().clone() {
                                if layout_i.import_state(&state) {
                                    status_i.set(format!("Restored  {}", describe(&state)));
                                }
                            } else {
                                status_i.set(String::from(
                                    "Nothing exported yet — click Export first.",
                                ));
                            }
                        }),
                    )
                    .child(Spacer::new())
                    .child(teksilo::widgets::ThemeSwitcher::new()),
            )
        };

        // The 3-pane IDE layout. Only the editor stretches.
        let three_pane = Splitter::new(self.layout.clone())
            .pane_label(0, lit!("Sidebar"))
            .pane_label(1, lit!("Editor"))
            .pane_label(2, lit!("Inspector"))
            .pane(list_pane(
                "Project",
                &["src", "crates", "examples", "docs", "README.md"],
            ))
            .pane(text_pane(
                "Editor",
                "Only this pane grows when you resize the window (stretch = 1). \
                 Drag a side divider past its minimum to snap that pane collapsed; \
                 drag back out to restore it. Double-click a divider, or focus it \
                 (Tab) and press Enter, to toggle collapse.",
            ))
            .pane(list_pane(
                "Inspector",
                &["Selection", "Layout", "Accessibility", "Animations"],
            ));

        // A vertical 2-pane split below.
        let vertical = FixedSize::new().height(260.0_f32).child(
            Splitter::new(self.vsplit.clone())
                .pane(text_pane(
                    "Console",
                    "A vertical split — logs above an inspector or terminal.",
                ))
                .pane(list_pane("Output", &["build", "test", "clippy", "run"])),
        );

        let status_line = TextWidget::new(lit!(""))
            .text(self.status.clone())
            .style(TextStyleRole::Small)
            .color(TextRole::Secondary);

        let root = ctx.add(
            VStack::new()
                .child(toolbar)
                .child(Panel::new().padding(8.0).child(status_line))
                .child(
                    Expand::new()
                        .flex(2.0)
                        .child(Panel::new().padding(8.0).child(three_pane)),
                )
                .child(Panel::new().padding(8.0).child(vertical)),
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

fn list_pane(title: &str, items: &[&str]) -> impl Widget {
    let mut stack = VStack::new().spacing(10.0).child(
        TextWidget::new(lit!(title))
            .style(TextStyleRole::BodyBold)
            .color(TextRole::Primary),
    );
    for item in items {
        stack = stack.child(Badge::new(lit!(*item)));
    }
    Panel::new()
        .background(SurfaceRole::Sunken)
        .padding(16.0)
        .child(ScrollArea::new().child(stack))
}

fn text_pane(title: &str, body: &str) -> impl Widget {
    Panel::new()
        .background(SurfaceRole::Raised)
        .padding(20.0)
        .child(
            VStack::new()
                .spacing(12.0)
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

fn main() {
    TeksiloAppBuilder::new()
        .install_automation_bridge_in_debug()
        .install_inspector_in_debug()
        .theme(teksilo::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Splitter")
                .size(1100, 820)
                .root(|tree, _state| tree.add(SplitterDemo::new())),
        )
        .run();
}
