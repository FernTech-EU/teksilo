//! End-to-end tests for `fern_fmt::format_block`.
//!
//! Each test passes a `fern!` body string through the formatter and
//! checks the output. Idempotence is exercised by every test:
//! `format(format(x)) == format(x)`.

use fern_fmt::{format_block, FmtConfig};

fn fmt(s: &str) -> String {
    let cfg = FmtConfig::default();
    let once = format_block(s, &cfg).expect("format_block failed");
    let twice = format_block(&once, &cfg).expect("format_block (idempotence) failed");
    assert_eq!(once, twice, "format is not idempotent");
    once
}

#[test]
fn empty_root() {
    assert_eq!(fmt("VStack"), "VStack");
}

#[test]
fn empty_body() {
    assert_eq!(fmt("VStack {}"), "VStack");
}

#[test]
fn single_property() {
    let out = fmt("VStack { spacing: 8.0 }");
    assert_eq!(out, "VStack {\n    spacing: 8.0\n}");
}

#[test]
fn multiple_properties() {
    let out = fmt("VStack { spacing: 8.0  padding: 4.0 }");
    assert_eq!(out, "VStack {\n    spacing: 8.0\n    padding: 4.0\n}");
}

#[test]
fn ctx_preamble() {
    let out = fmt("ctx => VStack { spacing: 8.0 }");
    assert_eq!(out, "ctx => VStack {\n    spacing: 8.0\n}");
}

#[test]
fn child_element_no_body() {
    let out = fmt(r#"VStack { Button("ok") }"#);
    assert_eq!(out, "VStack {\n    Button(\"ok\")\n}");
}

#[test]
fn child_element_with_body() {
    let out = fmt(r#"VStack { Button("ok") { on_activate_fn: cb } }"#);
    assert_eq!(
        out,
        "VStack {\n    Button(\"ok\") {\n        on_activate_fn: cb\n    }\n}"
    );
}

#[test]
fn binding() {
    let out = fmt(r#"VStack { btn = Button("ok") }"#);
    assert_eq!(out, "VStack {\n    btn = Button(\"ok\")\n}");
}

#[test]
fn line_comment_between_items() {
    let out = fmt("VStack {\n    spacing: 8.0\n    // a label\n    Button(\"ok\")\n}");
    let expected = "VStack {\n    spacing: 8.0\n    // a label\n    Button(\"ok\")\n}";
    assert_eq!(out, expected);
}

#[test]
fn block_comment_between_items() {
    let out = fmt("VStack {\n    spacing: 8.0\n    /* note */\n    Button(\"ok\")\n}");
    assert!(out.contains("/* note */"), "expected block comment preserved, got:\n{out}");
}

#[test]
fn blank_line_between_items() {
    let out = fmt("VStack {\n    spacing: 8.0\n\n    Button(\"ok\")\n}");
    let expected = "VStack {\n    spacing: 8.0\n\n    Button(\"ok\")\n}";
    assert_eq!(out, expected);
}

#[test]
fn nested_container() {
    let input = "VStack { HStack { Button(\"a\") Button(\"b\") } }";
    let out = fmt(input);
    let expected = "VStack {\n    HStack {\n        Button(\"a\")\n        Button(\"b\")\n    }\n}";
    assert_eq!(out, expected);
}

#[test]
fn argument_free_property() {
    let out = fmt("Expand { fills_stack }");
    assert_eq!(out, "Expand {\n    fills_stack\n}");
}

#[test]
fn explicit_constructor() {
    let out = fmt(r#"VStack { Button::new_literal("ok") }"#);
    assert_eq!(out, "VStack {\n    Button::new_literal(\"ok\")\n}");
}

#[test]
fn property_with_multiple_args() {
    let out = fmt(r#"Foo { tab_literal: "name", Card }"#);
    assert!(out.contains("tab_literal: \"name\","), "got:\n{out}");
}

#[test]
fn already_formatted_is_unchanged() {
    let canonical =
        "VStack {\n    spacing: 8.0\n    Button(\"ok\") {\n        on_activate_fn: cb\n    }\n}";
    assert_eq!(fmt(canonical), canonical);
}
