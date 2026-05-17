//! Minimal LSP types used by this server.
//!
//! Only the request/response shapes we actually emit. Incoming params
//! are read out of the raw `serde_json::Value` directly (LSP sends a
//! lot of optional fields we don't care about), so deserializing into
//! a typed struct would just add maintenance noise. We deserialize on
//! the way *out* — that's where strictness matters.

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Serialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextEdit {
    pub range: Range,
    pub new_text: String,
}
