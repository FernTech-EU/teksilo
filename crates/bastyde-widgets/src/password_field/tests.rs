// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Headless tests for `PasswordField` — masking semantics surfaced
//! through the accessibility tree (the observable, plaintext-free
//! contract), plus build/layout smoke coverage.

use bastyde_canvas::SizeProposal;
use bastyde_core::accesskit::Role;
use bastyde_core::signal::Signal;
use bastyde_core::widget_tree::WidgetTree;
use bastyde_i18n::lit;

use super::{AtRevealPolicy, EchoMode, PasswordField, RevealMode};

fn tree() -> WidgetTree {
    WidgetTree::new().with_theme(bastyde_core::presets::intui::light())
}

fn tick(tree: &mut WidgetTree) {
    tree.request_frame();
    tree.tick_animations(std::time::Duration::from_millis(16));
    tree.layout(SizeProposal::exact(320.0, 60.0));
}

/// The accesskit value of the first node carrying `role`, if any.
fn value_of_role(update: &bastyde_core::accesskit::TreeUpdate, role: Role) -> Option<String> {
    update
        .nodes
        .iter()
        .find(|(_, n)| n.role() == role)
        .and_then(|(_, n)| n.value().map(|v| v.to_string()))
}

fn has_role(update: &bastyde_core::accesskit::TreeUpdate, role: Role) -> bool {
    update.nodes.iter().any(|(_, n)| n.role() == role)
}

#[test]
fn constructs_and_lays_out() {
    let pw = Signal::new(String::new());
    let mut t = tree();
    let id = t.add(
        PasswordField::new(pw)
            .label(lit!("Password"))
            .placeholder(lit!("Enter…")),
    );
    t.layout(SizeProposal::exact(320.0, 60.0));
    tick(&mut t);
    let bounds = t.bounds(id);
    assert!(bounds.width > 0.0 && bounds.height > 0.0);
}

#[test]
fn masked_field_reports_password_role_and_bullet_value_never_plaintext() {
    let pw = Signal::new("hunter2".to_string());
    let mut t = tree();
    t.add(PasswordField::new(pw.clone()).label(lit!("Password")));
    t.layout(SizeProposal::exact(320.0, 60.0));
    tick(&mut t);

    let update = t.sync_accessibility();
    assert!(
        has_role(&update, Role::PasswordInput),
        "masked field must expose Role::PasswordInput"
    );
    let value = value_of_role(&update, Role::PasswordInput).expect("password node has a value");
    // Length-preserving bullets, never the secret.
    assert_eq!(value.chars().count(), "hunter2".chars().count());
    assert!(
        value.chars().all(|c| c == '\u{2022}'),
        "value must be bullets, got {value:?}"
    );
    assert_ne!(value, "hunter2", "plaintext must never reach the AT value");
    // The plaintext must not appear on ANY node.
    assert!(
        update
            .nodes
            .iter()
            .all(|(_, n)| n.value() != Some("hunter2")),
        "plaintext leaked into the accessibility tree"
    );
}

#[test]
fn revealed_swaps_to_text_role_with_plaintext_under_swap_policy() {
    let pw = Signal::new("hunter2".to_string());
    let revealed = Signal::new(false);
    let mut t = tree();
    t.add(
        PasswordField::new(pw.clone())
            .label(lit!("Password"))
            .at_reveal_policy(AtRevealPolicy::SwapRole)
            .bind_revealed(revealed.clone()),
    );
    t.layout(SizeProposal::exact(320.0, 60.0));
    tick(&mut t);
    assert!(has_role(&t.sync_accessibility(), Role::PasswordInput));

    // Reveal: the field becomes a normal text input exposing the value.
    revealed.set(true);
    tick(&mut t);
    let update = t.sync_accessibility();
    assert!(
        has_role(&update, Role::TextInput),
        "revealed SwapRole field must report Role::TextInput"
    );
    assert_eq!(
        value_of_role(&update, Role::TextInput).as_deref(),
        Some("hunter2"),
        "revealed field exposes the plaintext (matches what is on screen)"
    );
}

#[test]
fn always_protected_keeps_password_role_when_revealed() {
    let pw = Signal::new("hunter2".to_string());
    let revealed = Signal::new(true);
    let mut t = tree();
    t.add(
        PasswordField::new(pw.clone())
            .label(lit!("Password"))
            .at_reveal_policy(AtRevealPolicy::AlwaysProtected)
            .bind_revealed(revealed.clone()),
    );
    t.layout(SizeProposal::exact(320.0, 60.0));
    tick(&mut t);

    let update = t.sync_accessibility();
    assert!(
        has_role(&update, Role::PasswordInput),
        "AlwaysProtected must keep Role::PasswordInput even when revealed"
    );
    assert!(
        update
            .nodes
            .iter()
            .all(|(_, n)| n.value() != Some("hunter2")),
        "AlwaysProtected must never expose plaintext, even revealed"
    );
}

#[test]
fn no_echo_mode_hides_even_the_length() {
    let pw = Signal::new("secret".to_string());
    let mut t = tree();
    t.add(
        PasswordField::new(pw.clone())
            .label(lit!("Password"))
            .echo_mode(EchoMode::NoEcho),
    );
    t.layout(SizeProposal::exact(320.0, 60.0));
    tick(&mut t);

    let update = t.sync_accessibility();
    assert!(has_role(&update, Role::PasswordInput));
    // NoEcho exposes no value at all (not even the bullet count).
    assert_eq!(
        value_of_role(&update, Role::PasswordInput),
        None,
        "NoEcho must not expose a value (length stays secret)"
    );
}

#[test]
fn reveal_mode_none_builds() {
    let pw = Signal::new("x".to_string());
    let mut t = tree();
    let id = t.add(PasswordField::new(pw).reveal_mode(RevealMode::None));
    t.layout(SizeProposal::exact(320.0, 60.0));
    tick(&mut t);
    assert!(t.bounds(id).width > 0.0);
}

#[test]
fn hold_reveal_mode_builds() {
    let pw = Signal::new("x".to_string());
    let mut t = tree();
    let id = t.add(PasswordField::new(pw).reveal_mode(RevealMode::Hold));
    t.layout(SizeProposal::exact(320.0, 60.0));
    tick(&mut t);
    assert!(t.bounds(id).width > 0.0);
}

#[test]
fn revealed_signal_accessor_round_trips() {
    let pw = Signal::new(String::new());
    let revealed = Signal::new(false);
    let field = PasswordField::new(pw).bind_revealed(revealed.clone());
    field.revealed_signal().set(true);
    assert!(
        revealed.get(),
        "revealed_signal() must reflect the bound signal"
    );
}

// ── IME composition on a secure field ───────────────────────────────

#[test]
fn secure_field_composition_never_exposes_plaintext_to_at() {
    use bastyde_core::event::WidgetEvent;

    let pw = Signal::new(String::new());
    let mut t = tree();
    let id = t.add(PasswordField::new(pw.clone()).label(lit!("Password")));
    t.layout(SizeProposal::exact(320.0, 60.0));
    tick(&mut t);

    // Focus the inner field and compose a CJK candidate.
    let field = t
        .first_focusable_descendant(id)
        .expect("PasswordField exposes a focusable inner field");
    t.focus(field);
    t.dispatch_event(WidgetEvent::ImeComposition {
        text: "ni".to_string(),
        cursor: Some(2..2),
    });
    tick(&mut t);
    tick(&mut t);

    let update = t.sync_accessibility();
    // The masked field stays a PasswordInput showing bullets, and the
    // composing characters never reach any node's value.
    assert!(
        has_role(&update, Role::PasswordInput),
        "secure field stays a password input while composing"
    );
    assert!(
        update.nodes.iter().all(|(_, n)| n.value() != Some("ni")),
        "in-progress composition must never leak to assistive tech"
    );
    let value = value_of_role(&update, Role::PasswordInput).unwrap_or_default();
    assert!(
        value.chars().all(|c| c == '\u{2022}'),
        "the composing text is masked as bullets, got {value:?}"
    );
}

#[test]
fn focused_secure_field_is_a_password_ime_surface() {
    use bastyde_core::ime::ImeContext;

    let pw = Signal::new(String::new());
    let mut t = tree();
    let id = t.add(PasswordField::new(pw).label(lit!("Password")));
    t.layout(SizeProposal::exact(320.0, 60.0));
    tick(&mut t);
    let field = t.first_focusable_descendant(id).unwrap();
    t.focus(field);

    assert_eq!(
        t.ime_context_for_focused(),
        Some(ImeContext::password()),
        "a focused secure field declares a Password IME surface"
    );
}

// ── Tooltip ───────────────────────────────────────────────────────────

#[test]
fn tooltip_appears_on_hover() {
    let pw = Signal::new(String::new());
    let mut t = tree();
    let id = t.add(PasswordField::new(pw).tooltip(lit!("Tip")));
    t.layout(SizeProposal::exact(300.0, 200.0));
    t.pointer_move(t.bounds(id).center());
    t.advance_time(std::time::Duration::from_secs(1));
    assert_eq!(
        t.active_overlays().len(),
        1,
        "tooltip should appear on hover"
    );
    assert!(t.find_by_label("Tip").is_some());
}
