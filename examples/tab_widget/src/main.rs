// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! TabWidget showcase — exercises every capability of the rewritten
//! TabWidget / TabBar pair:
//!
//! - Static tabs (pinned, disabled, default)
//! - Dynamic tabs from a `ListModel<TabHandle>` with a registered
//!   `dynamic_tab::<DocState>` factory; "+" button in the bar's
//!   trailing slot opens new dynamic tabs.
//! - Closable tabs with the close button + middle-click; pinned
//!   tabs suppress the close button (Firefox / Chrome convention).
//! - Drag-to-reorder, with the insertion-line drop indicator.
//! - Overflow dropdown — `PopoverButton` + `ListView` listing all
//!   tabs by stable `TabId`, click activates and dismisses.
//! - Scroll arrows + mouse-wheel-to-horizontal mapping.
//! - Theme toggle, locale toggle (live retitling), and orientation
//!   toggle (Horizontal / Vertical) via toolbar buttons.
//! - Per-tab tooltip via `TabInfo::tooltip(...)`.
//! - Bar leading slot (mode toggle), trailing slot ("new tab"
//!   button).
//!
//! Run with: `cargo run -p tab-widget`.

use std::cell::Cell;
use std::rc::Rc;

use teksilo::core::widget::WidgetPlacement;
use teksilo::data::ListModel;
use teksilo::prelude::*;
use teksilo::widgets::{
    Badge, Breadcrumb, BreadcrumbItem, Button, ButtonVariant, Card, HStack, IconWidget, MessageBox,
    MessageBoxButtons, Panel, StandardButton, TabBarOrientation, TabHandle, TabId, TabInfo,
    TabSizing, TabWidget, TextWidget, VStack,
};

/// Per-document mutable state — kept on the `TabHandle::payload`
/// (`Rc<dyn Any>`) so reorder / pin toggles preserve the user's
/// scroll position, undo stack, etc. Here we just track a free-form
/// edit count to demonstrate the pattern.
#[derive(Debug)]
struct DocState {
    title: String,
    edits: Signal<usize>,
}

#[derive(Debug)]
struct Root {
    /// Stable ids for the always-present static tabs so we can flip
    /// selection from the toolbar without depending on indices.
    welcome_id: TabId,
    locked_id: TabId,
    settings_id: TabId,
    /// Active selection — stable across reorders / closes.
    selected: Signal<Option<TabId>>,
    /// Live document tabs. Mutate via `model.push` to open, the
    /// framework removes via the bar's default close handler.
    model: ListModel<TabHandle>,
    /// Reactive UI state. The orientation and sizing signals are
    /// bound into TabWidget via `.orientation_signal(...)` and
    /// `.sizing_signal(...)` — toolbar buttons just `.set(...)`
    /// them and the framework rebuilds.
    orientation: Signal<TabBarOrientation>,
    sizing: Signal<TabSizing>,
    /// Counter to give new docs unique titles.
    next_doc_n: Rc<Cell<u32>>,

    root_child_id: Option<WidgetId>,
}

impl Root {
    fn new() -> Self {
        // Seed the dynamic-model with three opened docs so the
        // showcase isn't empty on first run.
        let model: ListModel<TabHandle> =
            ListModel::from_vec(vec![new_doc_tab(1), new_doc_tab(2), new_doc_tab(3)]);
        Self {
            welcome_id: TabId::fresh(),
            locked_id: TabId::fresh(),
            settings_id: TabId::fresh(),
            selected: Signal::new(None),
            model,
            orientation: Signal::new(TabBarOrientation::Horizontal),
            sizing: Signal::new(TabSizing::Shared),
            next_doc_n: Rc::new(Cell::new(4)),
            root_child_id: None,
        }
    }
}

fn new_doc_tab(n: u32) -> TabHandle {
    let title = format!("Doc {n}");
    TabHandle::dynamic(
        TabId::fresh(),
        "doc",
        TabInfo::new()
            .title(lit!(title.clone()))
            .tooltip(lit!(format!("Document #{n}")))
            .closable(true),
        DocState {
            title,
            edits: Signal::new(0),
        },
    )
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // The demo intentionally uses **semantic roles** for every
        // color / typography choice — `TextRole`, `TextStyleRole`,
        // `SurfaceRole`, `BorderRole`. Frozen `theme.colors.X` reads
        // would baking the literal at construction time and miss
        // theme switches; roles resolve at paint time and update
        // reactively. (See [docs/reactive-theme.md](../../docs/reactive-theme.md).)
        let _ = ctx;

        // ── Toolbar slots ─────────────────────────────────────────
        //
        // Bar leading slot: a small breadcrumb-style label so the
        // bar visually anchors at a recognizable position.
        let leading_slot = TextWidget::new(lit!(" Showcase "))
            .style(TextStyleRole::SmallBold)
            .color(TextRole::Secondary);

        // Bar trailing slot: a mini-toolbar with mode toggles +
        // "new tab" button. Stays visible regardless of how many
        // tabs are open.
        let new_tab_n = self.next_doc_n.clone();
        let model_for_new = self.model.clone();
        // The + glyph isn't a built-in icon yet; a small text Button
        // with a literal "+" glyph reads cleanly in the trailing
        // slot.
        let new_tab_btn = Button::new(lit!("+ New tab"))
            .variant(ButtonVariant::Ghost)
            .tooltip(lit!("Open new document tab"))
            .on_activate_fn(move |_ctx: &mut EventContext| {
                let n = new_tab_n.get();
                new_tab_n.set(n + 1);
                model_for_new.push(new_doc_tab(n));
            });

        let theme_btn = teksilo::widgets::ThemeSwitcher::new();

        let orientation_for_btn = self.orientation.clone();
        let orient_btn = Button::new(lit!("Orient"))
            .variant(ButtonVariant::Ghost)
            .tooltip(lit!("Toggle bar orientation"))
            .on_activate_fn(move |_ctx: &mut EventContext| {
                let next = match orientation_for_btn.get() {
                    TabBarOrientation::Horizontal => TabBarOrientation::Vertical,
                    TabBarOrientation::Vertical => TabBarOrientation::Horizontal,
                };
                orientation_for_btn.set(next);
            });

        let sizing_for_btn = self.sizing.clone();
        let size_btn = Button::new(lit!("Sizing"))
            .variant(ButtonVariant::Ghost)
            .tooltip(lit!(
                "Cycle Shared (uniform tab widths) → Independent (size to content) → Fill (stretch across the bar)",
            ))
            .on_activate_fn(move |_ctx: &mut EventContext| {
                let next = match sizing_for_btn.get() {
                    TabSizing::Shared => TabSizing::Independent,
                    TabSizing::Independent => TabSizing::Fill,
                    TabSizing::Fill => TabSizing::Shared,
                };
                sizing_for_btn.set(next);
            });

        let trailing_slot = HStack::new()
            .spacing(4.0)
            .child(size_btn)
            .child(orient_btn)
            .child(theme_btn)
            .child(new_tab_btn);

        // ── Static tabs ───────────────────────────────────────────

        let welcome = static_welcome_tab();
        let locked = static_locked_tab();
        let settings = static_settings_tab(self.orientation.clone());

        // ── Dynamic tabs ──────────────────────────────────────────

        let model_for_factory = self.model.clone();
        let _ = model_for_factory; // captured implicitly via dynamic_tab

        // ── Compose the TabWidget ────────────────────────────────
        //
        // The orientation signal flows into TabWidget directly via
        // `.orientation(...)`. TabWidget binds it at
        // `BindingLevel::Rebuild`, so toggling from the toolbar
        // rebuilds with the new outer layout. Memoized panes
        // survive the rebuild — focus, scroll, and per-document
        // state (counts, undo, …) are preserved.
        let tw = TabWidget::new(self.selected.clone())
            .orientation(self.orientation.clone())
            .sizing(self.sizing.clone())
            // Static: pinned welcome (icon-only, tooltip-promoted title).
            .static_tab_with_id(
                self.welcome_id,
                TabInfo::new()
                    .title(lit!("Welcome"))
                    .tooltip(lit!("Welcome — start here"))
                    .icon(|| IconWidget::checkmark(16.0))
                    .pinned(true),
                welcome,
            )
            // Static: settings page (default style, with leading icon).
            .static_tab_with_id(
                self.settings_id,
                TabInfo::new()
                    .title(lit!("Settings"))
                    .icon(|| IconWidget::checkmark(16.0)),
                settings,
            )
            // Static: disabled tab — visible but unactivatable.
            .static_tab_with_id(
                self.locked_id,
                TabInfo::new()
                    .title(lit!("Locked"))
                    .tooltip(lit!("Disabled tabs cannot be activated",))
                    .enabled(false),
                locked,
            )
            // Dynamic: registration for "doc" kind. The framework
            // downcasts `handle.payload` to `DocState` before
            // calling the closure, so `Any` never leaks out.
            .dynamic_tab::<DocState>("doc", move |_h, state| {
                Box::new(doc_pane(state)) as Box<dyn Widget>
            })
            .dynamic_model(self.model.clone())
            // Behavior knobs.
            .reorderable(true)
            // Close interceptor — confirm before actually removing a
            // tab from the model. The handler receives a borrowed
            // `EventContext`, so it can present a modal MessageBox
            // and only mutate the model on accept. Demonstrates the
            // veto / confirm-then-act pattern.
            .on_close({
                let model = self.model.clone();
                move |id, ctx| {
                    let title = (0..model.len())
                        .find_map(|i| {
                            model
                                .with_item(i, |h| (h.id == id).then(|| h.info_title_cloned()))
                                .flatten()
                        })
                        .unwrap_or_else(|| "this tab".to_string());
                    let model_for_cb = model.clone();
                    MessageBox::question(lit!("Close tab?"))
                        .text(lit!(format!("Are you sure you want to close \"{title}\"?")))
                        .informative_text(lit!("Unsaved changes in the tab will be lost."))
                        .buttons(MessageBoxButtons::YesNo)
                        .default_button(StandardButton::No)
                        .escape_button(StandardButton::No)
                        .on_result(move |r, _ctx| {
                            if r.button == StandardButton::Yes
                                && let Some(idx) = (0..model_for_cb.len()).find(|&i| {
                                    model_for_cb.with_item(i, |h| h.id == id).unwrap_or(false)
                                })
                            {
                                let _ = model_for_cb.remove(idx);
                            }
                        })
                        .present(ctx);
                }
            })
            // Bar slots.
            .bar_leading_slot(leading_slot)
            .bar_trailing_slot(trailing_slot);

        let tabs = ctx.add(tw);

        // ── Status bar (selection summary + tab count) ────────────
        let selected_summary = self.selected.map({
            let welcome_id = self.welcome_id;
            let locked_id = self.locked_id;
            let settings_id = self.settings_id;
            let model = self.model.clone();
            move |id| {
                let Some(id) = *id else {
                    return "no selection".to_string();
                };
                if id == welcome_id {
                    return "Welcome (pinned static)".to_string();
                }
                if id == locked_id {
                    return "Locked".to_string();
                }
                if id == settings_id {
                    return "Settings".to_string();
                }
                let mut found = "Document".to_string();
                for i in 0..model.len() {
                    if let Some(label) = model.with_item(i, |h| (h.id, h.info_title_cloned()))
                        && label.0 == id
                    {
                        found = label.1;
                        break;
                    }
                }
                found
            }
        });

        let breadcrumb = ctx.add(
            Breadcrumb::new()
                .item(BreadcrumbItem::new(lit!("Library")))
                .item(BreadcrumbItem::new(lit!("Components")))
                .item(BreadcrumbItem::current(lit!("TabWidget"))),
        );

        let header = ctx.add(
            VStack::new()
                .spacing(8.0)
                .add_child(breadcrumb)
                .child(
                    TextWidget::new(lit!("TabWidget — full showcase"))
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new(lit!(
                        "Pinned + closable + dynamic + drag-reorder + overflow dropdown + \
                         orientation toggle + per-tab tooltip + accessibility custom actions."
                    ))
                    .style(TextStyleRole::Body)
                    .color(TextRole::Secondary),
                ),
        );

        let status = ctx.add(
            HStack::new()
                .spacing(12.0)
                .child(
                    TextWidget::new(lit!("Active:"))
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary),
                )
                .child(
                    TextWidget::new(lit!(""))
                        .text(selected_summary)
                        .style(TextStyleRole::Small)
                        .color(TextRole::Primary),
                ),
        );

        // Wrap the tabs id in `Expand::vertical()` so the
        // TabWidget takes all the slack between the static header
        // (top) and the status row (bottom). Without this the
        // TabWidget collapses to its natural height and the
        // window's vertical area shows mostly empty Panel.
        let tabs_filling = ctx.add(
            teksilo::widgets::Expand::vertical()
                .respect_intrinsic()
                .child_id(tabs),
        );
        let root_id = ctx.add(
            Panel::new().padding(20.0).child(
                VStack::new()
                    .spacing(12.0)
                    .add_child(header)
                    .add_child(tabs_filling)
                    .add_child(status),
            ),
        );

        self.root_child_id = Some(root_id);
        vec![root_id]
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

// Ad-hoc helpers for things the public TabHandle API doesn't expose.
trait TabHandleExt {
    fn info_title_cloned(&self) -> String;
}
impl TabHandleExt for TabHandle {
    fn info_title_cloned(&self) -> String {
        // info.title is private; resolve the Localized title via Debug
        // is overkill — we tracked the title separately on DocState.
        if let Some(state) = self.payload.downcast_ref::<DocState>() {
            return state.title.clone();
        }
        // Static handles with no DocState: fall through to a stub.
        String::from("(unknown)")
    }
}

// ─── Static tab content builders ────────────────────────────────────
//
// All content uses semantic roles (`TextRole`, `TextStyleRole`,
// `SurfaceRole`, `BorderRole`) — no frozen colors / typography
// snapshots. Theme switches retint everything without rebuilding.

fn static_welcome_tab() -> impl Widget + 'static {
    Card::new()
        .header(
            TextWidget::new(lit!("Welcome to TabWidget"))
                .style(TextStyleRole::BodyBold)
                .color(TextRole::Primary),
        )
        .content(
            VStack::new()
                .spacing(10.0)
                .child(
                    TextWidget::new(lit!(
                        "This first tab is pinned: icon-only, fixed-width, no close \
                         button. Drag a non-pinned tab into the leading strip to pin \
                         it (the bar will fire `on_pin_toggle`)."
                    ))
                    .style(TextStyleRole::Body)
                    .color(TextRole::Primary),
                )
                .child(
                    HStack::new()
                        .spacing(8.0)
                        .child(Badge::new(lit!("Pinned")))
                        .child(Badge::new(lit!("Icon-only")))
                        .child(Badge::new(lit!("No close"))),
                ),
        )
}

fn static_locked_tab() -> impl Widget + 'static {
    Panel::new().padding(20.0).child(
        VStack::new()
            .spacing(10.0)
            .child(
                TextWidget::new(lit!("Locked"))
                    .style(TextStyleRole::BodyBold)
                    .color(TextRole::Primary),
            )
            .child(
                TextWidget::new(lit!(
                    "Disabled tabs are visible but cannot be activated. Keyboard \
                     arrow nav skips over them — try Tab + ArrowRight from the \
                     active tab."
                ))
                .style(TextStyleRole::Body)
                .color(TextRole::Secondary),
            ),
    )
}

fn static_settings_tab(orientation: Signal<TabBarOrientation>) -> impl Widget + 'static {
    let toggle_orient = {
        let o = orientation.clone();
        Button::new(lit!("Toggle orientation"))
            .variant(ButtonVariant::Ghost)
            .on_activate_fn(move |_ctx: &mut EventContext| {
                let next = match o.get() {
                    TabBarOrientation::Horizontal => TabBarOrientation::Vertical,
                    TabBarOrientation::Vertical => TabBarOrientation::Horizontal,
                };
                o.set(next);
            })
    };

    Panel::new().padding(20.0).child(
        VStack::new()
            .spacing(12.0)
            .child(
                TextWidget::new(lit!("Settings"))
                    .style(TextStyleRole::BodyBold)
                    .color(TextRole::Primary),
            )
            .child(
                TextWidget::new(lit!(
                    "Bar configuration knobs are forwarded from `TabWidget` to the \
                     inner `TabBar`. Try the orientation toggle below — the bar \
                     flips between top (Horizontal) and leading edge (Vertical), \
                     and per-tab state survives the rebuild because content panes \
                     are memoized."
                ))
                .style(TextStyleRole::Body)
                .color(TextRole::Primary),
            )
            .child(toggle_orient)
            .child(
                HStack::new()
                    .spacing(8.0)
                    .child(Badge::new(lit!("Reorderable")))
                    .child(Badge::new(lit!("Closable docs")))
                    .child(Badge::new(lit!("Overflow dropdown")))
                    .child(Badge::new(lit!("Custom AT actions"))),
            ),
    )
}

// ─── Dynamic doc-tab content ───────────────────────────────────────

fn doc_pane(state: &DocState) -> impl Widget + 'static {
    let edits = state.edits.clone();
    let title = state.title.clone();

    let edit_btn = Button::new(lit!("Make an edit"))
        .variant(ButtonVariant::Filled)
        .on_activate_fn({
            let edits = edits.clone();
            move |_ctx: &mut EventContext| edits.set(edits.get() + 1)
        });

    Card::new()
        .header(
            TextWidget::new(lit!(title.clone()))
                .style(TextStyleRole::BodyBold)
                .color(TextRole::Primary),
        )
        .content(
            VStack::new()
                .spacing(10.0)
                .child(
                    TextWidget::new(lit!(
                        "Heavy state (here: an `edits: Signal<usize>` counter) lives \
                         on the `TabHandle::payload`, not in the widget. The pane \
                         widget reads the payload via the registered factory — \
                         reorders and pin toggles preserve the count."
                    ))
                    .style(TextStyleRole::Body)
                    .color(TextRole::Primary),
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
                                .text(edits.map(|n| n.to_string()))
                                .style(TextStyleRole::BodyBold)
                                .color(TextRole::Accent),
                        ),
                )
                .child(edit_btn),
        )
}

fn main() {
    TeksiloAppBuilder::new()
        .install_automation_bridge_in_debug()
        .install_inspector_in_debug()
        .theme(teksilo::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("TabWidget — Showcase")
                .size(1080, 720)
                .root(|tree, _state| tree.add(Root::new())),
        )
        .run();
}
