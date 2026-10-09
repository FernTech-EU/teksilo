// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The tree's redraw waker: what content updated off the UI thread wakes the
//! tree's window through.

use std::sync::Arc;

use teksilo_canvas::wake::{CountingWaker, RedrawWaker};

use super::WidgetTree;

fn thin(waker: &Arc<dyn RedrawWaker>) -> *const () {
    Arc::as_ptr(waker) as *const ()
}

#[test]
fn a_tree_starts_with_no_waker_and_set_redraw_waker_replaces_and_clears() {
    let mut tree = WidgetTree::new();
    assert!(
        tree.redraw_waker().is_none(),
        "a headless tree wakes nobody"
    );

    let first: Arc<dyn RedrawWaker> = Arc::new(CountingWaker::new());
    tree.set_redraw_waker(Some(first.clone()));
    assert_eq!(tree.redraw_waker().map(thin), Some(thin(&first)));

    let second: Arc<dyn RedrawWaker> = Arc::new(CountingWaker::new());
    tree.set_redraw_waker(Some(second.clone()));
    assert_eq!(tree.redraw_waker().map(thin), Some(thin(&second)));

    tree.set_redraw_waker(None);
    assert!(tree.redraw_waker().is_none());
}
