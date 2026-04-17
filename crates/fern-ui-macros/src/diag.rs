//! Diagnostic helpers for the `fern!` macro.
//!
//! Every `compile_error!` emitted by expansion runs through these
//! helpers so error spans land on a user token per spec §9.1, and the
//! messages match the patterns listed in §9.2.

use proc_macro2::Span;
use syn::Error;

pub(crate) fn error<T: std::fmt::Display>(span: Span, msg: T) -> Error {
    Error::new(span, msg)
}

/// Spec §9.2: a bare child element at body position inside a Category
/// B widget whose content is addressed by named slots. The list below
/// tracks the set of widgets that have no `.child()` method in the V2
/// builder API; if a user writes a bare child under one of them, the
/// compiler would otherwise produce a generic method-resolution error.
/// We pre-empt with a targeted message pointing at the slot name they
/// most likely meant.
pub(crate) fn category_b_bare_child(parent_ty: &str, child_span: Span) -> Error {
    let slot_hint = category_b_slot_hint(parent_ty);
    Error::new(
        child_span,
        format!(
            "`{parent_ty}` is a Category B widget with named slots — \
             use `{slot_hint}: <widget>` instead of a bare child element"
        ),
    )
}

/// Returns `Some(canonical type name)` if `ident` names a widget whose
/// content is addressed via named slots and which does not implement
/// `.child()`. `None` for every other type — including Category A
/// containers (VStack, Panel, …) where bare children are legal.
///
/// Kept in sync with spec §4.2.
pub(crate) fn is_category_b_widget(ident: &str) -> bool {
    matches!(
        ident,
        "Card"
            | "Accordion"
            | "SplitView"
            | "TitleBar"
            | "DialogContent"
            | "Breadcrumb"
            | "TabWidget"
            | "Popover"
            | "Snackbar"
            | "Dialog"
            | "Wizard"
    )
}

/// Pick the most likely slot name for a Category B widget. Used only
/// to render a better "use `<slot>:` instead" hint — if the user
/// actually wanted a different slot, the hint still points them at a
/// real method name and the rest of their fix is obvious.
fn category_b_slot_hint(ident: &str) -> &'static str {
    match ident {
        "Card" => "content",
        "Accordion" => "content",
        "SplitView" => "first",
        "TitleBar" => "leading",
        "DialogContent" => "body",
        "Breadcrumb" => "item",
        "TabWidget" => "tab_literal",
        "Popover" => "content",
        "Snackbar" => "content",
        "Dialog" => "content",
        "Wizard" => "step",
        _ => "content",
    }
}
