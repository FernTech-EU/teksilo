// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Mount every widget the Breeze preset restyles, in both schemes, render
//! it, and check the colours it paints against Plasma's own: the buttons,
//! the check box, the radio button, the switch, the line edit, the combo
//! box, the spin box, the slider, the menu, the popup frame, the tooltip,
//! the list and tree rows, the table, the tab, the scroll bar and the
//! progress bar, and the widgets whose stock recipe style the preset builds
//! with Breeze's metrics.
//!
//! The unit tests beside each style check its rules. This checks that the
//! styles run: a Tier-3 style is only exercised when its widget is built,
//! and its `make_body` and `paint` reach framework surfaces (derived
//! signals, the binding registry, the paint context's window state) whose
//! preconditions the trait signature does not show.
//!
//! The exact-colour checks read the colours from the render frame, which
//! carries what the style asked for, and compare them with the golden
//! samples of Plasma 6.8 painting the same controls
//! (`tools/kde_golden/vectors/pixels.json`). This file reads that file from
//! the repository, so it is left out of the published package (the
//! `exclude` list in `Cargo.toml`).

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use serde_json::Value;
use teksilo_canvas::{FillRule, MockTextBackend, Path, RenderFrame, ShapeKind, SizeProposal};
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{Key, Modifiers, PointerButton};
use teksilo_core::overlay::OverlayPlacement;
use teksilo_core::signal::Signal;
use teksilo_core::styles::TabIndicatorPosition;
use teksilo_core::styles::{PopoverStyleConfig, PopoverVariant, Theme};
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_data::{ListModel, TreeModel};
use teksilo_i18n::lit;
use teksilo_theme_kde::styles::indicator::BreezeIndicatorColors;
use teksilo_theme_kde::styles::table::BreezeHeaderColors;
use teksilo_theme_kde::{self as kde, KdePalette, PlasmaAccent};
use teksilo_tokens::{Color, Orientation};
use teksilo_widgets::primitives::{FixedSize, HStack, IconWidget, TextWidget};
use teksilo_widgets::scroll_bar::{ScrollBar, ScrollBarOrientation, ScrollBarVariant};
use teksilo_widgets::tab_widget::{TabBar, TabDelegate, TabId, TabSizing};
use teksilo_widgets::table_view::{Column, ColumnWidth, SortDirection};
use teksilo_widgets::{
    Button, ButtonVariant, CheckState, Checkbox, ComboBox, IconButton, IconButtonSize, MenuItem,
    MenuList, ProgressBar, RadioButton, Slider, SpinBox, SplitButton, StandardListItem,
    StandardTreeItem, TableView, TextInput, Toggle, TooltipWidget, TreeTableView, VStack,
};

const PIXELS_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/kde_golden/vectors/pixels.json"
));

const VARIANTS: [ButtonVariant; 7] = [
    ButtonVariant::Filled,
    ButtonVariant::Tinted,
    ButtonVariant::Outlined,
    ButtonVariant::Plain,
    ButtonVariant::Ghost,
    ButtonVariant::Link,
    ButtonVariant::Destructive,
];

const SIZES: [IconButtonSize; 5] = [
    IconButtonSize::Compact,
    IconButtonSize::Default,
    IconButtonSize::Toolbar,
    IconButtonSize::Large,
    IconButtonSize::Hero,
];

fn themes() -> [(&'static str, Theme); 2] {
    [("BreezeLight", kde::light()), ("BreezeDark", kde::dark())]
}

fn tree_with(theme: Theme) -> WidgetTree {
    WidgetTree::new()
        .with_theme(theme)
        .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())))
}

/// Mount `w` under `theme`, lay it out and render it: the number of draw
/// commands, so a caller can assert something was painted.
fn mount(theme: Theme, w: impl Widget + 'static) -> usize {
    let mut tree = tree_with(theme);
    tree.add(w);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.render().draw_order.len()
}

/// A tree holding `w` at its natural size in a row, laid out: the tree and
/// the widget's id.
fn placed(theme: Theme, w: impl Widget + 'static) -> (WidgetTree, WidgetId) {
    let mut tree = tree_with(theme);
    let row = tree.add(HStack::new().child(w));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let id = tree.children(row)[0];
    (tree, id)
}

/// A tree holding `w` at its natural height in a column, laid out: the tree
/// and the widget's id.
fn in_column(theme: Theme, w: impl Widget + 'static) -> (WidgetTree, WidgetId) {
    let mut tree = tree_with(theme);
    let column = tree.add(VStack::new().child(HStack::new().child(w)));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let row = tree.children(column)[0];
    let id = tree.children(row)[0];
    (tree, id)
}

fn rerender(tree: &mut WidgetTree) -> Rc<RenderFrame> {
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.render()
}

fn rgba8(c: [f32; 4]) -> [u8; 4] {
    c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// One painted shape: its colour in 8 bits, its stroke width (0 for a
/// fill), its rectangle, its kind and its top-leading corner radius.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Painted {
    color: [u8; 4],
    stroke: f32,
    screen: [f32; 4],
    kind: ShapeKind,
    radius: f32,
}

fn painted(frame: &RenderFrame) -> Vec<Painted> {
    frame
        .shapes
        .iter()
        .map(|s| Painted {
            color: rgba8(s.color),
            stroke: s.stroke_width,
            screen: s.screen,
            kind: s.shape,
            radius: s.corner_radii[0],
        })
        .collect()
}

fn fills(frame: &RenderFrame) -> Vec<[u8; 4]> {
    painted(frame)
        .into_iter()
        .filter(|p| p.stroke == 0.0)
        .map(|p| p.color)
        .collect()
}

fn strokes(frame: &RenderFrame) -> Vec<[u8; 4]> {
    painted(frame)
        .into_iter()
        .filter(|p| p.stroke > 0.0)
        .map(|p| p.color)
        .collect()
}

/// Black at 12.5 %, the button shadow, in 8 bits.
const SHADOW: [u8; 4] = [0, 0, 0, 32];

fn has_shadow(frame: &RenderFrame) -> bool {
    strokes(frame).contains(&SHADOW)
}

/// The colour Plasma painted at `(x, y)` of the control `id` in `scheme`.
fn sample(scheme: &str, id: &str, x: usize, y: usize) -> [u8; 4] {
    let all: Value = serde_json::from_str(PIXELS_JSON).expect("pixels.json parses");
    let record = all["schemes"][scheme]["records"]
        .as_array()
        .expect("the scheme's records")
        .iter()
        .find(|r| r["id"] == id)
        .unwrap_or_else(|| panic!("{scheme} has no {id}"));
    let (line, at) = if let Some(row) = record["rows"].get(y.to_string()) {
        (row, x)
    } else {
        (
            record["cols"]
                .get(x.to_string())
                .unwrap_or_else(|| panic!("{id} samples neither row {y} nor column {x}")),
            y,
        )
    };
    let run = line
        .as_array()
        .expect("a run-length line")
        .iter()
        .find(|run| {
            let start = run[0].as_u64().unwrap() as usize;
            let len = run[1].as_u64().unwrap() as usize;
            (start..start + len).contains(&at)
        })
        .expect("the line covers the pixel");
    let hex = run[2].as_str().unwrap().trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap();
    [byte(0), byte(2), byte(4), byte(6)]
}

/// `top` composited over the opaque `bottom`, in 8 bits.
fn over(top: [u8; 4], bottom: [u8; 4]) -> [u8; 4] {
    let a = f32::from(top[3]) / 255.0;
    let ch = |i: usize| (f32::from(top[i]) * a + f32::from(bottom[i]) * (1.0 - a)).round() as u8;
    [ch(0), ch(1), ch(2), 255]
}

/// Whether two colours are within one step a channel: Qt's raster painter
/// rounds an 8-bit blend its own way.
fn near(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 1)
}

// ── Every restyled widget mounts and paints ─────────────────────────────

#[test]
fn buttons_mount_in_every_variant() {
    for (_, theme) in themes() {
        for variant in VARIANTS {
            let drawn = mount(theme.clone(), Button::new(lit!("Save")).variant(variant));
            assert!(drawn > 0, "{variant:?} painted nothing");
        }
    }
}

#[test]
fn icon_buttons_mount_at_every_size() {
    for (_, theme) in themes() {
        for size in SIZES {
            let drawn = mount(
                theme.clone(),
                IconButton::new(IconWidget::checkmark(12.0)).size(size),
            );
            assert!(drawn > 0, "{size:?} painted nothing");
            for on in [false, true] {
                let drawn = mount(
                    theme.clone(),
                    IconButton::new(IconWidget::checkmark(12.0))
                        .size(size)
                        .toggle(Signal::new(on)),
                );
                assert!(drawn > 0, "a {size:?} toggle painted nothing");
            }
        }
    }
}

#[test]
fn split_buttons_mount_in_every_variant() {
    for (_, theme) in themes() {
        for variant in VARIANTS {
            let drawn = mount(
                theme.clone(),
                SplitButton::new()
                    .item(MenuItem::new(lit!("Run")))
                    .item(MenuItem::new(lit!("Debug")))
                    .variant(variant),
            );
            assert!(drawn > 0, "{variant:?} painted nothing");
        }
    }
}

#[test]
fn indicators_and_fields_mount_in_every_state_and_variant() {
    use teksilo_core::styles::{CheckboxVariant, RadioVariant, TextInputVariant};
    for (_, theme) in themes() {
        for on in [false, true] {
            assert!(
                mount(
                    theme.clone(),
                    Toggle::new(Signal::new(on)).label(lit!("Wi-Fi"))
                ) > 0
            );
            assert!(mount(theme.clone(), Toggle::new(Signal::new(on)).enabled(false)) > 0);
        }
        for state in [
            CheckState::Unchecked,
            CheckState::Checked,
            CheckState::Indeterminate,
        ] {
            for variant in [
                CheckboxVariant::Square,
                CheckboxVariant::Rounded,
                CheckboxVariant::Circle,
            ] {
                let drawn = mount(
                    theme.clone(),
                    Checkbox::tristate(Signal::new(state))
                        .variant(variant)
                        .label(lit!("Enable")),
                );
                assert!(drawn > 0, "{state:?} {variant:?} painted nothing");
            }
        }
        for variant in [
            RadioVariant::Circle,
            RadioVariant::Square,
            RadioVariant::Rounded,
        ] {
            let selected = Signal::new(0_usize);
            let drawn = mount(
                theme.clone(),
                VStack::new()
                    .child(
                        RadioButton::new(0, selected.clone())
                            .variant(variant)
                            .label(lit!("One")),
                    )
                    .child(
                        RadioButton::new(1, selected.clone())
                            .variant(variant)
                            .label(lit!("Two")),
                    ),
            );
            assert!(drawn > 0, "{variant:?} painted nothing");
        }
        for variant in [
            TextInputVariant::Outlined,
            TextInputVariant::Filled,
            TextInputVariant::Underline,
            TextInputVariant::Bare,
        ] {
            let drawn = mount(
                theme.clone(),
                TextInput::new(Signal::new("hello".to_string()))
                    .placeholder(lit!("Name"))
                    .variant(variant),
            );
            assert!(drawn > 0, "{variant:?} painted nothing");
        }
    }
}

#[test]
fn combo_boxes_spin_boxes_and_sliders_mount_in_every_state_and_variant() {
    use teksilo_core::styles::{ComboBoxVariant, SliderVariant};
    use teksilo_tokens::Orientation;
    use teksilo_widgets::SpinButtonLayout;
    for (_, theme) in themes() {
        for variant in [
            ComboBoxVariant::Outlined,
            ComboBoxVariant::Filled,
            ComboBoxVariant::Underline,
            ComboBoxVariant::Plain,
        ] {
            for enabled in [true, false] {
                let drawn = mount(
                    theme.clone(),
                    ComboBox::new(["Item", "Other"], Signal::new(Some("Item".to_string())))
                        .variant(variant)
                        .enabled(enabled),
                );
                assert!(drawn > 0, "{variant:?} painted nothing");
            }
        }
        for layout in [SpinButtonLayout::Stacked, SpinButtonLayout::Hidden] {
            for enabled in [true, false] {
                let drawn = mount(
                    theme.clone(),
                    SpinBox::new(Signal::new(3_i32), 0, 10)
                        .button_layout(layout)
                        .enabled(enabled),
                );
                assert!(drawn > 0, "{layout:?} painted nothing");
            }
        }
        for orientation in [Orientation::Horizontal, Orientation::Vertical] {
            for (variant, ticks) in [
                (SliderVariant::Continuous, None),
                (SliderVariant::Discrete, Some(5)),
            ] {
                for enabled in [true, false] {
                    let mut slider = Slider::new(Signal::new(0.4), 0.0, 1.0)
                        .orientation(orientation)
                        .variant(variant)
                        .enabled(enabled);
                    if let Some(n) = ticks {
                        slider = slider.tick_count(n);
                    }
                    let drawn = mount(theme.clone(), slider);
                    assert!(drawn > 0, "{orientation:?} {variant:?} painted nothing");
                }
            }
        }
    }
}

/// The styles read the theme at paint time, so a tree built under Breeze
/// survives a live swap to the other scheme, out to another design language
/// and back.
#[test]
fn a_mounted_tree_survives_a_theme_swap() {
    let mut tree = tree_with(kde::light());
    tree.add(
        VStack::new()
            .child(Button::new(lit!("Save")).variant(ButtonVariant::Filled))
            .child(Button::new(lit!("Cancel")))
            .child(IconButton::new(IconWidget::checkmark(12.0)).toggle(Signal::new(true)))
            .child(SplitButton::new().item(MenuItem::new(lit!("Run"))))
            .child(Toggle::new(Signal::new(true)))
            .child(Checkbox::new(Signal::new(true)).label(lit!("On")))
            .child(RadioButton::new(0, Signal::new(0_usize)).label(lit!("One")))
            .child(TextInput::new(Signal::new("text".to_string())))
            .child(ComboBox::new(
                ["A", "B"],
                Signal::new(Some("A".to_string())),
            ))
            .child(SpinBox::new(Signal::new(3_i32), 0, 10))
            .child(Slider::new(Signal::new(0.4), 0.0, 1.0).tick_count(5)),
    );
    for theme in [
        kde::light(),
        kde::dark(),
        teksilo_core::presets::intui::light(),
        kde::light(),
    ] {
        tree.set_theme(theme);
        let frame = rerender(&mut tree);
        assert!(!frame.draw_order.is_empty());
    }
}

/// The neutral button colours come from `KdePalette`; without it the styles
/// paint from the theme's roles rather than panicking or painting nothing.
#[test]
fn the_styles_degrade_without_the_palette() {
    let mut bare = kde::light();
    bare.extensions.remove::<KdePalette>();
    assert!(bare.extension::<KdePalette>().is_none());
    for variant in VARIANTS {
        assert!(mount(bare.clone(), Button::new(lit!("Save")).variant(variant)) > 0);
    }
    assert!(mount(bare.clone(), IconButton::new(IconWidget::checkmark(12.0))) > 0);
    assert!(
        mount(
            bare.clone(),
            SplitButton::new().item(MenuItem::new(lit!("Run")))
        ) > 0
    );
    assert!(mount(bare.clone(), Toggle::new(Signal::new(true))) > 0);
    assert!(mount(bare.clone(), Checkbox::new(Signal::new(true))) > 0);
    assert!(mount(bare.clone(), RadioButton::new(0, Signal::new(0_usize))) > 0);
    assert!(mount(bare.clone(), TextInput::new(Signal::new(String::new()))) > 0);
    assert!(
        mount(
            bare.clone(),
            ComboBox::new(["A", "B"], Signal::new(Some("A".to_string())))
        ) > 0
    );
    assert!(mount(bare.clone(), SpinBox::new(Signal::new(3_i32), 0, 10)) > 0);
    assert!(
        mount(
            bare.clone(),
            Slider::new(Signal::new(0.5), 0.0, 1.0).tick_count(3)
        ) > 0
    );
    // The push button still has a face and an outline, from the roles.
    let (mut tree, _) = placed(bare.clone(), Button::new(lit!("Save")));
    let frame = rerender(&mut tree);
    assert!(fills(&frame).contains(&rgba8(bare.colors.surface_content.to_array())));
    assert!(strokes(&frame).contains(&rgba8(bare.colors.border.to_array())));
}

#[test]
fn every_swatch_mounts() {
    for accent in PlasmaAccent::ALL {
        for theme in [
            kde::light_with_accent(accent.color()),
            kde::dark_with_accent(accent.color()),
        ] {
            let drawn = mount(
                theme,
                VStack::new()
                    .child(Button::new(lit!("Save")).variant(ButtonVariant::Filled))
                    .child(Button::new(lit!("Cancel")))
                    .child(IconButton::new(IconWidget::checkmark(12.0)))
                    .child(
                        SplitButton::new()
                            .item(MenuItem::new(lit!("Run")))
                            .variant(ButtonVariant::Filled),
                    )
                    .child(Toggle::new(Signal::new(true)))
                    .child(Checkbox::new(Signal::new(true)).label(lit!("On")))
                    .child(RadioButton::new(0, Signal::new(0_usize)).label(lit!("One")))
                    .child(TextInput::new(Signal::new(String::new())))
                    .child(ComboBox::new(
                        ["A", "B"],
                        Signal::new(Some("A".to_string())),
                    ))
                    .child(SpinBox::new(Signal::new(3_i32), 0, 10))
                    .child(Slider::new(Signal::new(0.5), 0.0, 1.0)),
            );
            assert!(drawn > 0, "{accent:?} painted nothing");
        }
    }
}

/// The widgets whose Breeze styling is the stock recipe style built with
/// Breeze's metrics ([`kde::styles::recipes`]): each mounts and paints in
/// both schemes and without the palette. A recipe field that stops existing
/// is a compile error, but a nonsensical dimension still compiles and can
/// make a widget paint nothing.
#[test]
fn recipe_styled_widgets_mount() {
    use teksilo_widgets::splitter::{Splitter, SplitterModel};
    use teksilo_widgets::{
        Avatar, Badge, Banner, Calendar, Card, Link, ListView, ModalContainer, Panel, SearchField,
        SegmentedControl,
    };
    let leaf = || FixedSize::new().width(120.0).height(40.0);
    let mut bare = kde::light();
    bare.extensions.remove::<KdePalette>();
    let mut all: Vec<(&str, Theme)> = themes().into_iter().collect();
    all.push(("no palette", bare));
    for (scheme, theme) in all {
        for (name, drawn) in [
            ("card", mount(theme.clone(), Card::new().content(leaf()))),
            ("panel", mount(theme.clone(), Panel::new().child(leaf()))),
            ("dialog", mount(theme.clone(), ModalContainer::new(leaf()))),
            (
                "banner",
                mount(
                    theme.clone(),
                    Banner::warning(lit!("Low disk")).description(lit!("Free some space")),
                ),
            ),
            ("link", mount(theme.clone(), Link::new(lit!("Docs")))),
            ("badge", mount(theme.clone(), Badge::new(lit!("9")))),
            (
                "calendar",
                mount(theme.clone(), Calendar::single(Signal::new(None))),
            ),
            (
                "search_field",
                mount(theme.clone(), SearchField::new(Signal::new(String::new()))),
            ),
            (
                "avatar",
                mount(theme.clone(), Avatar::with_name(lit!("Jane Doe"))),
            ),
            (
                "segmented_control",
                mount(
                    theme.clone(),
                    SegmentedControl::indexed(Signal::new(0_usize))
                        .segment(lit!("One"))
                        .segment(lit!("Two")),
                ),
            ),
            (
                "splitter",
                mount(
                    theme.clone(),
                    Splitter::new(SplitterModel::new(2, Orientation::Horizontal))
                        .pane(leaf())
                        .pane(leaf()),
                ),
            ),
            (
                "list_container",
                mount(
                    theme.clone(),
                    ListView::new(ListModel::from_vec(vec!["A", "B"]), |_, item, _| {
                        Box::new(StandardListItem::new(lit!(*item)))
                    }),
                ),
            ),
            ("snackbar", snackbar_drawn(theme.clone())),
            ("toast", mount(theme.clone(), toast_surface())),
        ] {
            assert!(drawn > 0, "{scheme}: {name} painted nothing");
        }
    }
}

/// A toast's surface as the toast host builds it for one entry: an
/// information toast titled "Saved", with a body and a close button.
fn toast_surface() -> teksilo_widgets::toast::ToastSurface {
    use teksilo_widgets::toast::surface::ToastSurfaceData;
    use teksilo_widgets::toast::{ToastInstallOptions, ToastRegistry, ToastSeverity};
    let data = ToastSurfaceData {
        entry_id: 1,
        severity: ToastSeverity::Info,
        priority: teksilo_core::styles::ToastPriority::Normal,
        title: lit!("Saved"),
        body: Some(lit!("The file is on disk")),
        announcement: None,
        actions: Rc::new(Vec::new()),
        show_close_button: true,
        on_click: None,
        style_override: None,
        body_state: Signal::new(0),
    };
    teksilo_widgets::toast::ToastSurface::new(
        data,
        None,
        ToastRegistry::new(ToastInstallOptions::default()),
        false,
    )
}

/// Open a snackbar under `theme` through its trigger and render it: the
/// number of shapes painted at the bounds of its surface's frame, which the
/// snackbar style builds.
fn snackbar_drawn(theme: Theme) -> usize {
    use teksilo_core::event::WidgetEvent;
    use teksilo_widgets::Snackbar;
    let mut tree = tree_with(theme);
    tree.add(Snackbar::new(lit!("Show")).content(TextWidget::new(lit!("File deleted"))));
    let proposal = SizeProposal::exact(400.0, 300.0);
    tree.layout(proposal);
    let trigger = tree.find_by_label("Show").expect("the snackbar's trigger");
    tree.dispatch_event(WidgetEvent::AccessAction {
        action: teksilo_core::accesskit::Action::Click,
        target: Some(trigger),
        target_node: teksilo_core::accessibility::root_node_id(),
        data: None,
    });
    // Past the fade-in, so the surface paints at full opacity.
    tree.advance_time(Duration::from_secs(1));
    tree.layout(proposal);
    let frame = tree.render();
    let surfaces: Vec<WidgetId> = tree
        .roots()
        .into_iter()
        .flat_map(|root| nodes_named(&tree, root, "SnackbarFrame"))
        .collect();
    assert_eq!(surfaces.len(), 1, "the snackbar is open");
    let at = tree.bounds(surfaces[0]).to_array();
    frame.shapes.iter().filter(|s| s.screen == at).count()
}

/// A badge rounds at the 5 px frame radius rather than as a pill: Breeze
/// draws no pill.
#[test]
fn a_badge_rounds_at_the_frame_radius() {
    use teksilo_widgets::Badge;
    for (scheme, theme) in themes() {
        let (mut tree, _) = placed(theme, Badge::new(lit!("12")));
        let frame = rerender(&mut tree);
        assert!(
            frame
                .shapes
                .iter()
                .any(|s| s.stroke_width == 0.0 && s.corner_radii == [5.0; 4]),
            "{scheme}: no fill at the frame radius"
        );
    }
}

// ── Breeze's colours, checked in the render frame ───────────────────────

/// A push button at rest: Plasma's button face and outline, and the shadow,
/// black at 12.5 %, which over the window is the line Plasma paints below
/// the button.
#[test]
fn the_push_button_paints_breezes_face_outline_and_shadow() {
    for (scheme, theme) in themes() {
        let window = rgba8(theme.colors.surface_main.to_array());
        let (mut tree, id) = placed(theme, Button::new(lit!("OK")));
        let frame = rerender(&mut tree);
        assert!(
            fills(&frame).contains(&sample(scheme, "push_button.normal", 42, 17)),
            "{scheme}: no face {:?} in {:?}",
            sample(scheme, "push_button.normal", 42, 17),
            fills(&frame)
        );
        assert!(
            strokes(&frame).contains(&sample(scheme, "push_button.normal", 1, 17)),
            "{scheme}: no outline in {:?}",
            strokes(&frame)
        );
        assert!(has_shadow(&frame), "{scheme}: no shadow");
        assert!(near(
            over(SHADOW, window),
            sample(scheme, "push_button.normal", 42, 33)
        ));
        // The outline is stroked on the node inset by 1.5 with radius 4.5,
        // and the shadow is that outline one pixel lower.
        let b = tree.bounds(id);
        let outline = painted(&frame)
            .into_iter()
            .find(|p| p.color == sample(scheme, "push_button.normal", 1, 17) && p.stroke > 0.0)
            .unwrap();
        assert_eq!(
            outline.screen,
            [b.x + 1.5, b.y + 1.5, b.width - 3.0, b.height - 3.0]
        );
        let shadow = painted(&frame)
            .into_iter()
            .find(|p| p.color == SHADOW)
            .unwrap();
        assert_eq!(shadow.screen[1], outline.screen[1] + 1.0);
        assert_eq!(shadow.screen[0], outline.screen[0]);
    }
}

/// The default button's fill is the button colour 20 % of the way to the
/// highlight and its outline half-way from the highlight to the button
/// outline, as Plasma paints them.
#[test]
fn the_default_button_blends_the_button_toward_the_highlight() {
    for (scheme, theme) in themes() {
        let (mut tree, _) = placed(
            theme.clone(),
            Button::new(lit!("OK")).variant(ButtonVariant::Filled),
        );
        let frame = rerender(&mut tree);
        let face = sample(scheme, "push_button.default", 42, 17);
        let outline = sample(scheme, "push_button.default", 1, 17);
        assert!(
            fills(&frame).contains(&face),
            "{scheme}: {:?}",
            fills(&frame)
        );
        assert!(strokes(&frame).contains(&outline), "{scheme}");
        assert!(has_shadow(&frame), "{scheme}: the default button casts one");
        // Its label is the primary text, not the on-accent one.
        let text = rgba8(theme.colors.text_primary.to_array());
        assert!(
            frame.glyphs.iter().all(|g| rgba8(g.color) == text),
            "{scheme}: the default button's label is not the primary text"
        );

        // In an inactive window it is a push button, and casts no shadow.
        tree.set_window_active(false);
        let frame = rerender(&mut tree);
        assert!(!fills(&frame).contains(&face), "{scheme}");
        assert!(fills(&frame).contains(&sample(scheme, "push_button.normal", 42, 17)));
        assert!(!has_shadow(&frame), "{scheme}");
    }
}

/// Under the pointer the outline turns to the highlight, and the face stays.
#[test]
fn a_hovered_button_is_outlined_in_the_highlight() {
    for (scheme, theme) in themes() {
        let highlight = sample(scheme, "push_button.hover", 1, 17);
        assert_eq!(highlight, rgba8(theme.colors.accent.to_array()));
        let (mut tree, id) = placed(theme, Button::new(lit!("OK")));
        let frame = rerender(&mut tree);
        assert!(!strokes(&frame).contains(&highlight), "{scheme}: at rest");

        tree.pointer_move(tree.bounds(id).center());
        let frame = rerender(&mut tree);
        assert!(strokes(&frame).contains(&highlight), "{scheme}: hovered");
        assert!(fills(&frame).contains(&sample(scheme, "push_button.hover", 42, 17)));
        assert!(
            has_shadow(&frame),
            "{scheme}: a hovered button keeps its shadow"
        );
    }
}

/// Pressing fills the button 30 % toward the highlight and drops the
/// shadow.
#[test]
fn a_pressed_button_casts_no_shadow() {
    for (scheme, theme) in themes() {
        let (mut tree, id) = placed(theme, Button::new(lit!("OK")));
        let center = tree.bounds(id).center();
        tree.pointer_move(center);
        tree.pointer_down_button(center, PointerButton::Primary);
        let frame = rerender(&mut tree);
        assert!(
            fills(&frame).contains(&sample(scheme, "push_button.pressed", 42, 17)),
            "{scheme}: {:?}",
            fills(&frame)
        );
        assert!(strokes(&frame).contains(&sample(scheme, "push_button.pressed", 1, 17)));
        assert!(
            !has_shadow(&frame),
            "{scheme}: a pressed button cast a shadow"
        );
        tree.pointer_up_button(center, PointerButton::Primary);
    }
}

/// The shadow is an active window's only.
#[test]
fn an_inactive_window_drops_the_shadow() {
    for (scheme, theme) in themes() {
        let (mut tree, _) = placed(theme, Button::new(lit!("OK")));
        assert!(has_shadow(&rerender(&mut tree)), "{scheme}");
        tree.set_window_active(false);
        let frame = rerender(&mut tree);
        assert!(!has_shadow(&frame), "{scheme}: inactive");
        // The rest is Plasma's inactive push button.
        assert!(fills(&frame).contains(&sample(scheme, "push_button.inactive_window", 42, 17)));
        tree.set_window_active(true);
        assert!(has_shadow(&rerender(&mut tree)), "{scheme}: active again");
    }
}

/// A flat button paints nothing at rest, never a shadow, and a highlight
/// outline with no fill under the pointer.
#[test]
fn a_flat_button_paints_nothing_until_hovered_and_never_a_shadow() {
    for (scheme, theme) in themes() {
        let (mut tree, id) = placed(theme, Button::new(lit!("OK")).variant(ButtonVariant::Ghost));
        let frame = rerender(&mut tree);
        assert!(frame.shapes.is_empty(), "{scheme}: {:?}", painted(&frame));
        tree.pointer_move(tree.bounds(id).center());
        let frame = rerender(&mut tree);
        assert!(
            fills(&frame).is_empty(),
            "{scheme}: a hovered flat button has no face"
        );
        assert!(strokes(&frame).contains(&sample(scheme, "push_button.flat_hover", 1, 17)));
        assert!(!has_shadow(&frame), "{scheme}");
    }
}

/// An `IconButton` is the auto-raise tool button: invisible at rest, the
/// highlight outline under the pointer, and a toggle that is on fills with
/// the button text at 12.5 % in the plain outline, as Plasma paints a
/// checked tool button.
#[test]
fn an_icon_button_is_breezes_tool_button() {
    for (scheme, theme) in themes() {
        let window = rgba8(theme.colors.surface_main.to_array());
        let (mut tree, id) = placed(theme.clone(), IconButton::new(IconWidget::checkmark(12.0)));
        let frame = rerender(&mut tree);
        assert!(
            frame.shapes.is_empty(),
            "{scheme}: an idle tool button painted {:?}",
            painted(&frame)
        );
        tree.pointer_move(tree.bounds(id).center());
        let frame = rerender(&mut tree);
        assert!(strokes(&frame).contains(&sample(scheme, "tool_button.hover", 1, 14)));
        assert!(!has_shadow(&frame));

        let (mut tree, _) = placed(
            theme.clone(),
            IconButton::new(IconWidget::checkmark(12.0)).toggle(Signal::new(true)),
        );
        let frame = rerender(&mut tree);
        let checked = painted(&frame)
            .into_iter()
            .find(|p| p.stroke == 0.0 && p.color[3] == 32)
            .unwrap_or_else(|| panic!("{scheme}: no checked fill in {:?}", painted(&frame)));
        assert!(near(
            over(checked.color, window),
            sample(scheme, "tool_button.checked", 14, 14)
        ));
        assert!(strokes(&frame).contains(&sample(scheme, "tool_button.checked", 1, 14)));
        assert!(!has_shadow(&frame));
    }
}

/// A split button is framed as a push button, its filled variants in the
/// accent so their on-accent label reads.
#[test]
fn a_split_button_is_framed_as_a_push_button() {
    for (scheme, theme) in themes() {
        let (mut tree, _) = placed(
            theme.clone(),
            SplitButton::new().item(MenuItem::new(lit!("Run"))),
        );
        let frame = rerender(&mut tree);
        assert!(fills(&frame).contains(&sample(scheme, "push_button.normal", 42, 17)));
        assert!(strokes(&frame).contains(&sample(scheme, "push_button.normal", 1, 17)));
        assert!(has_shadow(&frame));

        let (mut tree, _) = placed(
            theme.clone(),
            SplitButton::new()
                .item(MenuItem::new(lit!("Run")))
                .variant(ButtonVariant::Filled),
        );
        let frame = rerender(&mut tree);
        assert!(fills(&frame).contains(&rgba8(theme.colors.accent.to_array())));
        let on_accent: Color = theme.colors.text_on_accent;
        assert!(
            frame
                .glyphs
                .iter()
                .any(|g| rgba8(g.color) == rgba8(on_accent.to_array())),
            "{scheme}: the filled split button's label is not the on-accent text"
        );
    }
}

/// A filled split button's frame covers exactly what a push button's does:
/// its face and its outline, in the accent, lie on the same rectangles, so
/// the outline hides the shadow's sides and meets the shadow line below.
#[test]
fn a_filled_split_button_frame_has_the_push_buttons_bounds() {
    // Each shape's screen rectangle relative to the node.
    let local = |p: Painted, b: teksilo_canvas::Rect| {
        [
            p.screen[0] - b.x,
            p.screen[1] - b.y,
            p.screen[2],
            p.screen[3],
        ]
    };
    for (scheme, theme) in themes() {
        let (mut tree, id) = placed(
            theme.clone(),
            SplitButton::new().item(MenuItem::new(lit!("Run"))),
        );
        let frame = rerender(&mut tree);
        let b = tree.bounds(id);
        let face = sample(scheme, "push_button.normal", 42, 17);
        let outline = sample(scheme, "push_button.normal", 1, 17);
        let push = painted(&frame);
        let push_face = *push
            .iter()
            .find(|p| p.color == face && p.stroke == 0.0)
            .unwrap();
        let push_outline = *push
            .iter()
            .find(|p| p.color == outline && p.stroke > 0.0)
            .unwrap();

        let (mut tree, id) = placed(
            theme.clone(),
            SplitButton::new()
                .item(MenuItem::new(lit!("Run")))
                .variant(ButtonVariant::Filled),
        );
        let frame = rerender(&mut tree);
        let fb = tree.bounds(id);
        assert_eq!((fb.width, fb.height), (b.width, b.height), "{scheme}");
        let accent = rgba8(theme.colors.accent.to_array());
        let filled = painted(&frame);
        let accent_face = *filled
            .iter()
            .find(|p| p.color == accent && p.stroke == 0.0)
            .unwrap_or_else(|| panic!("{scheme}: no accent face in {filled:?}"));
        let accent_outline = *filled
            .iter()
            .find(|p| p.color == accent && p.stroke > 0.0)
            .unwrap_or_else(|| panic!("{scheme}: no accent outline in {filled:?}"));
        assert_eq!(accent_outline.stroke, push_outline.stroke, "{scheme}");
        assert_eq!(local(accent_face, fb), local(push_face, b), "{scheme}");
        assert_eq!(
            local(accent_outline, fb),
            local(push_outline, b),
            "{scheme}"
        );
        // The shadow is the outline one pixel lower, as under a push button.
        let shadow = *filled.iter().find(|p| p.color == SHADOW).unwrap();
        assert_eq!(shadow.screen[0], accent_outline.screen[0], "{scheme}");
        assert_eq!(shadow.screen[2], accent_outline.screen[2], "{scheme}");
        assert_eq!(shadow.screen[1], accent_outline.screen[1] + 1.0, "{scheme}");
    }
}

/// Keyboard focus adds the 2 px focus frame, centred on the node edge, in
/// the theme's focus colour.
#[test]
fn keyboard_focus_draws_the_focus_frame() {
    for (scheme, theme) in themes() {
        let ring = rgba8(theme.colors.focus_ring.to_array());
        let mut tree = tree_with(theme);
        let row = tree.add(
            HStack::new()
                .child(Button::new(lit!("One")))
                .child(Button::new(lit!("Two"))),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        tree.press_key(Key::Tab, Modifiers::NONE);
        let frame = rerender(&mut tree);
        let first = tree.bounds(tree.children(row)[0]);
        let focus = painted(&frame)
            .into_iter()
            .find(|p| p.color == ring && p.stroke == 2.0)
            .unwrap_or_else(|| panic!("{scheme}: no focus frame in {:?}", painted(&frame)));
        assert_eq!(focus.screen, [first.x, first.y, first.width, first.height]);
    }
}

// ── Check boxes, radio buttons, switches and fields ─────────────────────

/// The 15 x 15 shapes a painted box or disc lies on, filled or stroked.
fn box_shapes(frame: &RenderFrame) -> Vec<Painted> {
    painted(frame)
        .into_iter()
        .filter(|p| (p.screen[2] - 15.0).abs() < 0.01 && (p.screen[3] - 15.0).abs() < 0.01)
        .collect()
}

/// The centre of the painted box of the one indicator in `frame`.
fn box_centre(frame: &RenderFrame) -> teksilo_canvas::Point {
    let b = box_shapes(frame)
        .into_iter()
        .next()
        .expect("a 15 px box is painted");
    teksilo_canvas::Point::new(b.screen[0] + 7.5, b.screen[1] + 7.5)
}

/// An unchecked box: Plasma's button-coloured face on the stroked 15 px
/// box at 2.5 inside the indicator, outlined in the lifted outline (D6).
#[test]
fn an_unchecked_box_is_breezes_face_in_the_lifted_outline() {
    for (scheme, theme) in themes() {
        let (mut tree, id) = in_column(
            theme.clone(),
            Checkbox::new(Signal::new(false)).label(lit!("Check")),
        );
        let frame = rerender(&mut tree);
        let shapes = box_shapes(&frame);
        let face = sample(scheme, "check_box.off", 8, 8);
        assert!(
            shapes.iter().any(|p| p.stroke == 0.0 && p.color == face),
            "{scheme}: no face {face:?} in {shapes:?}"
        );
        let outline = rgba8(theme.colors.border_strong.to_array());
        let stroke = shapes
            .iter()
            .find(|p| p.stroke > 0.0)
            .unwrap_or_else(|| panic!("{scheme}: no outline"));
        assert_eq!(stroke.color, outline, "{scheme}");
        assert_eq!(stroke.stroke, 1.0);
        // The indicator hugs the node's leading edge, 2 px down in the 24 px
        // row: the box at 2.5 in it.
        let b = tree.bounds(id);
        assert_eq!(b.height, 24.0, "{scheme}: D8");
        assert_eq!(
            [stroke.screen[0] - b.x, stroke.screen[1] - b.y],
            [2.5, 4.5],
            "{scheme}"
        );
        // Plasma's own outline is the separator, which D6 lifts.
        let plasma = sample(scheme, "check_box.off", 2, 8);
        assert_ne!(plasma, outline, "{scheme}: the premise of D6");
    }
}

/// A checked box: the highlight outline, the highlight at 30 % over the
/// face, and the check in the view's text, drawn with a 2 px pen and mitred
/// joins.
#[test]
fn a_checked_box_is_washed_in_the_highlight_and_marked_in_the_text() {
    use teksilo_canvas::LineJoin;
    for (scheme, theme) in themes() {
        let (mut tree, id) = in_column(
            theme.clone(),
            Checkbox::new(Signal::new(true)).label(lit!("Check")),
        );
        let frame = rerender(&mut tree);
        let shapes = box_shapes(&frame);
        let face = sample(scheme, "check_box.off", 8, 8);
        let wash = shapes
            .iter()
            .find(|p| p.stroke == 0.0 && p.color[3] == 77)
            .unwrap_or_else(|| panic!("{scheme}: no wash in {shapes:?}"));
        assert!(
            near(over(wash.color, face), sample(scheme, "check_box.on", 8, 8)),
            "{scheme}: {:?}",
            over(wash.color, face)
        );
        assert!(
            shapes
                .iter()
                .any(|p| p.stroke > 0.0 && p.color == sample(scheme, "check_box.on", 2, 8)),
            "{scheme}: no highlight outline"
        );
        let mark = frame
            .paths
            .iter()
            .find(|p| p.stroke_style.width > 0.0)
            .unwrap_or_else(|| panic!("{scheme}: no check mark"));
        assert_eq!(
            rgba8(mark.color),
            sample(scheme, "check_box.on", 13, 8),
            "{scheme}"
        );
        assert_eq!(mark.stroke_style.width, 2.0);
        assert_eq!(mark.stroke_style.line_join, LineJoin::Miter);
        // The mark's three points, in the indicator.
        let b = tree.bounds(id);
        let points: Vec<[f32; 2]> =
            mark.path
                .commands()
                .iter()
                .filter_map(|c| match c {
                    teksilo_canvas::PathCommand::MoveTo(p)
                    | teksilo_canvas::PathCommand::LineTo(p) => Some([p.x - b.x, p.y - b.y]),
                    _ => None,
                })
                .collect();
        assert_eq!(
            points,
            vec![[6.0, 12.0], [9.0, 15.0], [14.5, 9.5]],
            "{scheme}: (6, 10), (9, 13), (14.5, 7.5) of an indicator 2 px down"
        );
    }
}

/// A partial box: three 2 px squares of the view's text, 4 px apart.
#[test]
fn a_partial_box_shows_three_squares() {
    for (scheme, theme) in themes() {
        let (mut tree, _) = placed(
            theme,
            Checkbox::tristate(Signal::new(CheckState::Indeterminate)).label(lit!("Some")),
        );
        let frame = rerender(&mut tree);
        let text = sample(scheme, "check_box.partial", 5, 10);
        let squares: Vec<[f32; 4]> = frame
            .decorations
            .iter()
            .filter(|d| rgba8(d.color) == text)
            .map(|d| d.rect)
            .collect();
        assert_eq!(squares.len(), 3, "{scheme}: {squares:?}");
        for s in &squares {
            assert_eq!((s[2], s[3]), (2.0, 2.0));
        }
        assert_eq!(squares[1][0] - squares[0][0], 4.0);
        assert_eq!(squares[2][0] - squares[1][0], 4.0);
    }
}

/// Under the pointer the box gains a 1 px ring of the focus colour; pressed,
/// its face darkens to Plasma's pressed face.
#[test]
fn a_hovered_box_rings_and_a_pressed_one_darkens() {
    for (scheme, theme) in themes() {
        let (mut tree, _) = placed(
            theme,
            Checkbox::new(Signal::new(false)).label(lit!("Check")),
        );
        let frame = rerender(&mut tree);
        let ring = sample(scheme, "check_box.hover_off", 2, 8);
        assert!(!strokes(&frame).contains(&ring), "{scheme}: at rest");
        let centre = box_centre(&frame);
        tree.pointer_move(centre);
        let frame = rerender(&mut tree);
        assert!(
            box_shapes(&frame)
                .iter()
                .any(|p| p.stroke > 0.0 && p.color == ring),
            "{scheme}: no hover ring"
        );
        tree.pointer_down_button(centre, PointerButton::Primary);
        let frame = rerender(&mut tree);
        let pressed = sample(scheme, "check_box.pressed_off", 8, 8);
        assert!(
            box_shapes(&frame)
                .iter()
                .any(|p| p.stroke == 0.0 && near(p.color, pressed)),
            "{scheme}: no pressed face {pressed:?} in {:?}",
            box_shapes(&frame)
        );
        tree.pointer_up_button(centre, PointerButton::Primary);
    }
}

/// A checked box in a backdrop window is outlined in the scheme's muted
/// highlight, not the active one.
#[test]
fn a_checked_box_in_a_backdrop_window_uses_the_muted_highlight() {
    for (scheme, theme) in themes() {
        let active = rgba8(theme.colors.accent.to_array());
        let (mut tree, _) = placed(
            theme.clone(),
            Checkbox::new(Signal::new(true)).label(lit!("Check")),
        );
        tree.set_window_active(false);
        let frame = rerender(&mut tree);
        let muted = theme.for_paint_state(teksilo_core::styles::PaintState {
            window_active: false,
            high_contrast: false,
        });
        let muted = rgba8(muted.colors.accent.to_array());
        assert_ne!(muted, active, "{scheme}: premise");
        let outlines: Vec<[u8; 4]> = box_shapes(&frame)
            .into_iter()
            .filter(|p| p.stroke > 0.0)
            .map(|p| p.color)
            .collect();
        assert!(outlines.contains(&muted), "{scheme}: {outlines:?}");
        assert!(!outlines.contains(&active), "{scheme}");
    }
}

/// Keyboard focus draws the 2 px focus frame outside the 16 px box.
#[test]
fn keyboard_focus_frames_the_box() {
    for (scheme, theme) in themes() {
        let ring = rgba8(theme.colors.focus_ring.to_array());
        let (mut tree, id) = in_column(theme, Checkbox::new(Signal::new(false)).label(lit!("A")));
        tree.press_key(Key::Tab, Modifiers::NONE);
        let frame = rerender(&mut tree);
        let focus = painted(&frame)
            .into_iter()
            .find(|p| p.color == ring && p.stroke == 2.0)
            .unwrap_or_else(|| panic!("{scheme}: no focus frame in {:?}", painted(&frame)));
        let b = tree.bounds(id);
        // The indicator at (0, 2): the ring's centre line 1 px outside the
        // box at (2, 2), so at (1, 3), 18 px wide.
        assert_eq!(
            [
                focus.screen[0] - b.x,
                focus.screen[1] - b.y,
                focus.screen[2],
                focus.screen[3]
            ],
            [1.0, 3.0, 18.0, 18.0],
            "{scheme}"
        );
    }
}

/// A radio button: the check box's face and lifted outline on a disc, and,
/// selected, the highlight outline, the wash and a 6 px dot of the view's
/// text.
#[test]
fn a_radio_button_is_breezes_disc_and_dot() {
    for (scheme, theme) in themes() {
        let selected = Signal::new(1_usize);
        let (mut tree, _) = placed(
            theme.clone(),
            RadioButton::new(0, selected.clone()).label(lit!("One")),
        );
        let frame = rerender(&mut tree);
        let shapes = box_shapes(&frame);
        assert!(
            shapes
                .iter()
                .any(|p| p.stroke == 0.0 && p.color == sample(scheme, "radio_button.off", 10, 10)),
            "{scheme}: no face in {shapes:?}"
        );
        assert!(
            shapes
                .iter()
                .any(|p| p.stroke > 0.0 && p.color == rgba8(theme.colors.border_strong.to_array())),
            "{scheme}: no lifted outline"
        );

        let (mut tree, _) = placed(
            theme.clone(),
            RadioButton::new(0, Signal::new(0_usize)).label(lit!("One")),
        );
        let frame = rerender(&mut tree);
        let shapes = box_shapes(&frame);
        let face = sample(scheme, "radio_button.off", 10, 10);
        let wash = shapes
            .iter()
            .find(|p| p.stroke == 0.0 && p.color[3] == 77)
            .unwrap_or_else(|| panic!("{scheme}: no wash"));
        assert!(near(
            over(wash.color, face),
            sample(scheme, "radio_button.on", 5, 10)
        ));
        assert!(
            shapes
                .iter()
                .any(|p| p.stroke > 0.0 && p.color == sample(scheme, "radio_button.on", 2, 10)),
            "{scheme}: no highlight outline"
        );
        let dot = painted(&frame)
            .into_iter()
            .find(|p| (p.screen[2] - 6.0).abs() < 0.01 && (p.screen[3] - 6.0).abs() < 0.01)
            .unwrap_or_else(|| panic!("{scheme}: no 6 px dot"));
        assert_eq!(dot.color, sample(scheme, "radio_button.on", 10, 10));
        let disc = shapes[0];
        assert_eq!(
            [dot.screen[0] + 3.0, dot.screen[1] + 3.0],
            [disc.screen[0] + 7.5, disc.screen[1] + 7.5],
            "{scheme}: the dot is centred"
        );
    }
}

/// A switch: the 12 px track in the button colour moved 10 % toward the
/// button text, an 18 px handle in the button colour, and, on, the
/// highlight at 50 % up to the handle, which sits at the trailing end; in
/// right-to-left at the leading one.
#[test]
fn a_switch_is_the_desktop_styles_track_and_handle() {
    use teksilo_core::environment::LayoutDirection;
    use teksilo_kde_scheme::KdeColor;
    use teksilo_kde_scheme::math::mix;
    for (scheme, theme) in themes() {
        let p = theme.extension::<KdePalette>().unwrap();
        let b = p.colors(teksilo_kde_scheme::ColorState::Active);
        let track = mix(
            KdeColor::from_color(b.button),
            KdeColor::from_color(b.button_text),
            0.1,
        )
        .to_color();
        let track = rgba8(track.to_array());
        let button = rgba8(b.button.to_array());
        for direction in [LayoutDirection::LeftToRight, LayoutDirection::RightToLeft] {
            for on in [false, true] {
                let mut tree = tree_with(theme.clone());
                tree.set_layout_direction(direction);
                let row = tree.add(HStack::new().child(Toggle::new(Signal::new(on))));
                tree.layout(SizeProposal::exact(400.0, 300.0));
                let id = tree.children(row)[0];
                let node = tree.bounds(id);
                let frame = rerender(&mut tree);
                let all = painted(&frame);
                let t = all
                    .iter()
                    .find(|s| s.stroke == 0.0 && s.screen[2] == 36.0 && s.screen[3] == 12.0)
                    .unwrap_or_else(|| panic!("{scheme}: no 36 x 12 track in {all:?}"));
                assert_eq!(t.color, track, "{scheme}");
                let handle = all
                    .iter()
                    .find(|s| s.stroke == 0.0 && s.screen[2] == 18.0 && s.screen[3] == 18.0)
                    .unwrap_or_else(|| panic!("{scheme}: no 18 px handle"));
                assert_eq!(handle.color, button, "{scheme}");
                let at_trailing = handle.screen[0] - t.screen[0] == 18.0;
                let rtl = direction == LayoutDirection::RightToLeft;
                assert_eq!(at_trailing, on != rtl, "{scheme} {direction:?} on {on}");
                let checked = all.iter().any(|s| s.stroke == 0.0 && s.color[3] == 128);
                assert_eq!(checked, on, "{scheme}: the checked fill");
                // The node is the density's target; the indicator is centred.
                assert_eq!((node.width, node.height), (36.0, 24.0));
                assert_eq!(t.screen[1] - node.y, 6.0);
            }
        }
    }
}

/// A line edit: Plasma's view fill on the node inset by 0.5 at radius 4.5,
/// the lifted outline at rest (D6) and the lifted focus colour while
/// focused, at the same 1 px. (The hover outline is the style's rule, tested
/// beside it: `TextInput` does not report the pointer over it.)
#[test]
fn a_field_is_breezes_line_edit() {
    for (scheme, theme) in themes() {
        let mut tree = tree_with(theme.clone());
        tree.add(
            VStack::new()
                .child(Button::new(lit!("Before")))
                .child(TextInput::new(Signal::new(String::new()))),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let frame = rerender(&mut tree);
        let fill = sample(scheme, "line_edit.normal", 75, 16);
        let rest = rgba8(theme.colors.border_strong.to_array());
        let outline = painted(&frame)
            .into_iter()
            .find(|p| p.stroke > 0.0 && p.color == rest)
            .unwrap_or_else(|| panic!("{scheme}: no lifted outline in {:?}", painted(&frame)));
        assert_eq!(outline.stroke, 1.0);
        assert_eq!(
            outline.screen[3], 31.0,
            "{scheme}: a 32 px field inset by 0.5"
        );
        assert!(
            painted(&frame)
                .iter()
                .any(|p| p.stroke == 0.0 && p.color == fill && p.screen == outline.screen),
            "{scheme}: no view fill on the frame"
        );
        assert_ne!(
            sample(scheme, "line_edit.normal", 0, 16),
            rest,
            "{scheme}: the premise of D6"
        );

        tree.press_key(Key::Tab, Modifiers::NONE);
        tree.press_key(Key::Tab, Modifiers::NONE);
        let frame = rerender(&mut tree);
        let focus = rgba8(theme.colors.border_focused.to_array());
        let focused = painted(&frame)
            .into_iter()
            .find(|p| p.color == focus && p.screen == outline.screen)
            .unwrap_or_else(|| panic!("{scheme}: no focus outline in {:?}", painted(&frame)));
        assert_eq!(focused.stroke, 1.0, "{scheme}: focus keeps the 1 px width");
    }
}

/// A disabled field: Plasma's disabled view fill in the disabled outline.
#[test]
fn a_disabled_field_paints_the_disabled_colours() {
    for (scheme, theme) in themes() {
        let (mut tree, _) = placed(
            theme.clone(),
            TextInput::new(Signal::new(String::new())).enabled(false),
        );
        let frame = rerender(&mut tree);
        let fill = sample(scheme, "line_edit.disabled", 75, 16);
        assert!(
            fills(&frame).contains(&fill),
            "{scheme}: no {fill:?} in {:?}",
            fills(&frame)
        );
        let outline = rgba8(theme.colors.border_disabled.to_array());
        assert!(strokes(&frame).contains(&outline), "{scheme}");
        assert!(near(outline, sample(scheme, "line_edit.disabled", 0, 16)));
    }
}

// ── Indicator geometry, focus, disabled and right-to-left ──────────────

/// The colours the indicator styles paint from, in an active window.
fn indicator_colors(theme: &Theme, disabled: bool) -> BreezeIndicatorColors {
    BreezeIndicatorColors::resolve(theme, true, false, disabled)
}

fn rgba8_of(c: Color) -> [u8; 4] {
    rgba8(c.to_array())
}

/// Every node under `root` whose type is `name`.
fn nodes_named(tree: &WidgetTree, root: WidgetId, name: &str) -> Vec<WidgetId> {
    fn walk(tree: &WidgetTree, id: WidgetId, name: &str, out: &mut Vec<WidgetId>) {
        if tree
            .widget_type_name(id)
            .is_some_and(|t| t.rsplit("::").next() == Some(name))
        {
            out.push(id);
        }
        for c in tree.children(id) {
            walk(tree, c, name, out);
        }
    }
    let mut out = Vec::new();
    walk(tree, root, name, &mut out);
    out
}

/// The track fill and the handle face of the one switch in `frame`.
fn switch_parts(frame: &RenderFrame) -> (Painted, Painted) {
    let all = painted(frame);
    let track = *all
        .iter()
        .find(|s| s.stroke == 0.0 && s.screen[2] == 36.0 && s.screen[3] == 12.0)
        .unwrap_or_else(|| panic!("no 36 x 12 track in {all:?}"));
    let handle = *all
        .iter()
        .find(|s| s.stroke == 0.0 && s.kind == ShapeKind::Circle && s.screen[2] == 18.0)
        .unwrap_or_else(|| panic!("no 18 px handle in {all:?}"));
    (track, handle)
}

/// The handle's leading edge from the track's, left to right.
fn handle_offset(frame: &RenderFrame) -> f32 {
    let (track, handle) = switch_parts(frame);
    handle.screen[0] - track.screen[0]
}

/// The handle's 1 px outline, a 17 px circle.
fn handle_outline(frame: &RenderFrame) -> Painted {
    *painted(frame)
        .iter()
        .find(|s| s.stroke > 0.0 && s.kind == ShapeKind::Circle && s.screen[2] == 17.0)
        .unwrap_or_else(|| panic!("no 17 px handle outline in {:?}", painted(frame)))
}

/// The track's 1 px outline, 35 x 11.
fn track_outline(frame: &RenderFrame) -> Painted {
    *painted(frame)
        .iter()
        .find(|s| s.stroke > 0.0 && s.screen[2] == 35.0 && s.screen[3] == 11.0)
        .unwrap_or_else(|| panic!("no 35 x 11 track outline in {:?}", painted(frame)))
}

/// The track's outline is inset by half its pen at radius 5.5, the handle's
/// is the 17 px circle inside the handle, and both take the D6 colours; the
/// track is outlined in the lifted highlight when on. Keyboard focus adds
/// the 2 px focus frame around the 36 x 18 indicator and turns the handle's
/// outline to the button set's hover colour.
#[test]
fn a_switch_is_outlined_and_framed_where_the_desktop_style_puts_it() {
    for (scheme, theme) in themes() {
        let c = indicator_colors(&theme, false);
        let ring = rgba8_of(theme.colors.focus_ring);
        for on in [false, true] {
            let (mut tree, id) = on_the_clock(theme.clone(), false, Toggle::new(Signal::new(on)));
            let frame = tick(&mut tree, 0);
            let node = tree.bounds(id);
            let (track, handle) = switch_parts(&frame);
            let t = track_outline(&frame);
            assert_eq!(t.kind, ShapeKind::RoundedRect, "{scheme}");
            assert_eq!(
                t.screen,
                [track.screen[0] + 0.5, track.screen[1] + 0.5, 35.0, 11.0],
                "{scheme} on {on}"
            );
            assert_eq!(t.radius, 5.5, "{scheme}");
            assert_eq!(t.stroke, 1.0);
            let expected = if on { c.checked_outline } else { c.outline };
            assert_eq!(t.color, rgba8_of(expected), "{scheme} on {on}: D6");
            let h = handle_outline(&frame);
            assert_eq!(
                h.screen,
                [handle.screen[0] + 0.5, handle.screen[1] + 0.5, 17.0, 17.0],
                "{scheme}"
            );
            assert_eq!(h.stroke, 1.0);
            assert_eq!(h.color, rgba8_of(c.button_outline), "{scheme}: at rest");
            assert!(
                !painted(&frame)
                    .iter()
                    .any(|p| p.color == ring && p.stroke == 2.0),
                "{scheme}: no focus frame unfocused"
            );

            tree.press_key(Key::Tab, Modifiers::NONE);
            let frame = tick(&mut tree, 100);
            let focus = painted(&frame)
                .into_iter()
                .find(|p| p.color == ring && p.stroke == 2.0)
                .unwrap_or_else(|| panic!("{scheme}: no focus frame in {:?}", painted(&frame)));
            // The indicator at (0, 3) of the 36 x 24 node: the frame's centre
            // line 1 px outside it, at the indicator's radius plus 1.
            assert_eq!(
                [
                    focus.screen[0] - node.x,
                    focus.screen[1] - node.y,
                    focus.screen[2],
                    focus.screen[3]
                ],
                [-1.0, 2.0, 38.0, 20.0],
                "{scheme} on {on}"
            );
            assert_eq!(focus.radius, 10.0, "{scheme}");
            assert_eq!(
                handle_outline(&frame).color,
                rgba8_of(c.button_hover),
                "{scheme}: keyboard focus outlines the handle in the hover colour"
            );
        }
    }
}

/// Keyboard focus draws the radio button's focus frame through the widget:
/// a circle 1 px outside the 16 px disc.
#[test]
fn keyboard_focus_frames_the_disc_in_a_circle() {
    for (scheme, theme) in themes() {
        let ring = rgba8_of(theme.colors.focus_ring);
        let (mut tree, id) = in_column(
            theme,
            RadioButton::new(0, Signal::new(1_usize)).label(lit!("A")),
        );
        tree.press_key(Key::Tab, Modifiers::NONE);
        let frame = rerender(&mut tree);
        let focus = painted(&frame)
            .into_iter()
            .find(|p| p.color == ring && p.stroke == 2.0)
            .unwrap_or_else(|| panic!("{scheme}: no focus frame in {:?}", painted(&frame)));
        let b = tree.bounds(id);
        assert_eq!(
            [
                focus.screen[0] - b.x,
                focus.screen[1] - b.y,
                focus.screen[2],
                focus.screen[3]
            ],
            [1.0, 3.0, 18.0, 18.0],
            "{scheme}"
        );
        assert_eq!(focus.radius, 9.0, "{scheme}: half its side, a circle");
    }
}

/// A disabled checked box and selected disc, against Plasma's disabled
/// samples: the disabled highlight's wash over the disabled face, the mark
/// and the dot in the disabled view text, the outline in the disabled
/// highlight, and no hover ring under the pointer.
#[test]
fn disabled_indicators_paint_plasmas_disabled_colours() {
    for (scheme, theme) in themes() {
        let c = indicator_colors(&theme, true);
        let highlight = rgba8_of(theme.colors.accent_disabled);

        let id = "check_box.disabled_on";
        let (mut tree, _) = placed(
            theme.clone(),
            Checkbox::new(Signal::new(true))
                .label(lit!("Check"))
                .enabled(false),
        );
        let frame = rerender(&mut tree);
        let shapes = box_shapes(&frame);
        let face = shapes
            .iter()
            .find(|p| p.stroke == 0.0 && p.color[3] == 255)
            .unwrap_or_else(|| panic!("{scheme}: no face in {shapes:?}"));
        assert_eq!(face.color, rgba8_of(c.button), "{scheme}");
        let wash = shapes
            .iter()
            .find(|p| p.stroke == 0.0 && p.color[3] == 77)
            .unwrap_or_else(|| panic!("{scheme}: no wash in {shapes:?}"));
        assert!(
            near(over(wash.color, face.color), sample(scheme, id, 4, 12)),
            "{scheme}: {:?} against {:?}",
            over(wash.color, face.color),
            sample(scheme, id, 4, 12)
        );
        let outline = shapes
            .iter()
            .find(|p| p.stroke > 0.0)
            .unwrap_or_else(|| panic!("{scheme}: no outline"));
        assert_eq!(outline.color, highlight, "{scheme}");
        let mark = frame
            .paths
            .iter()
            .find(|p| p.stroke_style.width > 0.0)
            .unwrap_or_else(|| panic!("{scheme}: no check mark"));
        assert!(
            near(rgba8(mark.color), sample(scheme, id, 13, 8)),
            "{scheme}: {:?} against {:?}",
            rgba8(mark.color),
            sample(scheme, id, 13, 8)
        );
        tree.pointer_move(box_centre(&frame));
        let frame = rerender(&mut tree);
        assert_eq!(
            box_shapes(&frame).iter().filter(|p| p.stroke > 0.0).count(),
            1,
            "{scheme}: no hover ring on a disabled box"
        );

        let id = "radio_button.disabled_on";
        let (mut tree, _) = placed(
            theme.clone(),
            RadioButton::new(0, Signal::new(0_usize))
                .label(lit!("One"))
                .enabled(false),
        );
        let frame = rerender(&mut tree);
        let shapes = box_shapes(&frame);
        let face = shapes
            .iter()
            .find(|p| p.stroke == 0.0 && p.color[3] == 255)
            .unwrap_or_else(|| panic!("{scheme}: no disc face in {shapes:?}"));
        let wash = shapes
            .iter()
            .find(|p| p.stroke == 0.0 && p.color[3] == 77)
            .unwrap_or_else(|| panic!("{scheme}: no disc wash"));
        assert!(
            near(over(wash.color, face.color), sample(scheme, id, 4, 10)),
            "{scheme}: {:?} against {:?}",
            over(wash.color, face.color),
            sample(scheme, id, 4, 10)
        );
        assert!(
            shapes
                .iter()
                .any(|p| p.stroke > 0.0 && p.color == highlight),
            "{scheme}: no disabled highlight outline"
        );
        let dot = painted(&frame)
            .into_iter()
            .find(|p| p.kind == ShapeKind::Circle && (p.screen[2] - 6.0).abs() < 0.01)
            .unwrap_or_else(|| panic!("{scheme}: no 6 px dot"));
        assert!(
            near(dot.color, sample(scheme, id, 10, 10)),
            "{scheme}: {:?} against {:?}",
            dot.color,
            sample(scheme, id, 10, 10)
        );
    }
}

/// Right to left, the box and the disc hug the node's trailing (right)
/// edge, the label ends 24 px from it, and the partial squares are
/// centred on the box, the first to appear on the leading (right) side.
#[test]
fn right_to_left_mirrors_the_indicators() {
    use teksilo_core::environment::LayoutDirection;
    for (scheme, theme) in themes() {
        let rows: [(&str, Box<dyn Widget>); 2] = [
            (
                "Checkbox",
                Box::new(Checkbox::new(Signal::new(false)).label(lit!("Check"))),
            ),
            (
                "RadioButton",
                Box::new(RadioButton::new(0, Signal::new(1_usize)).label(lit!("Radio"))),
            ),
        ];
        for (name, w) in rows {
            let mut tree = tree_with(theme.clone());
            tree.set_layout_direction(LayoutDirection::RightToLeft);
            let root = tree.add(VStack::new().child(HStack::new().child(w)));
            tree.layout(SizeProposal::exact(400.0, 300.0));
            let node = nodes_named(&tree, root, name);
            assert_eq!(node.len(), 1, "{scheme}: one {name}");
            let b = tree.bounds(node[0]);
            let frame = rerender(&mut tree);
            let stroke = box_shapes(&frame)
                .into_iter()
                .find(|p| p.stroke > 0.0)
                .unwrap_or_else(|| panic!("{scheme}: no {name} outline"));
            assert_eq!(
                [
                    b.x + b.width - (stroke.screen[0] + stroke.screen[2]),
                    stroke.screen[1] - b.y
                ],
                [2.5, 4.5],
                "{scheme} {name}: the box mirrored"
            );
            let label = nodes_named(&tree, node[0], "TextWidget");
            assert_eq!(label.len(), 1, "{scheme}: one label");
            let l = tree.bounds(label[0]);
            assert_eq!(
                b.x + b.width - (l.x + l.width),
                24.0,
                "{scheme} {name}: the label 24 px from the trailing edge"
            );
        }

        let state = Signal::new(CheckState::Unchecked);
        let mut tree = tree_with(theme.clone());
        tree.set_layout_direction(LayoutDirection::RightToLeft);
        tree.advance_time(Duration::ZERO);
        tree.add(
            VStack::new()
                .child(HStack::new().child(Checkbox::tristate(state.clone()).label(lit!("Some")))),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        tree.render();
        let text = sample(scheme, "check_box.partial", 5, 10);
        state.set(CheckState::Indeterminate);
        let squares = |f: &RenderFrame| -> Vec<[f32; 4]> {
            f.decorations
                .iter()
                .filter(|d| rgba8(d.color) == text && d.rect[2] == 2.0)
                .map(|d| d.rect)
                .collect()
        };
        tick(&mut tree, 0);
        let frame = tick(&mut tree, 40);
        let centre = box_centre(&frame);
        let first = squares(&frame);
        assert_eq!(first.len(), 1, "{scheme}: {first:?}");
        assert_eq!(first[0][0] + 1.0 - centre.x, 4.0, "{scheme}: on the right");
        let frame = tick(&mut tree, 60);
        let all = squares(&frame);
        assert_eq!(all.len(), 3, "{scheme}");
        let mut xs: Vec<f32> = all.iter().map(|s| s[0] + 1.0 - centre.x).collect();
        xs.sort_by(f32::total_cmp);
        assert_eq!(xs, vec![-4.0, 0.0, 4.0], "{scheme}: centred on the box");
    }
}

/// The hover ring is the view's focus decoration, not the highlight. With a
/// Plasma accent applied the two differ (the highlight is the accent at
/// 70 % over the view, the focus decoration the accent itself), so the
/// ring's colour tells them apart.
#[test]
fn the_hover_ring_is_the_focus_colour_not_the_highlight() {
    for accent in PlasmaAccent::ALL {
        for theme in [
            kde::light_with_accent(accent.color()),
            kde::dark_with_accent(accent.color()),
        ] {
            let c = indicator_colors(&theme, false);
            let focus = rgba8_of(c.focus);
            let highlight = rgba8_of(c.highlight);
            assert_ne!(focus, highlight, "{accent:?}: premise");
            for checked in [false, true] {
                let (mut tree, _) = placed(
                    theme.clone(),
                    Checkbox::new(Signal::new(checked)).label(lit!("Check")),
                );
                let frame = rerender(&mut tree);
                tree.pointer_move(box_centre(&frame));
                let frame = rerender(&mut tree);
                let outlines: Vec<[u8; 4]> = box_shapes(&frame)
                    .into_iter()
                    .filter(|p| p.stroke > 0.0)
                    .map(|p| p.color)
                    .collect();
                assert!(
                    outlines.contains(&focus),
                    "{accent:?} checked {checked}: {outlines:?}"
                );
                assert_eq!(
                    outlines.contains(&highlight),
                    checked,
                    "{accent:?} checked {checked}: the highlight outlines only a checked box"
                );
            }
            let (mut tree, _) = placed(
                theme.clone(),
                RadioButton::new(0, Signal::new(1_usize)).label(lit!("One")),
            );
            let frame = rerender(&mut tree);
            tree.pointer_move(box_centre(&frame));
            let frame = rerender(&mut tree);
            assert!(
                box_shapes(&frame)
                    .iter()
                    .any(|p| p.stroke > 0.0 && p.color == focus),
                "{accent:?}: no focus ring on the hovered disc"
            );
        }
    }
}

// ── Motion, on the tree's simulated clock ───────────────────────────────

/// A tree on the simulated clock, with or without the reduced-motion
/// preference, holding `w` in a row in a column, laid out and rendered
/// once: the tree and the widget's id.
fn on_the_clock(
    theme: Theme,
    reduced_motion: bool,
    w: impl Widget + 'static,
) -> (WidgetTree, WidgetId) {
    let mut tree = tree_with(theme);
    tree.set_accessibility_preferences(false, reduced_motion, 1.0);
    tree.advance_time(Duration::ZERO);
    let column = tree.add(VStack::new().child(HStack::new().child(w)));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let row = tree.children(column)[0];
    let id = tree.children(row)[0];
    tree.render();
    (tree, id)
}

/// Move the clock on by `ms` and render. An animation samples the clock
/// at most once a frame (about 16 ms), so the steps the tests take are at
/// least that long wherever the value is read.
fn tick(tree: &mut WidgetTree, ms: u64) -> Rc<RenderFrame> {
    tree.advance_time(Duration::from_millis(ms));
    rerender(tree)
}

/// The check mark's points, in the node's coordinates.
fn mark_points(frame: &RenderFrame, node: teksilo_canvas::Rect) -> Vec<[f32; 2]> {
    frame
        .paths
        .iter()
        .find(|p| p.stroke_style.width > 0.0)
        .map(|mark| {
            mark.path
                .commands()
                .iter()
                .filter_map(|c| match c {
                    teksilo_canvas::PathCommand::MoveTo(p)
                    | teksilo_canvas::PathCommand::LineTo(p) => Some([p.x - node.x, p.y - node.y]),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The diameter of the radio dot in `frame`, if one is painted.
fn dot_diameter(frame: &RenderFrame, text: [u8; 4]) -> Option<f32> {
    painted(frame)
        .into_iter()
        .find(|p| p.kind == ShapeKind::Circle && p.stroke == 0.0 && p.color == text)
        .map(|p| p.screen[2])
}

/// The whole mark, an indicator 2 px down in the 24 px row.
const WHOLE_MARK: [[f32; 2]; 3] = [[6.0, 12.0], [9.0, 15.0], [14.5, 9.5]];

/// The handle travels its 18 px in 90 ms (k.8: 100 ms, or 18 px at Qt's
/// 200 px a second if that is shorter), half way at the half time on its
/// ease-in-out, and back the same way.
#[test]
fn the_switch_handle_travels_in_ninety_milliseconds() {
    for (scheme, theme) in themes() {
        let on = Signal::new(false);
        let (mut tree, _) = on_the_clock(theme, false, Toggle::new(on.clone()));
        on.set(true);
        assert_eq!(handle_offset(&tick(&mut tree, 0)), 0.0, "{scheme}: at 0");
        let mid = handle_offset(&tick(&mut tree, 45));
        assert!(
            (8.5..9.5).contains(&mid),
            "{scheme}: half way at 45 ms, {mid}"
        );
        let late = handle_offset(&tick(&mut tree, 25));
        assert!(
            late > mid && late < 18.0,
            "{scheme}: still moving at 70 ms, {late}"
        );
        assert_eq!(
            handle_offset(&tick(&mut tree, 20)),
            18.0,
            "{scheme}: at 90 ms"
        );
        on.set(false);
        tick(&mut tree, 0);
        let back = handle_offset(&tick(&mut tree, 45));
        assert!((8.5..9.5).contains(&back), "{scheme}: {back}");
        assert_eq!(
            handle_offset(&tick(&mut tree, 45)),
            0.0,
            "{scheme}: back at 90 ms"
        );
    }
}

/// Under the pointer the handle's outline turns to the hover colour over the
/// short duration, 100 ms, on the ease-in cubic: an eighth of the way at
/// half the time.
#[test]
fn the_switch_handle_outline_turns_over_a_hundred_milliseconds() {
    for (scheme, theme) in themes() {
        let c = indicator_colors(&theme, false);
        let rest = rgba8_of(c.button_outline);
        let hover = rgba8_of(c.button_hover);
        let (mut tree, _) = on_the_clock(theme, false, Toggle::new(Signal::new(false)));
        let frame = tick(&mut tree, 0);
        let (_, handle) = switch_parts(&frame);
        assert_eq!(handle_outline(&frame).color, rest, "{scheme}");
        tree.pointer_move(teksilo_canvas::Point::new(
            handle.screen[0] + 9.0,
            handle.screen[1] + 9.0,
        ));
        assert_eq!(handle_outline(&tick(&mut tree, 0)).color, rest, "{scheme}");
        let half = handle_outline(&tick(&mut tree, 50)).color;
        let eighth = rgba8_of(c.button_outline.mix(c.button_hover, 0.125));
        assert!(near(half, eighth), "{scheme}: {half:?}, not {eighth:?}");
        let late = handle_outline(&tick(&mut tree, 30)).color;
        assert_ne!(late, hover, "{scheme}: still turning at 80 ms");
        assert_eq!(
            handle_outline(&tick(&mut tree, 20)).color,
            hover,
            "{scheme}"
        );
    }
}

/// The check mark draws along its length over 100 ms, linearly: part of its
/// first stroke at 25 ms, into its second at 50, whole at 100.
#[test]
fn the_check_mark_draws_itself_over_a_hundred_milliseconds() {
    for (scheme, theme) in themes() {
        let checked = Signal::new(false);
        let (mut tree, id) = on_the_clock(
            theme,
            false,
            Checkbox::new(checked.clone()).label(lit!("Check")),
        );
        let node = tree.bounds(id);
        checked.set(true);
        assert!(
            mark_points(&tick(&mut tree, 0), node).is_empty(),
            "{scheme}"
        );
        let quarter = mark_points(&tick(&mut tree, 25), node);
        assert_eq!(quarter.len(), 2, "{scheme}: {quarter:?}");
        assert_eq!(quarter[0], WHOLE_MARK[0]);
        assert!(
            quarter[1][0] > 6.0 && quarter[1][0] < 9.0,
            "{scheme}: {quarter:?}"
        );
        let half = mark_points(&tick(&mut tree, 25), node);
        assert_eq!(half.len(), 3, "{scheme}: {half:?}");
        assert_eq!(&half[..2], &WHOLE_MARK[..2]);
        assert!(half[2][0] > 9.0 && half[2][0] < 14.5, "{scheme}: {half:?}");
        let late = mark_points(&tick(&mut tree, 30), node);
        assert_ne!(late, WHOLE_MARK.to_vec(), "{scheme}: not whole at 80 ms");
        assert_eq!(mark_points(&tick(&mut tree, 20), node), WHOLE_MARK.to_vec());
    }
}

/// The partial squares appear one at each third of the 100 ms.
#[test]
fn the_partial_squares_appear_one_at_each_third() {
    for (scheme, theme) in themes() {
        let text = sample(scheme, "check_box.partial", 5, 10);
        let state = Signal::new(CheckState::Unchecked);
        let (mut tree, _) = on_the_clock(
            theme,
            false,
            Checkbox::tristate(state.clone()).label(lit!("Some")),
        );
        state.set(CheckState::Indeterminate);
        let count = |f: &RenderFrame| {
            f.decorations
                .iter()
                .filter(|d| rgba8(d.color) == text && d.rect[2] == 2.0)
                .count()
        };
        for (ms, shown) in [(0, 0), (20, 0), (20, 1), (30, 2), (30, 3)] {
            let frame = tick(&mut tree, ms);
            assert_eq!(count(&frame), shown, "{scheme}: after {ms} more ms");
        }
    }
}

/// The radio dot grows to its 6 px over 100 ms, linearly.
#[test]
fn the_radio_dot_grows_over_a_hundred_milliseconds() {
    for (scheme, theme) in themes() {
        let text = sample(scheme, "radio_button.on", 10, 10);
        let selected = Signal::new(1_usize);
        let (mut tree, _) = on_the_clock(
            theme,
            false,
            RadioButton::new(0, selected.clone()).label(lit!("One")),
        );
        selected.set(0);
        assert_eq!(dot_diameter(&tick(&mut tree, 0), text), None, "{scheme}");
        let half = dot_diameter(&tick(&mut tree, 50), text).expect("a dot at 50 ms");
        assert!((half - 3.0).abs() < 0.01, "{scheme}: {half}");
        let late = dot_diameter(&tick(&mut tree, 30), text).expect("a dot at 80 ms");
        assert!((late - 4.8).abs() < 0.01, "{scheme}: {late}");
        let whole = dot_diameter(&tick(&mut tree, 20), text).expect("a dot at 100 ms");
        assert!((whole - 6.0).abs() < 0.01, "{scheme}: {whole}");
    }
}

/// Under the reduced-motion preference every indicator snaps: the handle,
/// its outline, the mark, the partial squares and the dot reach their end
/// with no time passing.
#[test]
fn reduced_motion_snaps_every_indicator() {
    for (scheme, theme) in themes() {
        let c = indicator_colors(&theme, false);

        let on = Signal::new(false);
        let (mut tree, _) = on_the_clock(theme.clone(), true, Toggle::new(on.clone()));
        on.set(true);
        let frame = tick(&mut tree, 0);
        assert_eq!(handle_offset(&frame), 18.0, "{scheme}: the handle");
        let (_, handle) = switch_parts(&frame);
        tree.pointer_move(teksilo_canvas::Point::new(
            handle.screen[0] + 9.0,
            handle.screen[1] + 9.0,
        ));
        assert_eq!(
            handle_outline(&tick(&mut tree, 0)).color,
            rgba8_of(c.button_hover),
            "{scheme}: the handle's outline"
        );

        let checked = Signal::new(false);
        let (mut tree, id) = on_the_clock(
            theme.clone(),
            true,
            Checkbox::new(checked.clone()).label(lit!("Check")),
        );
        let node = tree.bounds(id);
        checked.set(true);
        assert_eq!(
            mark_points(&tick(&mut tree, 0), node),
            WHOLE_MARK.to_vec(),
            "{scheme}: the mark"
        );

        let state = Signal::new(CheckState::Unchecked);
        let (mut tree, _) = on_the_clock(
            theme.clone(),
            true,
            Checkbox::tristate(state.clone()).label(lit!("Some")),
        );
        state.set(CheckState::Indeterminate);
        let text = sample(scheme, "check_box.partial", 5, 10);
        let frame = tick(&mut tree, 0);
        let squares = frame
            .decorations
            .iter()
            .filter(|d| rgba8(d.color) == text && d.rect[2] == 2.0)
            .count();
        assert_eq!(squares, 3, "{scheme}: the partial squares");

        let selected = Signal::new(1_usize);
        let (mut tree, _) = on_the_clock(
            theme.clone(),
            true,
            RadioButton::new(0, selected.clone()).label(lit!("One")),
        );
        selected.set(0);
        let text = sample(scheme, "radio_button.on", 10, 10);
        let d = dot_diameter(&tick(&mut tree, 0), text).expect("a dot");
        assert!((d - 6.0).abs() < 0.01, "{scheme}: the dot, {d}");
    }
}

// ── Combo boxes, spin boxes and sliders ─────────────────────────────────

/// Every stroked path in `frame`: its colour and its points.
fn stroked_paths(frame: &RenderFrame) -> Vec<([u8; 4], Vec<[f32; 2]>)> {
    frame
        .paths
        .iter()
        .filter(|p| p.stroke_style.width > 0.0)
        .map(|p| {
            let points = p
                .path
                .commands()
                .iter()
                .filter_map(|c| match c {
                    teksilo_canvas::PathCommand::MoveTo(q)
                    | teksilo_canvas::PathCommand::LineTo(q) => Some([q.x, q.y]),
                    _ => None,
                })
                .collect();
            (rgba8(p.color), points)
        })
        .collect()
}

/// The points of a Breeze arrow pointing `direction` in `rect`.
fn arrow_at(
    rect: teksilo_canvas::Rect,
    direction: teksilo_theme_kde::styles::arrow::ArrowDirection,
) -> Vec<[f32; 2]> {
    teksilo_theme_kde::styles::arrow::arrow_points(rect, direction)
        .iter()
        .map(|p| [p.x, p.y])
        .collect()
}

/// The one node of type `name` under `root`, and its bounds.
fn the_node(tree: &WidgetTree, root: WidgetId, name: &str) -> (WidgetId, teksilo_canvas::Rect) {
    let nodes = nodes_named(tree, root, name);
    assert_eq!(nodes.len(), 1, "one {name}");
    (nodes[0], tree.bounds(nodes[0]))
}

fn combo() -> ComboBox<String> {
    ComboBox::new(
        ["Item", "Longer item text"],
        Signal::new(Some("Item".to_string())),
    )
}

/// A combo box at rest is Plasma's push button, face, outline and shadow,
/// 32 px tall, with its arrow in the 20 px box 2 px inside the trailing
/// edge, in the arrow colour of the button text.
#[test]
fn a_combo_box_is_a_push_button_with_breezes_arrow() {
    use teksilo_theme_kde::styles::arrow::ArrowDirection;
    use teksilo_theme_kde::styles::combo_box::combo_arrow_color;
    for (scheme, theme) in themes() {
        let (mut tree, id) = placed(theme.clone(), combo());
        let frame = rerender(&mut tree);
        let b = tree.bounds(id);
        assert_eq!(b.height, 32.0, "{scheme}");
        assert!(
            fills(&frame).contains(&sample(scheme, "combo_box.normal", 40, 16)),
            "{scheme}: no face in {:?}",
            fills(&frame)
        );
        let outline = sample(scheme, "combo_box.normal", 1, 16);
        let stroke = painted(&frame)
            .into_iter()
            .find(|p| p.stroke > 0.0 && p.color == outline)
            .unwrap_or_else(|| panic!("{scheme}: no outline in {:?}", painted(&frame)));
        assert_eq!(
            stroke.screen,
            [b.x + 1.5, b.y + 1.5, b.width - 3.0, b.height - 3.0]
        );
        assert!(has_shadow(&frame), "{scheme}: no shadow");
        assert!(near(
            over(SHADOW, rgba8_of(theme.colors.surface_main)),
            sample(scheme, "combo_box.normal", 40, 31)
        ));
        let arrow_box = teksilo_canvas::Rect::new(b.x + b.width - 22.0, b.y + 6.0, 20.0, 20.0);
        let rest = rgba8_of(combo_arrow_color(&theme, true, false, false, false));
        assert!(
            stroked_paths(&frame).contains(&(rest, arrow_at(arrow_box, ArrowDirection::Down))),
            "{scheme}: no arrow in {:?}",
            stroked_paths(&frame)
        );
    }
}

/// Under the pointer the outline turns to the highlight and the arrow keeps
/// its colour; with keyboard focus the outline is the highlight, the arrow
/// the window text, and the focus frame is drawn around the node.
#[test]
fn a_hovered_or_focused_combo_box_is_outlined_in_the_highlight() {
    use teksilo_theme_kde::styles::combo_box::combo_arrow_color;
    for (scheme, theme) in themes() {
        let (mut tree, id) = placed(theme.clone(), combo());
        let b = tree.bounds(id);
        tree.pointer_move(b.center());
        let frame = rerender(&mut tree);
        assert!(
            strokes(&frame).contains(&sample(scheme, "combo_box.hover", 1, 16)),
            "{scheme}: hovered"
        );
        let rest = rgba8_of(combo_arrow_color(&theme, true, false, false, false));
        assert!(
            stroked_paths(&frame).iter().any(|(c, _)| *c == rest),
            "{scheme}: the arrow keeps its colour under the pointer"
        );

        let (mut tree, id) = placed(theme.clone(), combo());
        tree.press_key(Key::Tab, Modifiers::NONE);
        let frame = rerender(&mut tree);
        let b = tree.bounds(id);
        assert!(
            strokes(&frame).contains(&sample(scheme, "combo_box.focus", 1, 16)),
            "{scheme}: focused"
        );
        let text = rgba8_of(theme.colors.text_primary);
        assert!(
            stroked_paths(&frame).iter().any(|(c, _)| *c == text),
            "{scheme}: the focused arrow is the window text"
        );
        let ring = rgba8_of(theme.colors.focus_ring);
        let focus = painted(&frame)
            .into_iter()
            .find(|p| p.color == ring && p.stroke == 2.0)
            .unwrap_or_else(|| panic!("{scheme}: no focus frame"));
        assert_eq!(focus.screen, [b.x, b.y, b.width, b.height]);
    }
}

/// Right to left, the arrow box is 2 px inside the left edge and the label
/// starts 6 px inside the right one.
#[test]
fn right_to_left_mirrors_the_combo_box() {
    use teksilo_core::environment::LayoutDirection;
    use teksilo_theme_kde::styles::arrow::ArrowDirection;
    for (scheme, theme) in themes() {
        let mut tree = tree_with(theme.clone());
        tree.set_layout_direction(LayoutDirection::RightToLeft);
        let root = tree.add(VStack::new().child(HStack::new().child(combo())));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let (node, b) = the_node(&tree, root, "BreezeComboLayout");
        let frame = rerender(&mut tree);
        let arrow_box = teksilo_canvas::Rect::new(b.x + 2.0, b.y + 6.0, 20.0, 20.0);
        let expected = arrow_at(arrow_box, ArrowDirection::Down);
        assert!(
            stroked_paths(&frame).iter().any(|(_, p)| *p == expected),
            "{scheme}: no mirrored arrow in {:?}",
            stroked_paths(&frame)
        );
        let (_, label) = the_node(&tree, node, "TextWidget");
        assert_eq!(b.x + b.width - (label.x + label.width), 6.0, "{scheme}");
    }
}

/// A spin box is the line edit, 32 px tall, with its two arrows in the 20 x
/// 10 halves of the column 2 px inside the trailing edge, in the arrow
/// colour of the view's text; the step buttons paint nothing of their own.
#[test]
fn a_spin_box_is_a_line_edit_with_two_arrows() {
    use teksilo_theme_kde::styles::arrow::ArrowDirection;
    use teksilo_theme_kde::styles::spin_box::BreezeSpinArrowColors;
    for (scheme, theme) in themes() {
        let (mut tree, id) = placed(theme.clone(), SpinBox::new(Signal::new(3_i32), 0, 10));
        let (_, b) = the_node(&tree, id, "BreezeSpinLayout");
        assert_eq!(b.height, 32.0, "{scheme}");
        let frame = rerender(&mut tree);
        let rest = rgba8_of(theme.colors.border_strong);
        let outline = painted(&frame)
            .into_iter()
            .find(|p| p.stroke > 0.0 && p.color == rest)
            .unwrap_or_else(|| panic!("{scheme}: no lifted outline in {:?}", painted(&frame)));
        assert_eq!(
            outline.screen,
            [b.x + 0.5, b.y + 0.5, b.width - 1.0, b.height - 1.0]
        );
        assert!(
            painted(&frame).iter().any(|p| p.stroke == 0.0
                && p.color == sample(scheme, "spin_box.normal", 50, 16)
                && p.screen == outline.screen),
            "{scheme}: no view fill on the frame"
        );
        assert_ne!(
            sample(scheme, "spin_box.normal", 0, 16),
            rest,
            "{scheme}: the premise of D6"
        );
        let arrow = rgba8_of(BreezeSpinArrowColors::resolve(&theme, true, false, false).arrow);
        let x = b.x + b.width - 22.0;
        let up = teksilo_canvas::Rect::new(x, b.y + 6.0, 20.0, 10.0);
        let down = teksilo_canvas::Rect::new(x, b.y + 16.0, 20.0, 10.0);
        let paths = stroked_paths(&frame);
        assert_eq!(
            paths,
            vec![
                (arrow, arrow_at(up, ArrowDirection::Up)),
                (arrow, arrow_at(down, ArrowDirection::Down)),
            ],
            "{scheme}: only Breeze's two arrows"
        );
        // The step buttons press on Breeze's halves.
        let steps = nodes_named(&tree, id, "StepButton");
        assert_eq!(steps.len(), 2, "{scheme}");
        assert_eq!(tree.bounds(steps[0]), up, "{scheme}: the up half");
        assert_eq!(tree.bounds(steps[1]), down, "{scheme}: the down half");
    }
}

/// Under the pointer the frame is outlined in the hover colour and the
/// arrow of the half under it turns to the hover colour; a press there
/// still steps the value.
#[test]
fn a_hovered_spin_box_half_turns_to_the_hover_colour() {
    use teksilo_theme_kde::styles::spin_box::BreezeSpinArrowColors;
    for (scheme, theme) in themes() {
        let value = Signal::new(3_i32);
        let (mut tree, id) = placed(theme.clone(), SpinBox::new(value.clone(), 0, 10));
        let steps = nodes_named(&tree, id, "StepButton");
        let up = tree.bounds(steps[0]);
        tree.pointer_move(up.center());
        let frame = rerender(&mut tree);
        let c = BreezeSpinArrowColors::resolve(&theme, true, false, false);
        let paths = stroked_paths(&frame);
        assert_eq!(paths.len(), 2, "{scheme}");
        assert_eq!(paths[0].0, rgba8_of(c.hover), "{scheme}: the up arrow");
        assert_eq!(paths[1].0, rgba8_of(c.arrow), "{scheme}: the down arrow");
        assert!(
            strokes(&frame).contains(&sample(scheme, "spin_box.hover", 0, 16)),
            "{scheme}: the hover outline in {:?}",
            strokes(&frame)
        );
        tree.pointer_down_button(up.center(), PointerButton::Primary);
        tree.pointer_up_button(up.center(), PointerButton::Primary);
        assert_eq!(value.get(), 4, "{scheme}: the up half steps");
    }
}

/// Right to left, the column is 2 px inside the left edge.
#[test]
fn right_to_left_mirrors_the_spin_box() {
    use teksilo_core::environment::LayoutDirection;
    for (scheme, theme) in themes() {
        let mut tree = tree_with(theme.clone());
        tree.set_layout_direction(LayoutDirection::RightToLeft);
        let root = tree.add(VStack::new().child(HStack::new().child(SpinBox::new(
            Signal::new(3_i32),
            0,
            10,
        ))));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let (_, b) = the_node(&tree, root, "BreezeSpinLayout");
        let steps = nodes_named(&tree, root, "StepButton");
        assert_eq!(
            tree.bounds(steps[0]),
            teksilo_canvas::Rect::new(b.x + 2.0, b.y + 6.0, 20.0, 10.0),
            "{scheme}"
        );
    }
}

/// A disabled spin box: the disabled view fill in the disabled outline, its
/// arrows in the arrow colour of the disabled view's text.
#[test]
fn a_disabled_spin_box_paints_the_disabled_colours() {
    use teksilo_theme_kde::styles::spin_box::BreezeSpinArrowColors;
    for (scheme, theme) in themes() {
        let (mut tree, _) = placed(
            theme.clone(),
            SpinBox::new(Signal::new(3_i32), 0, 10).enabled(false),
        );
        let frame = rerender(&mut tree);
        assert!(
            fills(&frame).contains(&sample(scheme, "line_edit.disabled", 75, 16)),
            "{scheme}"
        );
        assert!(strokes(&frame).contains(&rgba8_of(theme.colors.border_disabled)));
        let arrow = rgba8_of(BreezeSpinArrowColors::resolve(&theme, true, false, true).arrow);
        assert!(
            stroked_paths(&frame).iter().all(|(c, _)| *c == arrow),
            "{scheme}"
        );
    }
}

/// A slider at 30 % in a 200 px node, as Plasma's: the groove's two parts,
/// the handle's face, its D6 outline, its shadow ring.
fn slider_at(
    theme: &Theme,
    value: f32,
    direction: teksilo_core::environment::LayoutDirection,
    slider: impl FnOnce(Signal<f32>) -> Slider,
) -> (WidgetTree, WidgetId, Signal<f32>) {
    let value = Signal::new(value);
    let mut tree = tree_with(theme.clone());
    tree.set_layout_direction(direction);
    let root = tree.add(
        VStack::new().child(
            teksilo_widgets::primitives::FixedSize::new()
                .width(200.0)
                .child(slider(value.clone())),
        ),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    (tree, root, value)
}

fn slider_parts(
    tree: &WidgetTree,
    root: WidgetId,
    value: f32,
    direction: teksilo_core::environment::LayoutDirection,
) -> (
    teksilo_canvas::Rect,
    teksilo_theme_kde::styles::slider::SliderGeometry,
) {
    use teksilo_core::styles::SliderOrientation;
    let (_, b) = the_node(tree, root, "BreezeSliderBody");
    let g = teksilo_theme_kde::styles::slider::slider_geometry(
        b,
        value,
        SliderOrientation::Horizontal,
        direction,
        false,
    );
    (b, g)
}

fn inset(r: teksilo_canvas::Rect) -> [f32; 4] {
    [r.x + 0.5, r.y + 0.5, r.width - 1.0, r.height - 1.0]
}

#[test]
fn a_slider_is_breezes_groove_and_handle() {
    use teksilo_core::environment::LayoutDirection;
    let ltr = LayoutDirection::LeftToRight;
    for (scheme, theme) in themes() {
        let (mut tree, root, _) = slider_at(&theme, 0.3, ltr, |v| Slider::new(v, 0.0, 1.0));
        let (b, g) = slider_parts(&tree, root, 0.3, ltr);
        assert_eq!(b.height, 24.0, "{scheme}: raised to the target");
        assert_eq!(g.handle.width, 20.0);
        let frame = rerender(&mut tree);
        let all = painted(&frame);
        let groove = all
            .iter()
            .find(|p| p.stroke == 0.0 && p.screen == inset(g.groove))
            .unwrap_or_else(|| panic!("{scheme}: no groove in {all:?}"));
        assert!(
            near(groove.color, sample(scheme, "slider.normal", 190, 10)),
            "{scheme}: the groove {:?}",
            groove.color
        );
        // Breeze's radius 3, which the canvas holds to half the thickness of
        // the 5 px groove (6, inset by half the pen).
        assert_eq!(groove.radius, 2.5);
        let fill = all
            .iter()
            .find(|p| p.stroke == 0.0 && p.screen == inset(g.fill))
            .unwrap_or_else(|| panic!("{scheme}: no fill in {all:?}"));
        assert!(
            near(fill.color, sample(scheme, "slider.normal", 10, 10)),
            "{scheme}: the fill {:?}",
            fill.color
        );
        let circles: Vec<_> = all
            .iter()
            .filter(|p| p.kind == ShapeKind::Circle && p.screen[2] == 17.0)
            .collect();
        let face = circles
            .iter()
            .find(|p| p.stroke == 0.0)
            .unwrap_or_else(|| panic!("{scheme}: no handle face"));
        assert_eq!(
            face.color,
            sample(scheme, "slider.normal", 63, 10),
            "{scheme}"
        );
        assert_eq!(
            [face.screen[0], face.screen[1]],
            [g.handle.x + 1.5, g.handle.y + 1.5],
            "{scheme}: the box inset by 1, then stroked"
        );
        let outline = rgba8_of(theme.colors.border_strong);
        assert!(
            circles
                .iter()
                .any(|p| p.stroke == 1.0 && p.color == outline),
            "{scheme}: no lifted outline in {circles:?}"
        );
        assert_ne!(
            sample(scheme, "slider.normal", 63, 1),
            outline,
            "{scheme}: the premise of D6"
        );
        assert!(
            circles.iter().any(|p| p.stroke == 1.0 && p.color == SHADOW),
            "{scheme}: no shadow ring"
        );
    }
}

/// Hovered, the handle is outlined in the hover colour; focused from the
/// keyboard, in the lifted focus colour, with the focus frame around it;
/// dragged, it casts no shadow.
#[test]
fn a_slider_handle_shows_hover_focus_and_press() {
    use teksilo_core::environment::LayoutDirection;
    let ltr = LayoutDirection::LeftToRight;
    for (scheme, theme) in themes() {
        let hover = theme
            .extension::<KdePalette>()
            .unwrap()
            .colors(teksilo_theme_kde::scheme::ColorState::Active)
            .hover;
        let (mut tree, root, _) = slider_at(&theme, 0.3, ltr, |v| Slider::new(v, 0.0, 1.0));
        let (_, g) = slider_parts(&tree, root, 0.3, ltr);
        tree.pointer_move(g.handle.center());
        let frame = rerender(&mut tree);
        assert!(
            painted(&frame).iter().any(|p| p.kind == ShapeKind::Circle
                && p.stroke == 1.0
                && p.color == rgba8_of(hover)),
            "{scheme}: no hover outline"
        );
        tree.pointer_down_button(g.handle.center(), PointerButton::Primary);
        let c = g.handle.center();
        tree.pointer_move(teksilo_canvas::Point::new(c.x + 12.0, c.y));
        let frame = rerender(&mut tree);
        assert!(
            !strokes(&frame).contains(&SHADOW),
            "{scheme}: a dragged handle casts no shadow"
        );
        tree.pointer_up_button(g.handle.center(), PointerButton::Primary);

        let (mut tree, root, _) = slider_at(&theme, 0.3, ltr, |v| Slider::new(v, 0.0, 1.0));
        let (_, g) = slider_parts(&tree, root, 0.3, ltr);
        tree.press_key(Key::Tab, Modifiers::NONE);
        let frame = rerender(&mut tree);
        let focus = rgba8_of(theme.colors.border_focused);
        assert!(
            painted(&frame)
                .iter()
                .any(|p| p.kind == ShapeKind::Circle && p.stroke == 1.0 && p.color == focus),
            "{scheme}: no focus outline"
        );
        let ring = rgba8_of(theme.colors.focus_ring);
        let frame_ring = painted(&frame)
            .into_iter()
            .find(|p| p.color == ring && p.stroke == 2.0)
            .unwrap_or_else(|| panic!("{scheme}: no focus frame"));
        assert_eq!(
            frame_ring.screen,
            [g.handle.x, g.handle.y, 20.0, 20.0],
            "{scheme}: from 1 px inside the box to 1 px outside it"
        );
    }
}

/// Disabled, only the groove's background part is painted, in the disabled
/// colours, and the handle is the disabled button colour.
#[test]
fn a_disabled_slider_paints_only_the_groove_in_the_disabled_colours() {
    use teksilo_core::environment::LayoutDirection;
    let ltr = LayoutDirection::LeftToRight;
    for (scheme, theme) in themes() {
        let (mut tree, root, _) = slider_at(&theme, 0.3, ltr, |v| {
            Slider::new(v, 0.0, 1.0).enabled(false)
        });
        let (_, g) = slider_parts(&tree, root, 0.3, ltr);
        let frame = rerender(&mut tree);
        let all = painted(&frame);
        assert!(
            !all.iter().any(|p| p.screen == inset(g.fill)),
            "{scheme}: no fill part"
        );
        let groove = all
            .iter()
            .find(|p| p.stroke == 0.0 && p.screen == inset(g.groove))
            .unwrap_or_else(|| panic!("{scheme}: no groove"));
        assert!(
            near(groove.color, sample(scheme, "slider.disabled", 190, 10)),
            "{scheme}: {:?}",
            groove.color
        );
        assert!(
            all.iter().any(|p| p.kind == ShapeKind::Circle
                && p.stroke == 0.0
                && p.color == sample(scheme, "slider.disabled", 63, 10)),
            "{scheme}: the disabled handle"
        );
    }
}

/// A vertical slider has its minimum at the bottom, and its groove is
/// filled from the top down to the handle's centre, as Breeze fills a
/// vertical slider that is not inverted (spec k.5.10); a press puts the
/// value where the handle is then painted.
#[test]
fn a_vertical_slider_fills_from_the_top_to_the_handle() {
    use teksilo_core::environment::LayoutDirection;
    use teksilo_core::styles::SliderOrientation;
    use teksilo_tokens::Orientation;
    let ltr = LayoutDirection::LeftToRight;
    for (scheme, theme) in themes() {
        let value = Signal::new(0.25_f32);
        let mut tree = tree_with(theme.clone());
        let root = tree.add(
            VStack::new().child(
                teksilo_widgets::primitives::FixedSize::new()
                    .height(200.0)
                    .child(
                        Slider::new(value.clone(), 0.0, 1.0)
                            .orientation(Orientation::Vertical)
                            .jump_on_press(true),
                    ),
            ),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let (_, b) = the_node(&tree, root, "BreezeSliderBody");
        assert_eq!(b.height, 200.0, "{scheme}");
        assert_eq!(b.width, 24.0, "{scheme}: raised to the target");
        let g = teksilo_theme_kde::styles::slider::slider_geometry(
            b,
            0.25,
            SliderOrientation::Vertical,
            ltr,
            false,
        );
        assert_eq!(
            g.handle.center().y,
            b.y + b.height - 10.0 - 180.0 * 0.25,
            "{scheme}: a quarter up from the bottom"
        );
        let frame = rerender(&mut tree);
        let fill = painted(&frame)
            .into_iter()
            .find(|p| p.stroke == 0.0 && p.screen == inset(g.fill))
            .unwrap_or_else(|| panic!("{scheme}: no fill"));
        assert_eq!(fill.screen[1], b.y + 0.5, "{scheme}: from the top");
        assert_eq!(
            fill.screen[1] + fill.screen[3],
            g.handle.center().y - 0.5,
            "{scheme}: down to the handle's centre"
        );
        assert!(
            near(fill.color, sample(scheme, "slider.normal", 10, 10)),
            "{scheme}: the fill {:?}",
            fill.color
        );

        // 55 px down from the top, a quarter of the 180 px travel past the
        // handle's half from the maximum: 75 %.
        let at = teksilo_canvas::Point::new(b.x + b.width * 0.5, b.y + 55.0);
        tree.pointer_down_button(at, PointerButton::Primary);
        tree.pointer_up_button(at, PointerButton::Primary);
        let v = value.get();
        assert!(
            (v - 0.75).abs() < 1e-4,
            "{scheme}: the top is the high end, {v}"
        );
        let frame = rerender(&mut tree);
        let face = painted(&frame)
            .into_iter()
            .find(|p| p.kind == ShapeKind::Circle && p.stroke == 0.0 && p.screen[2] == 17.0)
            .unwrap_or_else(|| panic!("{scheme}: no handle"));
        assert!(
            (face.screen[1] + 8.5 - at.y).abs() < 0.01,
            "{scheme}: the handle is under the pointer"
        );
        let g = teksilo_theme_kde::styles::slider::slider_geometry(
            b,
            v,
            SliderOrientation::Vertical,
            ltr,
            false,
        );
        let fill = painted(&frame)
            .into_iter()
            .find(|p| p.stroke == 0.0 && p.screen == inset(g.fill))
            .unwrap_or_else(|| panic!("{scheme}: no fill at {v}"));
        assert_eq!(fill.screen[3], 55.0 - 1.0, "{scheme}: the top 55 px, inset");
    }
}

/// Right to left, the handle, the fill and the ticks run from the right,
/// and a press puts the value where the handle is then painted: the paint
/// and the `Slider`'s pointer mapping agree.
#[test]
fn right_to_left_mirrors_the_slider_and_keeps_the_handle_under_the_pointer() {
    use teksilo_core::environment::LayoutDirection;
    let rtl = LayoutDirection::RightToLeft;
    for (scheme, theme) in themes() {
        let (mut tree, root, value) = slider_at(&theme, 0.25, rtl, |v| {
            Slider::new(v, 0.0, 1.0).jump_on_press(true)
        });
        let (b, g) = slider_parts(&tree, root, 0.25, rtl);
        assert_eq!(
            g.handle.x,
            b.x + b.width - 20.0 - (b.width - 20.0) * 0.25,
            "{scheme}: a quarter in from the right"
        );
        let frame = rerender(&mut tree);
        let fill = painted(&frame)
            .into_iter()
            .find(|p| p.stroke == 0.0 && p.screen == inset(g.fill))
            .unwrap_or_else(|| panic!("{scheme}: no mirrored fill"));
        assert_eq!(fill.screen[0] + fill.screen[2], b.x + b.width - 0.5);

        // 55 px in from the left, a quarter of the 180 px travel past the
        // handle's half: 75 %, on the slider's step.
        let at = teksilo_canvas::Point::new(b.x + 55.0, b.y + b.height * 0.5);
        tree.pointer_down_button(at, PointerButton::Primary);
        tree.pointer_up_button(at, PointerButton::Primary);
        let v = value.get();
        assert!(
            (v - 0.75).abs() < 1e-4,
            "{scheme}: the left is the high end, {v}"
        );
        let frame = rerender(&mut tree);
        let (_, g) = slider_parts(&tree, root, v, rtl);
        let face = painted(&frame)
            .into_iter()
            .find(|p| p.kind == ShapeKind::Circle && p.stroke == 0.0 && p.screen[2] == 17.0)
            .unwrap_or_else(|| panic!("{scheme}: no handle"));
        assert!(
            (face.screen[0] + 8.5 - at.x).abs() < 0.01,
            "{scheme}: the handle is under the pointer"
        );
        assert!((g.handle.center().x - at.x).abs() < 0.01);
    }

    // The ticks of a discrete slider mirror with it: the minimum's tick is at
    // the right, in the highlight up to the value.
    let theme = kde::light();
    let (mut tree, root, _) =
        slider_at(&theme, 0.5, rtl, |v| Slider::new(v, 0.0, 1.0).tick_count(3));
    let (_, b) = the_node(&tree, root, "BreezeSliderBody");
    let frame = rerender(&mut tree);
    let colors =
        teksilo_theme_kde::styles::slider::BreezeSliderColors::resolve(&theme, true, false, false);
    let mut ticks: Vec<([u8; 4], f32)> = frame
        .decorations
        .iter()
        .filter(|d| d.rect[2] == 1.0 && d.rect[3] == 8.0)
        .map(|d| (rgba8(d.color), d.rect[0] + 0.5 - b.x))
        .collect();
    ticks.sort_by(|a, b| a.1.total_cmp(&b.1));
    assert_eq!(
        ticks,
        vec![
            (rgba8_of(colors.tick_off), 10.0),
            (rgba8_of(colors.tick_on), 100.0),
            (rgba8_of(colors.tick_on), 190.0),
        ]
    );
}

// ── Menus, popups and tooltips ──────────────────────────────────────────

/// A menu of every kind of row Breeze has, in a 200 px column.
fn menu() -> MenuList {
    MenuList::new()
        .item(MenuItem::new(lit!("Open")))
        .item(MenuItem::new(lit!("Save")).shortcut_label("Ctrl+S"))
        .separator()
        .header(TextWidget::new(lit!("Section")))
        .item(MenuItem::new(lit!("Checkable")).checked(Signal::new(true)))
        .item(MenuItem::new(lit!("Option")).radio(0, Signal::new(0_usize)))
        .item(MenuItem::submenu(lit!("More"), || {
            Box::new(MenuList::new().item(MenuItem::new(lit!("Inner"))))
        }))
        .item(MenuItem::new(lit!("Unavailable")).enabled(false))
}

/// A tree holding [`menu`] 200 px wide, laid out: the tree, the menu's id
/// and the ids of its rows, in order.
fn menu_tree(theme: Theme) -> (WidgetTree, WidgetId, Vec<WidgetId>) {
    let mut tree = tree_with(theme);
    let column = tree.add(VStack::new().child(FixedSize::new().width(200.0).child(menu())));
    tree.layout(SizeProposal::exact(400.0, 400.0));
    let rows = nodes_named(&tree, column, "MenuItem");
    (tree, column, rows)
}

/// Builds the theme's popover style around a label, as a popover does.
#[derive(Debug)]
struct PopoverProbe {
    variant: PopoverVariant,
    root: Option<WidgetId>,
}

impl Widget for PopoverProbe {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let content = ctx.add(TextWidget::new(lit!("Content")));
        let style = ctx
            .theme()
            .style_slots
            .popover
            .clone()
            .expect("the preset installs a popover style");
        let root = style.make_body(
            &PopoverStyleConfig {
                content,
                variant: self.variant,
                name: "Probe".to_string(),
                placement: OverlayPlacement::Below,
                show_caret: true,
                caret_size: 6.0,
            },
            ctx,
        );
        self.root = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root.into_iter().collect()
    }
}

fn popover(variant: PopoverVariant) -> PopoverProbe {
    PopoverProbe {
        variant,
        root: None,
    }
}

#[test]
fn menus_popovers_and_tooltips_mount_in_every_variant() {
    for (_, theme) in themes() {
        assert!(mount(theme.clone(), menu()) > 0);
        assert!(mount(theme.clone(), TooltipWidget::new(lit!("Tooltip text"))) > 0);
        for variant in [
            PopoverVariant::Default,
            PopoverVariant::Menu,
            PopoverVariant::Tooltip,
        ] {
            assert!(mount(theme.clone(), popover(variant)) > 0, "{variant:?}");
        }
        let (mut tree, _, rows) = menu_tree(theme.clone());
        // Hover, then press, every row: none panics, each paints.
        for row in rows {
            let at = tree.bounds(row).center();
            tree.pointer_move(at);
            assert!(!rerender(&mut tree).draw_order.is_empty());
            tree.pointer_down_button(at, PointerButton::Primary);
            assert!(!rerender(&mut tree).draw_order.is_empty());
            tree.pointer_up_button(at, PointerButton::Primary);
        }
    }
}

#[test]
fn menus_and_tooltips_survive_a_theme_swap_and_a_missing_palette() {
    let mut tree = tree_with(kde::light());
    tree.add(
        VStack::new()
            .child(menu())
            .child(TooltipWidget::new(lit!("Tooltip text")))
            .child(popover(PopoverVariant::Default)),
    );
    let mut bare = kde::light();
    bare.extensions.remove::<KdePalette>();
    for theme in [
        kde::light(),
        kde::dark(),
        teksilo_core::presets::intui::light(),
        bare,
        kde::light(),
    ] {
        tree.set_theme(theme);
        assert!(!rerender(&mut tree).draw_order.is_empty());
    }
    for accent in PlasmaAccent::ALL {
        for theme in [
            kde::light_with_accent(accent.color()),
            kde::dark_with_accent(accent.color()),
        ] {
            assert!(mount(theme, menu()) > 0, "{accent:?}");
        }
    }
}

/// The menu's frame is Plasma's: the frame background in the frame outline,
/// over both layers of Breeze's Large window shadow on the 5 px window.
#[test]
fn a_menu_is_framed_as_breezes_popup_menu() {
    for (scheme, theme) in themes() {
        let (mut tree, column, _) = menu_tree(theme);
        let (_, frame_bounds) = the_node(&tree, column, "BreezePopupFrame");
        let frame = rerender(&mut tree);
        let all = painted(&frame);
        let panel = inset(frame_bounds);
        let face = all
            .iter()
            .find(|p| p.stroke == 0.0 && p.screen == panel)
            .unwrap_or_else(|| panic!("{scheme}: no panel face in {all:?}"));
        assert_eq!(
            face.color,
            sample(scheme, "menu.panel", 100, 50),
            "{scheme}"
        );
        assert_eq!(face.radius, 4.5);
        let outline = all
            .iter()
            .find(|p| p.stroke == 1.0 && p.screen == panel)
            .unwrap_or_else(|| panic!("{scheme}: no panel outline"));
        assert_eq!(
            outline.color,
            sample(scheme, "menu.panel", 0, 50),
            "{scheme}"
        );
        assert_window_shadow(&frame, frame_bounds, scheme);
    }
}

/// Both layers of Breeze's Large shadow, on the 5 px window `bounds`.
fn assert_window_shadow(frame: &RenderFrame, bounds: teksilo_canvas::Rect, scheme: &str) {
    let window = [bounds.x, bounds.y, bounds.width, bounds.height];
    let layers: Vec<_> = frame
        .shadows
        .iter()
        .filter(|s| s.shape_rect == window)
        .collect();
    assert_eq!(layers.len(), 2, "{scheme}: two shadow layers");
    for (layer, (offset, blur, alpha)) in layers.iter().zip([(5.0, 20.0, 0.22), (2.0, 10.0, 0.12)])
    {
        assert_eq!(layer.blur_radius, blur, "{scheme}");
        assert!((layer.color[3] - alpha).abs() < 1e-6, "{scheme}");
        assert_eq!(&layer.color[..3], &[0.0, 0.0, 0.0], "{scheme}: black");
        assert_eq!(layer.corner_radii, [5.0; 4], "{scheme}");
        // The quad covers the blur around the window moved down by the
        // layer's offset.
        assert_eq!(layer.screen[1], bounds.y + offset - blur, "{scheme}");
        assert_eq!(layer.screen[0], bounds.x - blur, "{scheme}");
    }
}

/// Every row sits 4 px inside the frame, Breeze's menu margin, and is 29 px
/// tall; a separator is 5 px.
#[test]
fn a_menus_rows_sit_in_breezes_margin() {
    let (tree, column, rows) = menu_tree(kde::light());
    let (_, frame) = the_node(&tree, column, "BreezePopupFrame");
    let first = tree.bounds(rows[0]);
    assert_eq!((first.x - frame.x, first.y - frame.y), (4.0, 4.0));
    assert_eq!(first.width, frame.width - 8.0);
    for row in &rows {
        assert_eq!(tree.bounds(*row).height, 29.0);
    }
    let (_, separator) = the_node(&tree, column, "MenuSeparator");
    assert_eq!(separator.height, 5.0);
    assert_eq!(separator.y, tree.bounds(rows[1]).bottom());
}

/// The highlighted row's rectangle and the shapes painted on it.
fn row_shapes(frame: &RenderFrame, row: teksilo_canvas::Rect) -> Vec<Painted> {
    painted(frame)
        .into_iter()
        .filter(|p| p.screen == inset(row))
        .collect()
}

/// Under the pointer a row is washed with the focus colour at 30 % and
/// outlined at radius 4.5 on the row inset by half the pen: Plasma's
/// highlight. Breeze Dark's outline is Plasma's; Breeze Light's is lifted to
/// 3:1 on the menu (D5), its own falling short.
#[test]
fn a_hovered_menu_row_is_breezes_highlight() {
    for (scheme, theme) in themes() {
        let window = sample(scheme, "menu_item.normal", 100, 14);
        let (mut tree, _, rows) = menu_tree(theme.clone());
        let row = tree.bounds(rows[0]);
        assert!(
            row_shapes(&rerender(&mut tree), row).is_empty(),
            "{scheme}: at rest"
        );

        tree.pointer_move(row.center());
        let frame = rerender(&mut tree);
        let shapes = row_shapes(&frame, row);
        let wash = shapes
            .iter()
            .find(|p| p.stroke == 0.0)
            .unwrap_or_else(|| panic!("{scheme}: no wash in {shapes:?}"));
        assert!(
            near(
                over(wash.color, window),
                sample(scheme, "menu_item.selected", 100, 14)
            ),
            "{scheme}: the wash {:?}",
            wash.color
        );
        assert_eq!(wash.radius, 4.5);
        let outline = shapes
            .iter()
            .find(|p| p.stroke == 1.0)
            .unwrap_or_else(|| panic!("{scheme}: no outline in {shapes:?}"));
        assert_eq!(outline.radius, 4.5);
        let plasma = sample(scheme, "menu_item.selected", 100, 0);
        let panel = theme.colors.surface_raised;
        let contrast = |c: [u8; 4]| {
            teksilo_tokens::contrast::composite_contrast(
                Color::new(
                    f32::from(c[0]) / 255.0,
                    f32::from(c[1]) / 255.0,
                    f32::from(c[2]) / 255.0,
                    f32::from(c[3]) / 255.0,
                ),
                panel,
            )
        };
        if scheme == "BreezeDark" {
            assert_eq!(outline.color, plasma, "{scheme}: Breeze's own outline");
        } else {
            assert!(contrast(plasma) < 3.0, "{scheme}: the premise of D5");
            assert_ne!(outline.color, plasma);
        }
        assert!(contrast(outline.color) >= 3.0, "{scheme}");
        // The label stays the window text while highlighted.
        let text = rgba8_of(theme.colors.text_primary);
        assert!(frame.glyphs.iter().any(|g| rgba8(g.color) == text));
    }
}

/// Held down, the row is filled and outlined with the accent and its label
/// turns to the text on it.
#[test]
fn a_pressed_menu_row_is_filled_and_its_label_flips() {
    for (scheme, theme) in themes() {
        let (mut tree, _, rows) = menu_tree(theme.clone());
        let row = tree.bounds(rows[1]);
        tree.pointer_move(row.center());
        tree.pointer_down_button(row.center(), PointerButton::Primary);
        let frame = rerender(&mut tree);
        let shapes = row_shapes(&frame, row);
        let accent = rgba8_of(theme.colors.accent);
        assert!(
            shapes.iter().any(|p| p.stroke == 0.0 && p.color == accent),
            "{scheme}: {shapes:?}"
        );
        assert!(shapes.iter().any(|p| p.stroke == 1.0 && p.color == accent));
        let on_accent = rgba8_of(theme.colors.text_on_accent);
        let in_row: Vec<_> = frame
            .glyphs
            .iter()
            .filter(|g| g.screen[1] >= row.y && g.screen[1] < row.bottom())
            .map(|g| rgba8(g.color))
            .collect();
        assert!(!in_row.is_empty());
        assert!(
            in_row.iter().all(|c| *c == on_accent),
            "{scheme}: the label and its shortcut on the fill, {in_row:?}"
        );
        tree.pointer_up_button(row.center(), PointerButton::Primary);
    }
}

/// A disabled row is never highlighted.
#[test]
fn a_disabled_menu_row_is_not_highlighted() {
    let (mut tree, _, rows) = menu_tree(kde::light());
    let row = tree.bounds(*rows.last().unwrap());
    tree.pointer_move(row.center());
    assert!(row_shapes(&rerender(&mut tree), row).is_empty());
}

/// A tooltip is Plasma's: the tooltip background in its outline, over the
/// window shadow, the text 4 px inside the frame.
#[test]
fn a_tooltip_is_framed_as_breezes() {
    for (scheme, theme) in themes() {
        let mut tree = tree_with(theme);
        let root = tree.add(
            VStack::new().child(HStack::new().child(TooltipWidget::new(lit!("Tooltip text")))),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let (frame_id, bounds) = the_node(&tree, root, "BreezePopupFrame");
        let text = tree.bounds(tree.children(frame_id)[0]);
        assert_eq!((text.x - bounds.x, text.y - bounds.y), (4.0, 4.0));
        assert_eq!(bounds.height, text.height + 8.0);
        let frame = rerender(&mut tree);
        let all = painted(&frame);
        let face = all
            .iter()
            .find(|p| p.stroke == 0.0 && p.screen == inset(bounds))
            .unwrap_or_else(|| panic!("{scheme}: no face in {all:?}"));
        assert_eq!(
            face.color,
            sample(scheme, "tooltip.panel", 50, 15),
            "{scheme}"
        );
        assert_eq!(face.radius, 4.5);
        let outline = all
            .iter()
            .find(|p| p.stroke == 1.0 && p.screen == inset(bounds))
            .unwrap_or_else(|| panic!("{scheme}: no outline"));
        assert_eq!(
            outline.color,
            sample(scheme, "tooltip.panel", 0, 15),
            "{scheme}"
        );
        assert_window_shadow(&frame, bounds, scheme);
    }
}

/// A popover that is not a menu pads its content 4 px and draws no caret;
/// its `Tooltip` variant takes the tooltip's colours, and a menu's frame
/// has no padding.
#[test]
fn a_popover_is_a_breeze_popup_with_no_caret() {
    for (variant, padding, face) in [
        (PopoverVariant::Default, 4.0, "menu.panel"),
        (PopoverVariant::Tooltip, 4.0, "tooltip.panel"),
        (PopoverVariant::Menu, 0.0, "menu.panel"),
    ] {
        for (scheme, theme) in themes() {
            let (mut tree, root) = placed(theme, popover(variant));
            let (frame_id, bounds) = the_node(&tree, root, "BreezePopupFrame");
            let content = tree.bounds(tree.children(frame_id)[0]);
            assert_eq!(content.x - bounds.x, padding, "{variant:?}");
            assert_eq!(content.y - bounds.y, padding, "{variant:?}: no caret above");
            let frame = rerender(&mut tree);
            assert!(frame.paths.is_empty(), "{variant:?}: a caret was drawn");
            let all = painted(&frame);
            let fill_at = if face == "tooltip.panel" {
                (50, 15)
            } else {
                (100, 50)
            };
            assert!(
                all.iter().any(|p| p.stroke == 0.0
                    && p.screen == inset(bounds)
                    && p.color == sample(scheme, face, fill_at.0, fill_at.1)),
                "{scheme} {variant:?}: {all:?}"
            );
        }
    }
}

// ── Rows, tables, tabs, scroll bars and progress bars ───────────────────

/// Every plain rectangle in `frame` (`Canvas::fill_rect`): its colour in 8
/// bits and its rectangle.
fn decorations(frame: &RenderFrame) -> Vec<([u8; 4], [f32; 4])> {
    frame
        .decorations
        .iter()
        .map(|d| (rgba8(d.color), d.rect))
        .collect()
}

/// A two-tab bar, the first tab selected, laid out at its own height.
fn tab_bar(vertical: bool) -> TabBar<(TabId, &'static str)> {
    let first = TabId::fresh();
    let model = ListModel::from_vec(vec![(first, "Tab"), (TabId::fresh(), "Second")]);
    let delegate = TabDelegate::new(|_, t: &(TabId, &'static str)| lit!(t.1));
    let selected: Signal<Option<TabId>> = Signal::new(Some(first));
    let bar = if vertical {
        TabBar::vertical(model, delegate, selected, |_, t| t.0)
    } else {
        TabBar::horizontal(model, delegate, selected, |_, t| t.0)
    };
    bar.tab_sizing(TabSizing::Independent)
        .show_scroll_arrows(false)
        .show_overflow_dropdown(false)
}

fn name_column<T: 'static>(label: fn(&T) -> String) -> Column<T> {
    Column::new("name", lit!("Name"), move |row: &T, _| {
        Box::new(TextWidget::new(lit!(label(row))))
    })
    .width(ColumnWidth::Fixed(160.0))
}

fn table() -> TableView<String> {
    let model = ListModel::from_vec(vec!["alpha".to_string(), "beta".into(), "gamma".into()]);
    TableView::new(model).add_column(name_column(|s: &String| s.clone()))
}

fn tree_table() -> TreeTableView<String> {
    let model: TreeModel<String> = TreeModel::new();
    for (i, name) in ["alpha", "beta"].into_iter().enumerate() {
        model.insert_root(i, name.to_string());
    }
    TreeTableView::new(model).add_column(name_column(|s: &String| s.clone()))
}

fn scroll_bar(orientation: ScrollBarOrientation, variant: ScrollBarVariant) -> ScrollBar {
    ScrollBar::new(
        orientation,
        Signal::new(0.0),
        Signal::new(100.0),
        Signal::new(0.16),
    )
    .thickness(21.0)
    .variant(variant)
}

/// Every widget this section restyles, in a column, for the swap, the
/// palette-less and the swatch tests.
fn rows_and_bars() -> VStack {
    VStack::new()
        .child(
            FixedSize::new()
                .width(300.0)
                .child(StandardListItem::new(lit!("Row"))),
        )
        .child(
            FixedSize::new()
                .width(300.0)
                .child(StandardListItem::new(lit!("Picked")).selected(Signal::new(true))),
        )
        .child(
            FixedSize::new()
                .width(300.0)
                .child(StandardTreeItem::new(lit!("Node")).depth(1)),
        )
        .child(FixedSize::new().width(300.0).height(120.0).child(table()))
        .child(tab_bar(false))
        .child(FixedSize::new().width(21.0).height(100.0).child(scroll_bar(
            ScrollBarOrientation::Vertical,
            ScrollBarVariant::Permanent,
        )))
        .child(FixedSize::new().width(200.0).child(ProgressBar::new(0.4)))
        .child(
            FixedSize::new()
                .width(200.0)
                .child(ProgressBar::indeterminate()),
        )
}

#[test]
fn rows_tables_tabs_and_bars_mount_in_every_state_and_variant() {
    for (_, theme) in themes() {
        for row in [
            StandardListItem::new(lit!("Row")),
            StandardListItem::new(lit!("Row")).selected(Signal::new(true)),
            StandardListItem::new(lit!("Row")).checkbox(Signal::new(true)),
            StandardListItem::new(lit!("Row"))
                .leading_slot(IconWidget::checkmark(16.0))
                .subtitle(lit!("Below")),
            StandardListItem::new(lit!("Row"))
                .selected(Signal::new(true))
                .enabled(Signal::new(false)),
        ] {
            assert!(mount(theme.clone(), FixedSize::new().width(300.0).child(row)) > 0);
        }
        assert!(
            mount(
                theme.clone(),
                FixedSize::new()
                    .width(300.0)
                    .child(StandardTreeItem::new(lit!("Node")).selected(Signal::new(true)))
            ) > 0
        );
        assert!(mount(theme.clone(), table()) > 0);
        assert!(mount(theme.clone(), tree_table()) > 0);
        for vertical in [false, true] {
            assert!(mount(theme.clone(), tab_bar(vertical)) > 0);
            assert!(
                mount(
                    theme.clone(),
                    tab_bar(vertical).active_indicator(TabIndicatorPosition::InnerEdge)
                ) > 0
            );
        }
        for orientation in [
            ScrollBarOrientation::Vertical,
            ScrollBarOrientation::Horizontal,
        ] {
            for variant in [
                ScrollBarVariant::Permanent,
                ScrollBarVariant::Overlay,
                ScrollBarVariant::Thin,
            ] {
                assert!(mount(theme.clone(), scroll_bar(orientation, variant)) > 0);
            }
        }
        for orientation in [Orientation::Horizontal, Orientation::Vertical] {
            assert!(
                mount(
                    theme.clone(),
                    ProgressBar::new(0.4).orientation(orientation)
                ) > 0
            );
            assert!(
                mount(
                    theme.clone(),
                    ProgressBar::indeterminate().orientation(orientation)
                ) > 0
            );
        }
    }
}

#[test]
fn the_rows_and_bars_survive_a_theme_swap_and_every_swatch() {
    let mut tree = tree_with(kde::light());
    tree.add(rows_and_bars());
    for theme in [
        kde::light(),
        kde::dark(),
        teksilo_core::presets::intui::light(),
        kde::light(),
    ] {
        tree.set_theme(theme);
        assert!(!rerender(&mut tree).draw_order.is_empty());
    }
    for accent in PlasmaAccent::ALL {
        for theme in [
            kde::light_with_accent(accent.color()),
            kde::dark_with_accent(accent.color()),
        ] {
            assert!(mount(theme, rows_and_bars()) > 0, "{accent:?}");
        }
    }
    let mut bare = kde::light();
    bare.extensions.remove::<KdePalette>();
    assert!(mount(bare, rows_and_bars()) > 0);
}

/// A row of `width` holding `row`, alone at the top of a tree under `theme`.
fn row_tree(theme: Theme, width: f32, row: StandardListItem) -> (WidgetTree, WidgetId) {
    let mut tree = tree_with(theme);
    let column = tree.add(VStack::new().child(FixedSize::new().width(width).child(row)));
    tree.layout(SizeProposal::exact(600.0, 300.0));
    let id = nodes_named(&tree, column, "StandardListItem")[0];
    (tree, id)
}

/// The selection inside the row `b`: the row inset by the 2 px and 1 px
/// item margins, then by half the pen.
fn row_selection(b: teksilo_canvas::Rect) -> [f32; 4] {
    [b.x + 2.5, b.y + 1.5, b.width - 5.0, b.height - 3.0]
}

/// A selected row is filled and outlined with Plasma's highlight inside its
/// item margins, its label in the text on the accent, and the selection
/// spans the whole width the row is given.
#[test]
fn a_selected_row_is_the_highlight_with_on_accent_glyphs() {
    for (scheme, theme) in themes() {
        let accent = rgba8_of(theme.colors.accent);
        assert_eq!(accent, sample(scheme, "item_view_row.selected", 100, 12));
        let on_accent = rgba8_of(theme.colors.text_on_accent);
        let primary = rgba8_of(theme.colors.text_primary);
        assert_ne!(on_accent, primary, "the flip is visible");
        for width in [200.0, 480.0] {
            let (mut tree, id) = row_tree(
                theme.clone(),
                width,
                StandardListItem::new(lit!("Row")).selected(Signal::new(true)),
            );
            let b = tree.bounds(id);
            assert_eq!((b.width, b.height), (width, 24.0), "{scheme}: the row");
            let frame = rerender(&mut tree);
            let all = painted(&frame);
            let fill = all
                .iter()
                .find(|p| p.stroke == 0.0 && p.color == accent)
                .unwrap_or_else(|| panic!("{scheme}: no highlight in {all:?}"));
            assert_eq!(fill.screen, row_selection(b), "{scheme}: fills the row");
            assert_eq!(fill.radius, 4.5);
            assert!(
                all.iter()
                    .any(|p| p.stroke == 1.0 && p.color == accent && p.screen == fill.screen),
                "{scheme}: the opaque outline"
            );
            let glyphs: Vec<_> = frame.glyphs.iter().map(|g| rgba8(g.color)).collect();
            assert!(!glyphs.is_empty());
            assert!(
                glyphs.iter().all(|g| *g == on_accent),
                "{scheme}: {glyphs:?}"
            );
        }
    }
}

/// An unselected row at rest paints nothing but its label.
#[test]
fn an_unselected_row_paints_nothing() {
    for (scheme, theme) in themes() {
        let (mut tree, _) = row_tree(theme.clone(), 300.0, StandardListItem::new(lit!("Row")));
        let frame = rerender(&mut tree);
        assert!(
            painted(&frame).iter().all(|p| p.color[3] == 0),
            "{scheme}: {:?}",
            painted(&frame)
        );
        assert!(
            decorations(&frame).iter().all(|(c, _)| c[3] == 0),
            "{scheme}: {:?}",
            decorations(&frame)
        );
        assert!(frame.paths.is_empty());
        let primary = rgba8_of(theme.colors.text_primary);
        assert!(frame.glyphs.iter().all(|g| rgba8(g.color) == primary));
    }
}

/// A hovered row is washed with the highlight at 30 % over the view and
/// outlined in it at 80 %, as Plasma paints it.
#[test]
fn a_hovered_row_is_washed_and_outlined_in_the_highlight() {
    for (scheme, theme) in themes() {
        let (mut tree, id) = row_tree(theme.clone(), 200.0, StandardListItem::new(lit!("Row")));
        tree.pointer_move(tree.bounds(id).center());
        let frame = rerender(&mut tree);
        let b = tree.bounds(id);
        let all = painted(&frame);
        let wash = all
            .iter()
            .find(|p| p.stroke == 0.0 && p.screen == row_selection(b))
            .unwrap_or_else(|| panic!("{scheme}: no wash in {all:?}"));
        assert!(
            near(wash.color, sample(scheme, "item_view_row.hover", 100, 12)),
            "{scheme}: {:?}",
            wash.color
        );
        let outline = all
            .iter()
            .find(|p| p.stroke == 1.0 && p.screen == row_selection(b))
            .expect("an outline");
        let mut want = rgba8_of(theme.colors.accent);
        want[3] = 204;
        assert_eq!(outline.color, want, "{scheme}: the highlight at 80 %");
    }
}

/// In a backdrop window a selected row is Plasma's inactive highlight, the
/// window colour tinted toward the selection, and its label the window
/// text.
#[test]
fn a_selected_row_in_a_backdrop_window_is_the_inactive_highlight() {
    for (scheme, theme) in themes() {
        let (mut tree, _) = row_tree(
            theme.clone(),
            200.0,
            StandardListItem::new(lit!("Row")).selected(Signal::new(true)),
        );
        tree.set_window_active(false);
        let frame = rerender(&mut tree);
        let want = sample(scheme, "item_view_row.selected_inactive_window", 100, 12);
        assert!(
            fills(&frame).contains(&want),
            "{scheme}: {:?}",
            fills(&frame)
        );
        let primary = rgba8_of(theme.colors.text_primary);
        assert!(frame.glyphs.iter().all(|g| rgba8(g.color) == primary));
    }
}

/// Every filled path in `frame`: its colour in 8 bits and the path.
fn filled_paths(frame: &RenderFrame) -> Vec<([u8; 4], Path)> {
    frame
        .paths
        .iter()
        .filter(|p| p.stroke_style.width == 0.0)
        .map(|p| (rgba8(p.color), p.path.clone()))
        .collect()
}

fn covers(path: &Path, x: f32, y: f32) -> bool {
    path.contains_point(teksilo_canvas::Point::new(x, y), FillRule::Winding, 0.01)
}

/// Whether `r` is `[x, y, w, h]` to a hundredth of a pixel.
fn about(r: teksilo_canvas::Rect, want: [f32; 4]) -> bool {
    [r.x, r.y, r.width, r.height]
        .iter()
        .zip(want)
        .all(|(a, b)| (a - b).abs() < 0.01)
}

/// The headers of a two-tab bar: the selected one first.
fn tab_headers(theme: Theme, vertical: bool) -> (WidgetTree, Vec<teksilo_canvas::Rect>) {
    let mut tree = tree_with(theme);
    let bar = tree.add(tab_bar(vertical));
    tree.layout(SizeProposal {
        width: Some(800.0),
        height: if vertical { Some(400.0) } else { None },
    });
    let headers = nodes_named(&tree, bar, "TabHeader")
        .into_iter()
        .map(|h| tree.bounds(h))
        .collect();
    (tree, headers)
}

/// The selected tab: the highlight in a 3 px bar along its top, its
/// rounded outer corners at the frame radius, the window colour below it
/// and the frame outline down its two sides, both still turning through the
/// bar's corner in rows 3 and 4; the unselected tab the window darkened, and
/// hovered washed toward the hover colour. Against Plasma's samples of an
/// 80 x 30 tab.
#[test]
fn the_selected_tab_has_breezes_three_pixel_bar() {
    for (scheme, theme) in themes() {
        let (mut tree, headers) = tab_headers(theme, false);
        let (sel, other) = (headers[0], headers[1]);
        assert_eq!(sel.height, 30.0, "{scheme}");
        let frame = tree.render();
        let paths = filled_paths(&frame);
        let rects = decorations(&frame);
        let highlight = sample(scheme, "tab.selected", 40, 1);
        let bar = &paths
            .iter()
            .find(|(color, _)| *color == highlight)
            .unwrap_or_else(|| panic!("{scheme}: no bar in {paths:?}"))
            .1;
        let bb = bar.exact_bounds(0.01);
        assert!((bb.y - sel.y).abs() < 0.01 && (bb.height - 3.0).abs() < 0.01);
        assert!(bb.x > sel.x && bb.x < sel.x + 0.5, "{scheme}: {bb:?}");
        assert!(covers(bar, sel.x + 40.0, sel.y + 1.5));
        assert!(!covers(bar, sel.x + 0.3, sel.y + 0.3), "the outer corner");
        let window = sample(scheme, "tab.selected", 40, 15);
        let face = &paths
            .iter()
            .find(|(color, _)| *color == window)
            .unwrap_or_else(|| panic!("{scheme}: no tab face in {paths:?}"))
            .1;
        assert!(
            about(
                face.bounds(),
                [sel.x + 1.0, sel.y + 3.0, sel.width - 2.0, sel.height - 3.0]
            ),
            "{scheme}: the bar shows 3 px: {:?}",
            face.bounds()
        );
        let outline = sample(scheme, "tab.selected", 0, 15);
        let sides = &paths
            .iter()
            .find(|(color, _)| near(*color, outline))
            .unwrap_or_else(|| panic!("{scheme}: no outline in {paths:?}"))
            .1;
        assert!(about(
            sides.bounds(),
            [sel.x, sel.y + 3.0, sel.width, sel.height - 3.0]
        ));
        for x in [sel.x + 0.5, sel.x + sel.width - 0.5] {
            assert!(
                covers(sides, x, sel.y + 15.0) && !covers(face, x, sel.y + 15.0),
                "{scheme}: the outline down both sides"
            );
        }
        // Rows 3 and 4: the corner is still turning, as Plasma's column 0
        // is still blended there; the body is not square below the bar.
        assert!(!covers(sides, sel.x + 0.1, sel.y + 3.1), "{scheme}");
        assert!(covers(sides, sel.x + 0.5, sel.y + 4.5), "{scheme}");
        assert!(!covers(face, sel.x + 0.9, sel.y + 4.5), "{scheme}");
        assert_ne!(
            sample(scheme, "tab.selected", 0, 4),
            outline,
            "{scheme}: Plasma's corner at row 4"
        );
        let other_rect = [other.x, other.y, other.width, other.height];
        let unselected = rects
            .iter()
            .find(|(_, r)| *r == other_rect)
            .unwrap_or_else(|| panic!("{scheme}: no unselected tab"));
        assert!(near(unselected.0, sample(scheme, "tab.unselected", 40, 15)));
        tree.pointer_move(other.center());
        tree.layout(SizeProposal {
            width: Some(800.0),
            height: None,
        });
        let frame = tree.render();
        let hovered = decorations(&frame)
            .into_iter()
            .find(|(_, r)| *r == other_rect)
            .unwrap();
        assert!(
            near(hovered.0, sample(scheme, "tab.hover", 40, 15)),
            "{scheme}: {:?}",
            hovered.0
        );
    }
}

/// A vertical bar's selected tab carries its bar on the leading side, the
/// left, and on the right in a right-to-left window.
#[test]
fn a_vertical_tabs_bar_is_on_its_leading_side() {
    for rtl in [false, true] {
        let mut tree = tree_with(kde::light());
        if rtl {
            tree.set_layout_direction(teksilo_core::environment::LayoutDirection::RightToLeft);
        }
        let bar = tree.add(tab_bar(true));
        tree.layout(SizeProposal::exact(400.0, 400.0));
        let sel = tree.bounds(nodes_named(&tree, bar, "TabHeader")[0]);
        let frame = tree.render();
        let window = sample("BreezeLight", "tab.selected", 40, 15);
        let (_, face) = filled_paths(&frame)
            .into_iter()
            .find(|(color, _)| *color == window)
            .expect("a tab face");
        let x = if rtl { sel.x } else { sel.x + 3.0 };
        assert!(
            about(
                face.bounds(),
                [x, sel.y + 1.0, sel.width - 3.0, sel.height - 2.0]
            ),
            "rtl {rtl}: {:?}",
            face.bounds()
        );
    }
}

/// A 21 px scroll bar: the separator in its leading column in the view's
/// text at the frame contrast, Plasma's pixels, and the handle 7 px from it
/// in the lifted thumb colour, 8 px wide painted 7 at radius 4, turning to
/// the lifted hover colour under the pointer. No track.
#[test]
fn the_scroll_bar_is_a_separator_and_a_handle_over_no_track() {
    for (scheme, theme) in themes() {
        let mut tree = tree_with(theme.clone());
        let bar = tree.add(scroll_bar(
            ScrollBarOrientation::Vertical,
            ScrollBarVariant::Permanent,
        ));
        tree.layout(SizeProposal::exact(21.0, 200.0));
        let frame = tree.render();
        let all = painted(&frame);
        let window = rgba8_of(theme.colors.surface_main);
        let separator = decorations(&frame)
            .into_iter()
            .find(|(_, r)| *r == [0.0, 0.0, 1.0, 200.0])
            .unwrap_or_else(|| panic!("{scheme}: no separator"));
        assert!(near(
            over(separator.0, window),
            sample(scheme, "scroll_bar.normal", 0, 150)
        ));
        let thumb = rgba8_of(theme.colors.scrollbar_thumb);
        let handle = all
            .iter()
            .find(|p| p.stroke == 0.0 && p.color == thumb)
            .unwrap_or_else(|| panic!("{scheme}: no handle"));
        assert_eq!(handle.screen, [7.5, 0.5, 7.0, 31.0], "{scheme}");
        // Breeze's radius 4, which the renderer holds to half the painted
        // width.
        assert_eq!(handle.radius, 3.5);
        assert!(
            all.iter()
                .any(|p| p.stroke == 1.0 && p.screen == handle.screen),
            "{scheme}: the handle's outline"
        );
        assert_eq!(all.len(), 2, "{scheme}: no track: {all:?}");
        assert_eq!(frame.decorations.len(), 1, "{scheme}: the separator alone");
        tree.pointer_move(tree.bounds(bar).center());
        tree.layout(SizeProposal::exact(21.0, 200.0));
        let frame = tree.render();
        let hover = rgba8_of(theme.colors.scrollbar_thumb_hover);
        assert!(fills(&frame).contains(&hover), "{scheme}");
    }
}

/// A determinate bar is Qt's 26 px, its 6 px groove 10 px down, the groove
/// and the contents to the half way point in Plasma's colours; right to
/// left the contents start at the right.
#[test]
fn the_progress_bar_is_breezes_groove_and_contents() {
    for (scheme, theme) in themes() {
        for rtl in [false, true] {
            let mut tree = tree_with(theme.clone());
            if rtl {
                tree.set_layout_direction(teksilo_core::environment::LayoutDirection::RightToLeft);
            }
            let column = tree.add(
                VStack::new().child(FixedSize::new().width(200.0).child(ProgressBar::new(0.5))),
            );
            tree.layout(SizeProposal::exact(400.0, 300.0));
            let bar = nodes_named(&tree, column, "ProgressBar")[0];
            let b = tree.bounds(bar);
            assert_eq!(b.height, 26.0, "{scheme}: Qt's hint");
            let frame = rerender(&mut tree);
            let all = painted(&frame);
            let groove = all
                .iter()
                .find(|p| p.stroke == 0.0 && p.screen == [b.x + 0.5, b.y + 10.5, 199.0, 5.0])
                .unwrap_or_else(|| panic!("{scheme}: no groove in {all:?}"));
            if scheme == "BreezeLight" {
                assert!(near(
                    groove.color,
                    sample(scheme, "progress_bar.half", 150, 9)
                ));
            }
            // Breeze's radius 3, held to half the stroked groove.
            assert_eq!(groove.radius, 2.5);
            let x = if rtl { b.x + 100.5 } else { b.x + 0.5 };
            let contents = all
                .iter()
                .find(|p| p.stroke == 0.0 && p.screen == [x, b.y + 10.5, 99.0, 5.0])
                .unwrap_or_else(|| panic!("{scheme} rtl {rtl}: no contents in {all:?}"));
            if scheme == "BreezeLight" {
                assert!(near(
                    contents.color,
                    sample(scheme, "progress_bar.half", 50, 9)
                ));
            }
            assert!(frame.animated_quads.is_empty());
        }
    }
}

/// A busy bar is the style's stripes, 14 px of the highlight every 28 px,
/// which move forward 7 px in 200 ms and 14 px in 400 ms on the tree's clock and stand still under
/// reduced motion. The bar draws no sweep of its own.
#[test]
fn a_busy_progress_bar_moves_its_stripes() {
    let theme = kde::light();
    let highlight = rgba8_of(theme.colors.accent);
    let stripes = |frame: &RenderFrame| -> Vec<[f32; 4]> {
        painted(frame)
            .into_iter()
            .filter(|p| p.stroke == 0.0 && p.color == highlight)
            .map(|p| p.screen)
            .collect()
    };
    let (mut tree, bar) = on_the_clock(
        theme.clone(),
        false,
        FixedSize::new()
            .width(200.0)
            .child(ProgressBar::indeterminate()),
    );
    let x0 = tree.bounds(bar).x;
    let first = stripes(&rerender(&mut tree));
    assert!(rerender(&mut tree).animated_quads.is_empty());
    assert_eq!(first[0][0] - x0, 0.0);
    assert_eq!(first[0][2], 14.0);
    assert_eq!(first[1][0] - first[0][0], 28.0);
    // A quarter of a period on, the stripes have moved 7 px forward, toward
    // the end of the groove: moving back would put a stripe at the start.
    let quarter = stripes(&tick(&mut tree, 200));
    assert!(
        (quarter[0][0] - x0 - 7.0).abs() < 1.0,
        "a quarter period on: {quarter:?}"
    );
    assert!((quarter[0][2] - 14.0).abs() < 1.0, "{quarter:?}");
    let later = stripes(&tick(&mut tree, 200));
    assert!(
        (later[0][0] - x0 - 14.0).abs() < 1.0 || (later[1][0] - x0 - 14.0).abs() < 1.0,
        "half a period on: {later:?}"
    );
    let (mut still, _) = on_the_clock(
        theme,
        true,
        FixedSize::new()
            .width(200.0)
            .child(ProgressBar::indeterminate()),
    );
    let at_rest = stripes(&rerender(&mut still));
    assert_eq!(stripes(&tick(&mut still, 400)), at_rest);
}

/// A table's header sections are the button colour, outlined at the bottom
/// and trailing edge in the window text at the frame contrast, 30 px tall
/// over 24 px rows; hovered and resized they are Plasma's hovered and
/// sunken sections, and right to left the trailing line is on the left.
#[test]
fn a_table_header_is_breezes_section() {
    for (scheme, theme) in themes() {
        for rtl in [false, true] {
            let mut tree = tree_with(theme.clone());
            if rtl {
                tree.set_layout_direction(teksilo_core::environment::LayoutDirection::RightToLeft);
            }
            let root = tree.add(table());
            tree.layout(SizeProposal::exact(400.0, 300.0));
            let header = nodes_named(&tree, root, "BreezeHeaderSection");
            assert_eq!(header.len(), 1);
            let h = tree.bounds(header[0]);
            assert_eq!(h.height, 30.0);
            let frame = tree.render();
            let rects = decorations(&frame);
            let face = rects
                .iter()
                .find(|(_, r)| *r == [h.x, h.y, h.width, h.height])
                .unwrap_or_else(|| panic!("{scheme}: no section face in {rects:?}"));
            assert_eq!(face.0, sample(scheme, "header.normal", 50, 15), "{scheme}");
            let line = rects
                .iter()
                .find(|(_, r)| *r == [h.x, h.y + 29.0, h.width, 1.0])
                .expect("the bottom line");
            assert!(near(
                over(line.0, face.0),
                sample(scheme, "header.normal", 50, 29)
            ));
            let x = if rtl { h.x } else { h.x + h.width - 1.0 };
            let trailing = rects
                .iter()
                .find(|(_, r)| *r == [x, h.y, 1.0, 30.0])
                .unwrap_or_else(|| panic!("{scheme} rtl {rtl}: no trailing line in {rects:?}"));
            assert_eq!(trailing.0, line.0);
        }
    }
}

/// The face of the one header section of `root`.
fn header_face(tree: &mut WidgetTree, root: WidgetId) -> [u8; 4] {
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let h = tree.bounds(nodes_named(tree, root, "BreezeHeaderSection")[0]);
    let frame = tree.render();
    decorations(&frame)
        .into_iter()
        .find(|(_, r)| *r == [h.x, h.y, h.width, h.height])
        .unwrap_or_else(|| panic!("no section face at {h:?}"))
        .0
}

/// Hover the section of a table in `theme`, then grab its trailing edge and
/// drag it: the face hovered, and the face while the column resizes.
fn hovered_and_resizing_faces(theme: Theme) -> ([u8; 4], [u8; 4]) {
    use teksilo_core::event::WidgetEvent;
    let mut tree = tree_with(theme);
    let root = tree.add(table());
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let h = tree.bounds(nodes_named(&tree, root, "BreezeHeaderSection")[0]);
    tree.pointer_move(h.center());
    let hovered = header_face(&mut tree, root);
    let grip = teksilo_canvas::Point::new(h.x + h.width - 1.0, h.y + h.height * 0.5);
    let to = teksilo_canvas::Point::new(grip.x + 10.0, grip.y);
    tree.pointer_move(grip);
    tree.dispatch_event(WidgetEvent::pointer_down(
        grip,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.dispatch_event(WidgetEvent::pointer_move(to));
    let resizing = header_face(&mut tree, root);
    tree.dispatch_event(WidgetEvent::pointer_up(
        to,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    (hovered, resizing)
}

/// Under the pointer the section is Plasma's hovered one; grabbed by its
/// trailing edge to resize the column, Plasma's sunken one.
#[test]
fn a_hovered_or_resized_header_section_is_plasmas() {
    for (scheme, theme) in themes() {
        let (hovered, resizing) = hovered_and_resizing_faces(theme);
        assert_eq!(hovered, sample(scheme, "header.hover", 50, 15), "{scheme}");
        assert_eq!(
            resizing,
            sample(scheme, "header.sunken", 50, 15),
            "{scheme}"
        );
    }
}

/// Breeze's schemes give the view's focus and hover decorations one
/// colour, so Plasma's hovered and sunken sections match. In a scheme where
/// they differ, the hovered section moves toward the hover colour and the
/// one being resized toward the focus colour.
#[test]
fn a_resized_header_section_turns_toward_the_focus_colour() {
    let (scheme, _) = teksilo_kde_scheme::KdeColorScheme::parse(
        "[Colors:View]\nDecorationFocus=61,174,233\nDecorationHover=246,116,0\n",
    );
    let theme = kde::from_scheme(&scheme);
    let colors = BreezeHeaderColors::resolve(&theme, true, false);
    assert_ne!(colors.hovered, colors.pressed, "the premise");
    let (hovered, resizing) = hovered_and_resizing_faces(theme);
    assert_eq!(hovered, rgba8_of(colors.hovered));
    assert_eq!(resizing, rgba8_of(colors.pressed));
}

/// The column sorted down, Qt's indicator for an ascending sort, is
/// painted as Plasma's pressed section; a descending column is not.
#[test]
fn the_column_sorted_down_is_a_pressed_section() {
    for (scheme, theme) in themes() {
        for (direction, want) in [
            (SortDirection::Ascending, "header.sunken"),
            (SortDirection::Descending, "header.normal"),
        ] {
            let mut tree = tree_with(theme.clone());
            let model = ListModel::from_vec(vec!["alpha".to_string(), "beta".into()]);
            let root = tree.add(
                TableView::new(model)
                    .add_column(name_column(|s: &String| s.clone()).sortable(true))
                    .bind_sort(Signal::new(Some(("name".to_string(), direction)))),
            );
            assert_eq!(
                header_face(&mut tree, root),
                sample(scheme, want, 50, 15),
                "{scheme} {direction:?}"
            );
        }
    }
}
