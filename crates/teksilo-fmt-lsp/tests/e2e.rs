// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! End-to-end test that drives the compiled `teksilo-fmt-lsp` binary
//! through a complete `initialize → didOpen → formatting → shutdown →
//! exit` exchange. Validates the LSP framing layer and the dispatch
//! glue together as a real editor would.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_teksilo-fmt-lsp"))
}

fn frame(msg: &str) -> String {
    format!("Content-Length: {}\r\n\r\n{}", msg.len(), msg)
}

fn read_one_message<R: BufRead>(r: &mut R) -> String {
    let mut len: Option<usize> = None;
    let mut header = String::new();
    loop {
        header.clear();
        r.read_line(&mut header).expect("read header");
        let trimmed = header.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
            len = rest.trim().parse().ok();
        }
    }
    let n = len.expect("Content-Length");
    let mut body = vec![0u8; n];
    r.read_exact(&mut body).expect("read body");
    String::from_utf8(body).expect("utf-8 body")
}

#[test]
fn full_session_initialize_format_shutdown() {
    let mut child = Command::new(binary())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(stdout);

    // 1. initialize
    let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
    stdin.write_all(frame(init).as_bytes()).unwrap();

    let resp = read_one_message(&mut reader);
    assert!(resp.contains("documentFormattingProvider"));
    assert!(resp.contains("\"id\":1"));

    // 2. initialized notification (no response)
    let initd = r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#;
    stdin.write_all(frame(initd).as_bytes()).unwrap();

    // 3. textDocument/didOpen
    //
    // A single-field body like `VStack { spacing: 8.0 }` parses as a
    // plain `syn::Expr` (struct literal) and is deliberately left
    // untouched by `teksilo_fmt::format_file` — see its doc comment:
    // rustfmt already owns that shape, and fighting it would just
    // ping-pong the two formatters. Use a two-"field" body (a bare
    // property plus a child) so it's not a valid `syn::Expr` and the
    // DSL formatter actually reformats it — mirroring
    // `formatting_returns_full_replace_when_dirty` in `src/main.rs`.
    let dirty = "fn build() { teksu!(VStack { spacing: 8.0 Button(\"ok\") }); }\n";
    let did_open = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"file:///t.rs","languageId":"rust","version":1,"text":{}}}}}}}"#,
        serde_json::to_string(dirty).unwrap()
    );
    stdin.write_all(frame(&did_open).as_bytes()).unwrap();

    // 4. textDocument/formatting
    let fmt_req = r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/formatting","params":{"textDocument":{"uri":"file:///t.rs"},"options":{"tabSize":4,"insertSpaces":true}}}"#;
    stdin.write_all(frame(fmt_req).as_bytes()).unwrap();

    let resp = read_one_message(&mut reader);
    assert!(
        resp.contains("\"id\":2"),
        "expected formatting reply, got: {resp}"
    );
    assert!(
        resp.contains("VStack {\\n"),
        "expected a multi-line formatted body in newText, got: {resp}"
    );
    assert!(
        resp.contains("Button(\\\"ok\\\")"),
        "expected the child widget to survive reformatting, got: {resp}"
    );

    // 5. shutdown
    let shut = r#"{"jsonrpc":"2.0","id":3,"method":"shutdown"}"#;
    stdin.write_all(frame(shut).as_bytes()).unwrap();
    let resp = read_one_message(&mut reader);
    assert!(resp.contains("\"id\":3"));

    // 6. exit
    let exit_msg = r#"{"jsonrpc":"2.0","method":"exit"}"#;
    stdin.write_all(frame(exit_msg).as_bytes()).unwrap();
    drop(stdin);

    let status = child.wait().expect("wait");
    assert!(status.success(), "server exited unsuccessfully: {status:?}");

    // Drain any stderr noise for diagnostics on test failure.
    let mut stderr = String::new();
    if let Some(mut s) = child.stderr {
        let _ = s.read_to_string(&mut stderr);
    }
    assert!(stderr.is_empty(), "stderr should be empty: {stderr}");
}

#[test]
fn unknown_method_returns_error_response() {
    let mut child = Command::new(binary())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");

    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());

    let bogus = r#"{"jsonrpc":"2.0","id":42,"method":"made/up/method"}"#;
    stdin.write_all(frame(bogus).as_bytes()).unwrap();
    let resp = read_one_message(&mut reader);
    assert!(resp.contains("\"id\":42"));
    assert!(resp.contains("\"error\""));
    assert!(resp.contains("-32601")); // method-not-found

    let exit = r#"{"jsonrpc":"2.0","method":"exit"}"#;
    stdin.write_all(frame(exit).as_bytes()).unwrap();
    drop(stdin);
    let _ = child.wait();
}
