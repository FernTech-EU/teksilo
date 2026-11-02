// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Focus regions: the large parts of a window (side panels, the centre, a
//! bottom panel, a status bar) that F6 and Shift+F6 move focus between.
//!
//! A node marked with [`focus_region`](crate::widget_builder::WidgetBuilder::focus_region)
//! is a region. The rules, which `docs/events-and-gestures.md` ("Focus
//! regions") states for a reader:
//!
//! - **Which regions.** The active regions of the window's content, or, when
//!   the command comes from inside an in-window modal (or with nothing
//!   focused while one is open), of that modal's content, in the order Tab
//!   reaches their own focusables (whatever the layout direction). A region
//!   inside a non-modal overlay (a popover, a menu) is never a stop. A region
//!   hidden from assistive technology is still a stop, as its controls are
//!   still Tab stops; its name is announced all the same.
//! - **A region's own focusables** are the Tab stops of its subtree, in Tab
//!   order, that are not inside a region nested in it. A region with none is
//!   not a stop, and a nested region is a stop of its own. Taking every Tab
//!   stop of the subtree instead would send focus from an outer region into
//!   the nested one whose own first stop it is, and Shift+F6 from there back
//!   to the outer region, forever.
//! - **The current region** is the innermost region on the walk from the
//!   source of the command (the focused widget), crossing a popover to the
//!   widget it is attached to, as an intent's walk does. With nothing
//!   focused there is none.
//! - **Next and previous** wrap. With no current region, next goes to the first
//!   region and previous to the last; with one region, focus stays and its
//!   name is announced again.
//! - **Landing** goes, in this order, to:
//!   1. the region's remembered focus, when it is still one of the region's
//!      own focusables. Each region remembers the last focus that was one of
//!      its own (`set_focused`);
//!   2. else the first of its own focusables under the subtree the region
//!      node's own `Widget::focus_region_landing` (a hidden hook, read here
//!      only) names, when that subtree holds one: a dock side names its panel, so
//!      F6 lands in the panel's content and not on the activity rail drawn
//!      before it. A hint whose subtree has no Tab stop (a collapsed panel)
//!      is passed over;
//!   3. else its first own focusable.
//!
//!   A divider (a node whose role is `Role::Splitter`: a side panel's resize
//!   handle, a splitter's gutter) is passed over at every step while the
//!   region has any other focusable, though Tab still stops on it: F6 goes to
//!   the region's content, and a resize handle drawn on a side's inner edge
//!   comes first in its Tab order. So a divider the user focused on purpose
//!   (to resize with the arrow keys) is remembered but is not where F6 brings
//!   them back: F6 out and back lands as if nothing were remembered. A region
//!   whose only focusables are dividers lands on one.
//!
//!   Tab order is not changed by any of this: the landing only says where F6
//!   lands, and Tab still meets the rail, the panel and the handle in the
//!   order they are drawn in. Nor is a window's or a modal's first focus,
//!   which reads `initial_focus_hint` and never the landing.

use crate::widget_id::WidgetId;

use super::WidgetTree;

impl WidgetTree {
    /// Whether `id` is an active node marked as a focus region.
    fn is_region(&self, id: WidgetId) -> bool {
        self.arena.is_active(id)
            && self
                .arena
                .get(id)
                .is_some_and(|node| node.focus_region.is_some())
    }

    /// The innermost region `id` is inside, `id` included, by its own parents
    /// alone (an overlay's content is in no region of the page).
    pub(crate) fn innermost_region_of(&self, id: WidgetId) -> Option<WidgetId> {
        let mut current = Some(id);
        while let Some(at) = current {
            if self.is_region(at) {
                return Some(at);
            }
            current = self.arena.parent(at);
        }
        None
    }

    /// The regions F6 sent from `source` cycles through, in Tab order: those
    /// of the open modal `source` is in (with no source, the topmost one),
    /// else those of every root that is not an open overlay's content.
    ///
    /// Judged from the source, not from the topmost modal: a command palette
    /// asks for the editor it was opened over, and lists F6 when the page has
    /// regions.
    pub(crate) fn regions_in_scope(&self, source: Option<WidgetId>) -> Vec<WidgetId> {
        // Most windows mark no region: answer without a walk, since command
        // availability asks this on every pass.
        if !self.arena.focus_regions_seen {
            return Vec::new();
        }
        let modal = match source {
            Some(source) => self.intent_walk(source, &[]).2,
            None => self
                .overlay_manager
                .topmost_modal()
                .map(|modal| modal.content_id),
        };
        let roots: Vec<WidgetId> = match modal {
            Some(content) => vec![content],
            None => self
                .arena
                .roots()
                .iter()
                .copied()
                .filter(|&root| self.overlay_manager.find_by_content(root).is_none())
                .collect(),
        };
        let mut out = Vec::new();
        for &root in &roots {
            self.collect_regions(root, &mut out);
        }
        // In traversal order, the order Tab first reaches each region in
        // (scoped `tab_index`, traversal scopes), not the arena's child
        // order: F6 must go round the regions as Tab does. A nested region
        // ties with the one around it and follows it (the sort is stable and
        // the walk put the outer one first); a region with no Tab stop at all
        // is never a stop and goes last.
        let tab_order: Vec<WidgetId> = roots
            .iter()
            .flat_map(|&root| self.tab_stops_in_order(root))
            .collect();
        let first_stop = |region: WidgetId| {
            tab_order
                .iter()
                .position(|&id| self.is_descendant_of(id, region))
                .unwrap_or(usize::MAX)
        };
        out.sort_by_cached_key(|&region| first_stop(region));
        out
    }

    fn collect_regions(&self, id: WidgetId, out: &mut Vec<WidgetId>) {
        if !self.arena.is_active(id) {
            return;
        }
        if self.is_region(id) {
            out.push(id);
        }
        for &child in self.arena.children(id) {
            self.collect_regions(child, out);
        }
    }

    /// Whether `id` is a divider, which F6 lands on only when its region has
    /// nothing else to land on (see the module doc).
    fn is_divider(&self, id: WidgetId) -> bool {
        self.build_overridden_builder(id).role() == accesskit::Role::Splitter
    }

    /// The region's own focusables, in Tab order: its Tab stops not inside a
    /// region nested in it.
    pub(crate) fn region_own_focusables(&self, region: WidgetId) -> Vec<WidgetId> {
        self.tab_stops_in_order(region)
            .into_iter()
            .filter(|&id| self.innermost_region_of(id) == Some(region))
            .collect()
    }

    /// The region the command sent from `source` starts in: the innermost
    /// region on its intent walk, which crosses a popover to the widget it is
    /// attached to and stops at an open modal's content.
    fn region_of_source(&self, source: WidgetId) -> Option<WidgetId> {
        self.intent_walk(source, &[])
            .0
            .into_iter()
            .find(|&id| self.is_region(id))
    }

    /// Where F6 (`forward`) or Shift+F6 sent from `source` lands: the region
    /// and the widget focus goes to. `None` when no region in scope has a
    /// focusable of its own, which leaves the command to whatever else takes
    /// it.
    pub(crate) fn region_cycle_target_from(
        &self,
        source: Option<WidgetId>,
        forward: bool,
    ) -> Option<(WidgetId, WidgetId)> {
        let regions = self.regions_in_scope(source);
        if regions.is_empty() {
            return None;
        }
        let n = regions.len();
        // With nothing focused there is no current region: the source is then
        // only where an intent starts (the content root, a modal's content).
        let current = source
            .filter(|_| self.focused.is_some())
            .and_then(|source| self.region_of_source(source))
            .and_then(|region| regions.iter().position(|&r| r == region));
        // The order regions are tried in: after the current one, wrapping and
        // ending on it; with none, from the first (forward) or the last.
        let order: Vec<usize> = match current {
            Some(at) => (1..=n)
                .map(|k| {
                    if forward {
                        (at + k) % n
                    } else {
                        (at + n - k) % n
                    }
                })
                .collect(),
            None if forward => (0..n).collect(),
            None => (0..n).rev().collect(),
        };
        for index in order {
            let region = regions[index];
            let own = self.region_own_focusables(region);
            let Some(&first) = own.first() else {
                continue;
            };
            // Focus already in the one region with a stop: it stays.
            if Some(index) == current
                && let Some(focused) = self.focused
                && self.region_of_source(focused) == Some(region)
            {
                return Some((region, focused));
            }
            let remembered = self
                .arena
                .get(region)
                .and_then(|node| node.focus_region_last_focus)
                .filter(|id| own.contains(id));
            let control = |id: &WidgetId| !self.is_divider(*id);
            let landing = remembered
                .filter(control)
                .or_else(|| self.region_hinted_landing(region, &own))
                .or_else(|| own.iter().copied().find(control))
                .unwrap_or(first);
            return Some((region, landing));
        }
        None
    }

    /// The first of the region's own focusables (`own`, in Tab order) inside
    /// the subtree the region node's own
    /// [`focus_region_landing`](crate::widget::Widget::focus_region_landing)
    /// names, passing over a divider as the other landings do. `None` when
    /// the region names no hint, the hint is not an active node inside it, or
    /// nothing under it is one of the region's Tab stops (a panel collapsed
    /// to nothing), which leaves the landing to the region's first control.
    fn region_hinted_landing(&self, region: WidgetId, own: &[WidgetId]) -> Option<WidgetId> {
        let hint = self.arena.get(region)?.widget.focus_region_landing()?;
        if hint == region || !self.arena.is_active(hint) || !self.is_descendant_of(hint, region) {
            return None;
        }
        let under: Vec<WidgetId> = own
            .iter()
            .copied()
            .filter(|&id| self.is_descendant_of(id, hint))
            .collect();
        under
            .iter()
            .copied()
            .find(|&id| !self.is_divider(id))
            .or_else(|| under.first().copied())
    }

    /// Move focus as F6 or Shift+F6 does from `source`, and announce the
    /// region's name. Returns whether there was a region to go to.
    pub(crate) fn cycle_focus_region(
        &mut self,
        source: Option<WidgetId>,
        forward: bool,
        ops: &mut dyn crate::window::WindowOps,
    ) -> bool {
        let Some((region, landing)) = self.region_cycle_target_from(source, forward) else {
            return false;
        };
        if self.focused != Some(landing) {
            self.focus_with_origin_ops(landing, crate::focus::FocusOrigin::Keyboard, ops);
        }
        let name = self
            .arena
            .get(region)
            .and_then(|node| node.focus_region.as_ref())
            .map(|name| name.get())
            .unwrap_or_default();
        if !name.trim().is_empty() {
            self.announce(name);
        }
        true
    }

    /// Record `focused` as the remembered focus of its innermost region.
    pub(crate) fn remember_region_focus(&mut self, focused: WidgetId) {
        if let Some(region) = self.innermost_region_of(focused)
            && let Some(node) = self.arena.get_mut(region)
        {
            node.focus_region_last_focus = Some(focused);
        }
    }
}
