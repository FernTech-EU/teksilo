//! Milestone 3: Widget Catalog — side-by-side builder vs `fern!`
//!
//! A 1600×900 window split by a vertical [`SplitView`] into two panes.
//! Each pane renders the *exact same* widget tree — a toolbar, a
//! scrollable 13-section catalog of every Milestone 3 widget, and a
//! status bar. The left pane is assembled with builder-style
//! `.child()/.add_child()` chains; the right pane is assembled with the
//! `fern!` DSL. Both panes are driven by a single shared `Signals`
//! bundle, so toggling a checkbox, dragging a slider, or changing a
//! tab on one side mirrors on the other — visual proof that the two
//! authorings lower to identical trees.
//!
//! Run with: `cargo run -p widget-catalog`
//!
//! Demonstrates:
//! - Divider (horizontal, vertical, custom thickness/color)
//! - IconWidget (checkmark, chevrons, custom path)
//! - Grid (fixed, fractional, auto tracks with gaps)
//! - Wrap (flow layout with line breaking)
//! - AspectRatio (16:9, square)
//! - ProgressBar (determinate, indeterminate, vertical)
//! - Badge (custom colors)
//! - Checkbox (two-state, tristate, disabled, with tooltip)
//! - RadioButton (mutual exclusion group)
//! - Toggle (with/without label, disabled)
//! - Slider (horizontal, vertical, stepped)
//! - SegmentedControl (position-based click selection)
//! - Card (header/content/footer, shadow)
//! - Toolbar (compact action bar)
//! - StatusBar (bottom info bar)
//! - Accordion (animated expand/collapse)
//! - ToolBox (vertical exclusive disclosure, Int UI)
//! - Link (clickable text with underline)
//! - ScrollArea (wrapping all content)
//! - SplitView (twin-pane demo)
//! - Theme switching (light/dark)

use fern_ui::IntentKind;
use fern_ui::prelude::*;
use fern_ui::tokens::{FontWeight, Orientation, TextStyle, VAlignment};
use fern_ui::widgets::tooltip::TooltipContent;
use fern_ui::widgets::{
    Accordion, Avatar, AvatarPresence, AvatarShape, AvatarSize, Badge, Button, ButtonVariant, Card,
    CheckState, Checkbox, ComboBox, Divider, EventContextMessageBoxExt, Expand, FixedSize, Grid,
    GroupBox, GroupHeader, HStack, IconButton, IconButtonSize, IconLocation, IconWidget, ImageFit,
    ImageWidget, Link, MaxSize, MenuItem, MenuList, MessageBox, MessageBoxButtons, Padding, Panel,
    ProgressBar, RadioButton, ScrollArea, SegmentedControl, Slider, Spacer, SplitButton, SplitView,
    StandardButton, StatusBar, TabId, TabInfo, TabWidget, TextInput, TextWidget, Toggle, ToolBox,
    ToolBoxItem, Toolbar, TrackSize, VStack, Wrap,
};

/// `fern!`-DSL-friendly helper: a free fn returns a `TabInfo` that
/// the DSL accepts as a property value. (Method-chain expressions
/// like `TabInfo::new().title(...)` need `#{...}` escaping in the
/// DSL, which forces an `_id`-suffixed method lookup — wrong shape
/// for `TabWidget::static_tab`.)
fn tab_info(name: &'static str) -> TabInfo {
    TabInfo::new().title(fern_ui::i18n::LocalizedString::literal(name))
}

fn tab_info_disabled(name: &'static str) -> TabInfo {
    tab_info(name).enabled(false)
}

// ---------------------------------------------------------------------------
// Application intents
// ---------------------------------------------------------------------------

#[derive(Debug, IntentKind)]
enum CatalogIntent {
    #[name = "catalog.toggle_dark_mode"]
    ToggleDarkMode,
}

// ---------------------------------------------------------------------------
// Shared signal bundle
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Signals {
    checkbox_checked: Signal<bool>,
    tristate: Signal<CheckState>,
    radio_selected: Signal<usize>,
    toggle_on: Signal<bool>,
    toggle_label_on: Signal<bool>,
    slider_value: Signal<f32>,
    slider_v_value: Signal<f32>,
    slider_stepped: Signal<f32>,
    segment_selected: Signal<usize>,
    accordion_expanded: Signal<bool>,
    accordion2_expanded: Signal<bool>,
    group_box_notifications_on: Signal<bool>,
    cb_disabled_state: Signal<bool>,
    cb_sounds: Signal<bool>,
    toggle_disabled_state: Signal<bool>,
    slider_disabled_state: Signal<f32>,
    tabs_selected: Signal<Option<TabId>>,
    tool_box_selected: Signal<usize>,
    combo_selected: Signal<Option<String>>,
    visibility_signal: Signal<bool>,
    pinned_signal: Signal<bool>,
    search_text: Signal<String>,
    username_text: Signal<String>,
    readonly_text: Signal<String>,
}

impl Signals {
    fn new(ctx: &mut BuildContext) -> Self {
        Self {
            checkbox_checked: ctx.signal(false),
            tristate: ctx.signal(CheckState::Unchecked),
            radio_selected: ctx.signal(0_usize),
            toggle_on: ctx.signal(false),
            toggle_label_on: ctx.signal(true),
            slider_value: ctx.signal(50.0_f32),
            slider_v_value: ctx.signal(0.3_f32),
            slider_stepped: ctx.signal(25.0_f32),
            segment_selected: ctx.signal(0_usize),
            accordion_expanded: ctx.signal(false),
            accordion2_expanded: ctx.signal(true),
            group_box_notifications_on: ctx.signal(true),
            cb_disabled_state: ctx.signal(true),
            cb_sounds: ctx.signal(true),
            toggle_disabled_state: ctx.signal(false),
            slider_disabled_state: ctx.signal(30.0_f32),
            tabs_selected: ctx.signal(None),
            tool_box_selected: ctx.signal(0_usize),
            combo_selected: ctx.signal(None::<String>),
            visibility_signal: ctx.signal(false),
            pinned_signal: ctx.signal(false),
            search_text: ctx.signal(String::new()),
            username_text: ctx.signal("cyril".to_string()),
            readonly_text: ctx.signal("Read-only value".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

fn surface_swatch(
    _theme: &Theme,
    bg: impl Into<ColorProp>,
    name: &str,
    text_role: &str,
    text_color: impl Into<ColorProp>,
) -> VStack {
    VStack::new()
        .spacing(4.0)
        .child(
            Panel::new()
                .background(bg)
                .border_color(BorderRole::Strong)
                .border_width(1.0)
                .corner_radius(4.0)
                .padding(10.0)
                .child(
                    MaxSize::new(f32::INFINITY, 44.0).child(
                        TextWidget::new_literal(name)
                            .style(TextStyleRole::Small)
                            .color(text_color),
                    ),
                ),
        )
        .child(
            TextWidget::new_literal(text_role)
                .style(TextStyleRole::Tiny)
                .color(TextRole::Secondary),
        )
}

fn text_sample(
    _theme: &Theme,
    name: &str,
    color: impl Into<ColorProp>,
    description: &str,
) -> HStack {
    HStack::new()
        .spacing(12.0)
        .child(
            TextWidget::new_literal("The quick brown 🦊 jumps over the lazy 🐶 🎉")
                .style(TextStyleRole::Body)
                .color(color),
        )
        .child(Spacer::new())
        .child(
            TextWidget::new_literal(name)
                .style(TextStyleRole::Tiny)
                .color(TextRole::Secondary),
        )
        .child(
            TextWidget::new_literal(description)
                .style(TextStyleRole::Tiny)
                .color(TextRole::Secondary),
        )
}

fn editor_line(_theme: &Theme, line_no: &str, code: &str) -> HStack {
    HStack::new()
        .spacing(12.0)
        .child(
            FixedSize::new().bind_width(24.0_f32).child(
                TextWidget::new_literal(line_no)
                    .style(TextStyleRole::Mono)
                    .color(TextRole::EditorGutterFg),
            ),
        )
        .child(
            TextWidget::new_literal(code)
                .style(TextStyleRole::Mono)
                .color(TextRole::EditorFg),
        )
}

fn editor_swatch(
    _theme: &Theme,
    bg: impl Into<ColorProp>,
    name: &str,
    sample_color: impl Into<ColorProp>,
) -> VStack {
    VStack::new()
        .spacing(4.0)
        .child(
            Panel::new()
                .background(bg)
                .border_color(BorderRole::Strong)
                .border_width(1.0)
                .corner_radius(4.0)
                .padding(10.0)
                .child(
                    MaxSize::new(f32::INFINITY, 44.0).child(
                        TextWidget::new_literal("Aa Bb 123")
                            .style(TextStyleRole::Mono)
                            .color(sample_color),
                    ),
                ),
        )
        .child(
            TextWidget::new_literal(name)
                .style(TextStyleRole::Tiny)
                .color(TextRole::Secondary),
        )
}

fn build_color_cell(color: impl Into<ColorProp>, label: &str) -> Panel {
    Panel::new()
        .background(color)
        .corner_radius(4.0)
        .padding(8.0)
        .child(
            TextWidget::new_literal(label)
                .style(TextStyle {
                    family: "sans-serif".into(),
                    size: 12.0,
                    weight: FontWeight::BOLD,
                    line_height: 1.4,
                    letter_spacing: 0.0,
                })
                .color(Color::WHITE),
        )
}

// ---------------------------------------------------------------------------
// Root composite
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct WidgetCatalog {
    root_child_id: Option<WidgetId>,
}

impl WidgetCatalog {
    fn new() -> Self {
        Self {
            root_child_id: None,
        }
    }
}

impl Widget for WidgetCatalog {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme_signal().get();
        let sigs = Signals::new(ctx);

        let is_dark = ctx.signal(false);
        let is_dark_for_action = is_dark.clone();
        ctx.register_action(Action::new("catalog.toggle_dark_mode").on_invoke(
            move |_intent, ctx| {
                let dark = !is_dark_for_action.get();
                is_dark_for_action.set(dark);
                ctx.set_theme(if dark {
                    Theme::dark_default()
                } else {
                    Theme::light_default()
                });
            },
        ));

        // ============================ LEFT PANE (builder) ============================
        let palette_section = self.palette_builder(ctx, &theme, &sigs);
        let primitives_section = self.primitives_builder(ctx, &theme, &sigs);
        let layout_section = self.layout_builder(ctx, &theme, &sigs);
        let controls_section = self.controls_builder(ctx, &theme, &sigs);
        let display_section = self.display_builder(ctx, &theme, &sigs);
        let text_overflow_section = self.text_overflow_builder(ctx, &theme, &sigs);
        let containers_section = self.containers_builder(ctx, &theme, &sigs);
        let nav_section = self.nav_builder(ctx, &theme, &sigs);
        let rich_tooltips_section = self.rich_tooltips_builder(ctx, &theme, &sigs);
        let message_box_section = self.message_box_builder(ctx, &theme, &sigs);
        let menus_section = self.menus_builder(ctx, &theme, &sigs);
        let image_section = self.image_builder(ctx, &theme, &sigs);
        let builtin_section = self.builtin_builder(ctx, &theme, &sigs);
        let text_input_section = self.text_input_builder(ctx, &theme, &sigs);

        let left_toolbar = ctx.add(
            Toolbar::new().child(
                HStack::new()
                    .child(
                        TextWidget::new_literal("Widget Catalog -- builder")
                            .style(TextStyleRole::BodyBold)
                            .color(TextRole::Primary),
                    )
                    .child(Spacer::new())
                    .child(
                        Button::new_literal("Toggle Dark Mode")
                            .style(ButtonVariant::Regular)
                            .on_activate_fn(|ctx| {
                                ctx.send_intent(CatalogIntent::ToggleDarkMode);
                            }),
                    ),
            ),
        );

        let left_content_col = ctx.add(
            VStack::new()
                .spacing(24.0)
                .add_child(palette_section)
                .child(Divider::new())
                .add_child(primitives_section)
                .child(Divider::new())
                .add_child(layout_section)
                .child(Divider::new())
                .add_child(controls_section)
                .child(Divider::new())
                .add_child(display_section)
                .child(Divider::new())
                .add_child(text_overflow_section)
                .child(Divider::new())
                .add_child(containers_section)
                .child(Divider::new())
                .add_child(nav_section)
                .child(Divider::new())
                .add_child(rich_tooltips_section)
                .child(Divider::new())
                .add_child(message_box_section)
                .child(Divider::new())
                .add_child(menus_section)
                .child(Divider::new())
                .add_child(image_section)
                .child(Divider::new())
                .add_child(builtin_section)
                .child(Divider::new())
                .add_child(text_input_section),
        );
        let left_padded = ctx.add(Padding::uniform(24.0).child_id(left_content_col));
        let left_scroll = ctx.add(ScrollArea::from_id(left_padded));
        let left_expand = ctx.add(Expand::new().child_id(left_scroll));
        let left_root = ctx.add(
            VStack::new()
                .add_child(left_toolbar)
                .add_child(left_expand)
                .child(
                    StatusBar::new().child(
                        TextWidget::new_literal("Builder -- .child() / .add_child() chains")
                            .style(TextStyleRole::Tiny)
                            .color(TextRole::Secondary),
                    ),
                ),
        );

        // ============================ RIGHT PANE (fern!) =============================
        let r_palette = self.palette_fern(ctx, &theme, &sigs);
        let r_primitives = self.primitives_fern(ctx, &theme, &sigs);
        let r_layout = self.layout_fern(ctx, &theme, &sigs);
        let r_controls = self.controls_fern(ctx, &theme, &sigs);
        let r_display = self.display_fern(ctx, &theme, &sigs);
        let r_text_overflow = self.text_overflow_fern(ctx, &theme, &sigs);
        let r_containers = self.containers_fern(ctx, &theme, &sigs);
        let r_nav = self.nav_fern(ctx, &theme, &sigs);
        let r_rich_tooltips = self.rich_tooltips_fern(ctx, &theme, &sigs);
        let r_message_box = self.message_box_fern(ctx, &theme, &sigs);
        let r_menus = self.menus_fern(ctx, &theme, &sigs);
        let r_image = self.image_fern(ctx, &theme, &sigs);
        let r_builtin = self.builtin_fern(ctx, &theme, &sigs);
        let r_text_input = self.text_input_fern(ctx, &theme, &sigs);

        let right_content_col = fern!(ctx => VStack {
                spacing: 24.0
                add_child: r_palette
                Divider
                add_child: r_primitives
                Divider
                add_child: r_layout
                Divider
                add_child: r_controls
                Divider
                add_child: r_display
                Divider
                add_child: r_text_overflow
                Divider
                add_child: r_containers
                Divider
                add_child: r_nav
                Divider
                add_child: r_rich_tooltips
                Divider
                add_child: r_message_box
                Divider
                add_child: r_menus
                Divider
                add_child: r_image
                Divider
                add_child: r_builtin
                Divider
                add_child: r_text_input
            }
        );
        let right_padded = ctx.add(Padding::uniform(24.0).child_id(right_content_col));
        let right_scroll = ctx.add(ScrollArea::from_id(right_padded));
        let right_root = fern!(ctx => VStack {
                Toolbar {
                    HStack {
                        TextWidget::new_literal("Widget Catalog 🦊 fern!") {
                            style: TextStyleRole::BodyBold
                            color: TextRole::Primary
                        }
                        Spacer
                        Button::new_literal("Toggle Dark Mode") {
                            style: ButtonVariant::Regular
                            on_activate_fn: |ctx| {
                                ctx.send_intent(CatalogIntent::ToggleDarkMode);
                            }
                        }
                    }
                }
                Expand {
                    child_id: right_scroll
                }
                StatusBar {
                    TextWidget::new_literal("fern! -- DSL body items") {
                        style: TextStyleRole::Tiny
                        color: TextRole::Secondary
                    }
                }
            }
        );

        // ============================ SPLIT ROOT =====================================
        let split = ctx.signal(0.5_f32);
        let root = ctx.add(
            SplitView::new(split)
                .orientation(Orientation::Horizontal)
                .min_first_size(400.0)
                .min_second_size(400.0)
                .first_id(left_root)
                .second_id(right_root),
        );
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        match self.root_child_id {
            Some(id) => ctx
                .child_size(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0)),
            None => proposal.resolve(0.0, 0.0),
        }
        .into()
    }
}

// ---------------------------------------------------------------------------
// Builder-style section helpers
// ---------------------------------------------------------------------------

impl WidgetCatalog {
    fn palette_builder(&self, ctx: &mut BuildContext, theme: &Theme, _sigs: &Signals) -> WidgetId {
        let surfaces_grid = ctx.add(
            Grid::new()
                .columns(vec![
                    TrackSize::Fractional(1.0),
                    TrackSize::Fractional(1.0),
                    TrackSize::Fractional(1.0),
                    TrackSize::Fractional(1.0),
                ])
                .column_gap(12.0)
                .row_gap(12.0)
                .rows(vec![TrackSize::Auto, TrackSize::Auto])
                .child(surface_swatch(
                    theme,
                    SurfaceRole::Main,
                    "surface_main",
                    "text_primary",
                    TextRole::Primary,
                ))
                .child(surface_swatch(
                    theme,
                    SurfaceRole::Content,
                    "surface_content",
                    "text_primary",
                    TextRole::Primary,
                ))
                .child(surface_swatch(
                    theme,
                    SurfaceRole::Raised,
                    "surface_raised",
                    "text_primary",
                    TextRole::Primary,
                ))
                .child(surface_swatch(
                    theme,
                    SurfaceRole::Sunken,
                    "surface_sunken",
                    "text_secondary",
                    TextRole::Secondary,
                ))
                .child(surface_swatch(
                    theme,
                    SurfaceRole::Hover,
                    "surface_hover",
                    "text_primary",
                    TextRole::Primary,
                ))
                .child(surface_swatch(
                    theme,
                    SurfaceRole::Pressed,
                    "surface_pressed",
                    "text_primary",
                    TextRole::Primary,
                ))
                .child(surface_swatch(
                    theme,
                    SurfaceRole::Selected,
                    "surface_selected",
                    "selection_text_active",
                    Color::WHITE,
                ))
                .child(surface_swatch(
                    theme,
                    SurfaceRole::SelectedInactive,
                    "surface_selected_inactive",
                    "selection_text_inactive",
                    TextRole::Primary,
                )),
        );

        let text_samples = ctx.add(
            Panel::new()
                .background(SurfaceRole::Main)
                .border_color(BorderRole::Default)
                .border_width(1.0)
                .corner_radius(8.0)
                .padding(16.0)
                .child(
                    VStack::new()
                        .spacing(6.0)
                        .child(text_sample(
                            theme,
                            "text_primary",
                            TextRole::Primary,
                            "body, main labels",
                        ))
                        .child(text_sample(
                            theme,
                            "text_secondary",
                            TextRole::Secondary,
                            "hints, captions, placeholders",
                        ))
                        .child(text_sample(
                            theme,
                            "text_disabled",
                            TextRole::Disabled,
                            "disabled labels",
                        ))
                        .child(text_sample(
                            theme,
                            "text_link",
                            TextRole::Link,
                            "hyperlinks",
                        ))
                        .child(text_sample(
                            theme,
                            "text_error",
                            TextRole::Error,
                            "validation errors",
                        ))
                        .child(text_sample(
                            theme,
                            "text_warning",
                            TextRole::Warning,
                            "validation warnings",
                        ))
                        .child(text_sample(
                            theme,
                            "text_success",
                            TextRole::Success,
                            "success messages",
                        )),
                ),
        );

        let text_on_accent_row = ctx.add(
            Panel::new()
                .background(SurfaceRole::Accent)
                .corner_radius(4.0)
                .padding(12.0)
                .child(
                    HStack::new()
                        .spacing(12.0)
                        .child(
                            TextWidget::new_literal("Default button label")
                                .style(TextStyleRole::Body)
                                .color(TextRole::OnAccent),
                        )
                        .child(Spacer::new())
                        .child(
                            TextWidget::new_literal("text_on_accent on accent")
                                .style(TextStyleRole::Tiny)
                                .color(TextRole::OnAccent),
                        ),
                ),
        );

        let current_line_content = ctx.add(
            HStack::new()
                .spacing(12.0)
                .child(
                    FixedSize::new().bind_width(24.0_f32).child(
                        TextWidget::new_literal("2")
                            .style(TextStyleRole::Mono)
                            .color(TextRole::EditorGutterFg),
                    ),
                )
                .child(
                    TextWidget::new_literal("    let ")
                        .style(TextStyleRole::Mono)
                        .color(TextRole::EditorFg),
                )
                .child(
                    Panel::new()
                        .background(SurfaceRole::EditorSelectionBg)
                        .corner_radius(2.0)
                        .padding(0.0)
                        .border_width(0.0)
                        .child(
                            Padding::symmetric(1.0, 2.0).child(
                                TextWidget::new_literal("x")
                                    .style(TextStyleRole::Mono)
                                    .color(TextRole::EditorFg),
                            ),
                        ),
                )
                .child(
                    TextWidget::new_literal(" = 42;")
                        .style(TextStyleRole::Mono)
                        .color(TextRole::EditorFg),
                )
                .child(
                    FixedSize::new()
                        .bind_width(1.5_f32)
                        .bind_height(16.0_f32)
                        .child(
                            Panel::new()
                                .background(SurfaceRole::EditorCaret)
                                .corner_radius(0.0)
                                .border_width(0.0)
                                .padding(0.0)
                                .child(Spacer::new()),
                        ),
                ),
        );
        let current_line_bg = ctx.add(
            Panel::new()
                .background(SurfaceRole::EditorCurrentLineBg)
                .corner_radius(0.0)
                .border_width(0.0)
                .padding(4.0)
                .child_id(current_line_content),
        );

        let mock_editor = ctx.add(
            Panel::new()
                .background(SurfaceRole::EditorBg)
                .border_color(BorderRole::Strong)
                .border_width(1.0)
                .corner_radius(6.0)
                .padding(8.0)
                .child(
                    VStack::new()
                        .spacing(2.0)
                        .child(editor_line(theme, "1", "fn main() {"))
                        .add_child(current_line_bg)
                        .child(editor_line(theme, "3", "    println!(\"{}\", x);"))
                        .child(editor_line(theme, "4", "}")),
                ),
        );

        let editor_swatches = ctx.add(
            Grid::new()
                .columns(vec![
                    TrackSize::Fractional(1.0),
                    TrackSize::Fractional(1.0),
                    TrackSize::Fractional(1.0),
                    TrackSize::Fractional(1.0),
                ])
                .column_gap(12.0)
                .row_gap(12.0)
                .rows(vec![TrackSize::Auto, TrackSize::Auto])
                .child(editor_swatch(
                    theme,
                    SurfaceRole::EditorBg,
                    "editor_bg",
                    TextRole::EditorFg,
                ))
                .child(editor_swatch(
                    theme,
                    TextRole::EditorFg,
                    "editor_fg",
                    SurfaceRole::EditorBg,
                ))
                .child(editor_swatch(
                    theme,
                    SurfaceRole::EditorCaret,
                    "editor_caret",
                    SurfaceRole::EditorBg,
                ))
                .child(editor_swatch(
                    theme,
                    SurfaceRole::EditorCurrentLineBg,
                    "editor_current_line_bg",
                    TextRole::EditorFg,
                ))
                .child(editor_swatch(
                    theme,
                    TextRole::EditorGutterFg,
                    "editor_gutter_fg",
                    SurfaceRole::EditorBg,
                ))
                .child(editor_swatch(
                    theme,
                    SurfaceRole::EditorSelectionBg,
                    "editor_selection_bg",
                    TextRole::EditorFg,
                )),
        );

        ctx.add(
            VStack::new()
                .spacing(12.0)
                .child(
                    TextWidget::new_literal("Theme Palette")
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new_literal("Surfaces")
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary),
                )
                .add_child(surfaces_grid)
                .child(
                    TextWidget::new_literal("Text")
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary),
                )
                .add_child(text_samples)
                .add_child(text_on_accent_row)
                .child(
                    TextWidget::new_literal("Editor")
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary),
                )
                .add_child(mock_editor)
                .add_child(editor_swatches),
        )
    }

    fn primitives_builder(
        &self,
        ctx: &mut BuildContext,
        _theme: &Theme,
        _sigs: &Signals,
    ) -> WidgetId {
        let div_row = ctx.add(
            HStack::new()
                .spacing(16.0)
                .child(
                    VStack::new()
                        .spacing(4.0)
                        .child(
                            TextWidget::new_literal("H")
                                .style(TextStyleRole::Tiny)
                                .color(TextRole::Secondary),
                        )
                        .child(Divider::new()),
                )
                .child(
                    HStack::new()
                        .spacing(4.0)
                        .child(
                            TextWidget::new_literal("V")
                                .style(TextStyleRole::Tiny)
                                .color(TextRole::Secondary),
                        )
                        .child(Divider::vertical().thickness(2.0).color(TextRole::Accent)),
                )
                .child(
                    VStack::new()
                        .spacing(4.0)
                        .child(
                            TextWidget::new_literal("Thick")
                                .style(TextStyleRole::Tiny)
                                .color(TextRole::Secondary),
                        )
                        .child(Divider::new().thickness(4.0).color(TextRole::Error)),
                ),
        );

        let icon_row = ctx.add(
            HStack::new()
                .spacing(12.0)
                .child(IconWidget::checkmark(24.0).color(TextRole::Accent))
                .child(IconWidget::chevron_down(24.0).color(SurfaceRole::AccentSubtle))
                .child(IconWidget::chevron_right(24.0).color(TextRole::Error)),
        );

        ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new_literal("Primitives")
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new_literal("Divider (horizontal, vertical, thick, colored)")
                        .style(TextStyleRole::Tiny)
                        .color(TextRole::Secondary),
                )
                .add_child(div_row)
                .child(
                    TextWidget::new_literal("IconWidget (checkmark, chevrons)")
                        .style(TextStyleRole::Tiny)
                        .color(TextRole::Secondary),
                )
                .add_child(icon_row),
        )
    }

    fn layout_builder(&self, ctx: &mut BuildContext, _theme: &Theme, _sigs: &Signals) -> WidgetId {
        ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new_literal("Layout Primitives")
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new_literal("Grid (Fixed 80px | 1fr | 2fr, with 8px gap)")
                        .style(TextStyleRole::Tiny)
                        .color(TextRole::Secondary),
                )
                .child(
                    Grid::new()
                        .columns(vec![
                            TrackSize::Fixed(80.0),
                            TrackSize::Fractional(1.0),
                            TrackSize::Fractional(2.0),
                        ])
                        .rows(vec![TrackSize::Auto, TrackSize::Auto])
                        .column_gap(8.0)
                        .row_gap(8.0)
                        .child(build_color_cell(SurfaceRole::Accent, "A1"))
                        .child(build_color_cell(SurfaceRole::AccentSubtle, "B1"))
                        .child(build_color_cell(TextRole::Error, "C1"))
                        .child(build_color_cell(TextRole::Accent, "A2"))
                        .child(build_color_cell(TextRole::Success, "B2"))
                        .child(build_color_cell(TextRole::Warning, "C2")),
                )
                .child(
                    TextWidget::new_literal("Wrap (flow layout, 8px spacing)")
                        .style(TextStyleRole::Tiny)
                        .color(TextRole::Secondary),
                )
                .child(
                    Wrap::new()
                        .spacing(8.0)
                        .line_spacing(8.0)
                        .child(Badge::new_literal("Rust"))
                        .child(Badge::new_literal("GUI"))
                        .child(Badge::new_literal("Accessible"))
                        .child(Badge::new_literal("Reactive"))
                        .child(Badge::new_literal("Fast"))
                        .child(Badge::new_literal("Cross-platform"))
                        .child(Badge::new_literal("Retained"))
                        .child(Badge::new_literal("wgpu")),
                ),
        )
    }

    fn controls_builder(&self, ctx: &mut BuildContext, _theme: &Theme, sigs: &Signals) -> WidgetId {
        let buttons_row = ctx.add(
            HStack::new()
                .spacing(8.0)
                .child(
                    Button::new_literal("Save")
                        .style(ButtonVariant::Default)
                        .on_activate_fn(|_| println!("Save")),
                )
                .child(
                    Button::new_literal("Cancel")
                        .style(ButtonVariant::Regular)
                        .on_activate_fn(|_| println!("Cancel")),
                )
                .child(
                    Button::new_literal("Learn more")
                        .style(ButtonVariant::Flat)
                        .on_activate_fn(|_| println!("LearnMore")),
                ),
        );
        let buttons_row_disabled = ctx.add(
            HStack::new()
                .spacing(8.0)
                .child(
                    Button::new_literal("Save")
                        .style(ButtonVariant::Default)
                        .enabled(false),
                )
                .child(
                    Button::new_literal("Cancel")
                        .style(ButtonVariant::Regular)
                        .enabled(false),
                )
                .child(
                    Button::new_literal("Learn more")
                        .style(ButtonVariant::Flat)
                        .enabled(false),
                ),
        );

        let save_icon = fern_ui::res!("resources/icons/save.svg");
        let home_icon = fern_ui::res!("resources/icons/home.svg");
        let star_icon = fern_ui::res!("resources/icons/star.png");
        let clock_icon = fern_ui::res!("resources/icons/clock.webp");
        let icon_buttons_row = ctx.add(
            HStack::new()
                .spacing(8.0)
                .child(
                    Button::new_literal("Save")
                        .icon(IconWidget::from_svg_icon(save_icon), IconLocation::Leading)
                        .style(ButtonVariant::Default)
                        .on_activate_fn(|_| println!("Save")),
                )
                .child(
                    Button::new_literal("Home")
                        .icon(IconWidget::from_svg_icon(home_icon), IconLocation::Leading)
                        .style(ButtonVariant::Regular)
                        .on_activate_fn(|_| println!("Cancel")),
                )
                .child(
                    Button::new_literal("Star")
                        .icon(
                            IconWidget::from_raster(star_icon, 24.0),
                            IconLocation::Leading,
                        )
                        .style(ButtonVariant::Regular)
                        .on_activate_fn(|_| println!("Save")),
                )
                .child(
                    Button::new_literal("Clock")
                        .icon(
                            IconWidget::from_raster(clock_icon, 24.0),
                            IconLocation::Leading,
                        )
                        .style(ButtonVariant::Regular)
                        .on_activate_fn(|_| println!("Cancel")),
                )
                // Icon-only buttons need explicit accessibility metadata —
                // their drawn glyph is meaningless to screen readers. Demo
                // of the builder-level overrides:
                //   .access_label_literal       — what AT software announces
                //   .access_shortcut_literal    — pre-formatted chord, when
                //                                 the binding is NOT routed
                //                                 through the Shortcut system
                //   .access_shortcut_id         — bind to a registered
                //                                 Shortcut; the announcement
                //                                 tracks user rebinds
                //   .access_has_popup           — flags the chevron as a disclosure
                .child(
                    Button::new_literal("Save")
                        .icon(IconWidget::from_svg_icon(save_icon), IconLocation::IconOnly)
                        .style(ButtonVariant::Flat)
                        .on_activate_fn(|_| println!("Save"))
                        .access_label_literal("Save")
                        // The literal variant — fine for demos with no
                        // registered shortcut. In production, prefer
                        // `.access_shortcut_id("app.save")` paired with
                        // a `Shortcut::new("app.save").primary(...)`
                        // registration so a user rebind via the
                        // settings UI also retitles the AT announcement.
                        .access_shortcut_literal("Ctrl+S"),
                )
                .child(
                    Button::new_literal("")
                        .icon(IconWidget::chevron_down(16.0), IconLocation::IconOnly)
                        .style(ButtonVariant::Regular)
                        .on_activate_fn(|_| println!("Cancel"))
                        .access_label_literal("More options")
                        .access_has_popup(fern_ui::core::accesskit::HasPopup::Menu),
                ),
        );

        let split_buttons_row = ctx.add(
            HStack::new()
                .spacing(8.0)
                .child(
                    SplitButton::new()
                        .item(
                            MenuItem::new_literal("Run")
                                .on_activate_fn(|_| println!("Run"))
                                .tooltip_literal("Run the current configuration"),
                        )
                        .item(
                            MenuItem::new_literal("Run Tests")
                                .on_activate_fn(|_| println!("RunTests"))
                                .tooltip_literal("Run the test suite"),
                        )
                        .item(
                            MenuItem::new_literal("Run with Coverage")
                                .on_activate_fn(|_| println!("RunCoverage"))
                                .tooltip_literal("Run and collect code coverage"),
                        )
                        .separator()
                        .item(
                            MenuItem::new_literal("Debug")
                                .on_activate_fn(|_| println!("Debug"))
                                .tooltip_literal("Launch the debugger"),
                        )
                        .tooltip_literal("Run the selected configuration")
                        .chevron_tooltip_literal("Other run configurations")
                        .style(ButtonVariant::Default),
                )
                .child(
                    SplitButton::new_static()
                        .item(
                            MenuItem::new_literal("Save")
                                .on_activate_fn(|_| println!("Save"))
                                .tooltip_literal("Save the current file"),
                        )
                        .item(
                            MenuItem::new_literal("Save As…")
                                .on_activate_fn(|_| println!("Save"))
                                .tooltip_literal("Save the current file under a new name"),
                        )
                        .tooltip_literal("Save (main action stays pinned)")
                        .style(ButtonVariant::Regular),
                )
                .child(
                    SplitButton::new()
                        .item(MenuItem::new_literal("Run").on_activate_fn(|_| println!("Run")))
                        .item(MenuItem::new_literal("Debug").on_activate_fn(|_| println!("Debug")))
                        .enabled(false),
                ),
        );

        let buttons_group = ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(GroupHeader::new_literal("Standard buttons"))
                .add_child(buttons_row)
                .add_child(buttons_row_disabled)
                .child(GroupHeader::new_literal("Icon buttons"))
                .add_child(icon_buttons_row)
                .child(GroupHeader::new_literal("Split buttons"))
                .add_child(split_buttons_row),
        );

        let checkbox_group = ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    Checkbox::new(sigs.checkbox_checked.clone())
                        .label_literal("Accept terms")
                        .tooltip_literal("Click to accept the terms and conditions"),
                )
                .child(
                    Checkbox::new(sigs.cb_sounds.clone())
                        .label_literal("Play notification sounds")
                        .caption_literal(
                            "Play a short chime when a new message arrives. \
                             Muted automatically while you're in a call.",
                        ),
                )
                .child(
                    Checkbox::new(sigs.cb_disabled_state.clone())
                        .label_literal("Always on (disabled)")
                        .enabled(false),
                )
                .child(
                    Checkbox::tristate(sigs.tristate.clone())
                        .label_literal("Select all (tristate)")
                        .caption_literal("Cycles unchecked → checked → indeterminate")
                        .tooltip_literal("Cycles: unchecked, checked, indeterminate"),
                ),
        );

        let radio_group = ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    RadioButton::new(0, sigs.radio_selected.clone())
                        .label_literal("Standard")
                        .caption_literal("Recommended for most users — sensible defaults.")
                        .tooltip_literal("First option"),
                )
                .child(
                    RadioButton::new(1, sigs.radio_selected.clone())
                        .label_literal("Advanced")
                        .caption_literal(
                            "Expose every option, including experimental features \
                             that may change between releases.",
                        ),
                )
                .child(RadioButton::new(2, sigs.radio_selected.clone()).label_literal("Custom")),
        );

        let toggle_group = ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(Toggle::new(sigs.toggle_on.clone()).label_literal("Enabled"))
                .child(Toggle::new(sigs.toggle_label_on.clone()).label_literal("Notifications"))
                .child(
                    Toggle::new(sigs.toggle_disabled_state.clone())
                        .label_literal("Unavailable")
                        .enabled(false),
                ),
        );

        let slider_vert = ctx.add(
            Slider::new(sigs.slider_v_value.clone(), 0.0, 1.0).orientation(Orientation::Vertical),
        );
        let slider_section = ctx.add(
            HStack::new()
                .spacing(16.0)
                .child(
                    VStack::new()
                        .spacing(8.0)
                        .child(
                            TextWidget::new_literal("Horizontal")
                                .style(TextStyleRole::Tiny)
                                .color(TextRole::Secondary),
                        )
                        .child(Slider::new(sigs.slider_value.clone(), 0.0, 100.0))
                        .child(
                            TextWidget::new_literal("Stepped (25)")
                                .style(TextStyleRole::Tiny)
                                .color(TextRole::Secondary),
                        )
                        .child(Slider::new(sigs.slider_stepped.clone(), 0.0, 100.0).step(25.0))
                        .child(
                            TextWidget::new_literal("Disabled")
                                .style(TextStyleRole::Tiny)
                                .color(TextRole::Secondary),
                        )
                        .child(
                            Slider::new(sigs.slider_disabled_state.clone(), 0.0, 100.0)
                                .enabled(false),
                        ),
                )
                .child(
                    VStack::new()
                        .spacing(4.0)
                        .child(
                            TextWidget::new_literal("Vertical")
                                .style(TextStyleRole::Tiny)
                                .color(TextRole::Secondary),
                        )
                        .child(MaxSize::new(f32::MAX, 120.0).child_id(slider_vert)),
                ),
        );

        let form_row = ctx.add(
            Grid::new()
                .columns(vec![
                    TrackSize::Fractional(1.0),
                    TrackSize::Fractional(1.0),
                    TrackSize::Fractional(1.0),
                ])
                .column_gap(16.0)
                .rows(vec![TrackSize::Auto])
                .child(
                    VStack::new()
                        .spacing(4.0)
                        .child(
                            TextWidget::new_literal("Checkbox")
                                .style(TextStyleRole::Small)
                                .color(TextRole::Secondary),
                        )
                        .add_child(checkbox_group),
                )
                .child(
                    VStack::new()
                        .spacing(4.0)
                        .child(
                            TextWidget::new_literal("RadioButton")
                                .style(TextStyleRole::Small)
                                .color(TextRole::Secondary),
                        )
                        .add_child(radio_group),
                )
                .child(
                    VStack::new()
                        .spacing(4.0)
                        .child(
                            TextWidget::new_literal("Toggle")
                                .style(TextStyleRole::Small)
                                .color(TextRole::Secondary),
                        )
                        .add_child(toggle_group),
                ),
        );

        ctx.add(
            VStack::new()
                .spacing(12.0)
                .child(
                    TextWidget::new_literal("Form Controls")
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new_literal("Button")
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary),
                )
                .add_child(buttons_group)
                .child(Divider::new())
                .add_child(form_row)
                .child(Divider::new())
                .child(
                    TextWidget::new_literal("Slider")
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary),
                )
                .add_child(slider_section)
                .child(Divider::new())
                .child(
                    TextWidget::new_literal("SegmentedControl")
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary),
                )
                .child(SegmentedControl::new(
                    vec!["Day".into(), "Week".into(), "Month".into(), "Year".into()],
                    sigs.segment_selected.clone(),
                )),
        )
    }

    fn display_builder(&self, ctx: &mut BuildContext, _theme: &Theme, _sigs: &Signals) -> WidgetId {
        let pb_vert = ctx.add(
            ProgressBar::new(0.7)
                .orientation(Orientation::Vertical)
                .thickness(8.0),
        );
        let progress_section = ctx.add(
            HStack::new()
                .spacing(16.0)
                .child(
                    VStack::new()
                        .spacing(8.0)
                        .child(
                            TextWidget::new_literal("Determinate (65%)")
                                .style(TextStyleRole::Tiny)
                                .color(TextRole::Secondary),
                        )
                        .child(ProgressBar::new(0.65))
                        .child(
                            TextWidget::new_literal("Indeterminate")
                                .style(TextStyleRole::Tiny)
                                .color(TextRole::Secondary),
                        )
                        .child(ProgressBar::indeterminate())
                        .child(
                            TextWidget::new_literal("Custom colors + thick")
                                .style(TextStyleRole::Tiny)
                                .color(TextRole::Secondary),
                        )
                        .child(
                            ProgressBar::new(0.4)
                                .thickness(8.0)
                                .fill_color(TextRole::Success)
                                .track_color(SurfaceRole::Sunken),
                        ),
                )
                .child(
                    VStack::new()
                        .spacing(4.0)
                        .child(
                            TextWidget::new_literal("Vertical")
                                .style(TextStyleRole::Tiny)
                                .color(TextRole::Secondary),
                        )
                        .child(MaxSize::new(f32::MAX, 80.0).child_id(pb_vert)),
                ),
        );

        ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new_literal("Display Widgets")
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new_literal("ProgressBar")
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary),
                )
                .add_child(progress_section)
                .child(Divider::new())
                .child(
                    TextWidget::new_literal("Badge")
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary),
                )
                .child(
                    HStack::new()
                        .spacing(8.0)
                        .child(Badge::new_literal("Default"))
                        .child(
                            Badge::new_literal("3")
                                .color(TextRole::Error)
                                .text_color(Color::WHITE),
                        )
                        .child(
                            Badge::new_literal("New")
                                .color(TextRole::Success)
                                .text_color(Color::WHITE),
                        )
                        .child(Badge::new_literal("Beta").color(TextRole::Warning)),
                )
                .child(Divider::new())
                .child(
                    TextWidget::new_literal("Avatar — sizes (24 / 32 / 48 / 64)")
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary),
                )
                .child(
                    HStack::new()
                        .spacing(12.0)
                        .alignment(VAlignment::Center)
                        .child(Avatar::with_name_literal("Jane Doe").size(AvatarSize::Small))
                        .child(Avatar::with_name_literal("Jane Doe").size(AvatarSize::Medium))
                        .child(Avatar::with_name_literal("Jane Doe").size(AvatarSize::Large))
                        .child(Avatar::with_name_literal("Jane Doe").size(AvatarSize::XLarge)),
                )
                .child(
                    TextWidget::new_literal("Avatar — shapes")
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary),
                )
                .child(
                    HStack::new()
                        .spacing(12.0)
                        .child(
                            Avatar::with_name_literal("Project Alpha")
                                .size(AvatarSize::Large)
                                .shape(AvatarShape::Circle),
                        )
                        .child(
                            Avatar::with_name_literal("Project Alpha")
                                .size(AvatarSize::Large)
                                .shape(AvatarShape::RoundedSquare),
                        )
                        .child(
                            Avatar::with_name_literal("Project Alpha")
                                .size(AvatarSize::Large)
                                .shape(AvatarShape::Square),
                        ),
                )
                .child(
                    TextWidget::new_literal(
                        "Avatar — presence indicator (Online / Away / Busy / Offline)",
                    )
                    .style(TextStyleRole::Small)
                    .color(TextRole::Secondary),
                )
                .child(
                    HStack::new()
                        .spacing(12.0)
                        .child(
                            Avatar::with_name_literal("Sherlock Holmes")
                                .size(AvatarSize::Large)
                                .presence(AvatarPresence::Online),
                        )
                        .child(
                            Avatar::with_name_literal("Marie Curie")
                                .size(AvatarSize::Large)
                                .presence(AvatarPresence::Away),
                        )
                        .child(
                            Avatar::with_name_literal("Ada Lovelace")
                                .size(AvatarSize::Large)
                                .presence(AvatarPresence::Busy),
                        )
                        .child(
                            Avatar::with_name_literal("Nikola Tesla")
                                .size(AvatarSize::Large)
                                .presence(AvatarPresence::Offline),
                        ),
                )
                .child(
                    TextWidget::new_literal(
                        "Avatar — distinct seeds pick distinct hash-derived tints",
                    )
                    .style(TextStyleRole::Small)
                    .color(TextRole::Secondary),
                )
                .child(
                    HStack::new()
                        .spacing(8.0)
                        .child(Avatar::with_name_literal("Anna García"))
                        .child(Avatar::with_name_literal("Bob Wong"))
                        .child(Avatar::with_name_literal("Cher"))
                        .child(Avatar::with_name_literal("Diego Pereira"))
                        .child(Avatar::with_name_literal("Eva Schmidt"))
                        .child(Avatar::with_name_literal("Felix Weber"))
                        .child(Avatar::with_name_literal("Greta Lin"))
                        .child(Avatar::with_name_literal("Hassan Ali")),
                )
                .child(
                    TextWidget::new_literal(
                        "Avatar — outer ring + clickable trigger (Tab to focus, Enter to activate)",
                    )
                    .style(TextStyleRole::Small)
                    .color(TextRole::Secondary),
                )
                .child(
                    HStack::new()
                        .spacing(12.0)
                        .alignment(VAlignment::Center)
                        .child(
                            Avatar::with_name_literal("Jane Doe")
                                .size(AvatarSize::Large)
                                .border(2.0),
                        )
                        .child(
                            Avatar::with_name_literal("Jane Doe")
                                .size(AvatarSize::Large)
                                .label_literal("Open user menu")
                                .on_activate_fn(|_ctx| {
                                    // The catalog wires no real intent here; in a
                                    // real app this would `ctx.send_intent(...)`.
                                }),
                        ),
                ),
        )
    }

    fn text_overflow_builder(
        &self,
        ctx: &mut BuildContext,
        _theme: &Theme,
        _sigs: &Signals,
    ) -> WidgetId {
        const LOREM: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing \
                             elit. Sed do eiusmod tempor incididunt ut labore \
                             et dolore magna aliqua. Ut enim ad minim veniam, \
                             quis nostrud exercitation ullamco laboris nisi ut \
                             aliquip ex ea commodo consequat.";
        const LONG_TITLE: &str =
            "A somewhat verbose section title that almost certainly will not fit";

        ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new_literal("Text overflow")
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary)
                        .single_line(),
                )
                .child(
                    TextWidget::new_literal("Wrap (default) — grows vertically")
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary)
                        .single_line(),
                )
                .child(
                    FixedSize::new().bind_width(360.0_f32).child(
                        TextWidget::new_literal(LOREM)
                            .style(TextStyleRole::Body)
                            .color(TextRole::Primary),
                    ),
                )
                .child(
                    TextWidget::new_literal("Wrap capped at 2 lines")
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary)
                        .single_line(),
                )
                .child(
                    FixedSize::new().bind_width(360.0_f32).child(
                        TextWidget::new_literal(LOREM)
                            .style(TextStyleRole::Body)
                            .color(TextRole::Primary)
                            .max_lines(2),
                    ),
                )
                .child(
                    TextWidget::new_literal("Ellipsis — trailing / middle / leading")
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary)
                        .single_line(),
                )
                .child(
                    FixedSize::new().bind_width(280.0_f32).child(
                        TextWidget::new_literal(LONG_TITLE)
                            .style(TextStyleRole::Body)
                            .color(TextRole::Primary)
                            .overflow(TextOverflow::Ellipsis(EllipsisMode::Trailing)),
                    ),
                )
                .child(
                    FixedSize::new().bind_width(280.0_f32).child(
                        TextWidget::new_literal(LONG_TITLE)
                            .style(TextStyleRole::Body)
                            .color(TextRole::Primary)
                            .overflow(TextOverflow::Ellipsis(EllipsisMode::Middle)),
                    ),
                )
                .child(
                    FixedSize::new().bind_width(280.0_f32).child(
                        TextWidget::new_literal(LONG_TITLE)
                            .style(TextStyleRole::Body)
                            .color(TextRole::Primary)
                            .overflow(TextOverflow::Ellipsis(EllipsisMode::Leading)),
                    ),
                ),
        )
    }

    fn containers_builder(
        &self,
        ctx: &mut BuildContext,
        _theme: &Theme,
        sigs: &Signals,
    ) -> WidgetId {
        let acc_content1 = ctx.add(
            TextWidget::new_literal("This content is revealed with an animated expand.")
                .style(TextStyleRole::Body)
                .color(TextRole::Primary),
        );
        let acc_content2 = ctx.add(
            TextWidget::new_literal("This section starts expanded and can be collapsed.")
                .style(TextStyleRole::Body)
                .color(TextRole::Primary),
        );

        ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new_literal("Containers")
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new_literal("Card")
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary),
                )
                .child(
                    Card::new()
                        .header(
                            TextWidget::new_literal("Card Header")
                                .style(TextStyleRole::Small)
                                .color(TextRole::Secondary),
                        )
                        .content(
                            TextWidget::new_literal(
                                "Card content with shadow and themed background.",
                            )
                            .style(TextStyleRole::Body)
                            .color(TextRole::Primary),
                        )
                        .footer(
                            TextWidget::new_literal("Footer text")
                                .style(TextStyleRole::Tiny)
                                .color(TextRole::Secondary),
                        ),
                )
                .child(Divider::new())
                .child(
                    TextWidget::new_literal("Accordion")
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary),
                )
                .child(
                    Accordion::new_literal("Click to expand", sigs.accordion_expanded.clone())
                        .content_id(acc_content1),
                )
                .child(
                    Accordion::new_literal("Already expanded", sigs.accordion2_expanded.clone())
                        .content_id(acc_content2),
                )
                .child(Divider::new())
                .child(
                    TextWidget::new_literal("ToolBox")
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary),
                )
                .child(
                    ToolBox::new(sigs.tool_box_selected.clone())
                        .add(
                            ToolBoxItem::new_literal(
                                "Outline",
                                Panel::new().padding(12.0).child(
                                    VStack::new()
                                        .spacing(4.0)
                                        .child(TextWidget::new_literal("Chapter 1"))
                                        .child(TextWidget::new_literal("Chapter 2"))
                                        .child(TextWidget::new_literal("Chapter 3")),
                                ),
                            )
                            .leading(IconWidget::chevron_down(14.0))
                            .trailing(Badge::new_literal("3")),
                        )
                        .item_literal(
                            "Properties",
                            Panel::new().padding(12.0).child(
                                VStack::new()
                                    .spacing(4.0)
                                    .child(TextWidget::new_literal("Title: Untitled"))
                                    .child(TextWidget::new_literal("Words: 42 318")),
                            ),
                        )
                        .add(
                            ToolBoxItem::new_literal(
                                "Build tasks (disabled)",
                                Panel::new().padding(12.0).child(TextWidget::new_literal(
                                    "Disabled item — never activates.",
                                )),
                            )
                            .enabled(false),
                        ),
                )
                .child(Divider::new())
                .child(
                    TextWidget::new_literal("GroupBox")
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary),
                )
                .child(
                    GroupBox::new_literal("Appearance").child(
                        VStack::new()
                            .spacing(4.0)
                            .child(
                                TextWidget::new_literal("Indented content under a bold title.")
                                    .style(TextStyleRole::Body)
                                    .color(TextRole::Primary),
                            )
                            .child(
                                TextWidget::new_literal("No border, no frame — Int UI style.")
                                    .style(TextStyleRole::Body)
                                    .color(TextRole::Secondary),
                            ),
                    ),
                )
                .child(
                    GroupBox::new_literal("Notifications")
                        .checkable(sigs.group_box_notifications_on.clone())
                        .child(
                            VStack::new()
                                .spacing(4.0)
                                .child(
                                    TextWidget::new_literal(
                                        "Uncheck the title to disable this whole subtree.",
                                    )
                                    .style(TextStyleRole::Body)
                                    .color(TextRole::Primary),
                                )
                                .child(
                                    Button::new_literal("Inside — tap me")
                                        .style(ButtonVariant::Default),
                                ),
                        ),
                ),
        )
    }

    fn nav_builder(&self, ctx: &mut BuildContext, _theme: &Theme, sigs: &Signals) -> WidgetId {
        let tabs = ctx.add(
            TabWidget::new(sigs.tabs_selected.clone())
                .static_tab(
                    TabInfo::new().title(fern_ui::i18n::LocalizedString::literal("Overview")),
                    Panel::new().padding(16.0).child(
                        VStack::new()
                            .spacing(8.0)
                            .child(
                                TextWidget::new_literal("Overview")
                                    .style(TextStyleRole::BodyBold)
                                    .color(TextRole::Primary),
                            )
                            .child(
                                TextWidget::new_literal(
                                    "TabWidget is a retained container with dormant panes: \
                                     only the active tab is built, inactive panes keep \
                                     their state but don't receive layout or paint until \
                                     they're re-activated.",
                                )
                                .style(TextStyleRole::Body)
                                .color(TextRole::Primary),
                            )
                            .child(
                                HStack::new()
                                    .spacing(8.0)
                                    .child(Badge::new_literal("Dormant panes"))
                                    .child(Badge::new_literal("Arrow keys"))
                                    .child(Badge::new_literal("Trailing slot")),
                            ),
                    ),
                )
                .static_tab(
                    TabInfo::new().title(fern_ui::i18n::LocalizedString::literal("Usage")),
                    Panel::new().padding(16.0).child(
                        VStack::new()
                            .spacing(8.0)
                            .child(
                                TextWidget::new_literal("Usage")
                                    .style(TextStyleRole::BodyBold)
                                    .color(TextRole::Primary),
                            )
                            .child(
                                TextWidget::new_literal(
                                    "Press Tab to move focus into the tab strip, then \
                                     Arrow Left / Arrow Right to switch between tabs. \
                                     Disabled tabs are skipped by keyboard navigation.",
                                )
                                .style(TextStyleRole::Body)
                                .color(TextRole::Primary),
                            ),
                    ),
                )
                .static_tab(
                    TabInfo::new().title(fern_ui::i18n::LocalizedString::literal("Structure")),
                    Panel::new().padding(16.0).child(
                        VStack::new()
                            .spacing(8.0)
                            .child(
                                TextWidget::new_literal("Structure")
                                    .style(TextStyleRole::BodyBold)
                                    .color(TextRole::Primary),
                            )
                            .child(
                                TextWidget::new_literal(
                                    "Int UI tabs: flat headers, no rounded corners, no \
                                     borders. The selected tab is marked only by a 3 dp \
                                     accent underline at its bottom edge, which \
                                     overpaints the tab bar's own 1 dp separator.",
                                )
                                .style(TextStyleRole::Body)
                                .color(TextRole::Primary),
                            ),
                    ),
                )
                .static_tab(
                    TabInfo::new()
                        .title(fern_ui::i18n::LocalizedString::literal("Disabled"))
                        .enabled(false),
                    Panel::new().padding(16.0).child(
                        TextWidget::new_literal(
                            "Disabled panes are still listed in the tab bar but \
                             cannot be activated by click or keyboard.",
                        )
                        .style(TextStyleRole::Body)
                        .color(TextRole::Primary),
                    ),
                )
                .bar_trailing_slot(
                    Button::new_literal("More")
                        .style(ButtonVariant::Flat)
                        .on_activate_fn(|_| println!("LinkClicked")),
                ),
        );
        let tabs_block = ctx.add(FixedSize::new().bind_height(240.0_f32).child_id(tabs));

        ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new_literal("Navigation")
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new_literal("TabWidget")
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary),
                )
                .add_child(tabs_block)
                .child(
                    TextWidget::new_literal("Link")
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary),
                )
                .child(
                    HStack::new()
                        .spacing(16.0)
                        .child(
                            Link::new_literal("Click me")
                                .on_activate_fn(|_| println!("LinkClicked"))
                                .tooltip_literal("Fires the LinkClicked command"),
                        )
                        .child(
                            Link::new_literal("FernUI Documentation")
                                .url("https://github.com/jacquetc/fern-ui"),
                        ),
                ),
        )
    }

    fn rich_tooltips_builder(
        &self,
        ctx: &mut BuildContext,
        _theme: &Theme,
        _sigs: &Signals,
    ) -> WidgetId {
        ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new_literal("Rich Tooltips")
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new_literal(
                        "Hover the buttons below. Inline `[label](:key)` links \
                         open nested tooltips; `https://` links open in the browser.",
                    )
                    .style(TextStyleRole::Small)
                    .color(TextRole::Secondary),
                )
                .child(
                    HStack::new()
                        .spacing(12.0)
                        .child(Button::new_literal("Save As…").rich_tooltip("save-as"))
                        .child(Button::new_literal("Autosave info").rich_tooltip("autosave"))
                        .child(Button::new_literal("Compile").rich_tooltip("compile")),
                ),
        )
    }

    fn message_box_builder(
        &self,
        ctx: &mut BuildContext,
        _theme: &Theme,
        _sigs: &Signals,
    ) -> WidgetId {
        // Each trigger presents a MessageBox exercising one severity +
        // preset combination. The primary comprehensive demo lives in
        // examples/dialogs_and_popovers; this section exists so the
        // catalog's readers can see MessageBox alongside Dialog,
        // Popover, and Snackbar.
        let info_btn = Button::new_literal("Information")
            .style(ButtonVariant::Regular)
            .on_activate_fn(|ctx| {
                ctx.present_message_box(
                    MessageBox::information_literal("Build complete")
                        .text_literal("13 files compiled in 2.4 s.")
                        .buttons(MessageBoxButtons::Ok),
                );
            });
        let question_btn = Button::new_literal("Question")
            .style(ButtonVariant::Regular)
            .on_activate_fn(|ctx| {
                ctx.present_message_box(
                    MessageBox::question_literal("Enable analytics?")
                        .text_literal("Send anonymous usage data to help improve FernUI.")
                        .buttons(MessageBoxButtons::YesNo)
                        .default_button(StandardButton::No),
                );
            });
        let warning_btn = Button::new_literal("Warning")
            .style(ButtonVariant::Regular)
            .on_activate_fn(|ctx| {
                ctx.present_message_box(
                    MessageBox::warning_literal("Unsaved changes")
                        .text_literal("report.skrib has unsaved changes.")
                        .informative_text_literal("Save before closing?")
                        .buttons(MessageBoxButtons::SaveDiscardCancel)
                        .default_button(StandardButton::Save)
                        .escape_button(StandardButton::Cancel),
                );
            });
        let critical_btn = Button::new_literal("Critical")
            .style(ButtonVariant::Regular)
            .on_activate_fn(|ctx| {
                ctx.present_message_box(
                    MessageBox::critical_literal("Could not open file")
                        .text_literal("Insufficient permissions.")
                        .detailed_text_literal("open() returned EACCES (errno 13).")
                        .buttons(MessageBoxButtons::RetryIgnoreAbort)
                        .default_button(StandardButton::Retry),
                );
            });
        let dsa_btn = Button::new_literal("With 'Don't show again'")
            .style(ButtonVariant::Regular)
            .on_activate_fn(|ctx| {
                ctx.present_message_box(
                    MessageBox::information_literal("Welcome")
                        .text_literal("First-time message.")
                        .show_again_checkbox_literal("Don't show this again")
                        .buttons(MessageBoxButtons::Ok),
                );
            });

        ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new_literal("MessageBox")
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new_literal(
                        "QMessageBox-style alert dialog: severity icon + title + 3-level text + standard button row, with Enter → default, Escape → escape button. Critical severity disables click-outside dismiss.",
                    )
                    .style(TextStyleRole::Small)
                    .color(TextRole::Secondary),
                )
                .child(
                    HStack::new()
                        .spacing(10.0)
                        .child(info_btn)
                        .child(question_btn)
                        .child(warning_btn)
                        .child(critical_btn)
                        .child(dsa_btn),
                ),
        )
    }

    fn menus_builder(&self, ctx: &mut BuildContext, _theme: &Theme, sigs: &Signals) -> WidgetId {
        ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new_literal("Menus & Dropdowns")
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new_literal("ComboBox")
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary),
                )
                .child(
                    HStack::new().spacing(16.0).child(
                        ComboBox::new(
                            vec!["Apple", "Banana", "Cherry", "Date", "Elderberry"],
                            sigs.combo_selected.clone(),
                        )
                        .placeholder("Select a fruit..."),
                    ),
                )
                .child(
                    TextWidget::new_literal("Context Menu (right-click the panel below)")
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary),
                )
                .child(
                    Panel::new()
                        .background(SurfaceRole::Main)
                        .corner_radius(8.0)
                        .padding(16.0)
                        .child(
                            TextWidget::new_literal("Right-click here for a context menu")
                                .style(TextStyleRole::Body)
                                .color(TextRole::Primary),
                        )
                        .context_menu(|_pos, _ctx| {
                            Some(Box::new(
                                MenuList::new()
                                    .item(
                                        MenuItem::new_literal("Cut")
                                            .on_activate_fn(|_| println!("Cut"))
                                            .shortcut_label("Ctrl+X"),
                                    )
                                    .item(
                                        MenuItem::new_literal("Copy")
                                            .on_activate_fn(|_| println!("Copy"))
                                            .shortcut_label("Ctrl+C"),
                                    )
                                    .item(
                                        MenuItem::new_literal("Paste")
                                            .on_activate_fn(|_| println!("Paste"))
                                            .shortcut_label("Ctrl+V"),
                                    )
                                    .separator()
                                    .item(MenuItem::new_literal("Disabled item").enabled(false)),
                            ))
                        }),
                ),
        )
    }

    fn image_builder(&self, ctx: &mut BuildContext, _theme: &Theme, _sigs: &Signals) -> WidgetId {
        let tree_img = fern_ui::res!("resources/icons/tree.webp");
        ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new_literal("Image Widget")
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new_literal("Full-color WebP photo, Contain fit, 300x200 display")
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary),
                )
                .child(
                    ImageWidget::new(tree_img)
                        .size(300.0, 200.0)
                        .fit(ImageFit::Contain)
                        .alt("A tree"),
                ),
        )
    }

    fn builtin_builder(&self, ctx: &mut BuildContext, _theme: &Theme, sigs: &Signals) -> WidgetId {
        let vis_label = sigs.visibility_signal.map(|v| {
            if *v {
                "Visible".to_string()
            } else {
                "Hidden".to_string()
            }
        });
        let pin_label = sigs.pinned_signal.map(|v| {
            if *v {
                "Pinned".to_string()
            } else {
                "Unpinned".to_string()
            }
        });

        ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new_literal("Icon Buttons")
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new_literal(
                        "Predefined constructors — stand-alone (default) visual mode",
                    )
                    .style(TextStyleRole::Small)
                    .color(TextRole::Secondary),
                )
                .child(
                    HStack::new()
                        .spacing(4.0)
                        .child(IconButton::browse().on_activate_fn(|_| println!("Browse")))
                        .child(IconButton::expand().on_activate_fn(|_| println!("Expand")))
                        .child(IconButton::search().on_activate_fn(|_| println!("Search")))
                        .child(IconButton::copy().on_activate_fn(|_| println!("Copy")))
                        .child(IconButton::clear().on_activate_fn(|_| println!("Clear")))
                        .child(IconButton::add().on_activate_fn(|_| println!("Add"))),
                )
                .child(
                    TextWidget::new_literal(
                        "Same buttons in .embedded() mode — Secondary at rest, the dim \
                         look used inside TextInput / ComboBox trailing slots",
                    )
                    .style(TextStyleRole::Small)
                    .color(TextRole::Secondary),
                )
                .child(
                    HStack::new()
                        .spacing(4.0)
                        .child(
                            IconButton::browse()
                                .embedded()
                                .on_activate_fn(|_| println!("Browse")),
                        )
                        .child(
                            IconButton::expand()
                                .embedded()
                                .on_activate_fn(|_| println!("Expand")),
                        )
                        .child(
                            IconButton::search()
                                .embedded()
                                .on_activate_fn(|_| println!("Search")),
                        )
                        .child(
                            IconButton::copy()
                                .embedded()
                                .on_activate_fn(|_| println!("Copy")),
                        )
                        .child(
                            IconButton::clear()
                                .embedded()
                                .on_activate_fn(|_| println!("Clear")),
                        )
                        .child(
                            IconButton::add()
                                .embedded()
                                .on_activate_fn(|_| println!("Add")),
                        ),
                )
                .child(
                    TextWidget::new_literal(
                        "Five sizes: Compact (22 dp), Default (24 dp), Toolbar (30 dp), \
                         Large (40 dp), Hero (50 dp)",
                    )
                    .style(TextStyleRole::Small)
                    .color(TextRole::Secondary),
                )
                .child(
                    HStack::new()
                        .spacing(8.0)
                        .child(
                            IconButton::search()
                                .size(IconButtonSize::Compact)
                                .on_activate_fn(|_| println!("Compact")),
                        )
                        .child(IconButton::search().on_activate_fn(|_| println!("Default")))
                        .child(
                            IconButton::search()
                                .toolbar()
                                .on_activate_fn(|_| println!("Toolbar")),
                        )
                        .child(
                            IconButton::search()
                                .large()
                                .on_activate_fn(|_| println!("Large")),
                        )
                        .child(
                            IconButton::search()
                                .hero()
                                .on_activate_fn(|_| println!("Hero")),
                        ),
                )
                .child(
                    TextWidget::new_literal(
                        "Bistate — surface-tint (icon stays). Click to pin / unpin.",
                    )
                    .style(TextStyleRole::Small)
                    .color(TextRole::Secondary),
                )
                .child(
                    HStack::new()
                        .spacing(8.0)
                        .child(
                            IconButton::new(IconWidget::checkmark(20.0))
                                .toolbar()
                                .tooltip_literal("Pin")
                                .toggle(sigs.pinned_signal.clone())
                                .on_activate_fn(|_| println!("TogglePin")),
                        )
                        .child(
                            TextWidget::new_literal("Unpinned")
                                .bind_text(pin_label)
                                .style(TextStyleRole::Body)
                                .color(TextRole::Primary),
                        ),
                )
                .child(
                    TextWidget::new_literal(
                        "Bistate — surface-tint + icon-swap. Visibility toggle (eye / eye-off).",
                    )
                    .style(TextStyleRole::Small)
                    .color(TextRole::Secondary),
                )
                .child(
                    HStack::new()
                        .spacing(8.0)
                        .child(IconButton::visibility_toggle(
                            sigs.visibility_signal.clone(),
                        ))
                        .child(
                            TextWidget::new_literal("Hidden")
                                .bind_text(vis_label)
                                .style(TextStyleRole::Body)
                                .color(TextRole::Primary),
                        ),
                )
                .child(
                    TextWidget::new_literal("Disabled")
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary),
                )
                .child(
                    HStack::new()
                        .spacing(4.0)
                        .child(IconButton::browse().enabled(false))
                        .child(IconButton::clear().enabled(false)),
                )
                .child(
                    TextWidget::new_literal("Custom icon")
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary),
                )
                .child(
                    IconButton::new(IconWidget::checkmark(16.0))
                        .tooltip_literal("Custom checkmark")
                        .on_activate_fn(|_| println!("Save")),
                ),
        )
    }

    fn text_input_builder(
        &self,
        ctx: &mut BuildContext,
        _theme: &Theme,
        sigs: &Signals,
    ) -> WidgetId {
        ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new_literal("Text Input")
                        .style(TextStyleRole::BodyBold)
                        .color(TextRole::Primary),
                )
                .child(
                    TextWidget::new_literal(
                        "Single-line text editing with placeholder, clear button, slots",
                    )
                    .style(TextStyleRole::Small)
                    .color(TextRole::Secondary),
                )
                .child(
                    TextInput::new(sigs.search_text.clone())
                        .placeholder("Search...")
                        .show_clear_button(true)
                        .leading_slot(IconWidget::checkmark(14.0).color(TextRole::Secondary)),
                )
                .child(
                    TextInput::new(sigs.username_text.clone())
                        .placeholder("Username")
                        .label("Username")
                        .trailing_slot(
                            IconButton::browse()
                                .embedded()
                                .on_activate_fn(|_| println!("Save")),
                        ),
                )
                .child(TextInput::new(sigs.readonly_text.clone()).read_only(true)),
        )
    }

    // =========================================================================
    // fern! section helpers — exact equivalents of *_builder methods above.
    // =========================================================================

    fn palette_fern(&self, ctx: &mut BuildContext, theme: &Theme, _sigs: &Signals) -> WidgetId {
        fern!(ctx => VStack {
                spacing: 12.0
                TextWidget::new_literal("Theme Palette") {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new_literal("Surfaces") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                Grid {
                    columns: vec![
                        TrackSize::Fractional(1.0),
                        TrackSize::Fractional(1.0),
                        TrackSize::Fractional(1.0),
                        TrackSize::Fractional(1.0),
                    ]
                    column_gap: 12.0
                    row_gap: 12.0
                    rows: vec![TrackSize::Auto, TrackSize::Auto]
                    child: surface_swatch(theme, SurfaceRole::Main, "surface_main", "text_primary", TextRole::Primary)
                    child: surface_swatch(theme, SurfaceRole::Content, "surface_content", "text_primary", TextRole::Primary)
                    child: surface_swatch(theme, SurfaceRole::Raised, "surface_raised", "text_primary", TextRole::Primary)
                    child: surface_swatch(theme, SurfaceRole::Sunken, "surface_sunken", "text_secondary", TextRole::Secondary)
                    child: surface_swatch(theme, SurfaceRole::Hover, "surface_hover", "text_primary", TextRole::Primary)
                    child: surface_swatch(theme, SurfaceRole::Pressed, "surface_pressed", "text_primary", TextRole::Primary)
                    child: surface_swatch(theme, SurfaceRole::Selected, "surface_selected", "selection_text_active", Color::WHITE)
                    child: surface_swatch(theme, SurfaceRole::SelectedInactive, "surface_selected_inactive", "selection_text_inactive", TextRole::Primary)
                }
                TextWidget::new_literal("Text") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                Panel {
                    background: SurfaceRole::Main
                    border_color: BorderRole::Default
                    border_width: 1.0
                    corner_radius: 8.0
                    padding: 16.0
                    VStack {
                        spacing: 6.0
                        child: text_sample(theme, "text_primary", TextRole::Primary, "body, main labels")
                        child: text_sample(theme, "text_secondary", TextRole::Secondary, "hints, captions, placeholders")
                        child: text_sample(theme, "text_disabled", TextRole::Disabled, "disabled labels")
                        child: text_sample(theme, "text_link", TextRole::Link, "hyperlinks")
                        child: text_sample(theme, "text_error", TextRole::Error, "validation errors")
                        child: text_sample(theme, "text_warning", TextRole::Warning, "validation warnings")
                        child: text_sample(theme, "text_success", TextRole::Success, "success messages")
                    }
                }
                Panel {
                    background: SurfaceRole::Accent
                    corner_radius: 4.0
                    padding: 12.0
                    HStack {
                        spacing: 12.0
                        TextWidget::new_literal("Default button label") {
                            style: TextStyleRole::Body
                            color: TextRole::OnAccent
                        }
                        Spacer
                        TextWidget::new_literal("text_on_accent on accent") {
                            style: TextStyleRole::Tiny
                            color: TextRole::OnAccent
                        }
                    }
                }
                TextWidget::new_literal("Editor") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                Panel {
                    background: SurfaceRole::EditorBg
                    border_color: BorderRole::Strong
                    border_width: 1.0
                    corner_radius: 6.0
                    padding: 8.0
                    VStack {
                        spacing: 2.0
                        child: editor_line(theme, "1", "fn main() {")
                        Panel {
                            background: SurfaceRole::EditorCurrentLineBg
                            corner_radius: 0.0
                            border_width: 0.0
                            padding: 4.0
                            HStack {
                                spacing: 12.0
                                FixedSize {
                                    bind_width: 24.0_f32
                                    TextWidget::new_literal("2") {
                                        style: TextStyleRole::Mono
                                        color: TextRole::EditorGutterFg
                                    }
                                }
                                TextWidget::new_literal("    let ") {
                                    style: TextStyleRole::Mono
                                    color: TextRole::EditorFg
                                }
                                Panel {
                                    background: SurfaceRole::EditorSelectionBg
                                    corner_radius: 2.0
                                    padding: 0.0
                                    border_width: 0.0
                                    Padding::symmetric(1.0, 2.0) {
                                        TextWidget::new_literal("x") {
                                            style: TextStyleRole::Mono
                                            color: TextRole::EditorFg
                                        }
                                    }
                                }
                                TextWidget::new_literal(" = 42;") {
                                    style: TextStyleRole::Mono
                                    color: TextRole::EditorFg
                                }
                                FixedSize {
                                    bind_width: 1.5_f32
                                    bind_height: 16.0_f32
                                    Panel {
                                        background: SurfaceRole::EditorCaret
                                        corner_radius: 0.0
                                        border_width: 0.0
                                        padding: 0.0
                                        Spacer
                                    }
                                }
                            }
                        }
                        child: editor_line(theme, "3", "    println!(\"{}\", x);")
                        child: editor_line(theme, "4", "}")
                    }
                }
                Grid {
                    columns: vec![
                        TrackSize::Fractional(1.0),
                        TrackSize::Fractional(1.0),
                        TrackSize::Fractional(1.0),
                        TrackSize::Fractional(1.0),
                    ]
                    column_gap: 12.0
                    row_gap: 12.0
                    rows: vec![TrackSize::Auto, TrackSize::Auto]
                    child: editor_swatch(theme, SurfaceRole::EditorBg, "editor_bg", TextRole::EditorFg)
                    child: editor_swatch(theme, TextRole::EditorFg, "editor_fg", SurfaceRole::EditorBg)
                    child: editor_swatch(theme, SurfaceRole::EditorCaret, "editor_caret", SurfaceRole::EditorBg)
                    child: editor_swatch(theme, SurfaceRole::EditorCurrentLineBg, "editor_current_line_bg", TextRole::EditorFg)
                    child: editor_swatch(theme, TextRole::EditorGutterFg, "editor_gutter_fg", SurfaceRole::EditorBg)
                    child: editor_swatch(theme, SurfaceRole::EditorSelectionBg, "editor_selection_bg", TextRole::EditorFg)
                }
            }
        )
    }

    fn primitives_fern(&self, ctx: &mut BuildContext, _theme: &Theme, _sigs: &Signals) -> WidgetId {
        fern!(ctx => VStack {
                spacing: 8.0
                TextWidget::new_literal("Primitives") {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new_literal("Divider (horizontal, vertical, thick, colored)") {
                    style: TextStyleRole::Tiny
                    color: TextRole::Secondary
                }
                HStack {
                    spacing: 16.0
                    VStack {
                        spacing: 4.0
                        TextWidget::new_literal("H") {
                            style: TextStyleRole::Tiny
                            color: TextRole::Secondary
                        }
                        Divider
                    }
                    HStack {
                        spacing: 4.0
                        TextWidget::new_literal("V") {
                            style: TextStyleRole::Tiny
                            color: TextRole::Secondary
                        }
                        Divider::vertical() {
                            thickness: 2.0
                            color: TextRole::Accent
                        }
                    }
                    VStack {
                        spacing: 4.0
                        TextWidget::new_literal("Thick") {
                            style: TextStyleRole::Tiny
                            color: TextRole::Secondary
                        }
                        Divider {
                            thickness: 4.0
                            color: TextRole::Error
                        }
                    }
                }
                TextWidget::new_literal("IconWidget (checkmark, chevrons)") {
                    style: TextStyleRole::Tiny
                    color: TextRole::Secondary
                }
                HStack {
                    spacing: 12.0
                    IconWidget::checkmark(24.0) {
                        color: TextRole::Accent
                    }
                    IconWidget::chevron_down(24.0) {
                        color: SurfaceRole::AccentSubtle
                    }
                    IconWidget::chevron_right(24.0) {
                        color: TextRole::Error
                    }
                }
            }
        )
    }

    fn layout_fern(&self, ctx: &mut BuildContext, _theme: &Theme, _sigs: &Signals) -> WidgetId {
        fern!(ctx => VStack {
                spacing: 8.0
                TextWidget::new_literal("Layout Primitives") {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new_literal("Grid (Fixed 80px | 1fr | 2fr, with 8px gap)") {
                    style: TextStyleRole::Tiny
                    color: TextRole::Secondary
                }
                Grid {
                    columns: vec![
                        TrackSize::Fixed(80.0),
                        TrackSize::Fractional(1.0),
                        TrackSize::Fractional(2.0),
                    ]
                    rows: vec![TrackSize::Auto, TrackSize::Auto]
                    column_gap: 8.0
                    row_gap: 8.0
                    child: build_color_cell(SurfaceRole::Accent, "A1")
                    child: build_color_cell(SurfaceRole::AccentSubtle, "B1")
                    child: build_color_cell(TextRole::Error, "C1")
                    child: build_color_cell(TextRole::Accent, "A2")
                    child: build_color_cell(TextRole::Success, "B2")
                    child: build_color_cell(TextRole::Warning, "C2")
                }
                TextWidget::new_literal("Wrap (flow layout, 8px spacing)") {
                    style: TextStyleRole::Tiny
                    color: TextRole::Secondary
                }
                Wrap {
                    spacing: 8.0
                    line_spacing: 8.0
                    Badge::new_literal("Rust")
                    Badge::new_literal("GUI")
                    Badge::new_literal("Accessible")
                    Badge::new_literal("Reactive")
                    Badge::new_literal("Fast")
                    Badge::new_literal("Cross-platform")
                    Badge::new_literal("Retained")
                    Badge::new_literal("wgpu")
                }
            }
        )
    }

    fn controls_fern(&self, ctx: &mut BuildContext, _theme: &Theme, sigs: &Signals) -> WidgetId {
        let save_icon = fern_ui::res!("resources/icons/save.svg");
        let home_icon = fern_ui::res!("resources/icons/home.svg");
        let star_icon = fern_ui::res!("resources/icons/star.png");
        let clock_icon = fern_ui::res!("resources/icons/clock.webp");

        fern!(ctx => VStack {
                spacing: 12.0
                TextWidget::new_literal("Form Controls") {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new_literal("Button") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                VStack {
                    spacing: 8.0
                    GroupHeader::new_literal("Standard buttons")
                    HStack {
                        spacing: 8.0
                        Button::new_literal("Save") {
                            style: ButtonVariant::Default
                            on_activate_fn: |_| println!("Save")
                        }
                        Button::new_literal("Cancel") {
                            style: ButtonVariant::Regular
                            on_activate_fn: |_| println!("Cancel")
                        }
                        Button::new_literal("Learn more") {
                            style: ButtonVariant::Flat
                            on_activate_fn: |_| println!("LearnMore")
                        }
                    }
                    HStack {
                        spacing: 8.0
                        Button::new_literal("Save") {
                            style: ButtonVariant::Default
                            enabled: false
                        }
                        Button::new_literal("Cancel") {
                            style: ButtonVariant::Regular
                            enabled: false
                        }
                        Button::new_literal("Learn more") {
                            style: ButtonVariant::Flat
                            enabled: false
                        }
                    }
                    GroupHeader::new_literal("Icon buttons")
                    HStack {
                        spacing: 8.0
                        Button::new_literal("Save") {
                            icon: IconWidget::from_svg_icon(save_icon), IconLocation::Leading
                            style: ButtonVariant::Default
                            on_activate_fn: |_| println!("Save")
                        }
                        Button::new_literal("Home") {
                            icon: IconWidget::from_svg_icon(home_icon), IconLocation::Leading
                            style: ButtonVariant::Regular
                            on_activate_fn: |_| println!("Cancel")
                        }
                        Button::new_literal("Star") {
                            icon: IconWidget::from_raster(star_icon, 24.0), IconLocation::Leading
                            style: ButtonVariant::Regular
                            on_activate_fn: |_| println!("Save")
                        }
                        Button::new_literal("Clock") {
                            icon: IconWidget::from_raster(clock_icon, 24.0), IconLocation::Leading
                            style: ButtonVariant::Regular
                            on_activate_fn: |_| println!("Cancel")
                        }
                        // Icon-only buttons + a11y overrides — mirror of
                        // the controls_builder() block. `name: value` in
                        // fern! body desugars to `.name(value)`, so each
                        // `.access_*` builder method composes the same way.
                        Button::new_literal("Save") {
                            icon: IconWidget::from_svg_icon(save_icon), IconLocation::IconOnly
                            style: ButtonVariant::Flat
                            on_activate_fn: |_| println!("Save")
                            access_label_literal: "Save"
                            access_shortcut_literal: "Ctrl+S"
                        }
                        Button::new_literal("") {
                            icon: IconWidget::chevron_down(16.0), IconLocation::IconOnly
                            style: ButtonVariant::Regular
                            on_activate_fn: |_| println!("Cancel")
                            access_label_literal: "More options"
                            access_has_popup: fern_ui::core::accesskit::HasPopup::Menu
                        }
                    }
                    GroupHeader::new_literal("Split buttons")
                    HStack {
                        spacing: 8.0
                        SplitButton {
                            item: MenuItem::new_literal("Run") {
                                on_activate_fn: |_| println!("Run")
                                tooltip_literal: "Run the current configuration"
                            }
                            item: MenuItem::new_literal("Run Tests") {
                                on_activate_fn: |_| println!("RunTests")
                                tooltip_literal: "Run the test suite"
                            }
                            item: MenuItem::new_literal("Run with Coverage") {
                                on_activate_fn: |_| println!("RunCoverage")
                                tooltip_literal: "Run and collect code coverage"
                            }
                            separator
                            item: MenuItem::new_literal("Debug") {
                                on_activate_fn: |_| println!("Debug")
                                tooltip_literal: "Launch the debugger"
                            }
                            tooltip_literal: "Run the selected configuration"
                            chevron_tooltip_literal: "Other run configurations"
                            style: ButtonVariant::Default
                        }
                        SplitButton::new_static() {
                            item: MenuItem::new_literal("Save") {
                                on_activate_fn: |_| println!("Save")
                                tooltip_literal: "Save the current file"
                            }
                            item: MenuItem::new_literal("Save As…") {
                                on_activate_fn: |_| println!("SaveAs")
                                tooltip_literal: "Save the current file under a new name"
                            }
                            tooltip_literal: "Save (main action stays pinned)"
                            style: ButtonVariant::Regular
                        }
                        SplitButton {
                            item: MenuItem::new_literal("Run") {
                                on_activate_fn: |_| println!("Run")
                            }
                            item: MenuItem::new_literal("Debug") {
                                on_activate_fn: |_| println!("Debug")
                            }
                            enabled: false
                        }
                    }
                }
                Divider
                Grid {
                    columns: vec![
                        TrackSize::Fractional(1.0),
                        TrackSize::Fractional(1.0),
                        TrackSize::Fractional(1.0),
                    ]
                    column_gap: 16.0
                    rows: vec![TrackSize::Auto]
                    VStack {
                        spacing: 4.0
                        TextWidget::new_literal("Checkbox") {
                            style: TextStyleRole::Small
                            color: TextRole::Secondary
                        }
                        VStack {
                            spacing: 8.0
                            Checkbox(sigs.checkbox_checked.clone()) {
                                label_literal: "Accept terms"
                                tooltip_literal: "Click to accept the terms and conditions"
                            }
                            Checkbox(sigs.cb_sounds.clone()) {
                                label_literal: "Play notification sounds"
                                caption_literal: "Play a short chime when a new message arrives. Muted automatically while you're in a call."
                            }
                            Checkbox(sigs.cb_disabled_state.clone()) {
                                label_literal: "Always on (disabled)"
                                enabled: false
                            }
                            Checkbox::tristate(sigs.tristate.clone()) {
                                label_literal: "Select all (tristate)"
                                caption_literal: "Cycles unchecked → checked → indeterminate"
                                tooltip_literal: "Cycles: unchecked, checked, indeterminate"
                            }
                        }
                    }
                    VStack {
                        spacing: 4.0
                        TextWidget::new_literal("RadioButton") {
                            style: TextStyleRole::Small
                            color: TextRole::Secondary
                        }
                        VStack {
                            spacing: 8.0
                            RadioButton(0, sigs.radio_selected.clone()) {
                                label_literal: "Standard"
                                caption_literal: "Recommended for most users — sensible defaults."
                                tooltip_literal: "First option"
                            }
                            RadioButton(1, sigs.radio_selected.clone()) {
                                label_literal: "Advanced"
                                caption_literal: "Expose every option, including experimental features that may change between releases."
                            }
                            RadioButton(2, sigs.radio_selected.clone()) {
                                label_literal: "Custom"
                            }
                        }
                    }
                    VStack {
                        spacing: 4.0
                        TextWidget::new_literal("Toggle") {
                            style: TextStyleRole::Small
                            color: TextRole::Secondary
                        }
                        VStack {
                            spacing: 8.0
                            Toggle(sigs.toggle_on.clone()) {
                                label_literal: "Enabled"
                            }
                            Toggle(sigs.toggle_label_on.clone()) {
                                label_literal: "Notifications"
                            }
                            Toggle(sigs.toggle_disabled_state.clone()) {
                                label_literal: "Unavailable"
                                enabled: false
                            }
                        }
                    }
                }
                Divider
                TextWidget::new_literal("Slider") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                HStack {
                    spacing: 16.0
                    VStack {
                        spacing: 8.0
                        TextWidget::new_literal("Horizontal") {
                            style: TextStyleRole::Tiny
                            color: TextRole::Secondary
                        }
                        Slider(sigs.slider_value.clone(), 0.0, 100.0)
                        TextWidget::new_literal("Stepped (25)") {
                            style: TextStyleRole::Tiny
                            color: TextRole::Secondary
                        }
                        Slider(sigs.slider_stepped.clone(), 0.0, 100.0) {
                            step: 25.0
                        }
                        TextWidget::new_literal("Disabled") {
                            style: TextStyleRole::Tiny
                            color: TextRole::Secondary
                        }
                        Slider(sigs.slider_disabled_state.clone(), 0.0, 100.0) {
                            enabled: false
                        }
                    }
                    VStack {
                        spacing: 4.0
                        TextWidget::new_literal("Vertical") {
                            style: TextStyleRole::Tiny
                            color: TextRole::Secondary
                        }
                        MaxSize(f32::MAX, 120.0) {
                            Slider(sigs.slider_v_value.clone(), 0.0, 1.0) {
                                orientation: Orientation::Vertical
                            }
                        }
                    }
                }
                Divider
                TextWidget::new_literal("SegmentedControl") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                SegmentedControl(vec!["Day".into(), "Week".into(), "Month".into(), "Year".into()], sigs.segment_selected.clone())
            }
        )
    }

    fn display_fern(&self, ctx: &mut BuildContext, _theme: &Theme, _sigs: &Signals) -> WidgetId {
        fern!(ctx => VStack {
                spacing: 8.0
                TextWidget::new_literal("Display Widgets") {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new_literal("ProgressBar") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                HStack {
                    spacing: 16.0
                    VStack {
                        spacing: 8.0
                        TextWidget::new_literal("Determinate (65%)") {
                            style: TextStyleRole::Tiny
                            color: TextRole::Secondary
                        }
                        ProgressBar(0.65)
                        TextWidget::new_literal("Indeterminate") {
                            style: TextStyleRole::Tiny
                            color: TextRole::Secondary
                        }
                        ProgressBar::indeterminate()
                        TextWidget::new_literal("Custom colors + thick") {
                            style: TextStyleRole::Tiny
                            color: TextRole::Secondary
                        }
                        ProgressBar(0.4) {
                            thickness: 8.0
                            fill_color: TextRole::Success
                            track_color: SurfaceRole::Sunken
                        }
                    }
                    VStack {
                        spacing: 4.0
                        TextWidget::new_literal("Vertical") {
                            style: TextStyleRole::Tiny
                            color: TextRole::Secondary
                        }
                        MaxSize(f32::MAX, 80.0) {
                            ProgressBar(0.7) {
                                orientation: Orientation::Vertical
                                thickness: 8.0
                            }
                        }
                    }
                }
                Divider
                TextWidget::new_literal("Badge") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                HStack {
                    spacing: 8.0
                    Badge::new_literal("Default")
                    Badge::new_literal("3") {
                        color: TextRole::Error
                        text_color: Color::WHITE
                    }
                    Badge::new_literal("New") {
                        color: TextRole::Success
                        text_color: Color::WHITE
                    }
                    Badge::new_literal("Beta") {
                        color: TextRole::Warning
                    }
                }
            }
        )
    }

    fn text_overflow_fern(
        &self,
        ctx: &mut BuildContext,
        _theme: &Theme,
        _sigs: &Signals,
    ) -> WidgetId {
        const LOREM: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing \
                             elit. Sed do eiusmod tempor incididunt ut labore \
                             et dolore magna aliqua. Ut enim ad minim veniam, \
                             quis nostrud exercitation ullamco laboris nisi ut \
                             aliquip ex ea commodo consequat.";
        const LONG_TITLE: &str =
            "A somewhat verbose section title that almost certainly will not fit";

        fern!(ctx => VStack {
                spacing: 8.0
                TextWidget::new_literal("Text overflow") {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                    single_line
                }
                TextWidget::new_literal("Wrap (default) — grows vertically") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                    single_line
                }
                FixedSize {
                    bind_width: 360.0_f32
                    TextWidget::new_literal(LOREM) {
                        style: TextStyleRole::Body
                        color: TextRole::Primary
                    }
                }
                TextWidget::new_literal("Wrap capped at 2 lines") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                    single_line
                }
                FixedSize {
                    bind_width: 360.0_f32
                    TextWidget::new_literal(LOREM) {
                        style: TextStyleRole::Body
                        color: TextRole::Primary
                        max_lines: 2
                    }
                }
                TextWidget::new_literal("Ellipsis — trailing / middle / leading") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                    single_line
                }
                FixedSize {
                    bind_width: 280.0_f32
                    TextWidget::new_literal(LONG_TITLE) {
                        style: TextStyleRole::Body
                        color: TextRole::Primary
                        overflow: TextOverflow::Ellipsis(EllipsisMode::Trailing)
                    }
                }
                FixedSize {
                    bind_width: 280.0_f32
                    TextWidget::new_literal(LONG_TITLE) {
                        style: TextStyleRole::Body
                        color: TextRole::Primary
                        overflow: TextOverflow::Ellipsis(EllipsisMode::Middle)
                    }
                }
                FixedSize {
                    bind_width: 280.0_f32
                    TextWidget::new_literal(LONG_TITLE) {
                        style: TextStyleRole::Body
                        color: TextRole::Primary
                        overflow: TextOverflow::Ellipsis(EllipsisMode::Leading)
                    }
                }
            }
        )
    }

    fn containers_fern(&self, ctx: &mut BuildContext, _theme: &Theme, sigs: &Signals) -> WidgetId {
        fern!(ctx => VStack {
                spacing: 8.0
                TextWidget::new_literal("Containers") {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new_literal("Card") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                Card {
                    header: TextWidget::new_literal("Card Header") {
                        style: TextStyleRole::Small
                        color: TextRole::Secondary
                    }
                    content: (TextWidget::new_literal("Card content with shadow and themed background.").style(TextStyleRole::Body).color(TextRole::Primary))

                    footer: TextWidget::new_literal("Footer text") {
                        style: TextStyleRole::Tiny
                        color: TextRole::Secondary
                    }
                }
                Divider
                TextWidget::new_literal("Accordion") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                Accordion::new_literal("Click to expand", sigs.accordion_expanded.clone()) {
                    content: TextWidget::new_literal("This content is revealed with an animated expand.") {
                        style: TextStyleRole::Body
                        color: TextRole::Primary
                    }
                }
                Accordion::new_literal("Already expanded", sigs.accordion2_expanded.clone()) {
                    content: TextWidget::new_literal("This section starts expanded and can be collapsed.") {
                        style: TextStyleRole::Body
                        color: TextRole::Primary
                    }
                }
                Divider
                TextWidget::new_literal("ToolBox") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                let disabled_tool_box_item = ToolBoxItem::new_literal(
                    "Build tasks (disabled)",
                    Panel::new().padding(12.0).child(
                        TextWidget::new_literal("Disabled item — never activates."),
                    ),
                )
                .enabled(false);
                ToolBox(sigs.tool_box_selected.clone()) {
                    item_literal: "Outline", Panel {
                        padding: 12.0
                        VStack {
                            spacing: 4.0
                            TextWidget::new_literal("Chapter 1")
                            TextWidget::new_literal("Chapter 2")
                            TextWidget::new_literal("Chapter 3")
                        }
                    }
                    item_literal: "Properties", Panel {
                        padding: 12.0
                        VStack {
                            spacing: 4.0
                            TextWidget::new_literal("Title: Untitled")
                            TextWidget::new_literal("Words: 42 318")
                        }
                    }
                    add: disabled_tool_box_item
                }
                Divider
                TextWidget::new_literal("GroupBox") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                GroupBox::new_literal("Appearance") {
                    child: VStack {
                        spacing: 4.0
                        TextWidget::new_literal("Indented content under a bold title.") {
                            style: TextStyleRole::Body
                            color: TextRole::Primary
                        }
                        TextWidget::new_literal("No border, no frame — Int UI style.") {
                            style: TextStyleRole::Body
                            color: TextRole::Secondary
                        }
                    }
                }
                GroupBox::new_literal("Notifications") {
                    checkable: sigs.group_box_notifications_on.clone()
                    child: VStack {
                        spacing: 4.0
                        TextWidget::new_literal("Uncheck the title to disable this whole subtree.") {
                            style: TextStyleRole::Body
                            color: TextRole::Primary
                        }
                        Button::new_literal("Inside — tap me") {
                            style: ButtonVariant::Default
                        }
                    }
                }
            }
        )
    }

    fn nav_fern(&self, ctx: &mut BuildContext, _theme: &Theme, sigs: &Signals) -> WidgetId {
        fern!(ctx => VStack {
                spacing: 8.0
                TextWidget::new_literal("Navigation") {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new_literal("TabWidget") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                FixedSize {
                    bind_height: 240.0_f32
                    TabWidget(sigs.tabs_selected.clone()) {
                        static_tab: tab_info("Overview"), Panel {
                            padding: 16.0
                            VStack {
                                spacing: 8.0
                                TextWidget::new_literal("Overview") {
                                    style: TextStyleRole::BodyBold
                                    color: TextRole::Primary
                                }
                                TextWidget::new_literal("TabWidget is a retained container with dormant panes: \
                                                             only the active tab is built, inactive panes keep \
                                                             their state but don't receive layout or paint until \
                                                             they're re-activated.") {
                                    style: TextStyleRole::Body
                                    color: TextRole::Primary
                                }
                                HStack {
                                    spacing: 8.0
                                    Badge::new_literal("Dormant panes")
                                    Badge::new_literal("Arrow keys")
                                    Badge::new_literal("Trailing slot")
                                }
                            }
                        }
                        static_tab: tab_info("Usage"), Panel {
                            padding: 16.0
                            VStack {
                                spacing: 8.0
                                TextWidget::new_literal("Usage") {
                                    style: TextStyleRole::BodyBold
                                    color: TextRole::Primary
                                }
                                TextWidget::new_literal("Press Tab to move focus into the tab strip, then \
                                                             Arrow Left / Arrow Right to switch between tabs. \
                                                             Disabled tabs are skipped by keyboard navigation.") {
                                    style: TextStyleRole::Body
                                    color: TextRole::Primary
                                }
                            }
                        }
                        static_tab: tab_info("Structure"), Panel {
                            padding: 16.0
                            VStack {
                                spacing: 8.0
                                TextWidget::new_literal("Structure") {
                                    style: TextStyleRole::BodyBold
                                    color: TextRole::Primary
                                }
                                TextWidget::new_literal("Int UI tabs: flat headers, no rounded corners, no \
                                                             borders. The selected tab is marked only by a 3 dp \
                                                             accent underline at its bottom edge, which \
                                                             overpaints the tab bar's own 1 dp separator.") {
                                    style: TextStyleRole::Body
                                    color: TextRole::Primary
                                }
                            }
                        }
                        static_tab: tab_info_disabled("Disabled"), Panel {
                            padding: 16.0
                            TextWidget::new_literal("Disabled panes are still listed in the tab bar but \
                                             cannot be activated by click or keyboard.") {
                                style: TextStyleRole::Body
                                color: TextRole::Primary
                            }
                        }
                        bar_trailing_slot: Button::new_literal("More") {
                            style: ButtonVariant::Flat
                            on_activate_fn: |_| println!("LinkClicked")
                        }
                    }
                }
                TextWidget::new_literal("Link") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                HStack {
                    spacing: 16.0
                    Link::new_literal("Click me") {
                        on_activate_fn: |_| println!("LinkClicked")
                        tooltip_literal: "Fires the LinkClicked command"
                    }
                    Link::new_literal("FernUI Documentation") {
                        url: "https://github.com/jacquetc/fern-ui"
                    }
                }
            }
        )
    }

    fn rich_tooltips_fern(
        &self,
        ctx: &mut BuildContext,
        _theme: &Theme,
        _sigs: &Signals,
    ) -> WidgetId {
        fern!(ctx => VStack {
                spacing: 8.0
                TextWidget::new_literal("Rich Tooltips") {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new_literal("Hover the buttons below. Inline `[label](:key)` links \
                                             open nested tooltips; `https://` links open in the browser.") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                HStack {
                    spacing: 12.0
                    Button::new_literal("Save As…") {
                        rich_tooltip: "save-as"
                    }
                    Button::new_literal("Autosave info") {
                        rich_tooltip: "autosave"
                    }
                    Button::new_literal("Compile") {
                        rich_tooltip: "compile"
                    }
                }
            }
        )
    }

    fn message_box_fern(
        &self,
        ctx: &mut BuildContext,
        _theme: &Theme,
        _sigs: &Signals,
    ) -> WidgetId {
        fern!(ctx => VStack {
                spacing: 8.0
                TextWidget::new_literal("MessageBox") {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new_literal("QMessageBox-style alert dialog: severity icon + title + 3-level text + standard button row, with Enter → default, Escape → escape button. Critical severity disables click-outside dismiss.") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                HStack {
                    spacing: 10.0
                    Button::new_literal("Information") {
                        style: ButtonVariant::Regular
                        on_activate_fn: |ctx| {
                            ctx.present_message_box(
                                MessageBox::information_literal("Build complete")
                                    .text_literal("13 files compiled in 2.4 s.")
                                    .buttons(MessageBoxButtons::Ok),
                            );
                        }
                    }
                    Button::new_literal("Question") {
                        style: ButtonVariant::Regular
                        on_activate_fn: |ctx| {
                            ctx.present_message_box(
                                MessageBox::question_literal("Enable analytics?")
                                    .text_literal("Send anonymous usage data to help improve FernUI.")
                                    .buttons(MessageBoxButtons::YesNo)
                                    .default_button(StandardButton::No),
                            );
                        }
                    }
                    Button::new_literal("Warning") {
                        style: ButtonVariant::Regular
                        on_activate_fn: |ctx| {
                            ctx.present_message_box(
                                MessageBox::warning_literal("Unsaved changes")
                                    .text_literal("report.skrib has unsaved changes.")
                                    .informative_text_literal("Save before closing?")
                                    .buttons(MessageBoxButtons::SaveDiscardCancel)
                                    .default_button(StandardButton::Save)
                                    .escape_button(StandardButton::Cancel),
                            );
                        }
                    }
                    Button::new_literal("Critical") {
                        style: ButtonVariant::Regular
                        on_activate_fn: |ctx| {
                            ctx.present_message_box(
                                MessageBox::critical_literal("Could not open file")
                                    .text_literal("Insufficient permissions.")
                                    .detailed_text_literal("open() returned EACCES (errno 13).")
                                    .buttons(MessageBoxButtons::RetryIgnoreAbort)
                                    .default_button(StandardButton::Retry),
                            );
                        }
                    }
                    Button::new_literal("With 'Don't show again'") {
                        style: ButtonVariant::Regular
                        on_activate_fn: |ctx| {
                            ctx.present_message_box(
                                MessageBox::information_literal("Welcome")
                                    .text_literal("First-time message.")
                                    .show_again_checkbox_literal("Don't show this again")
                                    .buttons(MessageBoxButtons::Ok),
                            );
                        }
                    }
                }
            }
        )
    }

    fn menus_fern(&self, ctx: &mut BuildContext, _theme: &Theme, sigs: &Signals) -> WidgetId {
        fern!(ctx => VStack {
                spacing: 8.0
                TextWidget::new_literal("Menus & Dropdowns") {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new_literal("ComboBox") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                HStack {
                    spacing: 16.0
                    ComboBox(vec!["Apple", "Banana", "Cherry", "Date", "Elderberry"], sigs.combo_selected.clone()) {
                        placeholder: "Select a fruit..."
                    }
                }
                TextWidget::new_literal("Context Menu (right-click the panel below)") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                Panel {
                    background: SurfaceRole::Main
                    corner_radius: 8.0
                    padding: 16.0
                    context_menu: |_pos, _ctx| Some(Box::new(
                        MenuList::new()
                            .item(MenuItem::new_literal("Cut").on_activate_fn(|_| println!("Cut")).shortcut_label("Ctrl+X"))
                            .item(MenuItem::new_literal("Copy").on_activate_fn(|_| println!("Copy")).shortcut_label("Ctrl+C"))
                            .item(MenuItem::new_literal("Paste").on_activate_fn(|_| println!("Paste")).shortcut_label("Ctrl+V"))
                            .separator()
                            .item(MenuItem::new_literal("Disabled item").enabled(false))
                    ))
                    TextWidget::new_literal("Right-click here for a context menu") {
                        style: TextStyleRole::Body
                        color: TextRole::Primary
                    }
                }
            }
        )
    }

    fn image_fern(&self, ctx: &mut BuildContext, _theme: &Theme, _sigs: &Signals) -> WidgetId {
        let tree_img = fern_ui::res!("resources/icons/tree.webp");

        fern!(ctx => VStack {
                spacing: 8.0
                TextWidget::new_literal("Image Widget") {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new_literal("Full-color WebP photo, Contain fit, 300x200 display") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                ImageWidget(tree_img) {
                    size: 300.0, 200.0
                    fit: ImageFit::Contain
                    alt: "A tree"
                }
            }
        )
    }

    fn builtin_fern(&self, ctx: &mut BuildContext, _theme: &Theme, sigs: &Signals) -> WidgetId {
        let vis_label = sigs.visibility_signal.map(|v| {
            if *v {
                "Visible".to_string()
            } else {
                "Hidden".to_string()
            }
        });
        let pin_label = sigs.pinned_signal.map(|v| {
            if *v {
                "Pinned".to_string()
            } else {
                "Unpinned".to_string()
            }
        });

        fern!(ctx => VStack {
                spacing: 8.0
                TextWidget::new_literal("Icon Buttons") {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new_literal("Predefined constructors — stand-alone (default) visual mode") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                HStack {
                    spacing: 4.0
                    IconButton::browse() {
                        on_activate_fn: |_| println!("Browse")
                    }
                    IconButton::expand() {
                        on_activate_fn: |_| println!("Expand")
                    }
                    IconButton::search() {
                        on_activate_fn: |_| println!("Search")
                    }
                    IconButton::copy() {
                        on_activate_fn: |_| println!("Copy")
                    }
                    IconButton::clear() {
                        on_activate_fn: |_| println!("Clear")
                    }
                    IconButton::add() {
                        on_activate_fn: |_| println!("Add")
                    }
                }
                TextWidget::new_literal("Same buttons in .embedded() mode — Secondary at rest") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                HStack {
                    spacing: 4.0
                    IconButton::browse() {
                        embedded
                        on_activate_fn: |_| println!("Browse")
                    }
                    IconButton::expand() {
                        embedded
                        on_activate_fn: |_| println!("Expand")
                    }
                    IconButton::search() {
                        embedded
                        on_activate_fn: |_| println!("Search")
                    }
                    IconButton::copy() {
                        embedded
                        on_activate_fn: |_| println!("Copy")
                    }
                    IconButton::clear() {
                        embedded
                        on_activate_fn: |_| println!("Clear")
                    }
                    IconButton::add() {
                        embedded
                        on_activate_fn: |_| println!("Add")
                    }
                }
                TextWidget::new_literal("Five sizes: Compact, Default, Toolbar, Large, Hero") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                HStack {
                    spacing: 8.0
                    IconButton::search() {
                        size: IconButtonSize::Compact
                        on_activate_fn: |_| println!("Compact")
                    }
                    IconButton::search() {
                        on_activate_fn: |_| println!("Default")
                    }
                    IconButton::search() {
                        toolbar
                        on_activate_fn: |_| println!("Toolbar")
                    }
                    IconButton::search() {
                        large
                        on_activate_fn: |_| println!("Large")
                    }
                    IconButton::search() {
                        hero
                        on_activate_fn: |_| println!("Hero")
                    }
                }
                TextWidget::new_literal("Bistate — surface-tint (icon stays). Click to pin / unpin.") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                HStack {
                    spacing: 8.0
                    IconButton(IconWidget::checkmark(20.0)) {
                        toolbar
                        tooltip_literal: "Pin"
                        toggle: sigs.pinned_signal.clone()
                        on_activate_fn: |_| println!("TogglePin")
                    }
                    TextWidget::new_literal("Unpinned") {
                        bind_text: pin_label
                        style: TextStyleRole::Body
                        color: TextRole::Primary
                    }
                }
                TextWidget::new_literal("Bistate — surface-tint + icon-swap (visibility toggle)") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                HStack {
                    spacing: 8.0
                    IconButton::visibility_toggle(sigs.visibility_signal.clone())
                    TextWidget::new_literal("Hidden") {
                        bind_text: vis_label
                        style: TextStyleRole::Body
                        color: TextRole::Primary
                    }
                }
                TextWidget::new_literal("Disabled") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                HStack {
                    spacing: 4.0
                    IconButton::browse() {
                        enabled: false
                    }
                    IconButton::clear() {
                        enabled: false
                    }
                }
                TextWidget::new_literal("Custom icon") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                IconButton(IconWidget::checkmark(16.0)) {
                    tooltip_literal: "Custom checkmark"
                    on_activate_fn: |_| println!("Save")
                }
            }
        )
    }

    fn text_input_fern(&self, ctx: &mut BuildContext, _theme: &Theme, sigs: &Signals) -> WidgetId {
        fern!(ctx => VStack {
                spacing: 8.0
                TextWidget::new_literal("Text Input") {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new_literal("Single-line text editing with placeholder, clear button, slots") {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
                TextInput(sigs.search_text.clone()) {
                    placeholder: "Search..."
                    show_clear_button: true
                    leading_slot: IconWidget::checkmark(14.0) {
                        color: TextRole::Secondary
                    }
                }
                TextInput(sigs.username_text.clone()) {
                    placeholder: "Username"
                    label: "Username"
                    trailing_slot: IconButton::browse() {
                        embedded
                        on_activate_fn: |_| println!("Save")
                    }
                }
                TextInput(sigs.readonly_text.clone()) {
                    read_only: true
                }
            }
        )
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    FernAppBuilder::new()
        .install_inspector_in_debug()
        .theme(Theme::light_default())
        .register_tooltips(vec![
            TooltipContent::new(
                "save-as",
                LocalizedString::literal("Save the current file under a new name"),
            )
            .with_shortcut_label("Ctrl+Shift+S"),
            TooltipContent::new(
                "autosave",
                LocalizedString::literal(
                    "FernUI **autosaves** your work every *2 minutes*. \
                     Click [here](:autosave-details) for details, or \
                     read the [full docs](https://github.com/jacquetc/fern-ui).",
                ),
            )
            .with_more(LocalizedString::literal(
                "Autosave uses **debounced writes** so bursts of edits \
                 only hit disk once. Disable it in [Preferences](:prefs-general).",
            )),
            TooltipContent::new(
                "autosave-details",
                LocalizedString::literal(
                    "Autosave runs on a debounced timer: it only writes \
                     to disk after typing pauses for 500ms.",
                ),
            ),
            TooltipContent::new(
                "prefs-general",
                LocalizedString::literal(
                    "Open Preferences → General to toggle autosave and \
                     change its interval.",
                ),
            ),
            TooltipContent::new(
                "compile",
                LocalizedString::literal(
                    "**Compile** the current project. Uses incremental \
                     builds when possible.",
                ),
            )
            .with_more(LocalizedString::literal(
                "The compile step runs `cargo build` under the hood. \
                 Output lands in the Build tab.",
            ))
            .with_shortcut_label("F9"),
        ])
        .initial_window(
            WindowConfig::new()
                .title("FernUI -- Widget Catalog (Milestone 3)")
                .size(1600, 900)
                .root(|tree, _state| tree.add(WidgetCatalog::new())),
        )
        .run();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use fern_render::test_support;
    use fern_ui::core::WidgetTree;
    use fern_ui::prelude::*;
    use fern_ui::text::SharedTypesetter;
    use fern_ui::widgets::*;

    use super::WidgetCatalog;

    fn tree_with_real_text_backend() -> WidgetTree {
        let typesetter = SharedTypesetter::new_with_default_font();
        WidgetTree::new()
            .with_theme(Theme::light_default())
            .with_text_backend(typesetter.as_text_backend())
    }

    fn tree_and_typesetter() -> (WidgetTree, SharedTypesetter) {
        let typesetter = SharedTypesetter::new_with_default_font();
        let tree = WidgetTree::new()
            .with_theme(Theme::light_default())
            .with_text_backend(typesetter.as_text_backend());
        (tree, typesetter)
    }

    #[test]
    fn catalog_builds_and_layouts() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let root = tree.add(WidgetCatalog::new());
        tree.layout(SizeProposal::exact(1600.0, 900.0));
        let b = tree.bounds(root);
        assert!(b.width > 0.0);
        assert!(b.height > 0.0);
    }

    #[test]
    fn catalog_renders_without_crash() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(WidgetCatalog::new());
        tree.layout(SizeProposal::exact(1600.0, 900.0));
        let frame = tree.render();
        assert!(
            !frame.shapes.is_empty() || !frame.decorations.is_empty(),
            "catalog should produce render output"
        );
    }

    #[test]
    fn catalog_theme_switch() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(WidgetCatalog::new());
        tree.layout(SizeProposal::exact(1600.0, 900.0));
        let frame_light = tree.render();

        tree.set_theme(Theme::dark_default());
        tree.layout(SizeProposal::exact(1600.0, 900.0));
        let frame_dark = tree.render();

        assert_ne!(
            frame_light.shapes, frame_dark.shapes,
            "theme switch should produce different output"
        );
    }

    /// Diagnostic helper exposing theme-switch coverage. Run with
    /// `cargo test -p widget-catalog catalog_theme_switch_coverage -- --nocapture`
    /// to see how many unique shape/glyph colors survive vs. update on switch.
    #[test]
    fn catalog_theme_switch_coverage() {
        let typesetter = SharedTypesetter::new_with_default_font();
        let mut tree = WidgetTree::new()
            .with_theme(Theme::light_default())
            .with_text_backend(typesetter.as_text_backend());
        tree.add(WidgetCatalog::new());
        tree.layout(SizeProposal::exact(1600.0, 900.0));
        let frame_light = tree.render();
        let light_shape_count = frame_light.shapes.len();
        let light_glyph_count = frame_light.glyphs.len();
        let light_shape_palette: std::collections::HashSet<[u8; 4]> = frame_light
            .shapes
            .iter()
            .map(|s| {
                [
                    (s.color[0] * 255.0) as u8,
                    (s.color[1] * 255.0) as u8,
                    (s.color[2] * 255.0) as u8,
                    (s.color[3] * 255.0) as u8,
                ]
            })
            .collect();
        let light_glyph_palette: std::collections::HashSet<[u8; 4]> = frame_light
            .glyphs
            .iter()
            .map(|g| {
                [
                    (g.color[0] * 255.0) as u8,
                    (g.color[1] * 255.0) as u8,
                    (g.color[2] * 255.0) as u8,
                    (g.color[3] * 255.0) as u8,
                ]
            })
            .collect();

        tree.set_theme(Theme::dark_default());
        tree.layout(SizeProposal::exact(1600.0, 900.0));
        let frame_dark = tree.render();
        let dark_shape_palette: std::collections::HashSet<[u8; 4]> = frame_dark
            .shapes
            .iter()
            .map(|s| {
                [
                    (s.color[0] * 255.0) as u8,
                    (s.color[1] * 255.0) as u8,
                    (s.color[2] * 255.0) as u8,
                    (s.color[3] * 255.0) as u8,
                ]
            })
            .collect();
        let dark_glyph_palette: std::collections::HashSet<[u8; 4]> = frame_dark
            .glyphs
            .iter()
            .map(|g| {
                [
                    (g.color[0] * 255.0) as u8,
                    (g.color[1] * 255.0) as u8,
                    (g.color[2] * 255.0) as u8,
                    (g.color[3] * 255.0) as u8,
                ]
            })
            .collect();

        let shape_shared = light_shape_palette
            .intersection(&dark_shape_palette)
            .count();
        let glyph_shared = light_glyph_palette
            .intersection(&dark_glyph_palette)
            .count();

        let mut shared_glyphs: Vec<_> = light_glyph_palette
            .intersection(&dark_glyph_palette)
            .copied()
            .collect();
        shared_glyphs.sort();
        eprintln!("shared glyph colors (present in both light and dark):");
        for cc in &shared_glyphs {
            eprintln!("  #{:02X}{:02X}{:02X}{:02X}", cc[0], cc[1], cc[2], cc[3]);
        }

        eprintln!("widget-catalog theme switch coverage:");
        eprintln!(
            "  shapes: {} ops, palette {}→{} (shared {}, changed light→dark {})",
            light_shape_count,
            light_shape_palette.len(),
            dark_shape_palette.len(),
            shape_shared,
            light_shape_palette.len() - shape_shared,
        );
        eprintln!(
            "  glyphs: {} ops, palette {}→{} (shared {}, changed light→dark {})",
            light_glyph_count,
            light_glyph_palette.len(),
            dark_glyph_palette.len(),
            glyph_shared,
            light_glyph_palette.len() - glyph_shared,
        );

        // The assertion isn't strict — it exists only to guarantee that theme
        // switching visibly changes SOMETHING in the catalog. If both palettes
        // were identical, this test would fail and flag a regression.
        assert!(
            light_shape_palette != dark_shape_palette || light_glyph_palette != dark_glyph_palette,
            "theme switch must change either shape or glyph palette"
        );
    }

    /// Diagnostic: quantify how much of the render actually reflects the new
    /// theme. Shape colors (panel / rect backgrounds) and glyph colors
    /// should both change. If the widget catalog ever regresses to
    /// build-time-frozen colors, this test flags which bucket is stuck.
    #[test]
    fn catalog_theme_switch_affects_shapes_and_glyphs() {
        let typesetter = SharedTypesetter::new_with_default_font();
        let mut tree = WidgetTree::new()
            .with_theme(Theme::light_default())
            .with_text_backend(typesetter.as_text_backend());
        tree.add(WidgetCatalog::new());
        tree.layout(SizeProposal::exact(1600.0, 900.0));
        let frame_light = tree.render();

        tree.set_theme(Theme::dark_default());
        tree.layout(SizeProposal::exact(1600.0, 900.0));
        let frame_dark = tree.render();

        let shape_colors_light: std::collections::HashSet<[u8; 4]> = frame_light
            .shapes
            .iter()
            .map(|s| {
                [
                    (s.color[0] * 255.0) as u8,
                    (s.color[1] * 255.0) as u8,
                    (s.color[2] * 255.0) as u8,
                    (s.color[3] * 255.0) as u8,
                ]
            })
            .collect();
        let shape_colors_dark: std::collections::HashSet<[u8; 4]> = frame_dark
            .shapes
            .iter()
            .map(|s| {
                [
                    (s.color[0] * 255.0) as u8,
                    (s.color[1] * 255.0) as u8,
                    (s.color[2] * 255.0) as u8,
                    (s.color[3] * 255.0) as u8,
                ]
            })
            .collect();
        assert!(
            !shape_colors_light.is_subset(&shape_colors_dark),
            "shape palette should change: light={} dark={}",
            shape_colors_light.len(),
            shape_colors_dark.len()
        );

        let glyph_colors_light: std::collections::HashSet<[u8; 4]> = frame_light
            .glyphs
            .iter()
            .map(|g| {
                [
                    (g.color[0] * 255.0) as u8,
                    (g.color[1] * 255.0) as u8,
                    (g.color[2] * 255.0) as u8,
                    (g.color[3] * 255.0) as u8,
                ]
            })
            .collect();
        let glyph_colors_dark: std::collections::HashSet<[u8; 4]> = frame_dark
            .glyphs
            .iter()
            .map(|g| {
                [
                    (g.color[0] * 255.0) as u8,
                    (g.color[1] * 255.0) as u8,
                    (g.color[2] * 255.0) as u8,
                    (g.color[3] * 255.0) as u8,
                ]
            })
            .collect();
        assert_ne!(
            glyph_colors_light, glyph_colors_dark,
            "glyph (text) colors should change on theme switch — \
             this flags TextWidgets that captured light-theme colors at build time \
             (use .bind_color with a theme_signal-derived Signal instead of .color(...))"
        );
    }

    #[test]
    fn second_render_same_output() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(WidgetCatalog::new());
        tree.layout(SizeProposal::exact(1600.0, 900.0));
        let frame1 = tree.render();
        let cmds1 = frame1.draw_order.len();

        tree.layout(SizeProposal::exact(1600.0, 900.0));
        let frame2 = tree.render();
        let cmds2 = frame2.draw_order.len();

        assert_eq!(
            cmds1, cmds2,
            "second render must produce same draw commands (got {cmds1} vs {cmds2})"
        );
    }

    #[test]
    fn theme_switch_preserves_draw_commands() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(WidgetCatalog::new());
        tree.layout(SizeProposal::exact(1600.0, 900.0));
        let frame1 = tree.render();
        let cmds_before = frame1.draw_order.len();

        tree.set_theme(Theme::dark_default());
        tree.layout(SizeProposal::exact(1600.0, 900.0));
        let frame2 = tree.render();
        let cmds_after = frame2.draw_order.len();

        assert!(
            cmds_after > cmds_before / 2,
            "draw commands after theme switch ({cmds_after}) should be \
             close to before ({cmds_before})"
        );
    }

    #[test]
    fn scroll_area_fills_remaining_space() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let root = tree.add(WidgetCatalog::new());
        tree.layout(SizeProposal::exact(1600.0, 900.0));

        // root is WidgetCatalog → SplitView → [first, handle, second],
        // each pane is a VStack → [Toolbar, Expand, StatusBar].
        let split_children = {
            let adapter_children = tree.children(root);
            assert_eq!(
                adapter_children.len(),
                1,
                "composite adapter has one child (SplitView)"
            );
            tree.children(adapter_children[0])
        };
        assert_eq!(
            split_children.len(),
            3,
            "SplitView must have first, handle, second (got {})",
            split_children.len()
        );

        for (label, pane_id) in [("left", split_children[0]), ("right", split_children[2])] {
            // SplitView wraps each pane in a `ClipPane` container so
            // overflowing content is clipped to the pane bounds — the
            // actual VStack is one level deeper.
            let clip_children = tree.children(pane_id);
            assert_eq!(
                clip_children.len(),
                1,
                "{label} pane is a ClipPane with exactly one child"
            );
            let vstack_children = tree.children(clip_children[0]);
            assert_eq!(
                vstack_children.len(),
                3,
                "{label} pane VStack must have toolbar, expand, statusbar"
            );

            let toolbar_bounds = tree.bounds(vstack_children[0]);
            let expand_bounds = tree.bounds(vstack_children[1]);
            let status_bounds = tree.bounds(vstack_children[2]);

            assert!(
                expand_bounds.height > 400.0,
                "{label} pane Expand wrapping ScrollArea should fill >400px, got {}",
                expand_bounds.height
            );

            assert!(
                (status_bounds.y + status_bounds.height - 900.0).abs() < 1.0,
                "{label} pane StatusBar should reach bottom of window: y={}, h={}",
                status_bounds.y,
                status_bounds.height
            );

            assert!(
                expand_bounds.y >= toolbar_bounds.y + toolbar_bounds.height - 0.1,
                "{label} pane Expand should start below toolbar"
            );
            assert!(
                status_bounds.y >= expand_bounds.y + expand_bounds.height - 0.1,
                "{label} pane StatusBar should start below Expand"
            );
        }
    }

    #[test]
    fn badge_renders_text_with_real_text_backend() {
        let mut tree = tree_with_real_text_backend();
        tree.add(Badge::new_literal("Badge text"));
        tree.layout(SizeProposal::exact(200.0, 80.0));

        let frame = tree.render();
        assert!(
            !frame.glyphs.is_empty(),
            "badge should emit glyphs when rendered with the real text backend"
        );
    }

    #[test]
    fn catalog_missing_text_candidates_produce_bright_pixels_offscreen() {
        const TEST_HEIGHT: u32 = 2048;

        fn pixel_extrema(
            pixels: &[u8],
            width: u32,
            height: u32,
            bounds: Rect,
        ) -> ([u8; 3], [u8; 3]) {
            let x0 = bounds.x.floor().max(0.0) as u32;
            let y0 = bounds.y.floor().max(0.0) as u32;
            let x1 = (bounds.x + bounds.width).ceil().max(0.0) as u32;
            let y1 = (bounds.y + bounds.height).ceil().max(0.0) as u32;

            let mut best = [0u8; 3];
            let mut darkest = [255u8; 3];

            for y in y0.min(height)..y1.min(height) {
                for x in x0.min(width)..x1.min(width) {
                    let offset = ((y * width + x) * 4) as usize;
                    let r = pixels.get(offset).copied().unwrap_or(0);
                    let g = pixels.get(offset + 1).copied().unwrap_or(0);
                    let b = pixels.get(offset + 2).copied().unwrap_or(0);
                    if u16::from(r) + u16::from(g) + u16::from(b)
                        > u16::from(best[0]) + u16::from(best[1]) + u16::from(best[2])
                    {
                        best = [r, g, b];
                    }
                    if u16::from(r) + u16::from(g) + u16::from(b)
                        < u16::from(darkest[0]) + u16::from(darkest[1]) + u16::from(darkest[2])
                    {
                        darkest = [r, g, b];
                    }
                }
            }
            (best, darkest)
        }

        fn atlas_max_alpha(
            atlas: &[u8],
            atlas_width: u32,
            atlas_height: u32,
            rect: [f32; 4],
        ) -> u8 {
            let x0 = rect[0].floor().max(0.0) as u32;
            let y0 = rect[1].floor().max(0.0) as u32;
            let x1 = (rect[0] + rect[2]).ceil().max(0.0) as u32;
            let y1 = (rect[1] + rect[3]).ceil().max(0.0) as u32;

            let mut max_alpha = 0u8;
            for y in y0.min(atlas_height)..y1.min(atlas_height) {
                for x in x0.min(atlas_width)..x1.min(atlas_width) {
                    let offset = ((y * atlas_width + x) * 4 + 3) as usize;
                    max_alpha = max_alpha.max(atlas.get(offset).copied().unwrap_or(0));
                }
            }
            max_alpha
        }

        let Some((mut renderer, device, queue)) = pollster::block_on(
            test_support::create_test_renderer("widget_catalog_test_device"),
        ) else {
            return;
        };

        let (mut tree, typesetter) = tree_and_typesetter();
        tree.add(WidgetCatalog::new());
        tree.layout(SizeProposal::exact(1600.0, TEST_HEIGHT as f32));
        let mut frame = tree.render();
        let atlas = typesetter.bridge().borrow_mut().atlas_info();
        renderer.upload_atlas(atlas.width, atlas.height, &atlas.pixels);
        if atlas.glyphs_evicted {
            tree.invalidate_all_paints();
            frame = tree.render();
            let atlas2 = typesetter.bridge().borrow_mut().atlas_info();
            renderer.upload_atlas(atlas2.width, atlas2.height, &atlas2.pixels);
        }

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("widget_catalog_test_target"),
            size: wgpu::Extent3d {
                width: 1600,
                height: TEST_HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        renderer.render(
            &frame,
            &view,
            1.0,
            1600,
            TEST_HEIGHT,
            tree.theme().colors.surface_main.to_array(),
        );
        let pixels = test_support::read_texture_rgba(&device, &queue, &texture, 1600, TEST_HEIGHT);

        for label in ["Rust", "A1", "Standard"] {
            let id = tree
                .find_by_label(label)
                .unwrap_or_else(|| panic!("expected to find text node for {label:?}"));
            let bounds = tree.bounds(id);
            let matching_glyphs: Vec<_> = frame
                .glyphs
                .iter()
                .filter(|glyph| {
                    let [x, y, w, h] = glyph.screen;
                    let glyph_right = x + w;
                    let glyph_bottom = y + h;
                    glyph_right > bounds.x
                        && x < bounds.x + bounds.width
                        && glyph_bottom > bounds.y
                        && y < bounds.y + bounds.height
                })
                .map(|glyph| (glyph.screen, glyph.atlas, glyph.color))
                .collect();
            let (brightest, darkest) = pixel_extrema(&pixels, 1600, TEST_HEIGHT, bounds);
            let atlas_alpha: Vec<_> = matching_glyphs
                .iter()
                .map(|(_, atlas_rect, _)| {
                    atlas_max_alpha(&atlas.pixels, atlas.width, atlas.height, *atlas_rect)
                })
                .collect();
            assert!(
                brightest[0] > 170 && brightest[1] > 170 && brightest[2] > 170,
                "expected bright text pixels for {label:?} in {:?}, brightest was {:?}, darkest was {:?}, matching glyphs were {:?}, atlas alpha was {:?}, total glyphs {}, draw commands {}",
                bounds,
                brightest,
                darkest,
                matching_glyphs,
                atlas_alpha,
                frame.glyphs.len(),
                frame.draw_order.len()
            );
        }
    }
}
