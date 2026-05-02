//! `fern-fmt-lsp` — minimal Language Server Protocol server for the
//! `fern!` DSL formatter.
//!
//! Speaks JSON-RPC 2.0 over LSP framing on stdin/stdout. Hand-rolled
//! over `serde_json` and `std::io` to avoid the tokio/tower-lsp
//! transitive-dep load — the protocol surface we need is small:
//!
//! - `initialize` / `initialized` — handshake.
//! - `textDocument/didOpen` / `didChange` / `didClose` — track text.
//!   We advertise full-document sync (no incremental diffs).
//! - `textDocument/formatting` — run `fern_fmt::format_file` on the
//!   stored text and return a single `TextEdit` replacing the whole
//!   document.
//! - `shutdown` / `exit`.
//!
//! Editor wiring (example for Helix / VSCode / nvim):
//!
//! ```text
//! command:  fern-fmt-lsp
//! filetype: rust
//! ```
//!
//! Most editors already run `rust-analyzer` for `.rs` files; fern-fmt-lsp
//! is meant to coexist as a *secondary* formatter. Users who don't
//! have a way to chain formatters can keep using `cargo fern-fmt` on
//! save instead — the LSP path is for editor-native integrations.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Stdin, Stdout, Write};

use fern_fmt::{FmtConfig, format_file};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

mod protocol;

use protocol::{Position, Range, TextEdit};

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut server = Server::new();
    server.run(stdin, stdout)
}

struct Server {
    documents: HashMap<String, String>,
    initialized: bool,
    shutting_down: bool,
}

impl Server {
    fn new() -> Self {
        Self {
            documents: HashMap::new(),
            initialized: false,
            shutting_down: false,
        }
    }

    fn run(&mut self, stdin: Stdin, stdout: Stdout) -> io::Result<()> {
        let mut reader = BufReader::new(stdin.lock());
        let mut writer = stdout.lock();
        loop {
            let msg = match read_message(&mut reader)? {
                Some(m) => m,
                None => return Ok(()), // EOF
            };
            if let Some(response) = self.dispatch(&msg) {
                write_message(&mut writer, &response)?;
            }
            // After `exit`, terminate the loop.
            if msg.method.as_deref() == Some("exit") {
                return Ok(());
            }
        }
    }

    fn dispatch(&mut self, msg: &IncomingMessage) -> Option<Value> {
        let method = msg.method.as_deref().unwrap_or("");
        let id = msg.id.clone();
        let params = msg.params.clone().unwrap_or(Value::Null);

        match (method, id.is_some()) {
            ("initialize", true) => Some(ok(id.unwrap(), self.handle_initialize())),
            ("initialized", false) => {
                self.initialized = true;
                None
            }
            ("textDocument/didOpen", false) => {
                self.handle_did_open(&params);
                None
            }
            ("textDocument/didChange", false) => {
                self.handle_did_change(&params);
                None
            }
            ("textDocument/didClose", false) => {
                self.handle_did_close(&params);
                None
            }
            ("textDocument/formatting", true) => {
                Some(self.handle_formatting(id.unwrap(), &params))
            }
            ("shutdown", true) => {
                self.shutting_down = true;
                Some(ok(id.unwrap(), Value::Null))
            }
            ("exit", false) => None,
            (_, true) => Some(err(
                id.unwrap(),
                METHOD_NOT_FOUND,
                &format!("method not handled: {method}"),
            )),
            _ => None,
        }
    }

    fn handle_initialize(&self) -> Value {
        json!({
            "capabilities": {
                // Full-document sync: every change resends the whole text.
                // Cheaper to implement than incremental diff tracking and
                // formatting always reads the whole doc anyway.
                "textDocumentSync": 1,
                "documentFormattingProvider": true,
            },
            "serverInfo": {
                "name": "fern-fmt-lsp",
                "version": env!("CARGO_PKG_VERSION"),
            }
        })
    }

    fn handle_did_open(&mut self, params: &Value) {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or("").to_string();
        let text = params["textDocument"]["text"].as_str().unwrap_or("").to_string();
        self.documents.insert(uri, text);
    }

    fn handle_did_change(&mut self, params: &Value) {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or("").to_string();
        // With full-sync (mode 1) the last contentChange.text is the
        // entire new document. We pick the last entry defensively.
        let new_text = params["contentChanges"]
            .as_array()
            .and_then(|arr| arr.last())
            .and_then(|c| c["text"].as_str())
            .unwrap_or("")
            .to_string();
        self.documents.insert(uri, new_text);
    }

    fn handle_did_close(&mut self, params: &Value) {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or("").to_string();
        self.documents.remove(&uri);
    }

    fn handle_formatting(&self, id: Value, params: &Value) -> Value {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
        let Some(text) = self.documents.get(uri) else {
            // No document tracked — return empty edits rather than an
            // error. Editors sometimes call formatting before didOpen
            // races complete.
            return ok(id, json!([]));
        };
        let cfg = FmtConfig::default();
        match format_file(text, &cfg) {
            Ok(formatted) if formatted == *text => ok(id, json!([])),
            Ok(formatted) => {
                let edit = whole_document_edit(text, &formatted);
                ok(id, json!([edit]))
            }
            // A parse error in the host file or in a fern! body is not
            // a server fault — return empty edits so the editor leaves
            // the buffer alone, mirroring how rustfmt handles invalid
            // Rust during format-on-save.
            Err(_) => ok(id, json!([])),
        }
    }
}

fn whole_document_edit(original: &str, formatted: &str) -> TextEdit {
    let end = end_position(original);
    TextEdit {
        range: Range {
            start: Position { line: 0, character: 0 },
            end,
        },
        new_text: formatted.to_string(),
    }
}

/// Compute the LSP `Position` past the last character of `text`. LSP
/// addresses positions in UTF-16 code units, but for ASCII text — and
/// the typical Rust source — UTF-16 == byte == char count, so we use
/// char counts on the last line which is correct for both pure ASCII
/// and all-BMP non-ASCII content. Astral plane characters (emoji
/// outside U+FFFF) would need a true UTF-16 count; deferred.
fn end_position(text: &str) -> Position {
    let mut line = 0u32;
    let mut last_line_chars = 0u32;
    for c in text.chars() {
        if c == '\n' {
            line += 1;
            last_line_chars = 0;
        } else {
            last_line_chars += 1;
        }
    }
    Position {
        line,
        character: last_line_chars,
    }
}

// ---------------------------------------------------------------------------
// JSON-RPC framing + helpers
// ---------------------------------------------------------------------------

const METHOD_NOT_FOUND: i64 = -32601;

#[derive(Deserialize, Serialize, Debug)]
struct IncomingMessage {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

fn ok(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn err(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

/// Read one LSP-framed message. Returns `Ok(None)` on clean EOF.
fn read_message<R: BufRead>(reader: &mut R) -> io::Result<Option<IncomingMessage>> {
    let mut content_length: Option<usize> = None;
    let mut header = String::new();
    loop {
        header.clear();
        let n = reader.read_line(&mut header)?;
        if n == 0 {
            return Ok(None);
        }
        let trimmed = header.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
            content_length = rest.trim().parse().ok();
        }
        // Other headers (Content-Type, etc.) are tolerated and ignored.
    }
    let len = content_length.ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length header")
    })?;
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body).map(Some).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("invalid JSON: {e}"))
    })
}

fn write_message<W: Write>(w: &mut W, msg: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(msg)?;
    write!(w, "Content-Length: {}\r\n\r\n", body.len())?;
    w.write_all(&body)?;
    w.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_position_empty_text() {
        let p = end_position("");
        assert_eq!(p.line, 0);
        assert_eq!(p.character, 0);
    }

    #[test]
    fn end_position_single_line() {
        let p = end_position("hello");
        assert_eq!(p.line, 0);
        assert_eq!(p.character, 5);
    }

    #[test]
    fn end_position_multi_line() {
        let p = end_position("a\nbb\nccc");
        assert_eq!(p.line, 2);
        assert_eq!(p.character, 3);
    }

    #[test]
    fn end_position_trailing_newline() {
        let p = end_position("hello\n");
        assert_eq!(p.line, 1);
        assert_eq!(p.character, 0);
    }

    #[test]
    fn dispatch_initialize_returns_capabilities() {
        let mut s = Server::new();
        let msg = IncomingMessage {
            id: Some(json!(1)),
            method: Some("initialize".into()),
            params: Some(json!({})),
        };
        let resp = s.dispatch(&msg).expect("response");
        let caps = &resp["result"]["capabilities"];
        assert_eq!(caps["documentFormattingProvider"], json!(true));
        assert_eq!(caps["textDocumentSync"], json!(1));
    }

    #[test]
    fn formatting_returns_empty_edits_when_already_formatted() {
        let mut s = Server::new();
        let canonical =
            "fn build() {\n    fern!(VStack {\n        spacing: 8.0\n    });\n}\n";
        s.documents
            .insert("file:///a.rs".to_string(), canonical.to_string());
        let resp = s.handle_formatting(
            json!(2),
            &json!({ "textDocument": { "uri": "file:///a.rs" } }),
        );
        assert_eq!(resp["result"], json!([]));
    }

    #[test]
    fn formatting_returns_full_replace_when_dirty() {
        let mut s = Server::new();
        let dirty = "fn build() { fern!(VStack { spacing: 8.0 Button(\"ok\") }); }\n";
        s.documents
            .insert("file:///a.rs".to_string(), dirty.to_string());
        let resp = s.handle_formatting(
            json!(2),
            &json!({ "textDocument": { "uri": "file:///a.rs" } }),
        );
        let arr = resp["result"].as_array().expect("edits array");
        assert_eq!(arr.len(), 1);
        assert!(arr[0]["newText"].as_str().unwrap().contains("VStack {\n"));
    }

    #[test]
    fn formatting_unknown_uri_returns_empty_edits() {
        let mut s = Server::new();
        let resp = s.handle_formatting(
            json!(3),
            &json!({ "textDocument": { "uri": "file:///never-opened.rs" } }),
        );
        assert_eq!(resp["result"], json!([]));
    }

    #[test]
    fn formatting_invalid_rust_returns_empty_edits() {
        let mut s = Server::new();
        // Garbage that won't parse via syn::parse_file. We treat parse
        // errors as "leave it alone" rather than returning a JSON-RPC
        // error — same model rustfmt uses on save.
        s.documents
            .insert("file:///b.rs".to_string(), "fn { not rust !!! }".to_string());
        let resp = s.handle_formatting(
            json!(4),
            &json!({ "textDocument": { "uri": "file:///b.rs" } }),
        );
        assert_eq!(resp["result"], json!([]));
    }
}
