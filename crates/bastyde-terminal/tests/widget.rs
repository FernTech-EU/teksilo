// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Widget-level integration tests for the `Terminal` view, driven headlessly
//! through a `WidgetTree` against the `MemoryEngine` test backend (no real
//! shell, no `alacritty` feature needed).

use bastyde_canvas::SizeProposal;
use bastyde_core::event::{Key, Modifiers, WidgetEvent};
use bastyde_core::widget_tree::WidgetTree;
use bastyde_core::window::NoopWindowOps;
use bastyde_terminal::{MemoryEngineFactory, Terminal};

fn theme() -> bastyde_core::styles::Theme {
    bastyde_core::presets::intui::light()
}

#[test]
fn terminal_exposes_role_terminal() {
    let mut tree = WidgetTree::new().with_theme(theme());
    let id = tree.add(Terminal::with_engine_factory(MemoryEngineFactory::new()).label("Shell"));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let info = tree.accessibility_node(id);
    assert_eq!(info.role(), bastyde_core::accesskit::Role::Terminal);
    assert_eq!(info.name(), Some("Shell"));
}

#[test]
fn key_press_writes_to_child() {
    let mut tree = WidgetTree::new().with_theme(theme());
    let factory = MemoryEngineFactory::new();
    let shared = factory.shared();
    let id = tree.add(Terminal::with_engine_factory(factory));

    tree.layout(SizeProposal::exact(400.0, 300.0));
    // Post-mount spawn creates the engine (no poster in a headless tree, so no
    // reader thread — the engine itself is still installed and writable).
    tree.run_mount_actions(&mut NoopWindowOps);
    tree.focus(id);

    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::A,
        modifiers: Modifiers::NONE,
        text: Some("a".to_string()),
    });

    assert_eq!(shared.borrow().writes, b"a");
}

#[test]
fn ctrl_c_reaches_child_as_sigint() {
    // The whole point of `keyboard_capture`: Ctrl+C must be delivered to the
    // child as 0x03, not swallowed by a host shortcut.
    let mut tree = WidgetTree::new().with_theme(theme());
    let factory = MemoryEngineFactory::new();
    let shared = factory.shared();
    let id = tree.add(Terminal::with_engine_factory(factory));

    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.run_mount_actions(&mut NoopWindowOps);
    tree.focus(id);

    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::C,
        modifiers: Modifiers::CTRL,
        text: None,
    });

    assert_eq!(shared.borrow().writes, vec![0x03]);
}

#[test]
fn controller_writes_and_observes_signals() {
    let mut tree = WidgetTree::new().with_theme(theme());
    let factory = MemoryEngineFactory::new();
    let shared = factory.shared();
    let terminal = Terminal::with_engine_factory(factory);
    let controller = terminal.controller();
    let id = tree.add(terminal);

    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.run_mount_actions(&mut NoopWindowOps);

    // child_running flips true once the engine is spawned.
    assert!(controller.child_running_signal().get());

    controller.feed_text("echo hi\n");
    assert_eq!(shared.borrow().writes, b"echo hi\n");
    let _ = id;
}
