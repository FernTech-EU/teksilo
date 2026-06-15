// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Trait + types + registry for Bastyde's widget previewer
//! infrastructure.
//!
//! This crate is **framework-side, UI-free**: it defines the
//! `WidgetCatalog` trait, the `KnobSpec` / `KnobValues` types, and the
//! `inventory`-backed registry. The 3-pane previewer GUI lives in the
//! sibling [`bastyde-preview-ui`](../bastyde_preview_ui/index.html) crate;
//! per-application binaries (`bastyde-widgets-previewer`, etc.) link the
//! two together along with their own widget set.
//!
//! # Authoring a catalog impl
//!
//! ```ignore
//! use bastyde_preview::{
//!     register_widget_catalog, KnobSpec, KnobValues, PreviewVariant,
//!     KnobOverrides, WidgetCatalog,
//! };
//! use bastyde_core::widget::Widget;
//! use my_widgets::Button;
//!
//! impl WidgetCatalog for Button {
//!     fn id() -> &'static str { "button" }
//!     fn group() -> &'static str { "Controls" }
//!     fn display_name() -> &'static str { "Button" }
//!     fn knobs() -> KnobSpec {
//!         KnobSpec::new()
//!             .text("label", "Label", "Click me")
//!             .bool_("disabled", "Disabled", false)
//!     }
//!     fn variants() -> Vec<PreviewVariant> {
//!         vec![
//!             PreviewVariant::defaults("default"),
//!             PreviewVariant::knobs("disabled",
//!                 KnobOverrides::new().bool_("disabled", true)),
//!         ]
//!     }
//!     fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
//!         let label = knobs.text("label").get();
//!         Box::new(Button::new(label).enabled(!knobs.bool_("disabled").get()))
//!     }
//! }
//! register_widget_catalog!(Button);
//! ```

mod catalog;
mod knob;
mod registry;
mod source_loc;
mod variant;

pub use bastyde_core::widget_id::WidgetId;
pub use catalog::{CatalogEntry, SlottedChild, WidgetCatalog, WidgetCategory};
pub use knob::{KnobDecl, KnobKind, KnobOverrides, KnobSpec, KnobValue, KnobValues};
pub use registry::{entries_by_group, find_by_file, find_by_id, iter_entries};
pub use source_loc::SourceLoc;
pub use variant::{PreviewVariant, ScenarioBuilder};

// Internal re-exports used by the `register_widget_catalog!` macro.
// Hidden from the public API; exposed only because the macro must
// reference these symbols through the crate path.
#[doc(hidden)]
pub mod __widget {
    pub use bastyde_core::widget::Widget;
}

#[doc(hidden)]
pub mod __widget_id {
    pub use bastyde_core::widget_id::WidgetId;
}

#[doc(hidden)]
pub use inventory as __inventory;
