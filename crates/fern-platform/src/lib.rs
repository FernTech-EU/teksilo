pub mod accessibility_prefs;
pub mod clipboard;
pub mod event_translation;
#[cfg(target_os = "linux")]
pub(crate) mod linux_helpers;
pub mod os_theme;
pub mod title_bar_host;
pub mod window;
pub mod window_system;

pub use accessibility_prefs::AccessibilityPreferences;
pub use clipboard::{ClipboardBackend, ClipboardHandle, MemoryClipboard};
#[cfg(feature = "clipboard")]
pub use clipboard::ArboardClipboard;
pub use event_translation::TranslationState;
pub use title_bar_host::create_title_bar_host;
pub use window::PlatformWindow;
pub use window_system::{WindowSystem, active_window_system, supports_native_modal_windows};
