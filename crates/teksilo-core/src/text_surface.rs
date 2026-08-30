// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `TextSurface` — "the widget the caret is in", as an application sees it.
//!
//! # Why the framework has to answer this
//!
//! An application that wants **one** Undo command — one chord, one menu row,
//! routed to whatever the writer is actually editing — has to register that
//! chord globally, because shortcuts resolve before any widget sees the raw key.
//! The moment it does, it has taken `Ctrl+Z` away from every text widget in the
//! tree, and it owes each of them an answer.
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
//! [`BuildContext::register_text_surface`]: crate::build_context::BuildContext::register_text_surface
//! [`WidgetTree::focused_text_surface`]: crate::widget_tree::WidgetTree::focused_text_surface

use std::cell::RefCell;
use std::rc::Rc;

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
}

impl std::fmt::Debug for TextSurfaces {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextSurfaces")
            .field("registered", &self.entries.borrow().len())
            .field("focused_is_text", &self.focused_is_text_surface())
            .finish()
    }
}

impl TextSurfaces {
    pub(crate) fn new(focused: Signal<Option<WidgetId>>) -> Self {
        Self {
            entries: Rc::new(RefCell::new(Vec::new())),
            focused,
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

    /// The signal to react to. A host mirroring "can undo" into a menu row
    /// re-reads when focus moves.
    pub fn focus_signal(&self) -> Signal<Option<WidgetId>> {
        self.focused.clone()
    }
}
