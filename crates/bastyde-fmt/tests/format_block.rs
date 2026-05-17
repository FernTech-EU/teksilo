//! End-to-end tests for `bastyde_fmt::format_block`.
//!
//! Each test passes a `bati!` body string through the formatter and
//! checks the output. Idempotence is exercised by every test:
//! `format(format(x)) == format(x)`.

use bastyde_fmt::{FmtConfig, format_block};

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
    assert!(
        out.contains("/* note */"),
        "expected block comment preserved, got:\n{out}"
    );
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

// Regression: the synthesized TokenStream for structural forms must
// reach past the form's closing `}`. Earlier versions stopped at the
// last element's last token and silently truncated the trailing `)`,
// `,`, and `}` characters during a verbatim source slice.

#[test]
fn if_no_else_preserves_closing_brace() {
    let src = r#"VStack { if cond { Banner("x") } }"#;
    let out = fmt(src);
    assert!(
        out.contains(r#"if cond { Banner("x") }"#),
        "expected if-form intact, got:\n{out}"
    );
}

#[test]
fn if_else_preserves_else_block_closing_brace() {
    let src = r#"VStack { if flag { YesBanner } else { NoBanner("hi") } }"#;
    let out = fmt(src);
    assert!(
        out.contains(r#"NoBanner("hi")"#) && out.contains("} else {"),
        "expected else block intact, got:\n{out}"
    );
    // The structural form must end with `}` before the parent's `}`.
    assert!(
        out.contains("NoBanner(\"hi\") }") || out.contains("NoBanner(\"hi\")\n            }"),
        "expected else-block close-brace preserved, got:\n{out}"
    );
}

#[test]
fn match_preserves_arm_and_block_closing_braces() {
    let src = "Holder { match s { S::A => One, S::B(x) => Two(x.clone()), } }";
    let out = fmt(src);
    assert!(
        out.contains("S::B(x) => Two(x.clone())"),
        "expected last arm intact, got:\n{out}"
    );
    // Last arm's constructor `)` and the match block's `}` must survive.
    assert!(
        out.contains("Two(x.clone()),") || out.contains("Two(x.clone())\n"),
        "expected last arm closing intact, got:\n{out}"
    );
    assert!(
        out.matches('}').count() >= 2,
        "expected match `}}` and parent `}}`, got:\n{out}"
    );
}

#[test]
fn for_loop_preserves_closing_braces() {
    let src = "VLike { for item in items.iter() { ListItem(item) { tag: 1 } } }";
    let out = fmt(src);
    assert!(
        out.contains("ListItem(item)") && out.contains("tag: 1"),
        "expected for-body intact, got:\n{out}"
    );
    // Inner element body `}`, for block `}`, parent `}` — three closing braces.
    assert!(
        out.matches('}').count() >= 3,
        "expected three closing braces, got:\n{out}"
    );
}

#[test]
fn else_if_chain_preserves_innermost_close() {
    let src = "Holder { if a { A } else if b { B } else { C(\"tail\") } }";
    let out = fmt(src);
    assert!(
        out.contains("else if b") && out.contains("C(\"tail\")"),
        "expected else-if chain intact, got:\n{out}"
    );
}

// Regression: continuation lines inside a multiline verbatim slice
// (structural form bodies, closures, `rust { }` blocks) must align to
// `self.indent`, not `self.indent + 1`. The min-indent line in the
// slice — typically the closing `}` — anchors at the form's keyword
// column. An earlier off-by-one shifted everything inside one extra
// level deeper than the surrounding source.

#[test]
fn match_canonical_indent_is_stable() {
    let canonical = "Holder {\n    match state {\n        State::A => OneArm,\n        State::B(x) => TwoArm(x.clone()),\n    }\n}";
    assert_eq!(fmt(canonical), canonical);
}

#[test]
fn if_else_canonical_indent_is_stable() {
    let canonical = "Holder {\n    if cond {\n        YesBanner\n    } else {\n        NoBanner(\"hi\")\n    }\n}";
    assert_eq!(fmt(canonical), canonical);
}

#[test]
fn for_canonical_indent_is_stable() {
    let canonical = "VLike {\n    for item in items {\n        ListItem(item)\n    }\n}";
    assert_eq!(fmt(canonical), canonical);
}

#[test]
fn closure_canonical_indent_is_stable() {
    let canonical = "Button {\n    on_activate: |ctx| {\n        ctx.send(X);\n    }\n}";
    assert_eq!(fmt(canonical), canonical);
}

// Regression: element-header args with multi-line content (a string
// literal split with `\` continuations, or a nested call broken at
// parens) used to be emitted via `verbatim_slice`, preserving source-
// absolute column positions. When the result was then re-indented by
// `format_file::reindent_block`, every continuation line gained
// `base_indent` spaces — and the next run repeated the addition,
// growing the file indefinitely. They now route through
// `write_verbatim_multiline`, which dedents to the slice's min-indent
// and reanchors at the current printer depth (round-trip stable).

#[test]
fn multiline_string_literal_arg_is_stable() {
    let canonical = "VStack {\n    TextWidget::new_literal(\"hello \\\n    world, line 2 \\\n    line 3\") {\n        style: BodyBold\n    }\n}";
    assert_eq!(fmt(canonical), canonical);
}

#[test]
fn multiline_call_expr_arg_is_stable() {
    let canonical = "HStack {\n    MinSizeForLabel::new(TextWidget::new_literal(\n        \"Fill\",\n    )) {\n        width: 220.0\n    }\n}";
    assert_eq!(fmt(canonical), canonical);
}
