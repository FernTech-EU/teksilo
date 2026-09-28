// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The document's blocks and tables in reading order, quotations included.
//!
//! Shared by the table keys (Tab, Shift+Tab and Enter look their table up
//! here) and the paint (a selection's tables are found here), which both need
//! the tables standing in quotations as well as in the main text.

use std::collections::HashSet;

use teksilo_text::text_document::{FlowElement, TextDocument};

/// The document's blocks and tables in reading order, each quotation's own
/// flow in its place rather than as a frame: what a caret runs through,
/// however deep the quotations nest. A table's cells are its own and are not
/// listed.
///
/// `TextDocument::flow` lists the main text's own elements only, with a
/// quotation as one `Frame`: looked up there, a table in a quotation was not
/// found, and Tab, Shift+Tab and Enter in its cells did nothing. Walked with
/// a stack, as text-document walks a flow, so a quotation nested past any
/// depth costs no recursion, and a frame met twice is read once.
pub(super) struct ReadingOrder {
    stack: Vec<std::vec::IntoIter<FlowElement>>,
    seen: HashSet<usize>,
}

impl ReadingOrder {
    pub(super) fn of(document: &TextDocument) -> Self {
        Self {
            stack: vec![document.flow().into_iter()],
            seen: HashSet::new(),
        }
    }
}

impl Iterator for ReadingOrder {
    type Item = FlowElement;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let frame = self.stack.last_mut()?;
            match frame.next() {
                None => {
                    self.stack.pop();
                }
                Some(FlowElement::Frame(sub_frame)) => {
                    if self.seen.insert(sub_frame.id()) {
                        self.stack.push(sub_frame.flow().into_iter());
                    }
                }
                Some(element) => return Some(element),
            }
        }
    }
}
