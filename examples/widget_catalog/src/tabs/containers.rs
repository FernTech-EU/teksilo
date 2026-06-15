// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Containers tab — Panel, Card, GroupBox, GroupHeader, Accordion, ToolBox,
//! ScrollArea, ScrollBar, SplitView.

use bastyde::prelude::*;
use bastyde::tokens::Orientation;
use bastyde::widgets::scroll_bar::ScrollBarOrientation;
use bastyde::widgets::{
    Accordion, Card, Checkbox, Divider, FixedSize, GroupBox, GroupHeader, Padding, Panel,
    ScrollArea, ScrollBar, Splitter, SplitterModel, TabId, TabInfo, TabWidget, TextWidget, ToolBox,
    ToolBoxItem, VStack,
};

use crate::shared::{Signals, color_cell, section, tab_header};

/// A small embedded TabWidget with three static tabs. The fully
/// featured TabWidget (closable / reorderable / overflow / pinned) is
/// in the `tab_widget` example; this is the catalog's at-a-glance demo.
fn embedded_tab_widget(selected: Signal<Option<TabId>>) -> TabWidget {
    let body = |s: &'static str| {
        Padding::uniform(12.0).child(TextWidget::new(lit!(s)).style(TextStyleRole::Body))
    };
    TabWidget::new(selected)
        .static_tab(
            TabInfo::new().title(lit!("Overview")),
            body("Overview — an embedded TabWidget with three static tabs."),
        )
        .static_tab(
            TabInfo::new().title(lit!("Details")),
            body(
                "Details — closable + reorderable + overflow tabs live in the tab_widget example.",
            ),
        )
        .static_tab(
            TabInfo::new().title(lit!("Settings")),
            body("Settings — tab content."),
        )
}

pub fn title() -> LocalizedString {
    tr!(tab_containers_title())
}

pub fn refs() -> LocalizedString {
    tr!(tab_containers_refs())
}

pub fn classic(ctx: &mut BuildContext, sigs: &Signals) -> WidgetId {
    let header = tab_header(ctx, title(), refs());
    let panel = section(
        ctx,
        lit!("Panel"),
        Panel::new()
            .background(SurfaceRole::Raised)
            .border_color(BorderRole::Default)
            .border_width(1.0)
            .padding(12.0)
            .child(TextWidget::new(tr!(cnt_panel_body())).style(TextStyleRole::Small)),
    );
    let card = section(
        ctx,
        lit!("Card"),
        Card::new()
            .header(
                TextWidget::new(tr!(cnt_card_header()))
                    .style(TextStyleRole::BodyBold)
                    .color(TextRole::Primary),
            )
            .content(TextWidget::new(tr!(cnt_card_body())).style(TextStyleRole::Body))
            .footer(
                TextWidget::new(tr!(cnt_card_footer()))
                    .style(TextStyleRole::Small)
                    .color(TextRole::Secondary),
            ),
    );
    let tab_widget = section(
        ctx,
        lit!("TabWidget"),
        FixedSize::new()
            .bind_width(420.0_f32)
            .bind_height(160.0_f32)
            .child(embedded_tab_widget(sigs.inner_tabs_selected.clone())),
    );
    let group_box = section(
        ctx,
        lit!("GroupBox"),
        GroupBox::new(tr!(cnt_groupbox_title()))
            .checkable(sigs.group_box_notifications_on.clone())
            .child(
                VStack::new()
                    .spacing(4.0)
                    .child(Checkbox::new(sigs.cb_sounds.clone()).label(tr!(cnt_cb_sounds())))
                    .child(
                        Checkbox::new(sigs.cb_disabled_state.clone()).label(tr!(cnt_cb_banner())),
                    ),
            ),
    );
    let group_header = section(
        ctx,
        lit!("GroupHeader"),
        VStack::new()
            .spacing(6.0)
            .child(GroupHeader::new(tr!(cnt_groupheader_title())))
            .child(TextWidget::new(tr!(cnt_groupheader_body())).style(TextStyleRole::Body)),
    );
    let accordion = section(
        ctx,
        lit!("Accordion"),
        VStack::new()
            .spacing(6.0)
            .child(
                Accordion::new(
                    tr!(cnt_accordion_1_title()),
                    sigs.accordion_expanded.clone(),
                )
                .content(TextWidget::new(tr!(cnt_accordion_1_body())).style(TextStyleRole::Body)),
            )
            .child(
                Accordion::new(
                    tr!(cnt_accordion_2_title()),
                    sigs.accordion2_expanded.clone(),
                )
                .content(TextWidget::new(tr!(cnt_accordion_2_body())).style(TextStyleRole::Body)),
            ),
    );
    let tool_box = section(
        ctx,
        lit!("ToolBox"),
        FixedSize::new().bind_height(220.0_f32).child(
            ToolBox::new(sigs.tool_box_selected.clone())
                .add(ToolBoxItem::new(
                    tr!(cnt_toolbox_general()),
                    TextWidget::new(tr!(cnt_toolbox_general_body())).style(TextStyleRole::Body),
                ))
                .add(ToolBoxItem::new(
                    tr!(cnt_toolbox_editor()),
                    TextWidget::new(tr!(cnt_toolbox_editor_body())).style(TextStyleRole::Body),
                ))
                .add(ToolBoxItem::new(
                    tr!(cnt_toolbox_privacy()),
                    TextWidget::new(tr!(cnt_toolbox_privacy_body())).style(TextStyleRole::Body),
                )),
        ),
    );
    let scroll_area = section(
        ctx,
        lit!("ScrollArea"),
        FixedSize::new()
            .bind_width(280.0_f32)
            .bind_height(120.0_f32)
            .child(ScrollArea::new().child({
                let mut col = VStack::new().spacing(4.0);
                for _ in 0..30 {
                    col = col.child(color_cell(SurfaceRole::AccentSubtle, "row"));
                }
                col
            })),
    );
    let sb_pos = ctx.signal(0.4_f32);
    let sb_max = ctx.signal(1.0_f32);
    let sb_vp = ctx.signal(0.3_f32);
    let scrollbar = section(
        ctx,
        tr!(cnt_scrollbar_standalone_heading()),
        FixedSize::new()
            .bind_width(280.0_f32)
            .bind_height(14.0_f32)
            .child(ScrollBar::new(
                ScrollBarOrientation::Horizontal,
                sb_pos,
                sb_max,
                sb_vp,
            )),
    );
    let split = section(
        ctx,
        lit!("Splitter"),
        FixedSize::new()
            .bind_width(360.0_f32)
            .bind_height(120.0_f32)
            .child(
                Splitter::new(SplitterModel::new(2, Orientation::Horizontal))
                    .pane(Panel::new().background(SurfaceRole::AccentSubtle).child(
                        TextWidget::new(tr!(cnt_split_leading())).style(TextStyleRole::Small),
                    ))
                    .pane(Panel::new().background(SurfaceRole::Raised).child(
                        TextWidget::new(tr!(cnt_split_trailing())).style(TextStyleRole::Small),
                    )),
            ),
    );

    ctx.add(
        VStack::new()
            .spacing(20.0)
            .add_child(header)
            .child(Divider::new())
            .add_child(panel)
            .add_child(card)
            .add_child(tab_widget)
            .add_child(group_box)
            .add_child(group_header)
            .add_child(accordion)
            .add_child(tool_box)
            .add_child(scroll_area)
            .add_child(scrollbar)
            .add_child(split),
    )
}

pub fn bati(ctx: &mut BuildContext, sigs: &Signals) -> WidgetId {
    // Card.header/.content/.footer take Widgets — pre-register where
    // chained method calls are unavoidable. ToolBox::add takes
    // ToolBoxItem; chained .add() calls don't translate to one
    // property per call, so pre-register the whole ToolBox. Same for
    // ScrollArea (loop body) and SplitView (multi-arg first/second
    // wrapping a chained Panel).
    let card_widget = ctx.add(
        Card::new()
            .header(
                TextWidget::new(tr!(cnt_card_header()))
                    .style(TextStyleRole::BodyBold)
                    .color(TextRole::Primary),
            )
            .content(TextWidget::new(tr!(cnt_card_body())).style(TextStyleRole::Body))
            .footer(
                TextWidget::new(tr!(cnt_card_footer()))
                    .style(TextStyleRole::Small)
                    .color(TextRole::Secondary),
            ),
    );
    let inner_sel: Signal<Option<TabId>> = ctx.signal(None);
    let tab_widget_widget = ctx.add(
        FixedSize::new()
            .bind_width(420.0_f32)
            .bind_height(160.0_f32)
            .child(embedded_tab_widget(inner_sel)),
    );
    let toolbox_widget = ctx.add(
        ToolBox::new(sigs.tool_box_selected.clone())
            .add(ToolBoxItem::new(
                tr!(cnt_toolbox_general()),
                TextWidget::new(tr!(cnt_toolbox_general_body())).style(TextStyleRole::Body),
            ))
            .add(ToolBoxItem::new(
                tr!(cnt_toolbox_editor()),
                TextWidget::new(tr!(cnt_toolbox_editor_body())).style(TextStyleRole::Body),
            ))
            .add(ToolBoxItem::new(
                tr!(cnt_toolbox_privacy()),
                TextWidget::new(tr!(cnt_toolbox_privacy_body())).style(TextStyleRole::Body),
            )),
    );
    let scroll_area_widget = ctx.add(ScrollArea::new().child({
        let mut col = VStack::new().spacing(4.0);
        for _ in 0..30 {
            col = col.child(color_cell(SurfaceRole::Raised, "row"));
        }
        col
    }));
    let sb_pos = ctx.signal(0.4_f32);
    let sb_max = ctx.signal(1.0_f32);
    let sb_vp = ctx.signal(0.3_f32);
    let scrollbar_widget = ctx.add(ScrollBar::new(
        ScrollBarOrientation::Horizontal,
        sb_pos,
        sb_max,
        sb_vp,
    ));
    let splitview_widget = ctx.add(
        Splitter::new(SplitterModel::new(2, Orientation::Horizontal))
            .pane(
                Panel::new()
                    .background(SurfaceRole::AccentSubtle)
                    .child(TextWidget::new(tr!(cnt_split_leading())).style(TextStyleRole::Small)),
            )
            .pane(
                Panel::new()
                    .background(SurfaceRole::Raised)
                    .child(TextWidget::new(tr!(cnt_split_trailing())).style(TextStyleRole::Small)),
            ),
    );

    let acc_body_1 =
        ctx.add(TextWidget::new(tr!(cnt_accordion_1_body())).style(TextStyleRole::Body));
    let acc_body_2 =
        ctx.add(TextWidget::new(tr!(cnt_accordion_2_body())).style(TextStyleRole::Body));
    let group_checked = sigs.group_box_notifications_on.clone();
    let cb_sounds = sigs.cb_sounds.clone();
    let cb_banner = sigs.cb_disabled_state.clone();
    let acc_open = sigs.accordion_expanded.clone();
    let acc2_open = sigs.accordion2_expanded.clone();

    bati!(ctx => VStack {
            spacing: 20.0
            VStack {
                spacing: 4.0
                TextWidget::new(tr!(tab_containers_title())) {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new(tr!(tab_containers_refs())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }
            Divider

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("Panel")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Panel {
                    background: SurfaceRole::Raised
                    border_color: BorderRole::Default
                    border_width: 1.0
                    padding: 12.0
                    TextWidget::new(tr!(cnt_panel_body())) {
                        style: TextStyleRole::Small
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("Card")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ card_widget }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("TabWidget")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ tab_widget_widget }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("GroupBox")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                GroupBox::new(tr!(cnt_groupbox_title())) {
                    checkable: group_checked
                    VStack {
                        spacing: 4.0
                        Checkbox::new(cb_sounds) {
                            label: tr!(cnt_cb_sounds())
                        }
                        Checkbox::new(cb_banner) {
                            label: tr!(cnt_cb_banner())
                        }
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("GroupHeader")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                VStack {
                    spacing: 6.0
                    GroupHeader::new(tr!(cnt_groupheader_title()))
                    TextWidget::new(tr!(cnt_groupheader_body())) {
                        style: TextStyleRole::Body
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("Accordion")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                VStack {
                    spacing: 6.0
                    Accordion::new(tr!(cnt_accordion_1_title()), acc_open) {
                        content_id: acc_body_1
                    }
                    Accordion::new(tr!(cnt_accordion_2_title()), acc2_open) {
                        content_id: acc_body_2
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("ToolBox")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                FixedSize {
                    bind_height: 220.0_f32
                    child_id: toolbox_widget
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("ScrollArea")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                FixedSize {
                    bind_width: 280.0_f32
                    bind_height: 120.0_f32
                    child_id: scroll_area_widget
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(cnt_scrollbar_standalone_heading())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                FixedSize {
                    bind_width: 280.0_f32
                    bind_height: 14.0_f32
                    child_id: scrollbar_widget
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("Splitter")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                FixedSize {
                    bind_width: 360.0_f32
                    bind_height: 120.0_f32
                    child_id: splitview_widget
                }
            }
        }
    )
}
