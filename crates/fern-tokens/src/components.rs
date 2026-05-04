//! Per-component style structs.
//!
//! In Int UI, spacing is per-component, not a sampled global scale. Each
//! widget owns its own dimensions as defaults that consumers can override.
//! Widgets read from `theme.components.<widget>.<field>`.

use serde::{Deserialize, Serialize};

use crate::Color;

// ─── Form controls ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ButtonStyle {
    pub height: f32,
    pub min_width: f32,
    pub padding_horizontal: f32,
    pub padding_vertical: f32,
    pub corner_radius: f32,
    pub border_width: f32,
    pub focus_ring_width: f32,
    pub focus_ring_offset: f32,
    pub icon_size: f32,
    pub icon_label_gap: f32,
}

impl Default for ButtonStyle {
    fn default() -> Self {
        Self {
            height: 24.0,
            min_width: 72.0,
            padding_horizontal: 14.0,
            padding_vertical: 0.0,
            corner_radius: 4.0,
            border_width: 1.0,
            focus_ring_width: 2.0,
            focus_ring_offset: 2.0,
            icon_size: 16.0,
            icon_label_gap: 4.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct IconButtonStyle {
    pub size_compact: f32,
    pub size_default: f32,
    pub size_large: f32,
    pub icon_size: f32,
    pub corner_radius: f32,
}

impl Default for IconButtonStyle {
    fn default() -> Self {
        Self {
            size_compact: 22.0,
            size_default: 24.0,
            size_large: 30.0,
            icon_size: 16.0,
            corner_radius: 4.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TextFieldStyle {
    pub height: f32,
    pub padding_horizontal: f32,
    pub padding_vertical: f32,
    pub border_width: f32,
    pub corner_radius: f32,
    pub caret_width: f32,
    /// Vertical gap between the field bottom edge and the inline
    /// validation-feedback strip. Int UI density convention.
    pub validation_strip_gap: f32,
    /// Pulse duration (ms) for the brief border-color attention cue
    /// on transitions into `Error`. Int UI uses a short single-shot
    /// pulse rather than a Material-style shake.
    pub error_pulse_duration_ms: u32,
    /// Decay window (ms) for the accent border tint + correction text
    /// after a `Corrected` outcome. After this elapses, the field
    /// returns to `None` state — the value is correct now, nothing
    /// to keep flagging.
    pub corrected_pulse_duration_ms: u32,
    /// Visible character used for unfilled editable positions when an
    /// input mask is set. Empty fields with a mask paint this char at
    /// every editable position — `__/__/____` for `99/99/9999`.
    pub mask_placeholder_char: char,
}

impl Default for TextFieldStyle {
    fn default() -> Self {
        Self {
            height: 28.0,
            padding_horizontal: 4.0,
            padding_vertical: 4.0,
            border_width: 1.0,
            corner_radius: 4.0,
            caret_width: 1.0,
            validation_strip_gap: 4.0,
            error_pulse_duration_ms: 240,
            corrected_pulse_duration_ms: 1500,
            mask_placeholder_char: '_',
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TextAreaStyle {
    pub min_height: f32,
    pub padding_horizontal: f32,
    pub padding_vertical: f32,
    pub corner_radius: f32,
}

impl Default for TextAreaStyle {
    fn default() -> Self {
        Self {
            min_height: 60.0,
            padding_horizontal: 9.0,
            padding_vertical: 6.0,
            corner_radius: 4.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CheckboxStyle {
    pub box_visual_size: f32,
    pub box_hit_area: f32,
    pub label_gap: f32,
    pub corner_radius: f32,
}

impl Default for CheckboxStyle {
    fn default() -> Self {
        Self {
            box_visual_size: 19.0,
            box_hit_area: 24.0,
            label_gap: 6.0,
            corner_radius: 3.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RadioStyle {
    pub visual_size: f32,
    pub hit_area: f32,
    pub label_gap: f32,
    pub inner_dot_size: f32,
}

impl Default for RadioStyle {
    fn default() -> Self {
        Self {
            visual_size: 19.0,
            hit_area: 24.0,
            label_gap: 6.0,
            inner_dot_size: 7.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ToggleStyle {
    pub track_width: f32,
    pub track_height: f32,
    pub thumb_diameter: f32,
    pub thumb_inset: f32,
    pub label_gap: f32,
}

impl Default for ToggleStyle {
    fn default() -> Self {
        Self {
            track_width: 28.0,
            track_height: 16.0,
            thumb_diameter: 12.0,
            thumb_inset: 2.0,
            label_gap: 6.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ComboBoxStyle {
    pub height: f32,
    pub padding_horizontal: f32,
    pub arrow_column_width: f32,
    pub corner_radius: f32,
}

impl Default for ComboBoxStyle {
    fn default() -> Self {
        Self {
            height: 24.0,
            padding_horizontal: 9.0,
            arrow_column_width: 23.0,
            corner_radius: 4.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SliderStyle {
    pub track_height: f32,
    pub thumb_diameter: f32,
    pub tick_size: f32,
}

impl Default for SliderStyle {
    fn default() -> Self {
        Self {
            track_height: 4.0,
            thumb_diameter: 14.0,
            tick_size: 2.0,
        }
    }
}

// ─── Navigation / chrome ────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TabStyle {
    pub editor_tab_height: f32,
    pub tool_window_tab_height: f32,
    pub padding_horizontal: f32,
    pub underline_active: f32,
    pub underline_hover: f32,
    pub close_button_size: f32,
}

impl Default for TabStyle {
    fn default() -> Self {
        Self {
            editor_tab_height: 50.0,
            tool_window_tab_height: 28.0,
            padding_horizontal: 12.0,
            underline_active: 3.0,
            underline_hover: 2.0,
            close_button_size: 16.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ToolbarStyle {
    pub height_compact: f32,
    pub height_default: f32,
    pub button_size_compact: f32,
    pub button_size_default: f32,
    pub icon_size: f32,
    pub separator_width: f32,
    pub separator_inset: f32,
}

impl Default for ToolbarStyle {
    fn default() -> Self {
        Self {
            height_compact: 30.0,
            height_default: 40.0,
            button_size_compact: 22.0,
            button_size_default: 30.0,
            icon_size: 16.0,
            separator_width: 1.0,
            separator_inset: 4.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StatusBarStyle {
    pub height: f32,
    pub padding_horizontal: f32,
    pub item_gap: f32,
}

impl Default for StatusBarStyle {
    fn default() -> Self {
        Self {
            height: 22.0,
            padding_horizontal: 8.0,
            item_gap: 2.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MenuStyle {
    pub item_height: f32,
    pub item_padding_horizontal: f32,
    pub icon_column_width: f32,
    pub icon_label_gap: f32,
    pub shortcut_left_gap: f32,
    pub separator_height: f32,
    pub popup_corner_radius: f32,
    pub popup_border_width: f32,
}

impl Default for MenuStyle {
    fn default() -> Self {
        Self {
            item_height: 24.0,
            item_padding_horizontal: 12.0,
            icon_column_width: 16.0,
            icon_label_gap: 6.0,
            shortcut_left_gap: 24.0,
            separator_height: 9.0,
            popup_corner_radius: 8.0,
            popup_border_width: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TooltipStyle {
    pub padding_horizontal: f32,
    pub padding_vertical: f32,
    pub corner_radius: f32,
    pub max_width: f32,
}

impl Default for TooltipStyle {
    fn default() -> Self {
        Self {
            padding_horizontal: 10.0,
            padding_vertical: 6.0,
            corner_radius: 8.0,
            max_width: 320.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScrollBarStyle {
    pub thickness_idle: f32,
    pub thickness_hover: f32,
    pub min_thumb_length: f32,
    pub corner_radius: f32,
}

impl Default for ScrollBarStyle {
    fn default() -> Self {
        Self {
            thickness_idle: 4.0,
            thickness_hover: 8.0,
            min_thumb_length: 24.0,
            corner_radius: 2.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TreeListStyle {
    pub row_height: f32,
    pub indent_per_level: f32,
    pub expand_icon_size: f32,
    pub icon_label_gap: f32,
}

impl Default for TreeListStyle {
    fn default() -> Self {
        Self {
            row_height: 24.0,
            indent_per_level: 16.0,
            expand_icon_size: 12.0,
            icon_label_gap: 4.0,
        }
    }
}

// ─── Surfaces ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DialogStyle {
    pub content_padding: f32,
    pub button_row_gap: f32,
    pub button_row_top_gap: f32,
    pub min_width: f32,
    pub corner_radius: f32,
}

impl Default for DialogStyle {
    fn default() -> Self {
        Self {
            content_padding: 24.0,
            button_row_gap: 8.0,
            button_row_top_gap: 24.0,
            min_width: 280.0,
            corner_radius: 8.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NotificationStyle {
    pub padding_horizontal: f32,
    pub padding_vertical: f32,
    pub corner_radius: f32,
    pub icon_size: f32,
    pub icon_content_gap: f32,
    pub max_width: f32,
}

impl Default for NotificationStyle {
    fn default() -> Self {
        Self {
            padding_horizontal: 12.0,
            padding_vertical: 10.0,
            corner_radius: 8.0,
            icon_size: 16.0,
            icon_content_gap: 8.0,
            max_width: 360.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PanelStyle {
    pub padding: f32,
    pub corner_radius: f32,
    pub border_width: f32,
}

impl Default for PanelStyle {
    fn default() -> Self {
        Self {
            padding: 12.0,
            corner_radius: 8.0,
            border_width: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CardStyle {
    pub padding: f32,
    pub corner_radius: f32,
    pub border_width: f32,
}

impl Default for CardStyle {
    fn default() -> Self {
        Self {
            padding: 16.0,
            corner_radius: 8.0,
            border_width: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PopoverStyle {
    pub padding: f32,
    pub corner_radius: f32,
    pub border_width: f32,
}

impl Default for PopoverStyle {
    fn default() -> Self {
        Self {
            padding: 12.0,
            corner_radius: 8.0,
            border_width: 1.0,
        }
    }
}

// ─── Display widgets ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GroupBoxStyle {
    /// Leading indent applied to the content area so it aligns under the
    /// title text (not under the checkbox box).
    pub content_indent: f32,
    /// Vertical space between the title row and the content.
    pub title_content_spacing: f32,
    /// Horizontal gap between the optional checkbox and the title text.
    pub checkbox_gap: f32,
}

impl Default for GroupBoxStyle {
    fn default() -> Self {
        Self {
            content_indent: 24.0,
            title_content_spacing: 8.0,
            checkbox_gap: 6.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AccordionStyle {
    pub header_height: f32,
    pub header_padding_horizontal: f32,
    pub indicator_size: f32,
    pub indicator_gap: f32,
    pub corner_radius: f32,
}

impl Default for AccordionStyle {
    fn default() -> Self {
        Self {
            header_height: 28.0,
            header_padding_horizontal: 8.0,
            indicator_size: 12.0,
            indicator_gap: 6.0,
            corner_radius: 4.0,
        }
    }
}

/// ToolBox — vertically stacked collapsible sections, one expanded at a time.
///
/// Headers are flat (no corner radius per Int UI) and draw a 1 dp accent bar
/// on the leading edge of the active row. The animation duration and easing
/// are read from `theme.motion` — ToolBoxStyle only carries layout constants.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ToolBoxStyle {
    pub header_min_height: f32,
    pub header_padding_horizontal: f32,
    pub icon_text_spacing: f32,
    pub chevron_size: f32,
    pub indicator_thickness: f32,
}

impl Default for ToolBoxStyle {
    fn default() -> Self {
        Self {
            header_min_height: 28.0,
            header_padding_horizontal: 12.0,
            icon_text_spacing: 8.0,
            chevron_size: 12.0,
            indicator_thickness: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BadgeStyle {
    pub padding_horizontal: f32,
    pub padding_vertical: f32,
    pub min_height: f32,
    pub corner_radius: f32,
}

impl Default for BadgeStyle {
    fn default() -> Self {
        Self {
            padding_horizontal: 6.0,
            padding_vertical: 1.0,
            min_height: 16.0,
            corner_radius: 9999.0,
        }
    }
}

/// Avatar — circular (or rounded/square) user-identity widget.
///
/// Sizes follow the IconButton convention of three-step discrete variants
/// plus an `XLarge` step for profile cards. Initials font sizing scales
/// with the avatar diameter (Ant Design's "gap" idea); single-character
/// avatars get the larger ratio so a single letter has visual weight,
/// two-character avatars use the smaller ratio to fit comfortably.
///
/// Presence-dot geometry is expressed as a ratio of the avatar size with
/// min/max clamps so a 24-px avatar still has a legible dot and a 64-px
/// one doesn't get an oversized one. The outline width gives the
/// punched-out look that reads against any underlying surface.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AvatarStyle {
    pub size_small: f32,
    pub size_medium: f32,
    pub size_large: f32,
    pub size_x_large: f32,

    /// Default border (ring) thickness when `.border()` is called
    /// without an explicit width override.
    pub border_default: f32,

    /// Presence dot diameter as a fraction of avatar diameter, with
    /// hard min/max clamps so the dot stays legible at small sizes
    /// and proportionate at large sizes.
    pub presence_diameter_ratio: f32,
    pub presence_diameter_min: f32,
    pub presence_diameter_max: f32,
    /// Outline drawn around the presence dot — punches it visually
    /// out of the avatar regardless of the avatar background.
    pub presence_outline_width: f32,
    /// Inset of the presence dot from the avatar's bounding box edge.
    pub presence_inset: f32,

    /// Initials font-size as a fraction of avatar diameter.
    /// `font_ratio_1char` for one-grapheme initials, `font_ratio_2char`
    /// for two-grapheme.
    pub font_ratio_1char: f32,
    pub font_ratio_2char: f32,

    /// Corner-radius ratio for `AvatarShape::RoundedSquare`. 0.5 ⇒
    /// fully circular, 0.0 ⇒ square corners.
    pub rounded_radius_ratio: f32,
}

impl Default for AvatarStyle {
    fn default() -> Self {
        Self {
            size_small: 24.0,
            size_medium: 32.0,
            size_large: 48.0,
            size_x_large: 64.0,
            border_default: 2.0,
            presence_diameter_ratio: 0.30,
            presence_diameter_min: 8.0,
            presence_diameter_max: 14.0,
            presence_outline_width: 1.5,
            presence_inset: 2.0,
            font_ratio_1char: 0.50,
            font_ratio_2char: 0.42,
            rounded_radius_ratio: 0.25,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProgressBarStyle {
    pub height: f32,
    pub corner_radius: f32,
}

impl Default for ProgressBarStyle {
    fn default() -> Self {
        Self {
            height: 4.0,
            corner_radius: 2.0,
        }
    }
}

/// A button split into two adjacent regions (a default action on the left
/// and a chevron that opens a dropdown menu on the right), sharing a single
/// rounded border. The main region's dimensions mirror [`ButtonStyle`] so
/// the two controls can sit next to each other without visual drift.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SplitButtonStyle {
    pub height: f32,
    pub min_width: f32,
    pub padding_horizontal: f32,
    pub padding_vertical: f32,
    pub corner_radius: f32,
    pub border_width: f32,
    /// Fixed width of the trailing chevron region.
    pub chevron_width: f32,
    /// Thickness of the vertical divider between the main region and the
    /// chevron region.
    pub divider_width: f32,
    /// Pixel size of the chevron glyph drawn inside the chevron region.
    pub chevron_icon_size: f32,
    pub focus_ring_width: f32,
    pub focus_ring_offset: f32,
}

impl Default for SplitButtonStyle {
    fn default() -> Self {
        Self {
            // Dimensions mirror ButtonStyle so a Button and a SplitButton
            // sit on the same baseline.
            height: 24.0,
            min_width: 72.0,
            padding_horizontal: 14.0,
            padding_vertical: 0.0,
            corner_radius: 4.0,
            border_width: 1.0,
            // 22 dp matches ComboBoxStyle::arrow_column_width (minus the
            // combo's internal padding) so the chevron affordance is
            // visually consistent across dropdown-opening controls.
            chevron_width: 22.0,
            divider_width: 1.0,
            chevron_icon_size: 12.0,
            focus_ring_width: 2.0,
            focus_ring_offset: 2.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SegmentedControlStyle {
    pub height: f32,
    pub padding_horizontal: f32,
    pub padding_vertical: f32,
    pub corner_radius: f32,
    pub border_width: f32,
}

impl Default for SegmentedControlStyle {
    fn default() -> Self {
        Self {
            height: 24.0,
            padding_horizontal: 12.0,
            padding_vertical: 6.0,
            corner_radius: 3.0,
            border_width: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BreadcrumbStyle {
    pub item_height: f32,
    pub item_padding_horizontal: f32,
    pub separator_gap: f32,
    pub corner_radius: f32,
}

impl Default for BreadcrumbStyle {
    fn default() -> Self {
        Self {
            item_height: 20.0,
            item_padding_horizontal: 6.0,
            separator_gap: 4.0,
            corner_radius: 4.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LinkStyle {
    pub padding_horizontal: f32,
    pub padding_vertical: f32,
    pub corner_radius: f32,
}

impl Default for LinkStyle {
    fn default() -> Self {
        Self {
            padding_horizontal: 2.0,
            padding_vertical: 0.0,
            corner_radius: 4.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WizardStyle {
    pub content_padding: f32,
    pub step_indicator_size: f32,
    pub step_indicator_gap: f32,
}

impl Default for WizardStyle {
    fn default() -> Self {
        Self {
            content_padding: 24.0,
            step_indicator_size: 24.0,
            step_indicator_gap: 8.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SnackbarStyle {
    pub padding_horizontal: f32,
    pub padding_vertical: f32,
    pub corner_radius: f32,
    pub border_width: f32,
}

impl Default for SnackbarStyle {
    fn default() -> Self {
        Self {
            padding_horizontal: 12.0,
            padding_vertical: 10.0,
            corner_radius: 8.0,
            border_width: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DividerStyle {
    pub thickness: f32,
}

impl Default for DividerStyle {
    fn default() -> Self {
        Self { thickness: 1.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SplitViewStyle {
    /// Width of the gutter hit area in logical pixels. The visible
    /// divider is a 1px line centered in this area; the rest is
    /// invisible padding to give pointer interaction a comfortable
    /// target.
    pub gutter_thickness: f32,
    /// Visible thickness of the divider line painted at the center of
    /// the gutter, in logical pixels.
    pub divider_line_thickness: f32,
    /// Default minimum size of either pane, in logical pixels.
    pub min_pane_size: f32,
    /// Step size in logical pixels for arrow-key and a11y increment/decrement.
    pub keyboard_step: f32,
}

impl Default for SplitViewStyle {
    fn default() -> Self {
        Self {
            gutter_thickness: 6.0,
            divider_line_thickness: 1.0,
            min_pane_size: 96.0,
            keyboard_step: 24.0,
        }
    }
}

// ─── Charts ─────────────────────────────────────────────────────────────────

/// Style tokens for `fern-charts` (BarChart, LineChart, PieChart).
///
/// Charts pull their *colors* from theme roles + `ColorTokens::chart_palette`,
/// so this struct only carries dimensions. Themes override individual fields
/// to nudge density without touching layout code.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ChartStyle {
    pub plot_padding_top: f32,
    pub plot_padding_right: f32,
    pub plot_padding_bottom: f32,
    pub plot_padding_leading: f32,
    pub axis_tick_length: f32,
    pub axis_label_gap: f32,
    pub axis_title_gap: f32,
    pub gridline_width: f32,
    pub bar_min_width: f32,
    pub line_default_width: f32,
    pub point_default_radius: f32,
    pub legend_swatch_size: f32,
    pub legend_item_gap: f32,
    pub legend_to_plot_gap: f32,
    pub tooltip_padding: f32,
    pub pie_padding: f32,
    pub pie_label_gap: f32,
    pub pie_leader_length: f32,
    pub pie_min_slice_label_degrees: f32,
    pub donut_default_inner_ratio: f32,
}

impl Default for ChartStyle {
    fn default() -> Self {
        Self {
            plot_padding_top: 12.0,
            plot_padding_right: 12.0,
            plot_padding_bottom: 4.0,
            plot_padding_leading: 4.0,
            axis_tick_length: 4.0,
            axis_label_gap: 4.0,
            axis_title_gap: 8.0,
            gridline_width: 1.0,
            bar_min_width: 4.0,
            line_default_width: 1.5,
            point_default_radius: 3.0,
            legend_swatch_size: 10.0,
            legend_item_gap: 12.0,
            legend_to_plot_gap: 8.0,
            tooltip_padding: 8.0,
            pie_padding: 8.0,
            pie_label_gap: 4.0,
            pie_leader_length: 12.0,
            pie_min_slice_label_degrees: 12.0,
            donut_default_inner_ratio: 0.55,
        }
    }
}

// ─── Tabular ────────────────────────────────────────────────────────────────

/// Style tokens for `TableView` and `TreeTable`.
///
/// These are dimensions only; colors come from theme roles
/// (`SurfaceRole::Selected` / `Hover` / `AltRow` / `Raised`,
/// `BorderRole::Divider` / `DividerStrong` / `Focused`,
/// `TextRole::Primary` / `Secondary` / `Accent`). Both widgets snapshot
/// the layout numbers at build time so a theme change repaints without
/// rebuilding the row tree.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TableStyle {
    /// Body row height. Headers use `header_height`.
    pub row_height: f32,
    /// Sticky header row height.
    pub header_height: f32,
    /// Horizontal padding inside each cell (also applied to header cells).
    pub cell_padding_horizontal: f32,
    /// Vertical padding inside each cell.
    pub cell_padding_vertical: f32,
    /// Width of the right-edge resize hit zone on header cells.
    pub resize_handle_width: f32,
    /// Stroke width of grid lines drawn between rows / columns.
    pub grid_line_thickness: f32,
    /// Outer-frame corner radius.
    pub corner_radius: f32,
    /// Edge length of the sort-direction chevron in the header.
    pub sort_indicator_size: f32,
    /// Edge length of the filter glyph in the header.
    pub filter_indicator_size: f32,
    /// Spacing between adjacent header cells (in addition to grid lines).
    pub header_inter_cell_spacing: f32,
    /// Inset between the focused-cell bounds and the focus-ring stroke.
    pub focus_ring_inset: f32,
    /// Default minimum column width, used when a column does not set its own.
    pub min_column_width_default: f32,
    /// `TreeTable` only — pixels per indent level on the tree column.
    pub tree_indent_per_level: f32,
    /// `TreeTable` only — edge length of the twist (expand/collapse) chevron.
    pub tree_twist_size: f32,
    /// `TreeTable` only — gap between the twist chevron and the cell content.
    pub tree_twist_label_gap: f32,
}

impl Default for TableStyle {
    fn default() -> Self {
        Self {
            row_height: 28.0,
            header_height: 32.0,
            cell_padding_horizontal: 8.0,
            cell_padding_vertical: 4.0,
            resize_handle_width: 4.0,
            grid_line_thickness: 1.0,
            corner_radius: 4.0,
            sort_indicator_size: 10.0,
            filter_indicator_size: 12.0,
            header_inter_cell_spacing: 0.0,
            focus_ring_inset: 1.0,
            min_column_width_default: 32.0,
            tree_indent_per_level: 16.0,
            tree_twist_size: 12.0,
            tree_twist_label_gap: 4.0,
        }
    }
}

// ─── Date / Time pickers ────────────────────────────────────────────────────

/// `Calendar` — month grid + navigation header.
///
/// Dimensions only; colors come from `SurfaceRole::Selected` / `Hover`,
/// `BorderRole::Focused`, `TextRole::Primary` / `Secondary` / `Disabled` /
/// `Accent`. Pulls layout numbers at build time, so a runtime theme switch
/// repaints without rebuilding the cell tree.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CalendarStyle {
    /// Outer padding around the whole widget.
    pub outer_padding: f32,
    /// Vertical gap between header / weekday row / day grid / footer.
    pub section_gap: f32,
    /// Height of the navigation header row (prev / label / next).
    pub header_height: f32,
    /// Height of the weekday-name row.
    pub weekday_row_height: f32,
    /// Side length of each day cell (square cells).
    pub cell_size: f32,
    /// Visible day-cell content radius (selection fill).
    pub cell_radius: f32,
    /// Gap between day cells in both axes.
    pub cell_gap: f32,
    /// Stroke width of the today ring.
    pub today_ring_width: f32,
    /// Diameter of the marker dot painted under marked dates.
    pub marker_dot_size: f32,
    /// Horizontal gap between the marker dot and the cell number's
    /// baseline (vertical inset). Layout-only.
    pub marker_inset: f32,
    /// Edge length of header navigation arrow icons.
    pub nav_icon_size: f32,
    /// Width of the optional week-number column.
    pub week_number_column_width: f32,
}

impl Default for CalendarStyle {
    fn default() -> Self {
        Self {
            outer_padding: 8.0,
            section_gap: 4.0,
            header_height: 28.0,
            weekday_row_height: 20.0,
            cell_size: 32.0,
            cell_radius: 4.0,
            cell_gap: 0.0,
            today_ring_width: 1.0,
            marker_dot_size: 4.0,
            marker_inset: 2.0,
            nav_icon_size: 12.0,
            week_number_column_width: 28.0,
        }
    }
}

/// `DateEdit` — segmented date editor with optional calendar popover.
///
/// Extends the `TextFieldStyle` baseline with a fixed-size calendar
/// trigger button slot, a literal-text gap between segments, and a
/// focused-segment tint. Field height / border / corner radius are
/// inherited from `TextFieldStyle` at build time so DateEdit and
/// TextInput / SpinBox sit on the same baseline.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DateEditStyle {
    /// Width of the trailing calendar-button slot. The trigger icon is
    /// centered inside this slot.
    pub calendar_button_width: f32,
    /// Edge length of the calendar trigger glyph drawn inside the slot.
    pub calendar_icon_size: f32,
    /// Gap between adjacent segments (visual separator characters sit
    /// inside this gap).
    pub segment_gap: f32,
}

impl Default for DateEditStyle {
    fn default() -> Self {
        Self {
            calendar_button_width: 24.0,
            calendar_icon_size: 14.0,
            segment_gap: 1.0,
        }
    }
}

/// `TimeEdit` — segmented time-of-day editor.
///
/// Same baseline as `DateEditStyle`; the AM/PM segment is a small fixed
/// width because the toggle never reads anything but `"AM"` / `"PM"` (or
/// the localized 2-letter equivalent).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TimeEditStyle {
    /// Width of the AM/PM segment in 12-hour mode.
    pub period_segment_width: f32,
    /// Gap between adjacent segments.
    pub segment_gap: f32,
}

impl Default for TimeEditStyle {
    fn default() -> Self {
        Self {
            period_segment_width: 28.0,
            segment_gap: 1.0,
        }
    }
}

/// `ColorPicker` — composite color selector (HSV canvas + hue/alpha
/// strips + RGB/HSV spinners + hex input + swatch grid + preview).
///
/// All sizes are theme defaults; the picker is otherwise self-laid-out
/// per `ColorPickerLayout` (Compact / Standard / Wide). Indicator colors
/// are absolute (white / dark) so the HSV-canvas indicator stays visible
/// against any hue/value combination, regardless of theme. Checkerboard
/// colors are deliberately neutral so transparent swatches read clearly
/// against either light or dark surrounding chrome.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ColorPickerStyle {
    /// HSV (saturation × value) canvas dimensions.
    pub canvas_width: f32,
    pub canvas_height: f32,
    pub canvas_corner_radius: f32,

    /// 1D hue and alpha strips (vertical orientation in default layouts).
    pub strip_thickness: f32,
    pub strip_length: f32,
    pub strip_corner_radius: f32,

    /// HSV-canvas indicator: a double-ring (white outer, dark inner) so it
    /// stays visible on every hue × value combination.
    pub indicator_radius: f32,
    pub indicator_outer_stroke_width: f32,
    pub indicator_inner_stroke_width: f32,
    pub indicator_outer_color: Color,
    pub indicator_inner_color: Color,

    /// Strip thumb (the bar across hue/alpha sliders).
    pub strip_thumb_width: f32,
    pub strip_thumb_height: f32,
    pub strip_thumb_corner_radius: f32,

    /// Outer padding inside the picker frame and inter-section gap.
    pub padding: f32,
    pub gap: f32,

    /// Preset swatch cells.
    pub swatch_size: f32,
    pub swatch_spacing: f32,
    pub swatch_corner_radius: f32,
    pub swatch_selected_stroke_width: f32,

    /// Checkerboard pattern (alpha visualization on swatches and the
    /// alpha strip background).
    pub checker_cell: f32,
    pub checker_color_a: Color,
    pub checker_color_b: Color,

    /// Current-color preview swatch (inside the picker, distinct from the
    /// individual ColorSwatch sizes).
    pub preview_width: f32,
    pub preview_height: f32,
    pub preview_corner_radius: f32,

    /// RGB / HSV spinner cell width and hex-input field width.
    pub spinner_field_width: f32,
    pub hex_field_width: f32,
}

impl Default for ColorPickerStyle {
    fn default() -> Self {
        Self {
            canvas_width: 224.0,
            canvas_height: 192.0,
            canvas_corner_radius: 4.0,

            strip_thickness: 14.0,
            strip_length: 192.0,
            strip_corner_radius: 4.0,

            indicator_radius: 7.0,
            indicator_outer_stroke_width: 1.5,
            indicator_inner_stroke_width: 1.0,
            indicator_outer_color: Color::WHITE,
            indicator_inner_color: Color::new(0.0, 0.0, 0.0, 0.6),

            strip_thumb_width: 18.0,
            strip_thumb_height: 8.0,
            strip_thumb_corner_radius: 2.0,

            padding: 12.0,
            gap: 10.0,

            swatch_size: 22.0,
            swatch_spacing: 6.0,
            swatch_corner_radius: 4.0,
            swatch_selected_stroke_width: 2.0,

            checker_cell: 6.0,
            checker_color_a: Color::new(0.78, 0.78, 0.78, 1.0),
            checker_color_b: Color::WHITE,

            preview_width: 64.0,
            preview_height: 28.0,
            preview_corner_radius: 4.0,

            spinner_field_width: 56.0,
            hex_field_width: 96.0,
        }
    }
}

// ─── Aggregate ──────────────────────────────────────────────────────────────

/// All per-component style structs, owned by the [`crate::theme::Theme`].
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct ComponentStyles {
    pub button: ButtonStyle,
    pub icon_button: IconButtonStyle,
    pub text_field: TextFieldStyle,
    pub text_area: TextAreaStyle,
    pub checkbox: CheckboxStyle,
    pub radio: RadioStyle,
    pub toggle: ToggleStyle,
    pub combo_box: ComboBoxStyle,
    pub slider: SliderStyle,
    pub tab: TabStyle,
    pub toolbar: ToolbarStyle,
    pub status_bar: StatusBarStyle,
    pub menu: MenuStyle,
    pub tooltip: TooltipStyle,
    pub scrollbar: ScrollBarStyle,
    pub tree_list: TreeListStyle,
    pub dialog: DialogStyle,
    pub notification: NotificationStyle,
    pub panel: PanelStyle,
    pub card: CardStyle,
    pub popover: PopoverStyle,
    pub accordion: AccordionStyle,
    pub tool_box: ToolBoxStyle,
    pub group_box: GroupBoxStyle,
    pub badge: BadgeStyle,
    pub avatar: AvatarStyle,
    pub progress_bar: ProgressBarStyle,
    pub segmented_control: SegmentedControlStyle,
    pub split_button: SplitButtonStyle,
    pub breadcrumb: BreadcrumbStyle,
    pub link: LinkStyle,
    pub wizard: WizardStyle,
    pub snackbar: SnackbarStyle,
    pub divider: DividerStyle,
    pub split_view: SplitViewStyle,
    pub chart: ChartStyle,
    pub table: TableStyle,
    pub calendar: CalendarStyle,
    pub date_edit: DateEditStyle,
    pub time_edit: TimeEditStyle,
    pub color_picker: ColorPickerStyle,
}
