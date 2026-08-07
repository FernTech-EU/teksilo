// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Drag-and-drop plumbing for [`DockingLayout`](super::DockingLayout): the two
//! typed payloads and the helpers that extract them. The five split/stack drop
//! zones over a pane are the reusable [`DropTarget`](crate::DropTarget) (see
//! `DockPanePane` in [`panel`](super::panel)); the actual mutations are routed by
//! the panel / tab layer through [`DockingModel`](super::DockingModel).

use teksilo_core::DragPayload;

use crate::tab_widget::{TabBarDragData, TabHandle};

use super::model::{DockTabId, DockWidgetId};

/// Payload for dragging a single dock widget (promote / split / stack / move).
#[derive(Debug, Clone, Copy)]
pub(crate) struct DockDragData {
    pub dock_id: DockWidgetId,
}

/// Payload for dragging a whole tab (its arrangement + every dock) to another
/// side.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DockTabDragData {
    pub tab_id: DockTabId,
    pub source_side: super::geometry::DockSide,
}

/// Extract a dock **tab** id from a drag payload — recognising BOTH dock tab
/// drag sources: an activity-rail item (`DockTabDragData`) and a tab dragged
/// from the in-strip `TabWidget` (`TabBarDragData<TabHandle>`, whose
/// `source_id` the dock builds straight from the `DockTabId`). Every dock drop
/// site uses this so a tab drops the same way wherever it lands.
pub(crate) fn dropped_dock_tab(p: &DragPayload) -> Option<DockTabId> {
    if let Some(d) = p.get_typed::<DockTabDragData>() {
        return Some(d.tab_id);
    }
    if let Some(tbd) = p.get_typed::<TabBarDragData<TabHandle>>() {
        return Some(DockTabId::from_raw(tbd.source_id.raw().get()));
    }
    None
}

/// Extract a single **dock widget** id from a drag payload (a split-pane
/// header drag).
pub(crate) fn dropped_dock_widget(p: &DragPayload) -> Option<DockWidgetId> {
    p.get_typed::<DockDragData>().map(|d| d.dock_id)
}
