// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What the accessibility walk costs for a list-heavy document, against the
//! same text as plain paragraphs (the M1 measurement of the pending note
//! `a11y-walk-cost-lists.md`).
//!
//! The structural test runs with the suite: it counts the nodes a document
//! emits, so an extra node per list item turns it red. The timing test is
//! `#[ignore]`d and reports only; run it in release:
//!
//! ```text
//! cargo test -p teksilo-widgets --release a11y_walk_cost -- --ignored --nocapture
//! ```
//!
//! Decision rule, written before the run: the per-keystroke total (refresh,
//! deliver, and the platform adapter's text diffing) must stay under 4 ms on
//! the chapter-sized document and under 16 ms on 20,000 list items. Under both
//! means no action, whatever the list to paragraph ratio. The ratio and the
//! scaling from 2,000 to 20,000 blocks are diagnostics. The thresholds are a
//! proposal the maintainer may move.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use accesskit_consumer::{NodeRef, Tree, TreeChangeHandler, common_filter};
use teksilo_canvas::SizeProposal;
use teksilo_core::accesskit::TreeUpdate;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::TextDocument;

use super::{EditorHandle, RichTextEditor};

fn viewport() -> SizeProposal {
    SizeProposal::exact(480.0, 400.0)
}

#[derive(Clone, Copy, Debug)]
enum Shape {
    Paragraphs,
    Lists,
    Chapter,
}

fn markdown(shape: Shape, n: usize) -> String {
    let mut md = String::new();
    match shape {
        Shape::Paragraphs => {
            for i in 0..n {
                md.push_str(&format!("Item number {i} of the sample text.\n\n"));
            }
        }
        Shape::Lists => {
            // Top-level item, then one nested item, alternating: half nested.
            for i in 0..n {
                if i % 2 == 0 {
                    md.push_str(&format!("- Item number {i} of the sample text.\n"));
                } else {
                    md.push_str(&format!("  - Item number {i} of the sample text.\n"));
                }
            }
        }
        Shape::Chapter => {
            // About 200 paragraphs, 10 lists of 5 to 15 items, two headings.
            md.push_str("# Chapter\n\n");
            let mut item = 0;
            for p in 0..200 {
                md.push_str(&format!(
                    "Paragraph {p} of the chapter, with some words in it.\n\n"
                ));
                if p == 100 {
                    md.push_str("## Middle\n\n");
                }
                if p % 20 == 19 {
                    for _ in 0..(5 + (p / 20) % 11) {
                        md.push_str(&format!("- List item {item} of the chapter.\n"));
                        item += 1;
                    }
                    md.push('\n');
                }
            }
        }
    }
    md
}

struct Fixture {
    tree: WidgetTree,
    handle: EditorHandle,
    doc: TextDocument,
}

fn fixture(md: &str) -> Fixture {
    let doc = TextDocument::new();
    doc.set_markdown(md).unwrap().wait().unwrap();
    let editor = RichTextEditor::editor(doc.clone());
    let handle = editor.handle();
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(editor);
    tree.layout(viewport());
    let _ = tree.render();
    tree.focus(tree.first_focusable_descendant(id).unwrap_or(id));
    Fixture { tree, handle, doc }
}

fn role_counts(update: &TreeUpdate) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for (_, node) in &update.nodes {
        *counts.entry(format!("{:?}", node.role())).or_insert(0) += 1;
    }
    counts
}

fn count(counts: &BTreeMap<String, usize>, role: &str) -> usize {
    counts.get(role).copied().unwrap_or(0)
}

/// The node shape the walk documents: per list item one `ListItem` and one
/// text-container `Label`, one `List` per run of items of one list, and a plain
/// paragraph with neither. An extra node per item turns this red.
#[test]
fn a_list_item_emits_one_item_one_label_and_a_paragraph_neither() {
    let n = 40;
    let mut p = fixture(&markdown(Shape::Paragraphs, n));
    let mut l = fixture(&markdown(Shape::Lists, n));
    let pc = role_counts(&p.tree.sync_accessibility());
    let lc = role_counts(&l.tree.sync_accessibility());
    assert_eq!(count(&pc, "ListItem"), 0);
    assert_eq!(count(&pc, "List"), 0);
    assert_eq!(count(&pc, "Label"), 0, "a plain paragraph emits no Label");
    assert_eq!(count(&lc, "ListItem"), n);
    assert_eq!(count(&lc, "Label"), n, "one text container per item");
    assert_eq!(
        count(&lc, "List"),
        1 + n / 2,
        "one outer list, one nested run per pair"
    );
}

/// What `accesskit_atspi_common`'s `node_updated` does per updated node that
/// supports text ranges: the old and new document text, and the prefix and
/// suffix diff between them (adapter.rs `emit_text_change_if_needed_parent`).
#[derive(Default)]
struct AdapterDiff {
    checked: std::collections::HashSet<accesskit_consumer::FullNodeId>,
    diffed_bytes: usize,
}

impl AdapterDiff {
    fn parent_diff(&mut self, old: &NodeRef<'_>, new: &NodeRef<'_>) {
        if !new.supports_text_ranges() || !old.supports_text_ranges() {
            return;
        }
        if !self.checked.insert(new.id()) {
            return;
        }
        let old_text = old.document_range().text();
        let new_text = new.document_range().text();
        let mut prefix = 0;
        let mut a = old_text.chars();
        let mut b = new_text.chars();
        while let (Some(x), Some(y)) = (a.next(), b.next()) {
            if x != y {
                break;
            }
            prefix += x.len_utf8();
        }
        let suffix: usize = old_text[prefix..]
            .chars()
            .rev()
            .zip(new_text[prefix..].chars().rev())
            .take_while(|(x, y)| x == y)
            .map(|(c, _)| c.len_utf8())
            .sum();
        self.diffed_bytes += old_text.len() + new_text.len() - 2 * prefix - 2 * suffix;
    }
}

impl TreeChangeHandler for AdapterDiff {
    fn node_added(&mut self, _node: &NodeRef<'_>) {}
    fn node_updated(&mut self, old: &NodeRef<'_>, new: &NodeRef<'_>) {
        if matches!(
            new.role(),
            teksilo_core::accesskit::Role::TextRun
                | teksilo_core::accesskit::Role::GenericContainer
        ) {
            if let (Some(op), Some(np)) = (
                old.filtered_parent(&common_filter),
                new.filtered_parent(&common_filter),
            ) {
                self.parent_diff(&op, &np);
            }
        } else {
            self.parent_diff(old, new);
        }
    }
    fn focus_moved(&mut self, _o: Option<&NodeRef<'_>>, _n: Option<&NodeRef<'_>>) {}
    fn node_removed(&mut self, _node: &NodeRef<'_>) {}
}

/// Does nothing: the cost of the consumer's own update.
struct Nop;
impl TreeChangeHandler for Nop {
    fn node_added(&mut self, _node: &NodeRef<'_>) {}
    fn node_updated(&mut self, _o: &NodeRef<'_>, _n: &NodeRef<'_>) {}
    fn focus_moved(&mut self, _o: Option<&NodeRef<'_>>, _n: Option<&NodeRef<'_>>) {}
    fn node_removed(&mut self, _node: &NodeRef<'_>) {}
}

fn median_max(mut v: Vec<Duration>) -> (f64, f64) {
    v.sort();
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    (ms(v[v.len() / 2]), ms(*v.last().unwrap()))
}

fn settle(f: &mut Fixture) {
    f.tree.request_frame();
    f.tree.tick_animations(Duration::from_millis(16));
    f.tree.layout(viewport());
    let _ = f.tree.render();
}

fn measure(label: &str, shape: Shape, n: usize, caret_needle: &str) {
    let md = markdown(shape, n);
    let chars = md.chars().count();
    let t = Instant::now();
    let mut f = fixture(&md);
    let build = t.elapsed();
    let first = f.tree.sync_accessibility();
    let counts = role_counts(&first);
    let nodes = first.nodes.len();
    println!(
        "\n== {label}: {n} blocks, {chars} md chars, build+first layout {:.0} ms",
        build.as_secs_f64() * 1000.0
    );
    println!(
        "   nodes {nodes} ({:.3}/char) ListItem {} List {} Label {} TextRun {}",
        nodes as f64 / chars as f64,
        count(&counts, "ListItem"),
        count(&counts, "List"),
        count(&counts, "Label"),
        count(&counts, "TextRun")
    );

    // Cold path: a forced full walk and delivery.
    let (mut refresh, mut deliver) = (vec![], vec![]);
    for i in 0..22 {
        f.tree.request_accessibility_update();
        let t = Instant::now();
        f.tree.refresh_accessibility();
        let r = t.elapsed();
        let t = Instant::now();
        let _ = f.tree.deliver_accessibility(true);
        let d = t.elapsed();
        if i >= 2 {
            refresh.push(r);
            deliver.push(d);
        }
    }
    let (rm, rx) = median_max(refresh);
    let (dm, dx) = median_max(deliver);
    println!(
        "   cold   refresh median {rm:.2} max {rx:.2} ms | deliver median {dm:.2} max {dx:.2} ms"
    );

    // Per keystroke: caret in the needle, type one character at a time.
    let text = f.doc.to_plain_text().unwrap();
    let byte = text.find(caret_needle).expect("needle");
    let at = text[..byte].chars().count() + 2;
    f.handle.select_range(at, at);
    settle(&mut f);
    let mut consumer = Tree::new(f.tree.sync_accessibility(), true);
    let mut diff_consumer = Tree::new(f.tree.sync_accessibility(), true);
    let _ = f.tree.deliver_accessibility(true);
    let (mut lay, mut refr, mut del, mut nop_t, mut diff_t) =
        (vec![], vec![], vec![], vec![], vec![]);
    let (mut walked_by_typing, mut bytes, mut upd_nodes) = (0, 0, 0);
    let mut diffed = 0;
    for i in 0..22 {
        let editor = f.tree.focused().expect("a focused widget");
        f.tree
            .type_text(editor, &char::from(b'a' + (i % 26) as u8).to_string());
        let g0 = f.tree.a11y_walk_generation();
        let t = Instant::now();
        f.tree.request_frame();
        f.tree.tick_animations(Duration::from_millis(16));
        f.tree.layout(viewport());
        let _ = f.tree.render();
        let l = t.elapsed();
        let g1 = f.tree.a11y_walk_generation();
        let t = Instant::now();
        f.tree.refresh_accessibility();
        let r = t.elapsed();
        let g2 = f.tree.a11y_walk_generation();
        let t = Instant::now();
        let update = f.tree.deliver_accessibility(true);
        let d = t.elapsed();
        if g1 > g0 || g2 > g1 {
            walked_by_typing += 1;
        }
        if i >= 2 {
            let t = Instant::now();
            consumer.update_and_process_changes(update.clone(), &mut Nop);
            nop_t.push(t.elapsed());
            let mut diff = AdapterDiff::default();
            let t = Instant::now();
            diff_consumer.update_and_process_changes(update.clone(), &mut diff);
            diff_t.push(t.elapsed());
            diffed += diff.diffed_bytes;
            lay.push(l);
            refr.push(r);
            del.push(d);
            upd_nodes = update.nodes.len();
            bytes = format!("{update:?}").len();
        } else {
            consumer.update_and_process_changes(update.clone(), &mut Nop);
            diff_consumer.update_and_process_changes(update, &mut AdapterDiff::default());
        }
    }
    let (lm, lx) = median_max(lay);
    let (rm, rx) = median_max(refr);
    let (dm, dx) = median_max(del);
    let (nm, nx) = median_max(nop_t);
    let (fm, fx) = median_max(diff_t);
    println!(
        "   keystroke: {walked_by_typing}/22 iterations walked on their own (frame or refresh)"
    );
    println!("   keystroke layout+render median {lm:.2} max {lx:.2} ms (reported apart)");
    println!(
        "   keystroke refresh median {rm:.2} max {rx:.2} ms | deliver median {dm:.2} max {dx:.2} ms"
    );
    println!(
        "   adapter side: no-op consumer update median {nm:.2} max {nx:.2} ms | with text diffing median {fm:.2} max {fx:.2} ms (diff bytes {diffed})"
    );
    println!("   delivered update: {upd_nodes} nodes, ~{bytes} bytes of Debug text");
    println!(
        "   TOTAL per keystroke (refresh+deliver+diffing consumer), median {:.2} ms",
        rm + dm + fm
    );
}

#[test]
#[ignore = "a measurement: run in release with --ignored --nocapture"]
fn a11y_walk_cost() {
    measure("chapter", Shape::Chapter, 200, "List item 3 ");
    for n in [2_000, 20_000] {
        measure(
            "paragraphs",
            Shape::Paragraphs,
            n,
            &format!("number {} of", n / 2),
        );
        measure("lists", Shape::Lists, n, &format!("number {} of", n / 2));
    }
}
