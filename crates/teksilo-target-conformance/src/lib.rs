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
//! control measures as conformant. The rule this states is that **a fixture
//! without an eligible bubble owner on the path cannot tell an outset from the
//! slop pass**, and it is not a historical curiosity: when P24 first measured it,
//! most of the `hit_outset` implementations in the tree at the time passed their
//! own deletion. The count is deliberately not restated — the tree carries a
//! different number of them now, and the sentence is about the geometry, not
//! about how many widgets happened to be in it. Every fixture here therefore
//! builds its subject **inside the container it ships in**: a chevron inside a
//! tappable tree row, a swatch inside a picker row, a gutter between two panes.
//!
//! # Why this list is a crate and not a test
//!
//! The list names only `teksilo-widgets`' public API, so it was an integration
//! test of that crate until the gates needed it under more than one theme. A
//! preset lives *above* teksilo-widgets in the crate graph, so no test binary
//! inside teksilo-widgets can install one; and four copies of one list is how
//! two of them drift. So the list, the allow-list and every gate live here, in
//! an unpublished crate that dev-depends on all three presets — and nothing in
//! a published manifest changes at all.
//!
//! **What that costs, recorded rather than papered over:** `cargo test -p
//! teksilo-widgets` no longer catches an undersized control. `cargo test
//! --workspace` does. No stub is left behind in teksilo-widgets to stand in for
//! the gate — a test that passes by containing nothing is worse than an absent
//! one. The same boundary is written out in
//! `docs/accessibility-internal-audit.md` §3.7.
//!
//! # A widget author can redden three other crates' tests
//!
//! That is the package, not an accident: a control that falls under the floor
//! under macOS, Fluent or Material 3 is a failure whoever last touched it. The
//! correct response is an [`AllowedViolation`] naming the preset's owner, never
//! deleting the fixture.
//!
//! [`AllowedViolation`]: teksilo_core::accessibility::target_audit::AllowedViolation
//!
//! Reference: `docs/density-and-targets.md`, `docs/accessibility-internal-audit.md`.

use std::rc::Rc;

use jiff::civil::{Date, Time};
use teksilo_canvas::{Point, Size};
use teksilo_core::accessibility::target_audit::TargetFixture;
use teksilo_core::signal::Signal;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_data::{ListModel, SelectionMode, SelectionModel, TreeModel};
use teksilo_i18n::lit;
use teksilo_tokens::Color;
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
///
/// Public because the gate's own discrimination tests build one-off fixtures in
/// the same shape, and a second copy of this geometry is a second thing to get
/// wrong.
pub fn in_tappable_row(
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
///
/// Public for the same reason [`in_tappable_row`] is.
pub fn on_a_page(
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

pub fn widget_fixtures() -> Vec<TargetFixture> {
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
        // The same field with nothing typed in it. A second fixture rather than
        // a knob, because the clear affordance's target is a *different* target
        // when the query is empty: `HitTarget::active` withdraws its outset, and
        // what is left is a 16 dp slot that still takes the press and still
        // shows a pointer cursor. Measuring only the populated field reports
        // the conformant half of a two-state control.
        TargetFixture::new("search_field/empty", |t| {
            on_a_page(t, SearchField::new(Signal::new(String::new())))
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
/// `a_hover_revealed_close_button_is_a_target_only_at_touch_density`), and a
/// control whose paint is Compact-sized is the hardest case for the floor, which
/// is the density the gate cares most about.
pub const EXPECTED_SUBJECTS: &[(&str, &[&str])] = &[
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
    ("search_field/empty", &["TextInputField", "HitTarget"]),
    ("password_field", &["TextInputField", "IconButton"]),
    ("date_edit", &["TextInputField", "IconButton"]),
    ("time_edit", &["TextInputField"]),
    ("date_time_edit", &["TextInputField", "IconButton"]),
    ("calendar", &["DayCell", "NavArrow"]),
    (
        "color_picker",
        &[
            "ColorSwatch",
            "HueStrip",
            "AlphaStrip",
            "HsvCanvas",
            "StepButton",
        ],
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
