// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The system clipboard over the core Wayland protocol (`wl_data_device`).
//!
//! The protocol work — the device, the serial, serving and receiving — is the
//! shared [`WaylandDataHub`]'s; this module is the clipboard's policy on top of
//! it: which MIME types we offer, which of another application's we take, and
//! how their bytes become text.
//!
//! # Offering
//!
//! A copy offers, in one source, `text/html` (for [`set_html`]) and the plain
//! text under every name a Linux application asks for it:
//! `text/plain;charset=utf-8`, `UTF8_STRING`, `text/plain` and `TEXT` (all
//! UTF-8), and `STRING` (ISO 8859-1, as the X11 conventions XWayland follows
//! define it). The HTML goes out with a `<meta charset>` declaration when it has
//! none, the way browsers put HTML on the clipboard: without one, an importer
//! that guesses (LibreOffice does) reads UTF-8 as Windows-1252.
//!
//! # Taking
//!
//! A paste takes the best text type offered, in the order
//! [`TextMime::rank`] gives, and decodes it by its charset. HTML is decoded by
//! its byte-order mark first (Firefox used to send UTF-16 with one), then its
//! MIME `charset` parameter, then as UTF-8.
//!
//! What this process copied is answered from memory, exactly as it was given,
//! without a round trip through the compositor: the editor's self-paste
//! detection compares what it copied with what the clipboard reports, and a
//! re-decoded copy (with a charset declaration added) would not compare equal.
//!
//! [`set_html`]: super::ClipboardBackend::set_html

use std::sync::Arc;

use super::ClipboardBackend;
use crate::wayland_data::io::{Limits, read_bounded};
use crate::wayland_data::{Payload, SelectionView, WaylandDataHub};

/// What this process put on the clipboard, kept to answer its own paste.
#[derive(Debug)]
struct OwnContent {
    text: String,
    html: Option<String>,
}

/// The system clipboard on a Wayland display. See the module docs.
pub(crate) struct WaylandClipboard {
    hub: Arc<WaylandDataHub>,
}

impl WaylandClipboard {
    /// The clipboard of winit's Wayland display.
    pub(crate) fn new(display: &winit::event_loop::OwnedDisplayHandle) -> Result<Self, String> {
        Ok(Self {
            hub: WaylandDataHub::for_display(display)?,
        })
    }

    /// The clipboard on a hub the caller made (the compositor tests).
    #[cfg(test)]
    pub(crate) fn on_hub(hub: Arc<WaylandDataHub>) -> Self {
        Self { hub }
    }

    fn view(&self) -> Result<SelectionView, String> {
        self.hub.with_state(|state| state.selection())
    }

    /// Read `mime` from another application's selection.
    fn read(&self, mime: &str) -> Result<Vec<u8>, String> {
        // The hub is released before the read: its thread keeps serving the
        // compositor meanwhile.
        let fd = self
            .hub
            .with_state(|state| state.receive_selection(mime))??;
        read_bounded(fd, Limits::PASTE)
            .map_err(|err| format!("reading the clipboard as {mime} failed: {err}"))
    }

    fn set(&self, text: &str, html: Option<&str>) -> Result<(), String> {
        let payload = payload(text, html);
        let owner = Arc::new(OwnContent {
            text: text.to_string(),
            html: html.map(str::to_string),
        });
        self.hub
            .with_state(|state| state.set_selection(payload, owner))?
    }
}

fn own(data: &Arc<dyn std::any::Any + Send + Sync>) -> Option<&OwnContent> {
    data.downcast_ref::<OwnContent>()
}

impl ClipboardBackend for WaylandClipboard {
    fn get_text(&mut self) -> Result<String, String> {
        match self.view()? {
            SelectionView::Own(data) => own(&data)
                .map(|content| content.text.clone())
                .ok_or_else(|| "the clipboard holds no text".to_string()),
            SelectionView::Foreign(mimes) => {
                let best = best_text_mime(&mimes)
                    .ok_or_else(|| "the clipboard holds no text".to_string())?;
                let bytes = self.read(best.mime)?;
                Ok(best.decode(&bytes))
            }
            SelectionView::Empty => Err("the clipboard is empty".into()),
        }
    }

    fn set_text(&mut self, text: &str) -> Result<(), String> {
        self.set(text, None)
    }

    fn has_text(&mut self) -> bool {
        match self.view() {
            Ok(SelectionView::Own(data)) => own(&data).is_some_and(|c| !c.text.is_empty()),
            Ok(SelectionView::Foreign(mimes)) => best_text_mime(&mimes).is_some(),
            Ok(SelectionView::Empty) | Err(_) => false,
        }
    }

    fn get_html(&mut self) -> Result<String, String> {
        match self.view()? {
            SelectionView::Own(data) => own(&data)
                .and_then(|content| content.html.clone())
                .filter(|html| !html.is_empty())
                .ok_or_else(|| "no html payload".to_string()),
            SelectionView::Foreign(mimes) => {
                let mime = html_mime(&mimes).ok_or_else(|| "no html payload".to_string())?;
                let bytes = self.read(mime)?;
                Ok(decode_html(mime, &bytes))
            }
            SelectionView::Empty => Err("the clipboard is empty".into()),
        }
    }

    fn set_html(&mut self, html: &str, plain_fallback: &str) -> Result<(), String> {
        self.set(plain_fallback, Some(html))
    }

    fn has_html(&mut self) -> bool {
        match self.view() {
            Ok(SelectionView::Own(data)) => own(&data)
                .and_then(|c| c.html.as_deref())
                .is_some_and(|html| !html.is_empty()),
            Ok(SelectionView::Foreign(mimes)) => html_mime(&mimes).is_some(),
            Ok(SelectionView::Empty) | Err(_) => false,
        }
    }
}

// ---------------------------------------------------------------------------
// MIME policy (pure, tested without a compositor)
// ---------------------------------------------------------------------------

/// The text types a copy offers, each with how its bytes are written.
const TEXT_OFFERS: &[(&str, Charset)] = &[
    ("text/plain;charset=utf-8", Charset::Utf8),
    ("UTF8_STRING", Charset::Utf8),
    ("text/plain", Charset::Utf8),
    ("TEXT", Charset::Utf8),
    ("STRING", Charset::Latin1),
];

/// The HTML type a copy offers.
const HTML_MIME: &str = "text/html";

/// What a copy of `text` (and `html`) offers, in one source.
fn payload(text: &str, html: Option<&str>) -> Payload {
    let mut payload: Payload = Vec::with_capacity(TEXT_OFFERS.len() + 1);
    if let Some(html) = html {
        payload.push((
            HTML_MIME.to_string(),
            Arc::from(with_charset_declaration(html).into_bytes()),
        ));
    }
    let utf8: Arc<[u8]> = Arc::from(text.as_bytes());
    for (mime, charset) in TEXT_OFFERS {
        let bytes = match charset {
            Charset::Latin1 => Arc::from(encode_latin1(text)),
            _ => utf8.clone(),
        };
        payload.push(((*mime).to_string(), bytes));
    }
    payload
}

/// `html`, with a UTF-8 declaration in front unless it declares a charset of
/// its own near its head.
fn with_charset_declaration(html: &str) -> String {
    let head: String = html
        .chars()
        .take(1024)
        .collect::<String>()
        .to_ascii_lowercase();
    if head.contains("charset") {
        html.to_string()
    } else {
        format!("<meta http-equiv=\"content-type\" content=\"text/html; charset=utf-8\">{html}")
    }
}

/// ISO 8859-1, each character past U+00FF written as `?`.
fn encode_latin1(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
        .collect()
}

/// A character encoding we can read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Charset {
    Utf8,
    /// UTF-16 by its byte-order mark, little-endian without one.
    Utf16,
    Utf16Le,
    Utf16Be,
    Latin1,
    /// The X11 `TEXT` target: UTF-8 when it is valid UTF-8, else ISO 8859-1.
    Utf8OrLatin1,
}

impl Charset {
    fn from_label(label: &str) -> Option<Self> {
        match label.trim().trim_matches('"').to_ascii_lowercase().as_str() {
            "utf-8" | "utf8" => Some(Self::Utf8),
            "utf-16" | "utf16" | "unicode" => Some(Self::Utf16),
            "utf-16le" => Some(Self::Utf16Le),
            "utf-16be" => Some(Self::Utf16Be),
            "iso-8859-1" | "iso8859-1" | "iso_8859-1" | "latin1" | "latin-1" | "us-ascii"
            | "ascii" => Some(Self::Latin1),
            _ => None,
        }
    }

    fn decode(self, bytes: &[u8]) -> String {
        let text = match self {
            Self::Utf8 => decode_utf8(bytes),
            Self::Utf16 => match bytes {
                [0xFE, 0xFF, rest @ ..] => decode_utf16(rest, u16::from_be_bytes),
                [0xFF, 0xFE, rest @ ..] => decode_utf16(rest, u16::from_le_bytes),
                _ => decode_utf16(bytes, u16::from_le_bytes),
            },
            Self::Utf16Le => decode_utf16(strip_prefix(bytes, &[0xFF, 0xFE]), u16::from_le_bytes),
            Self::Utf16Be => decode_utf16(strip_prefix(bytes, &[0xFE, 0xFF]), u16::from_be_bytes),
            Self::Latin1 => bytes.iter().map(|&b| char::from(b)).collect(),
            Self::Utf8OrLatin1 => match std::str::from_utf8(bytes) {
                Ok(text) => text.to_string(),
                Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
            },
        };
        // C-string senders end with a NUL, which is not text.
        text.trim_end_matches('\0').to_string()
    }
}

fn strip_prefix<'a>(bytes: &'a [u8], prefix: &[u8]) -> &'a [u8] {
    bytes.strip_prefix(prefix).unwrap_or(bytes)
}

fn decode_utf8(bytes: &[u8]) -> String {
    String::from_utf8_lossy(strip_prefix(bytes, &[0xEF, 0xBB, 0xBF])).into_owned()
}

fn decode_utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> String {
    let units = bytes.as_chunks::<2>().0.iter().map(|pair| unit(*pair));
    char::decode_utf16(units)
        .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

/// A text type another application offers, and how to read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TextMime<'a> {
    mime: &'a str,
    charset: Charset,
    /// Lower is better.
    rank: u8,
}

impl TextMime<'_> {
    fn decode(&self, bytes: &[u8]) -> String {
        self.charset.decode(bytes)
    }
}

/// Split `type/subtype;param=value;…` into the lowercase essence and the
/// `charset` parameter, if any.
fn essence_and_charset(mime: &str) -> (String, Option<&str>) {
    let mut parts = mime.split(';');
    let essence = parts.next().unwrap_or("").trim().to_ascii_lowercase();
    let charset = parts.find_map(|param| {
        let (name, value) = param.split_once('=')?;
        name.trim().eq_ignore_ascii_case("charset").then_some(value)
    });
    (essence, charset)
}

impl<'a> TextMime<'a> {
    /// How good `mime` is as text, or `None` when it is not text we read.
    ///
    /// 0. `text/plain;charset=utf-8` — the standard, exact name;
    /// 1. `UTF8_STRING` — the X11 name for the same, all XWayland clients;
    /// 2. `text/plain` with another charset we can decode;
    /// 3. `text/plain` with none — UTF-8 in practice on Linux;
    /// 4. `TEXT` — X11's "any encoding", UTF-8 when it is valid UTF-8;
    /// 5. `STRING` — X11's ISO 8859-1, which loses everything past U+00FF.
    fn rank(mime: &'a str) -> Option<Self> {
        let (rank, charset) = match mime {
            "UTF8_STRING" => (1, Charset::Utf8),
            "TEXT" => (4, Charset::Utf8OrLatin1),
            "STRING" => (5, Charset::Latin1),
            _ => {
                let (essence, charset) = essence_and_charset(mime);
                if essence != "text/plain" {
                    return None;
                }
                match charset {
                    None => (3, Charset::Utf8),
                    Some(label) => match Charset::from_label(label)? {
                        Charset::Utf8 => (0, Charset::Utf8),
                        other => (2, other),
                    },
                }
            }
        };
        Some(Self {
            mime,
            charset,
            rank,
        })
    }
}

/// The best text type in `offered`.
fn best_text_mime(offered: &[String]) -> Option<TextMime<'_>> {
    offered
        .iter()
        .filter_map(|mime| TextMime::rank(mime))
        .min_by_key(|candidate| candidate.rank)
}

/// The HTML type in `offered`: `text/html` itself first, then one with
/// parameters.
fn html_mime(offered: &[String]) -> Option<&str> {
    offered
        .iter()
        .find(|m| m.as_str() == HTML_MIME)
        .or_else(|| {
            offered
                .iter()
                .find(|m| essence_and_charset(m).0 == HTML_MIME)
        })
        .map(String::as_str)
}

/// Decode HTML: byte-order mark first, then the type's `charset` parameter,
/// then UTF-8.
fn decode_html(mime: &str, bytes: &[u8]) -> String {
    let charset = match bytes {
        [0xEF, 0xBB, 0xBF, ..] => Charset::Utf8,
        [0xFF, 0xFE, ..] | [0xFE, 0xFF, ..] => Charset::Utf16,
        _ => essence_and_charset(mime)
            .1
            .and_then(Charset::from_label)
            .unwrap_or(Charset::Utf8),
    };
    charset.decode(bytes)
}

#[cfg(test)]
mod compositor_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn utf8_text_plain_beats_every_other_text_type() {
        let offered = strings(&[
            "STRING",
            "TEXT",
            "text/plain",
            "UTF8_STRING",
            "text/plain;charset=utf-8",
            "text/html",
        ]);
        assert_eq!(
            best_text_mime(&offered).map(|m| m.mime),
            Some("text/plain;charset=utf-8")
        );
    }

    #[test]
    fn the_ranking_falls_back_in_order() {
        let order = [
            "UTF8_STRING",
            "text/plain;charset=utf-16",
            "text/plain",
            "TEXT",
            "STRING",
        ];
        for i in 0..order.len() {
            let offered = strings(&order[i..]);
            assert_eq!(best_text_mime(&offered).map(|m| m.mime), Some(order[i]));
        }
    }

    #[test]
    fn charset_parameters_are_read_however_they_are_written() {
        for mime in [
            "text/plain;charset=UTF-8",
            "text/plain; charset=\"utf-8\"",
            "TEXT/PLAIN;CHARSET=utf8",
        ] {
            let offered = strings(&[mime]);
            let best = best_text_mime(&offered).unwrap();
            assert_eq!((best.rank, best.charset), (0, Charset::Utf8), "{mime}");
        }
    }

    #[test]
    fn an_unknown_charset_and_non_text_are_not_taken() {
        assert_eq!(
            best_text_mime(&strings(&[
                "text/plain;charset=koi8-r",
                "image/png",
                "text/uri-list"
            ])),
            None
        );
    }

    #[test]
    fn charsets_decode() {
        assert_eq!(Charset::Utf8.decode("été\0".as_bytes()), "été");
        assert_eq!(Charset::Utf8.decode(b"\xEF\xBB\xBFbom"), "bom");
        assert_eq!(Charset::Latin1.decode(&[0x63, 0x61, 0x66, 0xE9]), "café");
        assert_eq!(Charset::Utf16.decode(&[0xFF, 0xFE, b'h', 0, b'i', 0]), "hi");
        assert_eq!(Charset::Utf16.decode(&[0xFE, 0xFF, 0, b'h', 0, b'i']), "hi");
        assert_eq!(Charset::Utf16.decode(&[b'h', 0, b'i', 0]), "hi");
        assert_eq!(Charset::Utf16Be.decode(&[0, b'o', 0, b'k']), "ok");
        assert_eq!(Charset::Utf8OrLatin1.decode("été".as_bytes()), "été");
        assert_eq!(Charset::Utf8OrLatin1.decode(&[0xE9]), "é");
    }

    #[test]
    fn html_is_decoded_by_its_mark_then_its_charset_then_as_utf8() {
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("<b>é</b>".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(decode_html("text/html", &utf16), "<b>é</b>");
        assert_eq!(
            decode_html("text/html;charset=iso-8859-1", &[b'<', b'p', b'>', 0xE9]),
            "<p>é"
        );
        assert_eq!(decode_html("text/html", "<p>é</p>".as_bytes()), "<p>é</p>");
    }

    #[test]
    fn html_is_found_with_or_without_parameters() {
        assert_eq!(
            html_mime(&strings(&["text/plain", "text/html"])),
            Some("text/html")
        );
        assert_eq!(
            html_mime(&strings(&["text/html;charset=utf-8"])),
            Some("text/html;charset=utf-8")
        );
        assert_eq!(html_mime(&strings(&["text/plain", "text/xhtml"])), None);
    }

    #[test]
    fn a_copy_of_text_offers_every_text_name_and_no_html() {
        let payload = payload("café ✓", None);
        let mimes: Vec<&str> = payload.iter().map(|(m, _)| m.as_str()).collect();
        assert_eq!(
            mimes,
            [
                "text/plain;charset=utf-8",
                "UTF8_STRING",
                "text/plain",
                "TEXT",
                "STRING"
            ]
        );
        for (mime, bytes) in &payload {
            let expected: &[u8] = if mime == "STRING" {
                b"caf\xE9 ?"
            } else {
                "café ✓".as_bytes()
            };
            assert_eq!(&**bytes, expected, "{mime}");
        }
    }

    #[test]
    fn a_copy_of_html_offers_html_and_its_plain_text_in_one_source() {
        let payload = payload("plain", Some("<p>rich</p>"));
        assert_eq!(payload[0].0, "text/html");
        let html = std::str::from_utf8(&payload[0].1).unwrap();
        assert!(html.ends_with("<p>rich</p>"));
        assert!(html.contains("charset=utf-8"));
        let plain = payload
            .iter()
            .find(|(m, _)| m == "text/plain;charset=utf-8")
            .unwrap();
        assert_eq!(&*plain.1, b"plain");
    }

    #[test]
    fn html_that_declares_its_charset_is_offered_unchanged() {
        let html = "<html><head><meta charset=\"utf-8\"></head><body>x</body></html>";
        assert_eq!(with_charset_declaration(html), html);
    }

    #[test]
    fn the_editor_marker_stays_near_the_head() {
        // The rich-text editor finds its own copies by a comment in the first
        // 1024 bytes; the declaration must not push it out of that window.
        let html = "<!--teksilo-rtc:0123456789abcdef0123456789abcdef--><p>x</p>";
        let served = with_charset_declaration(html);
        assert!(served.find("teksilo-rtc:").unwrap() < 200);
    }

    #[test]
    fn our_copy_round_trips_through_our_own_decoder() {
        for (mime, bytes) in payload("ligne 1\nligne 2 — ✓", None) {
            let decoded = best_text_mime(std::slice::from_ref(&mime))
                .unwrap()
                .decode(&bytes);
            let expected = if mime == "STRING" {
                "ligne 1\nligne 2 ? ?"
            } else {
                "ligne 1\nligne 2 — ✓"
            };
            assert_eq!(decoded, expected, "{mime}");
        }
    }
}
