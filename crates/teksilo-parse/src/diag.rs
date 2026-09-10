// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Diagnostic helpers for the `teksu!` macro.
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
/// Membership is decided by the return type: a `WidgetBuilder` method
/// returning `WidgetWithHandlers<Self>` belongs here, because that
/// return is what breaks the chain. The `teksilo-teksu-guard` crate
/// parses `crates/teksilo-core/src/widget_builder.rs` and fails the
/// build when a wrapping method is absent from the list below, so this
/// is not kept in step by discipline.
pub fn is_widget_builder_method(name: &str) -> bool {
    matches!(
        name,
        // Gestures
        "on_tap"
            | "on_double_tap"
            | "on_triple_tap"
            | "on_long_press"
            | "on_drag"
            | "on_swipe"
            | "on_pinch"
            | "gesture_dead_zone"
            | "accept_tap_buttons"
            | "accept_double_tap_buttons"
            | "accept_triple_tap_buttons"
            | "accept_long_press_buttons"
            // Focus / keyboard / pointer
            | "on_focus"
            | "on_key"
            | "on_key_preview"
            | "on_pointer_event"
            | "on_pointer_cancel"
            | "on_hover"
            | "on_scroll"
            | "keyboard_capture"
            // Touch and pointer arbitration
            | "touch_action"
            | "scroll_container"
            | "pan_claim"
            | "overscroll_behavior"
            | "multi_contact"
            | "long_press_role"
            | "hit_slop"
            | "no_hit_slop"
            // Framework-level node properties
            | "focusable"
            | "tab_index"
            | "cursor"
            | "clips_children_on"
            | "ime_input"
            | "event_pass_through"
            | "hit_transparent"
            | "context_menu"
            | "focus_within"
            | "hover_within"
            | "visible_when"
            // Drag / drop
            | "drag_activation"
            | "on_drag_hover"
            | "on_drag_leave"
            | "on_drag_tick"
            | "on_drag_ended"
            | "on_drop"
            // Accessibility
            | "on_access_action"
            | "on_access_action_request"
            | "access_action"
            | "access_remove_action"
            | "access_custom_action"
            | "access_custom_action_literal"
            | "access_customize"
            | "access_label"
            | "access_label_literal"
            | "access_description"
            | "access_description_literal"
            | "access_hint"
            | "access_hint_literal"
            | "access_value"
            | "access_value_literal"
            | "access_role"
            | "access_hidden"
            | "access_disabled"
            | "access_identifier"
            | "access_controls"
            | "access_described_by"
            | "access_labelled_by"
            | "access_live"
            | "access_current"
            | "access_has_popup"
            | "access_orientation"
            | "access_numeric_value"
            | "access_numeric_range"
            | "access_numeric_step"
            | "access_shortcut_literal"
            | "access_shortcut_id"
            | "access_exclude_subtree"
            | "access_merge_subtree"
            | "access_subtree"
    )
}

/// A bare child element at body position inside a Category
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
pub fn is_category_b_widget(ident: &str) -> bool {
    matches!(
        ident,
        "Card"
            | "Accordion"
            | "TitleBar"
            | "DialogContent"
            | "Breadcrumb"
            | "TabWidget"
            // The popover family is four names, all of them aliases of
            // `PopoverWidget<T>`, and none of them spelled `Popover` — which is
            // what this list used to say, so a bare child in any real popover
            // fell through to the generic error instead of the slot hint.
            | "PopoverWidget"
            | "PopoverButton"
            | "PopoverIconButton"
            | "PopoverCustom"
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
        "TitleBar" => "leading",
        "DialogContent" => "body",
        "Breadcrumb" => "item",
        "TabWidget" => "tab",
        "PopoverWidget" | "PopoverButton" | "PopoverIconButton" | "PopoverCustom" => "content",
        "Snackbar" => "content",
        "Dialog" => "content",
        "Wizard" => "step",
        _ => "content",
    }
}
