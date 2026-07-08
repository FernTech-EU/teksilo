// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

pub mod accessibility_prefs;
pub mod clipboard;
pub mod event_translation;
pub mod external_dnd;
#[cfg(feature = "file-dialog")]
pub mod file_dialog;
#[cfg(target_os = "linux")]
pub(crate) mod linux_helpers;
#[cfg(feature = "native-menu")]
pub mod native_menu;
pub mod os_theme;
pub mod title_bar_host;
pub mod window;
pub mod window_activation;
pub mod window_system;

pub use accessibility_prefs::AccessibilityPreferences;
#[cfg(feature = "clipboard")]
pub use clipboard::ArboardClipboard;
pub use clipboard::{ClipboardBackend, ClipboardHandle, MemoryClipboard};
pub use event_translation::TranslationState;
#[cfg(feature = "rfd-backend")]
pub use file_dialog::RfdAsyncBackend;
#[cfg(feature = "file-dialog")]
pub use file_dialog::{
    EventContextFileDialogExt, FileDialogBackend, FileDialogEventPayload, FileDialogHandle,
    FileDialogRequest, FileDialogResult, FileFilter, MemoryFileDialog, RequestId,
};
pub use title_bar_host::create_title_bar_host;
pub use window::{FrameOutcome, PlatformWindow};
pub use window_system::{
    WindowSystem, active_window_system, attach_child_window, supports_native_modal_windows,
};
