//! Per-component style structs.
//!
//! In Int UI, spacing is per-component, not a sampled global scale. Each
//! widget owns its own dimensions as defaults that consumers can override.
//! Widgets read from `theme.components.<widget>.<field>`.

use serde::{Deserialize, Serialize};

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
}

impl Default for TextFieldStyle {
    fn default() -> Self {
        Self {
            height: 24.0,
            padding_horizontal: 9.0,
            padding_vertical: 4.0,
            border_width: 1.0,
            corner_radius: 4.0,
            caret_width: 1.0,
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
            editor_tab_height: 30.0,
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
    pub corner_radius: f32,
    pub border_width: f32,
}

impl Default for SegmentedControlStyle {
    fn default() -> Self {
        Self {
            height: 24.0,
            padding_horizontal: 10.0,
            corner_radius: 4.0,
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
    pub gutter_thickness: f32,
    pub gutter_handle_size: f32,
    pub corner_radius: f32,
    /// Default minimum size of either pane, in logical pixels.
    pub min_pane_size: f32,
    /// Step size in logical pixels for arrow-key and a11y increment/decrement.
    pub keyboard_step: f32,
}

impl Default for SplitViewStyle {
    fn default() -> Self {
        Self {
            gutter_thickness: 12.0,
            gutter_handle_size: 4.0,
            corner_radius: 2.0,
            min_pane_size: 96.0,
            keyboard_step: 24.0,
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
    pub group_box: GroupBoxStyle,
    pub badge: BadgeStyle,
    pub progress_bar: ProgressBarStyle,
    pub segmented_control: SegmentedControlStyle,
    pub split_button: SplitButtonStyle,
    pub breadcrumb: BreadcrumbStyle,
    pub link: LinkStyle,
    pub wizard: WizardStyle,
    pub snackbar: SnackbarStyle,
    pub divider: DividerStyle,
    pub split_view: SplitViewStyle,
}
