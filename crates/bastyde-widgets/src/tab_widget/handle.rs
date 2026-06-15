// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`TabHandle`] — the runtime entity that lives in a tab list.
//!
//! Carries a stable [`TabId`], its presentation [`TabInfo`], a
//! `kind` discriminator that selects the registered content
//! factory, and an `Rc<dyn Any>` payload holding the heavy state
//! (the document, the image, the page) the factory consumes.
//!
//! Heavy state lives **here**, in the handle, not in the content
//! widget. Reorders / model rebuilds destroy and recreate widgets
//! freely; the handle's payload is stable and the content factory
//! produces a fresh view over it.

use std::any::Any;
use std::rc::Rc;

use super::id::TabId;
use super::info::TabInfo;

/// Sentinel `kind` reserved for static tabs accumulated via
/// [`TabWidget::static_tab`](crate::tab_widget::TabWidget::static_tab).
/// Application-level `kind` strings must not collide with this
/// value — the framework panics with a clear message at registration
/// if [`dynamic_tab`](crate::tab_widget::TabWidget::dynamic_tab) is
/// called with this name.
pub const STATIC_KIND: &str = "__static__";

/// One tab's identity, presentation, and state pointer.
///
/// `Clone` is cheap: `TabInfo` is shallow (the icon is an
/// `Rc<dyn Fn() -> IconWidget>` factory) and `payload` is an
/// `Rc<dyn Any>`.
#[derive(Clone)]
pub struct TabHandle {
    pub id: TabId,
    pub info: TabInfo,
    pub kind: &'static str,
    pub payload: Rc<dyn Any>,
}

impl std::fmt::Debug for TabHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabHandle")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field("info", &self.info)
            .field("payload_type", &(*self.payload).type_id())
            .finish()
    }
}

impl TabHandle {
    /// Construct a handle for the dynamic-tab path. The `kind`
    /// must match a
    /// [`dynamic_tab::<S>`](crate::tab_widget::TabWidget::dynamic_tab)
    /// registration on the [`TabWidget`](crate::tab_widget::TabWidget)
    /// where this handle lands; the framework downcasts
    /// `payload` to `S` before calling the registered factory and
    /// panics with a clear message on type mismatch.
    pub fn dynamic<S: Any + 'static>(
        id: TabId,
        kind: &'static str,
        info: TabInfo,
        state: S,
    ) -> Self {
        assert!(
            kind != STATIC_KIND,
            "tab kind '{}' is reserved for static tabs; pick a different identifier",
            STATIC_KIND
        );
        Self {
            id,
            info,
            kind,
            payload: Rc::new(state),
        }
    }

    /// Construct a handle for the dynamic-tab path with a
    /// pre-built `Rc<dyn Any>` payload — useful when several
    /// handles share the same underlying state object.
    pub fn dynamic_shared(
        id: TabId,
        kind: &'static str,
        info: TabInfo,
        payload: Rc<dyn Any>,
    ) -> Self {
        assert!(
            kind != STATIC_KIND,
            "tab kind '{}' is reserved for static tabs; pick a different identifier",
            STATIC_KIND
        );
        Self {
            id,
            info,
            kind,
            payload,
        }
    }

    /// Construct a static handle (used internally by
    /// [`TabWidget::static_tab`](crate::tab_widget::TabWidget::static_tab)).
    /// The `kind` is the [`STATIC_KIND`] sentinel; the payload is
    /// the unit type. Apps should not call this directly — use
    /// the `static_tab` builder method.
    pub(crate) fn static_handle(id: TabId, info: TabInfo) -> Self {
        Self {
            id,
            info,
            kind: STATIC_KIND,
            payload: Rc::new(()),
        }
    }
}
