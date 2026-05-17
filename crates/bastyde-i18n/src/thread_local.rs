//! Thread-local handle to the active `I18nManager`.
//!
//! `BastydeAppBuilder` calls `install` once at startup, after constructing the
//! manager. `LocalizedString::to_signal` and the `resolve_message` free
//! functions reach this thread-local without threading the manager through
//! every call site — the same pattern Rust's logger and panic hook use.

use std::cell::RefCell;
use std::rc::Rc;

use bastyde_core::environment::LayoutDirection;
use bastyde_core::signal::Signal;
use unic_langid::LanguageIdentifier;

use crate::manager::I18nManager;

thread_local! {
    static ACTIVE: RefCell<Option<Rc<I18nManager>>> = const { RefCell::new(None) };
}

/// Install a manager on the current thread. Replaces any previously
/// installed manager.
pub fn install(mgr: Rc<I18nManager>) {
    ACTIVE.with(|slot| {
        *slot.borrow_mut() = Some(mgr);
    });
}

/// Remove the current thread's manager.
pub fn clear() {
    ACTIVE.with(|slot| {
        *slot.borrow_mut() = None;
    });
}

/// Run a closure with the active manager. Returns `None` if no manager is
/// installed on this thread.
pub fn with_active<R>(f: impl FnOnce(&I18nManager) -> R) -> Option<R> {
    ACTIVE.with(|slot| slot.borrow().as_ref().map(|mgr| f(mgr.as_ref())))
}

/// Clone of the active manager's version signal, or `None` if no manager
/// is installed.
pub fn current_version_signal() -> Option<Signal<u64>> {
    with_active(|mgr| mgr.version_signal().clone())
}

/// Clone of the active manager's locale signal, or `None` if no manager
/// is installed.
pub fn current_locale() -> Option<Signal<LanguageIdentifier>> {
    with_active(|mgr| mgr.locale_signal().clone())
}

/// Clone of the active manager's layout direction signal, or `None` if no
/// manager is installed.
pub fn current_direction() -> Option<Signal<LayoutDirection>> {
    with_active(|mgr| mgr.direction_signal().clone())
}

/// Snapshot the supported-locales list from the active manager.
/// Returns `None` if no manager is installed.
pub fn current_supported_locales() -> Option<Vec<LanguageIdentifier>> {
    with_active(|mgr| mgr.supported_locales().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::I18nConfig;

    #[test]
    fn install_and_read_back() {
        clear();
        assert!(current_version_signal().is_none());

        let cfg = I18nConfig::test_only("en-US", &[("k", "v")]);
        let mgr = I18nManager::from_config(&cfg);
        install(mgr);

        assert!(current_version_signal().is_some());
        assert!(current_locale().is_some());
        assert!(current_direction().is_some());
        clear();
    }
}
