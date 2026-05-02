//! Tests for `fern_fmt::format_file` — finding `fern!` invocations in a
//! Rust source file and reformatting them in place while leaving
//! surrounding source untouched.

use fern_fmt::{format_file, FmtConfig};

fn fmt(s: &str) -> String {
    let cfg = FmtConfig::default();
    format_file(s, &cfg).expect("format_file failed")
}

#[test]
fn file_with_no_fern_macros_is_untouched() {
    let src = "fn main() {\n    println!(\"hi\");\n}\n";
    assert_eq!(fmt(src), src);
}

#[test]
fn formats_single_fern_macro() {
    let src = "fn build() {\n    fern!(ctx => VStack { spacing: 8.0  Button(\"ok\") });\n}\n";
    let out = fmt(src);
    assert!(out.contains("VStack {\n"), "got:\n{out}");
    assert!(out.contains("    spacing: 8.0\n"), "got:\n{out}");
    assert!(out.contains("    Button(\"ok\")\n"), "got:\n{out}");
}

#[test]
fn untouched_outside_macro() {
    let src = "// keep me\nfn build() {\n    fern!(VStack {});\n}\n// keep me too\n";
    let out = fmt(src);
    assert!(out.starts_with("// keep me\nfn build()"));
    assert!(out.ends_with("// keep me too\n"));
}

#[test]
fn formats_multiple_fern_macros() {
    let src = r#"fn a() { fern!(VStack { Button("a") }); }
fn b() { fern!(VStack { Button("b") }); }
"#;
    let out = fmt(src);
    assert!(out.contains("Button(\"a\")"));
    assert!(out.contains("Button(\"b\")"));
    // Each macro got expanded across multiple lines.
    let line_count = out.lines().count();
    assert!(line_count > src.lines().count(), "expected expansion, got:\n{out}");
}

#[test]
fn nested_macro_invocations_only_format_fern() {
    let src = r#"fn main() {
    println!("hello");
    fern!(VStack {});
}
"#;
    let out = fmt(src);
    assert!(out.contains("println!(\"hello\");"));
}

#[test]
fn idempotent_at_file_level() {
    let src = "fn build() {\n    fern!(VStack { Button(\"ok\") });\n}\n";
    let once = fmt(src);
    let twice = fmt(&once);
    assert_eq!(once, twice, "format_file is not idempotent");
}
