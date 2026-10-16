// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Dialogs and pickers a page opens, for the application to present.
//!
//! A native engine shows `alert()`, `confirm()`, `prompt()` and a `<select>`'s
//! list itself. A [`WebViewRendering::Surface`](crate::WebViewRendering)
//! engine has no window to show them in, so its backend describes each one as
//! a [`WebViewControl`] and the widget hands it to the
//! [`WebViewControlPresenter`] installed in app-state. The presenter answers
//! once, with a [`WebViewControlAnswer`]; with no presenter installed, every
//! control gets its default answer at once (an alert acknowledged, a confirm
//! or prompt cancelled, a list left as it was), so no page waits on one.
//!
//! `teksilo`'s `install_web_view_default` installs a presenter built on
//! `MessageBox`, `InputDialog` and `MenuList` when the `widgets` feature is on.

use std::rc::Rc;

use teksilo_canvas::Rect;
use teksilo_core::widget::EventContext;

/// One option of a `<select>`.
#[derive(Debug, Clone, PartialEq)]
pub struct WebViewSelectOption {
    /// What [`WebViewControlAnswer::Selected`] names it by.
    pub id: usize,
    /// The option's text.
    pub label: String,
    /// Whether it can be chosen.
    pub disabled: bool,
    /// The label of the `<optgroup>` it belongs to, if any.
    pub group: Option<String>,
}

/// A dialog or picker a page opened. Its text is the page's, so a presenter
/// shows it in a way that cannot be mistaken for the application's own UI.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum WebViewControl {
    /// `alert(message)`: acknowledged with [`WebViewControlAnswer::Confirmed`].
    Alert { message: String },
    /// `confirm(message)`: [`WebViewControlAnswer::Confirmed`] or
    /// [`WebViewControlAnswer::Dismissed`].
    Confirm { message: String },
    /// `prompt(message, default)`: [`WebViewControlAnswer::Text`] or
    /// [`WebViewControlAnswer::Dismissed`].
    Prompt { message: String, default: String },
    /// A `<select>`'s list, opened at `area` (window-logical): answered with
    /// [`WebViewControlAnswer::Selected`].
    Select {
        options: Vec<WebViewSelectOption>,
        /// The option chosen now.
        selected: Option<usize>,
        area: Rect,
    },
}

/// How a presenter answered a [`WebViewControl`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebViewControlAnswer {
    /// Cancelled, closed, or for a list, left as it was.
    Dismissed,
    /// The alert acknowledged, the confirm accepted.
    Confirmed,
    /// The prompt's text.
    Text(String),
    /// The option chosen in a list, by [`WebViewSelectOption::id`].
    Selected(usize),
}

/// The answer callback a presenter calls once.
pub type WebViewControlReply = Box<dyn FnOnce(WebViewControlAnswer)>;

/// Presents a page's dialogs and pickers. See the [module docs](self).
pub trait WebViewControlPresenter: 'static {
    /// Show `control` and call `reply` once with the user's answer.
    fn present(&self, control: WebViewControl, ctx: &mut EventContext, reply: WebViewControlReply);
}

/// The presenter, as app-state holds it:
/// `.app_state(SharedWebViewControlPresenter::new(MyPresenter))`.
#[derive(Clone)]
pub struct SharedWebViewControlPresenter(pub Rc<dyn WebViewControlPresenter>);

impl SharedWebViewControlPresenter {
    /// Wrap `presenter` for app-state.
    pub fn new(presenter: impl WebViewControlPresenter) -> Self {
        Self(Rc::new(presenter))
    }
}

impl std::fmt::Debug for SharedWebViewControlPresenter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SharedWebViewControlPresenter")
    }
}
