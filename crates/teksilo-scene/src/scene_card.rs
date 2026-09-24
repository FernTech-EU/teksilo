// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`SceneCard`] — the chrome and the gesture regime of a heavyweight scene
//! item, with **no opinion about what is inside it**.
//!
//! # Why it is a card and not a note
//!
//! A note container, a pinned image, an embedded web page, a chart, a group of
//! dried ink and a sub-page thumbnail want the same five things — a surface, a
//! grab handle, a selection state, an edit state, and an accessibility shape —
//! and differ only in their body. Naming the type after one of those bodies
//! would put text-specific policy (a document, a commit, a word count) into a
//! crate whose whole claim is that the heavyweight tier is *any widget*. So the
//! card owns the container and the app owns the content:
//!
//! ```ignore
//! SceneCard::new(model.clone(), id)
//!     .height_for_width()
//!     .label(title.clone())
//!     .header(TextWidget::new(title))
//!     .body(RichTextEditor::editor(doc))
//! ```
//!
//! # The gesture regime, which is the part that could not be written outside
//!
//! Three presses, three different owners, decided **structurally** rather than
//! by a recognizer race:
//!
//! | press lands on | who owns the gesture | why |
//! | --- | --- | --- |
//! | the header | the header's own `on_drag` | the innermost node with a drag owns the sequence outright, and no ancestor is enrolled |
//! | the body | whatever the body installed (a text field's selection drag) | no drag on the captured node, so the walk climbs — and stops at the card root's dead zone |
//! | the card background | nothing but the card's own tap | the card root **is** the dead-zone boundary, so nothing above it arms |
//!
//! The load-bearing line is
//! [`gesture_dead_zone(true)`](teksilo_core::widget_builder::WidgetBuilder::gesture_dead_zone)
//! on the card root. Without it the `SceneView`'s marquee is enrolled as an
//! ancestor of every press inside the card, and dragging from the middle of a
//! note rubber-bands the page behind it. With it, a drag inside a card is the
//! card's business and the canvas never sees it.
//!
//! The trailing header slot is wrapped in a
//! [`DeadZone`](teksilo_widgets::primitives::DeadZone) of its own — the
//! `Accordion` header precedent exactly — so the `⋮` button can be clicked
//! *with the jitter a real click carries* without starting a move.
//!
//! # What drags a card, and what resizes it
//!
//! The **header** moves it, always. The selection frame the
//! [transform controller](crate::transform_session) draws moves and resizes it
//! too, once the card is selected — and that frame is drawn `padding` outside
//! the selection precisely so it lands on pixels the card does not own. There
//! is deliberately no third route: a body drag belongs to the body (that is how
//! you select text in an embedded editor), and a card with no header is a card
//! you move by its frame.
//!
//! Both routes end in the same model write. The header drag accumulates a
//! scene-space translation, previews it as a node transform, runs it past the
//! document's standing rule with
//! [`SceneModel::constrain_move`](crate::SceneModel::constrain_move) — the same
//! door the built-in drag uses, so a snap-to-grid rule cannot mean two things —
//! and commits **once**, on release, through
//! [`SceneModel::apply_transform_delta`](crate::SceneModel::apply_transform_delta).
//! One gesture is one reversible step, and a cancelled gesture has nothing to
//! roll back because nothing was written.
//!
//! # Modes, and why there is no `on_commit`
//!
//! [`CardMode`] is a `Signal` the **app owns**. The card writes into it
//! (`Editing` on activation, `Selected` when focus leaves its subtree or `Esc`
//! is pressed) and reads it for its chrome and its accessibility state. An app An app
//! that wants to persist on commit observes that signal; there is no second
//! `on_commit` callback, because the trigger for the most important case —
//! focus leaving the subtree — is the framework's `focus_within` signal, which
//! is written outside event dispatch. A callback there could not be handed an
//! `EventContext`, so it would be a worse `Signal` with a misleading shape.
//! [`on_activate`](SceneCard::on_activate) *does* take one, because an
//! activation is a gesture or a key and has a dispatch to belong to.
//!
//! # Accessibility
//!
//! **One** `Role::Group` per card, named by [`label`](SceneCard::label),
//! carrying `selected` when the mode says so and a custom **Edit** action that
//! is the non-pointer twin of the double-click. It is a tab stop, so a keyboard
//! user reaches every card with Tab, enters one with Enter and leaves with Esc.
//!
//! One, not two: the default surface is announced as a
//! `Role::GenericContainer` with no properties, which is the role the
//! accessibility walker prunes, promoting its children in order. Left alone,
//! `Card` publishes a second, nameless `Role::Group` *inside* the card's own,
//! and a screen reader reads that as a container within a container. The net
//! result is node-for-node identical to the hand-rolled `Panel` a note page
//! used before — with the container **named**, where the panel's was not.
//! Pinned by `a_card_publishes_no_more_nodes_than_the_hand_rolled_panel_it_replaces`.
//!
//! Everything inside the card is walked by the framework's ordinary walker —
//! the header's text, the trailing button, and a body that publishes
//! `Role::TextRun` children all hang off the group in the emitted tree.
//!
//! They reach a screen reader, too. The default surface is
//! `teksilo_widgets::Card`, whose `RecipeCardStyle` frame is a bare
//! `Role::GenericContainer`: the walker prunes it, and
//! `accesskit_consumer::common_filter`, which every platform adapter and this
//! repo's own `accessibility::audit` read through, drops such a node while
//! keeping its children. The frame once said "presentational" with
//! `set_hidden()` instead, which that filter reads as `ExcludeSubtree`, and a
//! card with a title and a body read `Window > Pane > Group "Note"` with
//! nothing inside it. The parity test counts nodes in the update, where a
//! hidden subtree still sits node for node, so it could not see that;
//! `a_cards_title_and_body_reach_a_screen_reader` walks the filtered tree
//! the way an adapter does and pins it.
//!
//! ## Tab stops, and the one thing the card does not decide
//!
//! The card contributes exactly **one** tab stop — itself, the object handle.
//! It does not take its body's away while idle, and the reason is not
//! reluctance: `set_tab_stop` reaches one node, and a composite body's tab
//! stops are its own inner nodes. Nothing short of parking the body dormant
//! takes a subtree out of the Tab ring.
//!
//! So a card with a focusable body is two stops, always. An app that wants one
//! stop per idle note puts a [`Switcher`](teksilo_widgets::Switcher) in the
//! body — a read-only viewer and an editor, driven by the same
//! [`CardMode`] — because a `Switcher` parks its hidden branch dormant, which
//! takes it out of focus, hit-testing *and* the accessibility tree while
//! keeping it mounted, so a mode round-trip does not destroy the caret. That
//! shape is the framework's answer to this question and the card defers to it
//! rather than building a worse one.
//!
//! ## …and the one frame that shape costs
//!
//! Deferring to the `Switcher` has a consequence the card has to answer for:
//! **the editor does not exist yet at the moment the user asks for it.**
//! A double-click flips [`CardMode`] inside one dispatch, and
//! `EventContext::request_focus_into` is drained at the end of that same
//! dispatch — but the `Switcher`'s branch is parked until its `visible_when`
//! gate is evaluated, which happens in the *next* layout pass. So the obvious
//! code focuses a subtree that has nothing focusable in it, the caret never
//! appears, and the two recommended shapes — one tab stop, and focus on
//! activation — are mutually exclusive.
//!
//! The card closes that by asking twice. `CardHandle::activate` asks straight
//! away, which is what a plain focusable body wants (instant, same dispatch),
//! and a private zero-size `EditFocusGate` child asks again from
//! [`BuildContext::run_after_mount`](teksilo_core::build_context::BuildContext::run_after_mount)
//! — the one hook that hands a widget a real `EventContext` *after* the pass
//! that mounts its siblings. The second ask is skipped when the card's
//! `focus_within` already says the keyboard is inside, so a body that took the
//! focus itself (or an `on_activate` that placed it deliberately) is never
//! yanked back to the first field. The caret lands one frame after the
//! double-click instead of zero, and it lands.
//!
//! The same gate is what makes the two-way [`mode`](SceneCard::mode) contract
//! true from the outside: a toolbar button that writes `Editing` into the
//! signal gets the body focused too, with no dispatch of its own to do it in.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect, Size, SizeProposal, Transform2D, Vec2};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, Modifiers, WidgetEvent};
use teksilo_core::gesture::DragPhase;
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::styles::CardVariant;
use teksilo_core::widget::{EventContext, LayoutContext, LayoutResponse, PendingChild, Widget};
use teksilo_core::widget_builder::{HandlerSet, WidgetBuilder};
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::lit;

use crate::item::ItemId;
use crate::scene::SizePolicy;
use crate::scene_model::SceneModel;
use crate::selection::SceneSelection;
use crate::transform_session::{TransformDelta, TransformFrame, TransformSource};

/// What a [`SceneCard`] is doing.
///
/// Three states rather than a `bool`, because `Selected` and `Editing` differ
/// in *both* the accessibility shape and the gesture regime: a selected card is
/// an object the canvas can move, and an editing card has handed the keyboard —
/// arrows, Backspace, type-ahead — to its content.
///
/// `#[non_exhaustive]`: this crate has out-of-tree consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum CardMode {
    /// Not selected, not editing. The resting state.
    #[default]
    Idle,
    /// Part of the scene selection: move, resize and delete apply.
    Selected,
    /// The body has the keyboard.
    Editing,
}

impl CardMode {
    /// Whether this mode reads as "selected" to assistive technology. `Editing`
    /// does: a card being edited is the selected one.
    pub fn is_selected(self) -> bool {
        matches!(self, CardMode::Selected | CardMode::Editing)
    }
}

/// In-flight state of a header drag. Lives behind an `Rc<Cell<..>>` because the
/// `on_drag` closure outlives the `&self` that installed it.
#[derive(Debug, Clone, Copy)]
struct DragState {
    /// The card's scene rect when the gesture started — the frame a geometry
    /// constraint measures its answer against.
    start: Rect,
    /// Where the press landed, in the header's own coordinates, measured with
    /// no preview in force. See [`SceneCard::build_header`] for why the drag is
    /// anchored rather than accumulated.
    anchor: Point,
    /// Scene-space translation currently previewed — what the constraint
    /// returned for the latest sample.
    applied: Vec2,
    /// The translation the pointer asked for, before the constraint. Recomputed
    /// from scratch each sample rather than accumulated, so a rule that snaps to
    /// a grid is always asked about the pointer's real displacement and never
    /// about the remainder of its own previous answer.
    raw: Vec2,
}

type Callback = Rc<dyn Fn(&mut EventContext)>;
type Surface = Rc<dyn Fn(WidgetId) -> Box<dyn Widget>>;

/// The second half of "enter edit mode and hand the keyboard to the body" —
/// the half that cannot be done from the dispatch that starts the edit.
///
/// A zero-size child of the card whose only job is to notice `Editing`
/// arriving and ask for the focus again **from the far side of a layout
/// pass**, where a `Switcher`'s editor branch has finally been woken by its
/// `visible_when` gate. See the `scene_card` module docs' "the one frame that
/// shape costs" for why the obvious code cannot work.
///
/// It publishes no accessibility node and is not marked hidden to achieve
/// that: emitting nothing leaves a content-free `Role::Unknown`, which is
/// exactly what the walker collapses out of the tree. Marking it hidden would
/// *add* a node, because the hidden flag is a property and a node carrying one
/// is no longer content-free.
///
/// It is a widget rather than a method on the card because the trigger has to
/// be a `BindingLevel::Rebuild` binding, and putting one of those on the card
/// would rebuild the card's whole chrome on every `Idle` ↔ `Selected` flip —
/// which is every click and every marquee. Here it rebuilds one leaf with no
/// children, and only when the *editing* half of the mode changes.
struct EditFocusGate {
    /// The card's mode, narrowed to the only transition that matters.
    editing: Signal<bool>,
    /// The card's `focus_within`. `true` means the keyboard is already
    /// somewhere inside the card, so the gate stays out of the way rather than
    /// yanking it back to the body's first field.
    focus_within: Signal<bool>,
    /// What to focus into.
    body: WidgetId,
    /// What the previous build saw, so the gate fires on the **transition**
    /// into `Editing` and not on every rebuild that happens to find it true.
    was_editing: bool,
}

impl std::fmt::Debug for EditFocusGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditFocusGate")
            .field("editing", &self.editing.get())
            .finish()
    }
}

impl Widget for EditFocusGate {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        self.editing
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Rebuild);
        let editing = self.editing.get();
        let entering = editing && !self.was_editing;
        self.was_editing = editing;
        if entering && !self.focus_within.get() {
            let body = self.body;
            // Drained by the app loop after this layout pass (and by
            // `WidgetTree::run_mount_actions` in a headless test), which is the
            // first moment the editor branch is awake. `request_focus_into`
            // leaves focus alone when the subtree still has nothing focusable,
            // so a card whose body is an image or a chart is unaffected.
            ctx.run_after_mount(move |ctx| ctx.request_focus_into(body));
        }
        vec![]
    }

    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::ZERO.into()
    }
}

/// A heavyweight scene item's container: surface, header, body, modes,
/// gestures and accessibility. See the `scene_card` module docs.
pub struct SceneCard {
    model: SceneModel,
    item: ItemId,
    header: Option<WidgetId>,
    header_pending: Option<Box<dyn Widget>>,
    trailing: Option<WidgetId>,
    trailing_pending: Option<Box<dyn Widget>>,
    body: Option<WidgetId>,
    body_pending: Option<Box<dyn Widget>>,
    surface: Option<Surface>,
    mode: Signal<CardMode>,
    selection: Option<SceneSelection>,
    label: Prop<String>,
    activate_label: Prop<String>,
    movable: bool,
    on_activate: Option<Callback>,
    /// The drag preview, as a node-level self transform. Bound at
    /// `RepaintOnly`: a card being dragged does not relayout, which is what
    /// keeps a ten-card selection from re-laying-out per pointer sample.
    preview: Signal<Transform2D>,
    drag: Rc<Cell<Option<DragState>>>,
    focus_within: Signal<bool>,
    root: Option<WidgetId>,
    /// What `build` returned, which is what `children` owes the arena.
    child_ids: Vec<WidgetId>,
}

impl std::fmt::Debug for SceneCard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SceneCard")
            .field("item", &self.item)
            .field("mode", &self.mode.get())
            .field("movable", &self.movable)
            .finish()
    }
}

/// The part of a [`SceneCard`] its handlers need, as something a closure can
/// own.
///
/// A `HandlerSet` closure is built once and lives in the arena long after the
/// `&mut SceneCard` that installed it is gone, so the behaviour cannot live on
/// `&self` — the same shape `TransformDriver` takes inside the view, and for
/// the same reason.
#[derive(Clone)]
struct CardHandle {
    model: SceneModel,
    item: ItemId,
    mode: Signal<CardMode>,
    selection: Option<SceneSelection>,
    body: Option<WidgetId>,
    root: Option<WidgetId>,
    on_activate: Option<Callback>,
}

impl CardHandle {
    /// Enter edit mode: flip the signal and hand the keyboard to the body.
    ///
    /// The hand-over is asked for **twice**, and has to be: this dispatch can
    /// only reach a body that is already mounted and awake, which the
    /// recommended [`Switcher`](teksilo_widgets::Switcher) shape's editor branch
    /// is not until the next layout pass. `EditFocusGate` asks again from the
    /// far side of that pass. See the module docs' "the one frame that shape
    /// costs".
    fn activate(&self, ctx: &mut EventContext) {
        let before = self.mode.get();
        if let Some(f) = &self.on_activate {
            f(ctx);
        }
        // **One rule: if the hook moved the mode, the hook decided.** It runs
        // before the flip precisely so it can, and every way it might want to
        // is the same write: `Editing` because it drove its own flow, anything
        // else because it is refusing this one. Comparing against what the hook
        // *found* rather than against a fixed value is what makes the refusal
        // expressible at all — a double-click arrives here with the card
        // already `Selected` (the press selected it), so "wrote `Selected`"
        // cannot mean anything, while "wrote something other than what was
        // there" can.
        if self.mode.get() != before {
            return;
        }
        self.select(ctx);
        self.mode.set(CardMode::Editing);
        if let Some(body) = self.body {
            // A no-op when the body has nothing focusable, which is the right
            // answer for an image or a chart: the card keeps the focus and the
            // mode still reads `Editing` for whatever the app does with it.
            ctx.request_focus_into(body);
        }
    }

    /// Leave edit mode and take the keyboard back onto the card itself, so
    /// `Tab` continues from the object rather than from inside it.
    fn deactivate(&self, ctx: &mut EventContext) {
        if self.mode.get() != CardMode::Editing {
            return;
        }
        self.mode.set(CardMode::Selected);
        if let Some(root) = self.root {
            ctx.request_focus(root);
        }
    }

    /// Select the card and bring it forward. The selection half is a no-op when
    /// the app wired no [`SceneSelection`]; the z half is a model mutation, so
    /// every view of this model restacks.
    fn select(&self, _ctx: &mut EventContext) {
        if let Some(sel) = &self.selection {
            sel.select_one(self.item);
        }
        if self.mode.get() == CardMode::Idle {
            self.mode.set(CardMode::Selected);
        }
        self.model.bring_to_front(self.item);
    }
}

impl SceneCard {
    /// A card bound to one heavyweight entry of `model`.
    ///
    /// The card **is** the widget the view's delegate returns for `item`:
    ///
    /// ```ignore
    /// SceneView::with_model(model.clone())
    ///     .delegate_typed::<Note>(move |note, id| {
    ///         Box::new(SceneCard::new(model.clone(), id).body(view_of(note)))
    ///     })
    /// ```
    pub fn new(model: SceneModel, item: ItemId) -> Self {
        Self {
            model,
            item,
            header: None,
            header_pending: None,
            trailing: None,
            trailing_pending: None,
            body: None,
            body_pending: None,
            surface: None,
            mode: Signal::new(CardMode::Idle),
            selection: None,
            label: Prop::Static(String::new()),
            activate_label: lit!("Edit").into(),
            movable: true,
            on_activate: None,
            preview: Signal::new(Transform2D::identity()),
            drag: Rc::new(Cell::new(None)),
            focus_within: Signal::new(false),
            root: None,
            child_ids: Vec::new(),
        }
    }

    /// The grab handle, and the card's title strip.
    ///
    /// A press here moves the card and nothing else — see the
    /// `scene_card` module docs for the three-row table that makes that true.
    ///
    /// Takes a widget, a `Box<dyn Widget>` (the shape a card factory hands
    /// back), or a [`WidgetId`] already in the tree — one method per slot, so
    /// the caller does not pick a spelling to match what they happen to hold.
    pub fn header(mut self, widget: impl teksilo_core::IntoTeksiChild) -> Self {
        Self::put(
            teksilo_core::IntoTeksiChild::into_pending(widget),
            &mut self.header,
            &mut self.header_pending,
        );
        self
    }

    /// A control at the trailing end of the header — a `⋮` menu, a colour
    /// swatch, a pin.
    ///
    /// Wrapped in a [`DeadZone`](teksilo_widgets::primitives::DeadZone), so the
    /// header drags everywhere **except** here and a click on the control with
    /// a few pixels of jitter still reads as a click.
    ///
    /// Takes a widget, a boxed widget or a [`WidgetId`], like
    /// [`header`](Self::header).
    pub fn header_trailing(mut self, widget: impl teksilo_core::IntoTeksiChild) -> Self {
        Self::put(
            teksilo_core::IntoTeksiChild::into_pending(widget),
            &mut self.trailing,
            &mut self.trailing_pending,
        );
        self
    }

    /// The content. Anything at all: a text editor, an image, a chart, a
    /// `SceneView` of its own.
    ///
    /// Not wrapped in anything — the card root's dead zone already covers it,
    /// and wrapping would put a node between the body and the card that the
    /// body's own gesture arena would have to argue with.
    ///
    /// Takes a widget, a boxed widget or a [`WidgetId`], like
    /// [`header`](Self::header).
    pub fn body(mut self, widget: impl teksilo_core::IntoTeksiChild) -> Self {
        Self::put(
            teksilo_core::IntoTeksiChild::into_pending(widget),
            &mut self.body,
            &mut self.body_pending,
        );
        self
    }

    /// File a slot's child into whichever of the two stores fits it, clearing
    /// the other.
    ///
    /// The pair is what `build` reads (`id.or_else(|| pending.take())`), so a
    /// method that only ever *set* its own store would make an id shadow a
    /// widget handed over later: last call has to win, and that means the
    /// losing store is cleared here rather than left to the resolution order.
    fn put(child: PendingChild, id: &mut Option<WidgetId>, pending: &mut Option<Box<dyn Widget>>) {
        match child {
            PendingChild::Id(w) => {
                *id = Some(w);
                *pending = None;
            }
            PendingChild::Deferred(w) => {
                *pending = Some(w);
                *id = None;
            }
        }
    }

    /// Replace the chrome. The closure is handed the id of the card's content
    /// (header plus body, already stacked) and returns the widget that wraps
    /// it.
    ///
    /// The default is
    /// `Card::new().variant(CardVariant::Elevated).content(content)`, so the
    /// card is Tier-3 themed through the existing `style_slots.card` with no
    /// new style protocol. Override it for a different variant, a per-card
    /// colour, or a surface of your own:
    ///
    /// ```ignore
    /// card.surface(|content| {
    ///     Box::new(Card::new().variant(CardVariant::Outlined).content(content))
    /// })
    /// ```
    pub fn surface(mut self, f: impl Fn(WidgetId) -> Box<dyn Widget> + 'static) -> Self {
        self.surface = Some(Rc::new(f));
        self
    }

    /// Bind the card's mode to an app-owned signal.
    ///
    /// Two-way: the card writes into it and reads from it, so a toolbar button
    /// or a shortcut can put a card into `Editing` and the card will focus its
    /// body. Observe it to persist on commit — see the `scene_card` module docs for
    /// why there is no separate `on_commit`.
    pub fn mode(mut self, mode: Signal<CardMode>) -> Self {
        self.mode = mode;
        self
    }

    /// The card's mode signal, for an app that did not supply one.
    pub fn mode_signal(&self) -> Signal<CardMode> {
        self.mode.clone()
    }

    /// The scene selection this card takes part in.
    ///
    /// Pass the same handle the view was given
    /// ([`SceneView::selection_model`](crate::SceneView::selection_model)), so
    /// a tap on the card and a marquee across it mean the same thing. Without
    /// one the card still tracks its own [`CardMode`], but a tap selects
    /// nothing.
    pub fn selection(mut self, selection: SceneSelection) -> Self {
        self.selection = Some(selection);
        self
    }

    /// The card's accessible name. Locale-reactive when given a `tr!` string.
    pub fn label(mut self, label: impl Into<Prop<String>>) -> Self {
        self.label = label.into();
        self
    }

    /// The name of the custom accessibility action that enters edit mode.
    /// Defaults to an untranslated `"Edit"`.
    pub fn activate_label(mut self, label: impl Into<Prop<String>>) -> Self {
        self.activate_label = label.into();
        self
    }

    /// Whether the header moves the card. `true` by default; `false` leaves the
    /// header as an ordinary strip (and the selection frame as the only way to
    /// move the card).
    pub fn movable(mut self, movable: bool) -> Self {
        self.movable = movable;
        self
    }

    /// Give the card's **height** to its content:
    /// [`SizePolicy::HeightForWidth`] on the entry.
    ///
    /// The width stays the model's — you resize a note by dragging its edge —
    /// and the height follows the words on every pass that lays the card out.
    ///
    /// One consequence worth knowing before you reach for it: **the height axis
    /// of the selection frame does nothing on such a card.** Not "writes a value
    /// that is then corrected" — nothing at all. The scale's vertical component
    /// is neutralised at the model door
    /// ([`Scene::apply_transform_delta`](crate::Scene::apply_transform_delta)),
    /// because a height written there is one the next pass measures straight
    /// back over: the gesture's whole contribution would be a reversible step
    /// that undoes nothing, and a top-edge drag would *move* the card rather
    /// than resize it. Dragging a side or corner still changes the width, and
    /// the words decide the rest. That is the intended reading of "the content
    /// decides" — and it is why this is opt-in rather than the default.
    ///
    /// The **horizontal** half is an ordinary resize and converges: the preview
    /// reflows the card live as the handle moves, nothing reaches the model
    /// until the release, and the release writes the width once.
    pub fn height_for_width(self) -> Self {
        self.size_policy(SizePolicy::HeightForWidth)
    }

    /// [`height_for_width`](Self::height_for_width), stated in full. See
    /// [`SizePolicy`].
    pub fn size_policy(self, policy: SizePolicy) -> Self {
        self.model.set_size_policy(self.item, policy);
        self
    }

    /// Double-click, `Enter` on the focused card, or the AT **Edit** action.
    ///
    /// Runs **before** the mode flips, and **any write it makes to the mode is
    /// the decision**: the card leaves the mode alone and does nothing further.
    /// Write [`CardMode::Editing`] to drive an edit flow of your own, or
    /// anything else to refuse this activation. A hook that writes nothing lets
    /// the card flip to `Editing` as usual.
    ///
    /// The comparison is against the mode the hook *found*, not against a fixed
    /// value, because a double-click arrives with the card already
    /// [`Selected`](CardMode::Selected) — the press selected it — and a rule
    /// phrased against a fixed value could not tell a refusal from the state
    /// the gesture had already produced.
    pub fn on_activate(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.on_activate = Some(Rc::new(f));
        self
    }

    // ------------------------------------------------------------------
    // Internals
    // ------------------------------------------------------------------

    /// The handlers' view of this card. Cheap: five handles and two `Option`s.
    fn handle(&self) -> CardHandle {
        CardHandle {
            model: self.model.clone(),
            item: self.item,
            mode: self.mode.clone(),
            selection: self.selection.clone(),
            body: self.body,
            root: self.root,
            on_activate: self.on_activate.clone(),
        }
    }

    /// The header strip: the app's header, a `Spacer`, and the trailing slot in
    /// its own dead zone — with the drag that moves the card installed on the
    /// row itself.
    ///
    /// `None` when the card has neither a header nor a trailing control, which
    /// is a card you move by its selection frame.
    fn build_header(&mut self, ctx: &mut BuildContext) -> Option<WidgetId> {
        let header = self
            .header
            .or_else(|| self.header_pending.take().map(|w| ctx.add_boxed(w)));
        let trailing = self
            .trailing
            .or_else(|| self.trailing_pending.take().map(|w| ctx.add_boxed(w)));
        self.header = header;
        self.trailing = trailing;
        if header.is_none() && trailing.is_none() {
            return None;
        }

        let mut row = teksilo_widgets::primitives::HStack::new().spacing(4.0);
        if let Some(h) = header {
            row = row.child(h);
        }
        row = row.child(teksilo_widgets::primitives::Spacer::new());
        if let Some(t) = trailing {
            // The `Accordion` precedent: the header drags everywhere except
            // over its own controls. A `DeadZone` is the *innermost* enrolment
            // boundary on a press that lands inside it, so the row's `on_drag`
            // above is never enrolled — even for a click that jitters by a few
            // pixels, which a recognizer-timing race would lose.
            row = row.child(teksilo_widgets::primitives::DeadZone::new().child(t));
        }

        if !self.movable {
            return Some(ctx.add(row.cursor(teksilo_core::widget::CursorIcon::Default)));
        }

        let model = self.model.clone();
        let item = self.item;
        let drag = self.drag.clone();
        let preview = self.preview.clone();
        Some(
            ctx.add(row.cursor(teksilo_core::widget::CursorIcon::Move).on_drag(
                move |phase, ctx| match phase {
                    DragPhase::Started { position, .. } => {
                        // Selection already happened, on the press, in the card
                        // root's preview handler — one mechanism, not two, and
                        // nothing here needs the card handle.
                        let _ = &ctx;
                        let start = model.scene_rect(item).unwrap_or(Rect::ZERO);
                        drag.set(Some(DragState {
                            start,
                            anchor: position,
                            applied: Vec2::ZERO,
                            raw: Vec2::ZERO,
                        }));
                    }
                    DragPhase::Moved { position, .. } => {
                        let Some(mut state) = drag.get() else {
                            return;
                        };
                        // **Anchored, not accumulated** — and the preview is
                        // added back, which is the whole subtlety of moving a
                        // widget from inside itself.
                        //
                        // `position` is localised against this node's *current*
                        // bounds and transform, read live at dispatch. The
                        // preview below moves the card, so the next sample is
                        // measured against a node that has already travelled:
                        // a pointer keeping pace with the card it is dragging
                        // reports a **zero** local delta, which is why the
                        // recogniser's own `delta` cannot be summed here (it
                        // reads 40, 0, 40 for three even 40-unit moves). Adding
                        // the preview back recovers the true displacement —
                        // `D = (position − anchor) + previewed` — and it
                        // recovers the part of the motion that crossed the drag
                        // slop as well, which an accumulated delta loses
                        // permanently and visibly: the card would trail the
                        // cursor by the slop for the rest of the gesture.
                        //
                        // The units are **scene** units on both terms, at every
                        // zoom, because a card's node-local space is
                        // axis-aligned scene space translated to its origin —
                        // the view's camera is a *content* transform one level
                        // up, which the localisation has already undone. So no
                        // view transform is needed here, and a card dragged at
                        // 4× follows the pointer exactly as one at 1× does.
                        state.raw = Vec2::new(
                            position.x - state.anchor.x + state.applied.x,
                            position.y - state.anchor.y + state.applied.y,
                        );
                        // The document's standing rule, through the public door
                        // the built-in drag uses — so an app's snap-to-grid
                        // cannot mean one thing for a lightweight item and
                        // another for a card. Asked with the **raw** total each
                        // sample, never with the remainder of its own previous
                        // answer, so a grid rule does not accumulate rounding.
                        state.applied = model.constrain_move(
                            &[item],
                            TransformFrame::new(state.start, 0.0, 1),
                            state.raw,
                            TransformSource::Pointer,
                        );
                        drag.set(Some(state));
                        // Preview only. Nothing is written until the release,
                        // which is what makes a cancelled drag have nothing to
                        // roll back — the same discipline the selection
                        // transform controller states in its module header.
                        preview.set(Transform2D::translate(state.applied.x, state.applied.y));
                    }
                    DragPhase::Ended { position, .. } => {
                        let Some(mut state) = drag.replace(None) else {
                            return;
                        };
                        // The release resolves like any other sample, and it
                        // has to: the recogniser reports the move that *crossed
                        // the slop* as `Started`, at the press position and with
                        // no delta, so a flick that produces one move and a
                        // release yields `Started → Ended` and never a `Moved`
                        // at all. Committing `state.applied` as it stood would
                        // move such a card by nothing. Resolving here also makes
                        // the committed position exact rather than one sample
                        // behind the pointer.
                        state.raw = Vec2::new(
                            position.x - state.anchor.x + state.applied.x,
                            position.y - state.anchor.y + state.applied.y,
                        );
                        state.applied = model.constrain_move(
                            &[item],
                            TransformFrame::new(state.start, 0.0, 1),
                            state.raw,
                            TransformSource::Pointer,
                        );
                        preview.set(Transform2D::identity());
                        let delta = TransformDelta::new(
                            Point::new(state.start.x, state.start.y),
                            0.0,
                            Vec2::new(1.0, 1.0),
                            0.0,
                            state.applied,
                        );
                        // One write, one transaction, one reversible step.
                        model.apply_transform_delta(&[item], &delta);
                    }
                    _ => {
                        // Cancelled. Drop the preview and the state; the model
                        // was never touched.
                        drag.set(None);
                        preview.set(Transform2D::identity());
                    }
                },
            )),
        )
    }
}

impl Widget for SceneCard {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        self.root = Some(self_id);

        // ---- content ------------------------------------------------
        let header_row = self.build_header(ctx);
        let body = self
            .body
            .or_else(|| self.body_pending.take().map(|w| ctx.add_boxed(w)));
        self.body = body;

        let mut stack = teksilo_widgets::primitives::VStack::new().spacing(0.0);
        if let Some(h) = header_row {
            stack = stack.child(h);
        }
        if let Some(b) = body {
            // `respect_intrinsic` is the whole of the size story. Without it an
            // `Expand` reports a basis of zero, which is right inside a stack
            // with slack to share and wrong when the question is "how tall are
            // you at this width" — the question `SizePolicy::HeightForWidth`
            // asks, with an unbounded height. With it the body reports its
            // content height when nothing is imposed and fills the box when
            // something is, which is the same widget serving both policies.
            stack = stack.child(
                teksilo_widgets::primitives::Expand::vertical()
                    .respect_intrinsic()
                    .child(b),
            );
        }
        let content = ctx.add(stack);

        let surface = match &self.surface {
            Some(f) => ctx.add_boxed(f(content)),
            None => ctx.add(
                teksilo_widgets::Card::new()
                    .variant(CardVariant::Elevated)
                    .content(content)
                    // The card itself is the named `Role::Group`; the surface
                    // is chrome. Left alone, `Card` publishes a second,
                    // nameless `Role::Group` **inside** the named one, which a
                    // screen reader reads as a container within a container.
                    // `GenericContainer` with no properties is the role the
                    // walker prunes, promoting its children in order.
                    .access_role(accesskit::Role::GenericContainer),
            ),
        };

        // ---- selection ring -----------------------------------------
        // A sibling of the surface rather than part of it: the ring must sit
        // *over* the content (a body that paints to its own edge would cover a
        // border underneath it) and must not contribute to the card's measured
        // height, which a `ZStack` child of zero intrinsic size does not.
        let ring = ctx.add(
            teksilo_widgets::primitives::RectWidget::new()
                // One role, three widths. The colour is a role so it follows
                // the theme (and desaturates with the window); the *mode* is
                // expressed as thickness, which is what lets both be a single
                // `RepaintOnly` binding instead of a rebuild.
                .border_color(teksilo_tokens::BorderRole::Focused)
                .border_width(self.mode.map(|m| match m {
                    CardMode::Idle => 0.0,
                    CardMode::Selected => 1.5,
                    CardMode::Editing => 2.5,
                }))
                .corner_radius(teksilo_tokens::CornerRadius::uniform(8.0))
                // Paint-only: the ring is chrome, and a pointer that lands on
                // it has landed on the card.
                .event_pass_through(true),
        );
        // **And deliberately not `access_hidden`.** A bare `RectWidget` emits a
        // `Role::Unknown` node carrying nothing, which is exactly the shape
        // `is_presentational_container` collapses out of the tree — so the ring
        // already costs no accessibility node at all. Marking it hidden would
        // *add* one: the hidden flag is a property, a node that carries a
        // property is no longer content-free, and the walker would start
        // publishing ten hidden boxes on a page of ten cards where it published
        // none. Measured, ten cards: 10 `Unknown` nodes with the ring, 10
        // without it, 30 with the ring marked hidden. Pinned by
        // `a_card_publishes_no_more_nodes_than_the_hand_rolled_panel_it_replaces`.

        // ---- the card's own node ------------------------------------
        // The drag preview. `RepaintOnly` on purpose: a move is a visual
        // translation until it commits, so a ten-card page costs a repaint per
        // sample rather than a relayout per sample.
        ctx.set_transform(self_id, self.preview.clone());
        self.preview
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
        // The chrome follows the mode without a rebuild: both ring props are
        // derived from this one signal.
        self.mode
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
        // …and the accessibility state does too, which is a different level.
        self.mode.bind_to(
            self_id,
            ctx.binding_registry(),
            BindingLevel::AccessibilityOnly,
        );

        // Focus leaving the subtree is the commit. Strict-descendants-only, so
        // the card's own focus does not flip it — which is what makes `Esc`
        // (which focuses the card root) not read as a commit twice.
        let mode_for_focus = self.mode.clone();
        ctx.effect(&self.focus_within, move |within| {
            if !*within && mode_for_focus.get() == CardMode::Editing {
                mode_for_focus.set(CardMode::Selected);
            }
        });

        let mut handlers = HandlerSet::new()
            // The line that keeps the canvas out of the card. See the module
            // docs' three-row table.
            .gesture_dead_zone(true)
            .focusable(true)
            .focus_within(self.focus_within.clone());

        {
            // **Selection notices a press; it does not claim one.**
            //
            // `on_tap` on the card root would not fire at all for the case that
            // matters — a click into an embedded editor. The body claims that
            // press (anything focusable does), so the tap belongs to the body
            // and no ancestor recogniser sees it, and "click a note's text and
            // the note becomes the selected object" would not work.
            //
            // `on_pointer_event` runs on strict ancestors during the **preview**
            // pass, before the target's own handlers, and on the target itself
            // during the bubble — so this one closure covers a press on the
            // card's background, its header and anything inside it. Returning
            // `Ignored` is the whole discipline: the preview pass is where a
            // `Handled` steals a descendant's press, which is the defect the
            // scene's own pickers were built to stop doing. This notices and
            // gets out of the way.
            //
            // On the **press**, not the release, which is the desktop
            // convention for selecting an object (and what makes grabbing a
            // card's header select it before the drag starts). The framework's
            // "a press is not an activation" rule is about *activation* —
            // committing a button, following a link — and a card's activation
            // is the double-click below.
            let this = self.handle();
            handlers = handlers.on_pointer_event(move |event, ctx| {
                if matches!(event, WidgetEvent::PointerDown { .. }) {
                    this.select(ctx);
                }
                EventResponse::Ignored
            });
        }
        {
            let this = self.handle();
            handlers = handlers.on_double_tap(move |_ev, ctx| {
                this.activate(ctx);
            });
        }
        {
            let this = self.handle();
            handlers = handlers.on_key(move |event, ctx| {
                let WidgetEvent::KeyDown { key, modifiers, .. } = event else {
                    return EventResponse::Ignored;
                };
                if *modifiers != Modifiers::NONE {
                    return EventResponse::Ignored;
                }
                match key {
                    Key::Enter => {
                        this.activate(ctx);
                        EventResponse::Handled
                    }
                    Key::Escape => {
                        this.deactivate(ctx);
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }
            });
        }
        {
            // `Esc` while the body has the focus. The preview pass reaches
            // strict ancestors only, so this never double-fires with the
            // `on_key` above — that one runs when the card itself is focused,
            // this one when a descendant is.
            let this = self.handle();
            handlers = handlers.on_key_preview(move |event, ctx| {
                let WidgetEvent::KeyDown { key, modifiers, .. } = event else {
                    return EventResponse::Ignored;
                };
                if *key == Key::Escape
                    && *modifiers == Modifiers::NONE
                    && this.mode.get() == CardMode::Editing
                {
                    this.deactivate(ctx);
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            });
        }
        {
            let this = self.handle();
            handlers = handlers.on_access_action(move |action, ctx| {
                use accesskit::Action;
                match action {
                    Action::Focus => {
                        this.select(ctx);
                        EventResponse::Handled
                    }
                    Action::CustomAction => {
                        this.activate(ctx);
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }
            });
        }
        ctx.apply_self_handlers(handlers);

        // One child, not two: a `ZStack` already knows how to lay a ring over a
        // surface, and a hand-rolled `place_children` here would have to
        // reproduce the framework's absolute-origin convention — which is
        // exactly the kind of duplicate that drifts.
        let mut layers = teksilo_widgets::primitives::ZStack::new()
            // A `ZStack` sizes each child to its own measurement and
            // centres it; the surface has to *fill* the box the model gave
            // the card, or a short note paints a floating panel in the
            // middle of its own rectangle.
            .child(
                teksilo_widgets::primitives::Expand::new()
                    .respect_intrinsic()
                    .child(surface),
            )
            .child(ring);
        // The focus gate, when there is a body to focus into. Zero size, so a
        // `ZStack` layer costs nothing; content-free, so the accessibility
        // walker collapses it out of the tree the same way it collapses the
        // ring; no children, so its rebuild is one node.
        if let Some(b) = body {
            layers = layers.child(ctx.add(EditFocusGate {
                editing: self.mode.map(|m| *m == CardMode::Editing),
                focus_within: self.focus_within.clone(),
                body: b,
                was_editing: self.mode.get() == CardMode::Editing,
            }));
        }
        let stack = ctx.add(layers);
        self.child_ids = vec![stack];
        self.child_ids.clone()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        // The surface decides; the ring is chrome and contributes nothing.
        // Asking the ring too would make an unbounded proposal answer `0`, and
        // `HeightForWidth` asks exactly that.
        let size = self
            .child_ids
            .first()
            .and_then(|id| ctx.child_size(*id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0));
        size.into()
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_ids.clone()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(accesskit::Role::Group);
        let name = self.label.get();
        if !name.is_empty() {
            builder.set_name(name);
        }
        builder.set_selected(self.mode.get().is_selected());
        builder.add_action(accesskit::Action::Focus);
        // The non-pointer twin of the double-click. One custom action, index 0,
        // which is what `Action::CustomAction` routes to — and the action
        // itself is advertised by
        // [`AccessNodeBuilder::set_custom_actions`](teksilo_core::accessibility::AccessNodeBuilder::set_custom_actions),
        // which owns that half so no caller can publish a list an adapter will
        // never report. See its docs for why the list alone is decoration.
        builder.set_custom_actions(vec![accesskit::CustomAction {
            id: 0,
            description: self.activate_label.get(),
        }]);
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}
