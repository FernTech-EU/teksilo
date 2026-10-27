// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `TextSurface` — "the widget the caret is in", as an application sees it.
//!
//! # Why the framework has to answer this
//!
//! An application that wants **one** Undo command — one chord, one menu row,
//! routed to whatever the writer is actually editing — handles the
//! [`edit.undo`](crate::shortcut::StandardEdit::Undo) intent. From the chord,
//! the walk starts at the focused widget, so a text widget's own action
//! answers before the application's. Two routes do not pass through the
//! widget at all, and there the application owes it an answer: a menu row or
//! a toolbar button, whose intent starts in the chrome, and a widget that
//! refused its history (a forward-only writing mode), which passes the intent
//! on rather than letting it reach the application's own history.
//!
//! It cannot produce one by itself. It can recognise the surfaces it built and
//! kept a handle on, and it is blind to the rest: a rename box in a table cell,
//! a search field, a text input inside a dialog it did not write. An application
//! that guesses gets it exactly backwards — Ctrl+Z in a rename box undoes
//! something else entirely, which is worse than not having the feature.
//!
//! Maintaining a list of every text widget in the application is not a fix: it
//! is correct on the day it is written and wrong the first time someone adds a
//! field. The framework already knows which widget has focus and which widgets
//! edit text, so it is the only place the question can be answered *completely*.
//!
//! # What implementors get
//!
//! Every text widget registers itself with [`BuildContext::register_text_surface`],
//! and the registration is torn down with the widget exactly as a global action
//! is. A host then asks [`WidgetTree::focused_text_surface`] and either drives it
//! or — knowing one exists — steps aside and lets the widget keep its own keys.
//!
//! # An empty history can pass the command on
//!
//! By default the focused text widget answers Undo and Redo whether or not it
//! has anything to step through, so an application's own `edit.undo` action is
//! reached only from a menu, or where no text widget has focus. An application
//! whose Undo deliberately falls back to a wider history when the caret's own
//! is exhausted — and says so, in a menu row naming what it will undo — turns
//! on [`TextSurfaces::set_empty_history_passes_on`]: a text widget with nothing
//! to undo then declines the command, and the intent climbs to the
//! application's action exactly as it does from a refused history. Off by
//! default, because an application that never announces the fallback would
//! have Ctrl+Z in an empty search field undo something the user is not looking
//! at.
//!
//! [`BuildContext::register_text_surface`]: crate::build_context::BuildContext::register_text_surface
//! [`WidgetTree::focused_text_surface`]: crate::widget_tree::WidgetTree::focused_text_surface

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::shortcut::StandardEdit;
use crate::signal::Signal;
use crate::widget::EventContext;
use crate::widget_id::WidgetId;

/// A widget that edits text, seen through the commands a host may need to
/// invoke on it from outside — a menu row, a routed shortcut, an assistive
/// technology.
///
/// Object-safe on purpose: a host holds `Rc<dyn TextSurface>` for whichever
/// widget has focus, without knowing which kind it is.
pub trait TextSurface {
    /// Is there anything in this surface's own history to step back through?
    fn can_undo(&self) -> bool;
    /// Is there anything to step forward into?
    fn can_redo(&self) -> bool;
    fn undo(&self);
    fn redo(&self);

    /// Is this surface refusing to step through its history at all?
    ///
    /// Distinct from having nothing to undo, and a host must treat them
    /// differently: an empty history may be a reason to look elsewhere, a
    /// refusal is not. Applications impose modes — a "forbid erasing" writing
    /// game, a read-only review pass — and a routed Undo that quietly went
    /// somewhere else would defeat them.
    fn history_frozen(&self) -> bool {
        false
    }

    /// Is any text selected right now?
    fn has_selection(&self) -> bool;
    /// Does this surface refuse edits? Cut and Paste are meaningless when it does.
    fn is_read_only(&self) -> bool;
    /// May its contents be copied at all? A password field says no.
    fn allows_copy(&self) -> bool;

    /// Does this surface take `command` at all, in the mode it is in now?
    ///
    /// The surface's own answer to a command from any route, the one its keys
    /// give: a forward-only editor takes a paste and refuses a cut, a
    /// read-only one refuses both, a masked password field refuses a copy.
    /// [`is_read_only`](Self::is_read_only) cannot say "takes a paste, refuses
    /// a cut", which is why this exists.
    ///
    /// Not whether there is anything to act on: a host greys a Cut row with
    /// `!accepts(Cut) || !has_selection()`, an Undo row with
    /// [`can_undo`](Self::can_undo) as well.
    ///
    /// The surface's own commands obey it. [`cut`](Self::cut),
    /// [`copy`](Self::copy), [`paste`](Self::paste),
    /// [`paste_plain`](Self::paste_plain), [`undo`](Self::undo),
    /// [`redo`](Self::redo) and [`select_all`](Self::select_all) do nothing
    /// for a command this refuses, so a shortcut a host routes to the focused
    /// surface cannot write into a read-only one or take text from a
    /// forward-only one.
    ///
    /// The default answers from the other questions, for an implementor that
    /// has no finer rule: a cut needs an editable surface that allows copying,
    /// a paste an editable one, a copy one that allows copying, Undo and Redo a
    /// history that is not frozen.
    fn accepts(&self, command: StandardEdit) -> bool {
        match command {
            StandardEdit::Cut => !self.is_read_only() && self.allows_copy(),
            StandardEdit::Copy => self.allows_copy(),
            StandardEdit::Paste | StandardEdit::PastePlain => !self.is_read_only(),
            StandardEdit::Undo | StandardEdit::Redo => !self.history_frozen(),
            StandardEdit::SelectAll => true,
        }
    }

    fn cut(&self, ctx: &EventContext<'_>);
    fn copy(&self, ctx: &EventContext<'_>);
    fn paste(&self, ctx: &EventContext<'_>);
    /// Paste stripped of formatting. A surface with no formatting to strip
    /// should do a plain paste rather than nothing.
    fn paste_plain(&self, ctx: &EventContext<'_>);
    fn select_all(&self);
}

/// A cloneable view of one tree's registered text surfaces.
///
/// Taken once, with [`BuildContext::text_surfaces`], and held by whatever needs
/// to ask the question later — a view-model refreshed from a frame tick has no
/// `&WidgetTree` to consult, which is precisely when it needs the answer.
///
/// [`BuildContext::text_surfaces`]: crate::build_context::BuildContext::text_surfaces
#[derive(Clone)]
pub struct TextSurfaces {
    entries: Rc<RefCell<Vec<(WidgetId, Rc<dyn TextSurface>)>>>,
    focused: Signal<Option<WidgetId>>,
    empty_history_passes_on: Rc<Cell<bool>>,
}

impl std::fmt::Debug for TextSurfaces {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextSurfaces")
            .field("registered", &self.entries.borrow().len())
            .field("focused_is_text", &self.focused_is_text_surface())
            .field("empty_history_passes_on", &self.empty_history_passes_on())
            .finish()
    }
}

impl TextSurfaces {
    pub(crate) fn new(focused: Signal<Option<WidgetId>>) -> Self {
        Self {
            entries: Rc::new(RefCell::new(Vec::new())),
            focused,
            empty_history_passes_on: Rc::new(Cell::new(false)),
        }
    }

    /// Record that `owner` edits text, replacing any previous registration from
    /// the same widget so a rebuild re-points rather than accumulating.
    pub(crate) fn insert(&self, owner: WidgetId, surface: Rc<dyn TextSurface>) {
        let mut entries = self.entries.borrow_mut();
        entries.retain(|(id, _)| *id != owner);
        entries.push((owner, surface));
    }

    /// Forget `owner`'s registration — on its rebuild or destroy.
    pub(crate) fn remove(&self, owner: WidgetId) {
        self.entries.borrow_mut().retain(|(id, _)| *id != owner);
    }

    /// The text-editing widget that currently holds the keyboard focus.
    ///
    /// `None` when focus is elsewhere — or nowhere — which is exactly what a
    /// host needs in order to know that a text chord is safe to route itself.
    pub fn focused(&self) -> Option<Rc<dyn TextSurface>> {
        let focused = self.focused.get()?;
        self.entries
            .borrow()
            .iter()
            .find(|(id, _)| *id == focused)
            .map(|(_, s)| Rc::clone(s))
    }

    /// Is the keyboard focus inside a widget that edits text? The cheap half of
    /// [`focused`](Self::focused), for a host that only needs to decide whether
    /// to step aside.
    pub fn focused_is_text_surface(&self) -> bool {
        self.focused().is_some()
    }

    /// Does the widget registered under `owner` refuse edits?
    ///
    /// `false` for a widget that edits nothing — the question is only asked of
    /// text surfaces, and a node that is not one is not read-only, it is simply
    /// not text. Read by the hit-test slop pass, which must never re-attribute
    /// a press to a surface that would ignore it.
    pub fn is_read_only(&self, owner: WidgetId) -> bool {
        self.entries
            .borrow()
            .iter()
            .find(|(id, _)| *id == owner)
            .map(|(_, s)| s.is_read_only())
            .unwrap_or(false)
    }

    /// Make every text widget of this tree decline Undo when it has nothing to
    /// undo, and Redo when it has nothing to redo, so the intent goes on to an
    /// ancestor's action — the application's own history — instead of stopping
    /// at a widget that would do nothing with it. See the
    /// [module documentation](self#an-empty-history-can-pass-the-command-on).
    ///
    /// Off by default. Applies to `RichTextEditor`, `TextInputField` and the
    /// fields built on it, and `CodeEditor`, from the next command they
    /// receive; set it once, from the root's `build`. The widgets ask their
    /// document at the moment the command runs, never a cached
    /// [`can_undo`](TextSurface::can_undo), so a keystroke typed a moment
    /// earlier is always undone where it was typed.
    pub fn set_empty_history_passes_on(&self, on: bool) {
        self.empty_history_passes_on.set(on);
    }

    /// Whether a text widget with nothing to undo (or redo) passes the command
    /// on. See [`set_empty_history_passes_on`](Self::set_empty_history_passes_on).
    pub fn empty_history_passes_on(&self) -> bool {
        self.empty_history_passes_on.get()
    }

    /// The signal to react to. A host mirroring "can undo" into a menu row
    /// re-reads when focus moves.
    pub fn focus_signal(&self) -> Signal<Option<WidgetId>> {
        self.focused.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A surface with no rule of its own, answering only the older questions.
    struct Plain {
        read_only: bool,
        allows_copy: bool,
        frozen: bool,
    }

    impl TextSurface for Plain {
        fn can_undo(&self) -> bool {
            true
        }
        fn can_redo(&self) -> bool {
            true
        }
        fn undo(&self) {}
        fn redo(&self) {}
        fn history_frozen(&self) -> bool {
            self.frozen
        }
        fn has_selection(&self) -> bool {
            true
        }
        fn is_read_only(&self) -> bool {
            self.read_only
        }
        fn allows_copy(&self) -> bool {
            self.allows_copy
        }
        fn cut(&self, _: &EventContext<'_>) {}
        fn copy(&self, _: &EventContext<'_>) {}
        fn paste(&self, _: &EventContext<'_>) {}
        fn paste_plain(&self, _: &EventContext<'_>) {}
        fn select_all(&self) {}
    }

    /// The default `accepts` follows the questions an implementor already
    /// answers, so one written before it existed keeps meaning what it said.
    #[test]
    fn the_default_accepts_follows_the_older_questions() {
        let editable = Plain {
            read_only: false,
            allows_copy: true,
            frozen: false,
        };
        for command in [
            StandardEdit::Cut,
            StandardEdit::Copy,
            StandardEdit::Paste,
            StandardEdit::PastePlain,
            StandardEdit::Undo,
            StandardEdit::Redo,
            StandardEdit::SelectAll,
        ] {
            assert!(editable.accepts(command), "{command:?}");
        }

        let read_only = Plain {
            read_only: true,
            allows_copy: true,
            frozen: true,
        };
        assert!(!read_only.accepts(StandardEdit::Cut));
        assert!(!read_only.accepts(StandardEdit::Paste));
        assert!(!read_only.accepts(StandardEdit::PastePlain));
        assert!(!read_only.accepts(StandardEdit::Undo));
        assert!(read_only.accepts(StandardEdit::Copy));
        assert!(read_only.accepts(StandardEdit::SelectAll));

        let secret = Plain {
            read_only: false,
            allows_copy: false,
            frozen: false,
        };
        assert!(!secret.accepts(StandardEdit::Copy));
        assert!(!secret.accepts(StandardEdit::Cut), "a cut is a copy too");
        assert!(secret.accepts(StandardEdit::Paste));
    }
}
