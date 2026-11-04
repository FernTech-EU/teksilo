// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`RowKeyboard::Actions`](crate::RowKeyboard::Actions): a `ListView` whose
//! rows are small forms the keyboard can step into.
//!
//! Which field holds focus is read by typing a marker and looking for it in
//! the fields' text, rather than by comparing widget ids: a rebuild replaces
//! every id, and what the user cares about is where their typing goes.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::*;
use crate::RowKeyboard;
use teksilo_core::WidgetTree;
use teksilo_core::event::{EventResponse, Key, Modifiers, WidgetEvent};
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_data::{SelectionMode, SelectionModel};

/// Two fields per row, keyed by the item (not the index), so a row that moves
/// keeps its fields' text.
struct Fields {
    name: Vec<Signal<String>>,
    note: Vec<Signal<String>>,
}

struct Fixture {
    tree: WidgetTree,
    lv: WidgetId,
    /// A button after the list, so a Tab leaving the list has somewhere to go.
    after: WidgetId,
    model: ListModel<usize>,
    fields: Rc<Fields>,
    rows: Rc<RefCell<Vec<(usize, WidgetId)>>>,
    selection: SelectionModel,
    activated: Rc<RefCell<Vec<usize>>>,
    /// Escapes the list let through to its parent.
    escapes_past: Rc<Cell<usize>>,
}

struct Options {
    mode: RowKeyboard,
    activate: bool,
    reorderable: bool,
    rows: usize,
    /// Rows `>= controls_from` have fields; the others are plain text.
    controls_from: usize,
    /// Rows `< disabled_below` have fields, both disabled.
    disabled_below: usize,
    /// Rows `>= plain_from` are plain text too.
    plain_from: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            mode: RowKeyboard::Actions,
            activate: false,
            reorderable: false,
            rows: 6,
            controls_from: 0,
            disabled_below: 0,
            plain_from: usize::MAX,
        }
    }
}

fn form_list(options: Options) -> Fixture {
    let total = options.rows + 8;
    let fields = Rc::new(Fields {
        name: (0..total).map(|_| Signal::new(String::new())).collect(),
        note: (0..total).map(|_| Signal::new(String::new())).collect(),
    });
    let model = ListModel::from_vec((0..options.rows).collect::<Vec<usize>>());
    let selection = SelectionModel::new(SelectionMode::Single);
    let activated: Rc<RefCell<Vec<usize>>> = Rc::default();
    let escapes_past = Rc::new(Cell::new(0));

    let f = fields.clone();
    let controls_from = options.controls_from;
    let disabled_below = options.disabled_below;
    let plain_from = options.plain_from;
    let mut list = ListView::new(model.clone(), move |_i, item, _selected| {
        if *item < controls_from || *item >= plain_from {
            return Box::new(crate::primitives::TextWidget::new(teksilo_i18n::lit!(
                format!("Plain {item}")
            )));
        }
        let enabled = *item >= disabled_below;
        Box::new(
            crate::primitives::HStack::new()
                .child(crate::TextInput::new(f.name[*item].clone()).enabled(enabled))
                .child(crate::TextInput::new(f.note[*item].clone()).enabled(enabled)),
        )
    })
    .item_height(40.0)
    .selection(selection.clone())
    .row_keyboard(options.mode)
    .reorderable(options.reorderable);
    if options.activate {
        let sink = activated.clone();
        list = list.on_activate(move |row, _ctx| sink.borrow_mut().push(row));
    }
    let rows = list.realized_row_ids();

    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let lv = tree.add(list);
    let after = tree.add(crate::Button::new(teksilo_i18n::lit!("After")));
    let count = escapes_past.clone();
    let _root = tree.add(
        crate::primitives::VStack::new()
            .child(crate::primitives::FixedSize::new().height(320.0).child(lv))
            .child(after)
            .on_key(move |event, _ctx| {
                if let WidgetEvent::KeyDown {
                    key: Key::Escape, ..
                } = event
                {
                    count.set(count.get() + 1);
                }
                EventResponse::Ignored
            }),
    );
    tree.layout(SizeProposal::exact(400.0, 400.0));
    tree.focus(lv);
    tree.layout(SizeProposal::exact(400.0, 400.0));
    Fixture {
        tree,
        lv,
        after,
        model,
        fields,
        rows,
        selection,
        activated,
        escapes_past,
    }
}

impl Fixture {
    fn settle(&mut self) {
        self.tree.layout(SizeProposal::exact(400.0, 400.0));
        self.tree.layout(SizeProposal::exact(400.0, 400.0));
    }

    fn key(&mut self, key: Key, modifiers: Modifiers) {
        self.tree.press_key(key, modifiers);
        self.settle();
    }

    /// The wrapper of the realized row at model index `index`.
    fn row(&self, index: usize) -> WidgetId {
        self.rows
            .borrow()
            .iter()
            .find(|(i, _)| *i == index)
            .map(|(_, id)| *id)
            .unwrap_or_else(|| panic!("row {index} is not realized"))
    }

    /// The `k`th field of row `index` (the `TextInput` composite).
    fn field(&self, index: usize, k: usize) -> WidgetId {
        let body = self.tree.children(self.row(index))[0];
        self.tree.children(body)[k]
    }

    /// Where typing goes: `(item, 0 for name / 1 for note)`, or `None` when
    /// it reaches no field.
    fn typing_goes_to(&mut self, marker: &str) -> Option<(usize, usize)> {
        let focused = self.tree.focused()?;
        self.tree.type_text(focused, marker);
        self.settle();
        let f = self.fields.clone();
        let hit = (0..f.name.len()).find_map(|item| {
            if f.name[item].get().contains(marker) {
                Some((item, 0))
            } else if f.note[item].get().contains(marker) {
                Some((item, 1))
            } else {
                None
            }
        });
        // Leave the fields as they were, so the next probe reads clean.
        if let Some((item, k)) = hit {
            let signal = if k == 0 { &f.name[item] } else { &f.note[item] };
            signal.set(signal.get().replace(marker, ""));
            self.settle();
        }
        hit
    }

    fn cursor(&self) -> Option<usize> {
        let any = self.tree.widget_as_any(self.lv).expect("as_any");
        any.downcast_ref::<ListView<usize>>()
            .expect("a ListView<usize>")
            .focused_index
            .get()
    }

    fn entered(&self) -> Option<usize> {
        let any = self.tree.widget_as_any(self.lv).expect("as_any");
        any.downcast_ref::<ListView<usize>>()
            .expect("a ListView<usize>")
            .row_actions
            .as_ref()
            .and_then(|actions| actions.entered.get())
    }

    fn list_stops(&self) -> usize {
        self.tree.tab_stops_within(self.lv).len()
    }
}

#[test]
fn the_default_mode_keeps_the_list_one_tab_stop_and_f2_inert() {
    let mut fx = form_list(Options {
        mode: RowKeyboard::Toggle,
        ..Options::default()
    });
    assert_eq!(fx.list_stops(), 1);
    fx.selection.select(2);
    fx.settle();
    fx.key(Key::F2, Modifiers::NONE);
    assert_eq!(fx.tree.focused(), Some(fx.lv), "F2 does nothing in Toggle");
    assert_eq!(fx.list_stops(), 1);
}

#[test]
fn a_list_in_action_mode_is_still_one_tab_stop_until_a_row_is_entered() {
    let mut fx = form_list(Options::default());
    assert_eq!(fx.list_stops(), 1, "the list, and none of its 12 fields");
    fx.key(Key::Tab, Modifiers::NONE);
    assert_eq!(
        fx.tree.focused(),
        Some(fx.after),
        "Tab from the list goes past it"
    );
}

#[test]
fn f2_steps_into_the_cursor_row_and_tab_cycles_inside_it() {
    let mut fx = form_list(Options {
        activate: true,
        ..Options::default()
    });
    fx.selection.select(2);
    fx.settle();
    fx.key(Key::F2, Modifiers::NONE);
    assert_eq!(fx.entered(), Some(2));
    assert_eq!(
        fx.typing_goes_to("a"),
        Some((2, 0)),
        "F2 lands on the first field"
    );

    fx.key(Key::Tab, Modifiers::NONE);
    assert_eq!(
        fx.typing_goes_to("b"),
        Some((2, 1)),
        "Tab reaches the second"
    );
    fx.key(Key::Tab, Modifiers::NONE);
    assert_eq!(
        fx.typing_goes_to("c"),
        Some((2, 0)),
        "and wraps to the first instead of leaving the list"
    );
    fx.key(Key::Tab, Modifiers::SHIFT);
    assert_eq!(fx.typing_goes_to("d"), Some((2, 1)), "Shift+Tab wraps back");
    assert!(fx.activated.borrow().is_empty(), "F2 activates nothing");
}

#[test]
fn f2_on_a_row_without_a_control_does_nothing() {
    let mut fx = form_list(Options {
        controls_from: 3,
        ..Options::default()
    });
    fx.selection.select(1);
    fx.settle();
    fx.key(Key::F2, Modifiers::NONE);
    assert_eq!(fx.tree.focused(), Some(fx.lv));
    assert_eq!(fx.entered(), None);
    fx.key(Key::Tab, Modifiers::NONE);
    assert_eq!(fx.tree.focused(), Some(fx.after), "and Tab still leaves");
}

#[test]
fn enter_steps_in_only_when_the_list_has_nothing_to_activate() {
    let mut fx = form_list(Options::default());
    fx.selection.select(1);
    fx.settle();
    fx.key(Key::Enter, Modifiers::NONE);
    assert_eq!(
        fx.typing_goes_to("e"),
        Some((1, 0)),
        "with no on_activate, Enter enters the row"
    );

    let mut fx = form_list(Options {
        activate: true,
        ..Options::default()
    });
    fx.selection.select(1);
    fx.settle();
    fx.key(Key::Enter, Modifiers::NONE);
    assert_eq!(*fx.activated.borrow(), vec![1], "with one, Enter activates");
    assert_eq!(fx.tree.focused(), Some(fx.lv), "and stays on the list");
}

#[test]
fn enter_on_a_row_without_a_control_keeps_selecting() {
    let mut fx = form_list(Options {
        controls_from: 3,
        ..Options::default()
    });
    fx.key(Key::ArrowDown, Modifiers::NONE);
    fx.key(Key::ArrowDown, Modifiers::NONE);
    fx.key(Key::Enter, Modifiers::NONE);
    assert_eq!(fx.selection.selected_indices(), vec![1]);
    assert_eq!(fx.tree.focused(), Some(fx.lv));
}

#[test]
fn the_lists_keys_stay_with_the_field() {
    let mut fx = form_list(Options {
        reorderable: true,
        ..Options::default()
    });
    fx.selection.select(2);
    fx.settle();
    fx.key(Key::F2, Modifiers::NONE);
    for (key, modifiers) in [
        (Key::ArrowDown, Modifiers::NONE),
        (Key::PageDown, Modifiers::NONE),
        (Key::ArrowUp, Modifiers::ALT),
        (Key::A, Modifiers::COMMAND | Modifiers::SHIFT),
    ] {
        fx.key(key, modifiers);
        assert_eq!(fx.cursor(), Some(2), "{key:?} left the cursor alone");
        assert_eq!(
            fx.selection.selected_indices(),
            vec![2],
            "{key:?} left the selection alone"
        );
        assert_eq!(
            fx.model.with_item(2, |item| *item),
            Some(2),
            "{key:?} did not reorder the row being edited"
        );
    }
    assert_eq!(fx.typing_goes_to("f"), Some((2, 0)), "focus never left");
}

#[test]
fn escape_returns_to_the_list_on_the_same_row_and_a_second_escape_passes() {
    let mut fx = form_list(Options::default());
    fx.selection.select(3);
    fx.settle();
    fx.key(Key::F2, Modifiers::NONE);
    fx.key(Key::Tab, Modifiers::NONE);
    fx.key(Key::Escape, Modifiers::NONE);
    assert_eq!(fx.tree.focused(), Some(fx.lv));
    assert_eq!(fx.cursor(), Some(3));
    assert_eq!(fx.entered(), None);
    assert_eq!(fx.escapes_past.get(), 0, "the list took that one");
    assert_eq!(fx.list_stops(), 1);

    fx.key(Key::Escape, Modifiers::NONE);
    assert_eq!(fx.escapes_past.get(), 1, "the next one reaches the parent");

    fx.key(Key::Tab, Modifiers::NONE);
    assert_eq!(fx.tree.focused(), Some(fx.after), "and Tab leaves the list");
}

#[test]
fn a_click_into_a_field_enters_its_row() {
    // The mouse path of the report: a field reached with the pointer, then
    // Tab, left the list.
    let mut fx = form_list(Options::default());
    let field = fx.field(1, 0);
    fx.tree.click(field);
    fx.settle();
    assert_eq!(fx.entered(), Some(1));
    assert_eq!(fx.cursor(), Some(1), "the cursor follows, for Escape");
    fx.key(Key::Tab, Modifiers::NONE);
    assert_eq!(fx.typing_goes_to("g"), Some((1, 1)));
    fx.key(Key::Tab, Modifiers::NONE);
    assert_eq!(fx.typing_goes_to("h"), Some((1, 0)), "Tab stays in the row");

    // From one row's field into another's.
    let other = fx.field(4, 0);
    fx.tree.click(other);
    fx.settle();
    assert_eq!(fx.entered(), Some(4));
    fx.key(Key::Tab, Modifiers::NONE);
    assert_eq!(fx.typing_goes_to("i"), Some((4, 1)));
    fx.tree.assert_no_leaked_pointer_state();
}

#[test]
fn a_row_left_with_the_mouse_does_not_leak_into_the_tab_order() {
    let mut fx = form_list(Options::default());
    let field = fx.field(1, 0);
    fx.tree.click(field);
    fx.settle();
    let after = fx.after;
    fx.tree.click(after);
    fx.settle();
    assert_eq!(fx.tree.focused(), Some(after));
    assert_eq!(fx.list_stops(), 1, "row 1's fields are no Tab stops now");
    fx.key(Key::Tab, Modifiers::SHIFT);
    assert_eq!(
        fx.tree.focused(),
        Some(fx.lv),
        "Shift+Tab lands on the list"
    );
    assert_eq!(fx.entered(), None, "which ends the mode");
    fx.tree.assert_no_leaked_pointer_state();
}

#[test]
fn focus_stays_in_the_field_across_the_rebuilds_a_list_lives_in() {
    let mut fx = form_list(Options::default());
    fx.selection.select(2);
    fx.settle();
    fx.key(Key::F2, Modifiers::NONE);
    fx.key(Key::Tab, Modifiers::NONE);
    assert_eq!(fx.typing_goes_to("j"), Some((2, 1)));

    // The row's own selection flips: the wrapper rebuilds.
    fx.selection.select(4);
    fx.settle();
    fx.selection.select(2);
    fx.settle();
    assert_eq!(fx.typing_goes_to("k"), Some((2, 1)), "selection rebuild");

    // The row's item is updated: the pane rebuilds.
    fx.model.set(2, 2);
    fx.settle();
    assert_eq!(fx.typing_goes_to("l"), Some((2, 1)), "ItemUpdated");
    assert_eq!(fx.entered(), Some(2));

    // A row inserted above: the entered row is now index 3.
    fx.model.insert(0, 7);
    fx.settle();
    assert_eq!(fx.entered(), Some(3));
    assert_eq!(
        fx.typing_goes_to("m"),
        Some((2, 1)),
        "still in the same field of the same item's row"
    );
    fx.key(Key::Tab, Modifiers::NONE);
    fx.key(Key::Tab, Modifiers::NONE);
    assert!(
        matches!(fx.typing_goes_to("n"), Some((2, _))),
        "and Tab still cycles there"
    );
    assert_eq!(fx.list_stops(), 3, "the list and the entered row's fields");
}

#[test]
fn removing_the_entered_row_returns_to_the_list() {
    let mut fx = form_list(Options::default());
    fx.selection.select(2);
    fx.settle();
    fx.key(Key::F2, Modifiers::NONE);
    fx.model.remove(2);
    fx.settle();
    assert_eq!(fx.tree.focused(), Some(fx.lv), "not another row's field");
    assert_eq!(fx.entered(), None);
    assert_eq!(fx.list_stops(), 1);
}

#[test]
fn a_reset_returns_to_the_list() {
    let mut fx = form_list(Options::default());
    fx.selection.select(2);
    fx.settle();
    fx.key(Key::F2, Modifiers::NONE);
    fx.model.replace_all((0..6).collect());
    fx.settle();
    assert_eq!(fx.tree.focused(), Some(fx.lv));
    assert_eq!(fx.entered(), None);
}

#[test]
fn a_row_scrolled_out_of_the_window_waits_on_the_list_and_comes_back() {
    let mut fx = form_list(Options {
        rows: 60,
        ..Options::default()
    });
    fx.selection.select(1);
    fx.settle();
    fx.key(Key::F2, Modifiers::NONE);
    fx.key(Key::Tab, Modifiers::NONE);
    assert_eq!(fx.typing_goes_to("o"), Some((1, 1)));

    let scroll = {
        let any = fx.tree.widget_as_any(fx.lv).expect("as_any");
        any.downcast_ref::<ListView<usize>>()
            .expect("a ListView<usize>")
            .scroll_y
            .clone()
    };
    scroll.set(1600.0);
    fx.settle();
    assert_eq!(
        fx.tree.focused(),
        Some(fx.lv),
        "focus waits on the list, not in another row's field"
    );

    scroll.set(0.0);
    fx.settle();
    assert_eq!(
        fx.typing_goes_to("p"),
        Some((1, 1)),
        "and goes back into the same field when the row returns"
    );

    // Scrolled away again, and focus moved elsewhere meanwhile: not taken back.
    scroll.set(1600.0);
    fx.settle();
    let after = fx.after;
    fx.tree.focus(after);
    fx.settle();
    scroll.set(0.0);
    fx.settle();
    assert_eq!(fx.tree.focused(), Some(after));
    assert_eq!(fx.entered(), None);
}

impl Fixture {
    fn scroll(&self) -> Signal<f32> {
        let any = self.tree.widget_as_any(self.lv).expect("as_any");
        any.downcast_ref::<ListView<usize>>()
            .expect("a ListView<usize>")
            .scroll_y
            .clone()
    }
}

#[test]
fn a_key_on_the_list_while_the_row_is_away_leaves_it() {
    // The row scrolled out, focus waits on the list. Down moves the cursor
    // to row 2 and the reveal scrolls back to the top, realizing row 1 again:
    // focus must stay on the list, on row 2, not jump into row 1's field.
    let mut fx = form_list(Options {
        rows: 60,
        ..Options::default()
    });
    fx.selection.select(1);
    fx.settle();
    fx.key(Key::F2, Modifiers::NONE);
    let scroll = fx.scroll();
    scroll.set(1600.0);
    fx.settle();
    assert_eq!(fx.tree.focused(), Some(fx.lv));

    fx.key(Key::ArrowDown, Modifiers::NONE);
    fx.settle();
    assert_eq!(fx.tree.focused(), Some(fx.lv), "focus stays on the list");
    assert_eq!(fx.cursor(), Some(2));
    assert_eq!(fx.entered(), None);
    assert_eq!(fx.list_stops(), 1);
}

#[test]
fn a_press_on_a_row_while_the_row_is_away_leaves_it() {
    // The rows down the list are plain text, so the press enters no other
    // row: the press itself has to end the wait.
    let mut fx = form_list(Options {
        rows: 60,
        plain_from: 20,
        ..Options::default()
    });
    fx.selection.select(1);
    fx.settle();
    fx.key(Key::F2, Modifiers::NONE);
    let scroll = fx.scroll();
    scroll.set(1600.0);
    fx.settle();
    // A row in view: the realized rows above the viewport are clipped.
    let row = fx.row(41);
    fx.tree.click(row);
    fx.settle();
    scroll.set(0.0);
    fx.settle();
    assert_eq!(fx.entered(), None);
    assert_eq!(
        fx.tree.focused(),
        Some(fx.lv),
        "row 1 did not take focus back"
    );
    fx.tree.assert_no_leaked_pointer_state();
}

#[test]
fn a_window_move_while_editing_keeps_the_same_field() {
    // A wheel scroll that moves the realized window while the row stays in it:
    // the old position among the pane's controls names another row's field.
    let mut fx = form_list(Options {
        rows: 60,
        ..Options::default()
    });
    fx.selection.select(9);
    fx.settle();
    fx.key(Key::F2, Modifiers::NONE);
    fx.key(Key::Tab, Modifiers::NONE);
    assert_eq!(fx.typing_goes_to("r"), Some((9, 1)));
    let first_realized = |fx: &Fixture| fx.rows.borrow().first().map(|row| row.0);
    let scroll = fx.scroll();
    scroll.set(520.0);
    fx.settle();
    assert_ne!(first_realized(&fx), Some(0), "the window moved");
    assert_eq!(fx.typing_goes_to("s"), Some((9, 1)));
    scroll.set(0.0);
    fx.settle();
    assert_eq!(first_realized(&fx), Some(0), "and back");
    assert_eq!(fx.typing_goes_to("t"), Some((9, 1)));
}

#[test]
fn a_row_whose_controls_are_all_disabled_is_not_entered() {
    let mut fx = form_list(Options {
        disabled_below: 2,
        ..Options::default()
    });
    let descriptions = row_descriptions(&mut fx.tree);
    for (position, description) in descriptions {
        assert_eq!(
            description.is_some(),
            position >= 2,
            "row {position}: described only when it has an enabled control"
        );
    }
    fx.selection.select(1);
    fx.settle();
    fx.key(Key::F2, Modifiers::NONE);
    assert_eq!(fx.tree.focused(), Some(fx.lv));
    assert_eq!(fx.entered(), None);
    fx.key(Key::Tab, Modifiers::NONE);
    assert_eq!(fx.tree.focused(), Some(fx.after));
}

#[test]
fn escape_in_a_combo_box_popup_closes_it_and_stays_in_the_row() {
    let model = ListModel::from_vec((0..4usize).collect());
    let choices: Vec<Signal<Option<String>>> = (0..4).map(|_| Signal::new(None)).collect();
    let picks = Rc::new(choices);
    let p2 = picks.clone();
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let lv = tree.add(
        ListView::new(model, move |_i, item, _selected| {
            Box::new(crate::ComboBox::new(
                ["Red", "Green", "Blue"],
                p2[*item].clone(),
            ))
        })
        .item_height(32.0)
        .row_keyboard(RowKeyboard::Actions),
    );
    let p = SizeProposal::exact(400.0, 240.0);
    tree.layout(p);
    tree.focus(lv);
    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    tree.layout(p);
    tree.press_key(Key::F2, Modifiers::NONE);
    tree.layout(p);
    let combo = tree.focused().expect("F2 focused the combo box");
    assert_ne!(combo, lv);

    tree.press_key(Key::ArrowDown, Modifiers::ALT);
    tree.layout(p);
    assert!(!tree.active_overlays().is_empty(), "the popup opened");

    tree.press_key(Key::Escape, Modifiers::NONE);
    tree.layout(p);
    assert!(tree.active_overlays().is_empty(), "Escape closed the popup");
    assert_ne!(tree.focused(), Some(lv), "and left focus in the row");
    let entered = {
        let any = tree.widget_as_any(lv).expect("as_any");
        any.downcast_ref::<ListView<usize>>()
            .expect("a ListView<usize>")
            .row_actions
            .as_ref()
            .and_then(|actions| actions.entered.get())
    };
    assert_eq!(entered, Some(0));

    tree.press_key(Key::Escape, Modifiers::NONE);
    tree.layout(p);
    assert_eq!(
        tree.focused(),
        Some(lv),
        "a second Escape returns to the list"
    );
}

struct TwoSections;

impl crate::GridSectionProvider for TwoSections {
    fn section_count(&self) -> usize {
        2
    }
    fn items_in_section(&self, _section: usize) -> usize {
        3
    }
    fn section_title(&self, section: usize) -> String {
        format!("S{section}")
    }
}

#[test]
fn section_header_controls_stay_tab_stops_in_action_mode() {
    let model = ListModel::from_vec((0..6usize).collect());
    let name: Vec<Signal<String>> = (0..6).map(|_| Signal::new(String::new())).collect();
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let lv = tree.add(
        ListView::new(model, move |_i, item, _selected| {
            Box::new(crate::TextInput::new(name[*item].clone()))
        })
        .item_height(32.0)
        .section_header_delegate(|_section, _title| {
            Box::new(crate::Button::new(teksilo_i18n::lit!("Play all")))
        })
        .sections(TwoSections)
        .row_keyboard(RowKeyboard::Actions),
    );
    let p = SizeProposal::exact(400.0, 400.0);
    tree.layout(p);
    tree.focus(lv);
    tree.layout(p);
    assert_eq!(
        tree.tab_stops_within(lv).len(),
        3,
        "the list and the two headers' buttons, no row field"
    );

    tree.press_key(Key::F2, Modifiers::NONE);
    tree.layout(p);
    assert_ne!(tree.focused(), Some(lv));
    assert_eq!(
        tree.tab_stops_within(lv).len(),
        4,
        "while a row is entered, its field too"
    );

    tree.press_key(Key::Escape, Modifiers::NONE);
    tree.layout(p);
    assert_eq!(tree.focused(), Some(lv));
    assert_eq!(tree.tab_stops_within(lv).len(), 3);
}

/// The row a screen reader is told is current, and the role of the node it is
/// told has focus.
fn focus_as_announced(tree: &mut WidgetTree) -> (Option<usize>, teksilo_core::accesskit::Role) {
    let snapshot = tree.accessibility_tree_snapshot();
    let consumer = accesskit_consumer::Tree::new(snapshot, true);
    let state = consumer.state();
    let focused = state
        .node_by_id(state.focus_id().expect("a focus"))
        .expect("the focused node");
    let resolved = focused.active_descendant().unwrap_or(focused);
    (resolved.position_in_set(), resolved.role())
}

#[test]
fn the_list_names_no_current_row_while_a_field_has_focus() {
    use teksilo_core::accesskit::Role;
    let mut fx = form_list(Options::default());
    fx.selection.select(2);
    fx.settle();
    // The consumer's `position_in_set` reads as the model index.
    assert_eq!(focus_as_announced(&mut fx.tree).0, Some(2));
    fx.key(Key::F2, Modifiers::NONE);
    let (_, role) = focus_as_announced(&mut fx.tree);
    assert_ne!(
        role,
        Role::ListBoxOption,
        "the field is the focus, not the row over it"
    );
    fx.key(Key::Escape, Modifiers::NONE);
    assert_eq!(
        focus_as_announced(&mut fx.tree),
        (Some(2), Role::ListBoxOption),
        "and back on the row after Escape"
    );
}

/// Each option's description, by its one-based position.
fn row_descriptions(tree: &mut WidgetTree) -> Vec<(usize, Option<String>)> {
    use teksilo_core::accesskit::Role;
    let snapshot = tree.accessibility_tree_snapshot();
    let consumer = accesskit_consumer::Tree::new(snapshot, true);
    let state = consumer.state();
    let mut out = Vec::new();
    fn walk(node: accesskit_consumer::NodeRef<'_>, out: &mut Vec<(usize, Option<String>)>) {
        if node.role() == Role::ListBoxOption {
            out.push((node.position_in_set().unwrap_or(0), node.description()));
        }
        for child in node.children() {
            walk(child, out);
        }
    }
    walk(state.root(), &mut out);
    out
}

#[test]
fn a_row_with_a_control_names_the_key_that_enters_it() {
    let mut fx = form_list(Options {
        controls_from: 2,
        ..Options::default()
    });
    let rows = row_descriptions(&mut fx.tree);
    assert!(!rows.is_empty());
    // Key names as this platform spells them (macOS writes glyphs).
    let named = |key| {
        crate::keystroke_format::format_keystroke(teksilo_core::shortcut::KeyStroke::new(
            key,
            Modifiers::NONE,
        ))
    };
    let (enter, escape) = (named(Key::Enter), named(Key::Escape));
    for (position, description) in rows {
        // `position_in_set` as the consumer reads it: the model index.
        if position < 2 {
            assert_eq!(description, None, "row {position} has nothing to enter");
        } else {
            let description = description.expect("a description");
            assert!(
                description.contains(&enter) && description.contains(&escape),
                "no on_activate: Enter enters ({description})"
            );
        }
    }

    let mut fx = form_list(Options {
        activate: true,
        ..Options::default()
    });
    let (_, description) = row_descriptions(&mut fx.tree).remove(0);
    assert!(
        description.expect("a description").contains("F2"),
        "with on_activate, F2 enters"
    );

    let mut fx = form_list(Options {
        mode: RowKeyboard::Toggle,
        ..Options::default()
    });
    let (_, description) = row_descriptions(&mut fx.tree).remove(0);
    assert_eq!(description, None, "Toggle says nothing");
}

#[test]
fn select_all_in_an_entered_row_does_not_select_every_row() {
    // A field answers select all with its own action. A control that does not
    // (a checkbox) left it to the list, which selected every row behind the
    // user's back.
    let model = ListModel::from_vec((0..6usize).collect());
    let checks = teksilo_data::CheckedModel::new();
    let selection = SelectionModel::new(SelectionMode::Multi);
    let ck = checks.clone();
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let lv = tree.add(
        ListView::new(model, move |i, _item, _selected| {
            Box::new(
                crate::checkbox::Checkbox::new(ck.signal_for(i)).label(teksilo_i18n::lit!("Row")),
            )
        })
        .item_height(32.0)
        .selection(selection.clone())
        .row_keyboard(RowKeyboard::Actions),
    );
    let p = SizeProposal::exact(400.0, 240.0);
    tree.layout(p);
    tree.focus(lv);
    selection.select(1);
    tree.layout(p);
    tree.press_key(Key::F2, Modifiers::NONE);
    tree.layout(p);
    assert_ne!(tree.focused(), Some(lv), "F2 reached the checkbox");

    tree.press_key(Key::A, Modifiers::COMMAND);
    tree.layout(p);
    assert_eq!(selection.selected_indices(), vec![1]);

    tree.press_key(Key::Escape, Modifiers::NONE);
    tree.layout(p);
    tree.press_key(Key::A, Modifiers::COMMAND);
    tree.layout(p);
    assert_eq!(
        selection.selected_indices().len(),
        6,
        "back on the list, select all is the list's again"
    );
}

/// Three rows, each a field and a two-item toolbar (a roving group: its
/// current item carries its own Tab-stop prop), above a button.
fn toolbar_rows() -> (
    WidgetTree,
    WidgetId,
    WidgetId,
    Rc<RefCell<Vec<(usize, WidgetId)>>>,
    SizeProposal,
) {
    use crate::{IconWidget, Toolbar, ToolbarAction};
    let model = ListModel::from_vec((0..3usize).collect());
    let names: Vec<Signal<String>> = (0..3).map(|_| Signal::new(String::new())).collect();
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let list = ListView::new(model, move |_i, item, _selected| {
        Box::new(
            crate::primitives::HStack::new()
                .child(crate::TextInput::new(names[*item].clone()))
                .child(
                    Toolbar::new()
                        .action(ToolbarAction::new(teksilo_i18n::lit!("One"), || {
                            IconWidget::checkmark(16.0)
                        }))
                        .action(ToolbarAction::new(teksilo_i18n::lit!("Two"), || {
                            IconWidget::checkmark(16.0)
                        })),
                ),
        )
    })
    .item_height(40.0)
    .row_keyboard(RowKeyboard::Actions);
    let rows = list.realized_row_ids();
    let lv = tree.add(list);
    let after = tree.add(crate::Button::new(teksilo_i18n::lit!("After")));
    let _root = tree.add(
        crate::primitives::VStack::new()
            .child(crate::primitives::FixedSize::new().height(200.0).child(lv))
            .child(after),
    );
    let p = SizeProposal::exact(600.0, 300.0);
    tree.layout(p);
    tree.focus(lv);
    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    tree.layout(p);
    (tree, lv, after, rows, p)
}

#[test]
fn a_roving_toolbar_in_a_row_is_a_tab_stop_only_while_the_row_is_entered() {
    // The toolbar's current item carries its own Tab-stop prop, nearer than
    // anything the row could set: only the row's gate keeps it out.
    let (mut tree, lv, _after, rows, p) = toolbar_rows();
    let row = rows.borrow()[0].1;
    let toolbar = |tree: &WidgetTree| tree.children(tree.children(row)[0])[1];
    // (the toolbar's stops as the list's traversal sees them, the row's, the
    // list's). `tab_stops_within(toolbar)` alone would start below the gate.
    let stops = |tree: &WidgetTree| {
        let in_list = tree.tab_stops_within(lv);
        let own = tree.tab_stops_within(toolbar(tree));
        (
            in_list.iter().filter(|id| own.contains(id)).count(),
            tree.tab_stops_within(row).len(),
            in_list.len(),
        )
    };
    assert_eq!(stops(&tree), (0, 0, 1), "before: the list is one Tab stop");

    tree.press_key(Key::F2, Modifiers::NONE);
    tree.layout(p);
    assert_eq!(
        stops(&tree),
        (1, 2, 3),
        "entered: the field and the toolbar"
    );

    tree.press_key(Key::Tab, Modifiers::NONE);
    tree.layout(p);
    let first = tree.focused();
    tree.press_key(Key::ArrowRight, Modifiers::NONE);
    tree.layout(p);
    assert_ne!(tree.focused(), first, "the toolbar's arrow moved its item");
    assert_eq!(stops(&tree), (1, 2, 3), "still one stop for the toolbar");

    tree.press_key(Key::Escape, Modifiers::NONE);
    tree.layout(p);
    assert_eq!(tree.focused(), Some(lv));
    assert_eq!(
        stops(&tree),
        (0, 0, 1),
        "after: the list is one Tab stop again"
    );
}

#[test]
fn tab_leaves_a_list_whose_rows_hold_a_toolbar() {
    let (mut tree, lv, after, _rows, p) = toolbar_rows();
    tree.press_key(Key::Tab, Modifiers::NONE);
    tree.layout(p);
    assert_eq!(tree.focused(), Some(after), "Tab from the list leaves it");

    tree.focus(lv);
    tree.layout(p);
    tree.press_key(Key::F2, Modifiers::NONE);
    tree.layout(p);
    tree.press_key(Key::Escape, Modifiers::NONE);
    tree.layout(p);
    assert_eq!(tree.focused(), Some(lv));
    tree.press_key(Key::Tab, Modifiers::NONE);
    tree.layout(p);
    assert_eq!(
        tree.focused(),
        Some(after),
        "and still after a visit to a row"
    );
}

#[test]
fn caps_lock_and_a_pan_do_not_leave_a_row_that_is_away() {
    // Neither is the user moving on: Caps Lock is a lone modifier, and a
    // finger panning the list starts on a row while scrolling back to it.
    let mut fx = form_list(Options {
        rows: 60,
        plain_from: 20,
        ..Options::default()
    });
    fx.selection.select(1);
    fx.settle();
    fx.key(Key::F2, Modifiers::NONE);
    fx.key(Key::Tab, Modifiers::NONE);
    let scroll = fx.scroll();
    scroll.set(1600.0);
    fx.settle();
    assert_eq!(fx.tree.focused(), Some(fx.lv));

    fx.key(Key::CapsLock, Modifiers::NONE);
    let at = fx.tree.bounds(fx.row(42)).center();
    fx.tree
        .touch_drag(at, teksilo_canvas::Point::new(at.x, at.y + 80.0), 8);
    fx.settle();
    assert_eq!(fx.entered(), Some(1), "still waiting");

    scroll.set(0.0);
    fx.settle();
    assert_eq!(
        fx.typing_goes_to("u"),
        Some((1, 1)),
        "and back in the same field"
    );
    fx.tree.assert_no_leaked_pointer_state();
}

#[test]
fn an_assistive_click_on_a_row_while_the_row_is_away_leaves_it() {
    let mut fx = form_list(Options {
        rows: 60,
        plain_from: 20,
        ..Options::default()
    });
    fx.selection.select(1);
    fx.settle();
    fx.key(Key::F2, Modifiers::NONE);
    let scroll = fx.scroll();
    scroll.set(1600.0);
    fx.settle();
    let node = teksilo_core::accessibility::widget_id_to_node_id(fx.row(42));
    fx.tree.dispatch_access_action(
        node,
        teksilo_core::accesskit::Action::Click,
        None,
        &mut teksilo_core::window::NoopWindowOps,
    );
    fx.settle();
    assert_eq!(fx.entered(), None);
    scroll.set(0.0);
    fx.settle();
    assert_eq!(
        fx.tree.focused(),
        Some(fx.lv),
        "row 1 did not take focus back"
    );
}
