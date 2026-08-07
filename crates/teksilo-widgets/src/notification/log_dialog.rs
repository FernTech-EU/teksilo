// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `NotificationLogDialog` — one-line modal preset wrapping
//! [`NotificationLog`] inside a
//! [`ModalContainer`].
//!
//! ```ignore
//! NotificationLogDialog::show(archive.clone(), ctx);
//! ```
//!
//! Presented through `ctx.present_modal(ModalRequest::deferred(…))`
//! — the modal layer picks Auto presentation (in-tree centered or
//! native window per platform conventions). The user can dismiss
//! via Escape or click-outside.

use std::rc::Rc;

use teksilo_core::modal::{ModalCloseBehavior, ModalPresentation, ModalRequest};
use teksilo_core::widget::EventContext;

use crate::dialog::ModalContainer;
use crate::notification::NotificationArchiveModel;
use crate::notification::log::NotificationLog;

/// One-liner modal preset around `NotificationLog`. Apps usually
/// wire this to a menu item or shortcut (e.g. "Window → Notification
/// Log…").
pub struct NotificationLogDialog;

impl NotificationLogDialog {
    /// Present the dialog with the standard chrome (title +
    /// 720x520 default size, escape-or-click-outside dismissal).
    pub fn show(archive: Rc<NotificationArchiveModel>, ctx: &mut EventContext) {
        Self::show_with(archive, ctx, |log| log);
    }

    /// Same as `show`, but lets the caller configure the embedded
    /// `NotificationLog` (e.g. attach an `on_action_invoked` hook
    /// for archive replay).
    pub fn show_with(
        archive: Rc<NotificationArchiveModel>,
        ctx: &mut EventContext,
        configure: impl FnOnce(NotificationLog) -> NotificationLog + 'static,
    ) {
        ctx.present_modal(
            ModalRequest::deferred(move |tree| {
                let log = configure(NotificationLog::new(archive));
                tree.add(
                    ModalContainer::new(log)
                        .title(teksilo_i18n::tr_widget!(notifications_title()))
                        .min_width(640.0),
                )
            })
            .presentation(ModalPresentation::Auto)
            .close_behavior(ModalCloseBehavior::EscapeOrClickOutside)
            .size(720, 520),
        );
    }
}
