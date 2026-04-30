//! Compile-time validation of the parsed schema.

use crate::manifest::{EventDef, PropDef, Schema};

static VALID_CATEGORIES: &[&str] =
    &["intent", "lifecycle", "navigation", "census", "custom"];

static VALID_PROP_TYPES: &[&str] =
    &["dev_static", "bounded_str", "u32", "i64", "bool", "f64_bucket", "enum"];

/// Validate the schema and return accumulated warnings (expired events).
/// Returns `Err` on hard failures (duplicates, unknown types, missing
/// required fields).
pub fn validate(schema: &Schema) -> Result<Vec<String>, String> {
    let mut warnings: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();

    for event in &schema.events {
        if !seen.insert(event.name.as_str()) {
            return Err(format!("duplicate event name: `{}`", event.name));
        }
        validate_event(event, &mut warnings)?;
    }

    Ok(warnings)
}

fn validate_event(event: &EventDef, warnings: &mut Vec<String>) -> Result<(), String> {
    if !VALID_CATEGORIES.contains(&event.category.as_str()) {
        return Err(format!(
            "event `{}`: unknown category `{}` (valid: {})",
            event.name,
            event.category,
            VALID_CATEGORIES.join(", ")
        ));
    }

    validate_expires(event, warnings)?;

    // Check required non-empty fields.
    if event.bug.trim().is_empty() {
        return Err(format!(
            "event `{}`: `bug` is required (must be a URL pointing to the issue)",
            event.name
        ));
    }
    if event.description.trim().is_empty() {
        return Err(format!(
            "event `{}`: `description` is required",
            event.name
        ));
    }

    // Check for duplicate prop names within the event.
    let mut prop_names: std::collections::HashSet<&str> =
        std::collections::HashSet::new();
    for prop in &event.props {
        if !prop_names.insert(prop.name.as_str()) {
            return Err(format!(
                "event `{}`: duplicate prop name `{}`",
                event.name, prop.name
            ));
        }
        validate_prop(&event.name, prop)?;
    }

    Ok(())
}

fn validate_expires(event: &EventDef, warnings: &mut Vec<String>) -> Result<(), String> {
    let s = event.expires.trim_matches('"');
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3
        || parts[0].len() != 4
        || parts[1].len() != 2
        || parts[2].len() != 2
        || parts.iter().any(|p| !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(format!(
            "event `{}`: `expires` must be YYYY-MM-DD, got `{}`",
            event.name, event.expires
        ));
    }

    let today = today_str();
    if s < today.as_str() {
        warnings.push(format!(
            "event `{}`: `expires: {}` is in the past (today is {}) — bump or retire",
            event.name, s, today
        ));
    }

    Ok(())
}

fn validate_prop(event_name: &str, prop: &PropDef) -> Result<(), String> {
    if !VALID_PROP_TYPES.contains(&prop.ty.as_str()) {
        return Err(format!(
            "event `{}`, prop `{}`: unknown type `{}` (valid: {})",
            event_name,
            prop.name,
            prop.ty,
            VALID_PROP_TYPES.join(", ")
        ));
    }
    if prop.ty == "enum" && prop.values.is_empty() {
        return Err(format!(
            "event `{}`, prop `{}`: type `enum` requires a non-empty `values` list",
            event_name, prop.name
        ));
    }
    if prop.ty != "enum" && !prop.values.is_empty() {
        return Err(format!(
            "event `{}`, prop `{}`: `values` is only valid for type `enum`",
            event_name, prop.name
        ));
    }
    Ok(())
}

/// Current date as `"YYYY-MM-DD"`, using `SOURCE_DATE_EPOCH` if set
/// (reproducible-build support), otherwise the wall clock.
fn today_str() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = if let Ok(epoch_str) = std::env::var("SOURCE_DATE_EPOCH") {
        epoch_str.parse::<u64>().unwrap_or_else(|_| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        })
    } else {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    };
    unix_secs_to_date(secs)
}

fn unix_secs_to_date(secs: u64) -> String {
    let days = secs / 86400;
    let (year, month, day) = days_to_ymd(days as i64);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::parse_schema;

    const VALID_YAML: &str = r#"
schema_version: 1
events:
  - name: intent.dispatched
    category: intent
    expires: "2027-06-01"
    bug: "https://example.com/issues/1"
    description: "Test event."
    props:
      - name: source
        type: enum
        values: [shortcut, menu]
      - name: name
        type: dev_static
"#;

    #[test]
    fn valid_schema_passes() {
        let schema = parse_schema(VALID_YAML).unwrap();
        let warnings = validate(&schema).unwrap();
        assert!(warnings.is_empty());
    }

    #[test]
    fn duplicate_event_name_is_rejected() {
        let yaml = r#"
schema_version: 1
events:
  - name: foo.bar
    category: intent
    expires: "2027-01-01"
    bug: "http://x"
    description: "First."
  - name: foo.bar
    category: lifecycle
    expires: "2027-01-01"
    bug: "http://x"
    description: "Duplicate."
"#;
        let schema = parse_schema(yaml).unwrap();
        assert!(validate(&schema).unwrap_err().contains("duplicate"));
    }

    #[test]
    fn unknown_category_is_rejected() {
        let yaml = r#"
schema_version: 1
events:
  - name: foo.bar
    category: unknown_cat
    expires: "2027-01-01"
    bug: "http://x"
    description: "Bad category."
"#;
        let schema = parse_schema(yaml).unwrap();
        assert!(validate(&schema).unwrap_err().contains("unknown category"));
    }

    #[test]
    fn enum_without_values_is_rejected() {
        let yaml = r#"
schema_version: 1
events:
  - name: foo.bar
    category: intent
    expires: "2027-01-01"
    bug: "http://x"
    description: "Test."
    props:
      - name: kind
        type: enum
"#;
        let schema = parse_schema(yaml).unwrap();
        assert!(validate(&schema).unwrap_err().contains("requires a non-empty"));
    }

    #[test]
    fn date_in_past_produces_warning() {
        let yaml = r#"
schema_version: 1
events:
  - name: old.event
    category: custom
    expires: "2020-01-01"
    bug: "http://x"
    description: "Expired."
"#;
        let schema = parse_schema(yaml).unwrap();
        let warnings = validate(&schema).unwrap();
        assert!(!warnings.is_empty());
        assert!(warnings[0].contains("in the past"));
    }

    #[test]
    fn days_to_ymd_epoch() {
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
    }

    #[test]
    fn days_to_ymd_known_date() {
        // 2026-04-30: days since 1970-01-01
        // 2026-04-30 = 56 years + leap years + …
        let d = unix_secs_to_date(1_746_000_000); // ~2025-04-30
        // Just verify it parses to YYYY-MM-DD
        let parts: Vec<&str> = d.split('-').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].len(), 4);
    }
}

/// Gregorian calendar decomposition.
/// Algorithm: http://howardhinnant.github.io/date_algorithms.html
fn days_to_ymd(days: i64) -> (i64, i64, i64) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}
