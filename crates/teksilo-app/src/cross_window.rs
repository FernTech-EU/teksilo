// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The per-window step of the sweep that follows every dispatch: what a window
//! that may not have seen the event is asked for, so a `Signal` another window
//! wrote still reaches its screen. The loop over the windows, and the reasons,
//! are [`WindowManager::request_redraw_needing_render`].
//!
//! Kept apart from the window manager so it runs on a bare [`WidgetTree`] and
//! [`RedrawGate`]: a test cannot open a real window, and these two are all the
//! step reads.
//!
//! [`WindowManager::request_redraw_needing_render`]: crate::window_manager::WindowManager::request_redraw_needing_render

use std::time::Instant;

use teksilo_canvas::SizeProposal;
use teksilo_core::WidgetTree;

use crate::redraw_gate::RedrawGate;

/// What the sweep asked of one window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Swept {
    /// Nothing: its reactive state changed nothing it shows, or a redraw is
    /// already on its way and its frame will take the change in.
    Nothing,
    /// A non-visual tick: the window draws nothing, and its state changed.
    Tick,
    /// A redraw.
    Redraw,
}

/// Sweep one window at `now`. `size` is the window's current size, asked only
/// when the window is laid out. `request_redraw` asks winit for its redraw, and
/// runs only through `redraw`, the window's [`RedrawGate`].
///
/// - A window with reactive work is [reconciled](WidgetTree::reconcile), which
///   marks the bound widgets without laying anything out. Cheap, so it is done
///   even when a redraw is on its way: it is how the sweep learns what the next
///   step needs.
/// - A window that then [needs a structural layout](WidgetTree::needs_structural_layout)
///   (a focus request waiting for a dormant target, a rebuild, a `visible_when`
///   gate to apply) is laid out now, whatever its redraw state. The events
///   winit has already queued may come before its frame, and they must reach
///   the widgets and the focus the handler asked for: the text typed after a
///   shortcut that shows and focuses a field goes into that field, and a
///   second Enter on a button that hid itself does not reach it. These follow discrete actions: the writes a
///   drag or a selection makes on every move only repaint or relayout, and
///   wait for the frame.
/// - A window that draws nothing (hidden, or its redraw withheld) is never
///   asked for a redraw: its paint marks wait for the frame that shows it
///   again, and a redraw request after each of its ticks would re-arm the next
///   one forever. When its state changed here, it is asked for a tick instead,
///   which lays it out and delivers its accessibility tree at the gate's tick
///   rate. That request follows a write or a structural layout, which a tick's
///   own layout consumes, so a tick asks for no other.
/// - A window awaiting a redraw needs nothing more: its frame lays it out.
///   A relayout mark waits for that frame, so between two events of one batch
///   hit-testing uses the geometry on screen.
/// - Any other window is asked for a redraw when it
///   [needs a render](WidgetTree::needs_render). That check is made with or
///   without reactive work: a handler can dirty a window directly (a node
///   marked by an event) with no binding involved.
///
/// The structural layout runs with no window operations
/// ([`WidgetTree::layout`]), as the sweep's layout always did: a handler it
/// runs (a rebuild's `build`, a focus handler) cannot open a window or end an
/// input method's composition from there.
pub(crate) fn sweep_window(
    tree: &mut WidgetTree,
    redraw: &RedrawGate,
    now: Instant,
    size: impl FnOnce() -> SizeProposal,
    request_redraw: impl FnOnce(),
) -> Swept {
    let reconciled = tree.needs_reconcile();
    if reconciled {
        tree.reconcile();
    }
    let laid_out = tree.needs_structural_layout();
    if laid_out {
        tree.layout(size());
    }
    if redraw.draws_nothing(now) {
        if reconciled || laid_out {
            redraw.request_tick();
            return Swept::Tick;
        }
        return Swept::Nothing;
    }
    if redraw.awaits_redraw() {
        return Swept::Nothing;
    }
    if tree.needs_render() {
        redraw.request_with(now, request_redraw);
        return Swept::Redraw;
    }
    Swept::Nothing
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use teksilo_canvas::{Canvas, Rect, Size, SizeProposal};
    use teksilo_core::binding::BindingLevel;
    use teksilo_core::build_context::BuildContext;
    use teksilo_core::event::{EventResponse, Key, Modifiers, WidgetEvent};
    use teksilo_core::signal::Signal;
    use teksilo_core::widget::{
        LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement,
    };
    use teksilo_core::widget_builder::WidgetBuilder;
    use teksilo_core::window::NoopWindowOps;
    use teksilo_core::{CursorIcon, WidgetId, WidgetTree};
    use teksilo_platform::safe_area::SafeAreaSides;
    use teksilo_platform::text_input_hook::{StoreFocus, TextStoreMode};
    use teksilo_tokens::Color;

    use super::*;
    use crate::input_loop::{ImeHost, InputChrome, dispatch_input};
    use crate::input_routing::tests::{backend, click, cursor};
    use crate::redraw_gate::WITHHELD_AFTER;
    use winit::event::{ElementState, WindowEvent};

    /// A window's input seam whose redraw requests go through its
    /// [`RedrawGate`], as `WindowChrome` does, counting what reaches winit.
    struct GateChrome<'a> {
        gate: &'a RedrawGate,
        asked_winit: u32,
    }

    impl InputChrome for GateChrome<'_> {
        fn set_cursor(&mut self, _icon: CursorIcon) {}
        fn request_redraw(&mut self, _reason: &'static str) {
            let asked = &mut self.asked_winit;
            self.gate.request_with(Instant::now(), || *asked += 1);
        }
        fn safe_area(&self) -> SafeAreaSides {
            SafeAreaSides::default()
        }
        fn occluded_band(&self, _screen: (i32, i32, i32, i32)) -> Option<Rect> {
            None
        }
        fn set_soft_keyboard_visible(&mut self, _visible: bool) {}
        fn set_ime(&mut self, _purpose: Option<teksilo_core::ImePurpose>, _allowed: Option<bool>) {}
        fn ime_host(&self) -> ImeHost {
            ImeHost::Other
        }
        fn text_store_mode(&self) -> TextStoreMode {
            TextStoreMode::Off
        }
        fn set_text_store_focus(&mut self, _focus: StoreFocus) {}
        fn terminate_text_store_composition(&mut self) -> bool {
            false
        }
    }

    /// A leaf that fills what it is given and paints `color` over it, bound
    /// to it at `level`: `RepaintOnly` as a drop indicator or a selection band
    /// is, `Relayout` as a caret pulse a margin lane lays out by is.
    #[derive(Debug)]
    struct Swatch {
        color: Signal<Color>,
        level: BindingLevel,
    }

    impl Swatch {
        fn repainted(color: &Signal<Color>) -> Self {
            Self {
                color: color.clone(),
                level: BindingLevel::RepaintOnly,
            }
        }
    }

    impl Widget for Swatch {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            self.color
                .bind_to(ctx.self_id(), ctx.binding_registry(), self.level);
            Vec::new()
        }
        fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            Size::new(
                proposal.width.unwrap_or(100.0),
                proposal.height.unwrap_or(100.0),
            )
            .into()
        }
        fn paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
            canvas.fill_rect(bounds, self.color.get());
        }
    }

    /// A tree whose one widget writes `color` on every pointer move, the
    /// `n`th move's color being [`shade`]`(n)`, and paints it, bound at
    /// `level`, if `paints`.
    fn writing_tree(color: &Signal<Color>, paints: bool, level: BindingLevel) -> WidgetTree {
        let write = color.clone();
        let moves = Rc::new(Cell::new(0_u32));
        let mut tree = WidgetTree::new();
        let handler = move |event: &WidgetEvent, _ctx: &mut teksilo_core::widget::EventContext| {
            if matches!(event, WidgetEvent::PointerMove { .. }) {
                moves.set(moves.get() + 1);
                write.set(shade(moves.get()));
            }
            EventResponse::Ignored
        };
        // A swatch that does not bind leaves the write to the other window.
        let painted = if paints {
            color.clone()
        } else {
            Signal::new(Color::new(0.0, 0.0, 0.0, 1.0))
        };
        tree.add(
            Swatch {
                color: painted,
                level,
            }
            .on_pointer_event(handler),
        );
        tree
    }

    fn shade(n: u32) -> Color {
        Color::new((n % 256) as f32 / 255.0, 0.5, 0.25, 1.0)
    }

    /// The colors of every background the tree's next frame paints.
    fn painted_colors(tree: &mut WidgetTree) -> Vec<[f32; 4]> {
        tree.render()
            .decorations
            .iter()
            .map(|decoration| decoration.color)
            .collect()
    }

    const SIZE: SizeProposal = SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    };

    /// Window B never sees the event that changes what it shows: a handler in
    /// window A writes a signal B binds. The sweep after A's dispatch asks B
    /// for a redraw without laying B out, and B's frame paints the new value.
    #[test]
    fn a_write_in_one_window_redraws_the_window_that_binds_it() {
        let shared = Signal::new(shade(0));
        let mut tree_a = writing_tree(&shared, false, BindingLevel::RepaintOnly);
        let mut tree_b = WidgetTree::new();
        tree_b.add(Swatch::repainted(&shared));
        for tree in [&mut tree_a, &mut tree_b] {
            tree.layout(SIZE);
            tree.render();
        }
        let (gate_a, gate_b) = (RedrawGate::default(), RedrawGate::default());
        let passes_b = tree_b.layout_passes();

        // Window A's dispatch, then the sweep `post_event` runs over both.
        let mut chrome = GateChrome {
            gate: &gate_a,
            asked_winit: 0,
        };
        dispatch_input(
            &mut chrome,
            &mut backend(),
            &mut tree_a,
            &mut NoopWindowOps,
            &cursor(20.0, 20.0),
        );
        assert_eq!(shared.get(), shade(1), "the handler in A wrote");
        let now = Instant::now();
        let mut b_asked_winit = 0;
        sweep_window(&mut tree_a, &gate_a, now, || SIZE, || {});
        let swept_b = sweep_window(&mut tree_b, &gate_b, now, || SIZE, || b_asked_winit += 1);

        assert_eq!(swept_b, Swept::Redraw, "B is asked for a redraw");
        assert_eq!(b_asked_winit, 1, "through winit");
        assert!(gate_b.awaits_redraw());
        assert_eq!(
            tree_b.layout_passes(),
            passes_b,
            "and is not laid out to find that out"
        );

        // B's frame: delivered, laid out and drawn with the new color.
        gate_b.delivered();
        tree_b.layout(SIZE);
        assert_eq!(painted_colors(&mut tree_b), vec![shade(1).to_array()]);

        // With nothing else written, the next sweep asks B for nothing.
        assert_eq!(
            sweep_window(&mut tree_b, &gate_b, Instant::now(), || SIZE, || {}),
            Swept::Nothing
        );
    }

    /// A drag or a selection writes a bound signal on every move. However many
    /// moves are dispatched, the only layouts are the frames', each frame
    /// draws the latest value, and between two frames the window is asked for
    /// one redraw.
    #[test]
    fn moves_that_write_a_repaint_binding_lay_out_only_in_frames() {
        moves_lay_out_only_in_frames(BindingLevel::RepaintOnly);
    }

    /// The same for a binding that asks for a relayout: it is not structural,
    /// so it waits for the frame too.
    #[test]
    fn moves_that_write_a_relayout_binding_lay_out_only_in_frames() {
        moves_lay_out_only_in_frames(BindingLevel::Relayout);
    }

    fn moves_lay_out_only_in_frames(level: BindingLevel) {
        const MOVES: u32 = 200;
        const MOVES_PER_FRAME: u32 = 25;
        let color = Signal::new(shade(0));
        let mut tree = writing_tree(&color, true, level);
        tree.layout(SIZE);
        tree.render();
        let gate = RedrawGate::default();
        let mut chrome = GateChrome {
            gate: &gate,
            asked_winit: 0,
        };
        let mut translation = backend();
        let passes = tree.layout_passes();
        let mut frames = 0;
        let mut redraws_asked_since_frame = 0;

        for n in 1..=MOVES {
            dispatch_input(
                &mut chrome,
                &mut translation,
                &mut tree,
                &mut NoopWindowOps,
                &cursor(10.0 + f64::from(n), 20.0),
            );
            if sweep_window(&mut tree, &gate, Instant::now(), || SIZE, || {}) == Swept::Redraw {
                redraws_asked_since_frame += 1;
            }
            assert!(
                redraws_asked_since_frame <= 1,
                "one redraw asked between two frames, at move {n}"
            );
            assert!(gate.awaits_redraw(), "a move's write is never left undrawn");
            if n % MOVES_PER_FRAME == 0 {
                gate.delivered();
                tree.layout(SIZE);
                assert_eq!(
                    painted_colors(&mut tree),
                    vec![shade(n).to_array()],
                    "frame after move {n} draws that move's value"
                );
                frames += 1;
                redraws_asked_since_frame = 0;
            }
        }

        assert_eq!(frames, MOVES / MOVES_PER_FRAME);
        assert_eq!(
            tree.layout_passes() - passes,
            u64::from(frames),
            "{MOVES} moves laid the tree out only in its {frames} frames"
        );
    }

    /// A window that draws nothing is asked for a tick, never a redraw, when
    /// its reactive state changed, and is not laid out by the sweep. A tick's
    /// own layout consumes the write, so the sweep after it asks for nothing.
    #[test]
    fn a_window_that_draws_nothing_is_ticked_for_a_write_and_only_for_one() {
        let shared = Signal::new(shade(0));
        let mut tree = WidgetTree::new();
        tree.add(Swatch::repainted(&shared));
        tree.layout(SIZE);
        tree.render();
        let passes = tree.layout_passes();

        let hidden = RedrawGate::default();
        hidden.set_hidden(true);
        let now = Instant::now();
        let mut asked_winit = 0;
        assert_eq!(
            sweep_window(&mut tree, &hidden, now, || SIZE, || asked_winit += 1),
            Swept::Nothing,
            "nothing written, nothing asked"
        );

        shared.set(shade(1));
        assert_eq!(
            sweep_window(&mut tree, &hidden, now, || SIZE, || asked_winit += 1),
            Swept::Tick
        );
        assert_eq!(asked_winit, 0, "no redraw asked of a hidden window");
        assert_eq!(tree.layout_passes(), passes, "and no layout run");
        assert!(hidden.take_tick(now), "its tick is due");

        // The tick lays the tree out (it draws nothing), and the sweep after
        // it, with nothing written since, asks for no other.
        tree.layout(SIZE);
        assert!(tree.needs_render(), "the paint waits for the reveal");
        assert_eq!(
            sweep_window(&mut tree, &hidden, now, || SIZE, || asked_winit += 1),
            Swept::Nothing
        );

        // A window whose redraw is withheld draws nothing too.
        let withheld = RedrawGate::default();
        withheld.request_with(now - WITHHELD_AFTER - Duration::from_millis(1), || {});
        shared.set(shade(2));
        assert_eq!(
            sweep_window(&mut tree, &withheld, now, || SIZE, || asked_winit += 1),
            Swept::Tick
        );
        assert_eq!(asked_winit, 0);
    }

    // -- What a handler changes in a window's structure or focus reaches the
    // next event of the same batch, dispatched before any frame.

    /// Keys that reached each widget, by name, in order.
    type KeyLog = Rc<RefCell<Vec<(&'static str, Key)>>>;

    /// A leaf that fills what it is given and draws nothing.
    #[derive(Debug)]
    struct Spot;

    impl Widget for Spot {
        fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            proposal.resolve(100.0, 30.0).into()
        }
    }

    /// A container that gives each child its whole box.
    #[derive(Debug)]
    struct Overlap;

    impl Widget for Overlap {
        fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
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
    }

    /// A focusable leaf that logs, under `name`, every key it receives.
    fn key_logger(name: &'static str, log: &KeyLog) -> impl Widget + 'static {
        let log = log.clone();
        Spot.focusable(true).on_key(move |event, _ctx| {
            if let WidgetEvent::KeyDown { key, .. } = event {
                log.borrow_mut().push((name, *key));
            }
            EventResponse::Handled
        })
    }

    /// A gate whose window asked for a redraw that has not come: the state of
    /// the window that just took a key or a click.
    fn awaiting_gate() -> RedrawGate {
        let gate = RedrawGate::default();
        gate.request_with(Instant::now(), || {});
        gate
    }

    /// A find command shows a parked search field and asks for focus on it.
    /// The text typed next, before any frame, goes into that field, not into
    /// the editor, although the window's redraw is already on its way.
    #[test]
    fn a_field_shown_and_focused_by_a_handler_takes_the_next_key() {
        let log: KeyLog = Rc::default();
        let shown = Signal::new(false);
        let field_slot: Rc<Cell<Option<WidgetId>>> = Rc::default();
        let mut tree = WidgetTree::new();
        let root = tree.add(Overlap);
        let editor = {
            let (log, shown, field_slot) = (log.clone(), shown.clone(), field_slot.clone());
            tree.add_child(
                root,
                Spot.focusable(true).on_key(move |event, ctx| {
                    let WidgetEvent::KeyDown { key, .. } = event else {
                        return EventResponse::Ignored;
                    };
                    if *key == Key::F
                        && let Some(field) = field_slot.get()
                    {
                        shown.set(true);
                        ctx.request_focus(field);
                    } else {
                        log.borrow_mut().push(("editor", *key));
                    }
                    EventResponse::Handled
                }),
            )
        };
        let field = tree.add_child(root, key_logger("field", &log));
        field_slot.set(Some(field));
        tree.visible_when(field, shown.clone());
        tree.layout(SIZE);
        tree.render();
        tree.focus(editor);
        assert!(!tree.is_active(field), "precondition: the field is parked");

        let gate = awaiting_gate();
        tree.press_key(Key::F, Modifiers::NONE);
        let passes = tree.layout_passes();
        sweep_window(&mut tree, &gate, Instant::now(), || SIZE, || {});
        assert_eq!(
            tree.layout_passes(),
            passes + 1,
            "the sweep lays the window out for it"
        );
        tree.press_key(Key::X, Modifiers::NONE);

        assert_eq!(tree.focused(), Some(field));
        assert_eq!(*log.borrow(), vec![("field", Key::X)]);
    }

    /// A control that hides itself when activated is gone for the second
    /// Enter of the same batch: activated once, not twice.
    #[test]
    fn a_control_that_hid_itself_takes_no_second_activation() {
        let visible = Signal::new(true);
        let activations = Rc::new(Cell::new(0_u32));
        let mut tree = WidgetTree::new();
        let root = tree.add(Overlap);
        let button = {
            let (visible, activations) = (visible.clone(), activations.clone());
            tree.add_child(
                root,
                Spot.focusable(true).on_key(move |event, _ctx| {
                    if let WidgetEvent::KeyDown {
                        key: Key::Enter, ..
                    } = event
                    {
                        activations.set(activations.get() + 1);
                        visible.set(false);
                        return EventResponse::Handled;
                    }
                    EventResponse::Ignored
                }),
            )
        };
        tree.visible_when(button, visible.clone());
        tree.layout(SIZE);
        tree.render();
        tree.focus(button);

        let gate = awaiting_gate();
        tree.press_key(Key::Enter, Modifiers::NONE);
        sweep_window(&mut tree, &gate, Instant::now(), || SIZE, || {});
        tree.press_key(Key::Enter, Modifiers::NONE);

        assert_eq!(activations.get(), 1);
        assert!(!tree.is_active(button), "it is parked");
        assert_ne!(tree.focused(), Some(button), "and holds no focus");
    }

    /// A container rebuilt from a key: the next key reaches the rebuilt
    /// child, where focus was restored, not the child the rebuild replaced.
    #[test]
    fn a_rebuild_asked_by_a_handler_is_done_before_the_next_key() {
        #[derive(Debug)]
        struct Generations {
            generation: Signal<u32>,
            log: Rc<RefCell<Vec<(u32, Key)>>>,
        }

        impl Widget for Generations {
            fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
                self.generation.bind_to(
                    ctx.self_id(),
                    ctx.binding_registry(),
                    BindingLevel::Rebuild,
                );
                let built = self.generation.get();
                let (generation, log) = (self.generation.clone(), self.log.clone());
                let child = ctx.add(Spot.focusable(true).on_key(move |event, _ctx| {
                    let WidgetEvent::KeyDown { key, .. } = event else {
                        return EventResponse::Ignored;
                    };
                    if *key == Key::R {
                        generation.set(built + 1);
                    } else {
                        log.borrow_mut().push((built, *key));
                    }
                    EventResponse::Handled
                }));
                vec![child]
            }
            fn layout_response(
                &self,
                proposal: SizeProposal,
                _ctx: &LayoutContext,
            ) -> LayoutResponse {
                proposal.resolve(100.0, 30.0).into()
            }
        }

        let log = Rc::new(RefCell::new(Vec::new()));
        let mut tree = WidgetTree::new();
        let root = tree.add(Generations {
            generation: Signal::new(0),
            log: log.clone(),
        });
        tree.layout(SIZE);
        tree.render();
        let first = tree.children(root)[0];
        tree.focus(first);

        let gate = awaiting_gate();
        tree.press_key(Key::R, Modifiers::NONE);
        sweep_window(&mut tree, &gate, Instant::now(), || SIZE, || {});
        tree.press_key(Key::A, Modifiers::NONE);

        assert_ne!(tree.children(root), vec![first], "rebuilt");
        assert_eq!(*log.borrow(), vec![(1, Key::A)]);
    }

    /// A rebuild written while a press holds its capture waits for the
    /// release, so the gesture keeps its widget. Once the release has ended
    /// the capture, the sweep carries the rebuild out: the next click, before
    /// any frame, reaches the rebuilt child, not the one it replaced.
    #[test]
    fn a_rebuild_held_back_for_a_press_is_done_before_the_next_click() {
        #[derive(Debug)]
        struct Pressable {
            generation: Signal<u32>,
            taps: Rc<RefCell<Vec<u32>>>,
        }

        impl Widget for Pressable {
            fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
                self.generation.bind_to(
                    ctx.self_id(),
                    ctx.binding_registry(),
                    BindingLevel::Rebuild,
                );
                let built = self.generation.get();
                let taps = self.taps.clone();
                vec![ctx.add(Spot.on_tap(move |_event, _ctx| taps.borrow_mut().push(built)))]
            }
            fn layout_response(
                &self,
                proposal: SizeProposal,
                _ctx: &LayoutContext,
            ) -> LayoutResponse {
                proposal.resolve(100.0, 30.0).into()
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
        }

        let generation = Signal::new(0_u32);
        let taps = Rc::new(RefCell::new(Vec::new()));
        let mut tree = WidgetTree::new();
        let root = tree.add(Pressable {
            generation: generation.clone(),
            taps: taps.clone(),
        });
        tree.layout(SIZE);
        tree.render();
        let first = tree.children(root);
        let gate = awaiting_gate();
        let mut chrome = GateChrome {
            gate: &gate,
            asked_winit: 0,
        };
        let mut translation = backend();
        let mut input = |tree: &mut WidgetTree, event: WindowEvent| {
            dispatch_input(
                &mut chrome,
                &mut translation,
                tree,
                &mut NoopWindowOps,
                &event,
            );
            sweep_window(tree, &gate, Instant::now(), || SIZE, || {});
        };

        input(&mut tree, cursor(10.0, 10.0));
        input(&mut tree, click(ElementState::Pressed));
        generation.set(1);
        sweep_window(&mut tree, &gate, Instant::now(), || SIZE, || {});
        assert_eq!(
            tree.children(root),
            first,
            "held back while the press holds its widget"
        );
        input(&mut tree, click(ElementState::Released));
        assert_ne!(tree.children(root), first, "rebuilt once released");
        input(&mut tree, click(ElementState::Pressed));
        input(&mut tree, click(ElementState::Released));

        assert_eq!(
            *taps.borrow(),
            vec![0, 1],
            "the next click reaches the rebuilt child"
        );
    }

    /// A button whose Enter disables it: focus moves on before the next key
    /// of the batch, which reaches the next control. A disabled control
    /// refuses a key, so focus left there until the frame loses what is
    /// typed.
    #[test]
    fn a_control_that_disabled_itself_hands_focus_on_before_the_next_key() {
        let enabled = Signal::new(true);
        let log: KeyLog = Rc::default();
        let mut tree = WidgetTree::new();
        let root = tree.add(Overlap);
        let save = {
            let enabled = enabled.clone();
            tree.add_child(
                root,
                Spot.focusable(true).on_key(move |event, _ctx| {
                    if let WidgetEvent::KeyDown {
                        key: Key::Enter, ..
                    } = event
                    {
                        enabled.set(false);
                        return EventResponse::Handled;
                    }
                    EventResponse::Ignored
                }),
            )
        };
        tree.add_child(root, key_logger("next", &log));
        tree.enabled_when(save, enabled.clone());
        tree.layout(SIZE);
        tree.render();
        tree.focus(save);

        let gate = awaiting_gate();
        tree.press_key(Key::Enter, Modifiers::NONE);
        sweep_window(&mut tree, &gate, Instant::now(), || SIZE, || {});
        tree.press_key(Key::A, Modifiers::NONE);

        assert_eq!(*log.borrow(), vec![("next", Key::A)]);
    }

    /// The only enabled control disables itself, so focus stays on it; then
    /// another control is enabled, by a write the window did not see. A key
    /// in the same batch reaches the newly enabled control: the sweep after
    /// the write hands focus on before it.
    #[test]
    fn a_control_enabled_while_focus_is_stuck_takes_the_next_key() {
        let save_enabled = Signal::new(true);
        let next_enabled = Signal::new(false);
        let log: KeyLog = Rc::default();
        let mut tree = WidgetTree::new();
        let root = tree.add(Overlap);
        let save = {
            let enabled = save_enabled.clone();
            tree.add_child(
                root,
                Spot.focusable(true).on_key(move |event, _ctx| {
                    if let WidgetEvent::KeyDown {
                        key: Key::Enter, ..
                    } = event
                    {
                        enabled.set(false);
                        return EventResponse::Handled;
                    }
                    EventResponse::Ignored
                }),
            )
        };
        let next = tree.add_child(root, key_logger("next", &log));
        tree.enabled_when(save, save_enabled.clone());
        tree.enabled_when(next, next_enabled.clone());
        tree.layout(SIZE);
        tree.render();
        tree.focus(save);

        let gate = awaiting_gate();
        tree.press_key(Key::Enter, Modifiers::NONE);
        sweep_window(&mut tree, &gate, Instant::now(), || SIZE, || {});
        assert_eq!(tree.focused(), Some(save), "precondition: nowhere to go");
        next_enabled.set(true);
        sweep_window(&mut tree, &gate, Instant::now(), || SIZE, || {});
        tree.press_key(Key::A, Modifiers::NONE);

        assert_eq!(*log.borrow(), vec![("next", Key::A)]);
    }

    /// An external drag over a target with a tick handler, moving inside one
    /// row: the feedback stays the same, so no move asks for a frame, and
    /// there is no preview in the tree to lay out. The tick, where spring-
    /// loaded folders and edge scrolling live, still runs in every frame the
    /// loop draws for the window, with the pointer moving or held still, and
    /// the frames stop once the drag has ended.
    #[test]
    fn a_drag_over_a_ticking_target_is_ticked_every_frame() {
        use teksilo_canvas::Point;
        use teksilo_core::DropFeedback;
        use teksilo_core::drag_payload::ExternalDropData;

        let ticks = Rc::new(Cell::new(0_u32));
        let mut tree = WidgetTree::new();
        let root = tree.add(Overlap);
        let counted = ticks.clone();
        tree.add_child(
            root,
            Spot.on_drag_hover(|_payload, _at, _ctx| DropFeedback::HighlightRect {
                rect: Rect::new(0.0, 0.0, 100.0, 30.0),
                color: Color::WHITE,
            })
            .on_drag_tick(move |_at, _ctx| counted.set(counted.get() + 1)),
        );
        tree.layout(SIZE);
        tree.render();
        let gate = RedrawGate::default();
        let mut ops = NoopWindowOps;
        // What the loop does for the window after an event: the sweep, then
        // a frame when a redraw was asked or a deadline of the tree's is
        // waited for.
        let frame = |tree: &mut WidgetTree| {
            sweep_window(tree, &gate, Instant::now(), || SIZE, || {});
            let due = gate.awaits_redraw() || tree.next_timer_deadline().is_some();
            if due {
                gate.delivered();
                tree.layout(SIZE);
                tree.render();
            }
            due
        };

        tree.begin_external_drag(
            Point::new(20.0, 10.0),
            ExternalDropData::default(),
            &mut ops,
        );
        frame(&mut tree);
        let after_entering = ticks.get();
        for n in 1..=10_u8 {
            tree.update_external_drag(Point::new(20.0 + f32::from(n), 10.0), &mut ops);
            assert!(frame(&mut tree), "a frame follows move {n}");
        }
        assert_eq!(ticks.get(), after_entering + 10, "one tick per frame");
        for _ in 0..5 {
            assert!(frame(&mut tree), "and goes on with the pointer still");
        }
        assert_eq!(ticks.get(), after_entering + 15);

        tree.cancel_external_drag(&mut ops);
        let mut tail = 0;
        while frame(&mut tree) {
            tail += 1;
            assert!(tail <= 2, "the frames stop after the drag");
        }
        assert_eq!(ticks.get(), after_entering + 15, "and no tick without it");
    }
}
