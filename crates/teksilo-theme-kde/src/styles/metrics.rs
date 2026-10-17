// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The dimensions the Breeze widget styles are built with, at a density.
//!
//! Every height here starts from a Breeze rule in [`crate::geometry`]: the
//! text-driven ones from the theme's body line box, the icon-driven ones
//! from an icon size. A height of 24 or more is a target, so it goes through
//! the density ladder's `dp(.., Target)`, which leaves it alone at Compact
//! and raises it to the density's target size above it (`[unpublished]`:
//! KDE publishes no touch ladder, so the ladder is Teksilo's generic one).
//! A gap or padding goes through `spacing`. At Compact every value is
//! Breeze's own.

use teksilo_core::styles::density::{dp, spacing};
use teksilo_tokens::{InputTokens, TargetRole, TypographyTokens};

use crate::geometry::{
    BUTTON_MARGIN_WIDTH, BreezeGeometry, CHECKBOX_FOCUS_MARGIN, CHECKBOX_ITEM_SPACING,
    CHECKBOX_SIZE, COMBO_BOX_FRAME_WIDTH, FRAME_WIDTH, HEADER_MARGIN, ITEM_VIEW_MARGIN_HEIGHT,
    ITEM_VIEW_MARGIN_WIDTH, ITEM_VIEW_PADDING, LARGE_ICON_SIZE, LINE_EDIT_FRAME_WIDTH,
    MEDIUM_ICON_SIZE, MENU_ITEM_ACCELERATOR_SPACE, MENU_ITEM_MARGIN, MENU_ITEM_TEXT_LEFT_MARGIN,
    PROGRESS_BAR_THICKNESS, SCROLL_BAR_EXTENT, SCROLL_BAR_HANDLE_WIDTH,
    SCROLL_BAR_MIN_HANDLE_LENGTH, SLIDER_CONTROL_THICKNESS, SLIDER_GROOVE_THICKNESS,
    SLIDER_TICK_LENGTH, SLIDER_TICK_MARGIN, SMALL_ICON_SIZE, SPIN_BOX_ARROW_BUTTON_WIDTH,
    SPIN_BOX_FRAME_WIDTH, TAB_MARGIN_WIDTH, TOOL_BUTTON_MARGIN_WIDTH,
};
use crate::shape::BREEZE_PEN_WIDTH;

/// `[M]` The square a check box or radio button indicator is painted in.
pub const INDICATOR_SIZE: f32 = CHECKBOX_SIZE;
/// `[Hp]` How far the box (or the radio disc) lies inside the indicator: the
/// indicator's focus margin.
pub const INDICATOR_BOX_INSET: f32 = CHECKBOX_FOCUS_MARGIN;
/// `[Hp]` The side of the box as stroked: the indicator inset by 2, then by
/// half the pen. 15.
pub const INDICATOR_BOX_STROKED: f32 =
    INDICATOR_SIZE - 2.0 * INDICATOR_BOX_INSET - BREEZE_PEN_WIDTH;
/// `[M]` Where a check box's or radio button's label starts, from the
/// indicator's leading edge: the indicator and Breeze's 4 px label spacing.
pub const INDICATOR_LABEL_X: f32 = CHECKBOX_SIZE + CHECKBOX_ITEM_SPACING;
/// `[Hp]` The width of the pen a check mark is drawn with.
pub const CHECK_MARK_PEN: f32 = 2.0;
/// `[Hp]` The side of each of the three squares of a partial check box.
pub const PARTIAL_DOT_SIZE: f32 = 2.0;
/// `[Hp]` The distance between the centres of the partial squares.
pub const PARTIAL_DOT_SPACING: f32 = 4.0;
/// `[Hp]` The radius of a selected radio button's dot: `floor((18 - 12) /
/// 2)`, a 6 px dot.
pub const RADIO_DOT_RADIUS: f32 = 3.0;
/// `[Hp]` How much of the highlight fills a checked box or a selected disc
/// over the button colour.
pub const INDICATOR_CHECKED_WASH: f32 = 0.3;
/// `[Hp]` How much a pressed indicator's button colour is darkened, in Qt's
/// percent (d.11 of the specification).
pub const INDICATOR_PRESSED_DARKER: i32 = 110;

/// `[K]` The height of a switch: Kirigami's grid unit.
pub const SWITCH_HEIGHT: f32 = 18.0;
/// `[Qd]` The width of a switch: twice its height.
pub const SWITCH_WIDTH: f32 = 2.0 * SWITCH_HEIGHT;
/// `[Qd]` How far the track lies inside the switch above and below: `floor(18
/// / 6)`.
pub const SWITCH_TRACK_INSET: f32 = 3.0;
/// `[Qd]` The track's radius: `round(12 / 2)`.
pub const SWITCH_TRACK_RADIUS: f32 = 6.0;
/// `[Qd]` The handle's radius: `floor(18 / 2)`, a circle of the switch's
/// full height.
pub const SWITCH_HANDLE_RADIUS: f32 = 9.0;
/// `[Qd]` How much of the button text the track is filled with over the
/// button colour.
pub const SWITCH_TRACK_TEXT: f64 = 0.1;
/// `[Qd]` The alpha of the highlight that fills a checked track up to the
/// handle.
pub const SWITCH_CHECKED_ALPHA: f32 = 0.5;
/// `[qt]` The speed Qt's smoothed animation moves the handle at, at most, in
/// pixels a second: a full travel of 18 px takes 90 ms rather than the
/// 100 ms duration.
pub const SWITCH_HANDLE_VELOCITY: f32 = 200.0;

// The switch is twice as wide as tall, its track centred inside it.
const _: () = assert!(SWITCH_WIDTH == 2.0 * SWITCH_HEIGHT);
const _: () = assert!(SWITCH_TRACK_RADIUS * 2.0 == SWITCH_HEIGHT - 2.0 * SWITCH_TRACK_INSET);
const _: () = assert!(SWITCH_HANDLE_RADIUS * 2.0 == SWITCH_HEIGHT);
// The box is the 15 px square of the specification.
const _: () = assert!(INDICATOR_BOX_STROKED == 15.0);

/// The width the body of a check box or radio button is laid out at, when
/// the widget puts `label_gap` between it and its label: the label then
/// starts at [`INDICATOR_LABEL_X`], where Breeze puts it.
///
/// With the widget's 6 px gap that is 18, the indicator less the trailing
/// 2 px of its focus margin; the box itself ends at 18, so it always lies in
/// the node, and only the focus frame reaches past it. Never narrower than
/// the box's far edge, so a larger gap moves the label rather than the box
/// out of the node.
pub fn indicator_body_width(label_gap: f32) -> f32 {
    (INDICATOR_LABEL_X - label_gap).max(INDICATOR_SIZE - INDICATOR_BOX_INSET)
}

/// The dimensions of a line edit, the frame a `TextInput` is drawn in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeFieldMetrics {
    /// `[S]` `[qt]` The field's height, at least: Qt's inner height
    /// `max(H, 14) + 2` plus the 6 px frame above and below, through the
    /// density ladder. 32 at Compact for the Breeze typography.
    pub height: f32,
    /// `[M]` The space between the frame and the text on the leading and
    /// trailing sides: the 6 px line-edit frame, scaled by the density's
    /// spacing.
    pub padding_horizontal: f32,
}

/// The line-edit dimensions at the density of `tokens`, for `typography`.
pub fn breeze_field_metrics_for(
    tokens: &InputTokens,
    typography: &TypographyTokens,
) -> BreezeFieldMetrics {
    let g = geometry_of(typography);
    BreezeFieldMetrics {
        height: dp(g.line_edit_height, TargetRole::Target, tokens),
        padding_horizontal: spacing(LINE_EDIT_FRAME_WIDTH, tokens),
    }
}

/// [`breeze_field_metrics_for`] at the Compact density and the Breeze
/// typography.
pub fn breeze_field_metrics() -> BreezeFieldMetrics {
    breeze_field_metrics_for(
        &InputTokens::default(),
        &crate::typography::breeze_typography(),
    )
}

/// The body line box `H` of `typography`, in whole pixels: the height every
/// text-driven Breeze control is derived from. 18 for the Breeze typography.
pub fn body_line_height(typography: &TypographyTokens) -> f32 {
    (typography.body.size * typography.body.line_height).round()
}

/// The Breeze geometry of `typography`'s body line box.
pub fn geometry_of(typography: &TypographyTokens) -> BreezeGeometry {
    BreezeGeometry::for_line_height(body_line_height(typography))
}

/// The dimensions of a push button, a default button and a flat push button,
/// which share them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeButtonMetrics {
    /// `[S]` The button's height, at least: `H + 16`, through the density
    /// ladder. 34 at Compact for the Breeze typography.
    pub height: f32,
    /// `[S]` The button's width, at least: 84, Breeze's 80 plus the 2 px
    /// frame margin on each side.
    pub min_width: f32,
    /// `[S]` The space between the frame and the label on the leading and
    /// trailing sides: the 6 px button margin and the 2 px frame margin,
    /// scaled by the density's spacing.
    pub padding_horizontal: f32,
    /// `[S]` The same above and below the label. It is part of the height
    /// rule rather than a gap, so it does not scale: the height floor rises
    /// with the density instead.
    pub padding_vertical: f32,
}

/// The button dimensions at the density of `tokens`, for `typography`.
pub fn breeze_button_metrics_for(
    tokens: &InputTokens,
    typography: &TypographyTokens,
) -> BreezeButtonMetrics {
    let g = geometry_of(typography);
    BreezeButtonMetrics {
        height: dp(g.push_button_height, TargetRole::Target, tokens),
        min_width: g.push_button_min_width,
        padding_horizontal: spacing(BUTTON_MARGIN_WIDTH + FRAME_WIDTH, tokens),
        padding_vertical: BUTTON_MARGIN_WIDTH + FRAME_WIDTH,
    }
}

/// [`breeze_button_metrics_for`] at the Compact density and the Breeze
/// typography.
pub fn breeze_button_metrics() -> BreezeButtonMetrics {
    breeze_button_metrics_for(
        &InputTokens::default(),
        &crate::typography::breeze_typography(),
    )
}

/// The square an `IconButton` is drawn in at each of its five sizes: Breeze's
/// auto-raise tool button, an icon with the 6 px tool-button margin around
/// it, through the density ladder.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeToolButtonSizes {
    /// `[unpublished]` Breeze has no tool button smaller than the one around
    /// a 16 px icon; the compact rung is the density's target size, 24 at
    /// Compact.
    pub compact: f32,
    /// `[S]` A 16 px icon: 28 at Compact.
    pub default: f32,
    /// `[S]` A 22 px icon, the toolbar size: 34 at Compact.
    pub toolbar: f32,
    /// `[derived]` Breeze's rule at Kirigami's 32 px medium icon: 44.
    pub large: f32,
    /// `[derived]` Breeze's rule at Kirigami's 48 px large icon: 60.
    pub hero: f32,
}

/// The tool-button squares at the density of `tokens`.
pub fn breeze_tool_button_sizes_for(tokens: &InputTokens) -> BreezeToolButtonSizes {
    // Icon-driven, so the font does not enter: the default geometry's
    // tool-button heights are those of any font.
    let g = BreezeGeometry::breeze_default();
    let around = |icon: f32| icon + 2.0 * TOOL_BUTTON_MARGIN_WIDTH;
    let target = |h: f32| dp(h, TargetRole::Target, tokens);
    BreezeToolButtonSizes {
        compact: target(tokens.min_target_conformance),
        default: target(g.tool_button_height),
        toolbar: target(g.tool_button_height_medium),
        large: target(around(MEDIUM_ICON_SIZE)),
        hero: target(around(LARGE_ICON_SIZE)),
    }
}

/// [`breeze_tool_button_sizes_for`] at the Compact density.
pub fn breeze_tool_button_sizes() -> BreezeToolButtonSizes {
    breeze_tool_button_sizes_for(&InputTokens::default())
}

/// `[S]` The square a combo box's arrow is centred in, at the trailing edge
/// inside the 2 px frame margin.
pub const COMBO_ARROW_BOX: f32 = 20.0;
/// `[S]` What a combo box adds to its label's width: the 6 px frame on both
/// sides of the label, the arrow box, and 2 and 4 more. 38.
pub const COMBO_CHROME_WIDTH: f32 = 2.0 * COMBO_BOX_FRAME_WIDTH + COMBO_ARROW_BOX + 2.0 + 4.0;
/// `[derived]` The space between a combo box's label and its arrow box: the
/// chrome width less the leading frame, the arrow box and the trailing frame
/// margin. 10.
pub const COMBO_ARROW_GAP: f32 =
    COMBO_CHROME_WIDTH - COMBO_BOX_FRAME_WIDTH - COMBO_ARROW_BOX - FRAME_WIDTH;
/// `[S]` The tallest a spin box's arrow column is, centred in the frame.
pub const SPIN_ARROW_MAX_HEIGHT: f32 = 20.0;

/// `[Hp]` The radius of a slider's groove, on the groove inset by half the
/// pen.
pub const SLIDER_GROOVE_RADIUS: f32 = 3.0;
/// `[Hp]` How much of the window text a slider's groove is filled with over
/// the window: 0.2 times 0.7.
pub const SLIDER_GROOVE_ALPHA: f64 = 0.2 * 0.7;
/// `[Hp]` How much of the highlight fills the groove up to the handle, over
/// the window.
pub const SLIDER_FILL_ALPHA: f64 = 0.7;
/// `[measured-qt]` A slider's length when nothing proposes one: Qt's size
/// hint for a Breeze slider, 84 at every font size.
pub const SLIDER_DEFAULT_LENGTH: f32 = 84.0;
/// `[derived]` How far a row of tick marks reaches past the handle box:
/// the groove's half thickness, the 2 px gap and the 8 px tick, less half
/// the handle box. 3, the step Qt measures for each side with ticks.
pub const SLIDER_TICK_OVERHANG: f32 =
    SLIDER_GROOVE_THICKNESS * 0.5 + SLIDER_TICK_MARGIN + SLIDER_TICK_LENGTH
        - SLIDER_CONTROL_THICKNESS * 0.5;

// The groove fits inside the handle box, and the ticks clear the groove.
const _: () = assert!(SLIDER_GROOVE_THICKNESS < SLIDER_CONTROL_THICKNESS);
const _: () = assert!(SLIDER_TICK_OVERHANG == 3.0);
const _: () = assert!(COMBO_CHROME_WIDTH == 38.0);
const _: () = assert!(COMBO_ARROW_GAP == 10.0);

/// The dimensions of a combo box, a push-button frame holding the selected
/// item and an arrow.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeComboMetrics {
    /// `[S]` The combo box's height, at least: `max(H, 20) + 2 x 6`, through
    /// the density ladder. 32 at Compact for the Breeze typography.
    pub height: f32,
    /// `[S]` The space before the label: the 6 px combo-box frame, scaled by
    /// the density's spacing.
    pub padding_leading: f32,
    /// `[derived]` The space between the label and the arrow box,
    /// [`COMBO_ARROW_GAP`], scaled by the density's spacing.
    pub arrow_gap: f32,
    /// `[S]` The arrow box, [`COMBO_ARROW_BOX`]. Painted, so it does not
    /// scale.
    pub arrow_box: f32,
    /// `[M]` The space after the arrow box: the 2 px frame margin. It holds
    /// the frame, so it does not scale.
    pub padding_trailing: f32,
}

impl BreezeComboMetrics {
    /// Everything the combo box adds to its label's width.
    pub fn chrome_width(&self) -> f32 {
        self.padding_leading + self.arrow_gap + self.arrow_box + self.padding_trailing
    }
}

/// The combo box dimensions at the density of `tokens`, for `typography`.
pub fn breeze_combo_metrics_for(
    tokens: &InputTokens,
    typography: &TypographyTokens,
) -> BreezeComboMetrics {
    let g = geometry_of(typography);
    BreezeComboMetrics {
        height: dp(g.combo_box_height, TargetRole::Target, tokens),
        padding_leading: spacing(COMBO_BOX_FRAME_WIDTH, tokens),
        arrow_gap: spacing(COMBO_ARROW_GAP, tokens),
        arrow_box: COMBO_ARROW_BOX,
        padding_trailing: FRAME_WIDTH,
    }
}

/// [`breeze_combo_metrics_for`] at the Compact density and the Breeze
/// typography.
pub fn breeze_combo_metrics() -> BreezeComboMetrics {
    breeze_combo_metrics_for(
        &InputTokens::default(),
        &crate::typography::breeze_typography(),
    )
}

/// The dimensions of a spin box: a line-edit frame holding the field and a
/// column of two arrows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeSpinMetrics {
    /// `[S]` The spin box's height, at least: the line edit's inner height
    /// `max(H, 14) + 2` and the 6 px frame on each side, at least 20,
    /// through the density ladder. 32 at Compact for the Breeze typography.
    pub height: f32,
    /// `[M]` The space before the field: the 6 px spin-box frame, scaled by
    /// the density's spacing.
    pub padding_leading: f32,
    /// `[M]` The width of the arrow column, [`SPIN_BOX_ARROW_BUTTON_WIDTH`].
    /// Painted, so it does not scale.
    pub arrow_width: f32,
    /// `[M]` The space after the arrow column: the 2 px frame margin.
    pub padding_trailing: f32,
}

impl BreezeSpinMetrics {
    /// `[S]` The two halves of the arrow column in a frame `height` tall:
    /// the column is `min(inner height, 20)` tall, the inner height being the
    /// frame less the 6 px frame above and below, and each half is the
    /// column halved and rounded down. 10 and 10 at every density for the
    /// Breeze typography.
    pub fn arrow_half(height: f32) -> f32 {
        let inner = (height - 2.0 * SPIN_BOX_FRAME_WIDTH).max(0.0);
        (inner.min(SPIN_ARROW_MAX_HEIGHT) * 0.5).floor()
    }
}

/// The spin box dimensions at the density of `tokens`, for `typography`.
pub fn breeze_spin_metrics_for(
    tokens: &InputTokens,
    typography: &TypographyTokens,
) -> BreezeSpinMetrics {
    let g = geometry_of(typography);
    BreezeSpinMetrics {
        height: dp(g.spin_box_height, TargetRole::Target, tokens),
        padding_leading: spacing(SPIN_BOX_FRAME_WIDTH, tokens),
        arrow_width: SPIN_BOX_ARROW_BUTTON_WIDTH,
        padding_trailing: FRAME_WIDTH,
    }
}

/// [`breeze_spin_metrics_for`] at the Compact density and the Breeze
/// typography.
pub fn breeze_spin_metrics() -> BreezeSpinMetrics {
    breeze_spin_metrics_for(
        &InputTokens::default(),
        &crate::typography::breeze_typography(),
    )
}

/// `[S]` The space a menu item's text starts after its icon column: Breeze's
/// 2 px and Qt's 1.
pub const MENU_ICON_TEXT_GAP: f32 = 3.0;
/// `[Hp]` The square a submenu arrow is centred in, at the trailing edge of
/// a menu item's contents.
pub const MENU_ARROW_BOX: f32 = 20.0;
/// `[Hp]` The space between the end of a menu item's text and its arrow
/// box.
pub const MENU_ARROW_TEXT_GAP: f32 = 3.0;
/// `[Hp]` How much of the focus colour washes a highlighted menu item: the
/// style's blend value.
pub const MENU_HIGHLIGHT_ALPHA: f32 = 0.3;
/// `[M]` How far a popup's content lies inside its frame: the menu margin,
/// which Breeze gives every popup panel.
pub const POPUP_PADDING: f32 = MENU_ITEM_MARGIN;
/// `[unpublished]` The widest a tooltip's text is laid out before it wraps:
/// Qt wraps a tooltip against the screen, which a tooltip in a Teksilo window
/// has no use for, so this is Teksilo's own 320 px.
pub const TOOLTIP_MAX_WIDTH: f32 = 320.0;

/// The dimensions of a menu: its rows, separators and section titles.
///
/// A row is laid out as Breeze lays out the item of a menu that shows icons:
/// `MenuItem` reserves its leading column on every row, the icon and the
/// check mark's alike.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeMenuMetrics {
    /// `[S]` A row's height: `max(H, 20, 16) + 1` and the 4 px item margin
    /// above and below, through the density ladder. 29 at Compact for the
    /// Breeze typography.
    pub item_height: f32,
    /// `[S]` A separator's height: its 1 px line and the 4 px item margin,
    /// scaled by the density's spacing. 5 at Compact.
    pub separator_height: f32,
    /// `[S]` A section title's height (a separator with text): `max(H, 20,
    /// 16)` and the 4 px item margin above and below, through the density
    /// ladder. 28 at Compact. `MenuList::header` shows a widget the app
    /// builds, which the style never sees, so this is what an app sizes its
    /// section titles with.
    pub section_height: f32,
    /// `[M]` The space before a row's icon column: the 4 px item margin and
    /// Breeze's 8 px text margin, scaled by the density's spacing. 12 at
    /// Compact.
    pub padding_leading: f32,
    /// `[K]` The icon column: the small icon size. Painted, so it does not
    /// scale.
    pub icon_column: f32,
    /// `[S]` The space between the icon column and the text,
    /// [`MENU_ICON_TEXT_GAP`], scaled by the density's spacing.
    pub icon_label_gap: f32,
    /// `[derived]` The column at a row's trailing edge: the 3 px gap after
    /// the text, the 20 px arrow box and the 4 px item margin, the gaps
    /// scaled by the density's spacing. 27 at Compact.
    pub trailing_column: f32,
    /// `[M]` The least space between a row's text and its shortcut: the
    /// 16 px accelerator space, scaled by the density's spacing.
    pub shortcut_gap: f32,
    /// `[M]` The gap between a menu's frame and its rows on every side: the
    /// 4 px menu margin, `MenuList`'s own inset.
    pub popup_margin: f32,
}

/// The menu dimensions at the density of `tokens`, for `typography`.
pub fn breeze_menu_metrics_for(
    tokens: &InputTokens,
    typography: &TypographyTokens,
) -> BreezeMenuMetrics {
    let g = geometry_of(typography);
    BreezeMenuMetrics {
        item_height: dp(g.menu_item_height, TargetRole::Target, tokens),
        separator_height: spacing(g.menu_separator_height, tokens),
        section_height: dp(g.menu_section_height, TargetRole::Target, tokens),
        padding_leading: spacing(MENU_ITEM_MARGIN + MENU_ITEM_TEXT_LEFT_MARGIN, tokens),
        icon_column: SMALL_ICON_SIZE,
        icon_label_gap: spacing(MENU_ICON_TEXT_GAP, tokens),
        trailing_column: spacing(MENU_ARROW_TEXT_GAP + MENU_ITEM_MARGIN, tokens) + MENU_ARROW_BOX,
        shortcut_gap: spacing(MENU_ITEM_ACCELERATOR_SPACE, tokens),
        popup_margin: MENU_ITEM_MARGIN,
    }
}

/// [`breeze_menu_metrics_for`] at the Compact density and the Breeze
/// typography.
pub fn breeze_menu_metrics() -> BreezeMenuMetrics {
    breeze_menu_metrics_for(
        &InputTokens::default(),
        &crate::typography::breeze_typography(),
    )
}

/// `[measured-qt]` How far a row's content lies inside its leading and
/// trailing edges: the 2 px item margin, the 2 px item padding and Qt's 3 px
/// text margin, where Qt puts a row's check box (7 px in, among the
/// `item_view_item` sub-rectangles of `sizes.json`).
pub const ROW_CONTENT_INSET: f32 = ITEM_VIEW_MARGIN_WIDTH + ITEM_VIEW_PADDING + 3.0;
/// `[Hp]` How much of the highlight washes a hovered row that is not
/// selected.
pub const ROW_HOVER_ALPHA: f32 = 0.3;
/// `[Hp]` The alpha of a hovered row's outline: the fill colour, at 0.8,
/// where a selected row's is opaque.
pub const ROW_HOVER_OUTLINE_ALPHA: f32 = 0.8;
/// `[Hp]` How much a hovered selected row's highlight is lightened, in Qt's
/// percent (d.11 of the specification).
pub const ROW_SELECTED_HOVER_LIGHTER: i32 = 110;

/// The dimensions of a list or tree row, `StandardListItem` and
/// `StandardTreeItem`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeRowMetrics {
    /// `[S]` A row of text: `H + 1 + 1 + 2 x 2`, the line box and the 1 px
    /// item margin and 2 px item padding above and below, through the
    /// density ladder. 24 at Compact for the Breeze typography.
    pub height: f32,
    /// `[measured-qt]` A row with an icon or a check box: 26 at Compact, Qt
    /// adding 2 px for an item's icon and making room for the 20 px check
    /// box, through the density ladder.
    pub height_decorated: f32,
    /// `[derived]` A row with a subtitle: two body line boxes and the same
    /// margins and padding, `2H + 6`, through the density ladder. 42 at
    /// Compact. Qt sizes an item to its text and has no two-line row of its
    /// own.
    pub height_two_line: f32,
    /// `[measured-qt]` [`ROW_CONTENT_INSET`], scaled by the density's
    /// spacing.
    pub padding_horizontal: f32,
    /// `[M]` The space above and below the content: the 1 px item margin.
    /// It holds the selection's outline, so it does not scale; the height
    /// floor rises with the density instead.
    pub padding_vertical: f32,
    /// `[M]` How far the selection lies inside the row's leading and
    /// trailing edges: the 2 px item margin. Painted, so it does not scale.
    pub selection_inset_horizontal: f32,
    /// `[M]` The same above and below: the 1 px item margin.
    pub selection_inset_vertical: f32,
}

/// The row dimensions at the density of `tokens`, for `typography`.
pub fn breeze_row_metrics_for(
    tokens: &InputTokens,
    typography: &TypographyTokens,
) -> BreezeRowMetrics {
    let g = geometry_of(typography);
    let margins = 2.0 * (ITEM_VIEW_MARGIN_HEIGHT + ITEM_VIEW_PADDING);
    BreezeRowMetrics {
        height: dp(g.list_row_height, TargetRole::Target, tokens),
        height_decorated: dp(
            g.list_row_height_with_icon
                .max(g.list_row_height_with_check),
            TargetRole::Target,
            tokens,
        ),
        height_two_line: dp(2.0 * g.line_height + margins, TargetRole::Target, tokens),
        padding_horizontal: spacing(ROW_CONTENT_INSET, tokens),
        padding_vertical: ITEM_VIEW_MARGIN_HEIGHT,
        selection_inset_horizontal: ITEM_VIEW_MARGIN_WIDTH,
        selection_inset_vertical: ITEM_VIEW_MARGIN_HEIGHT,
    }
}

/// [`breeze_row_metrics_for`] at the Compact density and the Breeze
/// typography.
pub fn breeze_row_metrics() -> BreezeRowMetrics {
    breeze_row_metrics_for(
        &InputTokens::default(),
        &crate::typography::breeze_typography(),
    )
}

/// The dimensions of a `TableView` or `TreeTableView`: its rows and its
/// column header.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeTableMetrics {
    /// `[wcag]` D7: a body row. Qt lays a Breeze table row out at `H + 2 x
    /// (1 + 1)` and adds its 1 px grid line, 23 at Plasma's default font,
    /// under WCAG 2.2 SC 2.5.8's 24 px; it is raised to 24, then through the
    /// density ladder.
    pub row_height: f32,
    /// `[S]` The column header: `max(H, 10) + 2 x 6`, room for the sort
    /// arrow, through the density ladder. 30 at Compact for the Breeze
    /// typography.
    pub header_height: f32,
    /// `[M]` The space between a header section's edges and its label: the
    /// 6 px header margin, scaled by the density's spacing.
    pub header_padding: f32,
}

/// The table dimensions at the density of `tokens`, for `typography`.
pub fn breeze_table_metrics_for(
    tokens: &InputTokens,
    typography: &TypographyTokens,
) -> BreezeTableMetrics {
    let g = geometry_of(typography);
    BreezeTableMetrics {
        row_height: dp(
            g.table_row_height.max(tokens.min_target_conformance),
            TargetRole::Target,
            tokens,
        ),
        header_height: dp(g.header_height, TargetRole::Target, tokens),
        header_padding: spacing(HEADER_MARGIN, tokens),
    }
}

/// [`breeze_table_metrics_for`] at the Compact density and the Breeze
/// typography.
pub fn breeze_table_metrics() -> BreezeTableMetrics {
    breeze_table_metrics_for(
        &InputTokens::default(),
        &crate::typography::breeze_typography(),
    )
}

/// `[Hp]` How far a hovered unselected tab's fill moves from the window
/// colour toward the hover colour.
pub const TAB_HOVER_MIX: f64 = 0.2;
/// `[Hp]` How much an unselected tab's window colour is darkened, in Qt's
/// percent.
pub const TAB_UNSELECTED_DARKER: i32 = 120;
/// `[Hp]` The radius of the active tab's highlight bar at its outer
/// corners: the frame radius.
pub const TAB_BAR_RADIUS: f32 = 5.0;
/// `[Hp]` The radius the selected tab's outline is stroked at, on the pen's
/// centre line: with a 1 px pen its outer edge turns at 5 px, the bar's
/// radius, and its inner edge at 4.
pub const TAB_OUTLINE_RADIUS: f32 = 4.5;

/// The dimensions of a tab in a `TabBar` or `TabWidget`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeTabMetrics {
    /// `[S]` A tab's height: `max(H + 2 x 4, 30)`, through the density
    /// ladder. 30 at Compact for the Breeze typography.
    pub height: f32,
    /// `[S]` A static tab bar's tab: `max(H + 2 x 4, 34)`, through the
    /// density ladder. Recorded, not laid out: a `TabStyle` is not told
    /// whether its bar is one (see the crate's known limitations).
    pub static_height: f32,
    /// `[M]` The space between a tab's edges and its title: the 8 px tab
    /// margin, scaled by the density's spacing.
    pub padding_horizontal: f32,
    /// `[M]` A tab's minimum width with a title. Recorded, not laid out: the
    /// bar applies its own minimum tab width (see the crate's known
    /// limitations).
    pub min_width: f32,
    /// `[M]` The thickness of the active tab's highlight bar. Painted, so it
    /// does not scale.
    pub indicator: f32,
}

/// The tab dimensions at the density of `tokens`, for `typography`.
pub fn breeze_tab_metrics_for(
    tokens: &InputTokens,
    typography: &TypographyTokens,
) -> BreezeTabMetrics {
    let g = geometry_of(typography);
    BreezeTabMetrics {
        height: dp(g.tab_height, TargetRole::Target, tokens),
        static_height: dp(g.static_tab_height, TargetRole::Target, tokens),
        padding_horizontal: spacing(TAB_MARGIN_WIDTH, tokens),
        min_width: crate::geometry::TAB_MIN_WIDTH,
        indicator: crate::geometry::TAB_ACTIVE_INDICATOR,
    }
}

/// [`breeze_tab_metrics_for`] at the Compact density and the Breeze
/// typography.
pub fn breeze_tab_metrics() -> BreezeTabMetrics {
    breeze_tab_metrics_for(
        &InputTokens::default(),
        &crate::typography::breeze_typography(),
    )
}

/// `[Hp]` The width of the separator line on a scroll bar's leading edge.
pub const SCROLL_BAR_SEPARATOR: f32 = 1.0;

/// The dimensions of a `ScrollBar`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeScrollBarMetrics {
    /// `[M]` Breeze's extent, its 1 px separator included. Recorded, not laid
    /// out: the widgets choose a scroll bar's thickness themselves (see the
    /// crate's known limitations).
    pub extent: f32,
    /// `[M]` The handle's width, centred across what the separator leaves.
    /// Painted, so it does not scale.
    pub handle_width: f32,
    /// `[M]` The shortest the handle gets. The handle is a grab, which this
    /// clears at every density, so it is Breeze's 20 at all three.
    pub min_handle_length: f32,
    /// `[Hp]` The separator line's width.
    pub separator: f32,
}

/// The scroll-bar dimensions at the density of `tokens`.
pub fn breeze_scroll_bar_metrics_for(tokens: &InputTokens) -> BreezeScrollBarMetrics {
    BreezeScrollBarMetrics {
        extent: SCROLL_BAR_EXTENT,
        handle_width: SCROLL_BAR_HANDLE_WIDTH,
        min_handle_length: dp(SCROLL_BAR_MIN_HANDLE_LENGTH, TargetRole::Grab, tokens),
        separator: SCROLL_BAR_SEPARATOR,
    }
}

/// [`breeze_scroll_bar_metrics_for`] at the Compact density.
pub fn breeze_scroll_bar_metrics() -> BreezeScrollBarMetrics {
    breeze_scroll_bar_metrics_for(&InputTokens::default())
}

/// `[Hp]` How much of the window text a progress bar's groove is filled with
/// over the window: 0.2 times 0.7, as a slider's.
pub const PROGRESS_GROOVE_ALPHA: f64 = 0.2 * 0.7;
/// `[Hp]` How much of the highlight fills a determinate bar's contents over
/// the window.
pub const PROGRESS_CONTENTS_ALPHA: f64 = 0.7;
/// `[Hp]` How far the busy bar's lighter stripes move from the highlight
/// toward the window.
pub const PROGRESS_BUSY_LIGHT_MIX: f64 = 0.7;

/// The dimensions of a `ProgressBar`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeProgressMetrics {
    /// `[qt]` `[measured-qt]` The bar's thickness: Qt's size hint, `H + 8`,
    /// 26 at Plasma's default font, the groove centred in it. A progress bar
    /// is not a target, so it does not rise with the density.
    pub thickness: f32,
    /// `[M]` The groove's thickness.
    pub groove: f32,
    /// `[Hp]` The groove's radius, on the groove inset by half the pen.
    pub radius: f32,
    /// `[Hp]` The period of the busy bar's stripes: one highlight stripe and
    /// one lighter stripe, each half of it.
    pub busy_period: f32,
}

/// The progress-bar dimensions for `typography`. None of them follows the
/// density.
pub fn breeze_progress_metrics_for(typography: &TypographyTokens) -> BreezeProgressMetrics {
    let g = geometry_of(typography);
    BreezeProgressMetrics {
        thickness: g.progress_bar_height,
        groove: PROGRESS_BAR_THICKNESS,
        radius: crate::shape::BREEZE_GROOVE_RADIUS,
        busy_period: crate::geometry::PROGRESS_BAR_BUSY_PERIOD,
    }
}

/// [`breeze_progress_metrics_for`] the Breeze typography.
pub fn breeze_progress_metrics() -> BreezeProgressMetrics {
    breeze_progress_metrics_for(&crate::typography::breeze_typography())
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_tokens::TargetDensity;

    fn at(density: TargetDensity) -> InputTokens {
        InputTokens::for_density(density)
    }

    #[test]
    fn the_compact_button_is_breezes() {
        let m = breeze_button_metrics();
        assert_eq!(m.height, 34.0);
        assert_eq!(m.min_width, 84.0);
        assert_eq!(m.padding_horizontal, 8.0);
        assert_eq!(m.padding_vertical, 8.0);
        // The label's 18 px line box and the two gutters make the height.
        let h = body_line_height(&crate::typography::breeze_typography());
        assert_eq!(h, 18.0);
        assert_eq!(h + 2.0 * m.padding_vertical, m.height);
        // The width rule: max(text + 12, 80) + 4 is text + 16 or 84.
        assert_eq!(2.0 * m.padding_horizontal, 12.0 + 2.0 * FRAME_WIDTH);
    }

    #[test]
    fn the_button_rises_with_the_density_and_never_falls() {
        let t = crate::typography::breeze_typography();
        let compact = breeze_button_metrics_for(&at(TargetDensity::Compact), &t);
        let comfortable = breeze_button_metrics_for(&at(TargetDensity::Comfortable), &t);
        let touch = breeze_button_metrics_for(&at(TargetDensity::Touch), &t);
        assert_eq!(comfortable.height, 34.0, "34 already clears 32");
        assert_eq!(touch.height, 44.0);
        assert!((comfortable.padding_horizontal - 9.2).abs() < 1e-4);
        assert!((touch.padding_horizontal - 10.4).abs() < 1e-4);
        for m in [compact, comfortable, touch] {
            assert_eq!(m.min_width, 84.0);
            assert_eq!(m.padding_vertical, 8.0);
        }
    }

    #[test]
    fn a_larger_body_font_makes_a_taller_button() {
        let mut t = crate::typography::breeze_typography();
        // Plasma's 12 pt: a 16 px face on a 22 px line, which Qt makes 38.
        t.body.size = 16.0;
        t.body.line_height = 22.0 / 16.0;
        let m = breeze_button_metrics_for(&InputTokens::default(), &t);
        assert_eq!(m.height, 38.0);
    }

    #[test]
    fn the_tool_buttons_are_breezes_and_grow_with_the_density() {
        let s = breeze_tool_button_sizes();
        assert_eq!(
            (s.compact, s.default, s.toolbar, s.large, s.hero),
            (24.0, 28.0, 34.0, 44.0, 60.0)
        );
        let comfortable = breeze_tool_button_sizes_for(&at(TargetDensity::Comfortable));
        assert_eq!(
            (
                comfortable.compact,
                comfortable.default,
                comfortable.toolbar
            ),
            (32.0, 32.0, 34.0)
        );
        let touch = breeze_tool_button_sizes_for(&at(TargetDensity::Touch));
        assert_eq!(
            (
                touch.compact,
                touch.default,
                touch.toolbar,
                touch.large,
                touch.hero
            ),
            (44.0, 44.0, 44.0, 44.0, 60.0)
        );
    }

    #[test]
    fn the_field_is_breezes_line_edit_and_grows_with_the_density() {
        let m = breeze_field_metrics();
        assert_eq!(m.height, 32.0);
        assert_eq!(m.padding_horizontal, 6.0);
        let t = crate::typography::breeze_typography();
        let comfortable = breeze_field_metrics_for(&at(TargetDensity::Comfortable), &t);
        let touch = breeze_field_metrics_for(&at(TargetDensity::Touch), &t);
        assert_eq!(comfortable.height, 32.0, "32 already clears 32");
        assert_eq!(touch.height, 44.0);
        assert!((touch.padding_horizontal - 7.8).abs() < 1e-4);
    }

    #[test]
    fn the_indicator_puts_the_label_where_breeze_does() {
        assert_eq!(INDICATOR_LABEL_X, 24.0);
        // The widgets' 6 px gap: an 18 px body.
        let gap = teksilo_widgets::styles::recipe_checkbox_style::CHECKBOX_LABEL_GAP;
        assert_eq!(
            gap,
            teksilo_widgets::styles::recipe_radio_style::RADIO_LABEL_GAP
        );
        assert_eq!(indicator_body_width(gap), 18.0);
        assert_eq!(indicator_body_width(gap) + gap, INDICATOR_LABEL_X);
        // The box ends 2 px inside the indicator, so it stays in the body.
        assert_eq!(INDICATOR_SIZE - INDICATOR_BOX_INSET, 18.0);
        assert_eq!(
            indicator_body_width(12.0),
            18.0,
            "never narrower than the box"
        );
        assert_eq!(indicator_body_width(0.0), 24.0);
    }

    #[test]
    fn the_combo_box_is_breezes_and_grows_with_the_density() {
        let m = breeze_combo_metrics();
        assert_eq!(m.height, 32.0);
        assert_eq!(m.padding_leading, 6.0);
        assert_eq!(m.arrow_gap, 10.0);
        assert_eq!(m.arrow_box, 20.0);
        assert_eq!(m.padding_trailing, 2.0);
        // Breeze's width rule: the label plus 12 + 20 + 2 + 4.
        assert_eq!(m.chrome_width(), 38.0);
        let t = crate::typography::breeze_typography();
        let comfortable = breeze_combo_metrics_for(&at(TargetDensity::Comfortable), &t);
        let touch = breeze_combo_metrics_for(&at(TargetDensity::Touch), &t);
        assert_eq!(comfortable.height, 32.0, "32 already clears 32");
        assert_eq!(touch.height, 44.0);
        for m in [comfortable, touch] {
            assert_eq!(m.arrow_box, 20.0, "the painted box does not scale");
            assert_eq!(m.padding_trailing, 2.0);
            assert!(m.padding_leading > 6.0 && m.arrow_gap > 10.0);
        }
    }

    #[test]
    fn the_spin_box_has_a_twenty_pixel_column_of_two_ten_pixel_halves() {
        let m = breeze_spin_metrics();
        assert_eq!(m.height, 32.0);
        assert_eq!(m.padding_leading, 6.0);
        assert_eq!(m.arrow_width, 20.0);
        assert_eq!(m.padding_trailing, 2.0);
        assert_eq!(BreezeSpinMetrics::arrow_half(32.0), 10.0);
        let t = crate::typography::breeze_typography();
        let touch = breeze_spin_metrics_for(&at(TargetDensity::Touch), &t);
        assert_eq!(touch.height, 44.0);
        assert_eq!(touch.arrow_width, 20.0);
        // The column stops at 20, so a taller frame keeps the halves.
        assert_eq!(BreezeSpinMetrics::arrow_half(touch.height), 10.0);
        // A frame too short for a 20 px column halves what is left, rounded
        // down: 29 tall leaves 17, two 8 px halves.
        assert_eq!(BreezeSpinMetrics::arrow_half(29.0), 8.0);
        assert_eq!(BreezeSpinMetrics::arrow_half(4.0), 0.0);
    }

    #[test]
    fn the_menu_is_breezes_and_grows_with_the_density() {
        let m = breeze_menu_metrics();
        assert_eq!(m.item_height, 29.0);
        assert_eq!(m.separator_height, 5.0);
        assert_eq!(m.section_height, 28.0);
        assert_eq!(m.padding_leading, 12.0);
        assert_eq!(m.icon_column, 16.0);
        assert_eq!(m.icon_label_gap, 3.0);
        assert_eq!(m.trailing_column, 27.0);
        assert_eq!(m.shortcut_gap, 16.0);
        assert_eq!(m.popup_margin, 4.0);
        // The text starts where Breeze puts it in a menu with icons: the
        // 4 px margin, the 8 px text margin, the icon and 3 px after it.
        assert_eq!(m.padding_leading + m.icon_column + m.icon_label_gap, 31.0);
        // And ends 3 px before a 20 px arrow box 4 px inside the row.
        assert_eq!(
            m.trailing_column,
            MENU_ARROW_TEXT_GAP + MENU_ARROW_BOX + MENU_ITEM_MARGIN
        );
        let t = crate::typography::breeze_typography();
        let comfortable = breeze_menu_metrics_for(&at(TargetDensity::Comfortable), &t);
        let touch = breeze_menu_metrics_for(&at(TargetDensity::Touch), &t);
        assert_eq!(comfortable.item_height, 32.0);
        assert_eq!(touch.item_height, 44.0);
        assert_eq!(comfortable.section_height, 32.0);
        assert_eq!(touch.section_height, 44.0);
        assert!((comfortable.separator_height - 5.75).abs() < 1e-4);
        assert!((touch.separator_height - 6.5).abs() < 1e-4);
        for m in [comfortable, touch] {
            assert_eq!(m.icon_column, 16.0, "the painted column does not scale");
            assert_eq!(m.popup_margin, 4.0, "MenuList's inset does not scale");
            assert!(m.trailing_column > 27.0 && m.trailing_column - MENU_ARROW_BOX > 7.0);
            assert!(m.padding_leading > 12.0 && m.shortcut_gap > 16.0);
        }
    }

    #[test]
    fn a_larger_body_font_makes_taller_menu_rows() {
        let mut t = crate::typography::breeze_typography();
        // Plasma's 12 pt: a 22 px line, which Qt makes a 31 px item and a
        // 30 px section.
        t.body.size = 16.0;
        t.body.line_height = 22.0 / 16.0;
        let m = breeze_menu_metrics_for(&InputTokens::default(), &t);
        assert_eq!(m.item_height, 31.0);
        assert_eq!(m.section_height, 30.0);
        assert_eq!(m.separator_height, 5.0, "a separator has no text");
    }

    #[test]
    fn the_rows_are_breezes_and_grow_with_the_density() {
        let m = breeze_row_metrics();
        assert_eq!(m.height, 24.0);
        assert_eq!(m.height_decorated, 26.0);
        assert_eq!(m.height_two_line, 42.0);
        assert_eq!(m.padding_horizontal, 7.0);
        assert_eq!(m.padding_vertical, 1.0);
        assert_eq!(
            (m.selection_inset_horizontal, m.selection_inset_vertical),
            (2.0, 1.0)
        );
        let t = crate::typography::breeze_typography();
        let comfortable = breeze_row_metrics_for(&at(TargetDensity::Comfortable), &t);
        let touch = breeze_row_metrics_for(&at(TargetDensity::Touch), &t);
        assert_eq!(
            (
                comfortable.height,
                comfortable.height_decorated,
                comfortable.height_two_line
            ),
            (32.0, 32.0, 42.0)
        );
        assert_eq!(
            (touch.height, touch.height_decorated, touch.height_two_line),
            (44.0, 44.0, 44.0)
        );
        for m in [comfortable, touch] {
            assert_eq!(m.padding_vertical, 1.0, "holds the outline");
            assert_eq!(m.selection_inset_horizontal, 2.0, "painted");
            assert!(m.padding_horizontal > 7.0);
        }
    }

    #[test]
    fn a_larger_body_font_makes_taller_rows() {
        let mut t = crate::typography::breeze_typography();
        // Plasma's 12 pt: a 22 px line, a 28 px row and a 30 px row with an
        // icon, as Qt measures them.
        t.body.size = 16.0;
        t.body.line_height = 22.0 / 16.0;
        let m = breeze_row_metrics_for(&InputTokens::default(), &t);
        assert_eq!((m.height, m.height_decorated), (28.0, 30.0));
        assert_eq!(m.height_two_line, 50.0);
    }

    /// D7: Qt's table row is 23 px with its grid line; Teksilo's is 24.
    #[test]
    fn d7_the_table_row_is_raised_to_the_target_size() {
        let g = BreezeGeometry::breeze_default();
        assert_eq!(g.table_row_height, 23.0, "Qt's row, below the floor");
        let m = breeze_table_metrics();
        assert_eq!(m.row_height, 24.0);
        assert_eq!(m.header_height, 30.0);
        assert_eq!(m.header_padding, 6.0);
        // The label's line box and the margin above and below fill the
        // header exactly.
        assert_eq!(g.line_height + 2.0 * m.header_padding, m.header_height);
        let t = crate::typography::breeze_typography();
        let touch = breeze_table_metrics_for(&at(TargetDensity::Touch), &t);
        assert_eq!((touch.row_height, touch.header_height), (44.0, 44.0));
        // A larger font's row is Qt's own once it clears the floor.
        let mut big = t;
        big.body.size = 16.0;
        big.body.line_height = 22.0 / 16.0;
        assert_eq!(
            breeze_table_metrics_for(&InputTokens::default(), &big).row_height,
            27.0
        );
    }

    #[test]
    fn the_tab_is_breezes_and_grows_with_the_density() {
        let m = breeze_tab_metrics();
        assert_eq!(m.height, 30.0);
        assert_eq!(m.static_height, 34.0);
        assert_eq!(m.padding_horizontal, 8.0);
        assert_eq!(m.min_width, 80.0);
        assert_eq!(m.indicator, 3.0);
        let t = crate::typography::breeze_typography();
        let comfortable = breeze_tab_metrics_for(&at(TargetDensity::Comfortable), &t);
        let touch = breeze_tab_metrics_for(&at(TargetDensity::Touch), &t);
        assert_eq!(comfortable.height, 32.0);
        assert_eq!(touch.height, 44.0);
        assert!((touch.padding_horizontal - 10.4).abs() < 1e-4);
        assert_eq!(touch.indicator, 3.0, "the painted bar does not scale");
    }

    #[test]
    fn the_scroll_bar_handle_is_breezes_at_every_density() {
        for density in [
            TargetDensity::Compact,
            TargetDensity::Comfortable,
            TargetDensity::Touch,
        ] {
            let tokens = at(density);
            let m = breeze_scroll_bar_metrics_for(&tokens);
            assert_eq!(m.extent, 21.0);
            assert_eq!(m.handle_width, 8.0);
            assert_eq!(m.min_handle_length, 20.0, "{density:?}");
            assert!(m.min_handle_length >= tokens.grab_size);
            assert_eq!(m.separator, 1.0);
            // The handle centred in what the separator leaves is 6 px clear
            // of it on either side.
            assert_eq!((m.extent - m.separator - m.handle_width) / 2.0, 6.0);
        }
    }

    #[test]
    fn the_progress_bar_is_qts_hint_around_breezes_groove() {
        let m = breeze_progress_metrics();
        assert_eq!(m.thickness, 26.0);
        assert_eq!(m.groove, 6.0);
        assert_eq!(m.radius, 3.0);
        assert_eq!(m.busy_period, 28.0);
        assert!((PROGRESS_GROOVE_ALPHA - 0.14).abs() < 1e-12);
    }

    #[test]
    fn every_tool_button_clears_the_conformance_floor_without_a_box() {
        for density in [
            TargetDensity::Compact,
            TargetDensity::Comfortable,
            TargetDensity::Touch,
        ] {
            let tokens = at(density);
            let s = breeze_tool_button_sizes_for(&tokens);
            for size in [s.compact, s.default, s.toolbar, s.large, s.hero] {
                assert!(size >= tokens.min_target_conformance, "{density:?}: {size}");
            }
            assert!(s.compact <= s.default && s.default <= s.toolbar);
            assert!(s.toolbar <= s.large && s.large <= s.hero);
        }
    }
}
