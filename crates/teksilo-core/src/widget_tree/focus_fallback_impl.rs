// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Where focus goes when the widget that had it, or the one it was meant to
//! go back to, is gone.
//!
//! One chain serves every overlay dismissal, the rebuild restore and a focused
//! node that becomes disabled:
//!
//! 1. the restore target, when it is active and enabled;
//! 2. the restore target of the overlay the target lived in, when the target
//!    was inside an overlay that has gone too (a menu row that opened a
//!    dialog): where focus was before that overlay opened;
//! 3. the dismissed overlay's anchor, or the nearest focusable above it;
//! 4. inside the innermost surviving ancestor of the target, its first Tab
//!    stop, else the nearest focusable above it;
//! 5. the first Tab stop of the current focus scope (the topmost modal, else
//!    the window).
//!
//! Every step after the first runs only when the dismissal left focus nowhere:
//! focus the user or a handler put somewhere stays there.

use super::*;
use crate::overlay::{FocusReturn, OverlayId, RemovedOverlay};

/// Taken before a dismissal, read by
/// [`WidgetTree::return_focus_after_dismissal`] after it: which overlays the
/// dismissal removed, and whether anything held focus before it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DismissMark {
    seq: u64,
    focus_before: Option<WidgetId>,
}

impl DismissMark {
    /// What held focus when the mark was taken.
    pub(crate) fn focused_before(&self) -> Option<WidgetId> {
        self.focus_before
    }
}

impl WidgetTree {
    /// Record `focus_id` as where focus goes back to when the topmost overlay
    /// is dismissed, with what the fallback chain needs if it cannot.
    pub(crate) fn record_top_focus_restore(&mut self, focus_id: WidgetId) {
        self.overlay_manager.set_top_focus_restore(focus_id);
        self.sync_focus_returns();
    }

    /// Fill in the [`FocusReturn`] of every overlay whose restore target has
    /// none yet, or a stale one. [`record_top_focus_restore`](Self::record_top_focus_restore)
    /// does it at once; a target set straight on the manager (teksilo-app's
    /// modal presentation) gets it here at the next layout pass, while its
    /// ancestors still exist.
    pub(crate) fn sync_focus_returns(&mut self) {
        let stale: Vec<(OverlayId, WidgetId)> = self
            .overlay_manager
            .stack
            .iter()
            .filter_map(|overlay| {
                let target = overlay.focus_restore?;
                let recorded = overlay.focus_return.as_ref().and_then(|r| r.target);
                (recorded != Some(target)).then_some((overlay.id, target))
            })
            .collect();
        for (id, target) in stale {
            let record = self.focus_return_for(id, target);
            if let Some(overlay) = self.overlay_manager.stack.iter_mut().find(|o| o.id == id) {
                overlay.focus_return = Some(record);
            }
        }
    }

    /// What [`sync_focus_returns`](Self::sync_focus_returns) records for
    /// `overlay`, whose restore target is `target`. The forwarding chain is
    /// computed now, while the overlays the target lives in are still open.
    fn focus_return_for(&self, overlay: OverlayId, target: WidgetId) -> FocusReturn {
        let mut forward = Vec::new();
        let mut visited = vec![overlay];
        let mut current = target;
        while let Some(holder) = self
            .overlay_manager
            .stack
            .iter()
            .find(|o| !visited.contains(&o.id) && self.is_descendant_of(current, o.content_id))
        {
            visited.push(holder.id);
            let Some(next) = holder.focus_restore else {
                break;
            };
            forward.push((holder.id, next, self.strict_ancestors_of(Some(next))));
            current = next;
        }
        FocusReturn {
            target: Some(target),
            ancestors: self.strict_ancestors_of(Some(target)),
            forward,
        }
    }

    /// Take before a dismissal, for
    /// [`return_focus_after_dismissal`](Self::return_focus_after_dismissal).
    pub(crate) fn dismissal_mark(&self) -> DismissMark {
        DismissMark {
            seq: self.overlay_manager.removal_mark(),
            focus_before: self.focused,
        }
    }

    /// Whether focus is on nothing, or on a node that can no longer take
    /// events.
    pub(super) fn focus_left_nowhere(&self) -> bool {
        self.focused.is_none_or(|id| !self.arena.is_active(id))
    }

    /// Whether `id` can be given focus back: active and enabled. Whether it
    /// is declared focusable is not asked, as before: a node focused
    /// programmatically without `.focusable()` is restored to.
    fn can_take_restored_focus(&self, id: WidgetId) -> bool {
        self.arena.is_active(id) && self.arena.is_enabled(id)
    }

    /// The nearest active, enabled, focusable node at or above `id`.
    fn enabled_focusable_at_or_above(&self, id: WidgetId) -> Option<WidgetId> {
        let mut current = Some(id);
        while let Some(cur) = current {
            let found = self.find_focusable_at_or_above(cur)?;
            if self.can_take_restored_focus(found) {
                return Some(found);
            }
            current = self.arena.parent(found);
        }
        None
    }

    /// Inside the innermost of `ancestors` that is still active: its first
    /// enabled Tab stop, else the nearest focusable above it.
    fn focus_in_surviving_ancestor(&self, ancestors: &[WidgetId]) -> Option<WidgetId> {
        let survivor = ancestors
            .iter()
            .copied()
            .find(|&id| self.arena.is_active(id))?;
        self.first_focusable_descendant(survivor)
            .filter(|&id| self.can_take_restored_focus(id))
            .or_else(|| self.enabled_focusable_at_or_above(survivor))
    }

    /// Give focus back after an overlay dismissal taken under `mark`.
    ///
    /// `target` is the restore target the dismissal path hands back, focused
    /// whenever it is active and enabled, as every path always did. Otherwise,
    /// and only when the dismissal took focus away (something held it before
    /// and nothing active holds it now), the rest of the chain in the module
    /// docs runs. A path that restores nothing itself passes `None`; the
    /// dismissed overlay's own target is then the first candidate. An overlay
    /// that recorded no target (nothing was focused when it opened) leaves
    /// focus on nothing, where it was.
    ///
    /// Returns the id it focused.
    pub(crate) fn return_focus_after_dismissal(
        &mut self,
        target: Option<WidgetId>,
        mark: DismissMark,
        ops: &mut dyn crate::window::WindowOps,
    ) -> Option<WidgetId> {
        if let Some(id) = target
            && self.can_take_restored_focus(id)
        {
            self.focus_ops(id, ops);
            return Some(id);
        }
        if mark.focus_before.is_none() || !self.focus_left_nowhere() {
            return None;
        }
        let removed: Vec<RemovedOverlay> = self
            .overlay_manager
            .removed_since(mark.seq)
            .cloned()
            .collect();
        let entry = match target {
            Some(id) => removed.iter().find(|r| r.focus_restore == Some(id)),
            None => removed
                .iter()
                .find(|r| r.focus_restore.is_some())
                .or(removed.first()),
        };
        let found = self.dismissal_fallback(target, entry)?;
        self.focus_ops(found, ops);
        Some(found)
    }

    /// Steps 2 to 5 of the chain for the overlay `entry` records.
    fn dismissal_fallback(
        &self,
        target: Option<WidgetId>,
        entry: Option<&RemovedOverlay>,
    ) -> Option<WidgetId> {
        let own_target = entry.and_then(|e| e.focus_restore);
        // A path that restores nothing itself: the overlay's own target first.
        if target.is_none()
            && let Some(id) = own_target
            && self.can_take_restored_focus(id)
        {
            return Some(id);
        }
        // Nothing was focused when the overlay opened (no target recorded):
        // focus goes back to nothing, where it was.
        let primary = target.or(own_target)?;
        let primary = Some(primary);
        let mut record = entry
            .and_then(|e| e.focus_return.clone())
            .filter(|r| r.target.is_some() && r.target == primary)
            .unwrap_or_else(|| FocusReturn {
                target: primary,
                ancestors: self.strict_ancestors_of(primary),
                forward: Vec::new(),
            });
        // Forward only through overlays that have gone: a target inside an
        // overlay still open (a field of the outer of two modals) falls back
        // inside that overlay, by its ancestors, never behind it.
        if let Some(open) = record
            .forward
            .iter()
            .position(|(holder, _, _)| self.overlay_manager.overlay(*holder).is_some())
        {
            record.forward.truncate(open);
        }
        if let Some(id) = record
            .forward
            .iter()
            .map(|(_, id, _)| *id)
            .find(|&id| self.can_take_restored_focus(id))
        {
            return Some(id);
        }
        if let Some(anchor) = entry.map(|e| e.anchor)
            && self.arena.is_active(anchor)
            && let Some(id) = self.enabled_focusable_at_or_above(anchor)
        {
            return Some(id);
        }
        std::iter::once(&record.ancestors)
            .chain(record.forward.iter().map(|(_, _, ancestors)| ancestors))
            .find_map(|ancestors| self.focus_in_surviving_ancestor(ancestors))
            .or_else(|| self.first_tab_stop_in_scope())
    }

    /// The rebuild restore's fallback: when the rebuilt subtree has nothing
    /// focusable left at the position focus had, the nearest enabled
    /// focusable at or above the innermost surviving root, else the first Tab
    /// stop of the scope. Only while a root is still active: a rebuild whose
    /// subtree also went dormant (a popover closing) leaves focus to the path
    /// that parked it, and drags none back.
    pub(super) fn rebuild_focus_fallback(&self, roots: &[WidgetId]) -> Option<WidgetId> {
        let root = roots.iter().copied().find(|&id| self.arena.is_active(id))?;
        self.enabled_focusable_at_or_above(root)
            .or_else(|| self.first_tab_stop_in_scope())
    }

    /// A focused node that has become disabled, itself or through an
    /// ancestor, hands focus to the next enabled Tab stop of its scope, or the
    /// previous one when there is none after it, without wrapping; a disabled
    /// ancestor is stepped over as a whole, to the stop after it. (Qt wraps
    /// round its focus chain instead, and clears focus when an ancestor is
    /// disabled.) The node that loses focus is sent `FocusLost` although it
    /// is disabled. Run at the end of every layout pass, so a handler that
    /// disables the control it is running in is repaired before the next key
    /// press. With nowhere to go, focus stays where it is. It does not come
    /// back when the control is enabled again. The move is `Programmatic`:
    /// it is not the same widget restored, and a text field it lands on keeps
    /// its caret.
    ///
    /// A focus that stays is recorded in `disabled_focus_stays`, so that
    /// `needs_structural_layout` does not ask for this pass on every event
    /// while nothing can come of it. Something can once an enabled state
    /// changes: the binding flush clears the record whenever an
    /// `enabled_when` binding fires, and so does the start of this function,
    /// at every pass.
    pub(super) fn move_focus_off_disabled(&mut self, ops: &mut dyn crate::window::WindowOps) {
        self.disabled_focus_stays = None;
        let Some(focused) = self.focused else {
            return;
        };
        if !self.arena.is_active(focused) || self.arena.is_enabled(focused) {
            return;
        }
        match self.enabled_stop_around(focused) {
            Some(next) => {
                self.focus_with_origin_ops(next, crate::focus::FocusOrigin::Programmatic, ops);
            }
            None => self.disabled_focus_stays = Some(focused),
        }
    }
}
