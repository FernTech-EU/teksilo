//! MenuContext — shared navigation state for the menu hierarchy.
//!
//! Created by `MenuBar` during `build()` and passed to `MenuOverlayHost` and
//! `MenuBarTrigger` widgets. Coordinates opening/closing menus, focus transfer,
//! and Left/Right arrow navigation between top-level menus.

use std::cell::RefCell;
use std::rc::Rc;

use bastyde_core::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
use bastyde_core::signal::Signal;
use bastyde_core::widget::EventContext;
use bastyde_core::widget_id::WidgetId;

struct MenuContextInner {
    trigger_ids: Vec<WidgetId>,
    /// The overlay host widget IDs (MenuOverlayHost — shown as overlay content).
    content_ids: Vec<WidgetId>,
    /// The inner focusable widget IDs (MenuList inside the host — receives focus).
    focus_ids: Vec<WidgetId>,
}

/// Shared state for a menu bar and its dropdowns.
#[derive(Clone)]
pub(crate) struct MenuContext {
    inner: Rc<RefCell<MenuContextInner>>,
    /// Which top-level menu index is open (None = all closed).
    /// Used by MenuBarTrigger to derive bg_color / text_color.
    pub open_index: Signal<Option<usize>>,
}

impl MenuContext {
    pub fn new(open_index: Signal<Option<usize>>) -> Self {
        Self {
            inner: Rc::new(RefCell::new(MenuContextInner {
                trigger_ids: Vec::new(),
                content_ids: Vec::new(),
                focus_ids: Vec::new(),
            })),
            open_index,
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
        ctx.activate(content_id);
        ctx.show_overlay(OverlayRequest {
            content_id,
            anchor: trigger_id,
            placement: OverlayPlacement::BelowPreferred,
            dismiss: DismissBehavior::EscapeOrClickOutside,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        // Focus the inner MenuList (not the host) so it receives key events
        // first. ArrowLeft/Escape bubble from MenuList → host for cross-menu nav.
        ctx.request_focus(focus_id);
    }

    /// Close all menus and reset the open_index signal.
    /// Restores focus to the currently open menu trigger if any.
    pub fn close(&self, ctx: &mut EventContext) {
        let current_index = self.open_index.get();
        self.open_index.set(None);
        ctx.dismiss_all_except_hosts();

        // Restore focus to the trigger that was open
        if let Some(index) = current_index
            && let Some(trigger_id) = self.trigger_id(index)
        {
            ctx.request_focus(trigger_id);
        }
    }

    /// Navigate to the menu at `(current + delta) % count`, wrapping around.
    /// Properly manages focus between menu triggers.
    pub fn navigate(&self, delta: i32, ctx: &mut EventContext) {
        let count = self.count();
        if count == 0 {
            return;
        }

        let current_index = self.open_index.get();
        let current = current_index.unwrap_or(0) as i32;
        let next = ((current + delta).rem_euclid(count as i32)) as usize;

        // If we're navigating from an open menu, close it first and restore focus to trigger
        if let Some(current_index) = current_index
            && let Some(trigger_id) = self.trigger_id(current_index)
        {
            ctx.request_focus(trigger_id);
        }

        // Open the next menu
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
