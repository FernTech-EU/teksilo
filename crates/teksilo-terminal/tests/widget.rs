// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Widget-level integration tests for the `Terminal` view, driven headlessly
//! through a `WidgetTree` against the `MemoryEngine` test backend (no real
//! shell, no `alacritty` feature needed).

use teksilo_canvas::SizeProposal;
use teksilo_core::event::{Key, Modifiers, WidgetEvent};
use teksilo_core::widget_tree::WidgetTree;
use teksilo_core::window::NoopWindowOps;
use teksilo_terminal::{MemoryEngineFactory, Terminal};

fn theme() -> teksilo_core::styles::Theme {
    teksilo_core::presets::intui::light()
}

#[test]
fn terminal_exposes_role_terminal() {
    let mut tree = WidgetTree::new().with_theme(theme());
    let id = tree.add(Terminal::with_engine_factory(MemoryEngineFactory::new()).label("Shell"));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let info = tree.accessibility_node(id);
    assert_eq!(info.role(), teksilo_core::accesskit::Role::Terminal);
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

#[test]
fn ctrl_tab_is_not_written_to_the_child() {
    // WCAG 2.1.2. The terminal encodes plain Tab as `\t` and Shift+Tab as
    // CSI Z, so those can never leave the widget. Ctrl+Tab is the reserved
    // escape chord: it must reach the framework's focus cycling instead of
    // the child process, so nothing is written and focus moves on.
    let mut tree = WidgetTree::new().with_theme(theme());
    let factory = MemoryEngineFactory::new();
    let shared = factory.shared();
    let id = tree.add(Terminal::with_engine_factory(factory));

    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.run_mount_actions(&mut NoopWindowOps);
    tree.focus(id);

    // Plain Tab is the child's business.
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::Tab,
        modifiers: Modifiers::NONE,
        text: None,
    });
    assert_eq!(shared.borrow().writes, b"\t");

    // Ctrl+Tab is the framework's.
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::Tab,
        modifiers: Modifiers::CTRL,
        text: None,
    });
    assert_eq!(
        shared.borrow().writes,
        b"\t",
        "Ctrl+Tab must not be encoded to the PTY"
    );
}

#[test]
fn a_read_only_terminal_declines_keys_it_cannot_use() {
    // A read-only terminal has no child to receive a keystroke, so consuming
    // every key trapped focus for no benefit. It now declines, which lets the
    // framework's plain-Tab focus cycling work as usual.
    let mut tree = WidgetTree::new().with_theme(theme());
    let factory = MemoryEngineFactory::new();
    let shared = factory.shared();
    let id = tree.add(Terminal::with_engine_factory(factory).read_only(true));

    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.run_mount_actions(&mut NoopWindowOps);
    tree.focus(id);

    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::A,
        modifiers: Modifiers::NONE,
        text: Some("a".to_string()),
    });
    assert!(
        shared.borrow().writes.is_empty(),
        "a read-only terminal must not write to the child"
    );

    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::Tab,
        modifiers: Modifiers::NONE,
        text: None,
    });
    assert!(
        shared.borrow().writes.is_empty(),
        "a read-only terminal must not encode Tab either"
    );
}
