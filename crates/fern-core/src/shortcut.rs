use crate::app_command::AppCommand;
use crate::event::{Key, Modifiers};
use crate::widget_id::WidgetId;

/// A keyboard shortcut (key + modifiers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Shortcut {
    pub key: Key,
    pub modifiers: Modifiers,
}

impl Shortcut {
    pub fn new(key: Key, modifiers: Modifiers) -> Self {
        Self { key, modifiers }
    }

    pub fn ctrl(key: Key) -> Self {
        Self::new(key, Modifiers::CTRL)
    }

    pub fn ctrl_shift(key: Key) -> Self {
        Self::new(key, Modifiers::CTRL | Modifiers::SHIFT)
    }

    pub fn alt(key: Key) -> Self {
        Self::new(key, Modifiers::ALT)
    }
}

impl std::fmt::Display for Shortcut {
    // TODO: Replace with ShortcutFormatter in fern-i18n for locale/platform-aware
    // display (e.g. "Ctrl+S" → "⌘S" on macOS, "Strg+S" in German).
    // See architecture Section 11.2.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", self.modifiers, self.key)
    }
}

/// Whether a shortcut binding is global or scoped to a widget subtree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutScope {
    Global,
    Scoped(WidgetId),
}

/// A single shortcut-to-command binding.
#[derive(Debug, Clone)]
pub struct ShortcutBinding<C: AppCommand> {
    pub shortcut: Shortcut,
    pub command: C,
    pub scope: ShortcutScope,
}

/// A bidirectional map between shortcuts and application commands.
/// Consulted during the preview pass before any widget sees the key event.
/// Modifiable at runtime (user preferences).
#[derive(Debug, Clone)]
pub struct ShortcutMap<C: AppCommand> {
    bindings: Vec<ShortcutBinding<C>>,
}

impl<C: AppCommand> ShortcutMap<C> {
    pub fn new() -> Self {
        Self {
            bindings: Vec::new(),
        }
    }

    /// Bind a global shortcut to a command.
    pub fn bind(mut self, shortcut: Shortcut, command: C) -> Self {
        self.bindings.push(ShortcutBinding {
            shortcut,
            command,
            scope: ShortcutScope::Global,
        });
        self
    }

    /// Bind a scoped shortcut (only active when focus is within the scope widget).
    pub fn bind_scoped(mut self, shortcut: Shortcut, command: C, scope: WidgetId) -> Self {
        self.bindings.push(ShortcutBinding {
            shortcut,
            command,
            scope: ShortcutScope::Scoped(scope),
        });
        self
    }

    /// Add a binding at runtime (for user preference changes).
    pub fn add(&mut self, shortcut: Shortcut, command: C) {
        self.bindings.push(ShortcutBinding {
            shortcut,
            command,
            scope: ShortcutScope::Global,
        });
    }

    /// Remove all bindings for a given shortcut.
    pub fn unbind(&mut self, shortcut: &Shortcut) {
        self.bindings.retain(|b| &b.shortcut != shortcut);
    }

    /// Find the command for a shortcut, considering scope.
    /// Scoped bindings are checked first; if none match, global bindings are checked.
    /// The `is_in_scope` closure checks whether the focused widget is within
    /// a scoped widget's subtree (the caller provides this from the widget tree).
    pub fn find(
        &self,
        shortcut: &Shortcut,
        focused: Option<WidgetId>,
        is_in_scope: impl Fn(WidgetId, WidgetId) -> bool,
    ) -> Option<&C> {
        // First, check scoped bindings (higher priority)
        if let Some(focused_id) = focused {
            for b in &self.bindings {
                if b.shortcut != *shortcut {
                    continue;
                }
                if let ShortcutScope::Scoped(scope_id) = b.scope
                    && is_in_scope(focused_id, scope_id)
                {
                    return Some(&b.command);
                }
            }
        }

        // Then, check global bindings
        self.bindings
            .iter()
            .find(|b| b.shortcut == *shortcut && b.scope == ShortcutScope::Global)
            .map(|b| &b.command)
    }

    /// Reverse lookup: find the shortcut bound to a command.
    /// Returns the first global binding that matches.
    pub fn find_shortcut_for(&self, command: &C) -> Option<&Shortcut> {
        self.bindings
            .iter()
            .find(|b| b.scope == ShortcutScope::Global && &b.command == command)
            .map(|b| &b.shortcut)
    }

    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }
}

impl<C: AppCommand> Default for ShortcutMap<C> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    enum Cmd {
        Save,
        Undo,
        Redo,
    }
    impl AppCommand for Cmd {}

    #[test]
    fn bind_and_find() {
        let map = ShortcutMap::new()
            .bind(Shortcut::ctrl(Key::S), Cmd::Save)
            .bind(Shortcut::ctrl(Key::Z), Cmd::Undo);

        assert_eq!(
            map.find(&Shortcut::ctrl(Key::S), None, |_, _| false),
            Some(&Cmd::Save)
        );
        assert_eq!(
            map.find(&Shortcut::ctrl(Key::Z), None, |_, _| false),
            Some(&Cmd::Undo)
        );
        assert_eq!(map.find(&Shortcut::ctrl(Key::X), None, |_, _| false), None);
    }

    #[test]
    fn ctrl_shift_binding() {
        let map = ShortcutMap::new().bind(Shortcut::ctrl_shift(Key::Z), Cmd::Redo);

        assert_eq!(
            map.find(&Shortcut::ctrl_shift(Key::Z), None, |_, _| false),
            Some(&Cmd::Redo)
        );
        // Ctrl+Z alone should not match Ctrl+Shift+Z
        assert_eq!(map.find(&Shortcut::ctrl(Key::Z), None, |_, _| false), None);
    }

    #[test]
    fn unbind_removes() {
        let mut map = ShortcutMap::new().bind(Shortcut::ctrl(Key::S), Cmd::Save);

        assert!(
            map.find(&Shortcut::ctrl(Key::S), None, |_, _| false)
                .is_some()
        );
        map.unbind(&Shortcut::ctrl(Key::S));
        assert!(
            map.find(&Shortcut::ctrl(Key::S), None, |_, _| false)
                .is_none()
        );
    }

    #[test]
    fn runtime_add() {
        let mut map = ShortcutMap::<Cmd>::new();
        assert!(map.is_empty());
        map.add(Shortcut::ctrl(Key::S), Cmd::Save);
        assert!(!map.is_empty());
        assert_eq!(
            map.find(&Shortcut::ctrl(Key::S), None, |_, _| false),
            Some(&Cmd::Save)
        );
    }

    #[test]
    fn scoped_binding_matches_when_focused_in_scope() {
        use slotmap::KeyData;
        let scope_id: WidgetId = KeyData::from_ffi(10).into();
        let focused_id: WidgetId = KeyData::from_ffi(20).into();

        let map = ShortcutMap::new()
            .bind(Shortcut::ctrl(Key::S), Cmd::Save)
            .bind_scoped(Shortcut::ctrl(Key::Z), Cmd::Undo, scope_id);

        // Scoped binding matches when focused is in scope
        let result = map.find(
            &Shortcut::ctrl(Key::Z),
            Some(focused_id),
            |focused, scope| focused == focused_id && scope == scope_id,
        );
        assert_eq!(result, Some(&Cmd::Undo));
    }

    #[test]
    fn scoped_binding_does_not_match_outside_scope() {
        use slotmap::KeyData;
        let scope_id: WidgetId = KeyData::from_ffi(10).into();
        let focused_id: WidgetId = KeyData::from_ffi(20).into();

        let map = ShortcutMap::new().bind_scoped(Shortcut::ctrl(Key::Z), Cmd::Undo, scope_id);

        // Scoped binding does not match when focused is outside scope
        let result = map.find(
            &Shortcut::ctrl(Key::Z),
            Some(focused_id),
            |_, _| false, // not in scope
        );
        assert_eq!(result, None);
    }

    #[test]
    fn scoped_binding_takes_priority_over_global() {
        use slotmap::KeyData;
        let scope_id: WidgetId = KeyData::from_ffi(10).into();
        let focused_id: WidgetId = KeyData::from_ffi(20).into();

        let map = ShortcutMap::new()
            .bind(Shortcut::ctrl(Key::Z), Cmd::Undo) // global
            .bind_scoped(Shortcut::ctrl(Key::Z), Cmd::Redo, scope_id); // scoped override

        // When focused in scope, scoped binding wins
        let result = map.find(&Shortcut::ctrl(Key::Z), Some(focused_id), |_, _| true);
        assert_eq!(result, Some(&Cmd::Redo));

        // When focused outside scope, global binding applies
        let result = map.find(&Shortcut::ctrl(Key::Z), Some(focused_id), |_, _| false);
        assert_eq!(result, Some(&Cmd::Undo));
    }
}
