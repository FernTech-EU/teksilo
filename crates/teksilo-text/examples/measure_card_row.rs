// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use teksilo_canvas::TextBackend;
use teksilo_text::TypesetterBridge;
use teksilo_tokens::{FontWeight, TextStyle};

fn main() {
    let mut bridge = TypesetterBridge::new_with_default_font();

    let small = TextStyle {
        family: "Inter".to_string(),
        size: 12.0,
        weight: FontWeight::REGULAR,
        line_height: 1.35,
        letter_spacing: 0.0,
    };
    let small_bold = TextStyle {
        family: "Inter".to_string(),
        size: 12.0,
        weight: FontWeight::SEMI_BOLD,
        line_height: 1.35,
        letter_spacing: 0.0,
    };

    let words = ["Plain", "Elevated", "Outlined", "Filled"];
    let mut card_widths = vec![];
    for w in words {
        let layout = bridge.layout_single_line(w, &small, None);
        let card_w = layout.width + 32.0; // CARD_PADDING=16 * 2
        println!(
            "{w:>10}: text_width={:>7.2}  card_width={:>7.2}",
            layout.width, card_w
        );
        card_widths.push(card_w);
    }
    let sum: f32 = card_widths.iter().sum();
    let spacing = 12.0 * 3.0;
    let row_width = sum + spacing;
    println!("row_width (4 cards + 3x12 spacing) = {row_width:.2}");

    // Section title text (SmallBold) for card_variants section:
    let title = "Tier 1 — CardVariant";
    let tl = bridge.layout_single_line(title, &small_bold, None);
    println!("title '{title}' width = {:.2}", tl.width);

    // Compare against 400dp window - 40dp padding = 360dp content width
    println!("content width available at 400dp window = 360.00");
    println!("content width available at 600dp window = 560.00");
}
