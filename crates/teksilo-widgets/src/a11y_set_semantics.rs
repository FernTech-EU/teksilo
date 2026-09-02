// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Test helpers for "item N of M", asked the way a platform adapter asks it.
//!
//! A collection widget publishes two halves of one sentence, and AccessKit
//! splits them across two nodes:
//!
//! * `position_in_set` on the **item** — zero-based, so the first item is 0 and
//!   the Windows and AT-SPI adapters add the 1 back before speaking.
//! * `size_of_set` on the **container** — unlike ARIA's per-item
//!   `aria-setsize`. `accesskit_consumer::Node::size_of_set_from_container`
//!   (`node.rs:629-641`) starts its walk at the item's *parent*, so a value
//!   written on the item is read by nobody, on any platform.
//!
//! Asserting the raw node therefore proves very little: a widget can set
//! `size_of_set` on every item, pass a node-level test, and still announce no
//! total anywhere. Every one of Teksilo's fifteen writes did exactly that. So
//! these helpers ask through the consumer, which runs the same walk and the
//! same filter each adapter runs.

#![cfg(test)]

use teksilo_core::accesskit::{NodeId, TreeUpdate};

/// What an adapter would announce for one item: its 1-based position and the
/// set size it resolves by walking up to a container.
///
/// `None` for either half means the adapter has nothing to say for it. A
/// missing size is the common failure and the reason this helper exists.
pub(crate) fn announced_set_position(
    update: &TreeUpdate,
    item: NodeId,
) -> (Option<usize>, Option<usize>) {
    let consumer = accesskit_consumer::Tree::new(update.clone(), false);
    let state = consumer.state();
    // `FullNodeId` is opaque, so the item is located by walking the tree and
    // comparing the local id `NodeRef::locate` hands back. There is one tree
    // here, so the walk is short and the comparison unambiguous.
    let mut found = None;
    let mut stack = vec![state.root()];
    while let Some(node) = stack.pop() {
        if node.locate().0 == item {
            found = Some(node);
            break;
        }
        for child in node.children() {
            stack.push(child);
        }
    }
    let Some(node) = found else {
        return (None, None);
    };
    // The +1 is what `accesskit_windows` and `accesskit_atspi_common` both do,
    // so this is the number a screen reader actually says.
    let position = node.position_in_set().map(|p| p + 1);
    let size = node.size_of_set_from_container(&accesskit_consumer::common_filter);
    (position, size)
}

/// Assert that `item` announces "position of size", by the adapters' own
/// resolution rules.
#[track_caller]
pub(crate) fn assert_announces(
    update: &TreeUpdate,
    item: NodeId,
    position: usize,
    size: usize,
    what: &str,
) {
    let got = announced_set_position(update, item);
    assert_eq!(
        got,
        (Some(position), Some(size)),
        "{what}: expected to announce {position} of {size}, but an adapter \
         resolves {got:?}. A missing size means no container on the ancestor \
         chain carries `size_of_set` — it is a container property in AccessKit, \
         unlike ARIA's per-item `aria-setsize`."
    );
}
