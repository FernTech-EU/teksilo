// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `window.ipc.postMessage` for a Servo page: JS→Rust messages.
//!
//! Servo has no message channel from a page to its embedder. This one is a
//! private URL scheme: [`IPC_SCRIPT`], run in every page, defines
//! `window.ipc.postMessage(m)` as a `fetch` of `teksilo-ipc://post?<m>`, and
//! [`IpcHandler`] answers each such request by posting the message as a
//! [`WebViewEvent::Message`] to the web view that made it. The message travels
//! in the URL because a request body reaches a protocol handler as a stream of
//! chunks from another process, and a URL is already whole.
//!
//! Servo names the web view a request came from
//! (`Request::target_webview_id`), so a page cannot post as another one.
//! Requests are answered on Servo's network threads, so two messages posted
//! back to back may arrive out of order.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use servo::WebViewId as ServoWebViewId;
use servo::protocol_handler::{
    DoneChannel, FetchContext, HttpStatus, ProtocolHandler, Request, ResourceFetchTiming, Response,
    ResponseBody,
};
use teksilo_core::AppEventPoster;
use teksilo_core::window::TeksiloWindowId;

use crate::backend::{WebViewEvent, WebViewId, post_event};

/// The URL scheme the page's messages travel by.
pub(super) const IPC_SCHEME: &str = "teksilo-ipc";

/// Run in every page before its own scripts.
pub(super) const IPC_SCRIPT: &str = "window.ipc = Object.freeze({ postMessage(message) { \
     fetch('teksilo-ipc://post?' + encodeURIComponent(String(message)), \
     { mode: 'no-cors', cache: 'no-store' }).catch(() => {}); } });";

/// Where a web view's messages go.
#[derive(Clone)]
pub(super) struct Route {
    pub(super) web_view_id: WebViewId,
    pub(super) window_id: TeksiloWindowId,
    pub(super) poster: Option<Arc<dyn AppEventPoster>>,
}

/// Every open web view's route, by Servo's id for it.
pub(super) type Routes = Arc<Mutex<HashMap<ServoWebViewId, Route>>>;

/// Answers `teksilo-ipc:` requests. See the [module docs](self).
pub(super) struct IpcHandler {
    pub(super) routes: Routes,
}

impl ProtocolHandler for IpcHandler {
    fn load<'a>(
        &'a self,
        request: &'a mut Request,
        _done_chan: &mut DoneChannel,
        _context: &FetchContext,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>> {
        let url = request.current_url();
        let route = request.target_webview_id.and_then(|id| {
            self.routes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(&id)
                .cloned()
        });
        if let (Some(route), Some(message)) = (route, message(url.as_str())) {
            post_event(
                &route.poster,
                route.window_id,
                route.web_view_id,
                WebViewEvent::Message(message),
            );
        }
        let mut response = Response::new(url, ResourceFetchTiming::new(request.timing_type()));
        *response.body.lock() = ResponseBody::Done(Vec::new());
        response.status = HttpStatus::default();
        Box::pin(std::future::ready(response))
    }

    fn is_fetchable(&self) -> bool {
        true
    }
}

/// The message a `teksilo-ipc://post?...` URL carries, percent-decoded.
fn message(url: &str) -> Option<String> {
    let query = url.strip_prefix(IPC_SCHEME)?.strip_prefix("://post?")?;
    let mut bytes = Vec::with_capacity(query.len());
    let mut rest = query.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        if byte == b'%'
            && let [h, l, ..] = tail
            && let (Some(h), Some(l)) = (hex(*h), hex(*l))
        {
            bytes.push(h << 4 | l);
            rest = &tail[2..];
        } else {
            bytes.push(byte);
            rest = tail;
        }
    }
    String::from_utf8(bytes).ok()
}

fn hex(digit: u8) -> Option<u8> {
    (digit as char).to_digit(16).map(|d| d as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_is_decoded_from_its_url() {
        // encodeURIComponent("a b+é\n\u{2028}%")
        assert_eq!(
            message("teksilo-ipc://post?a%20b%2B%C3%A9%0A%E2%80%A8%25").as_deref(),
            Some("a b+é\n\u{2028}%")
        );
        assert_eq!(message("teksilo-ipc://post?").as_deref(), Some(""));
        assert_eq!(message("https://example.com/?x"), None);
    }
}
