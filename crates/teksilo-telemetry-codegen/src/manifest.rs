// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! YAML manifest types and parser.

use serde::Deserialize;

/// Top-level schema document.
#[derive(Debug, Deserialize)]
pub struct Schema {
    pub schema_version: u32,
    pub events: Vec<EventDef>,
}

/// One event declaration.
#[derive(Debug, Deserialize)]
pub struct EventDef {
    /// Dotted name: `"intent.dispatched"`, `"lifecycle.app_started"`.
    pub name: String,
    /// One of: `intent`, `lifecycle`, `navigation`, `census`, `custom`.
    pub category: String,
    /// ISO date string `"YYYY-MM-DD"` — governance expiry.
    pub expires: String,
    /// URL pointing to the issue / PR that introduced this event.
    pub bug: String,
    /// Human-readable description for the PrivacySettings "Inspect" tab.
    pub description: String,
    #[serde(default)]
    pub props: Vec<PropDef>,
}

/// One property declaration on an event.
#[derive(Debug, Deserialize)]
pub struct PropDef {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    /// Required when `type == "enum"`.
    #[serde(default)]
    pub values: Vec<String>,
}

pub fn parse_schema(yaml: &str) -> Result<Schema, String> {
    serde_yaml::from_str(yaml).map_err(|e| format!("YAML parse error: {e}"))
}
