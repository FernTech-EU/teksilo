// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The canonical catalog of automation tools — one entry per MCP tool.
//!
//! This is the single source of truth for *which* tools exist, their
//! one-line descriptions, and whether they mutate the UI (and so accept a
//! `SettleSpec`). The MCP server binary registers a handler per entry; its
//! conformance test cross-checks that the registered set matches this
//! catalog exactly. Keeping the catalog in the GUI-free toolkit (which has
//! no `rmcp` dependency) means the tool surface is documented and testable
//! without pulling in the async stack.

/// One automation tool's metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolDescriptor {
    /// The MCP tool name (snake_case).
    pub name: &'static str,
    /// One-line human description.
    pub description: &'static str,
    /// Whether the tool mutates UI state and therefore accepts an optional
    /// `SettleSpec`.
    pub mutating: bool,
}

/// Every automation tool, in a stable order.
pub const TOOL_CATALOG: &[ToolDescriptor] = &[
    // ---- Query ----
    ToolDescriptor {
        name: "snapshot_tree",
        description: "Snapshot the accessibility tree (roles, labels, values, bounds, actions).",
        mutating: false,
    },
    ToolDescriptor {
        name: "read_node",
        description: "Read a single semantic node by its id.",
        mutating: false,
    },
    ToolDescriptor {
        name: "layout_tree",
        description: "Walk the full widget/layout tree (incl. widgets the AT tree prunes) with bounds + types.",
        mutating: false,
    },
    ToolDescriptor {
        name: "inspect_node",
        description: "One widget's full layout record: type, bounds, flags, tree position, Debug repr.",
        mutating: false,
    },
    ToolDescriptor {
        name: "find_node",
        description: "Find the first node matching a role and/or label.",
        mutating: false,
    },
    ToolDescriptor {
        name: "assert_node",
        description: "Assert a property of a node (role/label/value/toggled/expanded/…).",
        mutating: false,
    },
    ToolDescriptor {
        name: "list_windows",
        description: "List the app's managed windows with ids, labels, and titles.",
        mutating: false,
    },
    // ---- AT-action driving ----
    ToolDescriptor {
        name: "invoke_action",
        description: "Invoke an AccessKit action on a node (click/focus/expand/…).",
        mutating: true,
    },
    ToolDescriptor {
        name: "focus_node",
        description: "Move focus to a node.",
        mutating: true,
    },
    ToolDescriptor {
        name: "set_value",
        description: "Set a node's value via the SetValue AT action.",
        mutating: true,
    },
    ToolDescriptor {
        name: "expand",
        description: "Expand a disclosure / tree node.",
        mutating: true,
    },
    ToolDescriptor {
        name: "collapse",
        description: "Collapse a disclosure / tree node.",
        mutating: true,
    },
    ToolDescriptor {
        name: "scroll",
        description: "Scroll the widget under a node by a pixel delta, with optional modifiers (ctrl/shift/alt/meta/command). A modifier-held wheel is its own gesture, e.g. Ctrl+wheel to zoom. Use `command` for the platform accelerator (Control on Windows/Linux, Command on macOS); `ctrl` is literal Control.",
        mutating: true,
    },
    // ---- Synthetic input ----
    ToolDescriptor {
        name: "inject_pointer",
        description: "Inject a pointer event at a point: action = click (default), double_click, down, up or move; button = primary (default), secondary, middle, back, forward; kind = mouse (default), touch or pen; with optional ctrl/shift/alt/meta/command held for the press and release. A touch or pen enters through the tree's pointer door, so the kind reaches the hit test, the slop, the hover rules and the arbitration; pen carries `pressure` (0..1) and `tilt` ([x, y] degrees). Continue a contact a previous call left down with `pointer_id` from query_pointers (on a move or an up; a down and a click mint their own), or drive a whole gesture with inject_touch_sequence. Use `command` for the platform accelerator (Control on Windows/Linux, Command on macOS) — accelerator-click to extend a selection is `command`, not `ctrl`. Unknown names and unknown fields are refused rather than defaulted, and so are pressure/tilt/pointer_id on a mouse.",
        mutating: true,
    },
    ToolDescriptor {
        name: "right_click",
        description: "Right-click a node (secondary button at its point) to open its context menu.",
        mutating: true,
    },
    ToolDescriptor {
        name: "inject_key",
        description: "Inject a key press (with optional modifiers) to the focused widget. Use `command` for any accelerator chord (Control on Windows/Linux, Command on macOS) — a shortcut declared Ctrl+S resolves to the Command chord on macOS, so `ctrl` there injects a key that matches no binding and still reports success. `ctrl` stays literal Control, for chords that really are Control everywhere (Ctrl+Tab).",
        mutating: true,
    },
    ToolDescriptor {
        name: "type_text",
        description: "Focus a node and type text into it.",
        mutating: true,
    },
    ToolDescriptor {
        name: "type_ime",
        description: "Drive IME composition / commit on a node.",
        mutating: true,
    },
    ToolDescriptor {
        name: "drag_node",
        description: "Drag from a node to another node or a point.",
        mutating: true,
    },
    ToolDescriptor {
        name: "inject_touch_sequence",
        description: "Drive a whole multi-touch gesture in one call — a list of steps, each naming a finger slot, a phase (down/move/up/cancel), a point, and how many simulated milliseconds to advance first — and report the arbitration after every step: the frozen touch_action, every competitor with its role and state, and the winner. Fingers are named by slot, not by id: identities are minted by the framework and the reply says which one each slot got. A sequence that stops short of its `up` leaves the finger down, which is how a live arbitration stays observable.",
        mutating: true,
    },
    ToolDescriptor {
        name: "pinch",
        description: "Two fingers moving from one span to another — the pinch a zoomable surface reads. Both land before either moves, because the recogniser's reference span is the distance between the landings.",
        mutating: true,
    },
    ToolDescriptor {
        name: "fling",
        description: "One finger travelling from one point to another over N simulated milliseconds and released while still moving — the shape a kinetic coast is handed off from. A drag latches on distance; a fling hands a velocity to the scroller, so the duration is the whole difference.",
        mutating: true,
    },
    ToolDescriptor {
        name: "long_press",
        description: "Press at a point, hold for exactly the device's long-press threshold, release. The hold is read off the active input profile for the pointer kind, so the call means 'hold long enough' without the script knowing the number.",
        mutating: true,
    },
    ToolDescriptor {
        name: "cancel_pointer",
        description: "Revoke a live pointer the way the system does (a compositor grab, a lost capture). Not an up: no tap completes and every widget working on the pointer is told. Takes an id from query_pointers.",
        mutating: true,
    },
    ToolDescriptor {
        name: "query_pointers",
        description: "Every live pointer with its id, kind, position, pressure/tilt, capture, frozen touch_action, competitors and arbitration winner. How to learn the id of a contact left down by anything but inject_touch_sequence, whose reply names its own.",
        mutating: false,
    },
    ToolDescriptor {
        name: "set_density",
        description: "Switch the app's target density (compact / comfortable / touch). WARNING: a density change rebuilds every widget, so every node id captured before it is dead — re-snapshot or re-find afterwards. Setting the density it already has is a no-op and keeps the ids.",
        mutating: true,
    },
    // ---- Introspection ----
    ToolDescriptor {
        name: "get_overlays",
        description: "List active overlays (popovers, menus, tooltips, dialogs).",
        mutating: false,
    },
    ToolDescriptor {
        name: "get_shortcuts",
        description: "List effective keyboard shortcuts and their bindings.",
        mutating: false,
    },
    ToolDescriptor {
        name: "list_live_regions",
        description: "List nodes that are live regions (polite/assertive).",
        mutating: false,
    },
    ToolDescriptor {
        name: "pull_announcements",
        description: "Drain captured live-region announcements since a sequence number.",
        mutating: false,
    },
    // ---- Time / settle ----
    ToolDescriptor {
        name: "advance_clock",
        description: "Advance the simulation clock by N milliseconds.",
        mutating: true,
    },
    ToolDescriptor {
        name: "settle",
        description: "Run animations / layout to quiescence, then re-sync the tree.",
        mutating: true,
    },
    ToolDescriptor {
        name: "wait_for_condition",
        description: "Poll until a condition holds (node exists / value / gone / version).",
        mutating: true,
    },
    // ---- Visual ----
    ToolDescriptor {
        name: "screenshot",
        description: "Render the window (or a node's bounds) to a PNG image block.",
        mutating: false,
    },
];

/// The number of tools in the catalog (34).
pub const TOOL_COUNT: usize = TOOL_CATALOG.len();

#[cfg(test)]
mod tests {
    //! The catalog against the Python probe library's generated tool surface.
    //!
    //! `cargo-teksilo` embeds a Python library (`embedded/probe/teksilo_probe`)
    //! that an agent drives a live app with, and its `tools.py` is **generated**
    //! from this catalog. A generated file is only as good as the thing that
    //! notices it has gone stale, so that is what this is: add a tool here
    //! without regenerating, and this test names the script to run.
    //!
    //! This is the same class of bug as the 0.9.3 announce rename, which
    //! silently broke every probe in the harness this library generalises —
    //! a contract changed on one side of a seam that nothing was watching.

    use super::*;
    use std::path::{Path, PathBuf};

    /// Where the generated module lives, relative to this crate.
    const TOOLS_PY: &str = "cargo-teksilo/embedded/probe/teksilo_probe/tools.py";

    /// The command that rebuilds it.
    const GENERATOR: &str = "python3 crates/cargo-teksilo/embedded/probe/generate_tools.py";

    fn tools_py_path() -> PathBuf {
        // `CARGO_MANIFEST_DIR` is `<repo>/crates/teksilo-automation`; its parent
        // is `<repo>/crates`, which is where the sibling crate is. Derived
        // rather than walked up to a marker file, so a checkout nested inside
        // another repository cannot resolve the wrong one.
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the manifest dir always has a parent")
            .join(TOOLS_PY)
    }

    fn tools_py() -> String {
        let path = tools_py_path();
        std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "cannot read the generated probe tool surface at {}: {e}\n\
                 Generate it with:\n  {GENERATOR}\n\
                 It is committed, so an absent file means either a partial \
                 checkout or a generator that has never been run here.",
                path.display()
            )
        })
    }

    /// Every module-level `def` that is not a private helper.
    ///
    /// The generator emits one per tool, and its only other module-level
    /// function is `_present`; the leading underscore is the discriminator, and
    /// the `session`-first convention is asserted separately below.
    fn defined_functions(src: &str) -> Vec<String> {
        src.lines()
            .filter_map(|line| line.strip_prefix("def "))
            .filter_map(|rest| rest.split('(').next())
            .filter(|name| !name.starts_with('_'))
            .map(str::to_owned)
            .collect()
    }

    /// The string literals inside a `NAME = (` … `)` block at column zero.
    fn tuple_literals(src: &str, binding: &str) -> Vec<String> {
        let needle = format!("\n{binding} = (\n");
        let start = src
            .find(&needle)
            .unwrap_or_else(|| panic!("no `{binding} = (` in the generated module"))
            + needle.len();
        let body = &src[start..];
        let end = body
            .find("\n)")
            .unwrap_or_else(|| panic!("unterminated `{binding}` tuple"));
        quoted(&body[..end])
    }

    /// Every `"…"` in a chunk of Python, in order. The generator never emits an
    /// escaped quote inside one of these, so a plain split is exact.
    fn quoted(chunk: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = chunk;
        while let Some(open) = rest.find('"') {
            rest = &rest[open + 1..];
            match rest.find('"') {
                Some(close) => {
                    out.push(rest[..close].to_owned());
                    rest = &rest[close + 1..];
                }
                None => break,
            }
        }
        out
    }

    /// The `("name", True|False),` pairs feeding the `MUTATING` frozenset.
    fn mutating_pairs(src: &str) -> Vec<(String, bool)> {
        src.lines()
            .map(str::trim)
            .filter(|line| line.starts_with("(\"") && line.ends_with("),"))
            .filter_map(|line| {
                let name = quoted(line).into_iter().next()?;
                let flag = if line.contains("True") {
                    true
                } else if line.contains("False") {
                    false
                } else {
                    return None;
                };
                Some((name, flag))
            })
            .collect()
    }

    #[test]
    fn the_probe_library_defines_a_function_per_catalog_tool() {
        let src = tools_py();
        let generated = defined_functions(&src);
        let expected: Vec<&str> = TOOL_CATALOG.iter().map(|t| t.name).collect();

        let missing: Vec<&&str> = expected
            .iter()
            .filter(|name| !generated.iter().any(|g| g == **name))
            .collect();
        let extra: Vec<&String> = generated
            .iter()
            .filter(|name| !expected.contains(&name.as_str()))
            .collect();

        assert!(
            missing.is_empty() && extra.is_empty(),
            "the probe library's tool surface has drifted from TOOL_CATALOG.\n\
             missing (in the catalog, no wrapper): {missing:?}\n\
             extra   (a wrapper for no catalog entry): {extra:?}\n\
             Regenerate with:\n  {GENERATOR}"
        );
        assert_eq!(
            generated.len(),
            TOOL_COUNT,
            "{} wrappers for {TOOL_COUNT} catalog tools — a duplicate `def`?\n\
             Regenerate with:\n  {GENERATOR}",
            generated.len()
        );
    }

    #[test]
    fn the_probe_library_tool_names_match_the_catalog_in_order() {
        let src = tools_py();
        let listed = tuple_literals(&src, "TOOL_NAMES");
        let expected: Vec<String> = TOOL_CATALOG.iter().map(|t| t.name.to_owned()).collect();
        assert_eq!(
            listed, expected,
            "`TOOL_NAMES` in the generated module is not TOOL_CATALOG, in order.\n\
             Regenerate with:\n  {GENERATOR}"
        );
    }

    #[test]
    fn the_probe_library_agrees_about_which_tools_mutate() {
        // A flipped `mutating` flag is invisible to a name comparison, and it is
        // what decides whether a wrapper offers a `settle` at all.
        let src = tools_py();
        let pairs = mutating_pairs(&src);
        let expected: Vec<(String, bool)> = TOOL_CATALOG
            .iter()
            .map(|t| (t.name.to_owned(), t.mutating))
            .collect();
        assert_eq!(
            pairs, expected,
            "the generated module's `mutating` flags differ from TOOL_CATALOG.\n\
             Regenerate with:\n  {GENERATOR}"
        );
    }

    #[test]
    fn every_generated_wrapper_takes_the_session_first() {
        // The calling convention the whole library is written against. A
        // wrapper that lost it would still pass the name comparison and fail at
        // every call site.
        let src = tools_py();
        for tool in TOOL_CATALOG {
            let one_line = format!("\ndef {}(session", tool.name);
            let wrapped = format!("\ndef {}(\n    session,", tool.name);
            assert!(
                src.contains(&one_line) || src.contains(&wrapped),
                "`{}`'s wrapper does not take `session` first.\n\
                 Regenerate with:\n  {GENERATOR}",
                tool.name
            );
        }
    }

    #[test]
    fn the_catalog_itself_is_well_formed() {
        // Cheap invariants the generator relies on: unique snake_case names and
        // a description that can serve as a docstring.
        let mut seen = std::collections::BTreeSet::new();
        for tool in TOOL_CATALOG {
            assert!(
                seen.insert(tool.name),
                "duplicate tool name in TOOL_CATALOG: {}",
                tool.name
            );
            assert!(
                tool.name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "tool name is not snake_case: {}",
                tool.name
            );
            assert!(
                !tool.description.trim().is_empty(),
                "tool `{}` has no description to document",
                tool.name
            );
        }
        assert_eq!(seen.len(), TOOL_COUNT);
    }
}
