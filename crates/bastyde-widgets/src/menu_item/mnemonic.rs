//! Mnemonic-marker parsing for menu labels.
//!
//! Bastyde uses the Win32 / Qt convention: a single `&` in the label
//! marks the next character as the keyboard mnemonic for that item.
//! A doubled `&&` escapes to a literal `&` with no mnemonic. The
//! menubar Alt+letter / in-menu bare-letter dispatch reads
//! [`ParsedMnemonic::key_lower`]; the underline painter in
//! [`MenuLabel`](super::menu_label::MenuLabel) reads
//! [`ParsedMnemonic::byte_index`] and [`ParsedMnemonic::char_index`].
//!
//! ### Convention
//!
//! | Input            | `stripped`     | `byte_index` | `char_index` | `key_lower` |
//! |------------------|----------------|--------------|--------------|-------------|
//! | `"&Save"`        | `"Save"`       | `Some(0)`    | `Some(0)`    | `Some('s')` |
//! | `"Sa&ve"`        | `"Save"`       | `Some(2)`    | `Some(2)`    | `Some('v')` |
//! | `"&&Save"`       | `"&Save"`      | `None`       | `None`       | `None`      |
//! | `"S&&ve"`        | `"S&ve"`       | `None`       | `None`       | `None`      |
//! | `"trailing&"`    | `"trailing&"`  | `None`       | `None`       | `None`      |
//! | `"no marker"`    | `"no marker"`  | `None`       | `None`       | `None`      |
//! | `"&É"`           | `"É"`          | `Some(0)`    | `Some(0)`    | `Some('é')` |
//!
//! Only the FIRST un-escaped `&` is treated as a marker; subsequent
//! `&`s are emitted literally. A dangling `&` at end-of-string is
//! preserved as a literal `&`.

/// Result of parsing a mnemonic-bearing label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedMnemonic {
    /// The label with the marker `&` removed and `&&` collapsed to a
    /// single literal `&`. This is what gets rendered.
    pub stripped: String,
    /// Byte offset of the marked character inside `stripped`. UTF-8
    /// safe — points at the start of a code-point boundary.
    pub byte_index: Option<usize>,
    /// Grapheme-cluster index (zero-based) of the marked character
    /// inside `stripped`. Only meaningful in concert with
    /// `byte_index`; provided so the underline painter can advance
    /// per-cluster via the text backend's per-cluster API if a
    /// future enhancement needs cluster-correct positioning. Today
    /// the `byte_index` slice + measure-prefix path is sufficient.
    pub char_index: Option<usize>,
    /// Lowercased mnemonic character, ready to compare against
    /// `Key::Character` payloads at dispatch time. ASCII-lowercased
    /// only; non-ASCII letters (`É`) round-trip through
    /// `char::to_lowercase` which handles the common Latin-1 cases.
    pub key_lower: Option<char>,
}

impl ParsedMnemonic {
    pub(crate) fn plain(s: &str) -> Self {
        Self {
            stripped: s.to_string(),
            byte_index: None,
            char_index: None,
            key_lower: None,
        }
    }

    /// Whether this label carries a mnemonic marker.
    pub(crate) fn has_mnemonic(&self) -> bool {
        self.byte_index.is_some()
    }
}

/// Parse a label for a single mnemonic marker. See module-level docs
/// for the conventions.
pub(crate) fn parse_mnemonic(s: &str) -> ParsedMnemonic {
    let mut stripped = String::with_capacity(s.len());
    let mut byte_index: Option<usize> = None;
    let mut char_index: Option<usize> = None;
    let mut key_lower: Option<char> = None;
    let mut char_pos: usize = 0;

    let mut chars = s.char_indices().peekable();
    while let Some((_, c)) = chars.next() {
        if c == '&' {
            match chars.peek().copied() {
                // Doubled `&&` — emit a literal `&`, no mnemonic.
                Some((_, '&')) => {
                    chars.next(); // consume the second '&'
                    stripped.push('&');
                    char_pos += 1;
                }
                // First un-escaped `&` before some other character —
                // mark that character as the mnemonic and skip the `&`
                // in the output.
                Some(_) if byte_index.is_none() => {
                    let (_, next_c) = chars.next().expect("peek was Some");
                    byte_index = Some(stripped.len());
                    char_index = Some(char_pos);
                    // ASCII fold first (covers `&S` → `s`); fall back
                    // to `to_lowercase` which yields the first
                    // lowered code-point for the common case.
                    key_lower = Some(
                        next_c
                            .to_lowercase()
                            .next()
                            .unwrap_or(next_c)
                            .to_ascii_lowercase(),
                    );
                    stripped.push(next_c);
                    char_pos += 1;
                }
                // Second un-escaped `&` — emit literally.
                Some(_) => {
                    stripped.push('&');
                    char_pos += 1;
                }
                // Trailing `&` at end of string — emit literally.
                None => {
                    stripped.push('&');
                    char_pos += 1;
                }
            }
        } else {
            stripped.push(c);
            char_pos += 1;
        }
    }

    ParsedMnemonic {
        stripped,
        byte_index,
        char_index,
        key_lower,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_no_marker() {
        let p = parse_mnemonic("no marker");
        assert_eq!(p.stripped, "no marker");
        assert_eq!(p.byte_index, None);
        assert_eq!(p.char_index, None);
        assert_eq!(p.key_lower, None);
    }

    #[test]
    fn parse_marker_at_start() {
        let p = parse_mnemonic("&Save");
        assert_eq!(p.stripped, "Save");
        assert_eq!(p.byte_index, Some(0));
        assert_eq!(p.char_index, Some(0));
        assert_eq!(p.key_lower, Some('s'));
    }

    #[test]
    fn parse_marker_in_middle() {
        let p = parse_mnemonic("Sa&ve");
        assert_eq!(p.stripped, "Save");
        assert_eq!(p.byte_index, Some(2));
        assert_eq!(p.char_index, Some(2));
        assert_eq!(p.key_lower, Some('v'));
    }

    #[test]
    fn parse_escaped_double_ampersand_collapses_to_literal() {
        let p = parse_mnemonic("&&Save");
        assert_eq!(p.stripped, "&Save");
        assert_eq!(p.byte_index, None);
        assert_eq!(p.char_index, None);
        assert_eq!(p.key_lower, None);
    }

    #[test]
    fn parse_escape_does_not_consume_real_marker_later() {
        // Doubled `&&` collapses to literal `&` first; the next
        // `&` (if any) is treated as the marker.
        let p = parse_mnemonic("&&S&ave");
        assert_eq!(p.stripped, "&Save");
        assert_eq!(p.byte_index, Some(2));
        assert_eq!(p.char_index, Some(2));
        assert_eq!(p.key_lower, Some('a'));
    }

    #[test]
    fn parse_second_unescaped_ampersand_kept_literal() {
        let p = parse_mnemonic("&Sa&ve");
        // First `&` marks 'S'; second `&` is literal.
        assert_eq!(p.stripped, "Sa&ve");
        assert_eq!(p.byte_index, Some(0));
        assert_eq!(p.char_index, Some(0));
        assert_eq!(p.key_lower, Some('s'));
    }

    #[test]
    fn parse_dangling_trailing_ampersand_kept_literal() {
        let p = parse_mnemonic("trailing&");
        assert_eq!(p.stripped, "trailing&");
        assert_eq!(p.byte_index, None);
        assert_eq!(p.key_lower, None);
    }

    #[test]
    fn parse_empty_string() {
        let p = parse_mnemonic("");
        assert_eq!(p.stripped, "");
        assert_eq!(p.byte_index, None);
        assert_eq!(p.key_lower, None);
    }

    #[test]
    fn parse_single_ampersand() {
        let p = parse_mnemonic("&");
        // Lone `&` at end — literal.
        assert_eq!(p.stripped, "&");
        assert_eq!(p.byte_index, None);
        assert_eq!(p.key_lower, None);
    }

    #[test]
    fn parse_multibyte_marked_character() {
        // É is 2 bytes in UTF-8.
        let p = parse_mnemonic("&É");
        assert_eq!(p.stripped, "É");
        assert_eq!(p.byte_index, Some(0));
        assert_eq!(p.char_index, Some(0));
        // `to_lowercase('É').next() == 'é'`. ASCII fold leaves 'é' as-is.
        assert_eq!(p.key_lower, Some('é'));
    }

    #[test]
    fn parse_marker_before_multibyte_position() {
        let p = parse_mnemonic("Café&!");
        // `Café` = "Caf" + 2-byte é = 5 bytes; `&` then comes before `!`.
        // After stripping `&`, `!` sits at byte offset 5.
        assert_eq!(p.stripped, "Café!");
        assert_eq!(p.byte_index, Some(5));
        assert_eq!(p.char_index, Some(4));
        assert_eq!(p.key_lower, Some('!'));
    }

    #[test]
    fn parse_byte_and_char_indices_differ_on_multibyte_prefix() {
        // Multi-byte prefix → char_index < byte_index.
        let p = parse_mnemonic("é&q");
        // é = 2 bytes, then `q` (after stripping `&`) sits at byte 2,
        // char 1.
        assert_eq!(p.stripped, "éq");
        assert_eq!(p.byte_index, Some(2));
        assert_eq!(p.char_index, Some(1));
        assert_eq!(p.key_lower, Some('q'));
    }

    #[test]
    fn parse_marker_is_case_insensitive_for_key_lower() {
        let p = parse_mnemonic("&S");
        assert_eq!(p.key_lower, Some('s'));
        let p2 = parse_mnemonic("&s");
        assert_eq!(p2.key_lower, Some('s'));
    }

    #[test]
    fn has_mnemonic_true_only_when_marker_present() {
        assert!(parse_mnemonic("&Save").has_mnemonic());
        assert!(!parse_mnemonic("Save").has_mnemonic());
        assert!(!parse_mnemonic("&&Save").has_mnemonic());
    }

    #[test]
    fn plain_constructor_has_no_mnemonic() {
        let p = ParsedMnemonic::plain("Hello");
        assert_eq!(p.stripped, "Hello");
        assert!(!p.has_mnemonic());
    }
}
