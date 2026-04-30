//! YAML manifest types — duplicated from fern-telemetry-codegen since
//! proc-macro crates cannot be used as libraries by downstream crates.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Schema {
    #[allow(dead_code)]
    pub schema_version: u32,
    pub events: Vec<EventDef>,
}

#[derive(Debug, Deserialize)]
pub struct EventDef {
    pub name: String,
    pub category: String,
    pub expires: String,
    pub bug: String,
    pub description: String,
    #[serde(default)]
    pub props: Vec<PropDef>,
}

#[derive(Debug, Deserialize)]
pub struct PropDef {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default)]
    pub values: Vec<String>,
}

pub fn parse_schema(yaml: &str) -> Result<Schema, String> {
    serde_yaml::from_str(yaml).map_err(|e| format!("YAML parse error: {e}"))
}
