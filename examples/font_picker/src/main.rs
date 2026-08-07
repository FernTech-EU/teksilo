// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! FontPicker demo — pick a font, filter the list, preview it live.
//!
//! Run with: `cargo run -p font-picker` (add `--release` for snappy
//! shaping of the per-row font samples).
//!
//! What's on screen:
//!
//! - A [`FontPicker`] listing every installed font family. Each row shows
//!   the family name in a legible UI font next to a **sample rendered in
//!   that font** (script-aware — a Cyrillic font previews Cyrillic, an
//!   Arabic font previews Arabic, …); the closed control shows the selected
//!   family in its own typeface.
//! - A **Monospace only** checkbox, bound to the picker's spacing filter.
//! - A **writing system** selector, bound to the picker's writing-system
//!   filter — restricting the list to fonts that cover the chosen script.
//!   (The first time you pick a script, the picker builds its coverage
//!   index on a background thread; the list narrows once it's ready, and
//!   the UI never blocks.)
//! - A **live preview** paragraph rendered in the selected font.

use teksilo::core::{BindingLevel, WidgetPlacement};
use teksilo::prelude::*;
use teksilo::text::WritingSystem;
use teksilo::tokens::TextStyle;
use teksilo::widgets::{
    Checkbox, ComboBox, Expand, FontPicker, FontSpacingFilter, HStack, Padding, Panel, Spacer,
    TextWidget, ThemeSwitcher, Toolbar, VStack,
};

const PANGRAM: &str = "The quick brown fox jumps over the lazy dog. — 0123456789 ?!&@";

/// The writing systems offered by the demo's script selector, paired with
/// their display label. `None` = no filter.
fn writing_systems() -> Vec<(String, Option<WritingSystem>)> {
    vec![
        ("(all scripts)".to_string(), None),
        ("Latin".to_string(), Some(WritingSystem::Latin)),
        ("Greek".to_string(), Some(WritingSystem::Greek)),
        ("Cyrillic".to_string(), Some(WritingSystem::Cyrillic)),
        ("Arabic".to_string(), Some(WritingSystem::Arabic)),
        ("Hebrew".to_string(), Some(WritingSystem::Hebrew)),
        ("Devanagari".to_string(), Some(WritingSystem::Devanagari)),
        ("Thai".to_string(), Some(WritingSystem::Thai)),
        (
            "Chinese (Simplified)".to_string(),
            Some(WritingSystem::SimplifiedChinese),
        ),
        ("Japanese".to_string(), Some(WritingSystem::Japanese)),
        ("Korean".to_string(), Some(WritingSystem::Korean)),
    ]
}

/// A paragraph rendered in whichever font is currently selected. Rebuilds
/// itself when the bound `family` signal changes.
#[derive(Debug)]
struct FontPreview {
    family: Signal<Option<String>>,
    child: Option<WidgetId>,
}

impl FontPreview {
    fn new(family: Signal<Option<String>>) -> Self {
        Self {
            family,
            child: None,
        }
    }
}

impl Widget for FontPreview {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        self.family
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        let base = ctx.theme().typography.body.clone();
        let style = match self.family.get() {
            Some(family) => TextStyle {
                family,
                size: 22.0,
                ..base
            },
            None => TextStyle { size: 22.0, ..base },
        };
        let child = ctx.add(TextWidget::new(lit!(PANGRAM)).style(style));
        self.child = Some(child);
        vec![child]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.child
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
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
        self.child.into_iter().collect()
    }
}

#[derive(Debug)]
struct Root {
    family: Signal<Option<String>>,
    monospace_only: Signal<bool>,
    writing_system: Signal<Option<WritingSystem>>,
    root: Option<WidgetId>,
}

impl Root {
    fn new() -> Self {
        Self {
            family: Signal::new(None),
            monospace_only: Signal::new(false),
            writing_system: Signal::new(None),
            root: None,
        }
    }
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // The spacing filter follows the "Monospace only" checkbox.
        let spacing = self.monospace_only.map(|on| {
            if *on {
                FontSpacingFilter::Monospaced
            } else {
                FontSpacingFilter::Any
            }
        });

        let picker = FontPicker::new(self.family.clone())
            .label(lit!("Font family"))
            .spacing_filter(spacing)
            .writing_system(self.writing_system.clone());

        // Writing-system selector: a ComboBox of script labels that writes
        // the picker's writing-system filter on select.
        let systems = writing_systems();
        let ws_selected: Signal<Option<String>> = Signal::new(Some(systems[0].0.clone()));
        let ws_lookup = systems.clone();
        let ws_signal = self.writing_system.clone();
        let ws_combo = ComboBox::new(systems.iter().map(|(label, _)| label.clone()), ws_selected)
            .label(lit!("Writing system"))
            .on_select(move |label: &String, _ctx| {
                let ws = ws_lookup
                    .iter()
                    .find(|(l, _)| l == label)
                    .and_then(|(_, ws)| *ws);
                ws_signal.set(ws);
            });

        let mono = Checkbox::new(self.monospace_only.clone()).label(lit!("Monospace only"));

        let controls = Panel::new().child(
            Padding::symmetric(16.0, 16.0).child(
                VStack::new()
                    .spacing(12.0)
                    .child(
                        TextWidget::new(lit!("Font"))
                            .style(TextStyleRole::BodyBold)
                            .color(TextRole::Primary),
                    )
                    .child(picker)
                    .child(
                        HStack::new()
                            .spacing(16.0)
                            .child(mono)
                            .child(
                                HStack::new()
                                    .spacing(8.0)
                                    .child(
                                        TextWidget::new(lit!("Script:")).color(TextRole::Secondary),
                                    )
                                    .child(ws_combo),
                            )
                            .child(Spacer::new()),
                    ),
            ),
        );

        let preview = Panel::new().child(
            Padding::symmetric(16.0, 16.0).child(
                VStack::new()
                    .spacing(8.0)
                    .child(
                        TextWidget::new(lit!("Preview"))
                            .style(TextStyleRole::SmallBold)
                            .color(TextRole::Accent),
                    )
                    .child(FontPreview::new(self.family.clone())),
            ),
        );

        let root = ctx.add(
            Padding::symmetric(20.0, 20.0).child(
                VStack::new()
                    .spacing(20.0)
                    .child(controls)
                    .child(preview)
                    .child(Spacer::new()),
            ),
        );
        self.root = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
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
        self.root.into_iter().collect()
    }
}

fn theme_toolbar() -> impl Widget {
    teksu!(
        Toolbar {
            HStack {
                Spacer
                ThemeSwitcher::new()
            }
        }
    )
}

fn main() {
    TeksiloAppBuilder::new()
        .install_inspector_in_debug()
        .theme(teksilo::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Teksilo — FontPicker")
                .size(720, 620)
                .root(|tree, _state| {
                    teksu!(tree => VStack {
                            child: theme_toolbar()
                            Expand {
                                Root::new()
                            }
                        }
                    )
                }),
        )
        .run();
}
