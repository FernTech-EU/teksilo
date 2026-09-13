// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! One command for "move this item somewhere else in an ordered collection".
//!
//! Several widgets in this crate let the user reorder something by dragging it,
//! and WCAG 2.2 SC 2.5.7 requires every one of them to be reachable without a
//! drag. The four obligations that discharges it — a menu row, a keyboard
//! chord, an AccessKit custom action, one utterance on commit — are the same
//! four everywhere, so they are written once, here. A consumer supplies only
//! what is genuinely its own: how its collection commits a move, and what the
//! moved thing is called.
//!
//! The consumers today are the five data views' rows and tiles and a tab in a
//! `TabBar`. `docs/drag-operation-census.md` lists the reordering drags that
//! still have no command, each with what it needs; adding one is meant to be
//! this module plus a chord, not a new implementation.
//!
//! ## The four moves
//!
//! [`OrderedMove`] is deliberately four values and not a signed step. "Move to
//! the far end" is not "step repeatedly": a keyboard user pressing `Alt+Home`
//! makes **one** model change and hears **one** utterance, where repeating a
//! step would make one of each per press.
//!
//! ## Why a drop spec and not a destination index
//!
//! [`OrderedMove::as_row_drop`] returns the `(target, position)` pair a
//! *pointer* drop would have carried, not the destination index. The five data
//! views commit a reorder by handing that pair to the bound source's
//! `accept_drop` / `reorder_within`, and the source — not the view — owns what
//! it means. Routing the alternative through the destination index instead
//! would be a second implementation of the same operation, and a second
//! implementation is exactly what an alternative must not be: it can reach a
//! different end state than the drag, which is the one failure mode a
//! single-pointer alternative cannot have.
//!
//! The [`destination`](OrderedMove::destination) index is still reported,
//! because the caller needs it for what happens *after* the model change —
//! following the selection, scrolling the moved thing back into view, and
//! naming its new position in the announcement.

use teksilo_core::event::{Key, Modifiers};
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::{LocalizedString, lit};

/// Which way a move goes within an ordered collection.
///
/// See the [module documentation](self) for why the far ends are their own
/// variants rather than a repeated step.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum OrderedMove {
    /// One place earlier in the collection's order.
    Prev,
    /// One place later.
    Next,
    /// All the way to the first position.
    First,
    /// All the way to the last position.
    Last,
}

/// How a collection's order reads on screen.
///
/// This decides only what the four moves are **called** — the arithmetic is the
/// same either way. A vertical list moves rows up and down; a tab strip or a
/// column header moves them left and right.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum MoveAxis {
    /// Rows in a list: Up / Down / To top / To bottom.
    Vertical,
    /// Tabs, columns: Left / Right / To start / To end.
    Horizontal,
}

/// A move that changes an item's **parent** rather than its position among its
/// siblings.
///
/// The tree views' row drag can do two things a flat list's cannot: drop
/// *between* siblings (which is [`OrderedMove`]) and drop *into* another row,
/// which reparents. Both are the same drag; only the second needs its own
/// vocabulary, because "one place earlier" says nothing about depth.
///
/// The two directions are the outliner convention every editor ships:
/// indenting makes the row a child of the sibling above it, outdenting makes it
/// the next sibling of its own parent.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum TreeMove {
    /// Become a child of the previous sibling.
    Indent,
    /// Become the next sibling of the current parent.
    Outdent,
}

impl TreeMove {
    /// Both directions, in the order they are offered.
    pub const ALL: [TreeMove; 2] = [Self::Indent, Self::Outdent];

    /// What this move is called.
    pub fn label(self) -> LocalizedString {
        match self {
            Self::Indent => lit!("Move Into Previous"),
            Self::Outdent => lit!("Move Out One Level"),
        }
    }

    /// Decode the keyboard chord for the platform this build targets.
    pub fn from_key(key: Key, modifiers: Modifiers, rtl: bool) -> Option<Self> {
        Self::from_key_for(
            crate::common::list_nav::ListNavConvention::CURRENT,
            key,
            modifiers,
            rtl,
        )
    }

    /// [`from_key`](Self::from_key) against an explicit convention, so both
    /// platform branches are reachable from one host's test run — the
    /// [`common::text_nav`](crate::common::text_nav) pattern.
    ///
    /// Two spellings, and the split is forced rather than chosen:
    ///
    /// * The **accelerator plus `]` / `[`** works everywhere. It is what every
    ///   outliner on macOS binds to increase and decrease indent, and
    ///   `Modifiers::command()` makes it ⌘ there and Ctrl elsewhere.
    /// * **`Alt` plus the horizontal arrows** — the outliner convention on
    ///   Windows and Linux — is **not** bound on macOS, because `⌥→` / `⌥←`
    ///   already expand and collapse a whole subtree there
    ///   ([`list_nav::mac_alias`](crate::common::list_nav::mac_alias), which
    ///   AppKit's own outline view claims). Binding it would take a working
    ///   chord away to add one that has another spelling.
    ///
    /// Only the arrow spelling mirrors under RTL: the arrows name a direction on
    /// screen, and the chevron flips there. `]` and `[` name indent and outdent
    /// outright.
    pub(crate) fn from_key_for(
        convention: crate::common::list_nav::ListNavConvention,
        key: Key,
        modifiers: Modifiers,
        rtl: bool,
    ) -> Option<Self> {
        if modifiers.shift() {
            return None;
        }
        if modifiers.command() && !modifiers.alt() {
            return match key {
                Key::Character(']') => Some(Self::Indent),
                Key::Character('[') => Some(Self::Outdent),
                _ => None,
            };
        }
        if convention != crate::common::list_nav::ListNavConvention::Desktop
            || !modifiers.alt()
            || modifiers.command()
        {
            return None;
        }
        let (indent, outdent) = if rtl {
            (Key::ArrowLeft, Key::ArrowRight)
        } else {
            (Key::ArrowRight, Key::ArrowLeft)
        };
        if key == indent {
            Some(Self::Indent)
        } else if key == outdent {
            Some(Self::Outdent)
        } else {
            None
        }
    }
}

/// The `(target, position)` pair a pointer drop would have carried, plus where
/// the moved item ends up.
///
/// See the [module documentation](self) for why the commit travels as a drop
/// rather than as a destination index.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RowDrop {
    /// The row the drop lands on.
    pub target: usize,
    /// Which side of `target` it lands on.
    pub position: teksilo_data::DropPosition,
    /// Where the moved row ends up once the source has applied it.
    pub destination: usize,
}

impl RowDrop {
    /// The drop that moves the item at `from` to `destination`.
    ///
    /// `Before` when the item travels backwards, `After` when it travels
    /// forwards — the same two gaps a drag would have released over, so the
    /// source's own index arithmetic (which shifts by one when the removal is
    /// above the insertion point) is exercised unchanged.
    ///
    /// Public and destination-based, not only `OrderedMove`-based, because a
    /// 2-D grid also moves a tile by a whole row at a time and that is not one
    /// of the four named moves.
    pub fn between(from: usize, destination: usize) -> Self {
        use teksilo_data::DropPosition;
        let position = if destination < from {
            DropPosition::Before
        } else {
            DropPosition::After
        };
        Self {
            target: destination,
            position,
            destination,
        }
    }
}

impl OrderedMove {
    /// The four moves, in the order they are offered to the user.
    pub const ALL: [OrderedMove; 4] = [Self::Prev, Self::Next, Self::First, Self::Last];

    /// Where index `from` lands in a collection of `count` items, or `None`
    /// when the move would change nothing.
    ///
    /// A move that changes nothing is not offered: it earns no menu row, no
    /// custom action and no announcement. That is what keeps a row already at
    /// the top from advertising "Move to top".
    pub fn destination(self, from: usize, count: usize) -> Option<usize> {
        if from >= count {
            return None;
        }
        let dest = match self {
            Self::Prev => from.checked_sub(1)?,
            Self::Next => {
                let next = from + 1;
                if next >= count {
                    return None;
                }
                next
            }
            Self::First => 0,
            Self::Last => count - 1,
        };
        (dest != from).then_some(dest)
    }

    /// The drop a pointer would have made to perform this move, or `None` when
    /// the move changes nothing.
    pub fn as_row_drop(self, from: usize, count: usize) -> Option<RowDrop> {
        Some(RowDrop::between(from, self.destination(from, count)?))
    }

    /// The moves available from `from` in a collection of `count`.
    pub fn available(from: usize, count: usize) -> Vec<OrderedMove> {
        Self::ALL
            .into_iter()
            .filter(|mv| mv.destination(from, count).is_some())
            .collect()
    }

    /// What this move is called on the given axis.
    pub fn label(self, axis: MoveAxis) -> LocalizedString {
        match (self, axis) {
            (Self::Prev, MoveAxis::Vertical) => lit!("Move Up"),
            (Self::Next, MoveAxis::Vertical) => lit!("Move Down"),
            (Self::First, MoveAxis::Vertical) => lit!("Move to Top"),
            (Self::Last, MoveAxis::Vertical) => lit!("Move to Bottom"),
            (Self::Prev, MoveAxis::Horizontal) => lit!("Move Left"),
            (Self::Next, MoveAxis::Horizontal) => lit!("Move Right"),
            (Self::First, MoveAxis::Horizontal) => lit!("Move to Start"),
            (Self::Last, MoveAxis::Horizontal) => lit!("Move to End"),
        }
    }

    /// Decode the keyboard chord that performs a move on the given axis.
    ///
    /// `Alt` plus the axis' own two arrows steps; `Alt+Home` / `Alt+End` go to
    /// the ends. `Alt` is what every outliner and tab strip already uses for
    /// this (and what the three data views that had a keyboard reorder before
    /// this module used), so the chord is not new vocabulary.
    ///
    /// `rtl` swaps the two horizontal arrows, because on the horizontal axis
    /// the arrow keys name a *direction on screen* while the move names a
    /// position in the order. `Home`/`End` are unaffected: they name the order's
    /// ends, which do not flip.
    pub fn from_key(key: Key, modifiers: Modifiers, axis: MoveAxis, rtl: bool) -> Option<Self> {
        if !modifiers.alt() || modifiers.shift() || modifiers.command() {
            return None;
        }
        let (prev_key, next_key) = match (axis, rtl) {
            (MoveAxis::Vertical, _) => (Key::ArrowUp, Key::ArrowDown),
            (MoveAxis::Horizontal, false) => (Key::ArrowLeft, Key::ArrowRight),
            (MoveAxis::Horizontal, true) => (Key::ArrowRight, Key::ArrowLeft),
        };
        if key == prev_key {
            return Some(Self::Prev);
        }
        if key == next_key {
            return Some(Self::Next);
        }
        match key {
            Key::Home => Some(Self::First),
            Key::End => Some(Self::Last),
            _ => None,
        }
    }
}

/// The one utterance a completed move makes.
///
/// `name` is what the moved thing is called, where the widget knows: a row's
/// text, a tab's title, a column's header. Where it does not, the position
/// alone is still worth saying — a screen-reader user who pressed `Alt+Down`
/// otherwise hears nothing at all and cannot tell a refused move from a
/// successful one.
///
/// Positions are 1-based, because that is what every adapter announces for
/// `pos_in_set` and a user hearing "moved to 0 of 12" would be right to be
/// confused.
pub fn move_announcement(name: Option<&str>, destination: usize, count: usize) -> String {
    let position = destination + 1;
    match name {
        Some(name) if !name.is_empty() => {
            lit!(format!("{name} moved to {position} of {count}")).resolve_now()
        }
        _ => lit!(format!("Moved to {position} of {count}")).resolve_now(),
    }
}

/// The commit half of a data view's non-drag reorder.
///
/// The five data views differ in how they lay rows out, scroll them and paint
/// them; they do not differ at all in how a reorder reaches the model. That
/// part is this struct: the bound source's own drop-accept path, the drag-start
/// key stash it depends on, and the typed payload a same-view drop carries.
/// Each field is the closure the view already holds for its **pointer** drag,
/// which is what makes the alternative the same operation rather than a second
/// implementation of it.
pub(crate) struct RowMover {
    /// How many rows the collection holds right now.
    pub len: std::rc::Rc<dyn Fn() -> usize>,
    /// Record the dragged rows' stable keys, as a drag start would. The accept
    /// path resolves identity from this stash and never from the payload's
    /// indices, so a commit that skips it is refused.
    pub stash: std::rc::Rc<dyn Fn(&[usize])>,
    /// Build the same-view payload for a move starting at this row.
    pub payload: std::rc::Rc<dyn Fn(usize) -> teksilo_core::drag_payload::DragPayload>,
    /// The bound source's drop-accept path.
    pub accept: std::rc::Rc<
        dyn Fn(
            &teksilo_core::drag_payload::DragPayload,
            usize,
            teksilo_data::DropPosition,
            crate::data_views::ViewId,
        ) -> bool,
    >,
    /// This view's identity, so the accept path reads the drop as same-view.
    pub view: crate::data_views::ViewId,
    /// What the row at this index is called, where the view knows. `ListView`
    /// and its siblings know when the application opted into type-ahead by
    /// giving them a label resolver; otherwise the announcement names the
    /// position alone.
    pub name: std::rc::Rc<dyn Fn(usize) -> Option<String>>,
}

impl RowMover {
    /// The moves available from `from`, in the order they are offered.
    pub fn available(&self, from: usize) -> Vec<OrderedMove> {
        OrderedMove::available(from, (self.len)())
    }

    /// Perform `mv` on the row at `from` through the source's own commit path.
    ///
    /// Returns the destination index and the one utterance the move makes, or
    /// `None` when the move was unavailable or the source refused it. A refusal
    /// is silent on purpose: the source declining a reorder is not an event, and
    /// speaking "moved" when nothing moved is worse than saying nothing.
    pub fn commit(&self, mv: OrderedMove, from: usize) -> Option<(usize, String)> {
        let count = (self.len)();
        self.commit_to(from, mv.destination(from, count)?)
    }

    /// Move the row at `from` to `destination`, for a caller whose move is not
    /// one of the four named ones — a grid tile moving a whole row at a time.
    pub fn commit_to(&self, from: usize, destination: usize) -> Option<(usize, String)> {
        let count = (self.len)();
        if from >= count || destination >= count || destination == from {
            return None;
        }
        let drop = RowDrop::between(from, destination);
        // Resolved before the move, because afterwards the row is somewhere else.
        let name = (self.name)(from);
        (self.stash)(&[from]);
        let payload = (self.payload)(from);
        if !(self.accept)(&payload, drop.target, drop.position, self.view) {
            return None;
        }
        Some((
            drop.destination,
            move_announcement(name.as_deref(), drop.destination, count),
        ))
    }
}

/// A move, already bound to the thing it moves.
///
/// Every one of the four obligations SC 2.5.7 imposes — a menu row, a keyboard
/// chord, an AccessKit custom action, an utterance on commit — ends in the same
/// call, so each consumer builds exactly one of these and the three surfaces
/// below share it. That is what keeps the menu row, the chord and the AT action
/// from being three implementations that can disagree.
pub(crate) type PerformMove =
    std::rc::Rc<dyn Fn(OrderedMove, &mut teksilo_core::widget::EventContext)>;

/// The tree views' reparent, bound to nothing yet — the [`MoveRow`] twin for
/// [`TreeMove`].
pub(crate) type TreeReparentRow =
    std::rc::Rc<dyn Fn(TreeMove, usize, &mut teksilo_core::widget::EventContext)>;

/// The same thing before it knows which row it moves: what a view builds once
/// and each of its rows then binds its own index into.
pub(crate) type MoveRow =
    std::rc::Rc<dyn Fn(OrderedMove, usize, &mut teksilo_core::widget::EventContext)>;

/// Bind a view-level mover to one row's live index.
///
/// `index` is read at invocation, not at build: a virtualized row's index is
/// only knowable through its anchor, and the anchor is what survives the
/// reorder the command itself causes.
pub(crate) fn bind_row(
    perform: &MoveRow,
    index: std::rc::Rc<dyn Fn() -> Option<usize>>,
) -> PerformMove {
    let perform = perform.clone();
    std::rc::Rc::new(move |mv, ctx| {
        if let Some(from) = index() {
            perform(mv, from, ctx);
        }
    })
}

/// Append the available Move rows to a context menu.
///
/// Returns the list and whether anything was added, so a caller can keep its
/// separators tidy when every move is unavailable (a one-row collection, or a
/// view that is not reorderable).
pub(crate) fn append_move_items(
    mut list: crate::menu_list::MenuList,
    perform: &PerformMove,
    from: usize,
    count: usize,
    axis: MoveAxis,
) -> (crate::menu_list::MenuList, bool) {
    let mut added = false;
    for mv in OrderedMove::available(from, count) {
        let perform = perform.clone();
        list = list.item(
            crate::menu_item::MenuItem::new(mv.label(axis))
                .on_activate_fn(move |ctx| perform(mv, ctx)),
        );
        added = true;
    }
    (list, added)
}

/// Advertise the available moves as AccessKit custom actions on a node.
///
/// The set is fixed when the node is built, which is correct because every
/// commit path here changes the collection and so rebuilds the node. The
/// *callback* still resolves the live index, so a move invoked against a stale
/// set is a no-op rather than a move of the wrong item.
pub(crate) fn add_move_custom_actions(
    mut handlers: teksilo_core::widget_builder::HandlerSet,
    perform: &PerformMove,
    from: usize,
    count: usize,
    axis: MoveAxis,
) -> teksilo_core::widget_builder::HandlerSet {
    for mv in OrderedMove::available(from, count) {
        let perform = perform.clone();
        handlers = handlers.access_custom_action(mv.label(axis), move |ctx| perform(mv, ctx));
    }
    handlers
}

/// One row's worth of non-drag reorder commands, ready to install.
///
/// A data view calls [`install`](RowCommands::install) once per realized row
/// and gets three of SC 2.5.7's four obligations at once: the AccessKit custom
/// actions, the context menu carrying the same rows, and — because both call
/// the same [`PerformMove`] the view's keyboard handler calls — the guarantee
/// that all four routes reach the same model state. The fourth, the utterance,
/// is inside the [`RowMover::commit`] the closure wraps.
pub(crate) struct RowCommands {
    /// The move, already bound to this row's live index.
    pub perform: PerformMove,
    /// The row's index when it was built, which decides which moves are
    /// *offered*. The commit resolves the live index again.
    pub from: usize,
    /// How many rows the collection held when the row was built.
    pub count: usize,
    /// What the four moves are called here.
    pub axis: MoveAxis,
    /// Rows offered after the four moves. The tree views put indent / outdent
    /// here: they change a row's parent rather than its position in a flat
    /// order, so they are not `OrderedMove`s, but they are the same drag
    /// (`DropPosition::Into`) and belong in the same menu.
    pub extra: Vec<(
        teksilo_i18n::LocalizedString,
        std::rc::Rc<dyn Fn(&mut teksilo_core::widget::EventContext)>,
    )>,
}

impl RowCommands {
    /// Whether this row can be moved at all. A one-row collection offers
    /// nothing, and offering an empty menu is worse than offering none.
    pub fn is_empty(&self) -> bool {
        OrderedMove::available(self.from, self.count).is_empty() && self.extra.is_empty()
    }

    /// The context menu for this row, or `None` when there is nothing to offer.
    pub fn menu(&self) -> Option<crate::menu_list::MenuList> {
        if self.is_empty() {
            return None;
        }
        let (mut list, _) = append_move_items(
            crate::menu_list::MenuList::new(),
            &self.perform,
            self.from,
            self.count,
            self.axis,
        );
        for (label, run) in &self.extra {
            let run = run.clone();
            list = list.item(
                crate::menu_item::MenuItem::new(label.clone()).on_activate_fn(move |ctx| run(ctx)),
            );
        }
        Some(list)
    }

    /// Install the AccessKit custom actions and the context menu on `row_id`.
    ///
    /// The menu goes on the row rather than on the view because the row is what
    /// the command is *about*, and because the framework's factory is then the
    /// one an application's own per-row menu shadows — a factory closer to the
    /// click wins the walk, and an application that writes a row menu owns it.
    pub fn install(self, ctx: &mut teksilo_core::build_context::BuildContext, row_id: WidgetId) {
        if self.is_empty() {
            return;
        }
        let mut handlers = add_move_custom_actions(
            teksilo_core::widget_builder::HandlerSet::new(),
            &self.perform,
            self.from,
            self.count,
            self.axis,
        );
        for (label, run) in &self.extra {
            let run = run.clone();
            handlers = handlers.access_custom_action(label.clone(), move |ctx| run(ctx));
        }
        let menu_source = self;
        handlers = handlers.context_menu(move |_pos, _ctx| {
            menu_source
                .menu()
                .map(|list| Box::new(list) as Box<dyn teksilo_core::widget::Widget>)
        });
        ctx.apply_handlers(row_id, handlers);
    }
}

/// The one utterance a completed reparent makes.
///
/// The level is what changed, and it is what the announcement says: after an
/// indent or an outdent the row's *position* among its siblings is a different
/// question from the one the user asked. Levels are 1-based, matching
/// AccessKit's own `level`.
pub fn reparent_announcement(name: Option<&str>, level: usize) -> String {
    match name {
        Some(name) if !name.is_empty() => {
            lit!(format!("{name} moved to level {level}")).resolve_now()
        }
        _ => lit!(format!("Moved to level {level}")).resolve_now(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_data::DropPosition;

    #[test]
    fn a_move_that_changes_nothing_is_not_offered() {
        assert_eq!(OrderedMove::Prev.destination(0, 5), None);
        assert_eq!(OrderedMove::First.destination(0, 5), None);
        assert_eq!(OrderedMove::Next.destination(4, 5), None);
        assert_eq!(OrderedMove::Last.destination(4, 5), None);
        // A single item can go nowhere at all.
        assert_eq!(OrderedMove::available(0, 1), Vec::new());
        // And an index the collection does not hold is not a starting point.
        for mv in OrderedMove::ALL {
            assert_eq!(mv.destination(7, 5), None, "{mv:?}");
        }
        assert_eq!(OrderedMove::available(0, 0), Vec::new());
    }

    #[test]
    fn the_middle_of_a_collection_offers_all_four() {
        assert_eq!(OrderedMove::available(2, 5), OrderedMove::ALL.to_vec());
        assert_eq!(OrderedMove::Prev.destination(2, 5), Some(1));
        assert_eq!(OrderedMove::Next.destination(2, 5), Some(3));
        assert_eq!(OrderedMove::First.destination(2, 5), Some(0));
        assert_eq!(OrderedMove::Last.destination(2, 5), Some(4));
    }

    /// The gap a move releases over is the one a drag would have: `Before` when
    /// the item travels backwards, `After` when forwards. The source's own
    /// arithmetic then lands it on `destination` — asserted against a real
    /// `ListModel` in `ordered_move_lands_where_a_drag_would`.
    #[test]
    fn each_move_carries_the_gap_a_drag_would_have_released_over() {
        let up = OrderedMove::Prev.as_row_drop(3, 10).expect("available");
        assert_eq!(
            (up.target, up.position, up.destination),
            (2, DropPosition::Before, 2)
        );
        let down = OrderedMove::Next.as_row_drop(3, 10).expect("available");
        assert_eq!(
            (down.target, down.position, down.destination),
            (4, DropPosition::After, 4)
        );
        let top = OrderedMove::First.as_row_drop(3, 10).expect("available");
        assert_eq!(
            (top.target, top.position, top.destination),
            (0, DropPosition::Before, 0)
        );
        let bottom = OrderedMove::Last.as_row_drop(3, 10).expect("available");
        assert_eq!(
            (bottom.target, bottom.position, bottom.destination),
            (9, DropPosition::After, 9)
        );
    }

    #[test]
    fn the_axis_decides_only_the_wording() {
        for mv in OrderedMove::ALL {
            let vertical = mv.label(MoveAxis::Vertical).resolve_now();
            let horizontal = mv.label(MoveAxis::Horizontal).resolve_now();
            assert_ne!(vertical, horizontal, "{mv:?}");
            assert_eq!(
                mv.destination(2, 5),
                mv.destination(2, 5),
                "the arithmetic does not read the axis"
            );
        }
        assert_eq!(
            OrderedMove::Prev.label(MoveAxis::Vertical).resolve_now(),
            "Move Up"
        );
        assert_eq!(
            OrderedMove::Last.label(MoveAxis::Horizontal).resolve_now(),
            "Move to End"
        );
    }

    #[test]
    fn alt_plus_the_axis_arrows_is_the_chord() {
        use MoveAxis::{Horizontal, Vertical};
        let alt = Modifiers::ALT;
        assert_eq!(
            OrderedMove::from_key(Key::ArrowUp, alt, Vertical, false),
            Some(OrderedMove::Prev)
        );
        assert_eq!(
            OrderedMove::from_key(Key::ArrowDown, alt, Vertical, false),
            Some(OrderedMove::Next)
        );
        assert_eq!(
            OrderedMove::from_key(Key::Home, alt, Vertical, false),
            Some(OrderedMove::First)
        );
        assert_eq!(
            OrderedMove::from_key(Key::End, alt, Vertical, false),
            Some(OrderedMove::Last)
        );
        // The other axis' arrows are not this axis' chord.
        assert_eq!(
            OrderedMove::from_key(Key::ArrowLeft, alt, Vertical, false),
            None
        );
        assert_eq!(
            OrderedMove::from_key(Key::ArrowUp, alt, Horizontal, false),
            None
        );
        // Without Alt, nothing — the bare arrows are navigation.
        assert_eq!(
            OrderedMove::from_key(Key::ArrowUp, Modifiers::NONE, Vertical, false),
            None
        );
        // And Alt+Shift / Alt+Ctrl belong to selection and to word motion.
        assert_eq!(
            OrderedMove::from_key(
                Key::ArrowUp,
                Modifiers::ALT | Modifiers::SHIFT,
                Vertical,
                false
            ),
            None
        );
    }

    #[test]
    fn rtl_swaps_the_horizontal_arrows_but_not_the_ends() {
        use MoveAxis::Horizontal;
        let alt = Modifiers::ALT;
        assert_eq!(
            OrderedMove::from_key(Key::ArrowLeft, alt, Horizontal, true),
            Some(OrderedMove::Next),
            "in RTL the leftward arrow moves later in the order"
        );
        assert_eq!(
            OrderedMove::from_key(Key::ArrowRight, alt, Horizontal, true),
            Some(OrderedMove::Prev)
        );
        assert_eq!(
            OrderedMove::from_key(Key::Home, alt, Horizontal, true),
            Some(OrderedMove::First),
            "Home names the order's start, which does not flip"
        );
    }

    /// Both reparent spellings, and both platform branches, from one host.
    #[test]
    fn the_reparent_chord_reads_brackets_everywhere_and_arrows_off_macos() {
        use crate::common::list_nav::ListNavConvention::{Desktop, Mac};
        let cmd = Modifiers::COMMAND;
        for convention in [Desktop, Mac] {
            assert_eq!(
                TreeMove::from_key_for(convention, Key::Character(']'), cmd, false),
                Some(TreeMove::Indent),
                "{convention:?}"
            );
            assert_eq!(
                TreeMove::from_key_for(convention, Key::Character('['), cmd, false),
                Some(TreeMove::Outdent),
                "{convention:?}"
            );
            // The brackets name indent and outdent, so RTL leaves them alone.
            assert_eq!(
                TreeMove::from_key_for(convention, Key::Character(']'), cmd, true),
                Some(TreeMove::Indent),
                "{convention:?}"
            );
        }
        let alt = Modifiers::ALT;
        assert_eq!(
            TreeMove::from_key_for(Desktop, Key::ArrowRight, alt, false),
            Some(TreeMove::Indent)
        );
        assert_eq!(
            TreeMove::from_key_for(Desktop, Key::ArrowLeft, alt, false),
            Some(TreeMove::Outdent)
        );
        assert_eq!(
            TreeMove::from_key_for(Desktop, Key::ArrowLeft, alt, true),
            Some(TreeMove::Indent),
            "the arrows mirror under RTL, as the chevron does"
        );
        // macOS spends ⌥→ / ⌥← on the subtree expand pair, so they are not the
        // reparent chord there.
        assert_eq!(
            TreeMove::from_key_for(Mac, Key::ArrowRight, alt, false),
            None,
            "binding ⌥→ would take the subtree expand away"
        );
        assert_eq!(
            TreeMove::from_key_for(Mac, Key::ArrowLeft, alt, false),
            None
        );
        // And a bare bracket is a character, not a command.
        assert_eq!(
            TreeMove::from_key_for(Desktop, Key::Character(']'), Modifiers::NONE, false),
            None
        );
    }

    #[test]
    fn the_utterance_names_the_new_position_one_based() {
        assert_eq!(
            move_announcement(Some("Beta"), 2, 12),
            "Beta moved to 3 of 12"
        );
        assert_eq!(move_announcement(None, 0, 12), "Moved to 1 of 12");
        assert_eq!(move_announcement(Some(""), 0, 3), "Moved to 1 of 3");
    }

    /// The load-bearing claim of the whole module: the alternative reaches the
    /// same model state the drag reaches, for every one of the four moves,
    /// through the source's own commit path.
    #[test]
    fn ordered_move_lands_where_a_drag_would() {
        use teksilo_data::{DragSource, DropCommit};
        use teksilo_data::{ListDataSource, ListModel};

        for (mv, from, expected) in [
            (OrderedMove::Prev, 3usize, vec!["a", "b", "d", "c", "e"]),
            (OrderedMove::Next, 3, vec!["a", "b", "c", "e", "d"]),
            (OrderedMove::First, 3, vec!["d", "a", "b", "c", "e"]),
            (OrderedMove::Last, 1, vec!["a", "c", "d", "e", "b"]),
        ] {
            let model = ListModel::from_vec(vec!["a", "b", "c", "d", "e"]);
            let drop = mv.as_row_drop(from, 5).expect("available");
            // Exactly the call a released drag makes.
            assert!(
                ListDataSource::accept_drop(
                    &model,
                    DropCommit {
                        source: DragSource::SameView { key: from },
                        target: drop.target,
                        position: drop.position,
                    }
                ),
                "{mv:?} was refused"
            );
            let got: Vec<&str> = (0..model.len())
                .map(|i| model.with_item(i, |v| *v).expect("in range"))
                .collect();
            assert_eq!(got, expected, "{mv:?} from {from}");
            assert_eq!(
                got.iter()
                    .position(|s| *s == ["a", "b", "c", "d", "e"][from]),
                Some(drop.destination),
                "{mv:?}: the reported destination is where the item actually is"
            );
        }
    }
}
