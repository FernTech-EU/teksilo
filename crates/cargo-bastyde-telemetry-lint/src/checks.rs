// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Individual lint checks applied to the manifest and source tree.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::manifest::{EventDef, Schema};

/// Severity of an issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// One lint finding.
#[derive(Debug)]
pub struct Issue {
    pub severity: Severity,
    /// Short location label: event name, file path, etc.
    pub location: String,
    pub message: String,
}

impl Issue {
    fn error(location: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            location: location.into(),
            message: message.into(),
        }
    }
    fn warning(location: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            location: location.into(),
            message: message.into(),
        }
    }
}

/// Run all checks. Returns list of issues. Caller decides exit code.
pub fn run_checks(schema: &Schema, src_roots: &[&Path], _fail_on_warnings: bool) -> Vec<Issue> {
    let mut issues: Vec<Issue> = Vec::new();

    check_schema_structure(schema, &mut issues);
    let today = today_str();
    check_expiry(schema, &today, &mut issues);

    // Build emit-fn name list for source-file scanning.
    let emit_names: Vec<String> = schema
        .events
        .iter()
        .map(|e| emit_fn_name(&e.name))
        .collect();

    // Only scan source files when at least one source root is provided.
    if !src_roots.is_empty() {
        let source_hits = scan_source_files(src_roots, &emit_names, schema);
        check_unused_events(schema, &source_hits, &mut issues);
        check_undeclared_emits(schema, src_roots, &mut issues);
    }

    issues
}

// ----- structural checks ------------------------------------------------

fn check_schema_structure(schema: &Schema, issues: &mut Vec<Issue>) {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for event in &schema.events {
        // Duplicate event names.
        if let Some(prev) = seen.insert(event.name.as_str(), 1) {
            let _ = prev;
            issues.push(Issue::error(&event.name, "duplicate event name"));
        }
        check_event_fields(event, issues);
    }
}

fn check_event_fields(event: &EventDef, issues: &mut Vec<Issue>) {
    // Required fields.
    if event.bug.trim().is_empty() {
        issues.push(Issue::error(
            &event.name,
            "`bug` is required (must be a URL pointing to the tracking issue)",
        ));
    }
    if event.description.trim().is_empty() {
        issues.push(Issue::error(&event.name, "`description` is required"));
    }

    // Valid category.
    const VALID: &[&str] = &["intent", "lifecycle", "navigation", "census", "custom"];
    if !VALID.contains(&event.category.as_str()) {
        issues.push(Issue::error(
            &event.name,
            format!(
                "unknown category `{}` (valid: {})",
                event.category,
                VALID.join(", ")
            ),
        ));
    }

    // Prop checks.
    let mut prop_seen: HashMap<&str, usize> = HashMap::new();
    for prop in &event.props {
        if prop_seen.insert(prop.name.as_str(), 1).is_some() {
            issues.push(Issue::error(
                &event.name,
                format!("duplicate prop name `{}`", prop.name),
            ));
        }
        const VALID_TYPES: &[&str] = &[
            "dev_static",
            "bounded_str",
            "u32",
            "i64",
            "bool",
            "f64_bucket",
            "enum",
        ];
        if !VALID_TYPES.contains(&prop.ty.as_str()) {
            issues.push(Issue::error(
                &event.name,
                format!(
                    "prop `{}`: unknown type `{}` (valid: {})",
                    prop.name,
                    prop.ty,
                    VALID_TYPES.join(", ")
                ),
            ));
        }
        if prop.ty == "enum" && prop.values.is_empty() {
            issues.push(Issue::error(
                &event.name,
                format!(
                    "prop `{}`: type `enum` requires a non-empty `values` list",
                    prop.name
                ),
            ));
        }
    }
}

// ----- expiry check -----------------------------------------------------

fn check_expiry(schema: &Schema, today: &str, issues: &mut Vec<Issue>) {
    for event in &schema.events {
        let s = event.expires.trim_matches('"');
        // Validate format.
        let parts: Vec<&str> = s.split('-').collect();
        let valid_fmt = parts.len() == 3
            && parts[0].len() == 4
            && parts[1].len() == 2
            && parts[2].len() == 2
            && parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_digit()));
        if !valid_fmt {
            issues.push(Issue::error(
                &event.name,
                format!("`expires` must be YYYY-MM-DD, got `{}`", event.expires),
            ));
            continue;
        }
        if s < today {
            issues.push(Issue::warning(
                &event.name,
                format!(
                    "`expires: {s}` is in the past (today is {today}) — bump or retire this event"
                ),
            ));
        }
    }
}

// ----- source-file scanning ---------------------------------------------

/// Maps emit_fn_name → list of (file, line) hit sites.
type HitMap = BTreeMap<String, Vec<(String, usize)>>;

fn scan_source_files(src_roots: &[&Path], emit_names: &[String], _schema: &Schema) -> HitMap {
    let mut hits: HitMap = BTreeMap::new();
    for name in emit_names {
        hits.insert(name.clone(), Vec::new());
    }

    // Also track patterns that look like emit_* but aren't in the schema.
    for root in src_roots {
        walk_dir(root, &mut |file_path, content| {
            for (line_no, line) in content.lines().enumerate() {
                for name in emit_names {
                    if line.contains(name.as_str()) {
                        hits.entry(name.clone())
                            .or_default()
                            .push((file_path.to_string_lossy().to_string(), line_no + 1));
                    }
                }
            }
        });
    }
    hits
}

fn walk_dir(root: &Path, visitor: &mut impl FnMut(&Path, &str)) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_dir(&path, visitor);
        } else if path.extension().map(|e| e == "rs").unwrap_or(false)
            && let Ok(content) = std::fs::read_to_string(&path)
        {
            visitor(&path, &content);
        }
    }
}

// ----- unused / undeclared event checks ---------------------------------

fn check_unused_events(schema: &Schema, hits: &HitMap, issues: &mut Vec<Issue>) {
    for event in &schema.events {
        let fn_name = emit_fn_name(&event.name);
        if hits.get(&fn_name).map(|v| v.is_empty()).unwrap_or(true) {
            issues.push(Issue::warning(
                &event.name,
                format!(
                    "declared but never emitted — `{fn_name}` has no call sites in the scanned source tree"
                ),
            ));
        }
    }
}

fn check_undeclared_emits(schema: &Schema, src_roots: &[&Path], issues: &mut Vec<Issue>) {
    // Every `emit_*` name the manifest legitimately produces.
    let declared: std::collections::HashSet<String> = schema
        .events
        .iter()
        .map(|e| emit_fn_name(&e.name))
        .collect();

    // Scan the source tree for `emit_<ident>(...)` call sites whose name has
    // no matching declared event — a typo'd or removed event name. Best-effort
    // text scan (same fidelity as `scan_source_files`): it cross-checks the
    // identifier text, not a parsed AST.
    for root in src_roots {
        walk_dir(root, &mut |file_path, content| {
            for (line_no, line) in content.lines().enumerate() {
                for ident in find_emit_call_idents(line) {
                    if !declared.contains(&ident) {
                        issues.push(Issue::warning(
                            &ident,
                            format!(
                                "`{ident}` is emitted at {}:{} but no matching event is declared \
                                 in the manifest (typo, or the event was removed?)",
                                file_path.to_string_lossy(),
                                line_no + 1
                            ),
                        ));
                    }
                }
            }
        });
    }
}

/// Extract `emit_<ident>` names that appear as a *call* (`emit_foo(`) on a
/// line, skipping function definitions (`fn emit_foo`). Operates on bytes so
/// arbitrary UTF-8 in the surrounding text never splits a char boundary;
/// `emit_` and Rust identifiers are ASCII, so the captured slice is valid.
fn find_emit_call_idents(line: &str) -> Vec<String> {
    const NEEDLE: &[u8] = b"emit_";
    // Scan only the code portion: `//` comments and double-quoted string
    // contents are stripped first, so prose like `// emit_foo()`, doc
    // comments, and literals like `"emit_bar(arg)"` don't masquerade as
    // call sites. (Char literals / lifetimes are left alone — a char
    // literal can't contain `emit_foo(`, and tracking `'` would mis-handle
    // `&'a`.)
    let code = code_portion(line);
    let line = code.as_str();
    let b = line.as_bytes();
    let n = b.len();
    let is_ident = |c: u8| c == b'_' || c.is_ascii_alphanumeric();

    let mut out = Vec::new();
    let mut i = 0;
    while i + NEEDLE.len() <= n {
        if &b[i..i + NEEDLE.len()] == NEEDLE {
            // Must start an identifier (not the tail of `some_emit_x`).
            let starts_ident = i == 0 || !is_ident(b[i - 1]);
            if starts_ident {
                let mut j = i;
                while j < n && is_ident(b[j]) {
                    j += 1;
                }
                // Next non-space byte must be `(` for this to be a call.
                let mut k = j;
                while k < n && (b[k] == b' ' || b[k] == b'\t') {
                    k += 1;
                }
                let is_call = k < n && b[k] == b'(';
                // Exclude the definition site `fn emit_foo(...)`.
                let before = line[..i].trim_end();
                let is_def = before == "fn" || before.ends_with(" fn") || before.ends_with("\tfn");
                if is_call && !is_def {
                    out.push(line[i..j].to_string());
                }
                i = j;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Return the code portion of a single line: any `//` line comment is dropped,
/// double-quoted string *contents* are blanked to spaces, and a single-line
/// `/* ... */` block comment is blanked. This is a best-effort lexer (block
/// comments are not tracked across lines, raw-string `#` fences are ignored),
/// enough to keep `emit_*` text inside comments and string literals from being
/// flagged as undeclared call sites. Only ASCII bytes are substituted and all
/// other bytes are copied verbatim, so UTF-8 boundaries are preserved.
fn code_portion(line: &str) -> String {
    let b = line.as_bytes();
    let n = b.len();
    let mut out: Vec<u8> = Vec::with_capacity(n);
    let mut in_str = false;
    let mut i = 0;
    while i < n {
        let c = b[i];
        if in_str {
            if c == b'\\' {
                // Skip the escape and the escaped byte (blanked).
                out.push(b' ');
                if i + 1 < n {
                    out.push(b' ');
                    i += 2;
                } else {
                    i += 1;
                }
                continue;
            }
            if c == b'"' {
                in_str = false;
                out.push(c);
            } else {
                out.push(b' ');
            }
            i += 1;
            continue;
        }
        // In code.
        if c == b'/' && i + 1 < n && b[i + 1] == b'/' {
            break; // line comment — rest of the line is not code.
        }
        if c == b'/' && i + 1 < n && b[i + 1] == b'*' {
            out.push(b' ');
            out.push(b' ');
            i += 2;
            while i < n {
                if b[i] == b'*' && i + 1 < n && b[i + 1] == b'/' {
                    out.push(b' ');
                    out.push(b' ');
                    i += 2;
                    break;
                }
                out.push(b' ');
                i += 1;
            }
            continue;
        }
        if c == b'"' {
            in_str = true;
        }
        out.push(c);
        i += 1;
    }
    // SAFETY-equivalent: substitutions are ASCII spaces, other bytes verbatim.
    String::from_utf8(out).unwrap_or_else(|_| line.to_string())
}

// ----- date helpers (no external dep) -----------------------------------

pub fn today_str() -> String {
    let secs = if let Ok(s) = std::env::var("SOURCE_DATE_EPOCH") {
        s.parse::<u64>().unwrap_or_else(|_| now_secs())
    } else {
        now_secs()
    };
    unix_secs_to_date(secs)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn unix_secs_to_date(secs: u64) -> String {
    let days = secs / 86400;
    let (y, m, d) = days_to_ymd(days as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

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

// ----- name helpers -----------------------------------------------------

/// `"intent.dispatched"` → `"emit_intent_dispatched"`
pub fn emit_fn_name(event_name: &str) -> String {
    let s: String = event_name
        .chars()
        .map(|c| if c == '.' || c == '-' { '_' } else { c })
        .collect();
    format!("emit_{s}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::parse_schema;

    fn minimal_yaml(expires: &str) -> String {
        format!(
            r#"
schema_version: 1
events:
  - name: test.event
    category: intent
    expires: "{expires}"
    bug: "http://x"
    description: "Test."
"#
        )
    }

    #[test]
    fn future_event_no_issues() {
        let schema = parse_schema(&minimal_yaml("2099-01-01")).unwrap();
        let issues = run_checks(&schema, &[], false);
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn past_event_produces_warning() {
        let schema = parse_schema(&minimal_yaml("2020-01-01")).unwrap();
        let issues = run_checks(&schema, &[], false);
        assert!(issues.iter().any(|i| i.severity == Severity::Warning));
    }

    #[test]
    fn emit_fn_name_converts_dots() {
        assert_eq!(emit_fn_name("intent.dispatched"), "emit_intent_dispatched");
    }

    #[test]
    fn find_emit_call_idents_matches_calls_only() {
        // Calls are captured…
        assert_eq!(find_emit_call_idents("emit_foo();"), vec!["emit_foo"]);
        assert_eq!(
            find_emit_call_idents("    telemetry::emit_bar_baz (reporter);"),
            vec!["emit_bar_baz"]
        );
        // …definitions are not…
        assert!(find_emit_call_idents("pub fn emit_foo(reporter: &R) {}").is_empty());
        assert!(find_emit_call_idents("fn emit_foo() {}").is_empty());
        // …a name with no following `(` is not a call…
        assert!(find_emit_call_idents("let f = emit_foo;").is_empty());
        // …and `emit_` as the tail of another identifier is not matched.
        assert!(find_emit_call_idents("self.re_emit_foo();").is_empty());
        // …line comments are not code…
        assert!(find_emit_call_idents("// emit_foo() was renamed").is_empty());
        assert!(find_emit_call_idents("/// See [`emit_baz()`] for details").is_empty());
        assert!(find_emit_call_idents("    let x = 1; // emit_trailing()").is_empty());
        // …string literals are not code…
        assert!(find_emit_call_idents(r#"let s = "emit_bar(arg)";"#).is_empty());
        // …single-line block comments are not code…
        assert!(find_emit_call_idents("/* emit_block() */").is_empty());
        // …but a real call alongside a decoy comment is still captured.
        assert_eq!(
            find_emit_call_idents("emit_real(); // emit_fake()"),
            vec!["emit_real"]
        );
    }

    #[test]
    fn undeclared_emit_call_is_flagged() {
        use std::io::Write;

        let schema = parse_schema(&minimal_yaml("2099-01-01")).unwrap();
        // `test.event` → `emit_test_event`. The decoy definition must be
        // ignored; the typo'd call must be flagged.
        let dir = std::env::temp_dir().join(format!(
            "bastyde_lint_undeclared_{}_{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("usage.rs");
        {
            let mut f = std::fs::File::create(&file).unwrap();
            writeln!(f, "fn emit_decoy() {{}}").unwrap();
            writeln!(f, "fn use_them() {{").unwrap();
            writeln!(f, "    emit_test_event(reporter, None, sid);").unwrap();
            writeln!(f, "    emit_tset_event(reporter, None, sid);").unwrap();
            writeln!(f, "}}").unwrap();
        }

        let issues = run_checks(&schema, &[dir.as_path()], false);
        std::fs::remove_dir_all(&dir).ok();

        assert!(
            issues.iter().any(|i| i.severity == Severity::Warning
                && i.message.contains("emit_tset_event")
                && i.message.contains("no matching event is declared")),
            "the typo'd emit call must be flagged: {issues:?}"
        );
        assert!(
            !issues.iter().any(|i| i.message.contains("emit_decoy")),
            "the definition site must not be flagged as an undeclared call: {issues:?}"
        );
        assert!(
            !issues.iter().any(|i| i.message.contains("emit_test_event")
                && i.message.contains("no matching event")),
            "a declared event's call must not be flagged: {issues:?}"
        );
    }

    #[test]
    fn unknown_category_is_error() {
        let yaml = r#"
schema_version: 1
events:
  - name: foo.bar
    category: BAD
    expires: "2099-01-01"
    bug: "http://x"
    description: "Bad."
"#;
        let schema = parse_schema(yaml).unwrap();
        let issues = run_checks(&schema, &[], false);
        assert!(
            issues
                .iter()
                .any(|i| i.severity == Severity::Error && i.message.contains("unknown category"))
        );
    }
}
