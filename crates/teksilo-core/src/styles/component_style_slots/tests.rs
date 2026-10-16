// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The roster [`ComponentStyleSlots::NAMES`] is checked against a value built
//! with a struct literal, which the compiler, not `for_every_slot!`, holds to
//! every field.

use std::collections::BTreeSet;
use std::rc::Rc;

use super::ComponentStyleSlots;
use crate::build_context::BuildContext;
use crate::styles::*;
use crate::widget_id::WidgetId;

/// One type standing in for every Tier-3 style. Nothing here builds a widget:
/// the slots are only counted, compared and printed.
struct Stub;

macro_rules! stub_make_body {
    ($($trait:ident => $config:ident),* $(,)?) => {
        $(impl $trait for Stub {
            fn make_body(&self, _: &$config, _: &mut BuildContext) -> WidgetId {
                unreachable!("the stub style is never built")
            }
        })*
    };
}

stub_make_body!(
    AvatarStyle => AvatarStyleConfig,
    BadgeStyle => BadgeStyleConfig,
    BannerStyle => BannerStyleConfig,
    ButtonStyle => ButtonStyleConfig,
    CardStyle => CardStyleConfig,
    CheckboxStyle => CheckboxStyleConfig,
    ColorPickerStyle => ColorPickerStyleConfig,
    ComboBoxStyle => ComboBoxStyleConfig,
    DateEditStyle => DateEditStyleConfig,
    DropTargetStyle => DropTargetStyleConfig,
    DropZoneStyle => DropZoneStyleConfig,
    IconButtonStyle => IconButtonStyleConfig,
    LinkStyle => LinkStyleConfig,
    MenuItemStyle => MenuItemStyleConfig,
    PanelStyle => PanelStyleConfig,
    PopoverStyle => PopoverStyleConfig,
    ProgressBarStyle => ProgressBarStyleConfig,
    RadioStyle => RadioStyleConfig,
    RadioTileStyle => RadioTileStyleConfig,
    RichTextEditorStyle => RichTextEditorStyleConfig,
    ScrollBarStyle => ScrollBarStyleConfig,
    SearchFieldStyle => SearchFieldStyleConfig,
    SegmentedControlStyle => SegmentedControlStyleConfig,
    SliderStyle => SliderStyleConfig,
    SnackbarStyle => SnackbarStyleConfig,
    SpinBoxStyle => SpinBoxStyleConfig,
    SplitButtonStyle => SplitButtonStyleConfig,
    StandardItemStyle => StandardItemStyleConfig,
    TextInputStyle => TextInputStyleConfig,
    ToastStyle => ToastStyleConfig,
    ToggleStyle => ToggleStyleConfig,
    TooltipStyle => TooltipStyleConfig,
    WebViewStyle => WebViewStyleConfig,
);

impl CalendarStyle for Stub {
    fn make_day_cell(&self, _: &CalendarDayConfig, _: &mut BuildContext) -> WidgetId {
        unreachable!("the stub style is never built")
    }
    fn make_zoom_cell(&self, _: &CalendarZoomCellConfig, _: &mut BuildContext) -> WidgetId {
        unreachable!("the stub style is never built")
    }
    fn make_header(&self, _: &CalendarHeaderConfig, _: &mut BuildContext) -> WidgetId {
        unreachable!("the stub style is never built")
    }
}

impl ChartStyle for Stub {
    fn bar_fill(&self, _: &ChartFillContext) -> FillRecipe {
        unreachable!("the stub style is never built")
    }
    fn area_fill(&self, _: &ChartFillContext, _: f32) -> FillRecipe {
        unreachable!("the stub style is never built")
    }
    fn donut_fill(&self, _: &ChartFillContext) -> FillRecipe {
        unreachable!("the stub style is never built")
    }
    fn gridline(&self, _: &Theme) -> BorderRecipe {
        unreachable!("the stub style is never built")
    }
}

impl DialogStyle for Stub {
    fn make_panel(&self, _: &DialogStyleConfig, _: &mut BuildContext) -> WidgetId {
        unreachable!("the stub style is never built")
    }
    fn make_scrim(&self, _: &mut BuildContext) -> WidgetId {
        unreachable!("the stub style is never built")
    }
}

impl GridViewStyle for Stub {}

impl ListContainerStyle for Stub {
    fn make_insertion_indicator(&self, _: &ListInsertionConfig, _: &mut BuildContext) -> WidgetId {
        unreachable!("the stub style is never built")
    }
    fn insertion(&self) -> ListInsertionRecipe {
        unreachable!("the stub style is never built")
    }
}

impl SplitterStyle for Stub {
    fn make_handle(&self, _: &SplitterStyleConfig, _: &mut BuildContext) -> WidgetId {
        unreachable!("the stub style is never built")
    }
}

impl TabStyle for Stub {
    fn make_body(&self, _: &TabStyleConfig, _: &mut BuildContext) -> WidgetId {
        unreachable!("the stub style is never built")
    }
    fn make_bar(&self, _: &TabBarChromeConfig, _: &mut BuildContext) -> WidgetId {
        unreachable!("the stub style is never built")
    }
}

impl TableStyle for Stub {
    fn make_header_cell(&self, _: &TableHeaderCellConfig, _: &mut BuildContext) -> WidgetId {
        unreachable!("the stub style is never built")
    }
    fn make_sort_indicator(&self, _: SortDirection, _: &mut BuildContext) -> WidgetId {
        unreachable!("the stub style is never built")
    }
    fn make_row_background(&self, _: &TableRowConfig, _: &mut BuildContext) -> WidgetId {
        unreachable!("the stub style is never built")
    }
    fn grid(&self) -> TableGridRecipe {
        unreachable!("the stub style is never built")
    }
}

impl TextSelectionStyle for Stub {
    fn handle(&self, _: &Theme) -> TextSelectionHandleRecipe {
        unreachable!("the stub style is never built")
    }
    fn magnifier(&self, _: &Theme) -> TextMagnifierRecipe {
        unreachable!("the stub style is never built")
    }
}

/// Every slot filled, each with its own allocation. A struct literal with no
/// `..Default::default()`, so a slot added to the struct and not here fails to
/// compile, independently of the roster the code under test expands.
fn fully_populated() -> ComponentStyleSlots {
    ComponentStyleSlots {
        button: Some(Rc::new(Stub)),
        split_button: Some(Rc::new(Stub)),
        splitter: Some(Rc::new(Stub)),
        icon_button: Some(Rc::new(Stub)),
        toggle: Some(Rc::new(Stub)),
        checkbox: Some(Rc::new(Stub)),
        radio: Some(Rc::new(Stub)),
        radio_tile: Some(Rc::new(Stub)),
        slider: Some(Rc::new(Stub)),
        text_input: Some(Rc::new(Stub)),
        combo_box: Some(Rc::new(Stub)),
        menu_item: Some(Rc::new(Stub)),
        panel: Some(Rc::new(Stub)),
        card: Some(Rc::new(Stub)),
        chart: Some(Rc::new(Stub)),
        popover: Some(Rc::new(Stub)),
        tooltip: Some(Rc::new(Stub)),
        scroll_bar: Some(Rc::new(Stub)),
        standard_item: Some(Rc::new(Stub)),
        tab: Some(Rc::new(Stub)),
        dialog: Some(Rc::new(Stub)),
        snackbar: Some(Rc::new(Stub)),
        toast: Some(Rc::new(Stub)),
        banner: Some(Rc::new(Stub)),
        badge: Some(Rc::new(Stub)),
        progress_bar: Some(Rc::new(Stub)),
        link: Some(Rc::new(Stub)),
        segmented_control: Some(Rc::new(Stub)),
        avatar: Some(Rc::new(Stub)),
        calendar: Some(Rc::new(Stub)),
        color_picker: Some(Rc::new(Stub)),
        spin_box: Some(Rc::new(Stub)),
        date_edit: Some(Rc::new(Stub)),
        search_field: Some(Rc::new(Stub)),
        rich_text_editor: Some(Rc::new(Stub)),
        table: Some(Rc::new(Stub)),
        list_container: Some(Rc::new(Stub)),
        drop_zone: Some(Rc::new(Stub)),
        drop_target: Some(Rc::new(Stub)),
        grid_view: Some(Rc::new(Stub)),
        web_view: Some(Rc::new(Stub)),
        text_selection: Some(Rc::new(Stub)),
    }
}

#[test]
fn names_has_no_duplicates() {
    let unique: BTreeSet<&str> = ComponentStyleSlots::NAMES.iter().copied().collect();
    assert_eq!(unique.len(), ComponentStyleSlots::NAMES.len());
}

#[test]
fn names_is_what_a_fully_populated_value_reports_installed() {
    assert_eq!(fully_populated().installed(), ComponentStyleSlots::NAMES);
    assert!(ComponentStyleSlots::default().installed().is_empty());
}

#[test]
fn debug_shows_every_slot_and_whether_it_is_filled() {
    let full = format!("{:?}", fully_populated());
    let empty = format!("{:?}", ComponentStyleSlots::default());
    for name in ComponentStyleSlots::NAMES {
        assert!(full.contains(&format!("{name}: true")), "{name} in {full}");
        assert!(
            empty.contains(&format!("{name}: false")),
            "{name} in {empty}"
        );
    }
    assert!(full.starts_with("ComponentStyleSlots {"));
}

#[test]
fn equality_is_identity_of_every_slot() {
    let full = fully_populated();
    assert_eq!(full, full.clone(), "a clone shares every allocation");
    assert_eq!(
        ComponentStyleSlots::default(),
        ComponentStyleSlots::default()
    );
    assert_ne!(full, ComponentStyleSlots::default());
    // Equal-looking but separately allocated styles are different styles.
    assert_ne!(full, fully_populated());

    // Each slot, on its own, takes part: one slot replaced by another
    // allocation, or emptied, while every other slot stays shared, differs.
    let mut flipped = Vec::new();
    for (name, flip) in one_slot_flips() {
        let mut replaced = full.clone();
        flip(&mut replaced, Some(Rc::new(Stub)));
        assert_ne!(full, replaced, "{name} replaced");
        let mut emptied = full.clone();
        flip(&mut emptied, None);
        assert_ne!(full, emptied, "{name} emptied");
        flipped.push(name);
    }
    assert_eq!(flipped, ComponentStyleSlots::NAMES);
}

/// A slot's name, and a function that puts `Some(Stub)` or nothing in that
/// one slot. Fields cannot be addressed by a string, so the roster writes one
/// function per field; `flipped == NAMES` above holds the list to every slot.
type Flip = fn(&mut ComponentStyleSlots, Option<Rc<Stub>>);

fn one_slot_flips() -> Vec<(&'static str, Flip)> {
    macro_rules! probe {
        ($($slot:ident),* $(,)?) => {
            vec![$((stringify!($slot), {
                fn flip(slots: &mut ComponentStyleSlots, style: Option<Rc<Stub>>) {
                    slots.$slot = style.map(|style| style as _);
                }
                flip as Flip
            })),*]
        };
    }
    for_every_slot!(probe)
}
