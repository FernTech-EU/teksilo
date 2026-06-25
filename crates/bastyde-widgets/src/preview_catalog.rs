// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `WidgetCatalog` impls for `bastyde-widgets`.
//!
//! Gated behind the `preview` Cargo feature so production builds and
//! headless tests don't pull in the catalog data or the `inventory`
//! submission machinery.
//!
//! Per-widget impls are grouped by file section. Each one declares an
//! `id`, a `group`, a `display_name`, a `KnobSpec` of tweakable
//! properties, a `Vec<PreviewVariant>` of canonical scenarios, and a
//! `build` closure that constructs a fresh widget instance from the
//! `KnobValues` runtime view. The `register_widget_catalog!` macro
//! wires each impl into the global `inventory` so the previewer's
//! navigator surfaces it.
//!
//! Coverage in v1:
//! - Tier A (flat knob surface): Button, Checkbox, RadioButton, Toggle,
//!   Slider, ProgressBar, Badge, Link, ComboBox, SegmentedControl,
//!   IconWidget, Divider.
//! - Tier B (composites with fixture variants): Card, Panel, GroupBox,
//!   GroupHeader, IconButton, Snackbar, Breadcrumb, Toolbar,
//!   StatusBar, Accordion, RadioGroup, SplitButton.
//! - Tier C (data-driven / structural): ListView, TreeView, MenuList,
//!   ScrollArea, Splitter, TabWidget, ToolBox, Repeater.
//! - Skipped (modal / event-heavy / overlay-driven): Dialog,
//!   MessageBox, Popover, Wizard, MenuBar, MenuContext, TitleBar,
//!   ShortcutSettings, ImageWidget. These need additional context
//!   (intent registry, modal manager, raster resources) the catalog
//!   does not provide.

mod icons;

use bastyde_core::signal::Signal;
use bastyde_core::widget::Widget;
use bastyde_i18n::lit;
use bastyde_preview::{
    KnobOverrides, KnobSpec, KnobValues, PreviewVariant, SlottedChild, WidgetCatalog,
    WidgetCategory, register_widget_catalog_at,
};
use bastyde_core::styles::{CheckboxVariant, TextInputVariant};
use bastyde_tokens::{BorderRole, SurfaceRole, TextRole, TextStyleRole};

use crate::primitives::{
    Center, Expand, FixedSize, Grid, HStack, IconWidget, Padding, Spacer, TextWidget, TrackSize,
    VStack, ZStack,
};
// MaxSize / RectWidget are available for catalog impls in the section
// below; not every impl needs them, so the import carries
// `unused_imports` allow rather than gating each impl behind a feature.
#[allow(unused_imports)]
use crate::primitives::{MaxSize, RectWidget};
use crate::{
    Accordion, Avatar, AvatarPresence, AvatarShape, AvatarSize, Badge, Breadcrumb, BreadcrumbItem,
    Button, ButtonVariant, Card, Checkbox, ComboBox, GridSizing, GridView, GroupBox, GroupHeader,
    IconButton, IconButtonSize, Link, ListView, MenuItem, MenuList, Panel, ProgressBar,
    Orientation, PaneDescriptor, RadioButton, RadioGroup, ScrollArea, SegmentedControl, Slider,
    Snackbar, SplitButton, Splitter, SplitterModel, StandardListItem, StandardTreeItem, StatusBar,
    TabWidget, TextInput, Toggle, ToolBox, Toolbar, TreeView,
};

// ---------------------------------------------------------------------------
// Button
// ---------------------------------------------------------------------------

/// Maps a `variant` enum-knob index to `ButtonVariant` (declaration order).
fn button_variant(idx: usize) -> ButtonVariant {
    match idx {
        0 => ButtonVariant::Filled,
        1 => ButtonVariant::Tinted,
        2 => ButtonVariant::Outlined,
        4 => ButtonVariant::Ghost,
        5 => ButtonVariant::Link,
        6 => ButtonVariant::Destructive,
        _ => ButtonVariant::Plain,
    }
}

/// Maps a `variant` enum-knob index to `CheckboxVariant` (declaration order).
fn checkbox_variant(idx: usize) -> CheckboxVariant {
    match idx {
        1 => CheckboxVariant::Rounded,
        2 => CheckboxVariant::Circle,
        _ => CheckboxVariant::Square,
    }
}

/// Maps a `variant` enum-knob index to `TextInputVariant` (declaration order).
fn text_input_variant(idx: usize) -> TextInputVariant {
    match idx {
        1 => TextInputVariant::Filled,
        2 => TextInputVariant::Underline,
        3 => TextInputVariant::Bare,
        _ => TextInputVariant::Outlined,
    }
}

impl WidgetCatalog for Button {
    fn id() -> &'static str {
        "button"
    }
    fn group() -> &'static str {
        "Controls"
    }
    fn display_name() -> &'static str {
        "Button"
    }
    fn knobs() -> KnobSpec {
        KnobSpec::new()
            .text("label", "Label", "Click me")
            .ctor(0)
            .enum_(
                "variant",
                "Variant",
                "ButtonVariant",
                &["Filled", "Tinted", "Outlined", "Plain", "Ghost", "Link", "Destructive"],
                3,
            )
            .bool_("enabled", "Enabled", true)
            .opt_text("tooltip", "Tooltip", None)
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![
            PreviewVariant::defaults("default"),
            PreviewVariant::knobs(
                "primary",
                KnobOverrides::new().enum_("variant", 0).text("label", "Save"),
            ),
            PreviewVariant::knobs(
                "flat",
                KnobOverrides::new().enum_("variant", 4).text("label", "More…"),
            ),
            PreviewVariant::knobs("disabled", KnobOverrides::new().bool_("enabled", false)),
            PreviewVariant::knobs(
                "with-tooltip",
                KnobOverrides::new()
                    .text("label", "Help")
                    .opt_text("tooltip", Some("Open the help documentation")),
            ),
        ]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        let label = knobs.text("label").get();
        let variant = button_variant(knobs.enum_("variant").get());
        let enabled = knobs.bool_("enabled").get();
        let tooltip = knobs.opt_text("tooltip").get();
        let mut b = Button::new(lit!(label)).variant(variant).enabled(enabled);
        if let Some(t) = tooltip {
            b = b.tooltip(lit!(t));
        }
        Box::new(b)
    }
    fn icon() -> Option<Box<dyn Widget>> {
        Some(icons::button())
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/button.rs", Button);

// =========================================================================
// Layout primitives (designer-facing — ContainerA / Leaf, with runtime
// `build_with_children` so the designer's interpreted canvas can nest them)
// =========================================================================

impl WidgetCatalog for VStack {
    fn id() -> &'static str {
        "vstack"
    }
    fn group() -> &'static str {
        "Layout"
    }
    fn display_name() -> &'static str {
        "VStack"
    }
    fn knobs() -> KnobSpec {
        KnobSpec::new().f32_("spacing", "Spacing", 8.0, 0.0, 48.0)
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![PreviewVariant::defaults("default")]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        Box::new(
            VStack::new()
                .spacing(knobs.f32_("spacing").get())
                .child(sample_text("Item 1"))
                .child(sample_text("Item 2"))
                .child(sample_text("Item 3")),
        )
    }
    fn icon() -> Option<Box<dyn Widget>> {
        Some(icons::vstack())
    }
    fn category() -> WidgetCategory {
        WidgetCategory::ContainerA
    }
    fn build_with_children(
        _variant: &str,
        knobs: &KnobValues,
        children: Vec<SlottedChild>,
    ) -> Box<dyn Widget> {
        let mut s = VStack::new().spacing(knobs.f32_("spacing").get());
        for c in children {
            s = s.add_child(c.id);
        }
        Box::new(s)
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/primitives/vstack.rs", VStack);

impl WidgetCatalog for HStack {
    fn id() -> &'static str {
        "hstack"
    }
    fn group() -> &'static str {
        "Layout"
    }
    fn display_name() -> &'static str {
        "HStack"
    }
    fn knobs() -> KnobSpec {
        KnobSpec::new().f32_("spacing", "Spacing", 8.0, 0.0, 48.0)
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![PreviewVariant::defaults("default")]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        Box::new(
            HStack::new()
                .spacing(knobs.f32_("spacing").get())
                .child(sample_text("A"))
                .child(sample_text("B"))
                .child(sample_text("C")),
        )
    }
    fn icon() -> Option<Box<dyn Widget>> {
        Some(icons::hstack())
    }
    fn category() -> WidgetCategory {
        WidgetCategory::ContainerA
    }
    fn build_with_children(
        _variant: &str,
        knobs: &KnobValues,
        children: Vec<SlottedChild>,
    ) -> Box<dyn Widget> {
        let mut s = HStack::new().spacing(knobs.f32_("spacing").get());
        for c in children {
            s = s.add_child(c.id);
        }
        Box::new(s)
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/primitives/hstack.rs", HStack);

impl WidgetCatalog for ZStack {
    fn id() -> &'static str {
        "zstack"
    }
    fn group() -> &'static str {
        "Layout"
    }
    fn display_name() -> &'static str {
        "ZStack"
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![PreviewVariant::defaults("default")]
    }
    fn build(_variant: &str, _knobs: &KnobValues) -> Box<dyn Widget> {
        Box::new(
            ZStack::new()
                .child(RectWidget::new().background(SurfaceRole::Raised))
                .child(Center::new().child(sample_text("ZStack"))),
        )
    }
    fn icon() -> Option<Box<dyn Widget>> {
        Some(icons::zstack())
    }
    fn category() -> WidgetCategory {
        WidgetCategory::ContainerA
    }
    fn build_with_children(
        _variant: &str,
        _knobs: &KnobValues,
        children: Vec<SlottedChild>,
    ) -> Box<dyn Widget> {
        let mut s = ZStack::new();
        for c in children {
            s = s.add_child(c.id);
        }
        Box::new(s)
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/primitives/zstack.rs", ZStack);

impl WidgetCatalog for Grid {
    fn id() -> &'static str {
        "grid"
    }
    fn group() -> &'static str {
        "Layout"
    }
    fn display_name() -> &'static str {
        "Grid"
    }
    fn knobs() -> KnobSpec {
        KnobSpec::new()
            .choice("columns", "Columns", &["1", "2", "3", "4"], 1)
            .f32_("gap", "Gap", 8.0, 0.0, 32.0)
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![PreviewVariant::defaults("default")]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        let cols = knobs.choice("columns").get() + 1;
        let gap = knobs.f32_("gap").get();
        let mut g = Grid::new()
            .columns(vec![TrackSize::Fractional(1.0); cols])
            .column_gap(gap)
            .row_gap(gap);
        for i in 1..=6 {
            g = g.child(sample_text(&format!("{i}")));
        }
        Box::new(g)
    }
    fn icon() -> Option<Box<dyn Widget>> {
        Some(icons::grid())
    }
    fn category() -> WidgetCategory {
        WidgetCategory::ContainerA
    }
    fn build_with_children(
        _variant: &str,
        knobs: &KnobValues,
        children: Vec<SlottedChild>,
    ) -> Box<dyn Widget> {
        let cols = knobs.choice("columns").get() + 1;
        let gap = knobs.f32_("gap").get();
        let mut g = Grid::new()
            .columns(vec![TrackSize::Fractional(1.0); cols])
            .column_gap(gap)
            .row_gap(gap);
        for c in children {
            g = g.add_child(c.id);
        }
        Box::new(g)
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/primitives/grid.rs", Grid);

impl WidgetCatalog for Padding {
    fn id() -> &'static str {
        "padding"
    }
    fn group() -> &'static str {
        "Layout"
    }
    fn display_name() -> &'static str {
        "Padding"
    }
    fn knobs() -> KnobSpec {
        KnobSpec::new().f32_("amount", "Amount", 16.0, 0.0, 48.0)
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![PreviewVariant::defaults("default")]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        Box::new(Padding::uniform(knobs.f32_("amount").get()).child(sample_text("Padded content")))
    }
    fn icon() -> Option<Box<dyn Widget>> {
        Some(icons::padding())
    }
    fn category() -> WidgetCategory {
        WidgetCategory::ContainerA
    }
    fn build_with_children(
        _variant: &str,
        knobs: &KnobValues,
        children: Vec<SlottedChild>,
    ) -> Box<dyn Widget> {
        let mut p = Padding::uniform(knobs.f32_("amount").get());
        if let Some(c) = children.into_iter().next() {
            p = p.child_id(c.id);
        }
        Box::new(p)
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/primitives/padding.rs", Padding);

impl WidgetCatalog for Expand {
    fn id() -> &'static str {
        "expand"
    }
    fn group() -> &'static str {
        "Layout"
    }
    fn display_name() -> &'static str {
        "Expand"
    }
    fn knobs() -> KnobSpec {
        KnobSpec::new().f32_("flex", "Flex", 1.0, 0.0, 4.0)
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![PreviewVariant::defaults("default")]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        Box::new(
            FixedSize::new()
                .bind_width(220.0_f32)
                .bind_height(60.0_f32)
                .child(
                    Expand::new()
                        .flex(knobs.f32_("flex").get())
                        .child(RectWidget::new().background(SurfaceRole::AccentSubtle)),
                ),
        )
    }
    fn icon() -> Option<Box<dyn Widget>> {
        Some(icons::expand())
    }
    fn category() -> WidgetCategory {
        WidgetCategory::ContainerA
    }
    fn build_with_children(
        _variant: &str,
        knobs: &KnobValues,
        children: Vec<SlottedChild>,
    ) -> Box<dyn Widget> {
        let mut e = Expand::new().flex(knobs.f32_("flex").get());
        if let Some(c) = children.into_iter().next() {
            e = e.child_id(c.id);
        }
        Box::new(e)
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/primitives/expand.rs", Expand);

impl WidgetCatalog for Center {
    fn id() -> &'static str {
        "center"
    }
    fn group() -> &'static str {
        "Layout"
    }
    fn display_name() -> &'static str {
        "Center"
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![PreviewVariant::defaults("default")]
    }
    fn build(_variant: &str, _knobs: &KnobValues) -> Box<dyn Widget> {
        Box::new(
            FixedSize::new()
                .bind_width(200.0_f32)
                .bind_height(80.0_f32)
                .child(Center::new().child(sample_text("Centered"))),
        )
    }
    fn icon() -> Option<Box<dyn Widget>> {
        Some(icons::center())
    }
    fn category() -> WidgetCategory {
        WidgetCategory::ContainerA
    }
    fn build_with_children(
        _variant: &str,
        _knobs: &KnobValues,
        children: Vec<SlottedChild>,
    ) -> Box<dyn Widget> {
        let mut c0 = Center::new();
        if let Some(c) = children.into_iter().next() {
            c0 = c0.child_id(c.id);
        }
        Box::new(c0)
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/primitives/center.rs", Center);

impl WidgetCatalog for Spacer {
    fn id() -> &'static str {
        "spacer"
    }
    fn group() -> &'static str {
        "Layout"
    }
    fn display_name() -> &'static str {
        "Spacer"
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![PreviewVariant::defaults("default")]
    }
    fn build(_variant: &str, _knobs: &KnobValues) -> Box<dyn Widget> {
        Box::new(
            HStack::new()
                .child(sample_text("L"))
                .child(Spacer::new())
                .child(sample_text("R")),
        )
    }
    fn icon() -> Option<Box<dyn Widget>> {
        Some(icons::spacer())
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/primitives/spacer.rs", Spacer);

impl WidgetCatalog for TextWidget {
    fn id() -> &'static str {
        "text_widget"
    }
    fn group() -> &'static str {
        "Display"
    }
    fn display_name() -> &'static str {
        // Match the actual widget type name (`TextWidget`), like every other
        // catalog entry (`Button`, `VStack`, …). Tools that map source widget
        // names to catalog entries (e.g. bastyde-designer's interpreter) rely
        // on this; "Text" was an inconsistent shorthand.
        "TextWidget"
    }
    fn knobs() -> KnobSpec {
        KnobSpec::new()
            .text("text", "Text", "Label")
            .ctor(0)
            .text_role("color", "Color", TextRole::Primary)
            .text_style("style", "Style", TextStyleRole::Body)
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![
            PreviewVariant::defaults("default"),
            PreviewVariant::knobs("secondary", KnobOverrides::new().text_role("color", TextRole::Secondary)),
            PreviewVariant::knobs("bold", KnobOverrides::new().text_style("style", TextStyleRole::BodyBold)),
        ]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        Box::new(
            TextWidget::new(lit!(knobs.text("text").get()))
                .style(knobs.text_style("style").get())
                .color(knobs.text_role("color").get()),
        )
    }
    fn icon() -> Option<Box<dyn Widget>> {
        Some(icons::text_widget())
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/primitives/text_widget.rs", TextWidget);

impl WidgetCatalog for TextInput {
    fn id() -> &'static str {
        "text_input"
    }
    fn group() -> &'static str {
        "Controls"
    }
    fn display_name() -> &'static str {
        "TextInput"
    }
    fn knobs() -> KnobSpec {
        KnobSpec::new()
            .text("value", "Value", "")
            .ctor(0)
            .enum_(
                "variant",
                "Variant",
                "TextInputVariant",
                &["Outlined", "Filled", "Underline", "Bare"],
                0,
            )
            .text("placeholder", "Placeholder", "Type here…")
            .bool_("enabled", "Enabled", true)
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![PreviewVariant::defaults("default")]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        Box::new(
            FixedSize::new().bind_width(220.0_f32).child(
                TextInput::new(knobs.text("value"))
                    .variant(text_input_variant(knobs.enum_("variant").get()))
                    .placeholder(lit!(knobs.text("placeholder").get()))
                    .enabled(knobs.bool_("enabled").get()),
            ),
        )
    }
    fn icon() -> Option<Box<dyn Widget>> {
        Some(icons::text_input())
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/text_input.rs", TextInput);

// ---------------------------------------------------------------------------
// Checkbox
// ---------------------------------------------------------------------------

impl WidgetCatalog for Checkbox {
    fn id() -> &'static str {
        "checkbox"
    }
    fn group() -> &'static str {
        "Controls"
    }
    fn display_name() -> &'static str {
        "Checkbox"
    }
    fn knobs() -> KnobSpec {
        KnobSpec::new()
            .bool_("checked", "Checked", false)
            .ctor(0)
            .enum_("variant", "Variant", "CheckboxVariant", &["Square", "Rounded", "Circle"], 0)
            .text("label", "Label", "Enable feature")
            .bool_("enabled", "Enabled", true)
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![
            PreviewVariant::defaults("unchecked"),
            PreviewVariant::knobs("checked", KnobOverrides::new().bool_("checked", true)),
            PreviewVariant::knobs("disabled", KnobOverrides::new().bool_("enabled", false)),
        ]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        let label = knobs.text("label").get();
        let checked = knobs.bool_("checked");
        let enabled = knobs.bool_("enabled").get();
        Box::new(
            Checkbox::new(checked)
                .variant(checkbox_variant(knobs.enum_("variant").get()))
                .label(lit!(label))
                .enabled(enabled),
        )
    }
    fn icon() -> Option<Box<dyn Widget>> {
        Some(icons::checkbox())
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/checkbox.rs", Checkbox);

// ---------------------------------------------------------------------------
// RadioButton
// ---------------------------------------------------------------------------

impl WidgetCatalog for RadioButton {
    fn id() -> &'static str {
        "radio_button"
    }
    fn group() -> &'static str {
        "Controls"
    }
    fn display_name() -> &'static str {
        "RadioButton"
    }
    fn knobs() -> KnobSpec {
        // The knob's `selected` choice is the same `Signal<usize>` the
        // RadioButton reads as its group selector. Index 0 = "Yes" =
        // radio (value 0) selected; 1 = "No" = group on a different
        // value so radio 0 is unselected. Using the knob's signal
        // directly avoids needing a bridge.
        KnobSpec::new()
            .choice("selected", "Selected", &["Yes", "No"], 1)
            .opt_text("label", "Label", Some("Option"))
            .bool_("enabled", "Enabled", true)
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![
            PreviewVariant::defaults("unselected"),
            PreviewVariant::knobs("selected", KnobOverrides::new().choice("selected", 0)),
            PreviewVariant::knobs("disabled", KnobOverrides::new().bool_("enabled", false)),
            PreviewVariant::knobs("no-label", KnobOverrides::new().opt_text("label", None)),
        ]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        let label = knobs.opt_text("label").get();
        let enabled = knobs.bool_("enabled").get();
        let mut r = RadioButton::new(0, knobs.choice("selected")).enabled(enabled);
        if let Some(label) = label {
            r = r.label(lit!(label));
        }
        Box::new(r)
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/radio_button.rs", RadioButton);

// ---------------------------------------------------------------------------
// Toggle
// ---------------------------------------------------------------------------

impl WidgetCatalog for Toggle {
    fn id() -> &'static str {
        "toggle"
    }
    fn group() -> &'static str {
        "Controls"
    }
    fn display_name() -> &'static str {
        "Toggle"
    }
    fn knobs() -> KnobSpec {
        // Toggle's `accessibility()` insists on a non-empty label
        // (debug_assert!) — required for screen readers. Keep the
        // knob mandatory rather than optional.
        KnobSpec::new()
            .bool_("on", "On", false)
            .text("label", "Label", "Enable feature")
            .bool_("enabled", "Enabled", true)
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![
            PreviewVariant::defaults("off"),
            PreviewVariant::knobs("on", KnobOverrides::new().bool_("on", true)),
            PreviewVariant::knobs("disabled", KnobOverrides::new().bool_("enabled", false)),
        ]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        let on = knobs.bool_("on");
        let label = knobs.text("label").get();
        let enabled = knobs.bool_("enabled").get();
        Box::new(Toggle::new(on).label(lit!(label)).enabled(enabled))
    }
    fn icon() -> Option<Box<dyn Widget>> {
        Some(icons::toggle())
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/toggle.rs", Toggle);

// ---------------------------------------------------------------------------
// Slider
// ---------------------------------------------------------------------------

impl WidgetCatalog for Slider {
    fn id() -> &'static str {
        "slider"
    }
    fn group() -> &'static str {
        "Controls"
    }
    fn display_name() -> &'static str {
        "Slider"
    }
    fn knobs() -> KnobSpec {
        KnobSpec::new()
            .f32_("value", "Value", 0.5, 0.0, 1.0)
            .ctor(0)
            .enum_("orientation", "Orientation", "Orientation", &["Horizontal", "Vertical"], 0)
            .f32_step("step", "Step (0 = continuous)", 0.0, 0.0, 0.5, 0.05)
            .bool_("enabled", "Enabled", true)
            .opt_text("label", "Label", None)
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![
            PreviewVariant::defaults("default"),
            PreviewVariant::knobs("min", KnobOverrides::new().f32_("value", 0.0)),
            PreviewVariant::knobs("max", KnobOverrides::new().f32_("value", 1.0)),
            PreviewVariant::knobs(
                "stepped",
                KnobOverrides::new().f32_("value", 0.5).f32_("step", 0.1),
            ),
            PreviewVariant::knobs("vertical", KnobOverrides::new().enum_("orientation", 1)),
            PreviewVariant::knobs("disabled", KnobOverrides::new().bool_("enabled", false)),
            PreviewVariant::knobs(
                "with-label",
                KnobOverrides::new().opt_text("label", Some("Volume")),
            ),
        ]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        use bastyde_tokens::Orientation;
        let orient = match knobs.enum_("orientation").get() {
            1 => Orientation::Vertical,
            _ => Orientation::Horizontal,
        };
        let step = knobs.f32_("step").get();
        let enabled = knobs.bool_("enabled").get();
        let label = knobs.opt_text("label").get();
        let mut s = Slider::new(knobs.f32_("value"), 0.0, 1.0)
            .orientation(orient)
            .enabled(enabled);
        if step > 0.0 {
            s = s.step(step);
        }
        if let Some(label) = label {
            s = s.label(lit!(label));
        }
        // Vertical sliders need a fixed height to be visible.
        let widget: Box<dyn Widget> = if matches!(orient, Orientation::Vertical) {
            Box::new(FixedSize::new().bind_height(160.0_f32).child(s))
        } else {
            Box::new(s)
        };
        widget
    }
    fn icon() -> Option<Box<dyn Widget>> {
        Some(icons::slider())
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/slider.rs", Slider);

// ---------------------------------------------------------------------------
// ProgressBar
// ---------------------------------------------------------------------------

impl WidgetCatalog for ProgressBar {
    fn id() -> &'static str {
        "progress_bar"
    }
    fn group() -> &'static str {
        "Controls"
    }
    fn display_name() -> &'static str {
        "ProgressBar"
    }
    fn knobs() -> KnobSpec {
        KnobSpec::new()
            .f32_("value", "Value", 0.4, 0.0, 1.0)
            .bool_("indeterminate", "Indeterminate", false)
            .choice("orientation", "Orientation", &["Horizontal", "Vertical"], 0)
            .opt_text("label", "Label", None)
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![
            PreviewVariant::defaults("determinate"),
            PreviewVariant::knobs("empty", KnobOverrides::new().f32_("value", 0.0)),
            PreviewVariant::knobs("full", KnobOverrides::new().f32_("value", 1.0)),
            PreviewVariant::knobs(
                "indeterminate",
                KnobOverrides::new().bool_("indeterminate", true),
            ),
            PreviewVariant::knobs("vertical", KnobOverrides::new().choice("orientation", 1)),
            PreviewVariant::knobs(
                "with-label",
                KnobOverrides::new()
                    .f32_("value", 0.7)
                    .opt_text("label", Some("Uploading…")),
            ),
        ]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        use bastyde_tokens::Orientation;
        let indeterminate = knobs.bool_("indeterminate").get();
        let orient = match knobs.choice("orientation").get() {
            1 => Orientation::Vertical,
            _ => Orientation::Horizontal,
        };
        let label = knobs.opt_text("label").get();
        let mut bar = if indeterminate {
            ProgressBar::indeterminate()
        } else {
            ProgressBar::new(knobs.f32_("value").get())
        };
        bar = bar.orientation(orient);
        if let Some(label) = label {
            bar = bar.label(lit!(label));
        }
        let widget: Box<dyn Widget> = if matches!(orient, Orientation::Vertical) {
            Box::new(FixedSize::new().bind_height(160.0_f32).child(bar))
        } else {
            Box::new(bar)
        };
        widget
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/progress_bar.rs", ProgressBar);

// ---------------------------------------------------------------------------
// Badge
// ---------------------------------------------------------------------------

impl WidgetCatalog for Badge {
    fn id() -> &'static str {
        "badge"
    }
    fn group() -> &'static str {
        "Controls"
    }
    fn display_name() -> &'static str {
        "Badge"
    }
    fn knobs() -> KnobSpec {
        KnobSpec::new()
            .text("label", "Label", "NEW")
            .surface_role("background", "Background", SurfaceRole::Accent)
            .text_role("text_role", "Text colour", TextRole::OnAccent)
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![
            PreviewVariant::defaults("accent"),
            PreviewVariant::knobs("long", KnobOverrides::new().text("label", "EXPERIMENTAL")),
            PreviewVariant::knobs("short", KnobOverrides::new().text("label", "•")),
            PreviewVariant::knobs(
                "success",
                KnobOverrides::new()
                    .text("label", "OK")
                    .surface_role("background", SurfaceRole::StatusSuccess)
                    .text_role("text_role", TextRole::Success),
            ),
            PreviewVariant::knobs(
                "warning",
                KnobOverrides::new()
                    .text("label", "BETA")
                    .surface_role("background", SurfaceRole::StatusWarning)
                    .text_role("text_role", TextRole::Warning),
            ),
            PreviewVariant::knobs(
                "error",
                KnobOverrides::new()
                    .text("label", "ERR")
                    .surface_role("background", SurfaceRole::StatusError)
                    .text_role("text_role", TextRole::Error),
            ),
        ]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        let bg = knobs.surface_role("background");
        let fg = knobs.text_role("text_role");
        Box::new(
            Badge::new(lit!(knobs.text("label").get()))
                .background(bg)
                .text_role(fg),
        )
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/badge.rs", Badge);

// ---------------------------------------------------------------------------
// Avatar
// ---------------------------------------------------------------------------

impl WidgetCatalog for Avatar {
    fn id() -> &'static str {
        "avatar"
    }
    fn group() -> &'static str {
        "Controls"
    }
    fn display_name() -> &'static str {
        "Avatar"
    }
    fn knobs() -> KnobSpec {
        KnobSpec::new()
            // Free-form name; initials are derived (`Jane Doe` → `JD`).
            .text("name", "Name", "Jane Doe")
            // Optional override for the displayed initials when the
            // user wants something other than the auto-derived form.
            .opt_text("initials_override", "Initials override", None)
            .choice(
                "size",
                "Size",
                &["Small (24)", "Medium (32)", "Large (48)", "XLarge (64)"],
                1,
            )
            .choice("shape", "Shape", &["Circle", "RoundedSquare", "Square"], 0)
            .choice(
                "presence",
                "Presence",
                &["None", "Online", "Offline", "Away", "Busy"],
                0,
            )
            .choice(
                "presence_corner",
                "Presence corner",
                &[
                    "BottomTrailing",
                    "BottomLeading",
                    "TopTrailing",
                    "TopLeading",
                ],
                0,
            )
            .bool_("border", "Show ring", false)
            .bool_("clickable", "Clickable", false)
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![
            PreviewVariant::defaults("default"),
            PreviewVariant::knobs(
                "small",
                KnobOverrides::new().choice("size", 0).text("name", "AB"),
            ),
            PreviewVariant::knobs(
                "large",
                KnobOverrides::new()
                    .choice("size", 2)
                    .text("name", "Sherlock Holmes"),
            ),
            PreviewVariant::knobs(
                "xlarge",
                KnobOverrides::new()
                    .choice("size", 3)
                    .text("name", "Marie Curie"),
            ),
            PreviewVariant::knobs(
                "rounded-square",
                KnobOverrides::new()
                    .choice("shape", 1)
                    .text("name", "Project X"),
            ),
            PreviewVariant::knobs("online", KnobOverrides::new().choice("presence", 1)),
            PreviewVariant::knobs("away", KnobOverrides::new().choice("presence", 3)),
            PreviewVariant::knobs("busy", KnobOverrides::new().choice("presence", 4)),
            PreviewVariant::knobs("with-ring", KnobOverrides::new().bool_("border", true)),
            PreviewVariant::knobs("clickable", KnobOverrides::new().bool_("clickable", true)),
            PreviewVariant::knobs("single-letter", KnobOverrides::new().text("name", "Cher")),
            PreviewVariant::knobs(
                "email-derived",
                KnobOverrides::new().text("name", "jane.doe@example.com"),
            ),
        ]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        let name = knobs.text("name").get();
        let size = match knobs.choice("size").get() {
            0 => AvatarSize::Small,
            2 => AvatarSize::Large,
            3 => AvatarSize::XLarge,
            _ => AvatarSize::Medium,
        };
        let shape = match knobs.choice("shape").get() {
            1 => AvatarShape::RoundedSquare,
            2 => AvatarShape::Square,
            _ => AvatarShape::Circle,
        };
        let presence = match knobs.choice("presence").get() {
            1 => Some(AvatarPresence::Online),
            2 => Some(AvatarPresence::Offline),
            3 => Some(AvatarPresence::Away),
            4 => Some(AvatarPresence::Busy),
            _ => None,
        };
        let presence_corner = match knobs.choice("presence_corner").get() {
            1 => crate::AvatarCorner::BottomLeading,
            2 => crate::AvatarCorner::TopTrailing,
            3 => crate::AvatarCorner::TopLeading,
            _ => crate::AvatarCorner::BottomTrailing,
        };
        let border = knobs.bool_("border").get();
        let clickable = knobs.bool_("clickable").get();
        let initials_override = knobs.opt_text("initials_override").get();

        let mut a = if let Some(initials) = initials_override {
            Avatar::with_initials(lit!(&initials))
        } else {
            Avatar::with_name(lit!(&name))
        }
        .size(size)
        .shape(shape)
        .presence_corner(presence_corner);

        if let Some(p) = presence {
            a = a.presence(p);
        }
        if border {
            a = a.border(2.0);
        }
        if clickable {
            a = a.label(lit!("Open user menu")).on_activate_fn(|_ctx| {});
        }
        Box::new(a)
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/avatar.rs", Avatar);

// ---------------------------------------------------------------------------
// Link
// ---------------------------------------------------------------------------

impl WidgetCatalog for Link {
    fn id() -> &'static str {
        "link"
    }
    fn group() -> &'static str {
        "Controls"
    }
    fn display_name() -> &'static str {
        "Link"
    }
    fn knobs() -> KnobSpec {
        KnobSpec::new()
            .text("label", "Label", "Read more")
            .bool_("enabled", "Enabled", true)
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![
            PreviewVariant::defaults("default"),
            PreviewVariant::knobs("disabled", KnobOverrides::new().bool_("enabled", false)),
        ]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        Box::new(Link::new(lit!(knobs.text("label").get())).enabled(knobs.bool_("enabled").get()))
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/link.rs", Link);

// ---------------------------------------------------------------------------
// SegmentedControl
// ---------------------------------------------------------------------------

impl WidgetCatalog for SegmentedControl {
    fn id() -> &'static str {
        "segmented_control"
    }
    fn group() -> &'static str {
        "Controls"
    }
    fn display_name() -> &'static str {
        "SegmentedControl"
    }
    fn knobs() -> KnobSpec {
        KnobSpec::new()
            .choice("selected", "Selected", &["Day", "Week", "Month"], 0)
            .bool_("enabled", "Enabled", true)
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![
            PreviewVariant::defaults("default"),
            PreviewVariant::knobs("middle", KnobOverrides::new().choice("selected", 1)),
            PreviewVariant::knobs("disabled", KnobOverrides::new().bool_("enabled", false)),
        ]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        Box::new(
            SegmentedControl::new(knobs.choice("selected"))
                .segments([lit!("Day"), lit!("Week"), lit!("Month")])
                .enabled(knobs.bool_("enabled").get()),
        )
    }
}
register_widget_catalog_at!(
    "crates/bastyde-widgets/src/segmented_control.rs",
    SegmentedControl
);

// ---------------------------------------------------------------------------
// ComboBox
// ---------------------------------------------------------------------------

impl WidgetCatalog for ComboBox<String> {
    fn id() -> &'static str {
        "combo_box"
    }
    fn group() -> &'static str {
        "Controls"
    }
    fn display_name() -> &'static str {
        "ComboBox"
    }
    fn knobs() -> KnobSpec {
        KnobSpec::new()
            .opt_text("selected", "Selected", Some("Apple"))
            .text("placeholder", "Placeholder", "Select a fruit…")
            .opt_text("label", "A11y label", Some("Fruit"))
            .bool_("enabled", "Enabled", true)
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![
            PreviewVariant::defaults("with-selection"),
            PreviewVariant::knobs("empty", KnobOverrides::new().opt_text("selected", None)),
            PreviewVariant::knobs("disabled", KnobOverrides::new().bool_("enabled", false)),
        ]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        let items = vec![
            "Apple".to_string(),
            "Banana".to_string(),
            "Cherry".to_string(),
            "Date".to_string(),
            "Elderberry".to_string(),
        ];
        // ComboBox<String> takes `Signal<Option<String>>` — the knob's
        // `opt_text` accessor returns exactly that, so we use it
        // directly. User clicks on items mutate the knob signal,
        // which the inspector's editor sees and re-renders.
        let placeholder = knobs.text("placeholder").get();
        let enabled = knobs.bool_("enabled").get();
        let mut cb = ComboBox::new(items, knobs.opt_text("selected"))
            .placeholder(lit!(placeholder))
            .enabled(enabled);
        if let Some(label) = knobs.opt_text("label").get() {
            cb = cb.label(lit!(label));
        }
        Box::new(cb)
    }
    fn icon() -> Option<Box<dyn Widget>> {
        Some(icons::combo_box())
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/combo_box.rs", ComboBox<String>);

// ---------------------------------------------------------------------------
// Divider
// ---------------------------------------------------------------------------

impl WidgetCatalog for crate::primitives::Divider {
    fn id() -> &'static str {
        "divider"
    }
    fn group() -> &'static str {
        "Primitives"
    }
    fn display_name() -> &'static str {
        "Divider"
    }
    fn knobs() -> KnobSpec {
        KnobSpec::new()
            .choice("orientation", "Orientation", &["Horizontal", "Vertical"], 0)
            .f32_("thickness", "Thickness", 1.0, 0.5, 6.0)
            .border_role("color", "Colour", BorderRole::Divider)
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![
            PreviewVariant::defaults("horizontal"),
            PreviewVariant::knobs("vertical", KnobOverrides::new().choice("orientation", 1)),
            PreviewVariant::knobs("thick", KnobOverrides::new().f32_("thickness", 4.0)),
            PreviewVariant::knobs(
                "strong",
                KnobOverrides::new().border_role("color", BorderRole::DividerStrong),
            ),
            PreviewVariant::knobs(
                "accent",
                KnobOverrides::new().border_role("color", BorderRole::Accent),
            ),
        ]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        let orient = knobs.choice("orientation").get();
        let thickness = knobs.f32_("thickness").get();
        let role = knobs.border_role("color").get();
        let mut d = if orient == 1 {
            crate::primitives::Divider::vertical()
        } else {
            crate::primitives::Divider::horizontal()
        };
        d = d.thickness(thickness).color(role);
        // Wrap a vertical divider in a fixed-height block so it has
        // something to draw across; horizontal in a fixed width.
        let wrapped: Box<dyn Widget> = if orient == 1 {
            Box::new(
                crate::primitives::FixedSize::new()
                    .bind_height(120.0_f32)
                    .child(d),
            )
        } else {
            Box::new(
                crate::primitives::FixedSize::new()
                    .bind_width(220.0_f32)
                    .child(d),
            )
        };
        wrapped
    }
}
register_widget_catalog_at!(
    "crates/bastyde-widgets/src/primitives/divider.rs",
    crate::primitives::Divider
);

// ---------------------------------------------------------------------------
// IconWidget
// ---------------------------------------------------------------------------

impl WidgetCatalog for IconWidget {
    fn id() -> &'static str {
        "icon_widget"
    }
    fn group() -> &'static str {
        "Primitives"
    }
    fn display_name() -> &'static str {
        "IconWidget"
    }
    fn knobs() -> KnobSpec {
        KnobSpec::new()
            .f32_("size", "Size (dp)", 24.0, 12.0, 96.0)
            .choice("shape", "Shape", &["Square", "Circle", "Triangle"], 0)
            .text_role("color", "Colour", TextRole::Primary)
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![
            PreviewVariant::defaults("square"),
            PreviewVariant::knobs("circle", KnobOverrides::new().choice("shape", 1)),
            PreviewVariant::knobs("triangle", KnobOverrides::new().choice("shape", 2)),
            PreviewVariant::knobs("large", KnobOverrides::new().f32_("size", 64.0)),
            PreviewVariant::knobs(
                "accent",
                KnobOverrides::new().text_role("color", TextRole::Accent),
            ),
            PreviewVariant::knobs(
                "error",
                KnobOverrides::new().text_role("color", TextRole::Error),
            ),
        ]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        use bastyde_canvas::{Path, Point};
        let size = knobs.f32_("size").get();
        let shape = knobs.choice("shape").get();
        let mut path = Path::new();
        match shape {
            0 => {
                path.move_to(Point::new(2.0, 2.0));
                path.line_to(Point::new(size - 2.0, 2.0));
                path.line_to(Point::new(size - 2.0, size - 2.0));
                path.line_to(Point::new(2.0, size - 2.0));
                path.close();
            }
            1 => {
                let r = (size / 2.0) - 2.0;
                let c = Point::new(size / 2.0, size / 2.0);
                // Approximate a circle with cubic-bezier arcs (4 quadrants).
                let k = 0.552_284_8 * r;
                path.move_to(Point::new(c.x + r, c.y));
                path.cubic_to(
                    Point::new(c.x + r, c.y + k),
                    Point::new(c.x + k, c.y + r),
                    Point::new(c.x, c.y + r),
                );
                path.cubic_to(
                    Point::new(c.x - k, c.y + r),
                    Point::new(c.x - r, c.y + k),
                    Point::new(c.x - r, c.y),
                );
                path.cubic_to(
                    Point::new(c.x - r, c.y - k),
                    Point::new(c.x - k, c.y - r),
                    Point::new(c.x, c.y - r),
                );
                path.cubic_to(
                    Point::new(c.x + k, c.y - r),
                    Point::new(c.x + r, c.y - k),
                    Point::new(c.x + r, c.y),
                );
                path.close();
            }
            _ => {
                path.move_to(Point::new(size / 2.0, 2.0));
                path.line_to(Point::new(size - 2.0, size - 2.0));
                path.line_to(Point::new(2.0, size - 2.0));
                path.close();
            }
        }
        let role = knobs.text_role("color").get();
        Box::new(IconWidget::from_path(path, size).color(role))
    }
}
register_widget_catalog_at!(
    "crates/bastyde-widgets/src/primitives/icon_widget.rs",
    IconWidget
);

// =========================================================================
// Tier B — composites with fixture variants
// =========================================================================
//
// These widgets don't have flat knob surfaces; their interesting states
// are structural ("with header + footer", "expanded with content",
// "two segments"). Each `variants()` is a hand-authored list of
// scenarios, and `knobs()` is left empty.

// ---------------------------------------------------------------------------
// Card
// ---------------------------------------------------------------------------

fn sample_text(label: &str) -> TextWidget {
    TextWidget::new(lit!(label))
        .style(TextStyleRole::Body)
        .color(TextRole::Primary)
}

impl WidgetCatalog for Card {
    fn id() -> &'static str {
        "card"
    }
    fn group() -> &'static str {
        "Containers"
    }
    fn display_name() -> &'static str {
        "Card"
    }
    fn knobs() -> KnobSpec {
        KnobSpec::new()
            .text("title", "Title", "Card title")
            .text(
                "body",
                "Body",
                "Card body text. Cards group related controls into a labelled rectangular region.",
            )
            .bool_("show_header", "Show header", true)
            .bool_("show_footer", "Show footer", false)
            .surface_role("background", "Background", SurfaceRole::Main)
            .f32_("corner_radius", "Corner radius", 8.0, 0.0, 32.0)
            .f32_("padding", "Padding", 16.0, 0.0, 48.0)
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![
            PreviewVariant::defaults("default"),
            PreviewVariant::knobs(
                "with-footer",
                KnobOverrides::new()
                    .text("title", "Settings")
                    .text("body", "Configure the application settings here.")
                    .bool_("show_footer", true),
            ),
            PreviewVariant::knobs(
                "headerless",
                KnobOverrides::new().bool_("show_header", false),
            ),
            PreviewVariant::knobs(
                "raised",
                KnobOverrides::new().surface_role("background", SurfaceRole::Raised),
            ),
        ]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        let mut card = Card::new()
            .background(knobs.surface_role("background"))
            .corner_radius(knobs.f32_("corner_radius").get())
            .padding(knobs.f32_("padding").get())
            .content(
                TextWidget::new(lit!(knobs.text("body").get()))
                    .style(TextStyleRole::Body)
                    .color(TextRole::Primary),
            );
        if knobs.bool_("show_header").get() {
            card = card.header(
                TextWidget::new(lit!(knobs.text("title").get()))
                    .style(TextStyleRole::BodyBold)
                    .color(TextRole::Primary),
            );
        }
        if knobs.bool_("show_footer").get() {
            card = card.footer(
                HStack::new()
                    .spacing(8.0)
                    .child(Spacer::new())
                    .child(Button::new(lit!("Cancel")).variant(ButtonVariant::Plain))
                    .child(Button::new(lit!("Save")).variant(ButtonVariant::Filled)),
            );
        }
        Box::new(card)
    }
    fn icon() -> Option<Box<dyn Widget>> {
        Some(icons::card())
    }
    fn category() -> WidgetCategory {
        WidgetCategory::ContainerB
    }
    fn slots() -> &'static [&'static str] {
        &["header", "content", "footer"]
    }
    fn build_with_children(
        _variant: &str,
        knobs: &KnobValues,
        children: Vec<SlottedChild>,
    ) -> Box<dyn Widget> {
        let mut card = Card::new()
            .background(knobs.surface_role("background"))
            .corner_radius(knobs.f32_("corner_radius").get())
            .padding(knobs.f32_("padding").get());
        for c in children {
            match c.slot.as_deref() {
                Some("header") => card = card.header_id(c.id),
                Some("footer") => card = card.footer_id(c.id),
                _ => card = card.content_id(c.id),
            }
        }
        Box::new(card)
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/card.rs", Card);

// ---------------------------------------------------------------------------
// Panel
// ---------------------------------------------------------------------------

impl WidgetCatalog for Panel {
    fn id() -> &'static str {
        "panel"
    }
    fn group() -> &'static str {
        "Containers"
    }
    fn display_name() -> &'static str {
        "Panel"
    }
    fn knobs() -> KnobSpec {
        KnobSpec::new()
            .surface_role("background", "Background", SurfaceRole::Raised)
            .border_role("border_color", "Border colour", BorderRole::Default)
            .f32_("border_width", "Border width", 1.0, 0.0, 4.0)
            .f32_("corner_radius", "Corner radius", 6.0, 0.0, 32.0)
            .f32_("padding", "Padding", 16.0, 0.0, 48.0)
            .text("content", "Sample content", "Panel content")
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![
            PreviewVariant::defaults("default"),
            PreviewVariant::knobs(
                "accent",
                KnobOverrides::new()
                    .surface_role("background", SurfaceRole::AccentSubtle)
                    .border_role("border_color", BorderRole::Accent)
                    .text("content", "Accent panel"),
            ),
            PreviewVariant::knobs(
                "sunken",
                KnobOverrides::new()
                    .surface_role("background", SurfaceRole::Sunken)
                    .text("content", "Sunken panel"),
            ),
            PreviewVariant::knobs("no-border", KnobOverrides::new().f32_("border_width", 0.0)),
            PreviewVariant::knobs("rounded", KnobOverrides::new().f32_("corner_radius", 16.0)),
        ]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        Box::new(
            Panel::new()
                .background(knobs.surface_role("background"))
                .border_color(knobs.border_role("border_color"))
                .border_width(knobs.f32_("border_width").get())
                .corner_radius(knobs.f32_("corner_radius").get())
                .padding(knobs.f32_("padding").get())
                .child(sample_text(&knobs.text("content").get())),
        )
    }
    fn icon() -> Option<Box<dyn Widget>> {
        Some(icons::panel())
    }
    fn category() -> WidgetCategory {
        WidgetCategory::ContainerA
    }
    fn build_with_children(
        _variant: &str,
        knobs: &KnobValues,
        children: Vec<SlottedChild>,
    ) -> Box<dyn Widget> {
        let mut p = Panel::new()
            .background(knobs.surface_role("background"))
            .border_color(knobs.border_role("border_color"))
            .border_width(knobs.f32_("border_width").get())
            .corner_radius(knobs.f32_("corner_radius").get())
            .padding(knobs.f32_("padding").get());
        if let Some(c) = children.into_iter().next() {
            p = p.child_id(c.id);
        }
        Box::new(p)
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/panel.rs", Panel);

// ---------------------------------------------------------------------------
// GroupBox
// ---------------------------------------------------------------------------

impl WidgetCatalog for GroupBox {
    fn id() -> &'static str {
        "group_box"
    }
    fn group() -> &'static str {
        "Containers"
    }
    fn display_name() -> &'static str {
        "GroupBox"
    }
    fn knobs() -> KnobSpec {
        KnobSpec::new().text("title", "Title", "Notifications")
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![
            PreviewVariant::defaults("default"),
            PreviewVariant::knobs("alt-title", KnobOverrides::new().text("title", "Privacy")),
        ]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        Box::new(
            GroupBox::new(lit!(knobs.text("title").get(),)).child(
                VStack::new()
                    .spacing(8.0)
                    .child(Checkbox::new(Signal::new(true)).label(lit!("Sounds")))
                    .child(Checkbox::new(Signal::new(false)).label(lit!("Badges")))
                    .child(Checkbox::new(Signal::new(true)).label(lit!("Banners"))),
            ),
        )
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/group_box.rs", GroupBox);

// ---------------------------------------------------------------------------
// GroupHeader
// ---------------------------------------------------------------------------

impl WidgetCatalog for GroupHeader {
    fn id() -> &'static str {
        "group_header"
    }
    fn group() -> &'static str {
        "Containers"
    }
    fn display_name() -> &'static str {
        "GroupHeader"
    }
    fn knobs() -> KnobSpec {
        KnobSpec::new().text("label", "Label", "Section title")
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![
            PreviewVariant::defaults("default"),
            // Demonstrates the role-based styling API: a bold, accent-colored
            // header that tracks runtime theme changes.
            PreviewVariant::defaults("accent"),
        ]
    }
    fn build(variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        let header = GroupHeader::new(lit!(knobs.text("label").get(),));
        match variant {
            "accent" => Box::new(
                header
                    .style(bastyde_tokens::TextStyleRole::BodyBold)
                    .color(bastyde_tokens::TextRole::Accent),
            ),
            _ => Box::new(header),
        }
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/group_header.rs", GroupHeader);

// ---------------------------------------------------------------------------
// IconButton
// ---------------------------------------------------------------------------

impl WidgetCatalog for IconButton {
    fn id() -> &'static str {
        "icon_button"
    }
    fn group() -> &'static str {
        "Controls"
    }
    fn display_name() -> &'static str {
        "IconButton"
    }
    fn variants() -> Vec<PreviewVariant> {
        // Stand-alone scenarios (default visual mode).
        fn build_search_toolbar() -> Box<dyn Widget> {
            Box::new(IconButton::search().toolbar())
        }
        fn build_add_hero() -> Box<dyn Widget> {
            Box::new(IconButton::add().hero())
        }
        // Embedded scenarios — the JetBrains "built-in" dim look.
        fn build_browse_embedded() -> Box<dyn Widget> {
            Box::new(IconButton::browse().embedded())
        }
        fn build_clear_embedded_compact() -> Box<dyn Widget> {
            Box::new(IconButton::clear().embedded().size(IconButtonSize::Compact))
        }
        vec![
            PreviewVariant::scenario("search-toolbar", build_search_toolbar),
            PreviewVariant::scenario("add-hero", build_add_hero),
            PreviewVariant::scenario("browse-embedded", build_browse_embedded),
            PreviewVariant::scenario("clear-embedded-compact", build_clear_embedded_compact),
        ]
    }
    fn build(variant: &str, _knobs: &KnobValues) -> Box<dyn Widget> {
        scenario_for::<Self>(variant)
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/icon_button.rs", IconButton);

// ---------------------------------------------------------------------------
// Snackbar
// ---------------------------------------------------------------------------

/// `Snackbar` in this codebase is an *overlay-trigger* pattern: a
/// labelled button that opens a popup with the supplied content when
/// clicked. The `label` argument names the trigger button; `.content()`
/// (required — `expect(...)` panics if missing) holds the popup body.
/// We populate both: the trigger reads from a `trigger_label` knob and
/// the popup body reads from a `message` knob, wrapped in a `Panel`
/// for readability when the popup opens.
impl WidgetCatalog for Snackbar {
    fn id() -> &'static str {
        "snackbar"
    }
    fn group() -> &'static str {
        "Feedback"
    }
    fn display_name() -> &'static str {
        "Snackbar"
    }
    fn knobs() -> KnobSpec {
        KnobSpec::new()
            .text("trigger_label", "Trigger label", "Show notification")
            .text("message", "Message", "File saved successfully.")
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![
            PreviewVariant::defaults("default"),
            PreviewVariant::knobs(
                "long",
                KnobOverrides::new().text(
                    "message",
                    "The operation completed but with warnings — review the log for details.",
                ),
            ),
        ]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        let trigger_label = knobs.text("trigger_label").get();
        let message = knobs.text("message").get();
        let popup_content = Panel::new()
            .background(SurfaceRole::Raised)
            .border_color(BorderRole::Default)
            .border_width(1.0)
            .corner_radius(6.0)
            .padding(12.0)
            .child(
                TextWidget::new(lit!(message))
                    .style(TextStyleRole::Body)
                    .color(TextRole::Primary),
            );
        Box::new(Snackbar::new(lit!(trigger_label)).content(popup_content))
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/snackbar.rs", Snackbar);

// ---------------------------------------------------------------------------
// Breadcrumb
// ---------------------------------------------------------------------------

impl WidgetCatalog for Breadcrumb {
    fn id() -> &'static str {
        "breadcrumb"
    }
    fn group() -> &'static str {
        "Containers"
    }
    fn display_name() -> &'static str {
        "Breadcrumb"
    }
    fn variants() -> Vec<PreviewVariant> {
        fn build_path() -> Box<dyn Widget> {
            Box::new(
                Breadcrumb::new()
                    .item(BreadcrumbItem::new(lit!("Home",)))
                    .item(BreadcrumbItem::new(lit!("Projects",)))
                    .item(BreadcrumbItem::new(lit!("Bastyde",)))
                    .item(BreadcrumbItem::new(lit!("crates",))),
            )
        }
        vec![PreviewVariant::scenario("path", build_path)]
    }
    fn build(variant: &str, _knobs: &KnobValues) -> Box<dyn Widget> {
        scenario_for::<Self>(variant)
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/breadcrumb.rs", Breadcrumb);

// ---------------------------------------------------------------------------
// Toolbar (the chrome widget — distinct from the previewer's toolbar pane)
// ---------------------------------------------------------------------------

impl WidgetCatalog for Toolbar {
    fn id() -> &'static str {
        "toolbar"
    }
    fn group() -> &'static str {
        "Chrome"
    }
    fn display_name() -> &'static str {
        "Toolbar"
    }
    fn variants() -> Vec<PreviewVariant> {
        fn build_default() -> Box<dyn Widget> {
            Box::new(
                Toolbar::new()
                    .child(Button::new(lit!("New")).variant(ButtonVariant::Ghost))
                    .child(Button::new(lit!("Open…")).variant(ButtonVariant::Ghost))
                    .child(Button::new(lit!("Save")).variant(ButtonVariant::Ghost)),
            )
        }
        vec![PreviewVariant::scenario("default", build_default)]
    }
    fn build(variant: &str, _knobs: &KnobValues) -> Box<dyn Widget> {
        scenario_for::<Self>(variant)
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/toolbar.rs", Toolbar);

// ---------------------------------------------------------------------------
// StatusBar
// ---------------------------------------------------------------------------

impl WidgetCatalog for StatusBar {
    fn id() -> &'static str {
        "status_bar"
    }
    fn group() -> &'static str {
        "Chrome"
    }
    fn display_name() -> &'static str {
        "StatusBar"
    }
    fn variants() -> Vec<PreviewVariant> {
        fn build_default() -> Box<dyn Widget> {
            Box::new(
                StatusBar::new()
                    .child(
                        TextWidget::new(lit!("Ready"))
                            .style(TextStyleRole::Tiny)
                            .color(TextRole::Secondary),
                    )
                    .child(Spacer::new())
                    .child(
                        TextWidget::new(lit!("Ln 42, Col 17"))
                            .style(TextStyleRole::Tiny)
                            .color(TextRole::Secondary),
                    ),
            )
        }
        vec![PreviewVariant::scenario("default", build_default)]
    }
    fn build(variant: &str, _knobs: &KnobValues) -> Box<dyn Widget> {
        scenario_for::<Self>(variant)
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/status_bar.rs", StatusBar);

// ---------------------------------------------------------------------------
// Accordion
// ---------------------------------------------------------------------------

impl WidgetCatalog for Accordion {
    fn id() -> &'static str {
        "accordion"
    }
    fn group() -> &'static str {
        "Containers"
    }
    fn display_name() -> &'static str {
        "Accordion"
    }
    fn knobs() -> KnobSpec {
        KnobSpec::new()
            .text("title", "Title", "Advanced")
            .bool_("expanded", "Expanded", false)
            .text("content", "Content body", "Hidden until expanded.")
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![
            PreviewVariant::defaults("collapsed"),
            PreviewVariant::knobs(
                "expanded",
                KnobOverrides::new()
                    .bool_("expanded", true)
                    .text("content", "Now visible because the section is expanded."),
            ),
        ]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        let title = knobs.text("title").get();
        let body = knobs.text("content").get();
        // The accordion's `expanded` Signal IS the bool knob — clicking
        // the header in the canvas mutates the signal, the inspector
        // toggle reflects it.
        let expanded = knobs.bool_("expanded");
        Box::new(Accordion::new(lit!(title), expanded).content(sample_text(&body)))
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/accordion.rs", Accordion);

// ---------------------------------------------------------------------------
// RadioGroup
// ---------------------------------------------------------------------------

impl WidgetCatalog for RadioGroup {
    fn id() -> &'static str {
        "radio_group"
    }
    fn group() -> &'static str {
        "Controls"
    }
    fn display_name() -> &'static str {
        "RadioGroup"
    }
    fn variants() -> Vec<PreviewVariant> {
        fn build_default() -> Box<dyn Widget> {
            let selected = Signal::new(0_usize);
            Box::new(
                RadioGroup::new()
                    .child(RadioButton::new(0, selected.clone()).label(lit!("First")))
                    .child(RadioButton::new(1, selected.clone()).label(lit!("Second")))
                    .child(RadioButton::new(2, selected).label(lit!("Third"))),
            )
        }
        vec![PreviewVariant::scenario("default", build_default)]
    }
    fn build(variant: &str, _knobs: &KnobValues) -> Box<dyn Widget> {
        scenario_for::<Self>(variant)
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/radio_group.rs", RadioGroup);

// ---------------------------------------------------------------------------
// SplitButton
// ---------------------------------------------------------------------------

impl WidgetCatalog for SplitButton {
    fn id() -> &'static str {
        "split_button"
    }
    fn group() -> &'static str {
        "Controls"
    }
    fn display_name() -> &'static str {
        "SplitButton"
    }
    fn variants() -> Vec<PreviewVariant> {
        fn build_default() -> Box<dyn Widget> {
            Box::new(
                SplitButton::new_static()
                    .item(MenuItem::new(lit!("Save")))
                    .item(MenuItem::new(lit!("Save As…")))
                    .item(MenuItem::new(lit!("Save All"))),
            )
        }
        vec![PreviewVariant::scenario("default", build_default)]
    }
    fn build(variant: &str, _knobs: &KnobValues) -> Box<dyn Widget> {
        scenario_for::<Self>(variant)
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/split_button.rs", SplitButton);

// =========================================================================
// Tier C — data-driven / structural
// =========================================================================

// ---------------------------------------------------------------------------
// ListView
// ---------------------------------------------------------------------------

impl WidgetCatalog for ListView<String> {
    fn id() -> &'static str {
        "list_view"
    }
    fn group() -> &'static str {
        "Data"
    }
    fn display_name() -> &'static str {
        "ListView"
    }
    fn variants() -> Vec<PreviewVariant> {
        fn build_short() -> Box<dyn Widget> {
            let model = bastyde_data::ListModel::from_vec(vec![
                "Apple".to_string(),
                "Banana".to_string(),
                "Cherry".to_string(),
                "Date".to_string(),
                "Elderberry".to_string(),
                "Fig".to_string(),
            ]);
            Box::new(
                FixedSize::new()
                    .bind_width(280.0_f32)
                    .bind_height(220.0_f32)
                    .child(ListView::new(model, |_idx, item, selected| {
                        Box::new(StandardListItem::new(lit!(item.clone())).selected(selected))
                    })),
            )
        }
        vec![PreviewVariant::scenario("short", build_short)]
    }
    fn build(variant: &str, _knobs: &KnobValues) -> Box<dyn Widget> {
        scenario_for::<Self>(variant)
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/list_view.rs", ListView<String>);

// ---------------------------------------------------------------------------
// GridView
// ---------------------------------------------------------------------------

impl WidgetCatalog for GridView<String> {
    fn id() -> &'static str {
        "grid_view"
    }
    fn group() -> &'static str {
        "Data"
    }
    fn display_name() -> &'static str {
        "GridView"
    }
    fn variants() -> Vec<PreviewVariant> {
        fn items(n: usize) -> bastyde_data::ListModel<String> {
            bastyde_data::ListModel::from_vec((0..n).map(|i| format!("Tile {i}")).collect())
        }
        // RectWidget is a leaf, so layer the label over it in a ZStack.
        fn tile_z(caption: &str, selected: bool) -> Box<dyn Widget> {
            let bg = if selected {
                SurfaceRole::AccentSubtle
            } else {
                SurfaceRole::Raised
            };
            Box::new(
                crate::primitives::ZStack::new()
                    .child(RectWidget::new().background(bg))
                    .child(Center::new().child(
                        TextWidget::new(lit!(caption.to_string())).color(TextRole::Primary),
                    )),
            )
        }
        fn framed(grid: GridView<String>) -> Box<dyn Widget> {
            Box::new(
                FixedSize::new()
                    .bind_width(360.0_f32)
                    .bind_height(320.0_f32)
                    .child(grid),
            )
        }

        fn adaptive() -> Box<dyn Widget> {
            framed(
                GridView::new(items(40), |tc| tile_z(tc.item, tc.is_selected))
                    .sizing(GridSizing::Adaptive {
                        min_width: 90.0,
                        max_width: None,
                        height: 64.0,
                    })
                    .spacing(8.0),
            )
        }
        fn fixed_columns() -> Box<dyn Widget> {
            framed(
                GridView::new(items(40), |tc| tile_z(tc.item, tc.is_selected))
                    .column_count(4, 64.0)
                    .spacing(8.0),
            )
        }
        fn selectable() -> Box<dyn Widget> {
            use bastyde_data::{SelectionMode, SelectionModel};
            let sel = SelectionModel::new(SelectionMode::Multi);
            sel.select(2);
            framed(
                GridView::new(items(40), |tc| tile_z(tc.item, tc.is_selected))
                    .sizing(GridSizing::Adaptive {
                        min_width: 90.0,
                        max_width: None,
                        height: 64.0,
                    })
                    .spacing(8.0)
                    .selection(sel),
            )
        }
        fn waterfall() -> Box<dyn Widget> {
            // `.item_height` drives the exact per-item height, so the tile
            // widget itself can stay plain.
            framed(
                GridView::new(items(40), |tc| tile_z(tc.item, tc.is_selected))
                    .column_count(3, 64.0)
                    .waterfall(64.0)
                    .item_height(|i| 48.0 + (i % 5) as f32 * 18.0)
                    .spacing(8.0),
            )
        }

        vec![
            PreviewVariant::scenario("adaptive", adaptive),
            PreviewVariant::scenario("fixed_columns", fixed_columns),
            PreviewVariant::scenario("selection", selectable),
            PreviewVariant::scenario("waterfall", waterfall),
        ]
    }
    fn build(variant: &str, _knobs: &KnobValues) -> Box<dyn Widget> {
        scenario_for::<Self>(variant)
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/grid_view.rs", GridView<String>);

// ---------------------------------------------------------------------------
// TreeView
// ---------------------------------------------------------------------------

impl WidgetCatalog for TreeView<String> {
    fn id() -> &'static str {
        "tree_view"
    }
    fn group() -> &'static str {
        "Data"
    }
    fn display_name() -> &'static str {
        "TreeView"
    }
    fn variants() -> Vec<PreviewVariant> {
        fn build_default() -> Box<dyn Widget> {
            let model = bastyde_data::TreeModel::<String>::new();
            let root = model.insert_root(0, "Project".to_string());
            let crates_node = model.insert_child(root, 0, "crates".to_string());
            model.insert_child(crates_node, 0, "bastyde-core".to_string());
            model.insert_child(crates_node, 1, "bastyde-widgets".to_string());
            model.insert_child(crates_node, 2, "bastyde-render".to_string());
            let docs = model.insert_child(root, 1, "docs".to_string());
            model.insert_child(docs, 0, "architecture.md".to_string());
            Box::new(
                FixedSize::new()
                    .bind_width(280.0_f32)
                    .bind_height(220.0_f32)
                    .child(TreeView::new_with_context(
                        model,
                        |item, entry, selected, ctx| {
                            Box::new(
                                StandardTreeItem::new(lit!(item.clone()))
                                    .from_entry(entry)
                                    .selected(selected)
                                    .on_toggle_rc(ctx.toggle_callback()),
                            )
                        },
                    )),
            )
        }
        vec![PreviewVariant::scenario("default", build_default)]
    }
    fn build(variant: &str, _knobs: &KnobValues) -> Box<dyn Widget> {
        scenario_for::<Self>(variant)
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/tree_view.rs", TreeView<String>);

// ---------------------------------------------------------------------------
// StandardListItem
// ---------------------------------------------------------------------------

impl WidgetCatalog for StandardListItem {
    fn id() -> &'static str {
        "standard_list_item"
    }
    fn group() -> &'static str {
        "Data"
    }
    fn display_name() -> &'static str {
        "StandardListItem"
    }
    fn variants() -> Vec<PreviewVariant> {
        fn build_single_line() -> Box<dyn Widget> {
            Box::new(StandardListItem::new(lit!("Single-line item")))
        }
        fn build_with_all_primary_slots() -> Box<dyn Widget> {
            Box::new(
                StandardListItem::new(lit!("With every primary slot"))
                    .leading_slot(TextWidget::new(lit!("●")).color(TextRole::Accent))
                    .center_slot(TextWidget::new(lit!("•")).color(TextRole::Secondary))
                    .trailing_slot(TextWidget::new(lit!("12")).color(TextRole::Secondary)),
            )
        }
        fn build_two_line_with_subtitle_slots() -> Box<dyn Widget> {
            Box::new(
                StandardListItem::new(lit!("Title line"))
                    .subtitle(lit!("Subtitle line"))
                    .leading_slot(TextWidget::new(lit!("●")).color(TextRole::Accent))
                    .subtitle_leading_slot(TextWidget::new(lit!("•")).color(TextRole::Secondary))
                    .subtitle_trailing_slot(
                        TextWidget::new(lit!("just now")).color(TextRole::Secondary),
                    )
                    .trailing_slot(TextWidget::new(lit!("∗")).color(TextRole::Accent)),
            )
        }
        fn build_with_checkbox() -> Box<dyn Widget> {
            let checked = Signal::new(true);
            Box::new(StandardListItem::new(lit!("With two-state checkbox")).checkbox(checked))
        }
        fn build_with_tristate_checkbox() -> Box<dyn Widget> {
            use bastyde_data::CheckState;
            let s = Signal::new(CheckState::Indeterminate);
            Box::new(StandardListItem::new(lit!("With tristate checkbox")).tristate_checkbox(s))
        }
        fn build_selected() -> Box<dyn Widget> {
            Box::new(StandardListItem::new(lit!("Selected")).selected(true))
        }
        fn build_disabled() -> Box<dyn Widget> {
            Box::new(StandardListItem::new(lit!("Disabled")).enabled(false))
        }
        vec![
            PreviewVariant::scenario("single_line", build_single_line),
            PreviewVariant::scenario("all_primary_slots", build_with_all_primary_slots),
            PreviewVariant::scenario("two_line", build_two_line_with_subtitle_slots),
            PreviewVariant::scenario("checkbox", build_with_checkbox),
            PreviewVariant::scenario("tristate_checkbox", build_with_tristate_checkbox),
            PreviewVariant::scenario("selected", build_selected),
            PreviewVariant::scenario("disabled", build_disabled),
        ]
    }
    fn build(variant: &str, _knobs: &KnobValues) -> Box<dyn Widget> {
        scenario_for::<Self>(variant)
    }
}
register_widget_catalog_at!(
    "crates/bastyde-widgets/src/standard_item.rs",
    StandardListItem
);

// ---------------------------------------------------------------------------
// StandardTreeItem
// ---------------------------------------------------------------------------

impl WidgetCatalog for StandardTreeItem {
    fn id() -> &'static str {
        "standard_tree_item"
    }
    fn group() -> &'static str {
        "Data"
    }
    fn display_name() -> &'static str {
        "StandardTreeItem"
    }
    fn variants() -> Vec<PreviewVariant> {
        fn build_collapsed_branch() -> Box<dyn Widget> {
            Box::new(
                StandardTreeItem::new(lit!("Folder (collapsed)"))
                    .depth(0)
                    .has_children(true)
                    .is_expanded(false),
            )
        }
        fn build_expanded_branch() -> Box<dyn Widget> {
            Box::new(
                StandardTreeItem::new(lit!("Folder (expanded)"))
                    .depth(0)
                    .has_children(true)
                    .is_expanded(true),
            )
        }
        fn build_leaf_indented() -> Box<dyn Widget> {
            Box::new(
                StandardTreeItem::new(lit!("Deep leaf"))
                    .depth(2)
                    .has_children(false),
            )
        }
        fn build_with_tristate_checkbox() -> Box<dyn Widget> {
            use bastyde_data::CheckState;
            let s = Signal::new(CheckState::Indeterminate);
            Box::new(
                StandardTreeItem::new(lit!("Folder with tristate"))
                    .depth(1)
                    .has_children(true)
                    .is_expanded(true)
                    .tristate_checkbox(s),
            )
        }
        fn build_two_line() -> Box<dyn Widget> {
            Box::new(
                StandardTreeItem::new(lit!("Folder"))
                    .subtitle(lit!("3 items · last week"))
                    .depth(0)
                    .has_children(true)
                    .is_expanded(false)
                    .subtitle_trailing_slot(TextWidget::new(lit!("3")).color(TextRole::Secondary)),
            )
        }
        vec![
            PreviewVariant::scenario("collapsed_branch", build_collapsed_branch),
            PreviewVariant::scenario("expanded_branch", build_expanded_branch),
            PreviewVariant::scenario("leaf_indented", build_leaf_indented),
            PreviewVariant::scenario("tristate_checkbox", build_with_tristate_checkbox),
            PreviewVariant::scenario("two_line", build_two_line),
        ]
    }
    fn build(variant: &str, _knobs: &KnobValues) -> Box<dyn Widget> {
        scenario_for::<Self>(variant)
    }
}
register_widget_catalog_at!(
    "crates/bastyde-widgets/src/standard_item.rs",
    StandardTreeItem
);

// ---------------------------------------------------------------------------
// MenuList
// ---------------------------------------------------------------------------

impl WidgetCatalog for MenuList {
    fn id() -> &'static str {
        "menu_list"
    }
    fn group() -> &'static str {
        "Menus"
    }
    fn display_name() -> &'static str {
        "MenuList"
    }
    fn variants() -> Vec<PreviewVariant> {
        fn build_default() -> Box<dyn Widget> {
            Box::new(
                MenuList::new()
                    .item(MenuItem::new(lit!("New")))
                    .item(MenuItem::new(lit!("Open…")))
                    .item(MenuItem::new(lit!("Save")))
                    .item(MenuItem::new(lit!("Save As…")))
                    .item(MenuItem::new(lit!("Close"))),
            )
        }
        vec![PreviewVariant::scenario("default", build_default)]
    }
    fn build(variant: &str, _knobs: &KnobValues) -> Box<dyn Widget> {
        scenario_for::<Self>(variant)
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/menu_list.rs", MenuList);

// ---------------------------------------------------------------------------
// ScrollArea
// ---------------------------------------------------------------------------

impl WidgetCatalog for ScrollArea {
    fn id() -> &'static str {
        "scroll_area"
    }
    fn group() -> &'static str {
        "Data"
    }
    fn display_name() -> &'static str {
        "ScrollArea"
    }
    fn variants() -> Vec<PreviewVariant> {
        fn build_long_content() -> Box<dyn Widget> {
            let mut col = VStack::new().spacing(4.0);
            for i in 1..=40 {
                col = col.child(
                    Padding::symmetric(4.0, 8.0).child(
                        TextWidget::new(lit!(format!("Row {}", i)))
                            .style(TextStyleRole::Body)
                            .color(TextRole::Primary),
                    ),
                );
            }
            Box::new(
                FixedSize::new()
                    .bind_width(280.0_f32)
                    .bind_height(180.0_f32)
                    .child(ScrollArea::new().child(col)),
            )
        }
        vec![PreviewVariant::scenario("long-content", build_long_content)]
    }
    fn build(variant: &str, _knobs: &KnobValues) -> Box<dyn Widget> {
        scenario_for::<Self>(variant)
    }
    fn icon() -> Option<Box<dyn Widget>> {
        Some(icons::scroll_area())
    }
    fn category() -> WidgetCategory {
        WidgetCategory::ContainerA
    }
    fn build_with_children(
        _variant: &str,
        _knobs: &KnobValues,
        children: Vec<SlottedChild>,
    ) -> Box<dyn Widget> {
        match children.into_iter().next() {
            Some(c) => Box::new(ScrollArea::from_id(c.id)),
            None => Box::new(ScrollArea::new()),
        }
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/scroll_area.rs", ScrollArea);

// ---------------------------------------------------------------------------
// Splitter
// ---------------------------------------------------------------------------

impl WidgetCatalog for Splitter {
    fn id() -> &'static str {
        "splitter"
    }
    fn group() -> &'static str {
        "Containers"
    }
    fn display_name() -> &'static str {
        "Splitter"
    }
    fn variants() -> Vec<PreviewVariant> {
        fn build_horizontal() -> Box<dyn Widget> {
            let left = Panel::new()
                .background(SurfaceRole::Sunken)
                .padding(12.0)
                .child(sample_text("Left pane"));
            let right = Panel::new()
                .background(SurfaceRole::Raised)
                .padding(12.0)
                .child(sample_text("Right pane"));
            Box::new(
                FixedSize::new()
                    .bind_width(420.0_f32)
                    .bind_height(220.0_f32)
                    .child(
                        Splitter::new(SplitterModel::new(2, Orientation::Horizontal))
                            .pane(left)
                            .pane(right),
                    ),
            )
        }
        fn build_three_pane() -> Box<dyn Widget> {
            let model = SplitterModel::from_panes(
                vec![
                    PaneDescriptor::new().size(120.0).collapsible(true).stretch(0.0),
                    PaneDescriptor::new().stretch(1.0),
                    PaneDescriptor::new().size(120.0).collapsible(true).stretch(0.0),
                ],
                Orientation::Horizontal,
            );
            let pane = |label: &str, role| {
                Panel::new()
                    .background(role)
                    .padding(12.0)
                    .child(sample_text(label))
            };
            Box::new(
                FixedSize::new().bind_width(480.0_f32).bind_height(220.0_f32).child(
                    Splitter::new(model)
                        .pane(pane("Sidebar", SurfaceRole::Sunken))
                        .pane(pane("Editor", SurfaceRole::Raised))
                        .pane(pane("Inspector", SurfaceRole::Sunken)),
                ),
            )
        }
        vec![
            PreviewVariant::scenario("horizontal", build_horizontal),
            PreviewVariant::scenario("three_pane", build_three_pane),
        ]
    }
    fn build(variant: &str, _knobs: &KnobValues) -> Box<dyn Widget> {
        scenario_for::<Self>(variant)
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/splitter.rs", Splitter);

// ---------------------------------------------------------------------------
// TabWidget
// ---------------------------------------------------------------------------

impl WidgetCatalog for TabWidget {
    fn id() -> &'static str {
        "tab_widget"
    }
    fn group() -> &'static str {
        "Containers"
    }
    fn display_name() -> &'static str {
        "TabWidget"
    }
    fn variants() -> Vec<PreviewVariant> {
        fn build_three_tabs() -> Box<dyn Widget> {
            use crate::tab_widget::{TabId, TabInfo};
            let selected: Signal<Option<TabId>> = Signal::new(None);
            Box::new(
                FixedSize::new()
                    .bind_width(420.0_f32)
                    .bind_height(220.0_f32)
                    .child(
                        TabWidget::new(selected)
                            .static_tab(
                                TabInfo::new().title(lit!("Overview")),
                                Center::new().child(sample_text("Overview tab content")),
                            )
                            .static_tab(
                                TabInfo::new().title(lit!("Details")),
                                Center::new().child(sample_text("Details tab content")),
                            )
                            .static_tab(
                                TabInfo::new().title(lit!("Settings")),
                                Center::new().child(sample_text("Settings tab content")),
                            ),
                    ),
            )
        }
        vec![PreviewVariant::scenario("three-tabs", build_three_tabs)]
    }
    fn build(variant: &str, _knobs: &KnobValues) -> Box<dyn Widget> {
        scenario_for::<Self>(variant)
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/tab_widget.rs", TabWidget);

// ---------------------------------------------------------------------------
// ToolBox
// ---------------------------------------------------------------------------

impl WidgetCatalog for ToolBox {
    fn id() -> &'static str {
        "tool_box"
    }
    fn group() -> &'static str {
        "Containers"
    }
    fn display_name() -> &'static str {
        "ToolBox"
    }
    fn variants() -> Vec<PreviewVariant> {
        fn build_three_items() -> Box<dyn Widget> {
            let selected = Signal::new(0_usize);
            Box::new(
                FixedSize::new()
                    .bind_width(280.0_f32)
                    .bind_height(280.0_f32)
                    .child(
                        ToolBox::new(selected)
                            .item(
                                lit!("General"),
                                Padding::uniform(12.0).child(sample_text("General settings")),
                            )
                            .item(
                                lit!("Editor"),
                                Padding::uniform(12.0).child(sample_text("Editor settings")),
                            )
                            .item(
                                lit!("Keymap"),
                                Padding::uniform(12.0).child(sample_text("Keymap settings")),
                            ),
                    ),
            )
        }
        vec![PreviewVariant::scenario("three-items", build_three_items)]
    }
    fn build(variant: &str, _knobs: &KnobValues) -> Box<dyn Widget> {
        scenario_for::<Self>(variant)
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/tool_box.rs", ToolBox);

// ---------------------------------------------------------------------------
// Repeater
// ---------------------------------------------------------------------------

impl WidgetCatalog for crate::Repeater<String> {
    fn id() -> &'static str {
        "repeater"
    }
    fn group() -> &'static str {
        "Data"
    }
    fn display_name() -> &'static str {
        "Repeater"
    }
    fn variants() -> Vec<PreviewVariant> {
        fn build_default() -> Box<dyn Widget> {
            let model = bastyde_data::ListModel::from_vec(vec![
                "Alpha".to_string(),
                "Beta".to_string(),
                "Gamma".to_string(),
                "Delta".to_string(),
            ]);
            Box::new(
                crate::Repeater::new(model, |_idx, item| {
                    Box::new(
                        Padding::symmetric(4.0, 8.0).child(
                            TextWidget::new(lit!(item.clone()))
                                .style(TextStyleRole::Body)
                                .color(TextRole::Primary),
                        ),
                    )
                })
                .spacing(4.0),
            )
        }
        vec![PreviewVariant::scenario("default", build_default)]
    }
    fn build(variant: &str, _knobs: &KnobValues) -> Box<dyn Widget> {
        scenario_for::<Self>(variant)
    }
}
register_widget_catalog_at!(
    "crates/bastyde-widgets/src/repeater.rs",
    crate::Repeater<String>
);

// =========================================================================
// Helpers
// =========================================================================

/// Resolve a variant name to its scenario builder result. Used by
/// every Tier B/C `build(...)` method whose variants are all
/// `Scenario`-shaped — saves ~6 lines per impl. Falls back to the
/// first declared variant when `name` is unknown.
fn scenario_for<W: WidgetCatalog>(name: &str) -> Box<dyn Widget> {
    let variants = W::variants();
    let chosen = variants
        .iter()
        .find(|v| v.name() == name)
        .or_else(|| variants.first());
    match chosen {
        Some(PreviewVariant::Scenario { builder, .. }) => builder(),
        _ => Box::new(
            TextWidget::new(lit!(format!("(no scenario for variant '{}')", name)))
                .style(TextStyleRole::Small)
                .color(TextRole::Secondary),
        ),
    }
}

// =========================================================================
// Color picker family (HexColorInput, ColorPicker, ColorEdit)
// =========================================================================

mod color_family {
    use super::*;
    use crate::{ColorEdit, ColorPicker, ColorPickerLayout, HexColorInput};
    use bastyde_tokens::Color;

    impl WidgetCatalog for HexColorInput {
        fn id() -> &'static str {
            "hex-color-input"
        }
        fn group() -> &'static str {
            "Color"
        }
        fn display_name() -> &'static str {
            "HexColorInput"
        }
        fn knobs() -> KnobSpec {
            KnobSpec::new()
        }
        fn variants() -> Vec<PreviewVariant> {
            fn default_var() -> Box<dyn Widget> {
                Box::new(HexColorInput::new(Signal::new(Color::from_hex("#3584E4"))))
            }
            fn alpha_var() -> Box<dyn Widget> {
                Box::new(
                    HexColorInput::new(Signal::new(Color::from_rgba(1.0, 0.5, 0.0, 0.6)))
                        .alpha_enabled(true),
                )
            }
            vec![
                PreviewVariant::scenario("default", default_var),
                PreviewVariant::scenario("with-alpha", alpha_var),
            ]
        }
        fn build(variant: &str, _knobs: &KnobValues) -> Box<dyn Widget> {
            scenario_for::<Self>(variant)
        }
    }
    register_widget_catalog_at!(
        "crates/bastyde-widgets/src/hex_color_input.rs",
        HexColorInput
    );

    impl WidgetCatalog for ColorPicker {
        fn id() -> &'static str {
            "color-picker"
        }
        fn group() -> &'static str {
            "Color"
        }
        fn display_name() -> &'static str {
            "ColorPicker"
        }
        fn knobs() -> KnobSpec {
            KnobSpec::new()
        }
        fn variants() -> Vec<PreviewVariant> {
            fn default_var() -> Box<dyn Widget> {
                Box::new(ColorPicker::new(Signal::new(Color::from_hex("#3584E4"))))
            }
            fn with_alpha() -> Box<dyn Widget> {
                Box::new(
                    ColorPicker::new(Signal::new(Color::from_rgba(0.21, 0.66, 0.40, 0.5)))
                        .alpha_enabled(true),
                )
            }
            fn compact() -> Box<dyn Widget> {
                Box::new(
                    ColorPicker::new(Signal::new(Color::from_hex("#E91E63")))
                        .layout(ColorPickerLayout::Compact),
                )
            }
            fn wide() -> Box<dyn Widget> {
                Box::new(
                    ColorPicker::new(Signal::new(Color::from_hex("#FF9800")))
                        .alpha_enabled(true)
                        .layout(ColorPickerLayout::Wide)
                        .show_hsv_spinners(true),
                )
            }
            vec![
                PreviewVariant::scenario("default", default_var),
                PreviewVariant::scenario("with-alpha", with_alpha),
                PreviewVariant::scenario("compact", compact),
                PreviewVariant::scenario("wide", wide),
            ]
        }
        fn build(variant: &str, _knobs: &KnobValues) -> Box<dyn Widget> {
            scenario_for::<Self>(variant)
        }
    }
    register_widget_catalog_at!("crates/bastyde-widgets/src/color_picker.rs", ColorPicker);

    impl WidgetCatalog for ColorEdit {
        fn id() -> &'static str {
            "color-edit"
        }
        fn group() -> &'static str {
            "Color"
        }
        fn display_name() -> &'static str {
            "ColorEdit"
        }
        fn knobs() -> KnobSpec {
            KnobSpec::new()
        }
        fn variants() -> Vec<PreviewVariant> {
            fn default_var() -> Box<dyn Widget> {
                Box::new(ColorEdit::new(Signal::new(Color::from_hex("#3584E4"))))
            }
            fn with_alpha() -> Box<dyn Widget> {
                Box::new(
                    ColorEdit::new(Signal::new(Color::from_rgba(0.92, 0.27, 0.18, 0.6)))
                        .alpha_enabled(true),
                )
            }
            fn no_hex_in_trigger() -> Box<dyn Widget> {
                Box::new(
                    ColorEdit::new(Signal::new(Color::from_hex("#9C27B0")))
                        .show_hex_in_trigger(false),
                )
            }
            fn nullable_var() -> Box<dyn Widget> {
                let v: Signal<Option<Color>> = Signal::new(None);
                Box::new(ColorEdit::nullable(v))
            }
            vec![
                PreviewVariant::scenario("default", default_var),
                PreviewVariant::scenario("with-alpha", with_alpha),
                PreviewVariant::scenario("no-hex-in-trigger", no_hex_in_trigger),
                PreviewVariant::scenario("nullable", nullable_var),
            ]
        }
        fn build(variant: &str, _knobs: &KnobValues) -> Box<dyn Widget> {
            scenario_for::<Self>(variant)
        }
    }
    register_widget_catalog_at!("crates/bastyde-widgets/src/color_edit.rs", ColorEdit);
}

// =========================================================================
// Secure input family (PasswordField)
// =========================================================================

mod secure_input_family {
    use super::*;
    use crate::{EchoMode, PasswordField, RevealMode};

    impl WidgetCatalog for PasswordField {
        fn id() -> &'static str {
            "password-field"
        }
        fn group() -> &'static str {
            "Inputs"
        }
        fn display_name() -> &'static str {
            "PasswordField"
        }
        fn knobs() -> KnobSpec {
            KnobSpec::new()
                .text("placeholder", "Placeholder", "Enter your password")
                .text("text", "Initial text", "hunter2")
                .choice(
                    "echo_mode",
                    "Echo mode",
                    &["Masked", "NoEcho", "RevealWhileTyping"],
                    0,
                )
                .choice(
                    "reveal_mode",
                    "Reveal button",
                    &["Toggle", "Hold", "None"],
                    0,
                )
                .bool_("enabled", "Enabled", true)
                .bool_("caps_warning", "Caps Lock warning", true)
        }
        fn variants() -> Vec<PreviewVariant> {
            vec![
                PreviewVariant::defaults("default"),
                PreviewVariant::knobs(
                    "reveal-while-typing",
                    KnobOverrides::new().choice("echo_mode", 2),
                ),
                PreviewVariant::knobs(
                    "hold-to-reveal",
                    KnobOverrides::new().choice("reveal_mode", 1),
                ),
                PreviewVariant::knobs("no-echo", KnobOverrides::new().choice("echo_mode", 1)),
                PreviewVariant::knobs(
                    "no-reveal-button",
                    KnobOverrides::new().choice("reveal_mode", 2),
                ),
                PreviewVariant::knobs("disabled", KnobOverrides::new().bool_("enabled", false)),
            ]
        }
        fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
            let placeholder = knobs.text("placeholder").get();
            let initial = knobs.text("text").get();
            let echo = match knobs.choice("echo_mode").get() {
                1 => EchoMode::NoEcho,
                2 => EchoMode::RevealWhileTyping,
                _ => EchoMode::Masked,
            };
            let reveal = match knobs.choice("reveal_mode").get() {
                1 => RevealMode::Hold,
                2 => RevealMode::None,
                _ => RevealMode::Toggle,
            };
            let enabled = knobs.bool_("enabled").get();
            let caps = knobs.bool_("caps_warning").get();
            Box::new(
                PasswordField::new(Signal::new(initial))
                    .label(lit!("Password"))
                    .placeholder(lit!(placeholder))
                    .echo_mode(echo)
                    .reveal_mode(reveal)
                    .enabled(enabled)
                    .caps_lock_warning(caps),
            )
        }
    }
    register_widget_catalog_at!(
        "crates/bastyde-widgets/src/password_field.rs",
        PasswordField
    );
}

#[cfg(all(test, feature = "preview"))]
mod build_with_children_tests {
    use super::*;
    use bastyde_canvas::SizeProposal;
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_preview::{CatalogEntry, KnobValues, SlottedChild, WidgetCategory, find_by_id};

    fn knobs_for(entry: &dyn CatalogEntry) -> KnobValues {
        KnobValues::from_spec(&entry.knobs(), None)
    }

    /// Collect every descendant id under `root` (exclusive).
    fn descendants(tree: &WidgetTree, root: bastyde_core::widget_id::WidgetId) -> Vec<bastyde_core::widget_id::WidgetId> {
        let mut out = Vec::new();
        let mut stack = vec![root];
        while let Some(n) = stack.pop() {
            for ch in tree.children(n) {
                out.push(ch);
                stack.push(ch);
            }
        }
        out
    }

    #[test]
    fn leaf_button_default_ignores_children() {
        let entry = find_by_id("button").expect("button registered");
        assert_eq!(entry.category(), WidgetCategory::Leaf);
        assert!(entry.icon().is_some());
        let knobs = knobs_for(entry);
        let mut tree = WidgetTree::new();
        let stray = tree.add(TextWidget::new(lit!("x")));
        let w = entry.build_with_children(
            "default",
            &knobs,
            vec![SlottedChild { slot: None, id: stray }],
        );
        let id = tree.add_boxed(w);
        tree.layout(SizeProposal::exact(400.0, 200.0));
        // The leaf default ignores injected children — the stray is not adopted.
        assert!(!descendants(&tree, id).contains(&stray));
    }

    #[test]
    fn vstack_container_a_wires_ordered_children() {
        let entry = find_by_id("vstack").expect("vstack registered");
        assert_eq!(entry.category(), WidgetCategory::ContainerA);
        assert!(entry.icon().is_some());
        let knobs = knobs_for(entry);
        let mut tree = WidgetTree::new();
        let a = tree.add(TextWidget::new(lit!("A")));
        let b = tree.add(TextWidget::new(lit!("B")));
        let c = tree.add(TextWidget::new(lit!("C")));
        let w = entry.build_with_children(
            "default",
            &knobs,
            vec![
                SlottedChild { slot: None, id: a },
                SlottedChild { slot: None, id: b },
                SlottedChild { slot: None, id: c },
            ],
        );
        let id = tree.add_boxed(w);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        assert_eq!(tree.children(id), vec![a, b, c]);
    }

    #[test]
    fn card_container_b_routes_named_slots() {
        let entry = find_by_id("card").expect("card registered");
        assert_eq!(entry.category(), WidgetCategory::ContainerB);
        assert_eq!(entry.slots(), &["header", "content", "footer"][..]);
        let knobs = knobs_for(entry);
        let mut tree = WidgetTree::new();
        let header = tree.add(TextWidget::new(lit!("H")));
        let content = tree.add(TextWidget::new(lit!("C")));
        let footer = tree.add(TextWidget::new(lit!("F")));
        let w = entry.build_with_children(
            "default",
            &knobs,
            vec![
                SlottedChild { slot: Some("header".into()), id: header },
                SlottedChild { slot: Some("content".into()), id: content },
                SlottedChild { slot: Some("footer".into()), id: footer },
            ],
        );
        let id = tree.add_boxed(w);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let all = descendants(&tree, id);
        assert!(all.contains(&header), "header slot wired");
        assert!(all.contains(&content), "content slot wired");
        assert!(all.contains(&footer), "footer slot wired");
    }

    #[test]
    fn curated_widgets_have_icons() {
        for id in [
            "vstack", "hstack", "zstack", "grid", "padding", "expand", "center", "spacer",
            "button", "text_widget", "checkbox", "text_input", "toggle", "combo_box", "slider",
            "card", "panel", "scroll_area",
        ] {
            let entry = find_by_id(id).unwrap_or_else(|| panic!("{id} registered"));
            assert!(entry.icon().is_some(), "{id} should have an icon");
        }
    }
}
