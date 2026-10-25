// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! MenuContext — shared navigation state for the menu hierarchy.
//!
//! Created by `MenuBar` during `build()` and passed to `MenuOverlayHost` and
//! `MenuBarTrigger` widgets. Coordinates opening/closing menus, focus transfer,
//! and Left/Right arrow navigation between top-level menus.
//!
//! # Where focus goes
//!
//! A trigger takes no focus from a click (`focus_on_click(false)`), so a menu
//! opened by a click is opened over the editor the user was working in, and
//! the overlay gives focus back to that editor however the menu closes.
//!
//! F10, a bare Alt tap and Alt+letter focus a trigger on purpose (the bar's
//! keyboard mode, as on Windows). A menu opened from there gives focus back to
//! its trigger, which is right for Escape: the menu closes and its title stays
//! focused. For every other way out the trigger hands focus on as it gets it
//! back ([`take_handoff`](MenuContext::take_handoff)): to the next menu's
//! title when the user moved sideways, and back to the editor when a row was
//! chosen or the menu was clicked away, so a command sent from focus acts on
//! the editor. Escape on a focused trigger leaves the bar for the editor.
//! The bar keeps this memory across its rebuilds ([`MenuFocus`]): a model
//! change while F10 holds focus on a title still lets Escape go back to the
//! editor.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use teksilo_core::overlay::{
    DismissBehavior, DismissReason, OverlayDismissCallback, OverlayPlacement, OverlayRequest,
};
use teksilo_core::signal::Signal;
use teksilo_core::widget::EventContext;
use teksilo_core::widget_id::WidgetId;

struct MenuContextInner {
    trigger_ids: Vec<WidgetId>,
    /// The overlay host widget IDs (MenuOverlayHost — shown as overlay content).
    content_ids: Vec<WidgetId>,
    /// The inner focusable widget IDs (MenuList inside the host — receives focus).
    focus_ids: Vec<WidgetId>,
    /// The collapsed bar's hamburger button, which takes focus on its way into
    /// the bar like a trigger does.
    hamburger_id: Option<WidgetId>,
}

/// Shared state for a menu bar and its dropdowns.
#[derive(Clone)]
pub(crate) struct MenuContext {
    inner: Rc<RefCell<MenuContextInner>>,
    /// Which top-level menu index is open (None = all closed).
    /// Used by MenuBarTrigger to derive bg_color / text_color.
    ///
    /// Follows the open menu's overlay: set by [`open_at`](Self::open_at),
    /// cleared by that overlay's dismissal, whatever closed it. A trigger's
    /// `expanded` state and its press-to-toggle both read it, so it must
    /// never say a menu is open when none is, or the reverse.
    pub open_index: Signal<Option<usize>>,
    /// Bumped by every [`open_at`](Self::open_at). An overlay's dismissal
    /// clears `open_index` only if no menu was opened after it: reopening
    /// the open menu dismisses its old overlay *after* the new open has
    /// claimed `open_index`, and must not have it cleared behind it.
    open_generation: Rc<Cell<u64>>,
    /// Where focus was before the bar took it, and where it goes next. Owned
    /// by the `MenuBar` rather than by this context, which each build of the
    /// bar makes anew: a menu bar rebuilt while F10 holds focus on a title
    /// (its model changed) must still give focus back to the editor.
    focus: MenuFocus,
}

/// The focus a menu bar remembers across its rebuilds. See [`MenuContext`]'s
/// module documentation.
#[derive(Clone, Default)]
pub(crate) struct MenuFocus {
    /// The widget the window's focus was on outside the bar: the one the user
    /// was working in before a trigger took focus (F10 focuses one, and so
    /// does Tab). A menu opened from the bar gives focus back to it when it
    /// closes, so a command chosen there and sent from focus acts on that
    /// widget, and the user goes on typing where they were. Kept by
    /// [`MenuContext::note_focus`].
    outside_focus: Rc<Cell<Option<WidgetId>>>,
    /// Where a trigger sends the focus a closing menu is about to give back
    /// to it. Set by the menu's dismissal; see
    /// [`take_handoff`](MenuContext::take_handoff).
    handoff: Rc<Cell<Option<Handoff>>>,
    /// Whether the dismissal that set `handoff` has cleared the focus its menu
    /// held, which is what tells a trigger getting focus back from the
    /// dismissal from one focused by F10 or Tab afterwards.
    handoff_armed: Rc<Cell<bool>>,
    /// Whether a build of the bar has read the window's focus yet. Only the
    /// first does: when a rebuild runs, focus may be on a title of the bar it
    /// replaces, which is not focus outside the bar.
    primed: Rc<Cell<bool>>,
}

impl std::fmt::Debug for MenuFocus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MenuFocus")
            .field("outside_focus", &self.outside_focus.get())
            .finish_non_exhaustive()
    }
}

/// Where focus goes once a closing bar menu has given it back to a trigger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Handoff {
    /// Back to the widget focused before the bar took focus.
    Outside,
    /// To this trigger, the title of the menu opened in the closing one's place.
    Trigger(WidgetId),
}

impl MenuContext {
    pub fn new(open_index: Signal<Option<usize>>, focus: MenuFocus) -> Self {
        Self {
            inner: Rc::new(RefCell::new(MenuContextInner {
                trigger_ids: Vec::new(),
                content_ids: Vec::new(),
                focus_ids: Vec::new(),
                hamburger_id: None,
            })),
            open_index,
            open_generation: Rc::new(Cell::new(0)),
            focus,
        }
    }

    /// Read the window's focus as this build of the bar starts following it:
    /// the first build only. A rebuild keeps what the bars before it saw.
    pub fn prime_focus(&self, focus: Option<WidgetId>) {
        if !self.focus.primed.replace(true) {
            self.note_focus(focus);
        }
    }

    /// Record the collapsed bar's hamburger button, so focus on it does not
    /// count as focus outside the bar.
    pub fn set_hamburger(&self, id: WidgetId) {
        self.inner.borrow_mut().hamburger_id = Some(id);
    }

    /// The window's focus moved to `focus`. Called as the move happens, before
    /// the widget that gains focus hears of it.
    ///
    /// While no menu of the bar is open, focus on a widget other than the
    /// bar's own triggers and menus is the focus outside the bar. A menu
    /// closing with focus inside it clears the focus before giving it back,
    /// which arms the handoff its dismissal set; any other move than the one
    /// giving it back to a trigger drops the handoff.
    pub fn note_focus(&self, focus: Option<WidgetId>) {
        let Some(id) = focus else {
            if self.focus.handoff.get().is_some() {
                self.focus.handoff_armed.set(true);
            }
            return;
        };
        let inner = self.inner.borrow();
        let on_trigger = inner.trigger_ids.contains(&id);
        if !(on_trigger && self.focus.handoff_armed.get()) {
            self.focus.handoff.set(None);
            self.focus.handoff_armed.set(false);
        }
        if self.open_index.get().is_some() {
            return;
        }
        let inside = on_trigger
            || inner.content_ids.contains(&id)
            || inner.focus_ids.contains(&id)
            || inner.hamburger_id == Some(id);
        if !inside {
            self.focus.outside_focus.set(Some(id));
        }
    }

    /// A trigger got focus: send it on if a closing menu just gave it back.
    ///
    /// A menu opened from a focused trigger (F10, Alt) gives focus back to
    /// that trigger. When it closed because another menu opened in its place,
    /// focus goes on to that menu's title, so Escape there lands on it; when
    /// it closed for any reason but Escape (a row chosen, a click outside, its
    /// title clicked), focus goes back to the widget focused before the bar
    /// took it, and the bar's keyboard mode ends. Called from the trigger's
    /// focus handler, so the move is made before the handler that closed the
    /// menu sends anything from focus.
    pub fn take_handoff(&self, trigger: WidgetId, ctx: &mut EventContext) {
        if !self.focus.handoff_armed.replace(false) {
            return;
        }
        match self.focus.handoff.take() {
            Some(Handoff::Trigger(next)) if next != trigger => ctx.request_focus(next),
            Some(Handoff::Outside) => self.leave(ctx),
            _ => {}
        }
    }

    /// Give focus back to the widget focused before the bar took it, or to
    /// nothing when nothing was. Escape on a focused trigger.
    pub fn leave(&self, ctx: &mut EventContext) {
        match self.focus.outside_focus.get() {
            Some(outside) => ctx.request_focus(outside),
            None => ctx.clear_focus(),
        }
    }

    pub fn register(
        &self,
        index: usize,
        trigger_id: WidgetId,
        content_id: WidgetId,
        focus_id: WidgetId,
    ) {
        let mut inner = self.inner.borrow_mut();
        let existing_focus_id = inner.focus_ids.get(index).copied();
        while inner.trigger_ids.len() <= index {
            inner.trigger_ids.push(trigger_id);
        }
        while inner.content_ids.len() <= index {
            inner.content_ids.push(content_id);
        }
        while inner.focus_ids.len() <= index {
            inner.focus_ids.push(focus_id);
        }
        inner.trigger_ids[index] = trigger_id;
        inner.content_ids[index] = content_id;
        if existing_focus_id.is_none() {
            inner.focus_ids[index] = focus_id;
        }
    }

    /// Set the focus target for a menu index (called by MenuOverlayHost::build
    /// after the inner MenuList is created).
    pub fn set_focus_id(&self, index: usize, focus_id: WidgetId) {
        let mut inner = self.inner.borrow_mut();
        while inner.focus_ids.len() <= index {
            inner.focus_ids.push(focus_id);
        }
        inner.focus_ids[index] = focus_id;
    }

    pub fn count(&self) -> usize {
        self.inner.borrow().trigger_ids.len()
    }

    /// Open the menu at `index`. Dismisses any currently open overlays,
    /// activates the content widget, shows the overlay, and focuses it.
    pub fn open_at(&self, index: usize, ctx: &mut EventContext) {
        let inner = self.inner.borrow();
        if index >= inner.content_ids.len() {
            return;
        }
        let content_id = inner.content_ids[index];
        let trigger_id = inner.trigger_ids[index];
        let focus_id = inner.focus_ids[index];
        drop(inner);

        ctx.dismiss_all_except_hosts();
        self.open_index.set(Some(index));
        let generation = self.open_generation.get().wrapping_add(1);
        self.open_generation.set(generation);
        // Closed by any route (Escape, an outside press, focus leaving, an
        // item chosen in a submenu, a sibling menu replacing it), the menu
        // stops being the open one. Focus leaving was the only route that
        // used to say so, and it missed a menu closed by choosing an item in
        // its submenu.
        //
        // It also says where a trigger that gets focus back from it sends it
        // on (`take_handoff`): to the title of the menu replacing it, nowhere
        // after Escape, back out of the bar otherwise.
        let on_dismiss: OverlayDismissCallback = {
            let menu_ctx = self.clone();
            Rc::new(move |reason, _| {
                let replaced = menu_ctx.open_generation.get() != generation;
                let handoff = if replaced {
                    menu_ctx
                        .open_index
                        .get()
                        .and_then(|next| menu_ctx.trigger_id(next))
                        .map(Handoff::Trigger)
                } else {
                    menu_ctx.open_index.set(None);
                    (reason != DismissReason::Escape).then_some(Handoff::Outside)
                };
                menu_ctx.focus.handoff.set(handoff);
                menu_ctx.focus.handoff_armed.set(false);
            })
        };
        // Build this menu's content if it has never been opened, before the
        // overlay below is measured against it and focus moves into it. Set
        // `open_index` first — that is the signal the deferred host reveals on.
        ctx.materialize_now(content_id);
        ctx.activate(content_id);
        let mut request = OverlayRequest::new(
            content_id,
            trigger_id,
            OverlayPlacement::BelowPreferred,
            DismissBehavior::EscapeOrClickOutside,
        )
        .on_dismiss(on_dismiss);
        // A command chosen here acts on the widget focused before the bar,
        // also when the keyboard focused a title and the menu gives focus
        // back to it: so a row asks whether that widget's route handles its
        // command, not the bar's.
        if let Some(outside) = self.focus.outside_focus.get() {
            request = request.command_source(outside);
        }
        ctx.show_overlay(request);
        // Focus the inner MenuList (not the host) so it receives key events
        // first. ArrowLeft/Escape bubble from MenuList → host for cross-menu nav.
        ctx.request_focus(focus_id);
    }

    /// Close all menus and reset the open_index signal. Focus goes where the
    /// closing menu gives it back (see the module documentation).
    pub fn close(&self, ctx: &mut EventContext) {
        self.open_index.set(None);
        ctx.dismiss_all_except_hosts();
    }

    /// Navigate to the menu at `(current + delta) % count`, wrapping around.
    /// The menu it replaces gives focus back as it closes, and the new one
    /// takes it.
    pub fn navigate(&self, delta: i32, ctx: &mut EventContext) {
        let count = self.count();
        if count == 0 {
            return;
        }
        let current = self.open_index.get().unwrap_or(0) as i32;
        let next = ((current + delta).rem_euclid(count as i32)) as usize;
        self.open_at(next, ctx);
    }

    /// Get the trigger widget ID for an index (for focus return).
    pub fn trigger_id(&self, index: usize) -> Option<WidgetId> {
        self.inner.borrow().trigger_ids.get(index).copied()
    }
}

impl std::fmt::Debug for MenuContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MenuContext")
            .field("count", &self.count())
            .finish()
    }
}
