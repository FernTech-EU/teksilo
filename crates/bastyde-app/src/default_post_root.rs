//! App-wide default post-root wrapper.
//!
//! Stored in `app_state` by extensions (notably the debug inspector)
//! that want to splice a wrapper widget around every window's root
//! without per-window opt-in. [`WindowManager::create_window`] looks
//! this up after invoking the user's `root_builder`, before computing
//! modal focus targets.
//!
//! Per-window [`WindowConfig::post_root`] overrides this default for
//! that window only — useful when a particular window opts out of the
//! wrapper or wants a different one.

use std::rc::Rc;

use bastyde_core::widget_id::WidgetId;
use bastyde_core::widget_tree::WidgetTree;

/// Closure that runs after each window's `root_builder` returns, wrapping
/// the user's root widget. Stored in `app_state` so it is shared across
/// every window the app opens at runtime.
///
/// The closure may be invoked from many windows over the app's lifetime
/// — it receives `&mut WidgetTree` and the user's root id, and returns
/// the id of the (possibly wrapped) widget to use as the window's
/// effective root.
#[derive(Clone)]
pub struct DefaultPostRoot(pub Rc<dyn Fn(&mut WidgetTree, WidgetId) -> WidgetId>);

impl DefaultPostRoot {
    pub fn new(f: impl Fn(&mut WidgetTree, WidgetId) -> WidgetId + 'static) -> Self {
        Self(Rc::new(f))
    }
}

impl std::fmt::Debug for DefaultPostRoot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("DefaultPostRoot")
            .field(&"<closure>")
            .finish()
    }
}
