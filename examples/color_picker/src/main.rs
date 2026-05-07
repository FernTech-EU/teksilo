//! ColorPicker showcase — every layout / configuration of the new
//! color-selection widgets in one scrollable window.
//!
//! Run with: `cargo run -p color-picker-demo`.
//!
//! Sections:
//! 1. **Standard layout** (default) — HSV canvas + hue strip + RGB
//!    spinners + hex input + preset swatches.
//! 2. **With alpha** — same as Standard plus the alpha strip and an
//!    A spinner cell. The current-color preview shows the
//!    checkerboard underlay.
//! 3. **Compact layout** — HSV canvas + hue strip + hex only. Good
//!    for popovers.
//! 4. **Wide layout** — HSV canvas + strips on the left, RGB / HSV
//!    spinners stacked vertically on the right, swatches below.
//! 5. **HSV spinners only** — RGB spinners hidden, HSV spinners shown.
//! 6. **Custom swatches** — Material-flavored palette.
//! 7. **HexColorInput standalone** — bound to a Panel's background so
//!    typing `#FF8800` updates a live preview.
//! 8. **ColorEdit row** — three compact `ColorEdit` triggers with
//!    popover-style pickers (default, alpha, nullable).
//! 9. **Disabled** — `.enabled(false)` on a ColorPicker.

use fern_ui::core::WidgetPlacement;
use fern_ui::i18n::I18nConfig;
use fern_ui::prelude::*;
use fern_ui::tokens::{Color, Theme};
use fern_ui::widgets::{
    Button, ColorEdit, ColorPicker, ColorPickerLayout, Expand, HStack, HexColorInput, Padding,
    Panel, ScrollArea, Spacer, TextWidget, Toolbar, VStack,
};

fn dark_mode_toolbar() -> impl Widget {
    let is_dark = Signal::new(false);
    Toolbar::new().child(HStack::new().child(Spacer::new()).child(
        Button::new_literal("Toggle Dark Mode").on_activate_fn(move |ctx| {
            let next = !is_dark.get();
            is_dark.set(next);
            ctx.set_theme(if next {
                Theme::dark_default()
            } else {
                Theme::light_default()
            });
        }),
    ))
}

#[derive(Debug)]
struct Root {
    bound_color: Signal<Color>,
    alpha_color: Signal<Color>,
    hex_only_color: Signal<Color>,
    edit_color_a: Signal<Color>,
    edit_color_b: Signal<Color>,
    edit_color_c: Signal<Option<Color>>,
    root_child_id: Option<WidgetId>,
}

impl Root {
    fn new() -> Self {
        Self {
            bound_color: Signal::new(Color::from_hex("#3584E4")),
            alpha_color: Signal::new(Color::from_rgba(0.92, 0.27, 0.18, 0.6)),
            hex_only_color: Signal::new(Color::from_hex("#34A853")),
            edit_color_a: Signal::new(Color::from_hex("#E91E63")),
            edit_color_b: Signal::new(Color::from_rgba(0.13, 0.59, 0.95, 0.5)),
            edit_color_c: Signal::new(None),
            root_child_id: None,
        }
    }
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let _theme = ctx.theme_signal().get();

        let content = VStack::new()
            .spacing(20.0)
            // Heading.
            .child(
                TextWidget::new_literal("ColorPicker gallery")
                    .style(TextStyleRole::BodyBold)
                    .color(TextRole::Primary),
            )
            .child(
                TextWidget::new_literal(
                    "All ColorPicker / HexColorInput / ColorEdit configurations \
                     driving live Signal<Color> sources.",
                )
                .style(TextStyleRole::Body)
                .color(TextRole::Secondary),
            )
            // Section 1 — Standard layout.
            .child(section(
                "Standard layout (default)",
                HStack::new()
                    .spacing(20.0)
                    .child(ColorPicker::new(self.bound_color.clone()))
                    .child(live_preview(self.bound_color.clone())),
            ))
            // Section 2 — With alpha.
            .child(section(
                "With alpha enabled",
                HStack::new()
                    .spacing(20.0)
                    .child(ColorPicker::new(self.alpha_color.clone()).alpha_enabled(true))
                    .child(live_preview(self.alpha_color.clone())),
            ))
            // Section 3 — Compact layout.
            .child(section(
                "Compact layout (popover-friendly)",
                ColorPicker::new(self.bound_color.clone()).layout(ColorPickerLayout::Compact),
            ))
            // Section 4 — Wide layout.
            .child(section(
                "Wide layout",
                ColorPicker::new(self.alpha_color.clone())
                    .alpha_enabled(true)
                    .layout(ColorPickerLayout::Wide)
                    .show_hsv_spinners(true),
            ))
            // Section 5 — HSV spinners only.
            .child(section(
                "HSV spinners only",
                ColorPicker::new(self.bound_color.clone())
                    .show_rgb_spinners(false)
                    .show_hsv_spinners(true),
            ))
            // Section 6 — Custom swatches (Material-flavored).
            .child(section(
                "Custom swatch palette",
                ColorPicker::new(self.bound_color.clone()).swatches(material_palette()),
            ))
            // Section 7 — HexColorInput standalone.
            .child(section(
                "HexColorInput bound to a live Panel background",
                HStack::new()
                    .spacing(12.0)
                    .child(HexColorInput::new(self.hex_only_color.clone()).label("Background"))
                    .child(live_preview(self.hex_only_color.clone())),
            ))
            // Section 8 — ColorEdit row.
            .child(section(
                "ColorEdit (compact trigger + popover)",
                HStack::new()
                    .spacing(12.0)
                    .child(ColorEdit::new(self.edit_color_a.clone()))
                    .child(ColorEdit::new(self.edit_color_b.clone()).alpha_enabled(true))
                    .child(
                        ColorEdit::nullable(self.edit_color_c.clone())
                            .picker_layout(ColorPickerLayout::Standard),
                    ),
            ))
            // Section 9 — Disabled.
            .child(section(
                "Disabled",
                ColorPicker::new(self.bound_color.clone())
                    .layout(ColorPickerLayout::Compact)
                    .enabled(false),
            ));

        let root = ctx.add(ScrollArea::new().child(Padding::uniform(20.0).child(content)));
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        match self.root_child_id {
            Some(id) => ctx
                .child_layout_response(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0).into()),
            None => proposal.resolve(0.0, 0.0).into(),
        }
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

/// A titled section: a small header followed by the section body.
fn section(title: &'static str, body: impl Widget + 'static) -> impl Widget + 'static {
    VStack::new()
        .spacing(8.0)
        .child(
            TextWidget::new_literal(title)
                .style(TextStyleRole::BodyBold)
                .color(TextRole::Primary),
        )
        .child(body)
}

/// A 64×64 panel filled with the bound color — visible feedback that the
/// signal binding works end-to-end.
fn live_preview(color: Signal<Color>) -> impl Widget + 'static {
    use fern_ui::core::ColorProp;
    let bg: ColorProp = color.into();
    Panel::new()
        .background(bg)
        .child(Padding::uniform(20.0).child(TextWidget::new_literal("preview")))
}

fn material_palette() -> Vec<Color> {
    vec![
        Color::from_hex("#F44336"),
        Color::from_hex("#E91E63"),
        Color::from_hex("#9C27B0"),
        Color::from_hex("#673AB7"),
        Color::from_hex("#3F51B5"),
        Color::from_hex("#2196F3"),
        Color::from_hex("#03A9F4"),
        Color::from_hex("#00BCD4"),
        Color::from_hex("#009688"),
        Color::from_hex("#4CAF50"),
        Color::from_hex("#8BC34A"),
        Color::from_hex("#CDDC39"),
        Color::from_hex("#FFEB3B"),
        Color::from_hex("#FFC107"),
        Color::from_hex("#FF9800"),
        Color::from_hex("#FF5722"),
        Color::from_hex("#795548"),
        Color::from_hex("#9E9E9E"),
    ]
}

fn main() {
    FernAppBuilder::new()
        .install_inspector_in_debug()
        .theme(Theme::light_default())
        // Register fern-widgets' own translatable strings so internal
        // labels (Role::Slider names, swatch hex readouts, etc.) resolve
        // instead of falling back to literal Fluent keys.
        .i18n(I18nConfig::new().framework_locales(fern_ui::widgets::framework_locales()))
        .initial_window(
            WindowConfig::new()
                .title("FernUI — ColorPicker gallery")
                .size(960, 900)
                .root(|tree, _state| {
                    tree.add(
                        VStack::new()
                            .child(dark_mode_toolbar())
                            .child(Expand::new().child(Root::new())),
                    )
                }),
        )
        .run();
}
