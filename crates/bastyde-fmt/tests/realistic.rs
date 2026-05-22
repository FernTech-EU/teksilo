//! Smoke tests against bati! blocks lifted from the workspace examples.

use bastyde_fmt::{FmtConfig, format_block};

fn fmt(s: &str) -> String {
    let cfg = FmtConfig::default();
    format_block(s, &cfg).unwrap_or_else(|e| panic!("format failed: {e}\nsource:\n{s}"))
}

#[test]
fn widget_catalog_right_content_col() {
    let src = r#"ctx =>
        VStack {
            spacing: 24.0
            add_child: r_palette
            Divider { }
            add_child: r_primitives
            Divider { }
            add_child: r_controls
        }"#;
    let out = fmt(src);
    let twice = fmt(&out);
    assert_eq!(out, twice, "not idempotent. once:\n{out}\ntwice:\n{twice}");
    assert!(out.contains("ctx =>"));
    assert!(out.contains("spacing: 24.0"));
    assert!(out.contains("add_child: r_palette"));
}

#[test]
fn widget_catalog_with_closure_in_property() {
    let src = r#"ctx =>
        Button::new("Toggle") {
            variant: ButtonVariant::Plain
            on_activate_fn: |ctx| {
                ctx.send_intent(CatalogIntent::ToggleDarkMode);
            }
        }"#;
    let out = fmt(src);
    let twice = fmt(&out);
    assert_eq!(out, twice, "not idempotent. once:\n{out}\ntwice:\n{twice}");
    // Closure should be preserved through reformatting.
    assert!(out.contains("on_activate_fn:"), "got:\n{out}");
    assert!(out.contains("CatalogIntent::ToggleDarkMode"), "got:\n{out}");
}

#[test]
fn nested_three_levels() {
    let src = r#"VStack { HStack { Button("ok") { on_activate_fn: cb } } }"#;
    let out = fmt(src);
    let twice = fmt(&out);
    assert_eq!(out, twice);
    let expected = "VStack {\n    HStack {\n        Button(\"ok\") {\n            on_activate_fn: cb\n        }\n    }\n}";
    assert_eq!(out, expected);
}
