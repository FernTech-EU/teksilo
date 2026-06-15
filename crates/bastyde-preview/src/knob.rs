// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Knob specs and knob value containers.
//!
//! `KnobSpec` is a static, declarative description of which properties
//! a widget exposes for live tweaking — authored in `WidgetCatalog::knobs()`.
//! `KnobValues` is the runtime container holding one typed `Signal<T>` per
//! declared knob; the previewer constructs it from a spec and threads
//! signals into the displayed widget via `Prop::Bound`.
//!
//! Each knob `id` is a stable, ASCII string used both for spec lookup
//! and for variant override application. Accessors panic on a typo or
//! kind mismatch — this is developer-facing tooling, panic on misuse
//! is the right policy.

use std::collections::HashMap;

use bastyde_core::signal::Signal;
use bastyde_tokens::{BorderRole, SurfaceRole, TextRole, TextStyleRole};

// ---------------------------------------------------------------------------
// Spec — declarative description authored by the widget
// ---------------------------------------------------------------------------

/// A typed knob declaration. Each kind maps 1:1 to a `Signal<T>` in
/// `KnobValues` and to one row in the inspector's auto-generated form.
#[derive(Debug, Clone)]
pub enum KnobKind {
    Bool {
        default: bool,
    },
    OptBool {
        default: Option<bool>,
    },
    I32 {
        default: i32,
        min: i32,
        max: i32,
        step: i32,
    },
    OptI32 {
        default: Option<i32>,
        min: i32,
        max: i32,
        step: i32,
    },
    F32 {
        default: f32,
        min: f32,
        max: f32,
        step: f32,
    },
    OptF32 {
        default: Option<f32>,
        min: f32,
        max: f32,
        step: f32,
    },
    Text {
        default: String,
    },
    OptText {
        default: Option<String>,
    },
    /// Position-based selection over a fixed list of labels.
    Choice {
        options: Vec<&'static str>,
        default: usize,
    },
    TextRole {
        default: TextRole,
    },
    SurfaceRole {
        default: SurfaceRole,
    },
    BorderRole {
        default: BorderRole,
    },
    TextStyle {
        default: TextStyleRole,
    },
}

#[derive(Debug, Clone)]
pub struct KnobDecl {
    pub id: &'static str,
    pub label: &'static str,
    pub group: Option<&'static str>,
    pub kind: KnobKind,
}

/// Ordered list of knob declarations. Order is preserved by the
/// inspector when rendering the form.
#[derive(Debug, Clone, Default)]
pub struct KnobSpec {
    decls: Vec<KnobDecl>,
}

impl KnobSpec {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn empty() -> Self {
        Self::default()
    }

    pub fn declarations(&self) -> &[KnobDecl] {
        &self.decls
    }

    /// Look up a knob declaration by id.
    pub fn get(&self, id: &str) -> Option<&KnobDecl> {
        self.decls.iter().find(|d| d.id == id)
    }

    fn push(mut self, decl: KnobDecl) -> Self {
        debug_assert!(
            !self.decls.iter().any(|d| d.id == decl.id),
            "duplicate knob id: {}",
            decl.id
        );
        self.decls.push(decl);
        self
    }

    pub fn bool_(self, id: &'static str, label: &'static str, default: bool) -> Self {
        self.push(KnobDecl {
            id,
            label,
            group: None,
            kind: KnobKind::Bool { default },
        })
    }

    pub fn opt_bool(self, id: &'static str, label: &'static str, default: Option<bool>) -> Self {
        self.push(KnobDecl {
            id,
            label,
            group: None,
            kind: KnobKind::OptBool { default },
        })
    }

    pub fn i32_(
        self,
        id: &'static str,
        label: &'static str,
        default: i32,
        min: i32,
        max: i32,
    ) -> Self {
        self.push(KnobDecl {
            id,
            label,
            group: None,
            kind: KnobKind::I32 {
                default,
                min,
                max,
                step: 1,
            },
        })
    }

    pub fn f32_(
        self,
        id: &'static str,
        label: &'static str,
        default: f32,
        min: f32,
        max: f32,
    ) -> Self {
        self.push(KnobDecl {
            id,
            label,
            group: None,
            kind: KnobKind::F32 {
                default,
                min,
                max,
                step: ((max - min) / 100.0).max(0.01),
            },
        })
    }

    pub fn f32_step(
        self,
        id: &'static str,
        label: &'static str,
        default: f32,
        min: f32,
        max: f32,
        step: f32,
    ) -> Self {
        self.push(KnobDecl {
            id,
            label,
            group: None,
            kind: KnobKind::F32 {
                default,
                min,
                max,
                step,
            },
        })
    }

    pub fn text(self, id: &'static str, label: &'static str, default: &str) -> Self {
        self.push(KnobDecl {
            id,
            label,
            group: None,
            kind: KnobKind::Text {
                default: default.to_string(),
            },
        })
    }

    pub fn opt_text(self, id: &'static str, label: &'static str, default: Option<&str>) -> Self {
        self.push(KnobDecl {
            id,
            label,
            group: None,
            kind: KnobKind::OptText {
                default: default.map(|s| s.to_string()),
            },
        })
    }

    pub fn choice(
        self,
        id: &'static str,
        label: &'static str,
        options: &[&'static str],
        default: usize,
    ) -> Self {
        debug_assert!(default < options.len(), "default index out of range");
        self.push(KnobDecl {
            id,
            label,
            group: None,
            kind: KnobKind::Choice {
                options: options.to_vec(),
                default,
            },
        })
    }

    pub fn text_role(self, id: &'static str, label: &'static str, default: TextRole) -> Self {
        self.push(KnobDecl {
            id,
            label,
            group: None,
            kind: KnobKind::TextRole { default },
        })
    }

    pub fn surface_role(self, id: &'static str, label: &'static str, default: SurfaceRole) -> Self {
        self.push(KnobDecl {
            id,
            label,
            group: None,
            kind: KnobKind::SurfaceRole { default },
        })
    }

    pub fn border_role(self, id: &'static str, label: &'static str, default: BorderRole) -> Self {
        self.push(KnobDecl {
            id,
            label,
            group: None,
            kind: KnobKind::BorderRole { default },
        })
    }

    pub fn text_style(self, id: &'static str, label: &'static str, default: TextStyleRole) -> Self {
        self.push(KnobDecl {
            id,
            label,
            group: None,
            kind: KnobKind::TextStyle { default },
        })
    }

    /// Mark every knob added inside `f` as belonging to `group`. Used
    /// for inspector-side organisation: composite widgets group knobs
    /// per logical sub-component (`add_button`, `search`, …).
    pub fn group<F>(mut self, group: &'static str, f: F) -> Self
    where
        F: FnOnce(KnobSpec) -> KnobSpec,
    {
        let nested = f(KnobSpec::default());
        for mut decl in nested.decls {
            decl.group = Some(group);
            self.decls.push(decl);
        }
        self
    }
}

// ---------------------------------------------------------------------------
// Variant overrides — per-variant preset values
// ---------------------------------------------------------------------------

/// Concrete value for a knob, used by `PreviewVariant::Knobs` to override
/// the spec's defaults for a specific named variant.
#[derive(Debug, Clone)]
pub enum KnobValue {
    Bool(bool),
    OptBool(Option<bool>),
    I32(i32),
    OptI32(Option<i32>),
    F32(f32),
    OptF32(Option<f32>),
    Text(String),
    OptText(Option<String>),
    Choice(usize),
    TextRole(TextRole),
    SurfaceRole(SurfaceRole),
    BorderRole(BorderRole),
    TextStyle(TextStyleRole),
}

/// Variant override map: knob id → preset value. Built incrementally
/// with `KnobOverrides::new().bool_("disabled", true)…` and supplied
/// to `PreviewVariant::knobs(...)`.
#[derive(Debug, Clone, Default)]
pub struct KnobOverrides {
    map: HashMap<&'static str, KnobValue>,
}

impl KnobOverrides {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, id: &str) -> Option<&KnobValue> {
        self.map.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&&'static str, &KnobValue)> {
        self.map.iter()
    }

    pub fn bool_(mut self, id: &'static str, value: bool) -> Self {
        self.map.insert(id, KnobValue::Bool(value));
        self
    }

    pub fn opt_bool(mut self, id: &'static str, value: Option<bool>) -> Self {
        self.map.insert(id, KnobValue::OptBool(value));
        self
    }

    pub fn i32_(mut self, id: &'static str, value: i32) -> Self {
        self.map.insert(id, KnobValue::I32(value));
        self
    }

    pub fn f32_(mut self, id: &'static str, value: f32) -> Self {
        self.map.insert(id, KnobValue::F32(value));
        self
    }

    pub fn text(mut self, id: &'static str, value: impl Into<String>) -> Self {
        self.map.insert(id, KnobValue::Text(value.into()));
        self
    }

    pub fn opt_text(mut self, id: &'static str, value: Option<&str>) -> Self {
        self.map
            .insert(id, KnobValue::OptText(value.map(|s| s.to_string())));
        self
    }

    pub fn choice(mut self, id: &'static str, value: usize) -> Self {
        self.map.insert(id, KnobValue::Choice(value));
        self
    }

    pub fn text_role(mut self, id: &'static str, value: TextRole) -> Self {
        self.map.insert(id, KnobValue::TextRole(value));
        self
    }

    pub fn surface_role(mut self, id: &'static str, value: SurfaceRole) -> Self {
        self.map.insert(id, KnobValue::SurfaceRole(value));
        self
    }

    pub fn border_role(mut self, id: &'static str, value: BorderRole) -> Self {
        self.map.insert(id, KnobValue::BorderRole(value));
        self
    }

    pub fn text_style(mut self, id: &'static str, value: TextStyleRole) -> Self {
        self.map.insert(id, KnobValue::TextStyle(value));
        self
    }
}

// ---------------------------------------------------------------------------
// Runtime — typed signals constructed from a spec
// ---------------------------------------------------------------------------

/// Runtime container of one `Signal<T>` per declared knob, populated
/// from a `KnobSpec` and an optional set of variant overrides.
///
/// Construct once per (widget, variant) selection in the previewer; pass
/// by reference to `WidgetCatalog::build`. The widget reads each knob's
/// signal via the typed accessor methods (`bool_("disabled")`, …) and
/// threads it into the constructed widget through `Prop::Bound`.
///
/// All accessors return a `Signal<T>` clone. Cheap — `Signal<T>` is `Rc`-backed.
#[derive(Debug, Clone)]
pub struct KnobValues {
    bools: HashMap<&'static str, Signal<bool>>,
    opt_bools: HashMap<&'static str, Signal<Option<bool>>>,
    i32s: HashMap<&'static str, Signal<i32>>,
    opt_i32s: HashMap<&'static str, Signal<Option<i32>>>,
    f32s: HashMap<&'static str, Signal<f32>>,
    opt_f32s: HashMap<&'static str, Signal<Option<f32>>>,
    texts: HashMap<&'static str, Signal<String>>,
    opt_texts: HashMap<&'static str, Signal<Option<String>>>,
    choices: HashMap<&'static str, Signal<usize>>,
    text_roles: HashMap<&'static str, Signal<TextRole>>,
    surface_roles: HashMap<&'static str, Signal<SurfaceRole>>,
    border_roles: HashMap<&'static str, Signal<BorderRole>>,
    text_styles: HashMap<&'static str, Signal<TextStyleRole>>,
}

impl KnobValues {
    /// Build a fresh runtime view for a spec, applying optional
    /// variant overrides on top of each knob's declared default.
    pub fn from_spec(spec: &KnobSpec, overrides: Option<&KnobOverrides>) -> Self {
        let mut values = KnobValues {
            bools: HashMap::new(),
            opt_bools: HashMap::new(),
            i32s: HashMap::new(),
            opt_i32s: HashMap::new(),
            f32s: HashMap::new(),
            opt_f32s: HashMap::new(),
            texts: HashMap::new(),
            opt_texts: HashMap::new(),
            choices: HashMap::new(),
            text_roles: HashMap::new(),
            surface_roles: HashMap::new(),
            border_roles: HashMap::new(),
            text_styles: HashMap::new(),
        };
        for decl in &spec.decls {
            let ov = overrides.and_then(|o| o.get(decl.id));
            match (&decl.kind, ov) {
                (KnobKind::Bool { default: _ }, Some(KnobValue::Bool(v))) => {
                    values.bools.insert(decl.id, Signal::new(*v));
                }
                (KnobKind::Bool { default }, _) => {
                    values.bools.insert(decl.id, Signal::new(*default));
                }
                (KnobKind::OptBool { default: _ }, Some(KnobValue::OptBool(v))) => {
                    values.opt_bools.insert(decl.id, Signal::new(*v));
                }
                (KnobKind::OptBool { default }, _) => {
                    values.opt_bools.insert(decl.id, Signal::new(*default));
                }
                (KnobKind::I32 { default: _, .. }, Some(KnobValue::I32(v))) => {
                    values.i32s.insert(decl.id, Signal::new(*v));
                }
                (KnobKind::I32 { default, .. }, _) => {
                    values.i32s.insert(decl.id, Signal::new(*default));
                }
                (KnobKind::OptI32 { default: _, .. }, Some(KnobValue::OptI32(v))) => {
                    values.opt_i32s.insert(decl.id, Signal::new(*v));
                }
                (KnobKind::OptI32 { default, .. }, _) => {
                    values.opt_i32s.insert(decl.id, Signal::new(*default));
                }
                (KnobKind::F32 { default: _, .. }, Some(KnobValue::F32(v))) => {
                    values.f32s.insert(decl.id, Signal::new(*v));
                }
                (KnobKind::F32 { default, .. }, _) => {
                    values.f32s.insert(decl.id, Signal::new(*default));
                }
                (KnobKind::OptF32 { default: _, .. }, Some(KnobValue::OptF32(v))) => {
                    values.opt_f32s.insert(decl.id, Signal::new(*v));
                }
                (KnobKind::OptF32 { default, .. }, _) => {
                    values.opt_f32s.insert(decl.id, Signal::new(*default));
                }
                (KnobKind::Text { default: _ }, Some(KnobValue::Text(v))) => {
                    values.texts.insert(decl.id, Signal::new(v.clone()));
                }
                (KnobKind::Text { default }, _) => {
                    values.texts.insert(decl.id, Signal::new(default.clone()));
                }
                (KnobKind::OptText { default: _ }, Some(KnobValue::OptText(v))) => {
                    values.opt_texts.insert(decl.id, Signal::new(v.clone()));
                }
                (KnobKind::OptText { default }, _) => {
                    values
                        .opt_texts
                        .insert(decl.id, Signal::new(default.clone()));
                }
                (KnobKind::Choice { default: _, .. }, Some(KnobValue::Choice(v))) => {
                    values.choices.insert(decl.id, Signal::new(*v));
                }
                (KnobKind::Choice { default, .. }, _) => {
                    values.choices.insert(decl.id, Signal::new(*default));
                }
                (KnobKind::TextRole { default: _ }, Some(KnobValue::TextRole(v))) => {
                    values.text_roles.insert(decl.id, Signal::new(*v));
                }
                (KnobKind::TextRole { default }, _) => {
                    values.text_roles.insert(decl.id, Signal::new(*default));
                }
                (KnobKind::SurfaceRole { default: _ }, Some(KnobValue::SurfaceRole(v))) => {
                    values.surface_roles.insert(decl.id, Signal::new(*v));
                }
                (KnobKind::SurfaceRole { default }, _) => {
                    values.surface_roles.insert(decl.id, Signal::new(*default));
                }
                (KnobKind::BorderRole { default: _ }, Some(KnobValue::BorderRole(v))) => {
                    values.border_roles.insert(decl.id, Signal::new(*v));
                }
                (KnobKind::BorderRole { default }, _) => {
                    values.border_roles.insert(decl.id, Signal::new(*default));
                }
                (KnobKind::TextStyle { default: _ }, Some(KnobValue::TextStyle(v))) => {
                    values.text_styles.insert(decl.id, Signal::new(*v));
                }
                (KnobKind::TextStyle { default }, _) => {
                    values.text_styles.insert(decl.id, Signal::new(*default));
                }
            }
        }
        values
    }

    pub fn bool_(&self, id: &str) -> Signal<bool> {
        self.bools
            .get(id)
            .cloned()
            .unwrap_or_else(|| panic!("knob '{}' is not declared as Bool", id))
    }

    pub fn opt_bool(&self, id: &str) -> Signal<Option<bool>> {
        self.opt_bools
            .get(id)
            .cloned()
            .unwrap_or_else(|| panic!("knob '{}' is not declared as OptBool", id))
    }

    pub fn i32_(&self, id: &str) -> Signal<i32> {
        self.i32s
            .get(id)
            .cloned()
            .unwrap_or_else(|| panic!("knob '{}' is not declared as I32", id))
    }

    pub fn opt_i32(&self, id: &str) -> Signal<Option<i32>> {
        self.opt_i32s
            .get(id)
            .cloned()
            .unwrap_or_else(|| panic!("knob '{}' is not declared as OptI32", id))
    }

    pub fn f32_(&self, id: &str) -> Signal<f32> {
        self.f32s
            .get(id)
            .cloned()
            .unwrap_or_else(|| panic!("knob '{}' is not declared as F32", id))
    }

    pub fn opt_f32(&self, id: &str) -> Signal<Option<f32>> {
        self.opt_f32s
            .get(id)
            .cloned()
            .unwrap_or_else(|| panic!("knob '{}' is not declared as OptF32", id))
    }

    pub fn text(&self, id: &str) -> Signal<String> {
        self.texts
            .get(id)
            .cloned()
            .unwrap_or_else(|| panic!("knob '{}' is not declared as Text", id))
    }

    pub fn opt_text(&self, id: &str) -> Signal<Option<String>> {
        self.opt_texts
            .get(id)
            .cloned()
            .unwrap_or_else(|| panic!("knob '{}' is not declared as OptText", id))
    }

    pub fn choice(&self, id: &str) -> Signal<usize> {
        self.choices
            .get(id)
            .cloned()
            .unwrap_or_else(|| panic!("knob '{}' is not declared as Choice", id))
    }

    pub fn text_role(&self, id: &str) -> Signal<TextRole> {
        self.text_roles
            .get(id)
            .cloned()
            .unwrap_or_else(|| panic!("knob '{}' is not declared as TextRole", id))
    }

    pub fn surface_role(&self, id: &str) -> Signal<SurfaceRole> {
        self.surface_roles
            .get(id)
            .cloned()
            .unwrap_or_else(|| panic!("knob '{}' is not declared as SurfaceRole", id))
    }

    pub fn border_role(&self, id: &str) -> Signal<BorderRole> {
        self.border_roles
            .get(id)
            .cloned()
            .unwrap_or_else(|| panic!("knob '{}' is not declared as BorderRole", id))
    }

    pub fn text_style(&self, id: &str) -> Signal<TextStyleRole> {
        self.text_styles
            .get(id)
            .cloned()
            .unwrap_or_else(|| panic!("knob '{}' is not declared as TextStyle", id))
    }

    /// Bind every knob signal to `widget_id` at the given level via
    /// the supplied registry. Used by the canvas to force a rebuild
    /// whenever any knob mutates — many widgets read knob values
    /// once at construction time (no `Prop::Bound` for every property)
    /// so the catch-all is a rebuild on each change.
    pub fn bind_all(
        &self,
        widget_id: crate::__widget_id::WidgetId,
        registry: &bastyde_core::binding::BindingRegistry,
        level: bastyde_core::binding::BindingLevel,
    ) {
        for sig in self.bools.values() {
            sig.bind_to(widget_id, registry, level);
        }
        for sig in self.opt_bools.values() {
            sig.bind_to(widget_id, registry, level);
        }
        for sig in self.i32s.values() {
            sig.bind_to(widget_id, registry, level);
        }
        for sig in self.opt_i32s.values() {
            sig.bind_to(widget_id, registry, level);
        }
        for sig in self.f32s.values() {
            sig.bind_to(widget_id, registry, level);
        }
        for sig in self.opt_f32s.values() {
            sig.bind_to(widget_id, registry, level);
        }
        for sig in self.texts.values() {
            sig.bind_to(widget_id, registry, level);
        }
        for sig in self.opt_texts.values() {
            sig.bind_to(widget_id, registry, level);
        }
        for sig in self.choices.values() {
            sig.bind_to(widget_id, registry, level);
        }
        for sig in self.text_roles.values() {
            sig.bind_to(widget_id, registry, level);
        }
        for sig in self.surface_roles.values() {
            sig.bind_to(widget_id, registry, level);
        }
        for sig in self.border_roles.values() {
            sig.bind_to(widget_id, registry, level);
        }
        for sig in self.text_styles.values() {
            sig.bind_to(widget_id, registry, level);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knob_spec_records_decls_in_insertion_order() {
        let spec = KnobSpec::new()
            .text("label", "Label", "Click")
            .bool_("disabled", "Disabled", false)
            .choice("role", "Role", &["Primary", "Secondary"], 0);
        let ids: Vec<_> = spec.declarations().iter().map(|d| d.id).collect();
        assert_eq!(ids, vec!["label", "disabled", "role"]);
    }

    #[test]
    fn knob_values_from_spec_uses_defaults_when_no_override() {
        let spec = KnobSpec::new()
            .bool_("disabled", "Disabled", true)
            .text("label", "Label", "Hello")
            .choice("role", "Role", &["A", "B", "C"], 1);
        let values = KnobValues::from_spec(&spec, None);
        assert!(values.bool_("disabled").get());
        assert_eq!(values.text("label").get(), "Hello");
        assert_eq!(values.choice("role").get(), 1);
    }

    #[test]
    fn knob_values_applies_overrides() {
        let spec = KnobSpec::new()
            .bool_("disabled", "Disabled", false)
            .text("label", "Label", "Default");
        let overrides = KnobOverrides::new()
            .bool_("disabled", true)
            .text("label", "Overridden");
        let values = KnobValues::from_spec(&spec, Some(&overrides));
        assert!(values.bool_("disabled").get());
        assert_eq!(values.text("label").get(), "Overridden");
    }

    #[test]
    fn knob_values_kind_mismatch_panics() {
        let spec = KnobSpec::new().bool_("flag", "Flag", false);
        let values = KnobValues::from_spec(&spec, None);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            values.text("flag");
        }));
        assert!(result.is_err());
    }

    #[test]
    fn knob_spec_groups_propagate_to_decls() {
        let spec = KnobSpec::new()
            .bool_("global", "Global", false)
            .group("search", |g| {
                g.bool_("visible", "Visible", true)
                    .text("placeholder", "Placeholder", "Search…")
            });
        let decls = spec.declarations();
        assert_eq!(decls[0].group, None);
        assert_eq!(decls[1].group, Some("search"));
        assert_eq!(decls[2].group, Some("search"));
    }
}
