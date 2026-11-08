// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Which open window each string id names, and changing a window's id.
//!
//! A window's string id ([`WindowConfig::id`](teksilo_core::WindowConfig::id))
//! does two jobs: [`find_window`](teksilo_core::WindowOps::find_window) looks
//! a window up by it, and its geometry is saved under it in the
//! [`WindowStateService`]. An app that keys a document window by the
//! document's path needs to change it when the document moves (Save As, a
//! backup restored, a switch in place), and both jobs must follow, or the
//! file left behind raises the moved window and the moved document reopens
//! at the default size.
//!
//! The id itself lives in the window's [`WindowState`]; this table is the
//! reverse lookup, kept in step with it by the window manager on open, on
//! close and on [`rename`](StringIds::rename). Nothing here needs a real
//! window, so the rules are tested without a display.

use std::collections::HashMap;

use teksilo_core::{TeksiloWindowId, WindowState, WindowStringIdError};
use teksilo_settings::WindowStateService;

/// String id to window, for the open windows that have one.
///
/// Opening two windows with one id is allowed (nothing refuses it at
/// open), and then the id names the newer of them, until it closes.
#[derive(Debug, Default)]
pub(crate) struct StringIds {
    by_string: HashMap<String, TeksiloWindowId>,
}

impl StringIds {
    /// The open window `string_id` names.
    pub(crate) fn find(&self, string_id: &str) -> Option<TeksiloWindowId> {
        self.by_string.get(string_id).copied()
    }

    /// `window` opened with `string_id`: from now on the id names it, even
    /// if an older window has it too.
    pub(crate) fn opened(&mut self, string_id: &str, window: TeksiloWindowId) {
        self.by_string.insert(string_id.to_owned(), window);
    }

    /// The window `string_id` named no longer has it (it closed, or was
    /// renamed): the id now names the newest of the `live` windows that
    /// still have it, or nothing. `live` must not count that window under
    /// `string_id` any more.
    pub(crate) fn released<'a>(
        &mut self,
        string_id: &str,
        live: impl IntoIterator<Item = &'a WindowState>,
    ) {
        match newest_holder(string_id, live, None) {
            Some(other) => {
                self.by_string.insert(string_id.to_owned(), other);
            }
            None => {
                self.by_string.remove(string_id);
            }
        }
    }

    /// Give `window`, one of the `live` windows, the string id `new_id`; see
    /// [`EventContext::set_window_string_id`](teksilo_core::widget::EventContext::set_window_string_id)
    /// for the contract.
    ///
    /// `live` is every open window's state, the one a handler is running in
    /// included (the window manager has it out of its map then). `service`
    /// is the app's geometry store, when it saves window geometry: the
    /// window's current geometry is saved under `new_id` and its row under
    /// the old id forgotten, unless another open window still has the old
    /// id (a window opened twice with one id) and saves its own there. The
    /// savers installed at open read the id when they save, so later moves
    /// land under `new_id` with nothing else to do here.
    pub(crate) fn rename(
        &mut self,
        window: TeksiloWindowId,
        new_id: String,
        live: &[WindowState],
        service: Option<&WindowStateService>,
    ) -> Result<(), WindowStringIdError> {
        let state = live
            .iter()
            .find(|state| state.id() == window)
            .ok_or(WindowStringIdError::NoSuchWindow(window))?;
        let old = state.string_id();
        if old.as_deref() == Some(new_id.as_str()) {
            return Ok(());
        }
        if let Some(holder) = newest_holder(&new_id, live, Some(window)) {
            return Err(WindowStringIdError::HeldByAnotherWindow {
                string_id: new_id,
                holder,
            });
        }
        state.string_id_signal().set(Some(new_id.clone()));
        self.opened(&new_id, window);
        let old_still_held = match old.as_deref() {
            Some(old) => {
                self.released(old, live);
                self.find(old).is_some()
            }
            None => false,
        };
        if let Some(service) = service {
            let forget = old.as_deref().filter(|_| !old_still_held);
            crate::window_persist::move_saved_geometry(service, state, forget);
        }
        Ok(())
    }
}

/// The newest of the `live` windows, other than `except`, whose string id is
/// `string_id`. Window ids are allocated in increasing order and never
/// reused, so the largest is the newest.
fn newest_holder<'a>(
    string_id: &str,
    live: impl IntoIterator<Item = &'a WindowState>,
    except: Option<TeksiloWindowId>,
) -> Option<TeksiloWindowId> {
    live.into_iter()
        .filter(|state| Some(state.id()) != except)
        .filter(|state| state.string_id().as_deref() == Some(string_id))
        .map(WindowState::id)
        .max_by_key(|id| id.raw())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use teksilo_core::{WindowPlacement, WindowStateInit};

    fn window(id: u64, string_id: Option<&str>, size: (u32, u32)) -> WindowState {
        WindowState::new(WindowStateInit {
            id: TeksiloWindowId::new(id),
            string_id: string_id.map(str::to_owned),
            placement: WindowPlacement::Floating,
            title: String::new(),
            size,
            position: (40, 30),
            focused: true,
            resizable: true,
            always_on_top: false,
        })
    }

    /// The windows `live`, opened in order, as the window manager records
    /// them.
    fn opened(live: &[WindowState]) -> StringIds {
        let mut ids = StringIds::default();
        for state in live {
            if let Some(sid) = state.string_id() {
                ids.opened(&sid, state.id());
            }
        }
        ids
    }

    fn service(dir: &tempfile::TempDir) -> WindowStateService {
        WindowStateService::open_at(dir.path().join("window_state.toml"), Duration::ZERO).unwrap()
    }

    /// After Save As, the window is found where its document went and not
    /// where it was: the file left behind is another copy, and opening it
    /// must open a window of its own.
    #[test]
    fn a_renamed_window_is_found_by_its_new_id_and_not_its_old_one() {
        let live = [window(1, Some("/a/Novel.skrib"), (800, 600))];
        let mut ids = opened(&live);

        ids.rename(
            TeksiloWindowId::new(1),
            "/a/Novel 2.skrib".into(),
            &live,
            None,
        )
        .unwrap();

        assert_eq!(ids.find("/a/Novel 2.skrib"), Some(TeksiloWindowId::new(1)));
        assert_eq!(ids.find("/a/Novel.skrib"), None);
        assert_eq!(live[0].string_id().as_deref(), Some("/a/Novel 2.skrib"));
    }

    /// The old id is free once the window has moved on: a window opened
    /// with it is a new window, found by it, and the renamed one keeps its
    /// new id.
    #[test]
    fn a_window_opened_with_the_old_id_after_a_rename_is_a_new_window() {
        let mut live = vec![window(1, Some("old"), (800, 600))];
        let mut ids = opened(&live);
        ids.rename(TeksiloWindowId::new(1), "new".into(), &live, None)
            .unwrap();

        assert_eq!(ids.find("old"), None, "nothing to raise: open a window");
        let second = window(2, Some("old"), (800, 600));
        ids.opened("old", second.id());
        live.push(second);

        assert_eq!(ids.find("old"), Some(TeksiloWindowId::new(2)));
        assert_eq!(ids.find("new"), Some(TeksiloWindowId::new(1)));
        // Closing the new one leaves the renamed one where it is.
        live.pop();
        ids.released("old", &live);
        assert_eq!(ids.find("old"), None);
        assert_eq!(ids.find("new"), Some(TeksiloWindowId::new(1)));
    }

    /// Two windows with one id would leave `find_window` naming one of them
    /// and their geometry one row both overwrite, so the rename is refused
    /// and nothing changes.
    #[test]
    fn an_id_another_open_window_holds_is_refused() {
        let live = [
            window(1, Some("a"), (800, 600)),
            window(2, Some("b"), (800, 600)),
        ];
        let mut ids = opened(&live);
        let dir = tempfile::tempdir().unwrap();
        let service = service(&dir);

        let refused = ids.rename(TeksiloWindowId::new(1), "b".into(), &live, Some(&service));

        assert_eq!(
            refused,
            Err(WindowStringIdError::HeldByAnotherWindow {
                string_id: "b".into(),
                holder: TeksiloWindowId::new(2),
            })
        );
        assert_eq!(ids.find("a"), Some(TeksiloWindowId::new(1)));
        assert_eq!(ids.find("b"), Some(TeksiloWindowId::new(2)));
        assert_eq!(live[0].string_id().as_deref(), Some("a"));
        assert!(service.state_for("b").is_none(), "no geometry moved");
    }

    /// Renaming a window to the id it has is a no-op, even when a second
    /// window was opened with that id too.
    #[test]
    fn renaming_a_window_to_its_own_id_changes_nothing() {
        let live = [
            window(1, Some("a"), (800, 600)),
            window(2, Some("a"), (640, 480)),
        ];
        let mut ids = opened(&live);
        let dir = tempfile::tempdir().unwrap();
        let service = service(&dir);

        assert_eq!(
            ids.rename(TeksiloWindowId::new(1), "a".into(), &live, Some(&service)),
            Ok(())
        );
        assert_eq!(ids.find("a"), Some(TeksiloWindowId::new(2)));
        assert!(service.labels().is_empty(), "nothing saved");
    }

    #[test]
    fn a_window_that_is_not_open_cannot_be_renamed() {
        let live = [window(1, Some("a"), (800, 600))];
        let mut ids = opened(&live);
        assert_eq!(
            ids.rename(TeksiloWindowId::new(7), "b".into(), &live, None),
            Err(WindowStringIdError::NoSuchWindow(TeksiloWindowId::new(7)))
        );
        assert_eq!(ids.find("b"), None);
    }

    /// The window's saved geometry follows it: after the rename the row is
    /// under the new id, at the window's current size, replacing a row a
    /// window long closed left there, and the old row is gone, so a restart
    /// on the moved document opens it at the size it was left at.
    #[test]
    fn the_saved_geometry_moves_to_the_new_id() {
        let dir = tempfile::tempdir().unwrap();
        let service = service(&dir);
        let live = [window(1, Some("old"), (800, 600))];
        let _savers = crate::window_persist::install_persist_observers(&live[0], service.clone());
        live[0].set_size_from_os((1000, 700));
        // A row from an earlier session of a window with the new id.
        service
            .record(teksilo_settings::PerWindowState {
                label: "new".into(),
                x: 0,
                y: 0,
                width: 300,
                height: 200,
                placement: WindowPlacement::Floating,
            })
            .unwrap();
        let mut ids = opened(&live);

        ids.rename(TeksiloWindowId::new(1), "new".into(), &live, Some(&service))
            .unwrap();

        let moved = service.state_for("new").expect("saved under the new id");
        assert_eq!((moved.width, moved.height), (1000, 700));
        assert!(
            service.state_for("old").is_none(),
            "the old row is forgotten"
        );
    }

    /// The savers installed when the window opened save under the id the
    /// window has when it moves, not the one it had when they were
    /// installed.
    #[test]
    fn later_moves_are_saved_under_the_new_id() {
        let dir = tempfile::tempdir().unwrap();
        let service = service(&dir);
        let live = [window(1, Some("old"), (800, 600))];
        let _savers = crate::window_persist::install_persist_observers(&live[0], service.clone());
        let mut ids = opened(&live);
        ids.rename(TeksiloWindowId::new(1), "new".into(), &live, Some(&service))
            .unwrap();

        live[0].set_size_from_os((1200, 900));

        let saved = service.state_for("new").expect("saved under the new id");
        assert_eq!((saved.width, saved.height), (1200, 900));
        assert!(
            service.state_for("old").is_none(),
            "nothing comes back under the old id"
        );
    }

    /// A window opened without an id gets one, and from then on its
    /// geometry is saved under it.
    #[test]
    fn a_window_opened_without_an_id_is_saved_once_it_has_one() {
        let dir = tempfile::tempdir().unwrap();
        let service = service(&dir);
        let live = [window(1, None, (800, 600))];
        let _savers = crate::window_persist::install_persist_observers(&live[0], service.clone());
        live[0].set_size_from_os((900, 650));
        assert!(service.labels().is_empty(), "nothing to save it under yet");
        let mut ids = opened(&live);

        ids.rename(TeksiloWindowId::new(1), "doc".into(), &live, Some(&service))
            .unwrap();
        assert_eq!(ids.find("doc"), Some(TeksiloWindowId::new(1)));
        assert_eq!(service.state_for("doc").map(|s| s.width), Some(900));

        live[0].set_size_from_os((950, 650));
        assert_eq!(service.state_for("doc").map(|s| s.width), Some(950));
    }

    /// When a second window still has the old id (it was opened twice), the
    /// id now names that one, and its geometry row stays: it is the other
    /// window's row, which it goes on saving to, and the renamed window's
    /// moves no longer land there.
    #[test]
    fn the_old_id_stays_with_another_window_that_has_it() {
        let dir = tempfile::tempdir().unwrap();
        let service = service(&dir);
        let live = [
            window(1, Some("old"), (800, 600)),
            window(2, Some("old"), (640, 480)),
        ];
        let _savers_1 = crate::window_persist::install_persist_observers(&live[0], service.clone());
        let _savers_2 = crate::window_persist::install_persist_observers(&live[1], service.clone());
        // Window 1 is the last to save under the id the two share.
        live[0].set_size_from_os((820, 610));
        let mut ids = opened(&live);

        ids.rename(TeksiloWindowId::new(2), "new".into(), &live, Some(&service))
            .unwrap();

        assert_eq!(ids.find("old"), Some(TeksiloWindowId::new(1)));
        assert_eq!(ids.find("new"), Some(TeksiloWindowId::new(2)));
        let size = |label: &str| service.state_for(label).map(|s| (s.width, s.height));
        assert_eq!(
            size("old"),
            Some((820, 610)),
            "the rename leaves window 1's row where it is"
        );
        assert_eq!(
            size("new"),
            Some((640, 480)),
            "window 2 is saved under its new id"
        );

        live[1].set_size_from_os((700, 500));
        live[0].set_size_from_os((900, 680));
        assert_eq!(
            size("new"),
            Some((700, 500)),
            "window 2 saves under its new id"
        );
        assert_eq!(size("old"), Some((900, 680)), "window 1 still saves there");
    }
}
