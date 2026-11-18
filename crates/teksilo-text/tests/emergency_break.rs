// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A run with no break opportunity that is wider than its line (a rule of
//! asterisks in a licence text, set in a monospaced face) is cut inside the
//! width asked for. Up to text-typeset 3.2.0 the cut came one glyph late, so
//! a monospaced label was laid out, and drawn, wider than its box.

use text_typeset::{DocumentFlow, TextFontService, TextFormat};

#[test]
fn an_unbreakable_run_is_cut_inside_the_width() {
    let mut service = TextFontService::new_without_system_fonts();
    let face = service.register_font(include_bytes!("../fonts/JetBrainsMono.ttf"));
    service.set_default_font(face, 13.0);
    let mut flow = DocumentFlow::new();
    let format = TextFormat {
        font_size: Some(13.0),
        ..TextFormat::default()
    };
    for (text, max_width) in [
        ("*".repeat(72), 520.0f32),
        ("-".repeat(80), 200.0),
        ("a".repeat(30), 100.0),
    ] {
        let result = flow.layout_paragraph(&mut service, &text, &format, max_width, None, 1.0);
        assert!(
            result.width <= max_width,
            "{:?} x{}: {} wider than its {max_width} line",
            text.chars().next().unwrap(),
            text.chars().count(),
            result.width
        );
    }
}
