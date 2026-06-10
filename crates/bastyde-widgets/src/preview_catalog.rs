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
//!   ScrollArea, SplitView, TabWidget, ToolBox, Repeater.
//! - Skipped (modal / event-heavy / overlay-driven): Dialog,
//!   MessageBox, Popover, Wizard, MenuBar, MenuContext, TitleBar,
//!   ShortcutSettings, ImageWidget. These need additional context
//!   (intent registry, modal manager, raster resources) the catalog
//!   does not provide.

use bastyde_core::signal::Signal;
use bastyde_core::widget::Widget;
use bastyde_i18n::lit;
use bastyde_preview::{
    KnobOverrides, KnobSpec, KnobValues, PreviewVariant, WidgetCatalog, register_widget_catalog_at,
};
use bastyde_tokens::{BorderRole, SurfaceRole, TextRole, TextStyleRole};

use crate::primitives::{
    Center, FixedSize, HStack, IconWidget, Padding, Spacer, TextWidget, VStack,
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
    RadioButton, RadioGroup, ScrollArea, SegmentedControl, Slider, Snackbar, SplitButton,
    SplitView, StandardListItem, StandardTreeItem, StatusBar, TabWidget, Toggle, ToolBox, Toolbar,
    TreeView,
};

// ---------------------------------------------------------------------------
// Button
// ---------------------------------------------------------------------------

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
            .choice("variant", "Variant", &["Default", "Regular", "Flat"], 1)
            .bool_("enabled", "Enabled", true)
            .opt_text("tooltip", "Tooltip", None)
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![
            PreviewVariant::defaults("default"),
            PreviewVariant::knobs(
                "primary",
                KnobOverrides::new()
                    .choice("variant", 0)
                    .text("label", "Save"),
            ),
            PreviewVariant::knobs(
                "flat",
                KnobOverrides::new()
                    .choice("variant", 2)
                    .text("label", "More…"),
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
        let variant_idx = knobs.choice("variant").get();
        let enabled = knobs.bool_("enabled").get();
        let tooltip = knobs.opt_text("tooltip").get();
        let style = match variant_idx {
            0 => ButtonVariant::Filled,
            2 => ButtonVariant::Ghost,
            _ => ButtonVariant::Plain,
        };
        let mut b = Button::new(lit!(label)).variant(style).enabled(enabled);
        if let Some(t) = tooltip {
            b = b.tooltip(lit!(t));
        }
        Box::new(b)
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/button.rs", Button);

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
        Box::new(Checkbox::new(checked).label(lit!(label)).enabled(enabled))
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
            .choice("orientation", "Orientation", &["Horizontal", "Vertical"], 0)
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
            PreviewVariant::knobs("vertical", KnobOverrides::new().choice("orientation", 1)),
            PreviewVariant::knobs("disabled", KnobOverrides::new().bool_("enabled", false)),
            PreviewVariant::knobs(
                "with-label",
                KnobOverrides::new().opt_text("label", Some("Volume")),
            ),
        ]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        use bastyde_tokens::Orientation;
        let orient = match knobs.choice("orientation").get() {
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
            .text_role("text_color", "Text colour", TextRole::OnAccent)
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
                    .text_role("text_color", TextRole::Success),
            ),
            PreviewVariant::knobs(
                "warning",
                KnobOverrides::new()
                    .text("label", "BETA")
                    .surface_role("background", SurfaceRole::StatusWarning)
                    .text_role("text_color", TextRole::Warning),
            ),
            PreviewVariant::knobs(
                "error",
                KnobOverrides::new()
                    .text("label", "ERR")
                    .surface_role("background", SurfaceRole::StatusError)
                    .text_role("text_color", TextRole::Error),
            ),
        ]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        let bg = knobs.surface_role("background");
        let fg = knobs.text_role("text_color");
        Box::new(
            Badge::new(lit!(knobs.text("label").get()))
                .color(bg)
                .text_color(fg),
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
        vec![PreviewVariant::defaults("default")]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        Box::new(GroupHeader::new(lit!(knobs.text("label").get(),)))
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
}
register_widget_catalog_at!("crates/bastyde-widgets/src/scroll_area.rs", ScrollArea);

// ---------------------------------------------------------------------------
// SplitView
// ---------------------------------------------------------------------------

impl WidgetCatalog for SplitView {
    fn id() -> &'static str {
        "split_view"
    }
    fn group() -> &'static str {
        "Containers"
    }
    fn display_name() -> &'static str {
        "SplitView"
    }
    fn variants() -> Vec<PreviewVariant> {
        fn build_horizontal() -> Box<dyn Widget> {
            let split = Signal::new(0.4_f32);
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
                    .child(SplitView::new(split).first(left).second(right)),
            )
        }
        vec![PreviewVariant::scenario("horizontal", build_horizontal)]
    }
    fn build(variant: &str, _knobs: &KnobValues) -> Box<dyn Widget> {
        scenario_for::<Self>(variant)
    }
}
register_widget_catalog_at!("crates/bastyde-widgets/src/split_view.rs", SplitView);

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
