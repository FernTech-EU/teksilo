// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `CollapsibleBody` — a toast body that clamps to a few lines and offers to unfold.
//!
//! # Why a toast body needs a ceiling
//!
//! A title is one line by construction ([`ToastSurface`](super::surface::ToastSurface)
//! builds it `.single_line()`), but a body is whatever the app hands over — and apps hand
//! over error text. A formatted `anyhow` chain carries every context frame and every
//! absolute path in it; rendered at toast width that is easily eight or ten lines, and a
//! toast that tall stops being a notification and becomes a dialog nobody agreed to open.
//!
//! So the body clamps to [`TOAST_BODY_COLLAPSED_LINES`] and, *only when there is more to
//! see*, grows a thin disclosure row. Short bodies — the overwhelming majority — are
//! untouched and gain no chrome.
//!
//! # How "is there more to see" is answered
//!
//! By measuring, in [`Widget::layout_response`], at the width the body will actually be
//! given. That is the only place the real content width is known: it is the proposal this
//! widget receives, already net of the glyph, the close button, the padding and the gaps.
//! Deriving it any other way would mean re-deriving the chrome's arithmetic somewhere
//! else and keeping the two in step forever.
//!
//! The measured verdict lands in a `Signal`, which is what drives the disclosure row's
//! visibility — and writing a signal from layout is the thing to be careful about, since
//! it schedules another pass. Two properties make it safe:
//!
//!  * **Transition-guarded.** The signal is written only when the verdict *changes*,
//!    tracked in a `Cell`. A steady state writes nothing, so there is no loop. This is
//!    the same discipline `BuildContext::activation_signal` documents for itself.
//!  * **No feedback.** The disclosure row sits *below* the text in a `VStack`, so showing
//!    it changes the body's height but never its width — and width is the only input to
//!    the measurement. The verdict therefore cannot flip as a result of acting on it.

use std::cell::Cell;
use std::rc::Rc;

use bastyde_canvas::{Rect, Size, SizeProposal};
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_i18n::{LocalizedString, tr_widget};
use bastyde_platform::clipboard::ClipboardHandle;
use bastyde_tokens::{TextRole, TextStyleRole};

use crate::link::Link;
use crate::primitives::{HStack, TextWidget, VStack};

/// How many lines of body text a toast shows before offering to unfold.
///
/// Three is the point where a body still reads as a caption rather than a paragraph, and
/// it comfortably fits the two-sentence bodies most apps write — those never see the
/// disclosure row at all.
pub const TOAST_BODY_COLLAPSED_LINES: usize = 3;

/// Vertical gap between the body text and its disclosure row.
pub const TOAST_BODY_DISCLOSURE_GAP: f32 = 2.0;

/// Horizontal gap between the disclosure row's actions.
pub const TOAST_DISCLOSURE_ACTION_GAP: f32 = 12.0;

/// Put `text` on the system clipboard and flip `copied` so the row can say so.
///
/// A failed `set_text` leaves `copied` false rather than claiming success — the platform
/// returns an error when there is no clipboard to talk to (a headless session, a
/// compositor that denied access), and a "Copied" that did not happen is worse than no
/// feedback at all.
fn copy_to_clipboard(
    ctx: &mut bastyde_core::widget::EventContext,
    text: &str,
    copied: &Signal<bool>,
) {
    let ok = ctx
        .app_state::<ClipboardHandle>()
        .map(|cb| cb.set_text(text).is_ok())
        .unwrap_or(false);
    if ok {
        copied.set(true);
    }
}

/// What the body is currently doing. One signal rather than an `expanded` /
/// `overflowing` pair, because every visibility below is then a plain `.map()` off it —
/// no signal-combining, and no way to represent the impossible state "expanded but there
/// was never anything to expand".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BodyState {
    /// Fits within the clamp. No disclosure row.
    Fits,
    /// Clamped, with more to see.
    Collapsed,
    /// Showing everything.
    Expanded,
}

impl BodyState {
    fn as_u8(self) -> u8 {
        match self {
            Self::Fits => 0,
            Self::Collapsed => 1,
            Self::Expanded => 2,
        }
    }
}

/// A toast body: clamped text plus a disclosure row that appears only when clamping
/// actually hid something.
pub(crate) struct CollapsibleBody {
    text: LocalizedString,
    /// `BodyState::as_u8` — a plain scalar so `.map()` projections stay `Copy`-cheap.
    state: Signal<u8>,
    /// Run when the reader unfolds. The toast uses it to cancel auto-dismiss — see
    /// [`ToastRegistry::cancel_auto_dismiss`](crate::toast::registry::ToastRegistry).
    on_expand: Option<Rc<dyn Fn()>>,
    column_id: Option<WidgetId>,
    /// Last verdict written to `state`, so a steady state writes nothing. See the module
    /// docs on why this guard is what makes a layout-time signal write safe.
    last_overflowing: Cell<Option<bool>>,
}

impl std::fmt::Debug for CollapsibleBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CollapsibleBody")
            .field("text", &self.text)
            .field("state", &self.state.get())
            .finish()
    }
}

impl CollapsibleBody {
    /// `state` is supplied by the caller rather than created here, and that is load
    /// bearing: `ToastHost` builds a fresh `ToastSurface` — hence a fresh body — every
    /// time the live set changes, so a widget-owned signal would re-fold the toast the
    /// moment any *other* toast arrived. The entry owns it instead
    /// (`LiveEntry::body_state`) and it outlives every rebuild.
    pub(crate) fn new(text: LocalizedString, state: Signal<u8>) -> Self {
        Self {
            text,
            state,
            on_expand: None,
            column_id: None,
            last_overflowing: Cell::new(None),
        }
    }

    /// Run `f` when the reader unfolds the body.
    pub(crate) fn on_expand(mut self, f: impl Fn() + 'static) -> Self {
        self.on_expand = Some(Rc::new(f));
        self
    }
}

impl Widget for CollapsibleBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let state = self.state.clone();

        // Two text widgets rather than one with a reactive `max_lines`: `TextWidget`
        // takes a plain `usize` there, and swapping visibility is the pattern the rest of
        // the toolkit already uses for disclosure (see `Accordion`'s paired chevrons).
        // The cost is one extra measured layout of the same string.
        let clamped = ctx.add(
            TextWidget::new(self.text.clone())
                .style(TextStyleRole::Body)
                .color(TextRole::Secondary)
                .max_lines(TOAST_BODY_COLLAPSED_LINES),
        );
        let full = ctx.add(
            TextWidget::new(self.text.clone())
                .style(TextStyleRole::Body)
                .color(TextRole::Secondary),
        );
        ctx.visible_when(clamped, state.map(|s| *s != BodyState::Expanded.as_u8()));
        ctx.visible_when(full, state.map(|s| *s == BodyState::Expanded.as_u8()));

        // A `Link`, not a `Button`: this is a low-weight reveal inside a notification,
        // and a button's chrome would compete with any real action the toast carries.
        let expand_state = state.clone();
        let on_expand = self.on_expand.clone();
        let show_more = ctx.add(Link::new(tr_widget!(toast_show_more())).on_activate_fn(
            move |_| {
                expand_state.set(BodyState::Expanded.as_u8());
                if let Some(f) = &on_expand {
                    f();
                }
            },
        ));
        let collapse_state = state.clone();
        let show_less = ctx.add(
            Link::new(tr_widget!(toast_show_less()))
                .on_activate_fn(move |_| collapse_state.set(BodyState::Collapsed.as_u8())),
        );
        ctx.visible_when(show_more, state.map(|s| *s == BodyState::Collapsed.as_u8()));
        ctx.visible_when(show_less, state.map(|s| *s == BodyState::Expanded.as_u8()));

        // Copy. A body long enough to be clamped is, in practice, an error chain — the
        // exact text someone wants in a bug report or a search box, and the exact text
        // that is miserable to retype. Reading it and *keeping* it are the two things you
        // want from a truncated error, so the affordances sit together.
        //
        // Confirmation is a label swap rather than a timed revert or a nested toast:
        // "Copied" needs no timer to be honest, and the link stays live so a second click
        // still works (a reader who copied, scrolled away, and came back should not have
        // to guess whether it took).
        let copied = ctx.signal(false);
        let copy_text = self.text.clone();
        let copied_flag = copied.clone();
        let copy = ctx.add(Link::new(tr_widget!(toast_copy_body())).on_activate_fn(
            move |ctx: &mut bastyde_core::widget::EventContext| {
                copy_to_clipboard(ctx, &copy_text.resolve_now(), &copied_flag);
            },
        ));
        let recopy_text = self.text.clone();
        let recopy_flag = copied.clone();
        let copied_label = ctx.add(Link::new(tr_widget!(toast_body_copied())).on_activate_fn(
            move |ctx: &mut bastyde_core::widget::EventContext| {
                copy_to_clipboard(ctx, &recopy_text.resolve_now(), &recopy_flag);
            },
        ));
        ctx.visible_when(copy, copied.map(|c| !*c));
        ctx.visible_when(copied_label, copied.clone());

        let disclosure = ctx.add(
            HStack::new()
                .spacing(TOAST_DISCLOSURE_ACTION_GAP)
                .add_child(show_more)
                .add_child(show_less)
                .add_child(copy)
                .add_child(copied_label),
        );
        // The whole row rides on the clamp: a body short enough to be fully visible gains
        // no chrome at all, which is the overwhelming majority of toasts.
        ctx.visible_when(disclosure, state.map(|s| *s != BodyState::Fits.as_u8()));

        let column = ctx.add(
            VStack::new()
                .spacing(TOAST_BODY_DISCLOSURE_GAP)
                .add_child(clamped)
                .add_child(full)
                .add_child(disclosure),
        );
        self.column_id = Some(column);
        vec![column]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        // Measure the *unclamped* text at the width this body is being given. `max_lines:
        // None` on purpose — asking the clamped question would answer itself.
        if let (Some(width), Some(backend)) = (proposal.width, ctx.text_backend)
            && width > 0.0
        {
            let text = self.text.resolve_now();
            let style = TextStyleRole::Body.resolve(&ctx.theme.typography);
            // The same +0.5 epsilon `TextWidget` applies, so both land on one
            // `TypesetterBridge` cache key rather than two that disagree at the margin.
            let layout = backend
                .borrow_mut()
                .layout_paragraph(&text, &style, width + 0.5, None);
            let overflowing = layout.line_count > TOAST_BODY_COLLAPSED_LINES;

            if self.last_overflowing.get() != Some(overflowing) {
                self.last_overflowing.set(Some(overflowing));
                // Never override a reader who has already unfolded this: only the
                // shrinking direction is theirs to lose.
                let current = self.state.get();
                let next = if overflowing {
                    if current == BodyState::Expanded.as_u8() {
                        current
                    } else {
                        BodyState::Collapsed.as_u8()
                    }
                } else {
                    BodyState::Fits.as_u8()
                };
                if next != current {
                    self.state.set(next);
                }
            }
        }

        self.column_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or(Size::ZERO)
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
        self.column_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_canvas::text_backend::MockTextBackend;
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_core::window::NoopWindowOps;
    use bastyde_i18n::lit;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// `MockTextBackend` wraps on whole words at 8px/char and 16px lines, so the line
    /// count below is arithmetic rather than a guess.
    const LINE_H: f32 = 16.0;
    const WIDTH: f32 = 160.0; // 20 characters per line

    fn tree() -> WidgetTree {
        WidgetTree::new().with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())))
    }

    fn lay_out(text: &str, state: Signal<u8>) -> (WidgetTree, WidgetId) {
        let mut t = tree();
        let id = t.add(CollapsibleBody::new(lit!(text.to_string()), state));
        t.layout(SizeProposal {
            width: Some(WIDTH),
            height: None,
        });
        // A second pass: the first one's measurement is what *decides* the state, and the
        // disclosure row is laid out against the decision. The real app gets this for
        // free — the signal write schedules the next pass.
        t.layout(SizeProposal {
            width: Some(WIDTH),
            height: None,
        });
        (t, id)
    }

    /// The common case must gain nothing: no clamp, no disclosure row, no extra height.
    #[test]
    fn a_body_that_fits_gets_no_disclosure_row() {
        let state = Signal::new(BodyState::Fits.as_u8());
        let (t, id) = lay_out("short body", state.clone());

        assert_eq!(state.get(), BodyState::Fits.as_u8());
        assert!(
            (t.bounds(id).height - LINE_H).abs() < 0.5,
            "one line of text and nothing else; got {}",
            t.bounds(id).height
        );
    }

    /// A long body is clamped, and the clamp is what bounds the toast's height. Without
    /// it a formatted `anyhow` chain — every context frame and every absolute path —
    /// grows the toast without limit.
    #[test]
    fn a_long_body_is_clamped_and_offers_to_unfold() {
        let long = "aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj kkkk llll mmmm nnnn";
        let state = Signal::new(BodyState::Fits.as_u8());
        let (t, id) = lay_out(long, state.clone());

        assert_eq!(
            state.get(),
            BodyState::Collapsed.as_u8(),
            "the measurement must have found more than {TOAST_BODY_COLLAPSED_LINES} lines"
        );

        let clamped_height = t.bounds(id).height;
        let text_ceiling = TOAST_BODY_COLLAPSED_LINES as f32 * LINE_H;
        assert!(
            clamped_height > text_ceiling,
            "the disclosure row must add height; got {clamped_height}"
        );
        assert!(
            clamped_height < text_ceiling + 2.0 * LINE_H,
            "…but only a row's worth — a clamped body is not allowed to grow; got {clamped_height}"
        );
    }

    /// Unfolding shows everything. Driven through the signal rather than a synthetic
    /// click, because the signal IS the widget's contract with its host.
    #[test]
    fn unfolding_shows_the_whole_body() {
        let long = "aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj kkkk llll mmmm nnnn";
        let state = Signal::new(BodyState::Fits.as_u8());
        let (mut t, id) = lay_out(long, state.clone());
        let clamped_height = t.bounds(id).height;

        state.set(BodyState::Expanded.as_u8());
        t.layout(SizeProposal {
            width: Some(WIDTH),
            height: None,
        });

        assert!(
            t.bounds(id).height > clamped_height,
            "unfolding must reveal more than the clamp showed ({} vs {})",
            t.bounds(id).height,
            clamped_height
        );
    }

    /// The re-measure must not fold a body the reader just opened. This is the shape a
    /// resize or a locale change takes: another layout pass over an already-expanded body.
    #[test]
    fn a_relayout_does_not_refold_an_unfolded_body() {
        let long = "aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj kkkk llll mmmm nnnn";
        let state = Signal::new(BodyState::Fits.as_u8());
        let (mut t, _id) = lay_out(long, state.clone());

        state.set(BodyState::Expanded.as_u8());
        for _ in 0..3 {
            t.layout(SizeProposal {
                width: Some(WIDTH),
                height: None,
            });
        }

        assert_eq!(
            state.get(),
            BodyState::Expanded.as_u8(),
            "the layout-time probe must leave an unfolded body alone"
        );
    }

    fn ctx_with_memory_clipboard(
        tree: &mut WidgetTree,
    ) -> bastyde_platform::clipboard::ClipboardHandle {
        use bastyde_core::event_source::TreeAppContext;
        use bastyde_platform::clipboard::MemoryClipboard;
        use std::any::TypeId;
        use std::collections::HashMap;
        let handle = ClipboardHandle::new(MemoryClipboard::new());
        let mut registry: HashMap<TypeId, Box<dyn std::any::Any>> = HashMap::new();
        registry.insert(TypeId::of::<ClipboardHandle>(), Box::new(handle.clone()));
        tree.set_app_context(Rc::new(TreeAppContext::empty().with_app_state(registry)));
        handle
    }

    /// Copy puts the **whole** body on the clipboard, not the three lines on screen.
    /// A clamped error chain is precisely the text someone needs to paste somewhere, and
    /// pasting the visible truncation would be worse than useless.
    #[test]
    fn copy_puts_the_unclamped_body_on_the_clipboard() {
        let long = "aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj kkkk llll mmmm nnnn";
        let mut t = tree();
        let clipboard = ctx_with_memory_clipboard(&mut t);
        let copied = Signal::new(false);

        // Exercise the same helper the link's handler calls. Driving the Link itself
        // would test bastyde's hit-testing, not this widget's contract.
        t.run_with_event_context(&mut NoopWindowOps, |ctx| {
            copy_to_clipboard(ctx, long, &copied)
        });

        assert_eq!(clipboard.get_text().unwrap_or_default(), long);
        assert!(copied.get(), "the row must switch to its confirmed label");
    }

    /// No clipboard (headless, or a compositor that said no) must not claim success.
    #[test]
    fn a_failed_copy_does_not_claim_to_have_copied() {
        let mut t = tree();
        let copied = Signal::new(false);
        t.run_with_event_context(&mut NoopWindowOps, |ctx| {
            copy_to_clipboard(ctx, "anything", &copied)
        });
        assert!(
            !copied.get(),
            "with no ClipboardHandle registered there is nothing to confirm"
        );
    }

    /// The guard that makes a layout-time signal write safe: a steady state must stop
    /// writing. If it did not, every pass would schedule another one forever.
    #[test]
    fn a_steady_state_stops_writing_the_signal() {
        let long = "aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj kkkk llll mmmm nnnn";
        let state = Signal::new(BodyState::Fits.as_u8());
        let (mut t, _id) = lay_out(long, state.clone());
        assert_eq!(state.get(), BodyState::Collapsed.as_u8());

        // Count notifications across further passes: a settled body must emit none.
        let writes = Rc::new(Cell::new(0usize));
        let w = writes.clone();
        let _handle = state.observe(move |_| w.set(w.get() + 1));

        for _ in 0..5 {
            t.layout(SizeProposal {
                width: Some(WIDTH),
                height: None,
            });
        }

        assert_eq!(
            writes.get(),
            0,
            "a settled body must not keep rewriting its state signal"
        );
    }
}
