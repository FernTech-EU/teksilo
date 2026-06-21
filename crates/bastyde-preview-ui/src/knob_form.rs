// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Knob form generator — turns a `KnobSpec` plus a runtime
//! `KnobValues` into a column of editor rows that drive the runtime
//! signals on user input.
//!
//! The mapping kind → editor widget:
//!
//! | `KnobKind`        | Editor widget                                 |
//! |-------------------|-----------------------------------------------|
//! | `Bool`            | `Toggle`                                      |
//! | `OptBool`         | enable `Checkbox` + `Toggle`                  |
//! | `I32` / `F32`     | `Slider` + numeric label                      |
//! | `OptI32`/`OptF32` | enable `Checkbox` + `Slider`                  |
//! | `Text`            | `TextInput`                                   |
//! | `OptText`         | enable `Checkbox` + `TextInput`               |
//! | `Choice` (≤4)     | `SegmentedControl`                            |
//! | `Choice` (>4)     | `ComboBox` of strings                         |
//! | `TextRole` …      | `ComboBox` of role labels                     |
//!
//! Numeric knobs require a small bridge: `Slider` always operates on
//! `Signal<f32>`, so for `I32` knobs we create an auxiliary
//! `Signal<f32>` and observe it to back-write `i32` values into the
//! original signal. The reverse direction (someone else writes the
//! `i32` signal) is rare here — knob signals are written only by the
//! form — so we omit it. Equality checks in the observer prevent
//! self-feedback loops.

use bastyde_i18n::lit;
use std::rc::Rc;

use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::widget_id::WidgetId;
use bastyde_preview::{KnobDecl, KnobKind, KnobSpec, KnobValues};
use bastyde_tokens::{BorderRole, SurfaceRole, TextRole, TextStyleRole};
use bastyde_widgets::primitives::Padding;
use bastyde_widgets::{
    Checkbox, ComboBox, HStack, MaxSize, SegmentedControl, Slider, TextInput, TextWidget, Toggle,
    VStack,
};

/// Build the form for `(spec, values)`. Returns a single `WidgetId`
/// for the column of rows. Knobs that share a `group` are clustered
/// under a small group header.
pub fn build_knob_form(ctx: &mut BuildContext, spec: &KnobSpec, values: &KnobValues) -> WidgetId {
    let mut column = VStack::new().spacing(8.0);
    let mut current_group: Option<&'static str> = None;
    for decl in spec.declarations() {
        if decl.group != current_group {
            if let Some(label) = decl.group {
                let header = TextWidget::new(lit!(label))
                    .style(TextStyleRole::Tiny)
                    .color(TextRole::Secondary);
                column = column.child(Padding::new(8.0, 0.0, 2.0, 0.0).child(header));
            }
            current_group = decl.group;
        }
        let row_widget = build_row(ctx, decl, values);
        column = column.add_child(row_widget);
    }
    ctx.add(column)
}

fn build_row(ctx: &mut BuildContext, decl: &KnobDecl, values: &KnobValues) -> WidgetId {
    let editor = build_editor(ctx, decl, values);
    let label = TextWidget::new(lit!(decl.label))
        .style(TextStyleRole::Small)
        .color(TextRole::Secondary)
        .single_line();
    let label_box = MaxSize::new(110.0, f32::INFINITY).child(label);
    let editor_widget = MaxSize::new(f32::INFINITY, f32::INFINITY).child_id(editor);
    let row = HStack::new()
        .spacing(8.0)
        .child(label_box)
        .child(editor_widget);
    ctx.add(row)
}

fn build_editor(ctx: &mut BuildContext, decl: &KnobDecl, values: &KnobValues) -> WidgetId {
    match &decl.kind {
        KnobKind::Bool { .. } => {
            // Toggle requires a non-empty `label` (debug_assert! in
            // its `accessibility()`). The form already shows a row
            // label to the left, but the Toggle itself also needs an
            // a11y label. Pass `decl.label` — visually duplicated for
            // sighted users but correct for screen readers.
            let sig = values.bool_(decl.id);
            ctx.add(Toggle::new(sig).label(lit!(decl.label)))
        }
        KnobKind::OptBool { .. } => build_opt_bool(ctx, decl, values),
        KnobKind::I32 { min, max, .. } => build_i32(ctx, decl, values, *min, *max),
        KnobKind::OptI32 { min, max, .. } => build_opt_i32(ctx, decl, values, *min, *max),
        KnobKind::F32 { min, max, .. } => build_f32(ctx, decl, values, *min, *max),
        KnobKind::OptF32 { min, max, .. } => build_opt_f32(ctx, decl, values, *min, *max),
        KnobKind::Text { .. } => {
            let sig = values.text(decl.id);
            ctx.add(TextInput::new(sig))
        }
        KnobKind::OptText { .. } => build_opt_text(ctx, decl, values),
        KnobKind::Choice { options, .. } => build_choice(ctx, decl, values, options),
        // `Enum` is stored at runtime as a `usize` index just like `Choice`
        // (see KnobKind docs), so it renders with the same segmented/combo
        // editor over its variant idents.
        KnobKind::Enum { variants, .. } => build_choice(ctx, decl, values, variants),
        KnobKind::TextRole { .. } => build_text_role(ctx, decl, values),
        KnobKind::SurfaceRole { .. } => build_surface_role(ctx, decl, values),
        KnobKind::BorderRole { .. } => build_border_role(ctx, decl, values),
        KnobKind::TextStyle { .. } => build_text_style(ctx, decl, values),
    }
}

// ---------------------------------------------------------------------------
// numeric / optional editors
// ---------------------------------------------------------------------------

fn build_i32(
    ctx: &mut BuildContext,
    decl: &KnobDecl,
    values: &KnobValues,
    min: i32,
    max: i32,
) -> WidgetId {
    let int_sig = values.i32_(decl.id);
    let f_sig = Signal::new(int_sig.get() as f32);
    {
        let int_sig = int_sig.clone();
        let h = f_sig.observe(move |v| {
            let target = *v as i32;
            if int_sig.get() != target {
                int_sig.set(target);
            }
        });
        ctx.own_handle(h);
    }
    let value_label = int_sig.map(|v| format!("{}", *v));
    let slider = Slider::new(f_sig, min as f32, max as f32);
    let label = TextWidget::new(lit!(""))
        .style(TextStyleRole::Small)
        .color(TextRole::Secondary)
        .single_line()
        .bind_text(value_label);
    ctx.add(
        HStack::new()
            .spacing(8.0)
            .child(slider)
            .child(MaxSize::new(40.0, f32::INFINITY).child(label)),
    )
}

fn build_f32(
    ctx: &mut BuildContext,
    decl: &KnobDecl,
    values: &KnobValues,
    min: f32,
    max: f32,
) -> WidgetId {
    let sig = values.f32_(decl.id);
    let value_label = sig.map(|v| format!("{:.2}", *v));
    let slider = Slider::new(sig, min, max);
    let label = TextWidget::new(lit!(""))
        .style(TextStyleRole::Small)
        .color(TextRole::Secondary)
        .single_line()
        .bind_text(value_label);
    ctx.add(
        HStack::new()
            .spacing(8.0)
            .child(slider)
            .child(MaxSize::new(56.0, f32::INFINITY).child(label)),
    )
}

fn build_opt_bool(ctx: &mut BuildContext, decl: &KnobDecl, values: &KnobValues) -> WidgetId {
    let opt_sig = values.opt_bool(decl.id);
    let enabled = Signal::new(opt_sig.get().is_some());
    let inner = Signal::new(opt_sig.get().unwrap_or(false));
    bridge_optional(ctx, &enabled, &inner, &opt_sig, |v: bool| v);
    ctx.add(
        HStack::new()
            .spacing(8.0)
            .child(Checkbox::new(enabled).label(lit!("Enabled")))
            .child(Toggle::new(inner).label(lit!(decl.label))),
    )
}

fn build_opt_i32(
    ctx: &mut BuildContext,
    decl: &KnobDecl,
    values: &KnobValues,
    min: i32,
    max: i32,
) -> WidgetId {
    let opt_sig = values.opt_i32(decl.id);
    let enabled = Signal::new(opt_sig.get().is_some());
    let inner_f = Signal::new(opt_sig.get().unwrap_or(0) as f32);
    bridge_optional(ctx, &enabled, &inner_f, &opt_sig, |v: f32| v as i32);
    ctx.add(
        HStack::new()
            .spacing(8.0)
            .child(Checkbox::new(enabled).label(lit!("Enabled")))
            .child(Slider::new(inner_f, min as f32, max as f32)),
    )
}

fn build_opt_f32(
    ctx: &mut BuildContext,
    decl: &KnobDecl,
    values: &KnobValues,
    min: f32,
    max: f32,
) -> WidgetId {
    let opt_sig = values.opt_f32(decl.id);
    let enabled = Signal::new(opt_sig.get().is_some());
    let inner = Signal::new(opt_sig.get().unwrap_or(0.0));
    bridge_optional(ctx, &enabled, &inner, &opt_sig, |v: f32| v);
    ctx.add(
        HStack::new()
            .spacing(8.0)
            .child(Checkbox::new(enabled).label(lit!("Enabled")))
            .child(Slider::new(inner, min, max)),
    )
}

fn build_opt_text(ctx: &mut BuildContext, decl: &KnobDecl, values: &KnobValues) -> WidgetId {
    let opt_sig = values.opt_text(decl.id);
    let enabled = Signal::new(opt_sig.get().is_some());
    let inner = Signal::new(opt_sig.get().unwrap_or_default());
    {
        let opt = opt_sig.clone();
        let inner_c = inner.clone();
        let h = enabled.observe(move |on| {
            if *on {
                opt.set(Some(inner_c.get()));
            } else {
                opt.set(None);
            }
        });
        ctx.own_handle(h);
        let opt2 = opt_sig.clone();
        let h2 = inner.observe(move |v| {
            if opt2.get().is_some() {
                opt2.set(Some(v.clone()));
            }
        });
        ctx.own_handle(h2);
    }
    ctx.add(
        HStack::new()
            .spacing(8.0)
            .child(Checkbox::new(enabled).label(lit!("Enabled")))
            .child(TextInput::new(inner)),
    )
}

/// Wire the `(enabled, inner)` pair of signals onto an `Option<U>`
/// signal. When `enabled` is true, mutations to `inner` propagate
/// (mapped through `to_outer`); when it is false, the outer becomes
/// `None`.
fn bridge_optional<I, U>(
    ctx: &mut BuildContext,
    enabled: &Signal<bool>,
    inner: &Signal<I>,
    outer: &Signal<Option<U>>,
    to_outer: impl Fn(I) -> U + 'static,
) where
    I: Clone + 'static,
    U: Clone + PartialEq + 'static,
{
    let to_outer = Rc::new(to_outer);
    {
        let outer = outer.clone();
        let inner_c = inner.clone();
        let to_outer = to_outer.clone();
        let h = enabled.observe(move |on| {
            if *on {
                outer.set(Some((to_outer)(inner_c.get())));
            } else {
                outer.set(None);
            }
        });
        ctx.own_handle(h);
    }
    {
        let outer = outer.clone();
        let to_outer = to_outer.clone();
        let h = inner.observe(move |v| {
            if outer.get().is_some() {
                outer.set(Some((to_outer)(v.clone())));
            }
        });
        ctx.own_handle(h);
    }
}

// ---------------------------------------------------------------------------
// choice / enum editors
// ---------------------------------------------------------------------------

fn build_choice(
    ctx: &mut BuildContext,
    decl: &KnobDecl,
    values: &KnobValues,
    options: &[&'static str],
) -> WidgetId {
    let sig = values.choice(decl.id);
    // Clamp a stale override index (e.g. a persisted choice from when the knob
    // had more options) into range, so neither the SegmentedControl nor the
    // ComboBox below renders a blank/unselected control.
    if !options.is_empty() && sig.get() >= options.len() {
        sig.set(options.len() - 1);
    }
    if options.len() <= 4 {
        ctx.add(SegmentedControl::new(sig).segments(options.iter().map(|s| lit!(s.to_string()))))
    } else {
        let items: Vec<String> = options.iter().map(|s| s.to_string()).collect();
        let initial = items.get(sig.get()).cloned();
        let combo_sel: Signal<Option<String>> = Signal::new(initial);
        let items_obs = items.clone();
        let sig_w = sig.clone();
        let h = combo_sel.observe(move |selected| {
            if let Some(s) = selected
                && let Some(idx) = items_obs.iter().position(|o| o == s)
                && sig_w.get() != idx
            {
                sig_w.set(idx);
            }
        });
        ctx.own_handle(h);
        let items_obs2 = items.clone();
        let combo_w = combo_sel.clone();
        let h2 = sig.observe(move |idx| {
            let want = items_obs2.get(*idx).cloned();
            if combo_w.get() != want {
                combo_w.set(want);
            }
        });
        ctx.own_handle(h2);
        ctx.add(ComboBox::new(items, combo_sel))
    }
}

fn build_enum_combo<E>(
    ctx: &mut BuildContext,
    sig: Signal<E>,
    options: Vec<(E, &'static str)>,
) -> WidgetId
where
    E: Clone + PartialEq + 'static,
{
    let labels: Vec<String> = options.iter().map(|(_, l)| l.to_string()).collect();
    let lookup_label = {
        let opts = options.clone();
        move |e: &E| {
            opts.iter()
                .find(|(o, _)| o == e)
                .map(|(_, l)| l.to_string())
        }
    };
    let lookup_value = {
        let opts = options.clone();
        move |label: &str| -> Option<E> {
            opts.iter()
                .find(|(_, l)| *l == label)
                .map(|(e, _)| e.clone())
        }
    };
    let initial = lookup_label(&sig.get());
    let combo_sel: Signal<Option<String>> = Signal::new(initial);
    {
        let sig_w = sig.clone();
        let lookup_value = Rc::new(lookup_value);
        let h = combo_sel.observe(move |selected| {
            if let Some(label) = selected
                && let Some(value) = (lookup_value)(label)
                && sig_w.get() != value
            {
                sig_w.set(value);
            }
        });
        ctx.own_handle(h);
    }
    {
        let combo_w = combo_sel.clone();
        let lookup_label = Rc::new(lookup_label);
        let h = sig.observe(move |v| {
            let want = (lookup_label)(v);
            if combo_w.get() != want {
                combo_w.set(want);
            }
        });
        ctx.own_handle(h);
    }
    ctx.add(ComboBox::new(labels, combo_sel))
}

fn build_text_role(ctx: &mut BuildContext, decl: &KnobDecl, values: &KnobValues) -> WidgetId {
    let sig = values.text_role(decl.id);
    build_enum_combo(
        ctx,
        sig,
        vec![
            (TextRole::Primary, "Primary"),
            (TextRole::Secondary, "Secondary"),
            (TextRole::Disabled, "Disabled"),
            (TextRole::OnAccent, "OnAccent"),
            (TextRole::Accent, "Accent"),
            (TextRole::Error, "Error"),
            (TextRole::Warning, "Warning"),
            (TextRole::Success, "Success"),
            (TextRole::Link, "Link"),
            (TextRole::LinkHover, "LinkHover"),
            (TextRole::TooltipText, "TooltipText"),
            (TextRole::TooltipShortcut, "TooltipShortcut"),
            (TextRole::EditorFg, "EditorFg"),
            (TextRole::EditorGutterFg, "EditorGutterFg"),
        ],
    )
}

fn build_surface_role(ctx: &mut BuildContext, decl: &KnobDecl, values: &KnobValues) -> WidgetId {
    let sig = values.surface_role(decl.id);
    build_enum_combo(
        ctx,
        sig,
        vec![
            (SurfaceRole::Main, "Main"),
            (SurfaceRole::Content, "Content"),
            (SurfaceRole::Raised, "Raised"),
            (SurfaceRole::Sunken, "Sunken"),
            (SurfaceRole::Hover, "Hover"),
            (SurfaceRole::Pressed, "Pressed"),
            (SurfaceRole::Selected, "Selected"),
            (SurfaceRole::SelectedInactive, "SelectedInactive"),
            (SurfaceRole::Accent, "Accent"),
            (SurfaceRole::AccentHover, "AccentHover"),
            (SurfaceRole::AccentPressed, "AccentPressed"),
            (SurfaceRole::AccentDisabled, "AccentDisabled"),
            (SurfaceRole::AccentSubtle, "AccentSubtle"),
            (SurfaceRole::Transparent, "Transparent"),
        ],
    )
}

fn build_border_role(ctx: &mut BuildContext, decl: &KnobDecl, values: &KnobValues) -> WidgetId {
    let sig = values.border_role(decl.id);
    build_enum_combo(
        ctx,
        sig,
        vec![
            (BorderRole::Default, "Default"),
            (BorderRole::Strong, "Strong"),
            (BorderRole::Focused, "Focused"),
            (BorderRole::Error, "Error"),
            (BorderRole::Warning, "Warning"),
            (BorderRole::Divider, "Divider"),
            (BorderRole::DividerStrong, "DividerStrong"),
            (BorderRole::Accent, "Accent"),
            (BorderRole::AccentDisabled, "AccentDisabled"),
            (BorderRole::Transparent, "Transparent"),
        ],
    )
}

fn build_text_style(ctx: &mut BuildContext, decl: &KnobDecl, values: &KnobValues) -> WidgetId {
    let sig = values.text_style(decl.id);
    build_enum_combo(
        ctx,
        sig,
        vec![
            (TextStyleRole::Body, "Body"),
            (TextStyleRole::BodyBold, "BodyBold"),
            (TextStyleRole::Small, "Small"),
            (TextStyleRole::SmallBold, "SmallBold"),
            (TextStyleRole::Tiny, "Tiny"),
            (TextStyleRole::Mono, "Mono"),
        ],
    )
}
