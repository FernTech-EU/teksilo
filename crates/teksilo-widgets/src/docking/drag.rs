// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Drag-and-drop plumbing for [`DockingLayout`](super::DockingLayout): the two
//! typed payloads and the helpers that extract them. The five split/stack drop
//! zones over a pane are the reusable [`DropTarget`](crate::DropTarget) (see
//! `DockPanePane` in [`panel`](super::panel)); the actual mutations are routed by
//! the panel / tab layer through [`DockingModel`].

use std::any::Any;

use teksilo_core::DragPayload;

use crate::tab_widget::{TabBarDragData, TabHandle};

use super::DockingModel;
use super::model::{DockLayoutId, DockTabId, DockWidgetId};

/// Payload for dragging a single dock widget (promote / split / stack / move).
#[derive(Debug, Clone, Copy)]
pub(crate) struct DockDragData {
    pub dock_id: DockWidgetId,
    /// The layout the dock was dragged from.
    pub layout: DockLayoutId,
}

/// Payload for dragging a whole tab (its arrangement + every dock) to another
/// side.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DockTabDragData {
    pub tab_id: DockTabId,
    pub source_side: super::geometry::DockSide,
    /// The layout the tab was dragged from.
    pub layout: DockLayoutId,
}

/// What a side's in-strip `TabWidget` carries for each of its tabs, and so
/// what a tab dragged from the strip brings in its
/// `TabBarDragData<TabHandle>`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DockTabPayload {
    pub tab_id: DockTabId,
    /// The layout the tab belongs to.
    pub layout: DockLayoutId,
}

impl DockTabPayload {
    /// The payload of a dock tab handle from `model`'s layout.
    pub(crate) fn of(handle: &TabHandle, model: &DockingModel) -> Option<DockTabId> {
        let p = (handle.payload.as_ref() as &dyn Any).downcast_ref::<DockTabPayload>()?;
        (p.layout == model.layout_id()).then_some(p.tab_id)
    }
}

/// Extract a dock **tab** id from a drag payload — recognising BOTH dock tab
/// drag sources: an activity-rail item (`DockTabDragData`) and a tab dragged
/// from the in-strip `TabWidget` (`TabBarDragData<TabHandle>`, whose item
/// carries a [`DockTabPayload`]). Every dock drop site uses this so a tab
/// drops the same way wherever it lands.
///
/// `None` for a tab from another layout: tab ids repeat across layouts, so
/// its id would name one of `model`'s own tabs.
pub(crate) fn dropped_dock_tab(p: &DragPayload, model: &DockingModel) -> Option<DockTabId> {
    if let Some(d) = p.get_typed::<DockTabDragData>() {
        return (d.layout == model.layout_id()).then_some(d.tab_id);
    }
    if let Some(tbd) = p.get_typed::<TabBarDragData<TabHandle>>() {
        return DockTabPayload::of(tbd.item.as_ref()?, model);
    }
    None
}

/// Extract a single **dock widget** id from a drag payload (a split-pane
/// header drag); `None` for a dock from another layout.
pub(crate) fn dropped_dock_widget(p: &DragPayload, model: &DockingModel) -> Option<DockWidgetId> {
    p.get_typed::<DockDragData>()
        .filter(|d| d.layout == model.layout_id())
        .map(|d| d.dock_id)
}
