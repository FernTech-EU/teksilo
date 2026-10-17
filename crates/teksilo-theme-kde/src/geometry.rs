// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Breeze geometry: the widget style's metric constants, and the control
//! sizes they give for a font.
//!
//! Most Breeze control heights are not constants: the style adds fixed
//! margins to the height of a line of text, so a larger desktop font makes
//! larger controls. [`BreezeGeometry::for_line_height`] holds every such
//! height for a body line height `H` (18 px for Plasma's default Noto Sans
//! 10 pt, [`crate::typography::breeze_body_line_height`]). The rules are
//! exact at that default. At other sizes Qt measures some controls by a
//! text box that can be one to three pixels taller than `H`, so a rule can
//! fall short of what Qt reports by that much; Teksilo keeps the
//! deterministic rule, and the tests bound the difference.
//!
//! These are Breeze's own numbers, in logical pixels at the Compact
//! density. Where Teksilo raises a height for its touch target or
//! accessibility floors, the widget styles do it, on top of these.

/// `[M]` The margin of a frame drawn around a control.
pub const FRAME_WIDTH: f32 = 2.0;
/// `[M]` The top-level layout margin, a dialog's content margin.
pub const LAYOUT_TOP_LEVEL_MARGIN: f32 = 10.0;
/// `[M]` The margin of a nested layout.
pub const LAYOUT_CHILD_MARGIN: f32 = 6.0;
/// `[M]` The spacing between the items of a layout.
pub const LAYOUT_SPACING: f32 = 6.0;
/// `[M]` The frame width of a line edit.
pub const LINE_EDIT_FRAME_WIDTH: f32 = 6.0;
/// `[M]` The frame width of a combo box.
pub const COMBO_BOX_FRAME_WIDTH: f32 = 6.0;
/// `[M]` The frame width of a spin box.
pub const SPIN_BOX_FRAME_WIDTH: f32 = 6.0;
/// `[M]` The width of a spin box's arrow column.
pub const SPIN_BOX_ARROW_BUTTON_WIDTH: f32 = 20.0;
/// `[M]` A push button's minimum width before its margins.
pub const BUTTON_MIN_WIDTH: f32 = 80.0;
/// `[M]` A push button's horizontal margin.
pub const BUTTON_MARGIN_WIDTH: f32 = 6.0;
/// `[M]` The spacing between a button's icon, text and menu arrow.
pub const BUTTON_ITEM_SPACING: f32 = 4.0;
/// `[M]` A tool button's margin.
pub const TOOL_BUTTON_MARGIN_WIDTH: f32 = 6.0;
/// `[M]` The width of a button's menu indicator.
pub const MENU_BUTTON_INDICATOR_WIDTH: f32 = 20.0;
/// `[M]` The check box and radio button indicator, square.
pub const CHECKBOX_SIZE: f32 = 20.0;
/// `[M]` The spacing between a check box indicator and its label.
pub const CHECKBOX_ITEM_SPACING: f32 = 4.0;
/// `[M]` The margin a check box's focus frame takes.
pub const CHECKBOX_FOCUS_MARGIN: f32 = 2.0;
/// `[M]` The margin around a menu item, horizontally and vertically.
pub const MENU_ITEM_MARGIN: f32 = 4.0;
/// `[M]` How far a menu item's text starts from its contents' leading edge.
pub const MENU_ITEM_TEXT_LEFT_MARGIN: f32 = 8.0;
/// `[M]` The extra leading margin of a menu without an icon column.
pub const MENU_ITEM_EXTRA_LEFT_MARGIN: f32 = 4.0;
/// `[M]` The gap before a menu item's shortcut.
pub const MENU_ITEM_ACCELERATOR_SPACE: f32 = 16.0;
/// `[M]` A menu bar item's horizontal margin.
pub const MENU_BAR_ITEM_MARGIN_WIDTH: f32 = 10.0;
/// `[M]` A menu bar item's vertical margin.
pub const MENU_BAR_ITEM_MARGIN_HEIGHT: f32 = 6.0;
/// `[M]` The scroll bar's extent, including its 1 px separator line.
pub const SCROLL_BAR_EXTENT: f32 = 21.0;
/// `[M]` The width of the scroll bar's handle.
pub const SCROLL_BAR_HANDLE_WIDTH: f32 = 8.0;
/// `[M]` The shortest the scroll bar's handle gets.
pub const SCROLL_BAR_MIN_HANDLE_LENGTH: f32 = 20.0;
/// `[M]` The thickness of the progress bar's groove.
pub const PROGRESS_BAR_THICKNESS: f32 = 6.0;
/// `[M]` The progress bar's busy indicator size.
pub const PROGRESS_BAR_BUSY_INDICATOR_SIZE: f32 = 14.0;
/// `[Hp]` The period of the busy progress bar's stripes.
pub const PROGRESS_BAR_BUSY_PERIOD: f32 = 28.0;
/// `[M]` The thickness of a slider's groove.
pub const SLIDER_GROOVE_THICKNESS: f32 = 6.0;
/// `[M]` The thickness of a slider, the size of its handle box.
pub const SLIDER_CONTROL_THICKNESS: f32 = 20.0;
/// `[M]` The length of a slider's tick mark.
pub const SLIDER_TICK_LENGTH: f32 = 8.0;
/// `[M]` The gap between a slider's groove and its tick marks.
pub const SLIDER_TICK_MARGIN: f32 = 2.0;
/// `[Hp]` The diameter a slider handle is painted at: the handle box inset
/// by one, then stroked.
pub const SLIDER_HANDLE_PAINTED: f32 = 17.0;
/// `[M]` A tab's vertical margin.
pub const TAB_MARGIN_HEIGHT: f32 = 4.0;
/// `[M]` A tab's horizontal margin.
pub const TAB_MARGIN_WIDTH: f32 = 8.0;
/// `[M]` A tab's minimum width.
pub const TAB_MIN_WIDTH: f32 = 80.0;
/// `[M]` A tab's minimum height.
pub const TAB_MIN_HEIGHT: f32 = 30.0;
/// `[M]` A static tab's minimum height (document mode, not closable, not
/// movable, expanding or vertical).
pub const STATIC_TAB_MIN_HEIGHT: f32 = 34.0;
/// `[M]` The spacing between a tab's icon, text and buttons.
pub const TAB_ITEM_SPACING: f32 = 8.0;
/// `[M]` The thickness of the active tab's Highlight bar.
pub const TAB_ACTIVE_INDICATOR: f32 = 3.0;
/// `[M]` The margin of a table or tree header section.
pub const HEADER_MARGIN: f32 = 6.0;
/// `[M]` The size of a header's sort arrow.
pub const HEADER_ARROW_SIZE: f32 = 10.0;
/// `[M]` The size of an arrow (chevron).
pub const ARROW_SIZE: f32 = 10.0;
/// `[M]` An item-view item's left and right margins.
pub const ITEM_VIEW_MARGIN_WIDTH: f32 = 2.0;
/// `[M]` An item-view item's top and bottom margins.
pub const ITEM_VIEW_MARGIN_HEIGHT: f32 = 1.0;
/// `[M]` The extra top margin of an item view's first row.
pub const ITEM_VIEW_FIRST_ITEM_TOP_MARGIN: f32 = 2.0;
/// `[M]` An item-view item's padding, horizontally and vertically.
pub const ITEM_VIEW_PADDING: f32 = 2.0;
/// `[M]` The tooltip's frame width; Qt adds one more pixel of padding.
pub const TOOLTIP_FRAME_WIDTH: f32 = 3.0;
/// `[M]` The splitter handle's visual width.
pub const SPLITTER_WIDTH: f32 = 1.0;
/// `[M]` A group box title's margin.
pub const GROUP_BOX_TITLE_MARGIN: f32 = 4.0;
/// `[K]` The small icon size, used by buttons, menus and rows.
pub const SMALL_ICON_SIZE: f32 = 16.0;
/// `[K]` The small-medium icon size, used by larger tool buttons.
pub const SMALL_MEDIUM_ICON_SIZE: f32 = 22.0;

/// Every Breeze control height that follows the font, for one body line
/// height.
///
/// Built by [`BreezeGeometry::for_line_height`]. The values are in logical
/// pixels at the Compact density, and each field says the rule it comes
/// from; `H` is the line height and icons are [`SMALL_ICON_SIZE`] unless a
/// field says otherwise.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeGeometry {
    /// `H`, the body line height everything below is derived from.
    pub line_height: f32,
    /// `[S]` A push button with text: `H + 2 x 6 + 2 x 2`. 34 at H = 18.
    pub push_button_height: f32,
    /// `[S]` A push button's minimum width: `max(content + 12, 80) + 4`.
    pub push_button_min_width: f32,
    /// `[S]` A push button with only an icon: `I + 16`. 32 with a 16 px
    /// icon.
    pub icon_button_height: f32,
    /// `[measured-qt]` The width of that icon-only button: 31.
    pub icon_button_width: f32,
    /// `[S]` A tool button without a frame (auto-raise): the icon plus 12.
    /// 28 with a 16 px icon.
    pub tool_button_height: f32,
    /// `[S]` The same with a 22 px icon: 34.
    pub tool_button_height_medium: f32,
    /// `[S]` A tool button with a frame: the icon plus 16. 32 with a 16 px
    /// icon.
    pub framed_tool_button_height: f32,
    /// `[S]` The same with a 22 px icon: 38.
    pub framed_tool_button_height_medium: f32,
    /// `[S]` `[qt]` A line edit: Qt's inner height `max(H, 14) + 2`, plus
    /// the 6 px frame on each side. 32 at H = 18.
    pub line_edit_height: f32,
    /// `[S]` A combo box: `max(H, 20) + 2 x 6`. 32 at H = 18.
    pub combo_box_height: f32,
    /// `[S]` A spin box: the line edit's inner height plus its frame, at
    /// least 20. 32 at H = 18.
    pub spin_box_height: f32,
    /// `[S]` A check box or radio button row: `max(H + 2 x 2, 20)`. 22 at
    /// H = 18.
    pub check_row_height: f32,
    /// `[qt]` `[measured-qt]` A progress bar: Qt's hint, `H + 8`. 26 at
    /// H = 18.
    pub progress_bar_height: f32,
    /// `[S]` A menu item: `max(H, 20, icon) + 1 + 2 x 4`. 29 at H = 18.
    pub menu_item_height: f32,
    /// `[S]` A menu section (a separator with a title): `max(H, 20, icon)
    /// + 4 + 4`. 28 at H = 18.
    pub menu_section_height: f32,
    /// `[S]` A menu separator: `1 + 4`.
    pub menu_separator_height: f32,
    /// `[S]` A menu bar item: `H + 2 x 6`. 30 at H = 18.
    pub menu_bar_item_height: f32,
    /// `[S]` A tab: `max(H + 2 x 4, 30)`. 30 at H = 18.
    pub tab_height: f32,
    /// `[S]` A static tab: `max(H + 2 x 4, 34)`. 34 at H = 18.
    pub static_tab_height: f32,
    /// `[S]` A header section with a sort arrow: `max(H, 10) + 2 x 6`. 30
    /// at H = 18.
    pub header_height: f32,
    /// `[S]` A header section with an icon: `max(H, 22) + 2 x 6`.
    pub header_height_with_icon: f32,
    /// `[S]` A list or tree row of text: `H + 1 + 1 + 2 x 2`. 24 at H = 18.
    pub list_row_height: f32,
    /// `[measured-qt]` A row with a 16 px icon: `max(H, 16) + 2 + 1 + 1 +
    /// 2 x 2` (Qt adds 2 for a decoration). 26 at H = 18.
    pub list_row_height_with_icon: f32,
    /// `[measured-qt]` A row with a check box: `max(H, 20) + 1 + 1 + 2 x 2`.
    /// 26 at H = 18.
    pub list_row_height_with_check: f32,
    /// `[S]` The first row of a view without a frame is one taller: its top
    /// margin is 2, not 1.
    pub first_row_extra: f32,
    /// `[measured-qt]` A table row: `H + 2 x (1 + 1)`, plus the 1 px grid
    /// line the table view adds. 23 at H = 18.
    pub table_row_height: f32,
    /// `[measured-qt]` A one-line tooltip: `H` inside a 4 px inset. 26 at
    /// H = 18.
    pub tooltip_height: f32,
    /// `[measured-qt]` The tooltip's content inset on every side: the 3 px
    /// frame and Qt's 1.
    pub tooltip_padding: f32,
}

impl BreezeGeometry {
    /// Every font-driven height for a body line height of `h` pixels.
    pub fn for_line_height(h: f32) -> BreezeGeometry {
        let icon = SMALL_ICON_SIZE;
        let line_edit_inner = h.max(14.0) + 2.0;
        BreezeGeometry {
            line_height: h,
            push_button_height: h + 2.0 * BUTTON_MARGIN_WIDTH + 2.0 * FRAME_WIDTH,
            push_button_min_width: BUTTON_MIN_WIDTH + 2.0 * FRAME_WIDTH,
            icon_button_height: icon + 2.0 * BUTTON_MARGIN_WIDTH + 2.0 * FRAME_WIDTH,
            icon_button_width: 31.0,
            tool_button_height: icon + 2.0 * TOOL_BUTTON_MARGIN_WIDTH,
            tool_button_height_medium: SMALL_MEDIUM_ICON_SIZE + 2.0 * TOOL_BUTTON_MARGIN_WIDTH,
            framed_tool_button_height: icon + 2.0 * TOOL_BUTTON_MARGIN_WIDTH + 2.0 * FRAME_WIDTH,
            framed_tool_button_height_medium: SMALL_MEDIUM_ICON_SIZE
                + 2.0 * TOOL_BUTTON_MARGIN_WIDTH
                + 2.0 * FRAME_WIDTH,
            line_edit_height: line_edit_inner + 2.0 * LINE_EDIT_FRAME_WIDTH,
            combo_box_height: h.max(20.0) + 2.0 * COMBO_BOX_FRAME_WIDTH,
            spin_box_height: (line_edit_inner + 2.0 * SPIN_BOX_FRAME_WIDTH).max(20.0),
            check_row_height: (h + 2.0 * CHECKBOX_FOCUS_MARGIN).max(CHECKBOX_SIZE),
            progress_bar_height: h + 8.0,
            menu_item_height: h.max(20.0).max(icon) + 1.0 + 2.0 * MENU_ITEM_MARGIN,
            menu_section_height: h.max(20.0).max(icon) + 2.0 * MENU_ITEM_MARGIN,
            menu_separator_height: 1.0 + MENU_ITEM_MARGIN,
            menu_bar_item_height: h + 2.0 * MENU_BAR_ITEM_MARGIN_HEIGHT,
            tab_height: (h + 2.0 * TAB_MARGIN_HEIGHT).max(TAB_MIN_HEIGHT),
            static_tab_height: (h + 2.0 * TAB_MARGIN_HEIGHT).max(STATIC_TAB_MIN_HEIGHT),
            header_height: h.max(HEADER_ARROW_SIZE) + 2.0 * HEADER_MARGIN,
            header_height_with_icon: h.max(SMALL_MEDIUM_ICON_SIZE) + 2.0 * HEADER_MARGIN,
            list_row_height: h + 2.0 * ITEM_VIEW_MARGIN_HEIGHT + 2.0 * ITEM_VIEW_PADDING,
            list_row_height_with_icon: h.max(icon)
                + 2.0
                + 2.0 * ITEM_VIEW_MARGIN_HEIGHT
                + 2.0 * ITEM_VIEW_PADDING,
            list_row_height_with_check: h.max(CHECKBOX_SIZE)
                + 2.0 * ITEM_VIEW_MARGIN_HEIGHT
                + 2.0 * ITEM_VIEW_PADDING,
            first_row_extra: ITEM_VIEW_FIRST_ITEM_TOP_MARGIN - ITEM_VIEW_MARGIN_HEIGHT,
            table_row_height: h + 2.0 * (ITEM_VIEW_MARGIN_HEIGHT + 1.0) + 1.0,
            tooltip_height: h + 2.0 * (TOOLTIP_FRAME_WIDTH + 1.0),
            tooltip_padding: TOOLTIP_FRAME_WIDTH + 1.0,
        }
    }

    /// The geometry of Plasma's default fonts, `H` = 18.
    pub fn breeze_default() -> BreezeGeometry {
        BreezeGeometry::for_line_height(crate::typography::breeze_body_line_height())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typography::{NOTO_SANS_METRICS, face_px, qt_line_height};

    #[test]
    fn the_default_font_gives_the_breeze_sizes() {
        let g = BreezeGeometry::breeze_default();
        assert_eq!(g.line_height, 18.0);
        assert_eq!(g.push_button_height, 34.0);
        assert_eq!(g.push_button_min_width, 84.0);
        assert_eq!(g.icon_button_height, 32.0);
        assert_eq!(g.icon_button_width, 31.0);
        assert_eq!(g.tool_button_height, 28.0);
        assert_eq!(g.tool_button_height_medium, 34.0);
        assert_eq!(g.framed_tool_button_height, 32.0);
        assert_eq!(g.framed_tool_button_height_medium, 38.0);
        assert_eq!(g.line_edit_height, 32.0);
        assert_eq!(g.combo_box_height, 32.0);
        assert_eq!(g.spin_box_height, 32.0);
        assert_eq!(g.check_row_height, 22.0);
        assert_eq!(g.progress_bar_height, 26.0);
        assert_eq!(g.menu_item_height, 29.0);
        assert_eq!(g.menu_section_height, 28.0);
        assert_eq!(g.menu_separator_height, 5.0);
        assert_eq!(g.menu_bar_item_height, 30.0);
        assert_eq!(g.tab_height, 30.0);
        assert_eq!(g.static_tab_height, 34.0);
        assert_eq!(g.header_height, 30.0);
        assert_eq!(g.list_row_height, 24.0);
        assert_eq!(g.list_row_height_with_icon, 26.0);
        assert_eq!(g.list_row_height_with_check, 26.0);
        assert_eq!(g.list_row_height + g.first_row_extra, 25.0);
        assert_eq!(g.table_row_height, 23.0);
        assert_eq!(g.tooltip_height, 26.0);
        assert_eq!(g.tooltip_padding, 4.0);
    }

    /// The heights Qt reported for Plasma's Breeze at each font size: push
    /// button, line edit, combo box, spin box, check box row, menu item,
    /// menu section, tab, header, list row, list row with an icon, table
    /// row (with its grid line) and progress bar.
    const MEASURED: [(f32, [f32; 13]); 6] = [
        (
            8.0,
            [
                32., 29., 32., 29., 20., 29., 28., 30., 27., 21., 24., 20., 23.,
            ],
        ),
        (
            9.0,
            [
                34., 31., 32., 31., 21., 29., 28., 30., 29., 23., 25., 22., 25.,
            ],
        ),
        (
            10.0,
            [
                34., 32., 32., 32., 22., 29., 28., 30., 30., 24., 26., 23., 26.,
            ],
        ),
        (
            11.0,
            [
                38., 34., 35., 34., 25., 30., 28., 30., 33., 27., 29., 26., 28.,
            ],
        ),
        (
            12.0,
            [
                38., 36., 36., 36., 26., 31., 30., 30., 34., 28., 30., 27., 30.,
            ],
        ),
        (
            14.0,
            [
                42., 40., 40., 40., 30., 35., 34., 34., 38., 32., 34., 31., 34.,
            ],
        ),
    ];

    fn rules(g: &BreezeGeometry) -> [f32; 13] {
        [
            g.push_button_height,
            g.line_edit_height,
            g.combo_box_height,
            g.spin_box_height,
            g.check_row_height,
            g.menu_item_height,
            g.menu_section_height,
            g.tab_height,
            g.header_height,
            g.list_row_height,
            g.list_row_height_with_icon,
            g.table_row_height,
            g.progress_bar_height,
        ]
    }

    #[test]
    fn the_rules_are_exact_at_ten_points_and_never_more_than_three_pixels_short() {
        for (pt, measured) in MEASURED {
            let h = qt_line_height(face_px(pt), NOTO_SANS_METRICS) as f32;
            let derived = rules(&BreezeGeometry::for_line_height(h));
            for (i, (rule, qt)) in derived.iter().zip(measured).enumerate() {
                if pt == 10.0 {
                    assert_eq!(*rule, qt, "column {i} at {pt} pt");
                }
                assert!(
                    *rule <= qt && qt - *rule <= 3.0,
                    "column {i} at {pt} pt: the rule gives {rule}, Qt {qt}"
                );
            }
        }
    }

    #[test]
    fn a_larger_font_never_makes_a_control_smaller() {
        let small = rules(&BreezeGeometry::for_line_height(15.0));
        let large = rules(&BreezeGeometry::for_line_height(26.0));
        for (a, b) in small.iter().zip(large) {
            assert!(b >= *a);
        }
    }
}
