// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Every target in the stock widget catalog, measured at all three densities.
//!
//! # Why a named list and not the preview catalog
//!
//! `preview_catalog.rs` is an avowedly partial *"Coverage in v1"* set with a
//! written skip list, and it has no entry for most of the controls the touch
//! programme changed — the twist arrow, the tab close button, the spin-box step
//! buttons, the table header's resize grip, the splitter and dock gutters, the
//! colour strips. A conformance gate driven by it would be green because it
//! never looked. So the list below is explicit, and its selection rule is
//! written down:
//!
//! 1. every widget that implements one of the two hit-widening hooks
//!    (`Widget::hit_outset`, `Widget::target_regions`) — the controls that
//!    *declare* they are undersized and rely on a mechanism;
//! 2. every widget `docs/density-inventory.md` records as a sub-24 dp
//!    visual-fixed `Target` or `Grab` with a named mechanism — the 27 rows the
//!    floor rule pushed off `dp`;
//! 3. one representative of each family in `docs/widget-pointer-inventory.md`
//!    that the programme touched: the controls sweep, the composites, the five
//!    data views, the menus, the overlays, the chrome, the editors.
//!
//! # Why the fixtures are composed and not bare
//!
//! A control in a bare stack **over-reports its reach**. The miss-only slop pass
//! wins only when the exact hit's bubble path carries no eligible handler, and a
//! bare stack carries none — so the pass tops up every near miss and a broken
//! control measures as conformant. Five of seven `hit_outset` implementations
//! passed their own deletion for exactly this reason during P24. Every fixture
//! here therefore builds its subject **inside the container it ships in**: a
//! chevron inside a tappable tree row, a swatch inside a picker row, a gutter
//! between two panes.
//!
//! Reference: `docs/density-and-targets.md`, `docs/accessibility-internal-audit.md`.

use std::rc::Rc;

use jiff::civil::{Date, Time};
use teksilo_canvas::{Point, Size};
use teksilo_core::accessibility::target_audit::{
    AllowedViolation,
    PinnedDp::{ClearsFloor, Is},
    PinnedGeometry, TargetFixture, TargetRule, TargetViolation, audit_fixtures, measure_fixtures,
    unprojected_style_slots,
};
use teksilo_core::presets::intui;
use teksilo_core::signal::Signal;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_data::{ListModel, SelectionMode, SelectionModel, TreeModel};
use teksilo_i18n::lit;
use teksilo_tokens::{Color, InputTokens, TargetDensity};
use teksilo_widgets::primitives::{
    HStack, IconWidget, Padding, RectWidget, TextWidget, TouchTarget, TwistArrow, VStack,
};
use teksilo_widgets::toast::host::{ToastHost, ToastInstallOptions};
use teksilo_widgets::toast::registry::ToastRegistry;
use teksilo_widgets::*;

// =========================================================================
// Scaffolding
// =========================================================================

/// A tappable row wrapping a subject — the geometry that denies the slop pass
/// and makes a fixture discriminating. Returns the row's id.
fn in_tappable_row(
    tree: &mut WidgetTree,
    subject: impl teksilo_core::widget::Widget + 'static,
) -> WidgetId {
    use teksilo_core::widget_builder::WidgetBuilder;
    tree.add(
        Padding::uniform(20.0)
            .child(
                HStack::new()
                    .spacing(8.0)
                    .child(TextWidget::new(lit!("Row label")))
                    .child(subject),
            )
            .on_tap(|_, _| {}),
    )
}

/// A subject in ordinary page furniture, with no tappable ancestor. Use only
/// where the widget genuinely ships that way (a dialog, a toolbar, a window
/// shell) — a bare stack around a small control makes a fixture blind.
fn on_a_page(
    tree: &mut WidgetTree,
    subject: impl teksilo_core::widget::Widget + 'static,
) -> WidgetId {
    tree.add(
        Padding::uniform(20.0).child(
            VStack::new()
                .spacing(12.0)
                .child(TextWidget::new(lit!("Section")))
                .child(subject),
        ),
    )
}

/// One archived notification. `NotificationArchiveModel::push` takes a
/// `NotificationEntry` and every field is public, which is the only door into
/// the archive that does not need a running app.
fn archived(title: &str, actions: &[ArchivedAction]) -> NotificationEntry {
    NotificationEntry {
        id: 0,
        severity: teksilo_core::styles::BannerSeverity::Warning,
        priority: teksilo_core::styles::ToastPriority::Normal,
        title: title.to_string(),
        body: None,
        actions: actions.to_vec(),
        timestamp: jiff::Timestamp::UNIX_EPOCH,
        group: None,
        source: None,
        read: false,
        dedup_id: None,
        updates: Vec::new(),
        route: teksilo_widgets::toast::ToastRoute::Broadcast,
    }
}

fn list_model() -> ListModel<String> {
    ListModel::from_vec(
        (0..12)
            .map(|i| format!("Item {i}"))
            .collect::<Vec<String>>(),
    )
}

fn tree_model() -> TreeModel<String> {
    let model = TreeModel::new();
    for i in 0..4 {
        let parent = model.insert_root(i, format!("Group {i}"));
        for j in 0..3 {
            model.insert_child(parent, j, format!("Leaf {i}.{j}"));
        }
    }
    model
}

// =========================================================================
// The fixture list
// =========================================================================

fn fixtures() -> Vec<TargetFixture> {
    vec![
        // ---- Controls: the sweep P24 owns -------------------------------
        TargetFixture::new("button", |t| in_tappable_row(t, Button::new(lit!("Save")))),
        TargetFixture::new("button/filled", |t| {
            in_tappable_row(
                t,
                Button::new(lit!("Save")).variant(teksilo_core::styles::ButtonVariant::Filled),
            )
        }),
        TargetFixture::new("icon_button", |t| {
            in_tappable_row(t, IconButton::new(IconWidget::checkmark(16.0)))
        }),
        TargetFixture::new("icon_button/toolbar", |t| {
            in_tappable_row(t, IconButton::new(IconWidget::checkmark(16.0)).toolbar())
        }),
        TargetFixture::new("checkbox", |t| {
            in_tappable_row(
                t,
                Checkbox::new(Signal::new(true)).label(lit!("Wrap lines")),
            )
        }),
        TargetFixture::new("radio_button", |t| {
            in_tappable_row(
                t,
                RadioButton::new(0, Signal::new(0usize)).label(lit!("Left")),
            )
        }),
        TargetFixture::new("toggle", |t| {
            in_tappable_row(t, Toggle::new(Signal::new(true)))
        }),
        TargetFixture::new("slider", |t| {
            on_a_page(
                t,
                teksilo_widgets::primitives::FixedSize::new()
                    .width(200.0)
                    .child(Slider::new(Signal::new(0.4f32), 0.0, 1.0)),
            )
        }),
        TargetFixture::new("segmented_control", |t| {
            on_a_page(
                t,
                SegmentedControl::indexed(Signal::new(0usize))
                    .segment(Segment::new(lit!("Day")))
                    .segment(Segment::new(lit!("Week")))
                    .segment(Segment::new(lit!("Month"))),
            )
        }),
        TargetFixture::new("combo_box", |t| {
            on_a_page(
                t,
                ComboBox::new(
                    ["One", "Two", "Three"],
                    Signal::new(Some("One".to_string())),
                ),
            )
        }),
        TargetFixture::new("spin_box", |t| {
            on_a_page(
                t,
                SpinBox::new(Signal::new(12.5f64), 0.0f64, 100.0f64)
                    .single_step(0.5)
                    .label(lit!("Line height")),
            )
        }),
        TargetFixture::new("link", |t| {
            in_tappable_row(t, Link::new(lit!("Read the guide")))
        }),
        TargetFixture::new("badge", |t| in_tappable_row(t, Badge::new(lit!("draft")))),
        TargetFixture::new("avatar", |t| {
            in_tappable_row(t, Avatar::with_initials(lit!("CJ")).on_activate_fn(|_| {}))
        }),
        TargetFixture::new("progress_bar", |t| on_a_page(t, ProgressBar::new(0.6))),
        TargetFixture::new("spinner", |t| on_a_page(t, Spinner::new(20.0))),
        TargetFixture::new("split_button", |t| {
            on_a_page(
                t,
                SplitButton::new()
                    .item(MenuItem::new(lit!("Run")))
                    .item(MenuItem::new(lit!("Run tests"))),
            )
        }),
        TargetFixture::new("breadcrumb", |t| {
            on_a_page(
                t,
                Breadcrumb::new()
                    .item(BreadcrumbItem::new(lit!("Home")))
                    .item(BreadcrumbItem::new(lit!("Projects")))
                    .item(BreadcrumbItem::current(lit!("Chapter 1"))),
            )
        }),
        TargetFixture::new("stepper", |t| {
            on_a_page(
                t,
                Stepper::new()
                    .step(
                        Step::new(lit!("Details"))
                            .content(|| TextWidget::new(lit!("Name and e-mail"))),
                    )
                    .step(
                        Step::new(lit!("Review"))
                            .content(|| TextWidget::new(lit!("Check the details"))),
                    ),
            )
        }),
        TargetFixture::new("radio_tile", |t| {
            on_a_page(t, {
                let selected = Signal::new(0usize);
                VStack::new()
                    .spacing(8.0)
                    .child(
                        RadioTile::new()
                            .selection(0, selected.clone())
                            .title(lit!("Compact")),
                    )
                    .child(
                        RadioTile::new()
                            .selection(1, selected)
                            .title(lit!("Comfortable")),
                    )
            })
        }),
        // ---- Inputs -----------------------------------------------------
        TargetFixture::new("text_input", |t| {
            on_a_page(t, TextInput::new(Signal::new("Ada".to_string())))
        }),
        TargetFixture::new("search_field", |t| {
            on_a_page(t, SearchField::new(Signal::new("chapter".to_string())))
        }),
        TargetFixture::new("password_field", |t| {
            on_a_page(t, PasswordField::new(Signal::new("secret".to_string())))
        }),
        TargetFixture::new("date_edit", |t| {
            on_a_page(
                t,
                DateEdit::new(Signal::new(Some(Date::constant(2026, 3, 14)))),
            )
        }),
        TargetFixture::sized("calendar", 400.0, 460.0, |t| {
            on_a_page(
                t,
                Calendar::single(Signal::new(Some(Date::constant(2026, 3, 14)))),
            )
        }),
        // `alpha_enabled`, because the alpha strip is the second of the two
        // near-identical widgets that report a track-plus-thumb split from
        // `Widget::target_regions`, and without it only the hue strip is
        // measured.
        TargetFixture::sized("color_picker", 900.0, 700.0, |t| {
            on_a_page(
                t,
                ColorPicker::new(Signal::new(Color::from_hex("#3584E4"))).alpha_enabled(true),
            )
        }),
        // `on_activate_fn` on every swatch, because `ColorSwatch::hit_outset`
        // returns zero without one — "a widened node that then ignores the press
        // is a hole punched in the grid behind it", as the widget says. A
        // swatch with no callback is still focusable, so it is still measured;
        // it is simply measured with its declared mechanism switched off, which
        // is a number about nothing. The ColorPicker's own grid wires one.
        TargetFixture::new("color_swatch_row", |t| {
            in_tappable_row(
                t,
                HStack::new()
                    .spacing(6.0)
                    .child(
                        ColorSwatch::new(Color::from_hex("#3584E4"))
                            .size(22.0)
                            .on_activate_fn(|_| {}),
                    )
                    .child(
                        ColorSwatch::new(Color::from_hex("#2EC27E"))
                            .size(22.0)
                            .on_activate_fn(|_| {}),
                    )
                    .child(
                        ColorSwatch::new(Color::from_hex("#E01B24"))
                            .size(22.0)
                            .on_activate_fn(|_| {}),
                    ),
            )
        }),
        // ---- Rows and the chevron column --------------------------------
        TargetFixture::new("standard_list_item", |t| {
            on_a_page(
                t,
                VStack::new()
                    .child(StandardListItem::new(lit!("First")).checkbox(Signal::new(false)))
                    .child(StandardListItem::new(lit!("Second")).checkbox(Signal::new(true))),
            )
        }),
        TargetFixture::new("standard_tree_item", |t| {
            on_a_page(
                t,
                VStack::new()
                    .child(
                        StandardTreeItem::new(lit!("Group"))
                            .depth(0)
                            .has_children(true)
                            .is_expanded(true)
                            .checkbox(Signal::new(false))
                            .on_toggle(|_| {}),
                    )
                    .child(
                        StandardTreeItem::new(lit!("Leaf"))
                            .depth(1)
                            .checkbox(Signal::new(true)),
                    ),
            )
        }),
        TargetFixture::new("twist_arrow_in_a_row", |t| {
            in_tappable_row(t, TwistArrow::new(12.0, true, false).on_click(|_| {}))
        }),
        // ---- Menus ------------------------------------------------------
        TargetFixture::new("menu_list", |t| {
            on_a_page(
                t,
                MenuList::new()
                    .item(MenuItem::new(lit!("Open")))
                    .item(MenuItem::new(lit!("Save")).checked(Signal::new(true)))
                    .separator()
                    .item(MenuItem::new(lit!("Close"))),
            )
        }),
        // ---- Containers and chrome --------------------------------------
        TargetFixture::new("accordion", |t| {
            on_a_page(
                t,
                Accordion::new(lit!("Details"), Signal::new(true))
                    .content(TextWidget::new(lit!("Body"))),
            )
        }),
        TargetFixture::new("tool_box", |t| {
            on_a_page(
                t,
                ToolBox::new(Signal::new(0usize))
                    .item(lit!("Palette"), TextWidget::new(lit!("Tools")))
                    .item(lit!("Layers"), TextWidget::new(lit!("Layers"))),
            )
        }),
        TargetFixture::new("group_box", |t| {
            on_a_page(
                t,
                GroupBox::new(lit!("Options"))
                    .child(Checkbox::new(Signal::new(true)).label(lit!("Wrap"))),
            )
        }),
        TargetFixture::sized("splitter", 600.0, 400.0, |t| {
            let model = SplitterModel::new(2, Orientation::Horizontal);
            t.add(
                Splitter::new(model)
                    .pane(RectWidget::new().background(Color::from_hex("#DDDDDD")))
                    .pane(RectWidget::new().background(Color::from_hex("#EEEEEE"))),
            )
        }),
        TargetFixture::sized("scroll_area", 400.0, 300.0, |t| {
            t.add(ScrollArea::new().child(
                VStack::new().children((0..40).map(|i| TextWidget::new(lit!(format!("Line {i}"))))),
            ))
        }),
        TargetFixture::new("tab_widget", |t| {
            let selected = Signal::new(None);
            t.add(
                TabWidget::new(selected)
                    .tab(lit!("Draft"), TextWidget::new(lit!("Draft body")))
                    .tab(lit!("Notes"), TextWidget::new(lit!("Notes body"))),
            )
        }),
        // ---- Data views -------------------------------------------------
        // With a `SelectionModel`, because without one **no row is a pointer
        // target**: `ListView` installs the row wrapper's press handler inside
        // `if let Some(ref sel) = self.row_selection`
        // (`list_view/body_pane.rs`), where `TreeView`'s sibling call installs
        // it unconditionally. A selection-less fixture measured the 400 x 300
        // view and its scroll bar and nothing else — a comfortable number about
        // nothing, and the exact failure this file's `EXPECTATIONS` table
        // exists to catch.
        TargetFixture::sized("list_view", 400.0, 300.0, |t| {
            let model = list_model();
            t.add(
                ListView::new(model, |_index, item: &String, _selected| {
                    Box::new(StandardListItem::new(lit!(item.clone())))
                })
                .selection(SelectionModel::new(SelectionMode::Single)),
            )
        }),
        // `new_with_context` + `on_toggle_rc` is what all three shipped
        // examples wire, and it is what makes the chevron a target at all:
        // `TwistArrow::hit_outset` returns zero without an `on_click`, so a
        // `TreeView::new` fixture measures a tree with no chevron in it. That
        // is the shape of a gate that is green because it never looked.
        TargetFixture::sized("tree_view", 400.0, 300.0, |t| {
            let model = tree_model();
            t.add(TreeView::new_with_context(
                model,
                |item: &String, entry: &teksilo_data::FlatEntry, _selected, cx| {
                    Box::new(
                        StandardTreeItem::new(lit!(item.clone()))
                            .from_entry(entry)
                            .on_toggle_rc(cx.toggle_callback()),
                    )
                },
            ))
        }),
        TargetFixture::sized("tree_table_view", 520.0, 300.0, |t| {
            use teksilo_widgets::table_view::{Column, ColumnWidth};
            let model = tree_model();
            let view = TreeTableView::new(model)
                .add_column(
                    Column::new("name", lit!("Name"), |item: &String, _cx| {
                        Box::new(TextWidget::new(lit!(item.clone())))
                    })
                    .width(ColumnWidth::Flex(1.0)),
                )
                .add_column(
                    Column::new("kind", lit!("Kind"), |_: &String, _cx| {
                        Box::new(TextWidget::new(lit!("text")))
                    })
                    .width(ColumnWidth::Fixed(90.0)),
                );
            view.expand_all();
            t.add(view)
        }),
        // One column is `filterable`, because `HeaderCell::target_regions`
        // returns an EMPTY list otherwise (`header_cell_zones` gives up on a
        // zero-width filter zone) — and with it go the label zone AND the resize
        // grip, the one `Grab` a table reports. Measured: without this the whole
        // hook can be deleted with the gate still green.
        TargetFixture::sized("table_view", 500.0, 300.0, |t| {
            let model = list_model();
            t.add(
                TableView::new(model)
                    .add_column(
                        teksilo_widgets::table_view::Column::new(
                            "name",
                            lit!("Name"),
                            |item: &String, _cx| Box::new(TextWidget::new(lit!(item.clone()))),
                        )
                        .filterable(true),
                    )
                    .add_column(teksilo_widgets::table_view::Column::new(
                        "kind",
                        lit!("Kind"),
                        |_: &String, _cx| Box::new(TextWidget::new(lit!("text"))),
                    )),
            )
        }),
        TargetFixture::sized("grid_view", 500.0, 300.0, |t| {
            let model = list_model();
            t.add(
                GridView::new(model, |tc| Box::new(TextWidget::new(lit!(tc.item.clone())))).sizing(
                    GridSizing::Fixed {
                        width: 80.0,
                        height: 60.0,
                    },
                ),
            )
        }),
        // ---- Overlays and feedback --------------------------------------
        TargetFixture::new("banner", |t| {
            on_a_page(
                t,
                Banner::warning(lit!("Trial ends in 3 days")).action(Button::new(lit!("Add key"))),
            )
        }),
        TargetFixture::new("snackbar", |t| {
            on_a_page(
                t,
                Snackbar::new(lit!("Undo"))
                    .content(TextWidget::new(lit!("Draft saved")))
                    .trigger(Button::new(lit!("Save"))),
            )
        }),
        TargetFixture::new("drop_zone", |t| {
            on_a_page(
                t,
                teksilo_widgets::primitives::FixedSize::new()
                    .width(300.0)
                    .height(160.0)
                    .child(DropZone::new(lit!("Drop files here"))),
            )
        }),
        // A `DropTarget` wraps a child that stays visible and keeps its own
        // affordances — `examples/drag_and_drop` wraps a whole `ListView` in
        // one. So the claim worth holding is that the wrapper does not swallow
        // its child's target, which is what a wrapping container could plausibly
        // break. The wrapper's own node is deliberately *not* a press target:
        // it installs `on_drag_hover` / `on_drop` and nothing else, and a drop
        // is not a press.
        TargetFixture::new("drop_target", |t| {
            on_a_page(
                t,
                teksilo_widgets::primitives::FixedSize::new()
                    .width(300.0)
                    .height(160.0)
                    .child(
                        DropTarget::new()
                            .accept_any()
                            .on_drop(|_, _, _| true)
                            .child(
                                VStack::new()
                                    .spacing(8.0)
                                    .child(TextWidget::new(lit!("Playlist")))
                                    .child(Button::new(lit!("Clear"))),
                            ),
                    ),
            )
        }),
        // ---- Chrome, shells and window furniture ------------------------
        TargetFixture::new("toolbar", |t| {
            on_a_page(
                t,
                Toolbar::new()
                    .child(
                        Button::new(lit!("New"))
                            .variant(teksilo_core::styles::ButtonVariant::Ghost),
                    )
                    .child(
                        Button::new(lit!("Open\u{2026}"))
                            .variant(teksilo_core::styles::ButtonVariant::Ghost),
                    )
                    .child(IconButton::new(IconWidget::checkmark(16.0)).toolbar()),
            )
        }),
        TargetFixture::new("menu_bar", |t| {
            on_a_page(
                t,
                MenuBar::new()
                    .no_dispatcher_install()
                    .menu(lit!("&File"), || {
                        Box::new(
                            MenuList::new()
                                .item(MenuItem::new(lit!("&New")))
                                .item(MenuItem::new(lit!("&Open\u{2026}"))),
                        )
                    })
                    .menu(lit!("&Edit"), || {
                        Box::new(MenuList::new().item(MenuItem::new(lit!("&Undo"))))
                    }),
            )
        }),
        // The one fixture whose subject is four `ResizeStrip`s and four
        // corners: a 6 dp painted band whose whole conformance is
        // `Widget::hit_outset`, and whose parent is the frame itself, which is
        // the window. A hugging wrapper would make the declaration inert — see
        // the chevron entry in `ALLOW_LIST`.
        TargetFixture::sized("window_frame", 600.0, 400.0, |t| {
            t.add(
                WindowFrame::new(Rc::new(NoopTitleBarHost)).content(
                    VStack::new()
                        .spacing(8.0)
                        .child(TextWidget::new(lit!("Title")))
                        .child(Button::new(lit!("Body button"))),
                ),
            )
        }),
        TargetFixture::sized("docking", 900.0, 520.0, |t| {
            use teksilo_widgets::docking::{
                DockOpenLocation, DockSide, DockWidget, DockWidgetId, DockingLayout, DockingModel,
            };
            let model = DockingModel::new();
            model.set_side_rail(DockSide::Leading, 44.0);
            let explorer = DockWidgetId::fresh();
            let problems = DockWidgetId::fresh();
            let layout = DockingLayout::new(model.clone())
                .center(TextWidget::new(lit!("Editor")))
                .dock(DockWidget::new(explorer, lit!("Explorer"), |_| {
                    VStack::new()
                        .spacing(4.0)
                        .child(TextWidget::new(lit!("src")))
                        .child(TextWidget::new(lit!("docs")))
                }))
                .dock(DockWidget::new(problems, lit!("Problems"), |_| {
                    TextWidget::new(lit!("0 errors"))
                }));
            model.open_dock(explorer, DockOpenLocation::side(DockSide::Leading));
            model.open_dock(problems, DockOpenLocation::side(DockSide::Bottom));
            t.add(layout)
        }),
        // ---- The residue primitive, in the one composition that ships ----
        //
        // `TouchTarget` does not widen its child: it grows the **slot** so the
        // child's own `hit_outset` stops being hugged by a wrapper that fits it
        // exactly. That is the whole mechanism, and it is what the inspector's
        // resize strip uses (`teksilo-inspector/src/shell.rs`, the only
        // `TouchTarget` in the tree). So the child here has to be one that
        // declares an outset — a chevron does — or the fixture measures a
        // primitive doing nothing and reports a number about nothing.
        //
        // Not in a tappable row on purpose: the inspector's strip is not in
        // one, and at Compact the wrapper is documented to be the identity, so
        // a row there would only measure the absence of a mechanism that is
        // absent by design.
        TargetFixture::new("touch_target", |t| {
            on_a_page(
                t,
                TouchTarget::new().child(TwistArrow::new(16.0, true, false).on_click(|_| {})),
            )
        }),
        // ---- The remaining families the programme touched ----------------
        TargetFixture::new("tab_widget/closable", |t| {
            let selected = Signal::new(None);
            t.add(
                TabWidget::new(selected)
                    .static_tab(
                        TabInfo::new().title(lit!("Draft")).closable(true),
                        TextWidget::new(lit!("Draft body")),
                    )
                    .static_tab(
                        TabInfo::new().title(lit!("Notes")).closable(true),
                        TextWidget::new(lit!("Notes body")),
                    ),
            )
        }),
        TargetFixture::sized("log_view", 520.0, 220.0, |t| {
            let view = LogView::new();
            let handle = view.handle();
            let id = t.add(view);
            handle.append_lines([
                "12:04:01  INFO   watching 1 240 files",
                "12:04:07  WARN   2 unresolved references",
                "12:04:09  ERROR  export failed",
            ]);
            id
        }),
        TargetFixture::new("time_edit", |t| {
            on_a_page(
                t,
                TimeEdit::new(Signal::new(Some(Time::constant(9, 30, 0, 0)))),
            )
        }),
        TargetFixture::sized("dialog", 600.0, 400.0, |t| {
            use teksilo_widgets::dialog::{DialogContent, ModalContainer};
            t.add(
                ModalContainer::new(
                    DialogContent::new()
                        .title(lit!("Discard changes?"))
                        .supporting_text(lit!("Chapter 3 has unsaved edits."))
                        .footer(
                            HStack::new()
                                .spacing(8.0)
                                .child(Button::new(lit!("Keep editing")))
                                .child(Button::new(lit!("Discard"))),
                        ),
                )
                .min_width(380.0),
            )
        }),
        TargetFixture::new("notification_center_button", |t| {
            let archive = Rc::new(NotificationArchiveModel::in_memory());
            on_a_page(t, NotificationCenterButton::new(archive))
        }),
        // ---- The controls the sweep changed and the list had missed --------
        TargetFixture::new("command_link_button", |t| {
            in_tappable_row(
                t,
                CommandLinkButton::new(lit!("Create a project"))
                    .description(lit!("Start from an empty folder"))
                    .icon(IconWidget::checkmark(16.0)),
            )
        }),
        TargetFixture::new("date_time_edit", |t| {
            on_a_page(
                t,
                DateTimeEdit::new(Signal::new(Some(
                    Date::constant(2026, 3, 14).at(9, 30, 0, 0),
                ))),
            )
        }),
        // The window controls, which `window_frame` does not build: its host
        // supplies the strips, and the cluster is `TitleBar`'s.
        TargetFixture::sized("title_bar", 700.0, 200.0, |t| {
            t.add(
                VStack::new()
                    .child(
                        TitleBar::new(Rc::new(NoopTitleBarHost))
                            .center(TextWidget::new(lit!("Chapter 3 \u{2014} Teksilo"))),
                    )
                    .child(TextWidget::new(lit!("Body"))),
            )
        }),
        // The two overflow chevrons the programme's own `sized` doc anticipates
        // ("deliberately little of it") and no fixture had used: a command bar
        // and a segmented control both collapse their surplus into a trailing
        // `\u{2304}`, and that chevron is a target the wide fixtures never build.
        TargetFixture::sized("toolbar/overflowing", 150.0, 160.0, |t| {
            let root = on_a_page(
                t,
                Toolbar::new()
                    .action(ToolbarAction::new(lit!("New"), || {
                        IconWidget::checkmark(16.0)
                    }))
                    .action(ToolbarAction::new(lit!("Open"), || {
                        IconWidget::checkmark(16.0)
                    }))
                    .action(ToolbarAction::new(lit!("Save"), || {
                        IconWidget::checkmark(16.0)
                    }))
                    .action(ToolbarAction::new(lit!("Export"), || {
                        IconWidget::checkmark(16.0)
                    }))
                    .action(ToolbarAction::new(lit!("Print"), || {
                        IconWidget::checkmark(16.0)
                    }))
                    .action(ToolbarAction::new(lit!("Share"), || {
                        IconWidget::checkmark(16.0)
                    })),
            );
            // Two passes, deliberately. Both overflow chevrons gate their
            // trigger on an `is_overflowing` signal that is published from
            // `place_children` and bound at `Relayout`, so after one layout the
            // trigger is still dormant and a one-pass fixture measures a bar
            // that never collapsed. The driver lays out again after this
            // returns, which is the pass that admits the chevron.
            t.layout(teksilo_canvas::SizeProposal::exact(150.0, 160.0));
            root
        }),
        TargetFixture::sized("segmented_control/overflowing", 220.0, 160.0, |t| {
            let root = on_a_page(
                t,
                SegmentedControl::indexed(Signal::new(0usize))
                    .overflow(SegmentOverflow::Menu)
                    .segment(Segment::new(lit!("Outline")))
                    .segment(Segment::new(lit!("Manuscript")))
                    .segment(Segment::new(lit!("Research")))
                    .segment(Segment::new(lit!("Timeline")))
                    .segment(Segment::new(lit!("Statistics"))),
            );
            // See `toolbar/overflowing` for why this lays out twice.
            t.layout(teksilo_canvas::SizeProposal::exact(220.0, 160.0));
            root
        }),
        // A popover trigger: `overlay_trigger.rs` is the one press path that
        // opens an overlay instead of acting, and it is the shape every
        // dropdown in the catalog is built from.
        TargetFixture::new("popover_button", |t| {
            in_tappable_row(
                t,
                PopoverButton::new(Button::new(lit!("Options")))
                    .content(TextWidget::new(lit!("Panel"))),
            )
        }),
        // A live toast's own surface, which no other fixture reaches.
        //
        // `ToastRegistry::enqueue` is `pub(crate)` — the app-facing door is
        // `EventContext::show_toast`, which needs the registry installed in
        // app state — so this uses the one public no-context enqueue there is,
        // `show_settings_write_failed`. What that lands is a persistent
        // high-priority error toast: its surface, its severity glyph and its
        // close button, but **no action row**, since that toast sets no
        // actions. A toast action is an ordinary `Button` / `Link` in an
        // `HStack`, which `banner` measures, and the archived replay rows are
        // measured by `notification_log` below.
        TargetFixture::sized("toast", 520.0, 320.0, |t| {
            let options = ToastInstallOptions {
                archive: None,
                ..ToastInstallOptions::default()
            };
            let registry = ToastRegistry::new(options.clone());
            registry.show_settings_write_failed("settings.toml", 3, 1, "disk full");
            let host = t.add(ToastHost::new(registry, options));
            t.add(
                teksilo_widgets::primitives::ZStack::new()
                    .child(TextWidget::new(lit!("Page")))
                    .add_child(host),
            )
        }),
        TargetFixture::sized("notification_log", 420.0, 420.0, |t| {
            let archive = Rc::new(NotificationArchiveModel::in_memory());
            archive.push(archived(
                "2 unresolved references",
                &[ArchivedAction {
                    label: "Show".to_string(),
                    intent_name: Some("app.show_problems".to_string()),
                    style: ArchivedActionStyle::Link,
                    closes_on_invoke: false,
                }],
            ));
            archive.push(archived("Export failed", &[]));
            // With both callbacks wired, because without them the log's rows
            // are **not targets**: `build_row` installs the row's `on_tap` only
            // for `on_entry_invoked` and `build_actions_row` its replay handlers
            // only for `on_action_invoked` (`notification/log.rs`), so a
            // callback-less log measures its toolbar and nothing else.
            // `NotificationLogDialog` wires both.
            on_a_page(
                t,
                NotificationLog::new(archive)
                    .on_entry_invoked(|_, _| {})
                    .on_action_invoked(|_, _, _| {}),
            )
        }),
        // `code_editor/touch.rs` is a pointer surface the composites round
        // changed and the list had no entry for. Two shapes, because only one of
        // them can fail:
        //
        // A full-size editor is **one** target by design — the caret is placed
        // by coordinate inside the surface, and the gutter paints but takes no
        // press — so the fixture's claim is that the surface itself stays a
        // target, not that it has sub-targets. Its touch selection *handles* are
        // a `teksilo_core::text_touch` mechanism with no arena node, so they are
        // audited there and cannot be audited here.
        TargetFixture::sized("code_editor", 520.0, 320.0, |t| {
            let document = teksilo_text::text_document::TextDocument::new();
            t.add(CodeEditor::new(document).gutter(true))
        }),
        // A one-line intrinsic editor is where target size actually bites: the
        // messenger-composer shape (`min_lines`/`max_lines`, documented on
        // `RichTextEditor::editor` and `PlainTextEditor`) puts a whole editable
        // surface inside a single line box, which is under the floor before any
        // mechanism runs.
        TargetFixture::sized("plain_text_editor/one_line", 420.0, 200.0, |t| {
            let document = teksilo_text::text_document::TextDocument::new();
            on_a_page(t, PlainTextEditor::new(document).min_lines(1).max_lines(1))
        }),
        // The rich-text surface, absent from the list entirely: the largest
        // pointer surface in the catalog and the base of every text field.
        TargetFixture::sized("rich_text_editor", 520.0, 320.0, |t| {
            let document = teksilo_text::text_document::TextDocument::new();
            t.add(teksilo_widgets::rich_text::RichTextEditor::editor(document))
        }),
    ]
}

/// A platform host that answers every question and does nothing — enough to
/// build a `WindowFrame` and its strips in a headless tree.
#[derive(Debug)]
struct NoopTitleBarHost;

impl teksilo_core::window_chrome::PlatformTitleBarHost for NoopTitleBarHost {
    fn reserved_leading_inset(&self) -> Size {
        Size::ZERO
    }
    fn reserved_trailing_inset(&self) -> Size {
        Size::ZERO
    }
    fn renders_custom_controls(&self) -> bool {
        true
    }
    fn needs_custom_resize_handles(&self) -> bool {
        true
    }
    fn begin_drag(&self) -> Result<(), teksilo_core::PlatformError> {
        Ok(())
    }
    fn begin_resize(
        &self,
        _edge: teksilo_core::window_chrome::ResizeEdge,
    ) -> Result<(), teksilo_core::PlatformError> {
        Ok(())
    }
    fn show_window_menu(&self, _at: Point) -> Result<(), teksilo_core::PlatformError> {
        Ok(())
    }
    fn update_hit_regions(&self, _regions: &teksilo_core::HitRegions) {}
}

// =========================================================================
// The gate
// =========================================================================

const ALL_DENSITIES: &[TargetDensity] = &[
    TargetDensity::Compact,
    TargetDensity::Comfortable,
    TargetDensity::Touch,
];
const AT_COMPACT: &[TargetDensity] = &[TargetDensity::Compact];
const AT_COMFORTABLE: &[TargetDensity] = &[TargetDensity::Comfortable];
const AT_TOUCH: &[TargetDensity] = &[TargetDensity::Touch];
const ABOVE_COMPACT: &[TargetDensity] = &[TargetDensity::Comfortable, TargetDensity::Touch];

/// The findings, each measured, none of them a mechanism that could be switched
/// on to clear it.
///
/// The counts a reader needs are asserted rather than written here: see
/// [`the_allow_lists_roster_is_what_it_says_it_is`], which pins how many entries
/// there are, how many rest on an exception, and how many are escalated. The
/// enumeration of Compact-visible exceptions the programme has taken lives in
/// `docs/density-inventory.md` §0 and nowhere else, for the same reason — a
/// count in prose is a count that drifts.
///
/// Two shapes recur. A control that paints under 24 dp on one axis **packed
/// against another target of its own kind** has no point left for either
/// widening mechanism to claim (the step buttons, the swatches, the window
/// corners). And a control whose declared `Widget::hit_outset` is **hugged by a
/// wrapper** is offered no point outside its own box, because the arena consults
/// the hook only among a node's direct children and returns before it looks at
/// one whose parent's rectangle excludes the point — A10's reach limit, and the
/// tree chevron is the shipped instance of it.
///
/// All of them are real WCAG 2.2 SC 2.5.8 (AA) shortfalls. They are here rather
/// than fixed because every remedy left is a **layout** change at Compact, and
/// the programme's invariant is that Compact does not move: taking another such
/// exception is a decision for the owner named in each entry, not for the audit
/// that found it.
const ALLOW_LIST: &[AllowedViolation] = &[
    AllowedViolation {
        path: "StepButton",
        measured: &[PinnedGeometry {
            densities: ALL_DENSITIES,
            paints: (Is(18.0), Is(13.0)),
            reaches: (Is(18.0), Is(13.0)),
        }],
        owner: "whoever revisits SpinBox's step geometry",
        exception: Some("Equivalent"),
        why: "A SpinBox's two step buttons are stacked halves of the field's \
              `TEXT_FIELD_HEIGHT` box, and the pin says what they get from the \
              two widening mechanisms: nothing at all, at every density -- the \
              reach IS the paint. Vertically there is nothing to claim, because \
              the two are adjacent and two rings meet at the midpoint; \
              horizontally an outset could reach the floor on that axis alone, \
              which would not clear the violation. WCAG 2.2 SC 2.5.8 \
              *Equivalent*: the same value is set by typing in the field, by \
              Up/Down, and by the wheel. docs/density-inventory.md named \
              partition_targets as this row's mechanism, which cannot be right \
              -- the step buttons are their own nodes beside their own sibling, \
              the same error P25 corrected for the SplitButton chevron; that \
              row is corrected.",
    },
    AllowedViolation {
        path: "ColorSwatch",
        measured: &[
            // The ColorPicker's grid: rows are flush, so the vertical earns
            // nothing and the height is what falls short.
            PinnedGeometry {
                densities: AT_COMPACT,
                paints: (Is(22.0), Is(22.0)),
                reaches: (Is(23.0), Is(22.0)),
            },
            PinnedGeometry {
                densities: AT_COMPACT,
                paints: (Is(22.0), Is(22.0)),
                reaches: (Is(24.0), Is(22.0)),
            },
            PinnedGeometry {
                densities: ABOVE_COMPACT,
                paints: (Is(22.0), Is(22.0)),
                reaches: (Is(25.0), Is(22.0)),
            },
            PinnedGeometry {
                densities: ABOVE_COMPACT,
                paints: (Is(22.0), Is(22.0)),
                reaches: (Is(28.0), Is(22.0)),
            },
            // A single HStack run: the height earns its dp, and the two
            // swatches at the ends of the run are flush against the run's own
            // edge, so the width is what falls short -- at Compact only.
            PinnedGeometry {
                densities: AT_COMPACT,
                paints: (Is(22.0), Is(22.0)),
                reaches: (Is(23.0), Is(24.0)),
            },
        ],
        owner: "whoever revisits ColorPicker's grid geometry",
        exception: Some("Equivalent"),
        why: "A 22 dp swatch, 2 dp under the floor, whose ring earns back at \
              most 1 dp per side because every neighbour is a swatch: where two \
              rings meet the arena splits the gap halfway. Which axis falls \
              short depends on the run it is in, and both shapes are pinned \
              above -- flush rows in the ColorPicker's grid leave the height \
              short at every density, and the ends of a single HStack run leave \
              the width short at Compact only, since Comfortable's wider ring \
              clears it. WCAG 2.2 SC 2.5.8 *Equivalent*: the ColorPicker sets \
              the same value through its H/S/V spin boxes and its hex field. \
              The geometric remedy is a further docs/density-inventory.md \u{a7}0 \
              entry -- SWATCH_SIZE 22 -> 24 dp, Compact included, exactly as \
              the previewer's navigator row and the macOS control height were \
              raised -- which would clear both shapes at once.",
    },
    AllowedViolation {
        path: "Expand > Padding > TextInputField",
        measured: &[PinnedGeometry {
            densities: AT_COMPACT,
            paints: (ClearsFloor, Is(20.0)),
            reaches: (ClearsFloor, Is(20.0)),
        }],
        owner: "whoever revisits SpinBox's frame inset (the same owner as the \
                step buttons above)",
        exception: None,
        why: "A SpinBox's editable field is 20 dp tall inside the frame's own \
              box because the frame reserves space above and below it, and it \
              reaches exactly that 20 dp -- the width clears the floor and is \
              not the finding. Compact only: at Comfortable and Touch the box's \
              own MinSize carries the field to 24 and 36 dp, which is the \
              density ladder working. Neither mechanism can serve it: the step \
              buttons beside it are targets at distance zero, so the slop pass \
              is denied, and an outset would have to grow into the frame it is \
              inset from. A real AA shortfall with no exception behind it -- \
              the field IS the target for placing a caret, and no conforming \
              control does that job. The remedy is a taller field at Compact, \
              which the invariant forbids.",
    },
    AllowedViolation {
        path: "HStack > FixedSize > TwistArrow",
        measured: &[
            // In a TreeView the wrapper hugs the chevron and no mechanism is
            // credited: the reach is the paint at every density.
            PinnedGeometry {
                densities: ALL_DENSITIES,
                paints: (Is(16.0), Is(16.0)),
                reaches: (Is(16.0), Is(16.0)),
            },
            // A single StandardTreeItem on a page has no tappable row around
            // it, so the miss-only pass reaches this one -- and still leaves the
            // width short at Compact. A different geometry, named rather than
            // absorbed by the entry above.
            PinnedGeometry {
                densities: AT_COMPACT,
                paints: (Is(16.0), Is(16.0)),
                reaches: (Is(20.0), Is(24.0)),
            },
        ],
        owner: "whoever revisits the tree row's indent column",
        exception: None,
        why: "The tree chevron's outset is INERT in the one composition it \
              ships in, and this key names the reason: \
              `StandardTreeItem::build` wraps it in \
              `FixedSize::new().width(chevron_size)`, a wrapper exactly the \
              chevron's own size on both axes. An outset is only ever offered \
              points every ancestor's rectangle already contains, so a hugged \
              grip claims nothing -- A10's reach limit, measured here in a \
              shipped widget rather than the inspector. \
              `the_chevrons_outset_is_inert_inside_a_standard_tree_row` pins \
              both halves: the reach equals the paint with no mechanism \
              credited, while the same chevron unhugged in \
              `twist_arrow_in_a_row` reaches 24 x 24 through its outset. \
              Deleting the wrapper is necessary and NOT sufficient -- at depth \
              0 the chevron is flush against the row's content box, so the \
              ring's leading half falls outside the HStack and the reach is \
              still short at Compact.",
    },
    AllowedViolation {
        path: "CellA11y > Padding > HStack > TwistArrow",
        measured: &[
            PinnedGeometry {
                densities: AT_COMPACT,
                paints: (Is(12.0), Is(12.0)),
                reaches: (Is(18.0), Is(24.0)),
            },
            PinnedGeometry {
                densities: AT_COMFORTABLE,
                paints: (Is(12.0), Is(12.0)),
                reaches: (Is(22.0), Is(28.0)),
            },
        ],
        owner: "whoever revisits the tree table's indent column (the same \
                question as the entry above)",
        exception: Some("Equivalent"),
        why: "The same chevron in a TreeTableView cell, 12 dp rather than 16 \
              and NOT hugged -- its outset is credited, and the pins say by how \
              much: 6 of the 12 dp it needs at Compact, and enough at Touch \
              that the entry does not cover that density at all. What is short \
              is one axis and one side: the chevron sits at the leading edge of \
              its cell, so the ring's leading half falls outside the cell's \
              HStack. WCAG 2.2 SC 2.5.8 *Equivalent*: ArrowLeft / ArrowRight \
              expand and collapse the focused row. The row band that selects it \
              is not offered as the equivalent, because there is no node to \
              measure: a TreeTableView routes row presses from its own \
              rectangle, so the band's height is a row-height question and not \
              this audit's.",
    },
    AllowedViolation {
        path: "> Link",
        measured: &[PinnedGeometry {
            densities: ALL_DENSITIES,
            paints: (ClearsFloor, Is(17.0)),
            reaches: (ClearsFloor, Is(17.0)),
        }],
        owner: "",
        exception: Some("Inline"),
        why: "A link is text-height -- 17 dp, reaching exactly that at every \
              density, in both the `link` fixture and an archived \
              notification's replay action, with the long axis clearing the \
              floor in each. WCAG 2.2 SC 2.5.8 *Inline* -- the target's size is \
              constrained by the line height of the text it is set in -- which \
              is the second of the two legs `link.rs` already rests on. Its \
              FIRST leg is wrong and the sentence is corrected there: link.rs \
              said the miss-only slop pass reaches it, and the slop pass is \
              denied whenever the link sits in a row that takes presses, which \
              is where both of these links are. No owner: the exception is the \
              answer, not a deferral.",
    },
    AllowedViolation {
        path: "window_frame: WindowFrame > ResizeStrip",
        measured: &[PinnedGeometry {
            densities: ALL_DENSITIES,
            paints: (Is(6.0), Is(6.0)),
            reaches: (Is(6.0), Is(6.0)),
        }],
        owner: "whoever decides whether the diagonal grip should be bigger than \
                the edges it sits between",
        exception: Some("Equivalent"),
        why: "A window's four diagonal corner grips reach exactly their own \
              6 x 6 at every density, while the four EDGE strips on the same \
              path reach the floor through the same `hit_outset` -- pinned \
              together by \
              `a_window_corner_grip_is_boxed_in_by_its_own_edge_strips`. The \
              corner is boxed in by its own neighbours: the arena orders outset \
              candidates by distance to the uninflated rectangle, and a point \
              one dp inboard of a corner is already INSIDE an edge strip, at \
              distance zero. Two mutations say what the corner branch \
              (`EdgeInsets::uniform(out)`) is worth, and it is NOT the P35 \
              dead-declaration shape: deleting it costs the corner its own \
              body, which the adjacent edge then claims, and deleting the EDGE \
              branches instead lets the corners grow. So the declaration is \
              load-bearing for the grip existing at all, and powerless to grow \
              it; that named test reddens under either mutation. WCAG 2.2 \
              SC 2.5.8 *Equivalent*: the same rectangle is reached by dragging \
              the two adjacent edges, each of which conforms, and by the title \
              bar's Resize command.",
    },
    AllowedViolation {
        path: "docking: DockingLayout > SideClipPane",
        measured: &[PinnedGeometry {
            densities: AT_TOUCH,
            paints: (ClearsFloor, Is(38.0)),
            reaches: (Is(0.0), Is(0.0)),
        }],
        owner: "whoever owns A10's outset-versus-target precedence",
        exception: None,
        why: "At Touch a dock's tab strip is UNREACHABLE at the point a user \
              aims at -- a reach of exactly zero on both axes for a strip \
              38 dp tall -- and the cause is another target's ring, not its \
              size. `DockResizeHandle::hit_outset` grows a 6 dp gutter to \
              `TargetRole::Target`, 44 dp at Touch, and the strip below it \
              starts where the gutter ends, so the tab's centre line is inside \
              the ring and every press there resizes the dock instead of \
              switching tabs -- at every dock side, in every app. A10's \
              precedence chain settles an outset against ANOTHER OUTSET by \
              distance to the uninflated rectangle; it says nothing about an \
              outset against a plain target that contains the point, and that \
              is the gap. Escalated rather than papered over: the fixes on \
              offer are all mechanism changes (an outset that loses to a \
              containing target, or a gutter that inflates to `TargetRole::\
              Grab` -- 16 dp at Touch, but 6 dp at Compact, which is the \
              gutter's own thickness and would leave it no ring there, \
              reddening P30's Compact touch tests). Pinned by \
              `the_dock_gutters_touch_ring_swallows_the_tab_strip_beside_it`, \
              which also holds the other half: at Compact the same header is \
              reachable, so this is a finding about the ring and not about the \
              strip.",
    },
    AllowedViolation {
        path: "HeaderRow > HeaderCell",
        measured: &[PinnedGeometry {
            densities: ALL_DENSITIES,
            paints: (Is(4.0), ClearsFloor),
            reaches: (Is(4.0), ClearsFloor),
        }],
        owner: "whoever closes drag-operation-census rows 7 and 8",
        exception: None,
        why: "A table column's resize grip, reported from \
              `HeaderCell::target_regions` as `HEADER_PART_RESIZE`: 4 dp wide, \
              reaching exactly that at every density, with its height clearing \
              the floor. Two things make it unreachable by any mechanism. A \
              region is *measured*, not probed, so the only growth it can take \
              is what the walker confirmed for its node at an edge it shares -- \
              and its one shared edge is the cell's reading-order trailing one, \
              where the next header cell is flush and takes presses, so the \
              confirmed growth there is zero. And the 4 dp is a HALF-width by \
              construction: the type docs record that a divider is a boundary \
              between two cells and the grabbable band straddles it, \
              `resize_grip` inside each, so the divider a user aims at is two \
              cells' halves. `target_regions` reports only the trailing half, \
              so the audit reads half of the affordance; both are far under the \
              floor and the discrepancy is a reporting gap, not the shortfall. \
              NOT excused under SC 2.5.8 *Equivalent*: \
              docs/drag-operation-census.md row 7 rates this operation \
              *Partial* -- `on_access_action` Increment/Decrement by \
              `COLUMN_RESIZE_STEP` exists, and there is no keyboard route at \
              all, because the header cell is `.focusable(false)` and carries \
              no `on_key`. An AT action is not an equivalent control on the \
              same page. The remediation is already owned and is that row's \
              own: a focusable header row with a roving tab index, which rows 7 \
              and 8 share as a prerequisite.",
    },
    AllowedViolation {
        path: "OverlayTrigger > FilterIndicator",
        measured: &[PinnedGeometry {
            densities: ALL_DENSITIES,
            paints: (Is(12.0), Is(12.0)),
            reaches: (Is(12.0), Is(12.0)),
        }],
        owner: "whoever revisits the table header's filter affordance",
        exception: None,
        why: "A filterable column's filter glyph, `FILTER_INDICATOR_SIZE`, \
              reaching exactly its paint at every density. The header cell \
              around it takes presses, so the miss-only pass is denied at \
              distance zero, and neither `FilterIndicator` nor the \
              `OverlayTrigger` wrapping it declares a `Widget::hit_outset` -- \
              so the glyph is the whole target. The trigger does carry \
              `on_tap`, an `on_key` Enter/Space arm and \
              `on_access_action(Click)`, but `OverlayTrigger` sets no \
              `focusable`, so the key arm is reachable only if an ancestor \
              makes it a tab stop; this entry does not claim it does, and the \
              header cell it sits in is `.focusable(false)`. No SC 2.5.8 \
              exception is claimed either: there is no conforming equivalent \
              control for opening a column's filter. The remedy is a bigger \
              glyph or a `TouchTarget` around it, both of which move Compact \
              layout, which is the programme's invariant.",
    },
];

#[test]
fn zz_census() {
    let fixtures = fixtures();
    for density in [
        TargetDensity::Compact,
        TargetDensity::Comfortable,
        TargetDensity::Touch,
    ] {
        let mut n = 0;
        for v in audit_fixtures(&fixtures, density) {
            if !v.rule.is_conformance_failure() {
                continue;
            }
            n += 1;
            let subject = v.path.split(": ").next().unwrap_or("?").to_string();
            let leaf = v.path.rsplit(" > ").next().unwrap_or("?").to_string();
            println!(
                "CENSUS {density:?} {subject} {leaf} paints {:.1}x{:.1} reaches {:.1}x{:.1} src={}{}",
                v.size.width,
                v.size.height,
                v.expanded.width,
                v.expanded.height,
                if v.sources.outset { "outset " } else { "" },
                if v.sources.slop { "slop" } else { "" },
            );
        }
        println!("CENSUS-TOTAL {density:?} {n}");
    }
}

/// **The gate.** Zero conformance failures, at all three densities, over the
/// whole fixture list, except the written entries above.
#[test]
fn no_target_falls_below_the_conformance_floor_at_any_density() {
    let fixtures = fixtures();
    let mut failures = Vec::new();
    for density in [
        TargetDensity::Compact,
        TargetDensity::Comfortable,
        TargetDensity::Touch,
    ] {
        for violation in audit_fixtures(&fixtures, density) {
            if !violation.rule.is_conformance_failure() {
                continue;
            }
            if ALLOW_LIST.iter().any(|entry| entry.matches(&violation)) {
                continue;
            }
            failures.push(violation.to_string());
        }
    }
    assert!(
        failures.is_empty(),
        "{} target(s) below the 24 dp WCAG 2.2 SC 2.5.8 (AA) floor:\n{}",
        failures.len(),
        failures.join("\n"),
    );
}

/// Every allow-list entry, every geometry it pins, and every density it claims
/// still matches something — and the roster's own counts are what it says.
///
/// The failure mode of an allow-list is not that it is long, it is that it
/// outlives what it excused: an entry matching nothing is a hole waiting for the
/// next control whose name happens to contain the same substring. So this walks
/// the list from the other end — each [`Measured`] row must be *reached*, at
/// each density it names — which is strictly stronger than "the entry matches
/// something": an entry that pinned three geometries and only ever meets one has
/// two exemptions nothing justifies, and an entry claiming a density the control
/// actually conforms at is excusing a regression in advance.
#[test]
fn no_allow_list_entry_is_stale() {
    let fixtures = fixtures();
    let mut violations = Vec::new();
    for density in [
        TargetDensity::Compact,
        TargetDensity::Comfortable,
        TargetDensity::Touch,
    ] {
        violations.extend(
            audit_fixtures(&fixtures, density)
                .into_iter()
                .filter(|v| v.rule.is_conformance_failure()),
        );
    }
    for entry in ALLOW_LIST {
        assert!(
            !entry.why.is_empty(),
            "allow-list entry `{}` carries no justification",
            entry.path,
        );
        assert!(
            !entry.owner.is_empty() || entry.why.contains("No owner"),
            "allow-list entry `{}` names no owner and does not say why it needs \
             none — an entry is a debt with an address",
            entry.path,
        );
        assert!(
            !entry.measured.is_empty(),
            "allow-list entry `{}` pins no measurement, so it excuses whatever \
             its path happens to match",
            entry.path,
        );
        for (index, pin) in entry.measured.iter().enumerate() {
            assert!(
                !pin.densities.is_empty(),
                "`{}` pin {index} claims no density",
                entry.path,
            );
            for &density in pin.densities {
                assert!(
                    violations.iter().any(|v| v.density == density
                        && v.path.contains(entry.path)
                        && pin.covers(v)),
                    "allow-list entry `{}` pin {index} matches no conformance \
                     failure at {density:?} any more. Either the geometry moved \
                     — re-measure it and rewrite the pin — or the control now \
                     conforms there, in which case narrow the pin's densities \
                     or delete it, and its justification with it.",
                    entry.path,
                );
            }
        }
    }
}

/// The roster's counts, asserted rather than written in prose.
///
/// The paragraph over [`ALLOW_LIST`] used to carry four counts and not one held.
/// It said ten findings of which "four rest on an exception, six are escalated
/// with an owner" — the list directly beneath it said five and five. And it said
/// "the three exceptions taken so far are enumerated in
/// `docs/density-inventory.md` §0, and taking a fourth", where §0 had four rows.
/// Chasing that second pair to the code rather than to the table found the table
/// wrong as well: one of those four rows, `text_input.rs`'s clear button, no
/// longer changes Compact at all — a later package replaced its `MinSize` with a
/// `HitTarget` and a `hit_outset`, which is measured by
/// [`an_outsets_claim_survives_the_slop_pass_in_the_shipped_controls`] — so the
/// row is gone and §0 now enumerates three. The lesson is the reason these
/// numbers moved into a test: prose said three, the table said four, and the
/// code said three for a different reason than the prose did.
#[test]
fn the_allow_lists_roster_is_what_it_says_it_is() {
    assert_eq!(ALLOW_LIST.len(), 10, "ten written findings");
    let on_an_exception = ALLOW_LIST.iter().filter(|e| e.exception.is_some()).count();
    assert_eq!(
        on_an_exception,
        5,
        "five rest on an SC 2.5.8 exception: {:?}",
        ALLOW_LIST
            .iter()
            .filter(|e| e.exception.is_some())
            .map(|e| e.path)
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        ALLOW_LIST.len() - on_an_exception,
        5,
        "and five are real failures escalated to an owner",
    );
    assert_eq!(
        ALLOW_LIST.iter().filter(|e| e.owner.is_empty()).count(),
        1,
        "exactly one entry needs no owner — the *Inline* exception is the \
         answer there, not a deferral",
    );
    for entry in ALLOW_LIST {
        if let Some(name) = entry.exception {
            assert!(
                entry.why.contains(name),
                "`{}` claims the SC 2.5.8 *{name}* exception in a field its \
                 justification never mentions",
                entry.path,
            );
        }
    }
}

/// **The allow-list narrows.** A regression seeded into any geometry it excuses
/// is not excused by it.
///
/// This is the half a staleness test cannot reach. Staleness asks whether each
/// entry still matches something; this asks the opposite question — whether an
/// entry matches something it should not — and it is the question that matters,
/// because a path or a fixture-name matcher passes the first and fails the
/// second. Three of these entries were exactly that before the pins went in:
/// `Expand > Padding > TextInputField`, `> Link` and
/// `docking: DockingLayout > SideClipPane` carried no size discriminator at all,
/// so a table field, a link or a dock strip could have shrunk to two dp inside
/// an allow-listed region and the gate would have stayed green.
///
/// Every real violation the census produces is taken in turn, and each of its
/// four scalars is driven to 2 dp — a genuinely broken target, on an axis a real
/// entry pins exactly (which then disagrees) or requires to clear the floor
/// (which 2 dp does not). Nothing in the list may cover the result.
#[test]
fn the_allow_list_excuses_only_the_geometry_it_pinned() {
    let fixtures = fixtures();
    let mut seeded = 0_usize;
    for density in [
        TargetDensity::Compact,
        TargetDensity::Comfortable,
        TargetDensity::Touch,
    ] {
        for violation in audit_fixtures(&fixtures, density) {
            if !violation.rule.is_conformance_failure() {
                continue;
            }
            assert!(
                ALLOW_LIST.iter().any(|e| e.matches(&violation)),
                "the census produced a violation no entry covers, which the \
                 gate already says: {violation}",
            );
            for axis in 0..4 {
                let mut broken = violation.clone();
                match axis {
                    0 => broken.size.width = 2.0,
                    1 => broken.size.height = 2.0,
                    2 => broken.expanded.width = 2.0,
                    _ => broken.expanded.height = 2.0,
                }
                assert!(
                    !ALLOW_LIST.iter().any(|e| e.matches(&broken)),
                    "a regression seeded on axis {axis} of a real violation is \
                     still excused, so the entry covering it is a blanket over \
                     its region rather than a pin on a geometry:\n  real:   \
                     {violation}\n  seeded: {broken}",
                );
                seeded += 1;
            }
        }
    }
    assert!(
        seeded >= 400,
        "only {seeded} regressions were seeded — the census shrank, so this \
         test is proving less than it reads as proving",
    );
}

/// What each fixture must be seen to measure — by the type of the **measured
/// node**, not by a name that happens to appear in its path.
///
/// A blanket "every fixture produced *something*" is not enough: the audit's
/// characteristic failure is measuring the wrong thing, and a fixture whose
/// subject silently stopped being a target would still produce rows for the row
/// around it. So each fixture names the widget types that must appear as the
/// **last** path segment of one of its rows, and the assertion below fails if a
/// fixture is added without an entry — which is what stops the list growing
/// blind.
///
/// The leaf-exactness is the correction that matters. `path.contains(..)` was
/// satisfied by the subject's own name appearing as an ANCESTOR: the `list_view`
/// entry read `Some("ListView")` and passed because the 400 x 300 view is itself
/// a target, while the fixture (built with no `SelectionModel`) measured not one
/// row; `stepper` passed on `StepperFooter` while measuring a footer button;
/// `drop_zone` and `drop_target` passed on the wrapper's name while measuring the
/// `Button` inside. A claim about a measured leaf cannot be satisfied that way.
///
/// An **empty** slice means the subject is deliberately not a pointer target: a
/// progress bar and a spinner are output, not controls, and a badge is a label.
/// They stay in the list because a future change that made one tappable should
/// be measured, not because they have anything to measure today.
///
/// Measured at **Compact**. Two consequences worth knowing: a hover-revealed
/// affordance does not exist here (the tab close button is gated behind hover
/// below `RevealPolicy::Always`, and is measured at Touch by
/// [`a_hover_revealed_close_button_is_a_target_only_at_touch_density`]), and a
/// control whose paint is Compact-sized is the hardest case for the floor, which
/// is the density the gate cares most about.
const EXPECTATIONS: &[(&str, &[&str])] = &[
    // ---- Controls ---------------------------------------------------
    ("button", &["Button"]),
    ("button/filled", &["Button"]),
    ("icon_button", &["IconButton"]),
    ("icon_button/toolbar", &["IconButton"]),
    ("checkbox", &["Checkbox"]),
    ("radio_button", &["RadioButton"]),
    ("toggle", &["Toggle"]),
    // The node and its two reported regions — track and knob.
    ("slider", &["Slider"]),
    ("segmented_control", &["SegmentCell"]),
    ("combo_box", &["ComboBox"]),
    ("spin_box", &["StepButton", "TextInputField"]),
    ("link", &["Link"]),
    ("badge", &[]),
    ("avatar", &["Avatar"]),
    ("progress_bar", &[]),
    ("spinner", &[]),
    ("split_button", &["ChevronRegion"]),
    ("breadcrumb", &["BreadcrumbSegment"]),
    // The step markers are not targets — a `Stepper` is navigated by its
    // footer, and the footer's Back / Next buttons are what this measures.
    ("stepper", &["Button"]),
    ("radio_tile", &["RadioTile"]),
    ("command_link_button", &["CommandLinkButton"]),
    // ---- Inputs -----------------------------------------------------
    ("text_input", &["TextInputField"]),
    ("search_field", &["TextInputField", "HitTarget"]),
    ("password_field", &["TextInputField", "IconButton"]),
    ("date_edit", &["TextInputField", "IconButton"]),
    ("time_edit", &["TextInputField"]),
    ("date_time_edit", &["TextInputField", "IconButton"]),
    ("calendar", &["DayCell", "NavArrow"]),
    (
        "color_picker",
        &["ColorSwatch", "HueStrip", "HsvCanvas", "StepButton"],
    ),
    ("color_swatch_row", &["ColorSwatch"]),
    // ---- Rows and the chevron column --------------------------------
    // A standalone `StandardListItem` is NOT a target: selection is the data
    // view's, so a row on its own carries no press. What it owns is the
    // checkbox it embeds.
    ("standard_list_item", &["Checkbox"]),
    ("standard_tree_item", &["Checkbox", "TwistArrow"]),
    ("twist_arrow_in_a_row", &["TwistArrow"]),
    // ---- Menus and chrome -------------------------------------------
    ("menu_list", &["MenuItem"]),
    ("menu_bar", &["MenuBarTrigger"]),
    ("toolbar", &["Button", "IconButton"]),
    ("toolbar/overflowing", &["IconButton"]),
    (
        "segmented_control/overflowing",
        &["SegmentCell", "IconButton"],
    ),
    ("title_bar", &["ControlButton", "DragRegion"]),
    ("window_frame", &["ResizeStrip"]),
    // ---- Containers -------------------------------------------------
    ("accordion", &["Accordion"]),
    ("tool_box", &["ToolBoxHeader"]),
    ("group_box", &["Checkbox"]),
    ("splitter", &["SplitterHandle"]),
    ("scroll_area", &["ScrollBar"]),
    ("tab_widget", &["TabHeader"]),
    ("tab_widget/closable", &["TabHeader"]),
    (
        "docking",
        &["DockResizeHandle", "DockRailItem", "TabHeader"],
    ),
    ("touch_target", &["TwistArrow"]),
    // ---- Data views -------------------------------------------------
    ("list_view", &["ListItemWrapper", "ScrollBar"]),
    ("tree_view", &["TreeItemWrapper", "TwistArrow"]),
    ("table_view", &["CellA11y", "HeaderCell"]),
    ("tree_table_view", &["TwistArrow", "HeaderCell"]),
    ("grid_view", &["TileA11y"]),
    // ---- Overlays and feedback --------------------------------------
    ("banner", &["Button"]),
    // The trigger, not the snackbar: its content lives in an overlay this
    // fixture does not open.
    ("snackbar", &["Button"]),
    ("dialog", &["Button"]),
    ("popover_button", &["Button"]),
    ("toast", &["ToastSurface", "IconButton"]),
    ("notification_center_button", &["IconButton"]),
    ("notification_log", &["StandardListItem", "Link", "Button"]),
    // The keyboard Browse fallback inside the zone, and the child's own
    // affordance surviving the wrapper — the wrappers themselves install
    // `on_drag_hover` / `on_drop` and nothing else, and a drop is not a press.
    ("drop_zone", &["Button"]),
    ("drop_target", &["Button"]),
    // ---- Text surfaces ----------------------------------------------
    ("log_view", &["LogView", "ScrollBar"]),
    ("code_editor", &["CodeEditor"]),
    ("plain_text_editor/one_line", &["CodeEditor"]),
    ("rich_text_editor", &["RichTextEditor"]),
];

/// The type of the node a measurement is about — the last segment of its path.
fn measured_leaf(path: &str) -> &str {
    let tail = path.rsplit(" > ").next().unwrap_or(path);
    tail.rsplit(": ").next().unwrap_or(tail)
}

/// The list is not a stub: every fixture is audited, and every fixture that
/// claims a subject is seen to measure it.
#[test]
fn every_fixture_measures_the_subject_it_names() {
    let fixtures = fixtures();
    assert!(
        fixtures.len() >= 40,
        "the fixture list is the coverage claim; it holds {}",
        fixtures.len(),
    );
    let measured = measure_fixtures(&fixtures, TargetDensity::Compact);
    let mut problems = Vec::new();
    for fixture in &fixtures {
        let Some((_, expected)) = EXPECTATIONS.iter().find(|(name, _)| *name == fixture.name)
        else {
            problems.push(format!(
                "fixture `{}` has no entry in EXPECTATIONS — say what it must \
                 measure, or say that it is not a target",
                fixture.name,
            ));
            continue;
        };
        let prefix = format!("{}: ", fixture.name);
        let leaves: Vec<&str> = measured
            .iter()
            .filter(|m| m.path.starts_with(&prefix))
            .map(|m| measured_leaf(&m.path))
            .collect();
        for widget in *expected {
            if !leaves.contains(widget) {
                problems.push(format!(
                    "fixture `{}` measured no `{widget}` node; it measured: {}",
                    fixture.name,
                    if leaves.is_empty() {
                        "nothing".to_string()
                    } else {
                        leaves.join(", ")
                    },
                ));
            }
        }
        if expected.is_empty() && !leaves.is_empty() {
            // Not a failure: the row wrapper `in_tappable_row` installs is a
            // target and shows up here. What would be a failure is the subject
            // itself becoming one silently, which is why the entry is `&[]`
            // rather than absent — and why this prints instead of passing mute.
            println!(
                "NOTE fixture `{}` claims no target of its own and measured: {}",
                fixture.name,
                leaves.join(", "),
            );
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// A deliberately undersized fixture **must** fail, and it must fail for the
/// right reason: the row around it owns every near miss, so no mechanism can
/// rescue a 10 dp control.
///
/// This is the harness's liveness proof. A gate that reports zero because it
/// measures nothing passes every other test in this file.
#[test]
fn a_deliberately_undersized_fixture_fails() {
    let undersized = vec![TargetFixture::new("undersized", |t| {
        use teksilo_core::widget_builder::WidgetBuilder;
        t.add(
            Padding::uniform(20.0)
                .child(
                    HStack::new()
                        .spacing(8.0)
                        .child(TextWidget::new(lit!("Row label")))
                        .child(
                            teksilo_widgets::primitives::FixedSize::new()
                                .width(10.0)
                                .height(10.0)
                                .child(RectWidget::new().background(Color::from_hex("#FF0000")))
                                .on_tap(|_, _| {}),
                        ),
                )
                .on_tap(|_, _| {}),
        )
    })];
    for density in [
        TargetDensity::Compact,
        TargetDensity::Comfortable,
        TargetDensity::Touch,
    ] {
        let violations = audit_fixtures(&undersized, density);
        assert!(
            violations
                .iter()
                .any(|v| v.rule == TargetRule::MinTargetConformance),
            "a 10 dp control inside a tappable row must fail at {density:?}: {violations:#?}",
        );
    }
}

/// An app-installed Tier-3 style slot is **reported**, and the control it owns
/// is measured at the size its author froze it at — so a control that could not
/// follow the density ladder is attributed to the style that owns its metrics
/// rather than looking like a framework defect.
///
/// Built through a [`TargetFixture`] at each density rather than by switching
/// one tree with `set_input_density`, for the reason `audit_fixtures` states:
/// the switch marks the arena's roots for rebuild and a layout primitive at the
/// root re-attaches child ids the rebuild has already destroyed. Measured on the
/// shape this test used to build — `Padding > HStack > [TextWidget, Button]`
/// under `audit_at_density` — the tree went from **6 nodes to 2**, the custom
/// button was gone, and the single violation left was the 800 x 600 `Padding`
/// root: the assertion passed while measuring nothing it named.
///
/// Three legs, because the reporting alone would not show the consequence:
///
/// 1. [`unprojected_style_slots`] names the slot, and names nothing for the
///    stock preset;
/// 2. the custom button paints 12 x 12 at **every** density — `with_density`
///    preserves an app slot verbatim, which is the documented behaviour — and
///    is an SC 2.5.8 failure at each one;
/// 3. the **stock** button in the same fixture shape clears the floor at every
///    density, which is what makes leg 2 the slot's doing and not the fixture's.
#[test]
fn an_app_installed_style_slot_is_reported() {
    assert_eq!(
        unprojected_style_slots(&tiny_button_theme()),
        vec!["button"]
    );
    assert!(
        !unprojected_style_slots(&intui::light()).contains(&"button"),
        "the stock preset installs no slot to report",
    );

    let custom = [TargetFixture::new("app_slot", |t| {
        in_tappable_row(t, Button::new(lit!("Save")))
    })
    .with_theme(tiny_button_theme)];
    let stock = [TargetFixture::new("stock", |t| {
        in_tappable_row(t, Button::new(lit!("Save")))
    })];
    for density in [
        TargetDensity::Compact,
        TargetDensity::Comfortable,
        TargetDensity::Touch,
    ] {
        let measured = measure_fixtures(&custom, density);
        let button = measured
            .iter()
            .find(|m| measured_leaf(&m.path) == "Button")
            .unwrap_or_else(|| {
                panic!("the custom-styled button is measured at {density:?}: {measured:#?}")
            });
        assert_dp(
            button.size,
            (12.0, 12.0),
            "a hand-written style's body is frozen at its own metrics",
        );
        assert_dp(
            button.expanded,
            (12.0, 12.0),
            "reaching exactly its own paint — no mechanism serves it, and a \
             reach of zero would mean the fixture is measuring an occlusion \
             rather than the slot",
        );
        assert_eq!(
            button.rule,
            Some(TargetRule::MinTargetConformance),
            "and 12 dp inside a tappable row fails the floor at {density:?}: {button:#?}",
        );

        let measured = measure_fixtures(&stock, density);
        let button = measured
            .iter()
            .find(|m| measured_leaf(&m.path) == "Button")
            .unwrap_or_else(|| panic!("the stock button is measured at {density:?}"));
        assert_dp(
            button.expanded,
            (
                InputTokens::for_density(density).target_size + 1.0,
                InputTokens::for_density(density).target_size,
            ),
            "the stock style follows the ladder, so the same shape conforms \
             (the width is the probe budget spent, not a boundary)",
        );
        assert_eq!(
            button.rule, None,
            "and carries no verdict at {density:?}: {button:#?}",
        );
    }
}

/// The IntUI preset with one **app-installed** `ButtonStyle` — a hand-written
/// style whose metrics no density projection will re-derive.
fn tiny_button_theme() -> teksilo_core::styles::Theme {
    let mut theme = intui::light();
    theme.style_slots.button = Some(Rc::new(TinyButtonStyle));
    theme
}

/// A 12 dp button body, standing in for any app-installed style whose author
/// wrote its dimensions by hand.
///
/// `cfg.label` is attached rather than dropped. A style that drops it leaves the
/// pre-built label subtree parented to nothing, which makes it a second arena
/// **root** — laid out at the whole viewport and tested before the real root, so
/// it swallows every press in the tree and every target measures a reach of
/// zero. That is a bug in the style, and a fixture built on one measures the
/// bug rather than the slot.
#[derive(Debug)]
struct TinyButtonStyle;

impl teksilo_core::styles::ButtonStyle for TinyButtonStyle {
    fn make_body(
        &self,
        cfg: &teksilo_core::styles::ButtonStyleConfig,
        ctx: &mut teksilo_core::build_context::BuildContext,
    ) -> WidgetId {
        let label = cfg.label;
        ctx.add(
            teksilo_widgets::primitives::FixedSize::new()
                .width(12.0)
                .height(12.0)
                .child(
                    teksilo_widgets::primitives::ZStack::new()
                        .child(RectWidget::new().background(Color::from_hex("#888888")))
                        .add_child(label),
                ),
        )
    }
}

/// The two mechanisms the shipped controls declare are actually reached, and the
/// audit can tell them apart.
///
/// * A `ScrollBar` reports its thumb from `Widget::target_regions`, so the audit
///   sees a target the node's own rectangle hides.
/// * A `Splitter`'s gutter takes its reach from `Widget::hit_outset` inside the
///   exact pass, not from the slop pass — the panes on either side own every
///   near miss at distance zero.
#[test]
fn the_declared_mechanisms_are_reached_and_attributed() {
    let fixtures = fixtures();
    let measured = measure_fixtures(&fixtures, TargetDensity::Compact);

    let regions: Vec<_> = measured.iter().filter(|m| m.part.is_some()).collect();
    assert!(
        !regions.is_empty(),
        "no widget in the list reported a target region; the `target_regions` leg \
         of the walk is measuring nothing",
    );

    let outset_grown: Vec<_> = measured.iter().filter(|m| m.sources.outset).collect();
    assert!(
        !outset_grown.is_empty(),
        "no widget in the list took its reach from `hit_outset`; the exact-pass \
         leg of the walk is measuring nothing",
    );
}

// =========================================================================
// The findings, pinned
// =========================================================================
//
// Each of the three tests below asserts the measurement one `ALLOW_LIST` entry
// rests on. An excuse written as prose is an excuse nobody re-derives; written
// as an equality it reddens the day the geometry changes, and the entry has to
// be re-read then rather than outliving what it excused.

/// Every measurement one fixture produces for one leaf type, at one density.
fn measurements_of(
    fixture: &str,
    leaf: &str,
    density: TargetDensity,
) -> Vec<teksilo_core::accessibility::target_audit::TargetMeasurement> {
    let one: Vec<TargetFixture> = fixtures()
        .into_iter()
        .filter(|f| f.name == fixture)
        .collect();
    assert_eq!(
        one.len(),
        1,
        "fixture `{fixture}` is not in the list exactly once",
    );
    measure_fixtures(&one, density)
        .into_iter()
        .filter(|m| measured_leaf(&m.path) == leaf)
        .collect()
}

/// The probe finds each boundary by bisecting inside one `PROBE_STEP`, so a
/// reach is exact to `0.5 / 2^4` dp and is compared with the walker's own
/// epsilon. A *paint* is compared the same way for symmetry; both are
/// equalities, not bounds — a bound would be satisfied by a target sized by
/// something other than itself.
#[track_caller]
fn assert_dp(actual: Size, expected: (f32, f32), what: &str) {
    assert!(
        (actual.width - expected.0).abs() <= 0.05 && (actual.height - expected.1).abs() <= 0.05,
        "{what}: measured {:.4} x {:.4}, expected {:.2} x {:.2}",
        actual.width,
        actual.height,
        expected.0,
        expected.1,
    );
}

/// `TwistArrow::hit_outset` is inert inside `StandardTreeItem`, and the same
/// chevron in a bare row reaches the floor.
///
/// The hook is consulted only among a node's direct children and the recursion
/// returns before it looks at a child whose own parent excludes the point, so a
/// wrapper the size of its child offers the outset nothing.
/// `StandardTreeItem::build` wraps the chevron in
/// `FixedSize::new().width(chevron_size)`, which is exactly that wrapper.
///
/// Reddens the day the wrapper goes: the chevron's reach in a TreeView then
/// stops equalling its paint, and the `HStack > FixedSize > TwistArrow`
/// allow-list entry stops matching with it.
#[test]
fn the_chevrons_outset_is_inert_inside_a_standard_tree_row() {
    for density in [
        TargetDensity::Compact,
        TargetDensity::Comfortable,
        TargetDensity::Touch,
    ] {
        let in_a_tree = measurements_of("tree_view", "TwistArrow", density);
        assert!(
            !in_a_tree.is_empty(),
            "no chevron measured in a TreeView at {density:?} — the fixture \
             stopped building one, which is a blind gate, not a pass",
        );
        for m in &in_a_tree {
            assert_dp(m.size, (16.0, 16.0), "the tree chevron's paint moved");
            assert_dp(
                m.expanded,
                (16.0, 16.0),
                &format!(
                    "at {density:?} the chevron inside a StandardTreeItem \
                     reached past its own box, so a mechanism now delivers here \
                     and the `HStack > FixedSize > TwistArrow` allow-list entry \
                     is out of date"
                ),
            );
            assert!(
                !m.sources.any(),
                "and no mechanism may be credited: {:?}",
                m.sources,
            );
        }
    }

    // The discriminator: the same chevron, one wrapper fewer, clears the floor
    // at Compact through its outset. Without this half the test above would pass
    // just as well if the outset had never worked anywhere.
    let bare = measurements_of("twist_arrow_in_a_row", "TwistArrow", TargetDensity::Compact);
    assert_eq!(bare.len(), 1);
    assert_dp(bare[0].size, (12.0, 12.0), "the bare chevron's paint");
    assert_dp(
        bare[0].expanded,
        (24.0, 24.0),
        "the chevron's outset does not reach the floor even unhugged, so this \
         file is measuring a broken mechanism rather than a hugged one",
    );
    assert!(
        bare[0].sources.outset,
        "and it must be the outset that delivers it, not the slop pass",
    );
}

/// A window's diagonal corner grip is boxed in by its own edge strips, while
/// those edges reach the floor through the same `hit_outset`.
///
/// Outset candidates are ordered by distance to the uninflated rectangle, and a
/// point one dp inboard of a corner is already *inside* an edge strip, at
/// distance zero — so the corner branch cannot grow the grip. It is still
/// load-bearing, which is the part reading cannot tell you: deleting it drops the
/// corner to **0 x 0**, because the corner's own body is claimed by the adjacent
/// edge and declaring an outset is what puts it in the same candidate list to win
/// it back. Deleting the edge branches instead lets the corners reach 24 x 24.
/// The equality below reddens under either mutation.
#[test]
fn a_window_corner_grip_is_boxed_in_by_its_own_edge_strips() {
    let strips = measurements_of("window_frame", "ResizeStrip", TargetDensity::Compact);
    assert_eq!(strips.len(), 8, "a frame has four edges and four corners");

    let corners: Vec<_> = strips
        .iter()
        .filter(|m| m.size.width == 6.0 && m.size.height == 6.0)
        .collect();
    assert_eq!(corners.len(), 4, "four corners");
    for m in &corners {
        assert_dp(
            m.expanded,
            (6.0, 6.0),
            "a corner grip grew, so its outset now claims something and the \
             allow-list entry is out of date",
        );
    }

    let edges: Vec<_> = strips
        .iter()
        .filter(|m| m.size.width > 6.0 || m.size.height > 6.0)
        .collect();
    assert_eq!(edges.len(), 4, "four edges");
    for m in &edges {
        // The thin axis is the one the outset grows; the long axis spends the
        // probe budget and is reported as the cap.
        let thin_reach = if m.size.height == 6.0 {
            m.expanded.height
        } else {
            m.expanded.width
        };
        assert!(
            (thin_reach - 24.0).abs() <= 0.05,
            "an edge strip must reach the Compact floor across its thickness — \
             it is the same `hit_outset` the corner cannot use — measured \
             {thin_reach:.4}",
        );
        assert!(m.sources.outset, "and it is the outset that gets it there");
    }
}

/// At Touch a dock's tab strip is unreachable at the point a user aims at,
/// because the resize gutter above it inflates to `TargetRole::Target` (44 dp)
/// and its ring reaches past the strip's centre line.
///
/// Reach 0 for a header that paints 95.8 x 38 is not a size failure; it is an
/// outset-versus-target precedence gap, and the number is what makes that
/// legible.
#[test]
fn the_dock_gutters_touch_ring_swallows_the_tab_strip_beside_it() {
    let gutters = measurements_of("docking", "DockResizeHandle", TargetDensity::Touch);
    assert_eq!(gutters.len(), 2, "a leading side and a bottom side");
    let horizontal = gutters
        .iter()
        .find(|m| m.size.height == 6.0)
        .expect("the bottom side's gutter is 6 dp tall");
    assert!(
        (horizontal.expanded.height - 44.0).abs() <= 0.05,
        "the gutter's Touch ring is what covers the strip; if it shrank, this \
         finding is stale — measured {:.4}",
        horizontal.expanded.height,
    );

    let bars = measurements_of("docking", "TabBar", TargetDensity::Touch);
    assert_eq!(bars.len(), 1);
    assert_dp(
        bars[0].size,
        (900.0, 38.0),
        "the strip is the full-width bottom dock's",
    );

    // The header is the thing a user aims at, and it is what goes dark.
    let touch = measurements_of("docking", "TabHeader", TargetDensity::Touch);
    assert_eq!(touch.len(), 1);
    assert_dp(
        touch[0].expanded,
        (0.0, 0.0),
        "the dock tab header is reachable again at Touch — the \
         `docking: DockingLayout > SideClipPane` allow-list entry is out of date",
    );
    assert_eq!(touch[0].rule, Some(TargetRule::MinTargetConformance));

    // And at Compact the same header is reachable, which is what makes this a
    // finding about the ring's size rather than about the strip. Without this
    // half the test would pass on a header that was never reachable at all.
    let compact = measurements_of("docking", "TabHeader", TargetDensity::Compact);
    assert_eq!(compact.len(), 1);
    assert_eq!(
        compact[0].rule, None,
        "at Compact the ring is 24 dp and stops short of the header's centre",
    );
    assert!(compact[0].expanded.width > 0.0 && compact[0].expanded.height > 0.0);
}

/// The tab close button exists as a target only where a finger can find it.
///
/// It is gated behind hover below `RevealPolicy::Always`, and a finger produces
/// no hover — so at Compact the fixture measures no close button at all, and at
/// Touch it measures one at the full 44 dp. Both halves matter: the first says
/// the audit is not silently missing a control, the second says the reveal policy
/// actually lands.
#[test]
fn a_hover_revealed_close_button_is_a_target_only_at_touch_density() {
    let at_compact = measurements_of("tab_widget/closable", "IconButton", TargetDensity::Compact);
    assert!(
        at_compact.is_empty(),
        "a hover-revealed close button must not exist unhovered at Compact: \
         {at_compact:#?}",
    );
    let at_touch = measurements_of("tab_widget/closable", "IconButton", TargetDensity::Touch);
    assert!(
        !at_touch.is_empty(),
        "at Touch `RevealPolicy::Always` installs no hover gate, so every tab's \
         close button must be a target",
    );
    for m in &at_touch {
        assert_dp(
            m.size,
            (44.0, 44.0),
            "the close button follows the density ladder",
        );
        assert_dp(m.expanded, (44.0, 44.0), "and it reaches its own paint");
    }
}

/// The expectation check reads the **measured node**, not a name that appears
/// somewhere above it.
///
/// The hole this closes was live: `DropZone` and `DropTarget` appear in the path
/// of every row their subtree produces, so `path.contains("DropZone")` was
/// satisfied by the `Button` inside while the wrapper itself is deliberately not
/// a press target at all. This asserts both halves against real measurements —
/// the name is there, and it is not a leaf — so a regression to `contains` fails
/// here rather than quietly widening every entry in the table.
#[test]
fn the_expectation_check_reads_the_measured_node_not_its_ancestors() {
    assert_eq!(measured_leaf("f: A > B > C"), "C");
    assert_eq!(measured_leaf("f: Alone"), "Alone");

    let one: Vec<TargetFixture> = fixtures()
        .into_iter()
        .filter(|f| f.name == "drop_zone")
        .collect();
    let measured = measure_fixtures(&one, TargetDensity::Compact);
    assert!(
        measured.iter().any(|m| m.path.contains("DropZone")),
        "the subject must be in the path of what it contains",
    );
    assert!(
        !measured
            .iter()
            .any(|m| measured_leaf(&m.path) == "DropZone"),
        "a `DropZone` installs `on_drag_hover` / `on_drop` and nothing else, and \
         a drop is not a press — so it must never be a measured node, and an \
         expectation naming it would be satisfied by its child",
    );
}

/// The two discriminators the census cannot exercise, because the geometries
/// they refuse do not occur today.
///
/// [`the_allow_list_excuses_only_the_geometry_it_pinned`] seeds regressions into
/// violations that exist; these two are about violations that do not. The
/// `window_frame` fixture builds eight `ResizeStrip`s on one identical path —
/// four conforming edges and four boxed-in corners — so an entry keyed on the
/// path alone would excuse an edge the day it regressed, and nothing in the
/// census would notice because the edges are not violations. Same for a finding
/// that exists at one density: without a per-pin density list the entry would
/// cover the other two, where the control is reachable.
#[test]
fn the_allow_lists_discriminators_narrow_by_size_and_by_density() {
    let violation =
        |path: &str, paints: (f32, f32), reaches: (f32, f32), density: TargetDensity| {
            TargetViolation {
                widget: "ResizeStrip",
                node: WidgetId::default(),
                part: None,
                path: path.to_string(),
                density,
                size: Size::new(paints.0, paints.1),
                expanded: Size::new(reaches.0, reaches.1),
                sources: Default::default(),
                transformed: false,
                rule: TargetRule::MinTargetConformance,
            }
        };
    let corner_entry = ALLOW_LIST
        .iter()
        .find(|e| e.path == "window_frame: WindowFrame > ResizeStrip")
        .expect("the corner-grip finding");
    let strip = "window_frame: WindowFrame > ResizeStrip";
    assert!(
        corner_entry.matches(&violation(
            strip,
            (6.0, 6.0),
            (6.0, 6.0),
            TargetDensity::Compact
        )),
        "the 6 x 6 corner reaching its own paint is the finding",
    );
    assert!(
        !corner_entry.matches(&violation(
            strip,
            (600.0, 6.0),
            (600.0, 12.0),
            TargetDensity::Compact
        )),
        "a 600 x 6 EDGE strip on the same path must not be excused — it reaches \
         the floor across its thickness today, and the day it stops the gate has \
         to say so",
    );

    let dock_entry = ALLOW_LIST
        .iter()
        .find(|e| e.path == "docking: DockingLayout > SideClipPane")
        .expect("the dock tab-strip finding");
    let dock = "docking: DockingLayout > SideClipPane > DockSidePanel";
    assert!(
        dock_entry.matches(&violation(
            dock,
            (900.0, 38.0),
            (0.0, 0.0),
            TargetDensity::Touch
        )),
        "the finding is at Touch",
    );
    assert!(
        !dock_entry.matches(&violation(
            dock,
            (900.0, 38.0),
            (0.0, 0.0),
            TargetDensity::Compact
        )),
        "and it must not cover Compact, where the same strip is reachable",
    );
    assert!(
        !dock_entry.matches(&violation(
            dock,
            (900.0, 38.0),
            (900.0, 20.0),
            TargetDensity::Touch
        )),
        "nor a strip that is merely undersized rather than unreachable — the \
         finding is a reach of zero, and a different shortfall in the same \
         region is a different finding",
    );
}

/// An outset's claim survives the miss-only slop pass — measured in the two
/// shipped controls where the two mechanisms fight, at the two densities where
/// each fight is visible.
///
/// The mechanisms otherwise contradict each other and the outset always loses: a
/// grip only ever claims a point at a **positive** distance from its own shape,
/// which is exactly the condition under which a slop-eligible node lying under
/// the ring is strictly closer. `arena.rs`'s `won_through_outset` is what stops
/// that, and this is the measurement it rests on. Reverting it costs:
///
/// | subject | density | with | without |
/// | --- | --- | ---: | ---: |
/// | `SearchField`'s 16 dp clear button, beside its own field | Compact | 24 dp | 22 dp |
/// | `TableView`'s 12 dp scroll bar, beside 28 dp rows | Touch | 32 dp | 18 dp |
///
/// Both halves matter, and neither is the whole story on its own. The Compact
/// row is a **regression in a shipped control at the density CI runs at** — the
/// field beside the clear button is itself under 24 dp, so it is a slop
/// candidate there. The Touch row is the reach going **down** as density rises,
/// because `up_to` 44 turns a 28 dp table row into a candidate that a 24 dp
/// `up_to` left inert. A test that measured only one of them would report half
/// the mechanism.
#[test]
fn an_outsets_claim_survives_the_slop_pass_in_the_shipped_controls() {
    let clear = measurements_of("search_field", "HitTarget", TargetDensity::Compact);
    assert_eq!(
        clear.len(),
        1,
        "a search field with text has one clear button"
    );
    assert_dp(clear[0].size, (16.0, 16.0), "the clear button's paint");
    assert_dp(
        clear[0].expanded,
        (24.0, 24.0),
        "the clear button must keep the whole floor its outset claims — 22 dp \
         means the field beside it took the leading half back through the \
         miss-only pass, which is a Compact regression",
    );
    assert!(clear[0].sources.outset, "and the outset must be credited");
    assert_eq!(clear[0].rule, None, "so it carries no verdict");

    let bar = measurements_of("table_view", "ScrollBar", TargetDensity::Touch);
    let node = bar
        .iter()
        .find(|m| m.part.is_none() && m.size.width > 0.0)
        .expect("the scroll bar's own node");
    assert_dp(
        node.size,
        (12.0, 268.0),
        "the table's scroll bar paints 12 dp",
    );
    assert!(
        (node.expanded.width - 32.0).abs() <= 0.05,
        "the bar must keep its Touch ring across its thickness — 18 dp means a \
         28 dp row beside it won the ring back, and the reach then falls as the \
         density rises — measured {:.4}",
        node.expanded.width,
    );
    assert!(node.sources.outset, "and the outset must be credited");
    assert_eq!(
        node.rule,
        Some(TargetRule::TouchTargetRecommendation),
        "32 dp clears the 24 dp AA floor and is short of the 44 dp Touch \
         recommendation, which is informational: {node:#?}",
    );
}
