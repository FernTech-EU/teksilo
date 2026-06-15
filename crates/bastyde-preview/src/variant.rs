// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Preview variant — a named instance of a widget.
//!
//! Two flavours:
//! - `Knobs` — the variant supplies preset overrides for the
//!   widget's `KnobSpec`. The widget is built from a
//!   `KnobValues` populated with those overrides.
//! - `Scenario` — the variant ignores the spec entirely and runs a
//!   hand-authored builder function. Used by composites
//!   (Wizard, Dialog, ListView with sample data) where a
//!   flat knob surface doesn't describe the shape.

use crate::knob::KnobOverrides;
use bastyde_core::widget::Widget;

/// Builder fn used by `PreviewVariant::Scenario`. Returns a freshly
/// constructed widget instance — the previewer wraps it for layout
/// and paint just like any other root child.
pub type ScenarioBuilder = fn() -> Box<dyn Widget>;

#[derive(Debug, Clone)]
pub enum PreviewVariant {
    /// Knob preset — `WidgetCatalog::build` runs and consults
    /// `knobs()` to build the widget; the supplied overrides are
    /// applied on top of the spec's defaults.
    Knobs {
        name: &'static str,
        overrides: KnobOverrides,
    },
    /// Hand-authored scenario — `WidgetCatalog::build` ignores its
    /// `KnobValues` argument when this variant is selected and instead
    /// dispatches to the captured builder fn.
    Scenario {
        name: &'static str,
        builder: ScenarioBuilder,
    },
}

impl PreviewVariant {
    pub fn name(&self) -> &'static str {
        match self {
            PreviewVariant::Knobs { name, .. } => name,
            PreviewVariant::Scenario { name, .. } => name,
        }
    }

    pub fn knobs(name: &'static str, overrides: KnobOverrides) -> Self {
        PreviewVariant::Knobs { name, overrides }
    }

    pub fn defaults(name: &'static str) -> Self {
        PreviewVariant::Knobs {
            name,
            overrides: KnobOverrides::new(),
        }
    }

    pub fn scenario(name: &'static str, builder: ScenarioBuilder) -> Self {
        PreviewVariant::Scenario { name, builder }
    }
}
