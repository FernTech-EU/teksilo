// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Theme tab — preset switcher (Light / Dark) + JSON Export / Import +
//! per-color editor.
//!
//! Each shown color carries a draft `Signal<Color>` bound to a
//! [`ColorEdit`] in its row. The picker writes through to the draft on
//! every drag; **Apply** then folds every draft back into the live
//! theme via `ctx.set_theme(...)` in one shot — avoiding 60 Hz thrash
//! while the user is still adjusting sliders. **Reset** re-syncs the
//! drafts to the current theme. The same observer that keeps drafts
//! in lockstep with the active theme also catches preset switches and
//! Import — so "Light" / "Dark" / "Import" all flow through.

use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::Theme;
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::lit;
use teksilo_platform::ClipboardHandle;
use teksilo_tokens::{Color, ColorTokens, ShapeTokens, TextRole, TextStyleRole};
use teksilo_widgets::primitives::{HStack, Padding, Spacer, VStack};
use teksilo_widgets::{Button, ColorEdit, ScrollArea, Slider, TextWidget};

use crate::state::InspectorState;

/// Width of the color-name column. The row's `ColorEdit` claims
/// whatever's left.
const NAME_COLUMN_WIDTH: f32 = 180.0;

/// Curated subset of color tokens we surface in the inspector. The
/// full `ColorTokens` struct has 50+ fields; showing them all would
/// drown the panel. This list covers the colors a developer typically
/// reaches for first. Each entry carries both a getter (read from
/// theme) and a setter (apply draft back into a fresh theme).
type ColorAccess = (
    &'static str,
    fn(&ColorTokens) -> Color,
    fn(&mut ColorTokens, Color),
);

/// f32 token getter / setter pair, used for shape-shadow alphas
/// (read against `ShapeTokens`).
type F32Access<Owner> = (&'static str, fn(&Owner) -> f32, fn(&mut Owner, f32));

const SHOWN_SHADOW_ALPHAS: &[F32Access<ShapeTokens>] = &[
    (
        "shadow_xs.alpha",
        |s| s.shadow_xs.color.a(),
        |s, a| s.shadow_xs.color = s.shadow_xs.color.with_alpha(a),
    ),
    (
        "shadow_sm.alpha",
        |s| s.shadow_sm.color.a(),
        |s, a| s.shadow_sm.color = s.shadow_sm.color.with_alpha(a),
    ),
    (
        "shadow_md.alpha",
        |s| s.shadow_md.color.a(),
        |s, a| s.shadow_md.color = s.shadow_md.color.with_alpha(a),
    ),
    (
        "shadow_lg.alpha",
        |s| s.shadow_lg.color.a(),
        |s, a| s.shadow_lg.color = s.shadow_lg.color.with_alpha(a),
    ),
    (
        "shadow_inner_xs.alpha",
        |s| s.shadow_inner_xs.color.a(),
        |s, a| s.shadow_inner_xs.color = s.shadow_inner_xs.color.with_alpha(a),
    ),
    (
        "shadow_inner_sm.alpha",
        |s| s.shadow_inner_sm.color.a(),
        |s, a| s.shadow_inner_sm.color = s.shadow_inner_sm.color.with_alpha(a),
    ),
    (
        "shadow_inner_md.alpha",
        |s| s.shadow_inner_md.color.a(),
        |s, a| s.shadow_inner_md.color = s.shadow_inner_md.color.with_alpha(a),
    ),
    (
        "shadow_inner_lg.alpha",
        |s| s.shadow_inner_lg.color.a(),
        |s, a| s.shadow_inner_lg.color = s.shadow_inner_lg.color.with_alpha(a),
    ),
];

// `ComponentStyles` was removed; every dimension is now either a
// `pub const` on a `recipe_*_style.rs` module (themable widgets) or
// on the owning widget module (group-4 composites). Per-component
// density rows no longer exist; re-exposing runtime density overrides
// would require per-recipe-instance overrides on the active style
// trait object.

const SHOWN_COLORS: &[ColorAccess] = &[
    ("accent", |t| t.accent, |t, c| t.accent = c),
    (
        "accent_hover",
        |t| t.accent_hover,
        |t, c| t.accent_hover = c,
    ),
    (
        "surface_main",
        |t| t.surface_main,
        |t, c| t.surface_main = c,
    ),
    (
        "surface_content",
        |t| t.surface_content,
        |t, c| t.surface_content = c,
    ),
    (
        "surface_hover",
        |t| t.surface_hover,
        |t, c| t.surface_hover = c,
    ),
    (
        "surface_selected",
        |t| t.surface_selected,
        |t, c| t.surface_selected = c,
    ),
    (
        "text_primary",
        |t| t.text_primary,
        |t, c| t.text_primary = c,
    ),
    (
        "text_secondary",
        |t| t.text_secondary,
        |t, c| t.text_secondary = c,
    ),
    (
        "text_disabled",
        |t| t.text_disabled,
        |t, c| t.text_disabled = c,
    ),
    ("text_link", |t| t.text_link, |t, c| t.text_link = c),
    ("border", |t| t.border, |t, c| t.border = c),
    (
        "border_focused",
        |t| t.border_focused,
        |t, c| t.border_focused = c,
    ),
    ("focus_ring", |t| t.focus_ring, |t, c| t.focus_ring = c),
    (
        "status_error_fg",
        |t| t.status_error_fg,
        |t, c| t.status_error_fg = c,
    ),
    (
        "status_warning_fg",
        |t| t.status_warning_fg,
        |t, c| t.status_warning_fg = c,
    ),
    (
        "status_success_fg",
        |t| t.status_success_fg,
        |t, c| t.status_success_fg = c,
    ),
];

pub(crate) struct ThemeTab {
    state: InspectorState,
    root_child_id: Option<WidgetId>,
}

impl ThemeTab {
    pub fn new(state: InspectorState) -> Self {
        Self {
            state,
            root_child_id: None,
        }
    }
}

impl std::fmt::Debug for ThemeTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThemeTab").finish()
    }
}

/// Section divider (a small bold heading) to separate Colors / Shape /
/// Components groups inside the tab's scroll body.
fn section_header(text: &str) -> impl Widget {
    Padding::new(8.0, 0.0, 4.0, 0.0).child(
        TextWidget::new(lit!(text))
            .style(TextStyleRole::BodyBold)
            .color(TextRole::Primary),
    )
}

/// One row: name in a fixed-width column on the left, a 0..=1 slider on
/// the right, with the current value rendered after the slider so the
/// developer can read the exact alpha / density without dragging.
fn slider_row(name: &'static str, draft: Signal<f32>) -> impl Widget {
    let value_text = draft.map(|v| format!("{:.2}", v));
    let name_text = TextWidget::new(lit!(name))
        .style(TextStyleRole::Body)
        .color(TextRole::Primary);
    let value_label = TextWidget::new(lit!(""))
        .text(value_text)
        .style(TextStyleRole::Body)
        .color(TextRole::Secondary);
    HStack::new()
        .spacing(8.0)
        .child(
            teksilo_widgets::primitives::FixedSize::new()
                .width(Signal::new(NAME_COLUMN_WIDTH))
                .child(name_text),
        )
        .child(Spacer::new())
        .child(Slider::new(draft, 0.0, 1.0).step(0.01))
        .child(
            teksilo_widgets::primitives::FixedSize::new()
                .width(Signal::new(40.0_f32))
                .child(value_label),
        )
}

impl Widget for ThemeTab {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let _ = &self.state;
        let theme_sig = ctx.theme_signal();
        let initial_theme = theme_sig.get();

        // One draft signal per editable color. Initialized from the
        // active theme; re-synced whenever the active theme changes
        // externally (Light / Dark / Import / Apply commit).
        let drafts: Vec<Signal<Color>> = SHOWN_COLORS
            .iter()
            .map(|(_, get, _)| Signal::new(get(&initial_theme.colors)))
            .collect();
        let alpha_drafts: Vec<Signal<f32>> = SHOWN_SHADOW_ALPHAS
            .iter()
            .map(|(_, get, _)| Signal::new(get(&initial_theme.shape)))
            .collect();

        // Theme → drafts bridge. Fires after every successful
        // `ctx.set_theme(...)`. Without this the row swatches would
        // freeze at the values the user last edited even after a
        // preset switch.
        {
            let drafts = drafts.clone();
            let alpha_drafts = alpha_drafts.clone();
            let h = theme_sig.observe(move |theme| {
                for ((_, get, _), sig) in SHOWN_COLORS.iter().zip(&drafts) {
                    let v = get(&theme.colors);
                    if sig.get() != v {
                        sig.set(v);
                    }
                }
                for ((_, get, _), sig) in SHOWN_SHADOW_ALPHAS.iter().zip(&alpha_drafts) {
                    let v = get(&theme.shape);
                    if (sig.get() - v).abs() > f32::EPSILON {
                        sig.set(v);
                    }
                }
            });
            theme_sig.attach_keepalive(h);
        }

        // Preset buttons.
        let light_btn = Button::new(lit!("Light"))
            .on_activate_fn(|c| c.set_theme(teksilo_core::presets::intui::light()));
        let dark_btn = Button::new(lit!("Dark"))
            .on_activate_fn(|c| c.set_theme(teksilo_core::presets::intui::dark()));

        // Apply: fold every draft back into a fresh theme and commit.
        let drafts_for_apply = drafts.clone();
        let alpha_drafts_for_apply = alpha_drafts.clone();
        let theme_for_apply = theme_sig.clone();
        let apply_btn = Button::new(lit!("Apply")).on_activate_fn(move |c| {
            let mut next = theme_for_apply.get();
            for ((_, _, set), sig) in SHOWN_COLORS.iter().zip(&drafts_for_apply) {
                set(&mut next.colors, sig.get());
            }
            for ((_, _, set), sig) in SHOWN_SHADOW_ALPHAS.iter().zip(&alpha_drafts_for_apply) {
                set(&mut next.shape, sig.get());
            }
            c.set_theme(next);
        });

        // Reset: discard pending drafts by re-reading the active theme.
        let drafts_for_reset = drafts.clone();
        let alpha_drafts_for_reset = alpha_drafts.clone();
        let theme_for_reset = theme_sig.clone();
        let reset_btn = Button::new(lit!("Reset")).on_activate_fn(move |_c| {
            let theme = theme_for_reset.get();
            for ((_, get, _), sig) in SHOWN_COLORS.iter().zip(&drafts_for_reset) {
                let v = get(&theme.colors);
                if sig.get() != v {
                    sig.set(v);
                }
            }
            for ((_, get, _), sig) in SHOWN_SHADOW_ALPHAS.iter().zip(&alpha_drafts_for_reset) {
                let v = get(&theme.shape);
                if (sig.get() - v).abs() > f32::EPSILON {
                    sig.set(v);
                }
            }
        });

        // Export → JSON dump → clipboard.
        let theme_for_export = theme_sig.clone();
        let export_btn = Button::new(lit!("Export")).on_activate_fn(move |c| {
            if let Some(cb) = c.app_state::<ClipboardHandle>() {
                let theme = theme_for_export.get();
                if let Ok(json) = serde_json::to_string_pretty(&theme) {
                    let _ = cb.set_text(&json);
                }
            }
        });

        // Import ← clipboard JSON → set_theme. Silently ignores parse
        // errors (a debug tool — the developer can check the clipboard
        // and try again).
        let import_btn = Button::new(lit!("Import")).on_activate_fn(|c| {
            let Some(cb) = c.app_state::<ClipboardHandle>() else {
                return;
            };
            let Ok(text) = cb.get_text() else {
                return;
            };
            if let Ok(theme) = serde_json::from_str::<Theme>(&text) {
                c.set_theme(theme);
            }
        });

        let toolbar = Padding::symmetric(2.0, 4.0).child(
            HStack::new()
                .spacing(6.0)
                .child(light_btn)
                .child(dark_btn)
                .child(apply_btn)
                .child(reset_btn)
                .child(export_btn)
                .child(import_btn),
        );

        // One row per editable color. The name sits in a fixed-width
        // column; the `Spacer` pushes the `ColorEdit` to the trailing
        // edge of the row. HStack centers children vertically by
        // default so the swatch and the name stay aligned.
        let mut rows = VStack::new().spacing(2.0);
        for ((name, _, _), sig) in SHOWN_COLORS.iter().zip(&drafts) {
            let name_text = TextWidget::new(lit!(*name))
                .style(TextStyleRole::Body)
                .color(TextRole::Primary);
            let row = HStack::new()
                .spacing(8.0)
                .child(
                    teksilo_widgets::primitives::FixedSize::new()
                        .width(Signal::new(NAME_COLUMN_WIDTH))
                        .child(name_text),
                )
                .child(Spacer::new())
                .child(ColorEdit::new(sig.clone()).alpha_enabled(true));
            rows = rows.child(row);
        }

        // Shape — shadow alphas (outer + inner pair per scale).
        rows = rows.child(section_header("Shape — shadow alphas"));
        for ((name, _, _), sig) in SHOWN_SHADOW_ALPHAS.iter().zip(&alpha_drafts) {
            rows = rows.child(slider_row(name, sig.clone()));
        }

        // Per-component density rows are not shown — `ComponentStyles`
        // was removed; see the note above `SHOWN_COLORS`.

        let root = ctx.add(
            VStack::new()
                .spacing(4.0)
                .child(toolbar)
                .child(ScrollArea::new().child(rows)),
        );
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .map(LayoutResponse::from)
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0).into())
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for c in children.iter_mut() {
            c.origin = teksilo_canvas::Point::new(bounds.x, bounds.y);
            c.size = teksilo_canvas::Size::new(bounds.width, bounds.height);
        }
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}
