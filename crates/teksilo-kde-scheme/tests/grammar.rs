// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The configuration grammar, one rule per test, including the malformed
//! inputs the parser must survive without failing.

use teksilo_kde_scheme::{DiagnosticKind, KConfig, ParseDiagnostic};

fn parse(text: &str) -> (KConfig, Vec<ParseDiagnostic>) {
    KConfig::parse(text)
}

fn kinds(diagnostics: &[ParseDiagnostic]) -> Vec<DiagnosticKind> {
    diagnostics.iter().map(|d| d.kind.clone()).collect()
}

#[test]
fn an_empty_file_contributes_nothing() {
    let (config, diagnostics) = parse("");
    assert_eq!(config, KConfig::default());
    assert!(diagnostics.is_empty());
}

#[test]
fn entries_before_any_header_belong_to_the_root_group() {
    let (config, _) = parse("Key=Value\n[G]\nOther=1\n");
    assert_eq!(config.get(&[], "Key"), Some("Value"));
    assert_eq!(config.get(&["G"], "Other"), Some("1"));
    assert_eq!(config.get(&["G"], "Key"), None);
}

#[test]
fn keys_and_values_are_trimmed_and_the_value_may_contain_equals_signs() {
    let (config, _) = parse("[G]\n  Key  =  a=b = c  \n");
    assert_eq!(config.get(&["G"], "Key"), Some("a=b = c"));
}

#[test]
fn hash_lines_and_blank_lines_are_comments_but_semicolons_are_not() {
    let (config, diagnostics) =
        parse("# comment\n\n   \n  # indented comment\n[G]\n;Key=1\nValue=has # inside\n");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(config.get(&["G"], ";Key"), Some("1"));
    assert_eq!(config.get(&["G"], "Value"), Some("has # inside"));
}

#[test]
fn crlf_line_endings_read_like_lf() {
    let (crlf, d1) = parse("[G]\r\nKey=Value\r\n");
    let (lf, d2) = parse("[G]\nKey=Value\n");
    assert_eq!(crlf, lf);
    assert!(d1.is_empty() && d2.is_empty());
}

#[test]
fn a_file_with_lone_cr_separators_is_a_single_line() {
    let (config, _) = parse("[G]\rKey=Value\r");
    // The whole file is one header line: `[G]` followed by ignored text.
    assert_eq!(config.get(&["G"], "Key"), None);
    assert!(config.groups().next().is_none());
}

#[test]
fn a_byte_order_mark_hides_the_first_header_and_is_reported() {
    let (config, diagnostics) = parse("\u{FEFF}[General]\nColorScheme=BreezeDark\n");
    assert_eq!(
        kinds(&diagnostics),
        vec![DiagnosticKind::ByteOrderMark, DiagnosticKind::MissingEquals]
    );
    assert_eq!(config.get(&["General"], "ColorScheme"), None);
    assert_eq!(config.get(&[], "ColorScheme"), Some("BreezeDark"));
}

#[test]
fn nested_groups_are_separate_from_their_parent() {
    let (config, _) = parse("[Colors:Header]\nA=1\n[Colors:Header][Inactive]\nB=2\n");
    assert_eq!(config.get(&["Colors:Header"], "A"), Some("1"));
    assert_eq!(config.get(&["Colors:Header", "Inactive"], "B"), Some("2"));
    assert_eq!(config.get(&["Colors:Header", "Inactive"], "A"), None);
    assert_eq!(config.get(&["Colors:Header"], "B"), None);
}

#[test]
fn text_after_the_last_closing_bracket_of_a_header_is_ignored() {
    let (config, _) = parse("[G] trailing\nKey=1\n[A][B]x[C]\nOther=2\n");
    assert_eq!(config.get(&["G"], "Key"), Some("1"));
    assert_eq!(config.get(&["A", "B"], "Other"), Some("2"));
}

#[test]
fn an_unclosed_header_is_skipped_and_the_current_group_is_kept() {
    let (config, diagnostics) = parse("[G]\nA=1\n[Broken\nB=2\n");
    assert_eq!(
        kinds(&diagnostics),
        vec![DiagnosticKind::UnclosedGroupHeader]
    );
    assert_eq!(diagnostics[0].line, 3);
    assert_eq!(config.get(&["G"], "B"), Some("2"));
}

#[test]
fn a_line_without_equals_is_an_error_and_skipped() {
    let (config, diagnostics) = parse("[G]\nJustText\nKey=1\n");
    assert_eq!(kinds(&diagnostics), vec![DiagnosticKind::MissingEquals]);
    assert_eq!(config.keys(&["G"]), vec!["Key"]);
}

#[test]
fn a_line_with_an_empty_key_is_ignored_without_an_error() {
    let (config, diagnostics) = parse("[G]\n=value\n  = other\n");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(config.keys(&["G"]).is_empty());
}

#[test]
fn the_last_occurrence_of_a_key_in_a_file_wins() {
    let (config, _) = parse("[G]\nKey=first\nKey=second\n[Other]\n[G]\nKey=third\n");
    assert_eq!(config.get(&["G"], "Key"), Some("third"));
}

#[test]
fn an_immutable_occurrence_freezes_the_key_for_the_rest_of_the_file() {
    let (config, _) = parse("[G]\nKey=first\nKey[$i]=frozen\nKey=ignored\n");
    let entry = config.entry(&["G"], "Key").unwrap();
    assert_eq!(entry.value(), Some("frozen"));
    assert!(entry.is_immutable());
}

#[test]
fn inside_an_immutable_group_the_first_occurrence_wins() {
    let (config, _) = parse("[G][$i]\nKey=first\nKey=second\n");
    assert_eq!(config.get(&["G"], "Key"), Some("first"));
    assert!(config.is_group_immutable(&["G"]));
    assert!(config.entry(&["G"], "Key").unwrap().is_immutable());
}

#[test]
fn group_immutability_applies_to_that_group_only() {
    let (config, _) = parse("[G][$i]\nA=1\n[G][Sub]\nB=2\n");
    assert!(config.is_group_immutable(&["G"]));
    assert!(!config.is_group_immutable(&["G", "Sub"]));
    assert!(!config.entry(&["G", "Sub"], "B").unwrap().is_immutable());
}

#[test]
fn a_dollar_i_segment_that_is_not_last_is_part_of_the_name() {
    let (config, _) = parse("[G][$i][H]\nA=1\n[K][$i]x\nB=2\n");
    assert_eq!(config.get(&["G", "$i", "H"], "A"), Some("1"));
    assert_eq!(config.get(&["K", "$i"], "B"), Some("2"));
    assert!(!config.is_group_immutable(&["K"]));
}

#[test]
fn a_bare_dollar_i_line_makes_the_rest_of_the_file_immutable() {
    let (config, _) = parse("[Before]\nA=1\n[$i]\nB=2\n[After]\nC=3\n");
    assert!(config.is_file_immutable());
    assert!(!config.entry(&["Before"], "A").unwrap().is_immutable());
    // Entries right after the bare line go to the empty-named group.
    assert_eq!(config.get(&[""], "B"), Some("2"));
    assert!(config.entry(&[""], "B").unwrap().is_immutable());
    assert!(config.entry(&["After"], "C").unwrap().is_immutable());
}

#[test]
fn key_options_are_read_from_trailing_brackets_in_either_order_with_a_locale() {
    let (config, diagnostics) = parse("[G]\nA[$ie]=1\nB[$i][de]=2\nC[de][$i]=3\nD[$xz]=4\n");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let a = config.entry(&["G"], "A").unwrap();
    assert!(a.is_immutable() && a.expands());
    // Locale-tagged entries are ignored, whatever their options.
    assert_eq!(config.get(&["G"], "B"), None);
    assert_eq!(config.get(&["G"], "C"), None);
    // Unknown option letters are ignored.
    let d = config.entry(&["G"], "D").unwrap();
    assert!(!d.is_immutable() && !d.expands());
}

#[test]
fn locale_tagged_entries_are_ignored() {
    let (config, _) = parse("[General]\nName=Breeze Light\nName[fr]=Breeze clair\n");
    assert_eq!(config.get(&["General"], "Name"), Some("Breeze Light"));
    assert_eq!(config.keys(&["General"]), vec!["Name"]);
}

#[test]
fn a_second_locale_tag_is_an_error() {
    let (config, diagnostics) = parse("[G]\nName[fr][de]=x\nOk=1\n");
    assert_eq!(kinds(&diagnostics), vec![DiagnosticKind::SecondLocale]);
    assert_eq!(config.keys(&["G"]), vec!["Ok"]);
}

#[test]
fn an_unclosed_key_bracket_is_an_error() {
    let (config, diagnostics) = parse("[G]\nKey[de=x\nKe[y]x=literal\n");
    assert_eq!(
        kinds(&diagnostics),
        vec![DiagnosticKind::UnclosedKeyBracket]
    );
    assert_eq!(config.get(&["G"], "Ke[y]x"), Some("literal"));
}

#[test]
fn a_deleted_entry_needs_no_equals_and_reads_as_absent() {
    let (config, diagnostics) = parse("[G]\nKey[$d]\nOther[$d]=ignored\n");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(config.entry(&["G"], "Key").unwrap().is_deleted());
    assert_eq!(config.get(&["G"], "Key"), None);
    assert_eq!(config.get(&["G"], "Other"), None);
    assert!(!config.has_key(&["G"], "Other"));
    assert!(!config.group_exists(&["G"]));
}

#[test]
fn escapes_are_decoded_in_values_keys_and_group_names() {
    let (config, diagnostics) =
        parse("[Sp\\sace]\nTab\\tKey=\\sa\\tb\\nc\\rd\\\\e\\x41\\x6a\\s\nList=a\\,b\\;c\n");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(
        config.get(&["Sp ace"], "Tab\tKey"),
        Some(" a\tb\nc\rd\\eAj ")
    );
    assert_eq!(config.get(&["Sp ace"], "List"), Some("a\\,b\\;c"));
}

#[test]
fn a_trailing_backslash_is_kept() {
    let (config, _) = parse("[G]\nKey=path\\\n");
    assert_eq!(config.get(&["G"], "Key"), Some("path\\"));
}

#[test]
fn hex_escapes_with_too_few_or_non_hex_digits_yield_x() {
    let (config, diagnostics) = parse("[G]\nShort=ab\\x4\nBad=a\\xzzb\nEnd=a\\x\n");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(config.get(&["G"], "Short"), Some("abx"));
    assert_eq!(config.get(&["G"], "Bad"), Some("axb"));
    assert_eq!(config.get(&["G"], "End"), Some("ax"));
}

#[test]
fn an_unknown_escape_is_kept_verbatim_and_reported() {
    let (config, diagnostics) = parse("[G]\nKey=a\\qb\\sc\n");
    assert_eq!(
        kinds(&diagnostics),
        vec![DiagnosticKind::InvalidEscape('q')]
    );
    assert_eq!(config.get(&["G"], "Key"), Some("a\\qb c"));
}

#[test]
fn invalid_utf8_is_replaced_and_hex_escapes_can_produce_it_too() {
    let (config, diagnostics) = KConfig::parse_bytes(b"[G]\nKey=a\xffb\nEsc=\\xc3\\xa9\\xff\n");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(config.get(&["G"], "Key"), Some("a\u{FFFD}b"));
    assert_eq!(config.get(&["G"], "Esc"), Some("\u{e9}\u{FFFD}"));
}

#[test]
fn a_line_longer_than_one_and_a_half_mebibytes_is_skipped() {
    let long = format!("[G]\nLong={}\nShort=1\n", "x".repeat(1536 * 1024));
    let (config, diagnostics) = parse(&long);
    assert_eq!(kinds(&diagnostics), vec![DiagnosticKind::LineTooLong]);
    assert_eq!(config.get(&["G"], "Long"), None);
    assert_eq!(config.get(&["G"], "Short"), Some("1"));
}

#[test]
fn reading_stops_after_the_hundredth_counted_error() {
    let mut text = String::from("[G]\nBefore=1\n");
    for _ in 0..100 {
        text.push_str("broken\n");
    }
    text.push_str("After=2\n");
    let (config, diagnostics) = parse(&text);
    assert_eq!(config.get(&["G"], "Before"), Some("1"));
    assert_eq!(config.get(&["G"], "After"), None);
    assert_eq!(diagnostics.len(), 101);
    assert_eq!(
        diagnostics.last().unwrap().kind,
        DiagnosticKind::TooManyErrors
    );
}

#[test]
fn ninety_nine_errors_do_not_stop_the_file() {
    let mut text = String::from("[G]\n");
    for _ in 0..99 {
        text.push_str("broken\n");
    }
    text.push_str("After=2\n");
    let (config, diagnostics) = parse(&text);
    assert_eq!(config.get(&["G"], "After"), Some("2"));
    assert_eq!(diagnostics.len(), 99);
}

#[test]
fn a_group_exists_only_with_a_live_entry_in_it_or_below_it() {
    let (config, _) = parse("[Empty]\n[Gone]\nK[$d]\n[Parent][Child]\nK=1\n");
    assert!(!config.group_exists(&["Empty"]));
    assert!(!config.group_exists(&["Gone"]));
    assert!(config.group_exists(&["Parent"]));
    assert!(config.group_exists(&["Parent", "Child"]));
    assert!(!config.group_exists(&["Parent", "Other"]));
}

#[test]
fn typed_reads_follow_kde_rules() {
    let (config, _) = parse(
        "[G]\nT=yes\nF=Off\nZero=0\nEmpty=\nInt= 12\nBadInt=1.5\nReal=0.15\nBadReal=abc\nInf=inf\n",
    );
    let g = &["G"];
    assert_eq!(config.read_bool(g, "T"), Some(true));
    assert_eq!(config.read_bool(g, "F"), Some(false));
    assert_eq!(config.read_bool(g, "Zero"), Some(false));
    // An empty value is present, and every value but the four false words
    // reads as true.
    assert_eq!(config.read_bool(g, "Empty"), Some(true));
    assert_eq!(config.read_bool(g, "Missing"), None);
    // Values are trimmed when the entry is read, so " 12" is "12".
    assert_eq!(config.read_int(g, "Int", 7), 12);
    assert_eq!(config.read_int(g, "BadInt", 7), 7);
    assert_eq!(config.read_int(g, "Empty", 7), 7);
    assert_eq!(config.read_real(g, "Real", 0.2), 0.15);
    assert_eq!(config.read_real(g, "BadReal", 0.2), 0.2);
    assert_eq!(config.read_real(g, "Inf", 0.2), 0.2);
}

#[test]
fn diagnostics_carry_one_based_line_numbers_and_display_them() {
    let (_, diagnostics) = parse("[G]\nok=1\n\nbroken\n");
    assert_eq!(diagnostics[0].line, 4);
    assert_eq!(diagnostics[0].to_string(), "line 4: the line has no `=`");
}
