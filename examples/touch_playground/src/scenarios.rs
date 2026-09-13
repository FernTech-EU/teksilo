// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The arbitration scenarios: five shapes where two contenders want the same
//! press, each reporting which one got it.
//!
//! # How a winner is observed
//!
//! Not from the tree. `WidgetTree::sequence_winner` names the node that won a
//! pointer's arbitration, and it is public — but it is the *tree's*, and a
//! widget has no handle on the tree, so an application cannot read it from a
//! handler. What an application can see is the **consequence**: a scroll offset
//! that moved, a value that changed, a model whose order changed, a selection
//! that appeared. Each scenario therefore watches the one signal each contender
//! would move if it won, and publishes the name of whichever moved.
//!
//! That is a strictly weaker observation than the tree's, and the difference
//! matters in one direction only: a contender that wins and then does nothing
//! reads here as "nothing won". None of the five has that shape — each
//! contender's win is a visible change — but a scenario added later must check
//! it before trusting the readout.
//!
//! # What the readout is *not*
//!
//! It is not the arbitration matrix. `crates/teksilo-core/tests/arbitration_
//! matrix.rs` and the table it generates into `docs/events-and-gestures.md`
//! describe **core-only fixtures reproducing a widget's arbitration shape**, not
//! the widgets themselves, and the table says so. These scenarios are the real
//! widgets. Where the two agree that is worth knowing; where they disagree the
//! widget is the fact and the fixture is a model of it.

use std::rc::Rc;

use teksilo::core::build_context::BuildContext;
use teksilo::core::signal::Signal;
use teksilo::core::widget::Widget;
use teksilo::core::widget_id::WidgetId;
use teksilo::data::{DataChange, ListModel, SelectionMode, SelectionModel};
use teksilo::i18n::lit;
use teksilo::text_document::TextDocument;
use teksilo::tokens::{Orientation, SurfaceRole, TextRole, TextStyleRole};
use teksilo::widgets::primitives::{MinSize, Padding, ZStack};
use teksilo::widgets::rich_text::RichTextEditor;
use teksilo::widgets::scroll_area::ScrollArea;
use teksilo::widgets::splitter::{PaneDescriptor, Splitter, SplitterModel};
use teksilo::widgets::{HStack, ListView, Slider, StandardListItem, TextWidget, VStack};

use crate::state::{
    PlaygroundState, SCENARIO_GRIP, SCENARIO_LIST_ROW, SCENARIO_NESTED, SCENARIO_SLIDER,
    SCENARIO_TEXT, ScenarioId,
};

/// How far a scroll offset must move before it counts as a pan rather than as
/// the settle of something else. Well under a row, well over float noise.
const PAN_EPSILON: f32 = 0.5;

/// The demo list's row height.
///
/// Public because the scenario tests aim a press at a row *centre* rather than
/// at the view's, and they must not carry their own copy of this number: a
/// deferred drag is revoked if the first sample after the hold leaves the row
/// that was pressed, so where inside the row the press lands decides whether the
/// reorder can latch at all. See `docs/data-view-touch.md` §2.
pub const LIST_ITEM_HEIGHT: f32 = 40.0;

/// A scenario's frame: heading, the live winner readout, instructions, and the
/// subject itself.
fn framed(
    ctx: &mut BuildContext,
    state: &PlaygroundState,
    scenario: ScenarioId,
    contenders: &str,
    instruction: &str,
    subject: impl Widget + 'static,
) -> WidgetId {
    let outcomes = state.outcomes.clone();
    let key = scenario;
    let winner = outcomes.map(move |map| {
        map.get(key)
            .cloned()
            .unwrap_or_else(|| "— no gesture yet".to_string())
    });
    ctx.add(
        Padding::uniform(10.0).child(
            VStack::new()
                .spacing(6.0)
                .child(
                    HStack::new()
                        .spacing(8.0)
                        .child(TextWidget::new(lit!(scenario)).style(TextStyleRole::BodyBold))
                        .child(
                            TextWidget::new(lit!(""))
                                .text(winner)
                                .color(TextRole::Accent),
                        ),
                )
                .child(
                    TextWidget::new(lit!(contenders))
                        .style(TextStyleRole::Tiny)
                        .color(TextRole::Secondary),
                )
                .child(
                    TextWidget::new(lit!(instruction))
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary),
                )
                .child(subject),
        ),
    )
}

/// A row of the fifty-item demo list.
fn list_items() -> Vec<String> {
    (1..=50).map(|i| format!("Row {i}")).collect()
}

/// **A list row inside the scroller that carries it.**
///
/// Three contenders for one press: the scroller's pan claim, the row's own
/// reorder drag, and the row's selection. All three are the real `ListView`'s,
/// declared by `ListView::reorderable` and the selection model it is given.
pub fn list_row(ctx: &mut BuildContext, state: &PlaygroundState) -> WidgetId {
    let model = ListModel::from_vec(list_items());
    let selection = SelectionModel::new(SelectionMode::Single);

    let view = ListView::new(model.clone(), |_index, item: &String, _selected| {
        Box::new(StandardListItem::new(lit!(item.clone())))
    })
    .selection(selection.clone())
    .reorderable(true)
    .item_height(LIST_ITEM_HEIGHT);

    // The scroller's win: its offset moved.
    let scroll_y = view.scroll_y_signal().clone();
    {
        let state = state.clone();
        let last = Rc::new(std::cell::Cell::new(scroll_y.get()));
        ctx.effect(&scroll_y, move |y| {
            if (*y - last.get()).abs() > PAN_EPSILON {
                last.set(*y);
                state.report(SCENARIO_LIST_ROW, format!("pan — the scroller ({y:.0} dp)"));
            }
        });
    }
    // The row's reorder win: the model's order changed.
    {
        let state = state.clone();
        ctx.own_handle(model.observe_changes(move |change| {
            if let DataChange::ItemsMoved { from, to, .. } = change {
                state.report(
                    SCENARIO_LIST_ROW,
                    format!("reorder — the row ({from} → {to})"),
                );
            }
        }));
    }
    // The row's selection win.
    {
        let state = state.clone();
        ctx.effect(&selection.selection_signal(), move |set| {
            if let Some(index) = set.iter().next() {
                state.report(SCENARIO_LIST_ROW, format!("select — the row (#{index})"));
            }
        });
    }

    framed(
        ctx,
        state,
        SCENARIO_LIST_ROW,
        "contenders: the scroller's pan claim · the row's reorder drag · the row's selection",
        "Finger: drag to scroll. Hold a row, then drag, to reorder it. Tap to select. \
         Mouse: drag reorders straight away; the wheel scrolls.",
        MinSize::new(0.0, 200.0).child(view),
    )
}

/// **A slider inside a scroll area.**
///
/// The slider declares `TouchAction::NONE` over itself, which is what keeps the
/// enclosing claimant out of the press's member list entirely — so dragging the
/// thumb never pans, on any pointer, and the area still pans everywhere else.
pub fn slider(ctx: &mut BuildContext, state: &PlaygroundState) -> WidgetId {
    let value = Signal::new(50.0_f32);
    {
        let state = state.clone();
        ctx.effect(&value, move |v| {
            state.report(SCENARIO_SLIDER, format!("the slider ({v:.0})"));
        });
    }

    let area = ScrollArea::new().child(
        VStack::new()
            .spacing(14.0)
            .child(TextWidget::new(lit!("Drag the thumb — nothing pans.")))
            .child(Slider::new(value.clone(), 0.0, 100.0).label(lit!("Value")))
            .child(TextWidget::new(lit!(
                "Drag anywhere else in this box — the box pans."
            )))
            .children((1..=12).map(|i| TextWidget::new(lit!(format!("filler line {i}"))))),
    );
    {
        let state = state.clone();
        let scroll_y = area.scroll_y_signal().clone();
        let last = Rc::new(std::cell::Cell::new(scroll_y.get()));
        ctx.effect(&scroll_y, move |y| {
            if (*y - last.get()).abs() > PAN_EPSILON {
                last.set(*y);
                state.report(SCENARIO_SLIDER, format!("pan — the scroller ({y:.0} dp)"));
            }
        });
    }

    framed(
        ctx,
        state,
        SCENARIO_SLIDER,
        "contenders: the scroll area's pan claim · the slider's own drag",
        "A press on the thumb belongs to the slider whatever the device; the pan \
         is not even enrolled.",
        MinSize::new(0.0, 180.0).child(area),
    )
}

/// **A text editor that both pans and selects.**
///
/// This is also the playground's text-touch surface: the caret lands on the
/// release, a hold selects the word under the finger and raises handles, and a
/// drag pans because the editor's frozen action admits the claim it agrees with.
pub fn text(ctx: &mut BuildContext, state: &PlaygroundState) -> WidgetId {
    let document = TextDocument::new();
    document
        .set_plain_text(
            "Touch text editing, in one paragraph you can work on.\n\n\
         Drag: the editor pans, because a direct pointer's press belongs to the \
         pan and the frozen action admits it. Tap: the caret lands when the \
         finger lifts, not when it touches down — so a tap that slides off \
         commits nothing. Hold: the word under the contact is selected, two \
         handles come up, and dragging a handle moves that end of the range. \
         Drag a handle to the edge and the view scrolls to follow it.\n\n\
         A mouse still selects by dragging, from the press, exactly as before.",
        )
        .expect("seed the paragraph");
    let editor = RichTextEditor::editor(document).min_lines(6).max_lines(10);

    {
        let state = state.clone();
        let scroll_y = editor.scroll_y();
        let last = Rc::new(std::cell::Cell::new(scroll_y.get()));
        ctx.effect(&scroll_y, move |y| {
            if (*y - last.get()).abs() > PAN_EPSILON {
                last.set(*y);
                state.report(SCENARIO_TEXT, format!("pan — the editor ({y:.0} dp)"));
            }
        });
    }
    {
        let state = state.clone();
        ctx.effect(&editor.has_selection(), move |selected| {
            if *selected {
                state.report(SCENARIO_TEXT, "select — a word, from the hold");
            }
        });
    }
    {
        let state = state.clone();
        let selected = editor.has_selection();
        ctx.effect(&editor.cursor_position_signal(), move |offset| {
            if !selected.get() {
                state.report(SCENARIO_TEXT, format!("caret — placed at {offset}"));
            }
        });
    }

    framed(
        ctx,
        state,
        SCENARIO_TEXT,
        "contenders: the editor's own pan claim · the caret · the hold's word selection",
        "Tap for a caret, hold for a word and its handles, drag to pan. The caret \
         commits on the release, so sliding off cancels it.",
        editor,
    )
}

/// **A splitter grip inside a scroll area.**
///
/// The grip takes the pointer with an explicit `capture_pointer`, which for an
/// *indirect* pointer decides the arbitration at the press. A contact's capture
/// does not: it still has to out-latch the claimant, which is why the grip is
/// reachable with a finger at all rather than being shadowed by the pan.
pub fn grip(ctx: &mut BuildContext, state: &PlaygroundState) -> WidgetId {
    let model = SplitterModel::from_panes(
        vec![
            PaneDescriptor::new().size(140.0).min_size(60.0),
            PaneDescriptor::new().size(140.0).min_size(60.0),
        ],
        Orientation::Horizontal,
    );
    {
        let state = state.clone();
        let sizes = model.clone();
        ctx.effect(&model.version(), move |_| {
            state.report(
                SCENARIO_GRIP,
                format!(
                    "the grip ({:.0} / {:.0} dp)",
                    sizes.stored_size(0),
                    sizes.stored_size(1)
                ),
            );
        });
    }

    let splitter = Splitter::new(model)
        .pane(pane_fill("leading", SurfaceRole::Raised))
        .pane(pane_fill("trailing", SurfaceRole::Sunken));

    let area = ScrollArea::new().child(
        VStack::new()
            .spacing(10.0)
            .child(MinSize::new(0.0, 120.0).child(splitter))
            .children((1..=10).map(|i| TextWidget::new(lit!(format!("pan me — filler line {i}"))))),
    );
    {
        let state = state.clone();
        let scroll_y = area.scroll_y_signal().clone();
        let last = Rc::new(std::cell::Cell::new(scroll_y.get()));
        ctx.effect(&scroll_y, move |y| {
            if (*y - last.get()).abs() > PAN_EPSILON {
                last.set(*y);
                state.report(SCENARIO_GRIP, format!("pan — the scroller ({y:.0} dp)"));
            }
        });
    }

    framed(
        ctx,
        state,
        SCENARIO_GRIP,
        "contenders: the scroll area's pan claim · the grip's explicit capture",
        "The gutter is painted at the theme's grab size and reaches the \
         conformance floor by an outset. Drag it with a finger, then with a mouse.",
        MinSize::new(0.0, 200.0).child(area),
    )
}

/// A labelled filler pane for the splitter scenario. `RectWidget` takes no
/// child, so the fill and the label are layered.
fn pane_fill(label: &str, role: SurfaceRole) -> impl Widget + 'static {
    let label = label.to_string();
    ZStack::new()
        .child(teksilo::widgets::primitives::RectWidget::new().background(role))
        .child(
            Padding::uniform(8.0).child(TextWidget::new(lit!(label)).style(TextStyleRole::Small)),
        )
}

/// **Two scrollers, one inside the other.**
///
/// The boundary question: when the inner surface reaches its end, does the
/// gesture stop there or carry on outward? `OverscrollBehavior::Chain` — the
/// default — hands it out; `Contain` absorbs it. A finger's pan and a wheel
/// notch take the same decision, which is what makes the mouse guarantee
/// structural rather than a coincidence.
pub fn nested(ctx: &mut BuildContext, state: &PlaygroundState) -> WidgetId {
    let inner_model = ListModel::from_vec(
        (1..=14)
            .map(|i| format!("inner row {i}"))
            .collect::<Vec<_>>(),
    );
    let inner = ListView::new(inner_model, |_i, item: &String, _s| {
        Box::new(StandardListItem::new(lit!(item.clone())))
    })
    .item_height(34.0);
    {
        let state = state.clone();
        let scroll_y = inner.scroll_y_signal().clone();
        let last = Rc::new(std::cell::Cell::new(scroll_y.get()));
        ctx.effect(&scroll_y, move |y| {
            if (*y - last.get()).abs() > PAN_EPSILON {
                last.set(*y);
                state.report(SCENARIO_NESTED, format!("the inner list ({y:.0} dp)"));
            }
        });
    }

    let outer = ScrollArea::new().child(
        VStack::new()
            .spacing(10.0)
            .children((1..=4).map(|i| TextWidget::new(lit!(format!("outer line {i}")))))
            .child(MinSize::new(0.0, 150.0).child(inner))
            .children((5..=16).map(|i| TextWidget::new(lit!(format!("outer line {i}"))))),
    );
    {
        let state = state.clone();
        let scroll_y = outer.scroll_y_signal().clone();
        let last = Rc::new(std::cell::Cell::new(scroll_y.get()));
        ctx.effect(&scroll_y, move |y| {
            if (*y - last.get()).abs() > PAN_EPSILON {
                last.set(*y);
                state.report(SCENARIO_NESTED, format!("the outer area ({y:.0} dp)"));
            }
        });
    }

    framed(
        ctx,
        state,
        SCENARIO_NESTED,
        "contenders: the inner list's pan claim · the outer area's",
        "Pan inside the inner list to its end, keep going: the outer area takes \
         over. Flick it and let go to hand a velocity to the fling driver.",
        MinSize::new(0.0, 240.0).child(outer),
    )
}

/// Every scenario, stacked in the order [`crate::state::ALL_SCENARIOS`] names.
pub fn all(ctx: &mut BuildContext, state: &PlaygroundState) -> Vec<WidgetId> {
    vec![
        list_row(ctx, state),
        slider(ctx, state),
        text(ctx, state),
        grip(ctx, state),
        nested(ctx, state),
    ]
}

/// How long a hold has to last before a reorder or a word selection is armed,
/// read off the touch profile rather than retyped. Shown in the instructions so
/// a tester is not guessing at the threshold.
pub fn long_press_millis() -> u64 {
    teksilo::tokens::GestureProfile::TOUCH
        .long_press
        .as_millis() as u64
}
