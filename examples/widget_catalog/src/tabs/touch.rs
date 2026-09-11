// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Touch tab — the density ladder in force, the three gesture profiles, and the
//! behaviours a finger gets that a mouse does not.
//!
//! Everything here is read off `ctx.theme().input`, never retyped, so the tab
//! shows what the running catalog is actually built with. Launch it again with
//! `--density comfortable` or `--density touch` to see the same widgets on
//! another rung; the flag rebuilds the whole app, which an in-app switch cannot
//! do from a tab (see `crate::cli::CliOptions::density`).
//!
//! `cargo run -p touch-playground` is the instrumented companion: it reports
//! every pointer it is handed, shows which contender won each press, and switches
//! density and the kinetic constants live. This tab is the catalog's view — the
//! stock widgets, at a density, with the gestures named.

use teksilo::prelude::*;
use teksilo::widgets::primitives::MinSize;
use teksilo::widgets::scroll_area::ScrollArea;
use teksilo::widgets::splitter::{PaneDescriptor, Splitter, SplitterModel};
use teksilo::widgets::{
    Divider, HStack, ListView, Padding, Panel, Slider, StandardListItem, TextInput, TextWidget,
    VStack,
};

use crate::shared::{Signals, section, tab_header};

pub fn title() -> LocalizedString {
    tr!(tab_touch_title())
}

pub fn refs() -> LocalizedString {
    tr!(tab_touch_refs())
}

/// A block of fixed-pitch lines. The ladder and the profile table are tables,
/// and a proportional font makes a table unreadable.
fn mono_block(lines: Vec<String>) -> VStack {
    let mut column = VStack::new().spacing(1.0);
    for line in lines {
        column = column.child(TextWidget::new(lit!(line)).style(TextStyleRole::Mono));
    }
    column
}

/// The active density's own numbers, beside the two rungs it is not.
///
/// Read from `InputTokens::for_density` for the comparison columns and from the
/// live theme for the active row, so the "active" marker cannot disagree with
/// what the widgets below it were built with.
fn ladder(ctx: &mut BuildContext) -> VStack {
    let active = ctx.theme().input.density;
    let mut lines = vec!["rung           target  grab  slop budget  spacing  reveal".to_string()];
    for density in [
        teksilo::tokens::TargetDensity::Compact,
        teksilo::tokens::TargetDensity::Comfortable,
        teksilo::tokens::TargetDensity::Touch,
    ] {
        let tokens = teksilo::tokens::InputTokens::for_density(density);
        lines.push(format!(
            "{}{:<14}{:>4} dp{:>5} dp{:>10} dp{:>8.2}  {:?}",
            if density == active { "→ " } else { "  " },
            format!("{density:?}"),
            tokens.target_size,
            tokens.grab_size,
            tokens.slop_budget,
            tokens.spacing_factor,
            tokens.reveal,
        ));
    }
    let floor = ctx.theme().input.min_target_conformance;
    lines.push(String::new());
    lines.push(format!(
        "conformance floor {floor} dp at every rung, never scaled"
    ));
    mono_block(lines)
}

/// The three gesture profiles side by side.
///
/// This table is the whole reason a finger behaves differently from a mouse: the
/// thresholds, not the code paths, are what differ.
fn profiles(ctx: &mut BuildContext) -> VStack {
    let tokens = &ctx.theme().input;
    let mut lines = vec!["device   tap  drag   pan   hit  hold   activation".to_string()];
    for (name, kind) in [
        ("mouse", teksilo::tokens::PointerKind::Mouse),
        ("touch", teksilo::tokens::PointerKind::Touch),
        (
            "pen",
            teksilo::tokens::PointerKind::Pen(teksilo::tokens::PenKind::Pen),
        ),
    ] {
        let profile = tokens.profile(kind);
        lines.push(format!(
            "{name:<8}{:>4} {:>5} {:>5} {:>5} {:>4}ms  {:?}",
            profile.tap_slop,
            profile.drag_slop,
            profile
                .pan_slop
                .map(|v| format!("{v}"))
                .unwrap_or_else(|| "—".to_string()),
            profile.hit_slop,
            profile.long_press.as_millis(),
            profile.drag_activation,
        ));
    }
    mono_block(lines)
}

/// A list that pans under a finger and reorders behind a hold.
fn pan_and_reorder() -> MinSize {
    let model = teksilo::data::ListModel::from_vec(
        (1..=30)
            .map(|i| format!("row {i}"))
            .collect::<Vec<String>>(),
    );
    MinSize::new(0.0, 180.0).child(
        ListView::new(model, |_i, item: &String, _selected| {
            Box::new(StandardListItem::new(lit!(item.clone())))
        })
        .reorderable(true),
    )
}

/// A slider inside a scroller: the one manipulator that must never be mistaken
/// for a pan, on any device.
fn manipulator_vs_pan(ctx: &mut BuildContext) -> MinSize {
    let value = ctx.signal(40.0_f32);
    MinSize::new(0.0, 150.0).child(
        ScrollArea::new().child(
            VStack::new()
                .spacing(10.0)
                .child(Slider::new(value, 0.0, 100.0).label(tr!(touch_slider_label())))
                .children((1..=10).map(|i| TextWidget::new(lit!(format!("filler line {i}"))))),
        ),
    )
}

/// A splitter gutter: painted at the theme's grab size, reaching the conformance
/// floor for a coarse pointer through the widget's own hit outset.
fn grab_target(ctx: &mut BuildContext) -> VStack {
    let model = SplitterModel::from_panes(
        vec![
            PaneDescriptor::new().size(150.0).min_size(60.0),
            PaneDescriptor::new().size(150.0).min_size(60.0),
        ],
        teksilo::tokens::Orientation::Horizontal,
    );
    let gutter = model.gutter_thickness();
    let reach = ctx.theme().input.target_size;
    VStack::new()
        .spacing(6.0)
        .child(
            MinSize::new(0.0, 110.0).child(
                Splitter::new(model)
                    .pane(pane("leading", SurfaceRole::Raised))
                    .pane(pane("trailing", SurfaceRole::Sunken)),
            ),
        )
        .child(
            TextWidget::new(lit!(format!(
                "painted {gutter} dp · a coarse pointer reaches {reach} dp"
            )))
            .style(TextStyleRole::Mono)
            .color(TextRole::Secondary),
        )
}

fn pane(label: &str, role: SurfaceRole) -> Panel {
    let label = label.to_string();
    Panel::new().background(role).child(
        Padding::uniform(8.0).child(TextWidget::new(lit!(label)).style(TextStyleRole::Small)),
    )
}

/// A caption under a demo.
fn note(text: LocalizedString) -> TextWidget {
    TextWidget::new(text)
        .style(TextStyleRole::Small)
        .color(TextRole::Secondary)
}

pub fn classic(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    let header = tab_header(ctx, title(), refs());

    let ladder_block = ladder(ctx);
    let ladder_id = section(
        ctx,
        tr!(touch_section_ladder()),
        VStack::new()
            .spacing(6.0)
            .child(ladder_block)
            .child(note(tr!(touch_ladder_note()))),
    );

    let profiles_block = profiles(ctx);
    let profiles_id = section(
        ctx,
        tr!(touch_section_profiles()),
        VStack::new()
            .spacing(6.0)
            .child(profiles_block)
            .child(note(tr!(touch_profiles_note()))),
    );

    let pan_id = section(
        ctx,
        tr!(touch_section_pan()),
        VStack::new()
            .spacing(6.0)
            .child(note(tr!(touch_pan_note())))
            .child(pan_and_reorder()),
    );

    let manipulator = manipulator_vs_pan(ctx);
    let manipulator_id = section(
        ctx,
        tr!(touch_section_manipulator()),
        VStack::new()
            .spacing(6.0)
            .child(note(tr!(touch_manipulator_note())))
            .child(manipulator),
    );

    let grab = grab_target(ctx);
    let grab_id = section(
        ctx,
        tr!(touch_section_targets()),
        VStack::new()
            .spacing(6.0)
            .child(note(tr!(touch_targets_note())))
            .child(grab),
    );

    let field = ctx.signal(String::new());
    let text_id = section(
        ctx,
        tr!(touch_section_text()),
        VStack::new()
            .spacing(6.0)
            .child(note(tr!(touch_text_note())))
            .child(TextInput::new(field).placeholder(tr!(touch_text_placeholder()))),
    );

    ctx.add(
        VStack::new()
            .spacing(20.0)
            .add_child(header)
            .child(Divider::new())
            .add_child(ladder_id)
            .add_child(profiles_id)
            .add_child(pan_id)
            .add_child(manipulator_id)
            .add_child(grab_id)
            .add_child(text_id)
            .child(
                HStack::new()
                    .spacing(6.0)
                    .child(note(tr!(touch_playground_note()))),
            ),
    )
}

pub fn teksu(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    // The demos carry closures and read the live theme, which `teksu!` property
    // syntax cannot express — pre-build each and splice it in by id, the same
    // shape the Drag & Drop tab uses.
    let ladder_block = ladder(ctx);
    let ladder_id = ctx.add(ladder_block);
    let profiles_block = profiles(ctx);
    let profiles_id = ctx.add(profiles_block);
    let pan_id = ctx.add(pan_and_reorder());
    let manipulator = manipulator_vs_pan(ctx);
    let manipulator_id = ctx.add(manipulator);
    let grab = grab_target(ctx);
    let grab_id = ctx.add(grab);
    let field = ctx.signal(String::new());
    let text_id = ctx.add(TextInput::new(field).placeholder(tr!(touch_text_placeholder())));

    teksu!(ctx => VStack {
            spacing: 20.0
            VStack {
                spacing: 4.0
                TextWidget::new(tr!(tab_touch_title())) {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new(tr!(tab_touch_refs())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }
            Divider

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(touch_section_ladder())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ ladder_id }
                TextWidget::new(tr!(touch_ladder_note())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(touch_section_profiles())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ profiles_id }
                TextWidget::new(tr!(touch_profiles_note())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(touch_section_pan())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                TextWidget::new(tr!(touch_pan_note())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                #{ pan_id }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(touch_section_manipulator())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                TextWidget::new(tr!(touch_manipulator_note())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                #{ manipulator_id }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(touch_section_targets())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                TextWidget::new(tr!(touch_targets_note())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                #{ grab_id }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(touch_section_text())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                TextWidget::new(tr!(touch_text_note())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                #{ text_id }
            }

            TextWidget::new(tr!(touch_playground_note())) {
                style: TextStyleRole::Small
                color: TextRole::Secondary
            }
        }
    )
}
