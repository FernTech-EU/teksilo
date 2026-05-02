//! Diagnostic helpers for the `fern!` macro.
//!
//! Every `compile_error!` emitted by expansion runs through these
//! helpers so error spans land on a user token per spec §9.1, and the
//! messages match the patterns listed in §9.2.

use proc_macro2::Span;
use syn::Error;

pub fn error<T: std::fmt::Display>(span: Span, msg: T) -> Error {
    Error::new(span, msg)
}

/// Returns true if `name` is a method on `WidgetBuilder` (or the
/// inherent impl on `WidgetWithHandlers`). These methods wrap the
/// widget in `WidgetWithHandlers<T>`, which doesn't expose per-widget
/// builder methods. The lowering reorders handler-attachment items
/// to come AFTER every widget-specific item so users can write them
/// in any order without hitting "no method named `child` found for
/// `WidgetWithHandlers<T>`".
///
/// Kept in sync with `crates/fern-core/src/widget_builder.rs`.
pub fn is_widget_builder_method(name: &str) -> bool {
    matches!(
        name,
        "on_tap"
            | "on_double_tap"
            | "on_triple_tap"
            | "on_long_press"
            | "on_drag"
            | "on_swipe"
            | "on_pinch"
            | "on_focus"
            | "on_key"
            | "on_pointer_event"
            | "on_hover"
            | "on_scroll"
            | "on_access_action"
            | "on_access_action_request"
            | "on_drag_hover"
            | "on_drop"
            | "focusable"
            | "tab_index"
            | "cursor"
            | "clips_children_on"
            | "context_menu"
    )
}

/// Spec §9.2: a bare child element at body position inside a Category
/// B widget whose content is addressed by named slots. The list below
/// tracks the set of widgets that have no `.child()` method in the V2
/// builder API; if a user writes a bare child under one of them, the
/// compiler would otherwise produce a generic method-resolution error.
/// We pre-empt with a targeted message pointing at the slot name they
/// most likely meant.
pub fn category_b_bare_child(parent_ty: &str, child_span: Span) -> Error {
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
pub fn is_category_b_widget(ident: &str) -> bool {
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
