use bastyde_canvas::{Size, SizeProposal};
use bastyde_core::widget::{LayoutContext, Widget};
use bastyde_core::widget_tree::WidgetTree;
use bastyde_i18n::lit;
use bastyde_tokens::{Alignment, HAlignment};

use crate::primitives::{
    Center, Expand, FixedSize, HStack, MinSize, Padding, Spacer, TextWidget, VStack, ZStack,
};

/// A leaf that always reports a fixed intrinsic size.
#[derive(Debug)]
struct FixedLeaf(f32, f32);
impl Widget for FixedLeaf {
    fn layout_response(
        &self,
        _proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        Size::new(self.0, self.1).into()
    }
}

#[test]
fn vstack_with_mixed_sizes_and_center_alignment() {
    let mut tree = WidgetTree::new();
    let a = tree.add(FixedLeaf(80.0, 30.0));
    let b = tree.add(FixedLeaf(120.0, 40.0));
    let c = tree.add(FixedLeaf(60.0, 20.0));
    let _stack = tree.add(
        VStack::new()
            .alignment(HAlignment::Center)
            .spacing(5.0)
            .add_child(a)
            .add_child(b)
            .add_child(c),
    );
    tree.layout(SizeProposal::exact(200.0, 300.0));

    // a: width 80, centered in 200 → x=60
    assert!((tree.bounds(a).x - 60.0).abs() < 0.01);
    assert!((tree.bounds(a).y - 0.0).abs() < 0.01);

    // b: width 120, centered in 200 → x=40, y=30+5=35
    assert!((tree.bounds(b).x - 40.0).abs() < 0.01);
    assert!((tree.bounds(b).y - 35.0).abs() < 0.01);

    // c: width 60, centered in 200 → x=70, y=35+40+5=80
    assert!((tree.bounds(c).x - 70.0).abs() < 0.01);
    assert!((tree.bounds(c).y - 80.0).abs() < 0.01);
}

#[test]
fn hstack_toolbar_pattern_with_spacer() {
    // [Back] --- [Edit] [Save]
    let mut tree = WidgetTree::new();
    let back = tree.add(FixedLeaf(60.0, 30.0));
    let spacer = tree.add(Spacer::new());
    let edit = tree.add(FixedLeaf(50.0, 30.0));
    let save = tree.add(FixedLeaf(50.0, 30.0));
    let _toolbar = tree.add(
        HStack::new()
            .spacing(8.0)
            .add_child(back)
            .add_child(spacer)
            .add_child(edit)
            .add_child(save),
    );
    tree.layout(SizeProposal::exact(400.0, 40.0));

    // back at x=0
    assert!((tree.bounds(back).x - 0.0).abs() < 0.01);
    // spacer takes remaining: 400 - 60 - 50 - 50 - 3*8 = 216
    // edit at x = 60 + 8 + 216 + 8 = 292
    assert!((tree.bounds(edit).x - 292.0).abs() < 0.01);
    // save at x = 292 + 50 + 8 = 350
    assert!((tree.bounds(save).x - 350.0).abs() < 0.01);
}

#[test]
fn zstack_per_child_alignment_overrides() {
    let mut tree = WidgetTree::new();
    let bg = tree.add(FixedLeaf(200.0, 100.0));
    let title = tree.add(FixedLeaf(80.0, 20.0));
    let button = tree.add(FixedLeaf(40.0, 30.0));
    let _stack = tree.add(
        ZStack::new()
            .alignment(Alignment::CENTER)
            .add_child(bg)
            .add_child(title)
            .add_child(button),
    );
    // title centered, button overridden to bottom-trailing
    tree.set_alignment(button, Alignment::BOTTOM_TRAILING);
    tree.layout(SizeProposal::exact(300.0, 200.0));

    let tb = tree.bounds(title);
    assert!((tb.x - 110.0).abs() < 0.01); // (300-80)/2
    assert!((tb.y - 90.0).abs() < 0.01); // (200-20)/2

    let bb = tree.bounds(button);
    assert!((bb.x - 260.0).abs() < 0.01); // 300-40
    assert!((bb.y - 170.0).abs() < 0.01); // 200-30
}

#[test]
fn nested_hstack_in_vstack() {
    let mut tree = WidgetTree::new();
    // Row 1: two fixed leaves
    let a = tree.add(FixedLeaf(60.0, 25.0));
    let b = tree.add(FixedLeaf(40.0, 25.0));
    let row1 = tree.add(HStack::new().spacing(5.0).add_child(a).add_child(b));
    // Row 2: single item
    let c = tree.add(FixedLeaf(80.0, 30.0));
    let _col = tree.add(
        VStack::new()
            .alignment(HAlignment::Center)
            .spacing(10.0)
            .add_child(row1)
            .add_child(c),
    );
    tree.layout(SizeProposal::exact(200.0, 200.0));

    // row1 intrinsic width = 60+5+40 = 105, centered in 200 → x=47.5
    assert!((tree.bounds(row1).x - 47.5).abs() < 0.01);
    // c at y = row1.height + 10 = 25+10 = 35, centered: x=(200-80)/2=60
    assert!((tree.bounds(c).x - 60.0).abs() < 0.01);
    assert!((tree.bounds(c).y - 35.0).abs() < 0.01);
}

#[test]
fn min_size_wrapping_small_widget() {
    let mut tree = WidgetTree::new();
    let small = tree.add(FixedLeaf(20.0, 10.0));
    let min = tree.add(MinSize::new(48.0, 48.0).child_id(small));
    let _stack = tree.add(HStack::new().add_child(min));
    tree.layout(SizeProposal::exact(200.0, 60.0));

    let mb = tree.bounds(min);
    assert!((mb.width - 48.0).abs() < 0.01);
    assert!((mb.height - 48.0).abs() < 0.01);
}

#[test]
fn expand_horizontal_in_hstack() {
    // [fixed(60)] [Expand::horizontal()] in 300px stack:
    //   - fixed wants 60, no flex
    //   - Expand wants 0 on horizontal (default zero-basis), flex=1
    //   - slack = 300 - 60 = 240, all to Expand
    let mut tree = WidgetTree::new();
    let fixed = tree.add(FixedLeaf(60.0, 30.0));
    let inner = tree.add(FixedLeaf(40.0, 20.0));
    let expanded = tree.add(Expand::horizontal().child_id(inner));
    let _stack = tree.add(HStack::new().add_child(fixed).add_child(expanded));
    tree.layout(SizeProposal::exact(300.0, 50.0));

    assert!((tree.bounds(fixed).width - 60.0).abs() < 0.01);
    let eb = tree.bounds(expanded);
    assert!(
        (eb.width - 240.0).abs() < 0.01,
        "Expand should claim leftover slack, got width={}",
        eb.width
    );
}

#[test]
fn center_widget() {
    let mut tree = WidgetTree::new();
    let child = tree.add(FixedLeaf(40.0, 20.0));
    let _center = tree.add(Center::new().child_id(child));
    tree.layout(SizeProposal::exact(200.0, 100.0));

    let cb = tree.bounds(child);
    assert!((cb.x - 80.0).abs() < 0.01);
    assert!((cb.y - 40.0).abs() < 0.01);
}

#[test]
fn fixed_size_in_hstack_resists_stretching() {
    let mut tree = WidgetTree::new();
    let a = tree.add(FixedLeaf(40.0, 20.0));
    let fixed = tree.add(FixedSize::new().child_id(a));
    let b = tree.add(FixedLeaf(60.0, 30.0));
    let _stack = tree.add(HStack::new().spacing(5.0).add_child(fixed).add_child(b));
    tree.layout(SizeProposal::exact(300.0, 50.0));

    // FixedSize reports 40x20 regardless of proposal
    assert!((tree.bounds(fixed).width - 40.0).abs() < 0.01);
    assert!((tree.bounds(fixed).height - 20.0).abs() < 0.01);
    // b starts at 40+5=45
    assert!((tree.bounds(b).x - 45.0).abs() < 0.01);
}

/// Reproduces the text_and_layout demo structure to check for layout overlaps.
/// Bug report: the last child of the outermost VStack overlaps the first child
/// (both appear at the same y position), flickering when resizing.
#[test]
fn demo_layout_no_overlap_between_sections() {
    // Structure:
    //   Padding(24)
    //     VStack(spacing=20)  — outer
    //       HStack(toolbar): [TextWidget("Title"), Spacer, FixedLeaf(button)]
    //       VStack(typography, spacing=6): [TextWidget × 3]
    //       VStack(showcase, spacing=6):
    //         TextWidget("Section")
    //         HStack(spacing=8): [FixedLeaf × 3]
    //         TextWidget("Caption1")
    //         HStack: [TextWidget("Leading"), Spacer, TextWidget("Trailing")]
    //         TextWidget("Caption2")  — the "last child"
    let mut tree = WidgetTree::new();

    // -- Toolbar --
    let title = tree.add(TextWidget::new(lit!("Title text here"))); // 15*8=120 wide
    let toolbar_spacer = tree.add(Spacer::new());
    let btn = tree.add(FixedLeaf(140.0, 36.0)); // mock button
    let toolbar = tree.add(
        HStack::new()
            .add_child(title)
            .add_child(toolbar_spacer)
            .add_child(btn),
    );

    // -- Typography section --
    let typo_heading = tree.add(TextWidget::new(lit!("Typography"))); // 10*8=80
    let typo_body1 = tree.add(TextWidget::new(lit!("Body line one"))); // 13*8=104
    let typo_body2 = tree.add(TextWidget::new(lit!("Body line two"))); // 13*8=104
    let typography = tree.add(
        VStack::new()
            .spacing(6.0)
            .add_child(typo_heading)
            .add_child(typo_body1)
            .add_child(typo_body2),
    );

    // -- Layout showcase section --
    let section_heading = tree.add(TextWidget::new(lit!("Layout Primitives"))); // 17*8=136
    let box_a = tree.add(FixedLeaf(40.0, 32.0));
    let box_b = tree.add(FixedLeaf(40.0, 32.0));
    let box_c = tree.add(FixedLeaf(40.0, 32.0));
    let color_row = tree.add(
        HStack::new()
            .spacing(8.0)
            .add_child(box_a)
            .add_child(box_b)
            .add_child(box_c),
    );
    let caption1 = tree.add(TextWidget::new(lit!("Three colored boxes"))); // 19*8=152
    let leading = tree.add(TextWidget::new(lit!("Leading"))); // 7*8=56
    let inner_spacer = tree.add(Spacer::new());
    let trailing = tree.add(TextWidget::new(lit!("Trailing"))); // 8*8=64
    let spacer_row = tree.add(
        HStack::new()
            .add_child(leading)
            .add_child(inner_spacer)
            .add_child(trailing),
    );
    let caption2 = tree.add(TextWidget::new(lit!("Spacer pushing items to edges"))); // 30*8=240

    let showcase = tree.add(
        VStack::new()
            .spacing(6.0)
            .add_child(section_heading)
            .add_child(color_row)
            .add_child(caption1)
            .add_child(spacer_row)
            .add_child(caption2),
    );

    // -- Outer VStack with Padding --
    let outer = tree.add(
        VStack::new()
            .spacing(20.0)
            .add_child(toolbar)
            .add_child(typography)
            .add_child(showcase),
    );
    let _root = tree.add(Padding::uniform(24.0).child_id(outer));
    tree.layout(SizeProposal::exact(600.0, 500.0));

    // === Check that each section starts BELOW the previous section ===
    let toolbar_b = tree.bounds(toolbar);
    let typography_b = tree.bounds(typography);
    let showcase_b = tree.bounds(showcase);

    eprintln!(
        "toolbar:    y={:.1}, h={:.1}, bottom={:.1}",
        toolbar_b.y,
        toolbar_b.height,
        toolbar_b.y + toolbar_b.height
    );
    eprintln!(
        "typography: y={:.1}, h={:.1}, bottom={:.1}",
        typography_b.y,
        typography_b.height,
        typography_b.y + typography_b.height
    );
    eprintln!(
        "showcase:   y={:.1}, h={:.1}, bottom={:.1}",
        showcase_b.y,
        showcase_b.height,
        showcase_b.y + showcase_b.height
    );

    // Toolbar should start at y=24 (inside padding)
    assert!(
        (toolbar_b.y - 24.0).abs() < 0.01,
        "toolbar.y = {}",
        toolbar_b.y
    );

    // Typography should start after toolbar + spacing(20)
    let expected_typo_y = toolbar_b.y + toolbar_b.height + 20.0;
    assert!(
        (typography_b.y - expected_typo_y).abs() < 0.01,
        "typography.y = {} (expected {})",
        typography_b.y,
        expected_typo_y,
    );

    // Showcase should start after typography + spacing(20)
    let expected_showcase_y = typography_b.y + typography_b.height + 20.0;
    assert!(
        (showcase_b.y - expected_showcase_y).abs() < 0.01,
        "showcase.y = {} (expected {})",
        showcase_b.y,
        expected_showcase_y,
    );

    // === Check title and caption2 do NOT overlap ===
    let title_b = tree.bounds(title);
    let caption2_b = tree.bounds(caption2);
    eprintln!("title:    bounds={:?}", title_b);
    eprintln!("caption2: bounds={:?}", caption2_b);

    assert!(
        caption2_b.y > title_b.y + title_b.height,
        "caption2 (y={}) should be well below title (bottom={})",
        caption2_b.y,
        title_b.y + title_b.height,
    );

    // === Check inner HStack with Spacer has correct width ===
    let spacer_row_b = tree.bounds(spacer_row);
    let leading_b = tree.bounds(leading);
    let trailing_b = tree.bounds(trailing);
    eprintln!("spacer_row: bounds={:?}", spacer_row_b);
    eprintln!("leading:    bounds={:?}", leading_b);
    eprintln!("trailing:   bounds={:?}", trailing_b);

    // The spacer row should be full-width (same as its parent showcase VStack)
    assert!(
        (spacer_row_b.width - showcase_b.width).abs() < 0.01,
        "spacer_row width={} should match showcase width={}",
        spacer_row_b.width,
        showcase_b.width,
    );

    // "Trailing" should be pushed to the right edge
    let expected_trailing_x = spacer_row_b.x + spacer_row_b.width - trailing_b.width;
    assert!(
        (trailing_b.x - expected_trailing_x).abs() < 0.01,
        "trailing.x={} should be at right edge (expected {})",
        trailing_b.x,
        expected_trailing_x,
    );

    // "Leading" should be at the left edge
    assert!(
        (leading_b.x - spacer_row_b.x).abs() < 0.01,
        "leading.x={} should be at left edge (expected {})",
        leading_b.x,
        spacer_row_b.x,
    );

    // === Verify every TextWidget has a unique y position ===
    let all_texts = [
        ("title", tree.bounds(title)),
        ("typo_heading", tree.bounds(typo_heading)),
        ("typo_body1", tree.bounds(typo_body1)),
        ("typo_body2", tree.bounds(typo_body2)),
        ("section_heading", tree.bounds(section_heading)),
        ("caption1", tree.bounds(caption1)),
        ("caption2", tree.bounds(caption2)),
    ];
    for i in 0..all_texts.len() {
        for j in (i + 1)..all_texts.len() {
            let (name_a, a) = all_texts[i];
            let (name_b, b) = all_texts[j];
            // Check rects don't overlap vertically
            let overlap = a.y < b.y + b.height && b.y < a.y + a.height;
            assert!(
                !overlap,
                "{} (y={:.1}..{:.1}) overlaps {} (y={:.1}..{:.1})",
                name_a,
                a.y,
                a.y + a.height,
                name_b,
                b.y,
                b.y + b.height,
            );
        }
    }
}
