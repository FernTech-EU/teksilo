//! Input-mask grammar (Qt-compatible subset).
//!
//! Adopts the well-known `QLineEdit::setInputMask` grammar so masks
//! that work in any Qt-based desktop app port over verbatim. Used by
//! [`TextInputField`](super::super::text_input_field::TextInputField)
//! to constrain typed input, render the empty-state template
//! (`__/__/____` for `99/99/9999`), and auto-insert literal separators
//! between editable positions.
//!
//! # Grammar
//!
//! | Char | Meaning |
//! | --- | --- |
//! | `9` | Required digit |
//! | `0` | Optional digit |
//! | `A` | Required ASCII letter |
//! | `a` | Optional ASCII letter |
//! | `N` | Required alphanumeric |
//! | `n` | Optional alphanumeric |
//! | `X` | Any required character |
//! | `x` | Any optional character |
//! | `H` | Required hex digit |
//! | `h` | Optional hex digit |
//! | `>` | Uppercase the following editable chars (toggle) |
//! | `<` | Lowercase the following editable chars (toggle) |
//! | `!` | Cancel the case lock from `>` / `<` |
//! | `\X` | Literal `X` (escape) |
//!
//! Anything else is a fixed separator: `99/99/9999`, `(999) 999-9999`,
//! `>AA` for force-uppercase 2-letter codes.
//!
//! # Storage model
//!
//! The bound `Signal<String>` observes the **formatted** text including
//! literal separators (matches Qt's QLineEdit semantics): typing `12302026`
//! into `99/99/9999` produces `"12/30/2026"`, not `"12302026"`. Apps that
//! need raw digits strip separators trivially with `.replace('/', "")`.

use std::fmt::Write as _;

/// Per-position case lock applied during typing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseLock {
    None,
    Upper,
    Lower,
}

/// One position in the parsed mask. Either the user provides a
/// character there ([`Editable`](MaskPosition::Editable)) or it's a
/// fixed literal that the field paints automatically and the caret
/// skips over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaskPosition {
    /// Editable slot. `class` constrains accepted chars; `required`
    /// distinguishes mandatory vs optional positions; `case` applies
    /// to letter / alphanumeric / any classes.
    Editable {
        class: MaskClass,
        required: bool,
        case: CaseLock,
    },
    /// Fixed literal painted by the field; caret skips over it.
    Fixed(char),
}

/// Character classes that an editable position accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskClass {
    /// `9` / `0` — ASCII digit `0..9`.
    Digit,
    /// `A` / `a` — ASCII letter `a..z`/`A..Z`.
    Letter,
    /// `N` / `n` — letter or digit.
    Alphanumeric,
    /// `X` / `x` — any non-control character.
    Any,
    /// `H` / `h` — hex digit `0..9`/`a..f`/`A..F`.
    HexDigit,
}

impl MaskClass {
    /// Does `c` fit this class?
    pub fn accepts(self, c: char) -> bool {
        match self {
            Self::Digit => c.is_ascii_digit(),
            Self::Letter => c.is_ascii_alphabetic(),
            Self::Alphanumeric => c.is_ascii_alphanumeric(),
            Self::Any => !c.is_control(),
            Self::HexDigit => c.is_ascii_hexdigit(),
        }
    }
}

impl MaskPosition {
    /// Editable shorthand.
    pub fn is_editable(&self) -> bool {
        matches!(self, Self::Editable { .. })
    }
}

/// Parsed mask: a sequence of positions, ready for rendering and
/// per-character routing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputMask {
    positions: Vec<MaskPosition>,
}

impl InputMask {
    /// Parse a Qt-grammar mask string. Errors only on a trailing
    /// backslash with nothing to escape; any unrecognized printable
    /// char becomes a fixed literal. (Qt is similarly permissive.)
    pub fn parse(mask: &str) -> Result<Self, MaskError> {
        let mut positions = Vec::with_capacity(mask.len());
        let mut chars = mask.chars().peekable();
        let mut case = CaseLock::None;

        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    let Some(next) = chars.next() else {
                        return Err(MaskError::TrailingBackslash);
                    };
                    positions.push(MaskPosition::Fixed(next));
                }
                '>' => case = CaseLock::Upper,
                '<' => case = CaseLock::Lower,
                '!' => case = CaseLock::None,
                '9' => positions.push(MaskPosition::Editable {
                    class: MaskClass::Digit,
                    required: true,
                    case,
                }),
                '0' => positions.push(MaskPosition::Editable {
                    class: MaskClass::Digit,
                    required: false,
                    case,
                }),
                'A' => positions.push(MaskPosition::Editable {
                    class: MaskClass::Letter,
                    required: true,
                    case,
                }),
                'a' => positions.push(MaskPosition::Editable {
                    class: MaskClass::Letter,
                    required: false,
                    case,
                }),
                'N' => positions.push(MaskPosition::Editable {
                    class: MaskClass::Alphanumeric,
                    required: true,
                    case,
                }),
                'n' => positions.push(MaskPosition::Editable {
                    class: MaskClass::Alphanumeric,
                    required: false,
                    case,
                }),
                'X' => positions.push(MaskPosition::Editable {
                    class: MaskClass::Any,
                    required: true,
                    case,
                }),
                'x' => positions.push(MaskPosition::Editable {
                    class: MaskClass::Any,
                    required: false,
                    case,
                }),
                'H' => positions.push(MaskPosition::Editable {
                    class: MaskClass::HexDigit,
                    required: true,
                    case,
                }),
                'h' => positions.push(MaskPosition::Editable {
                    class: MaskClass::HexDigit,
                    required: false,
                    case,
                }),
                other => positions.push(MaskPosition::Fixed(other)),
            }
        }

        Ok(Self { positions })
    }

    /// Total number of positions (editable + fixed).
    pub fn len(&self) -> usize {
        self.positions.len()
    }

    /// `true` if the parsed mask has no positions.
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    /// Iterator over positions in document order.
    pub fn positions(&self) -> impl Iterator<Item = &MaskPosition> {
        self.positions.iter()
    }

    /// Position at index `i`, or `None` if out of bounds.
    pub fn get(&self, i: usize) -> Option<&MaskPosition> {
        self.positions.get(i)
    }

    /// Render the mask as the empty-state template: every editable
    /// position becomes `placeholder_char`, every fixed position keeps
    /// its literal. So `99/99/9999` with `_` → `"__/__/____"`.
    pub fn empty_template(&self, placeholder_char: char) -> String {
        let mut s = String::with_capacity(self.positions.len());
        for pos in &self.positions {
            match pos {
                MaskPosition::Editable { .. } => s.push(placeholder_char),
                MaskPosition::Fixed(c) => s.push(*c),
            }
        }
        s
    }

    /// Apply the mask to `raw` input. Walks `raw` and the mask in
    /// lockstep:
    /// - At a fixed-separator position, the separator is appended
    ///   automatically (the user doesn't need to type it). If the next
    ///   `raw` char matches the separator it's consumed; otherwise it
    ///   stays for the next editable position.
    /// - At an editable position, the next `raw` char is consumed
    ///   if it fits the class, else dropped.
    /// - Case lock is applied to letter/alphanumeric/any chars.
    /// - When `raw` is exhausted, the rest of the formatted string
    ///   uses `placeholder_char` for editable positions and the
    ///   literal for fixed ones — giving a partial template like
    ///   `"12/__/____"`.
    ///
    /// The result is the **fully-templated** string (with placeholder
    /// characters for unfilled positions), suitable for direct
    /// rendering. To get just the user's input keep raw and don't
    /// call this; to get a string with separators but no placeholders,
    /// truncate at the position past the last filled editable slot.
    pub fn format(&self, raw: &str, placeholder_char: char) -> FormattedMask {
        let mut buf = String::with_capacity(self.positions.len());
        let mut raw_iter = raw.chars().peekable();
        let mut last_filled_index: Option<usize> = None;

        for (i, pos) in self.positions.iter().enumerate() {
            match pos {
                MaskPosition::Fixed(sep) => {
                    let _ = write!(buf, "{}", sep);
                    // If the user typed the separator themselves,
                    // consume it so subsequent chars route to the
                    // next editable slot (don't double-eat user input).
                    if raw_iter.peek() == Some(sep) {
                        raw_iter.next();
                    }
                }
                MaskPosition::Editable { class, case, .. } => {
                    // Find the next raw char that fits the class.
                    // Skip raw chars that don't fit (consistent with
                    // Qt: if the user pastes `abc12` into `99`, the
                    // letters drop and the digits land).
                    let mut filled = false;
                    while let Some(&c) = raw_iter.peek() {
                        raw_iter.next();
                        if class.accepts(c) {
                            let cased = match case {
                                CaseLock::None => c,
                                CaseLock::Upper => c.to_ascii_uppercase(),
                                CaseLock::Lower => c.to_ascii_lowercase(),
                            };
                            buf.push(cased);
                            last_filled_index = Some(i);
                            filled = true;
                            break;
                        }
                    }
                    if !filled {
                        buf.push(placeholder_char);
                    }
                }
            }
        }

        FormattedMask {
            full: buf,
            last_filled_index,
            mask_len: self.positions.len(),
        }
    }

    /// Strip placeholder characters from the tail of a formatted
    /// string. Returns the prefix containing only filled positions
    /// + their preceding separators. `"12/__/____"` → `"12/"`.
    /// `"12/30/____"` → `"12/30/"`. `"____"` → `""`.
    pub fn strip_trailing_placeholders(
        &self,
        formatted: &FormattedMask,
    ) -> String {
        let Some(last) = formatted.last_filled_index else {
            return String::new();
        };
        // Count chars up to and including position `last`. The
        // formatted buffer is one char per position (mask is ASCII;
        // non-ASCII separators are still single chars in our model).
        formatted.full.chars().take(last + 1).collect()
    }

    /// Position-aware char filter: returns `true` iff `c` would be
    /// accepted at editable position `pos_index`. Fixed positions
    /// never accept input directly (the caret should skip over them).
    /// Out-of-range indices reject. Used by composites that want to
    /// gate keystrokes per-position.
    pub fn accepts_at(&self, pos_index: usize, c: char) -> bool {
        match self.positions.get(pos_index) {
            Some(MaskPosition::Editable { class, .. }) => class.accepts(c),
            _ => false,
        }
    }
}

/// Result of [`InputMask::format`]: the fully-templated string plus
/// metadata about how much of the user's input was consumed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormattedMask {
    /// Fully-templated string with placeholder chars at unfilled
    /// editable positions. Always `mask.len()` chars long.
    pub full: String,
    /// Index of the last editable position that received a user
    /// character. `None` if no editable position was filled.
    pub last_filled_index: Option<usize>,
    /// Total length of the parsed mask (number of positions).
    pub mask_len: usize,
}

impl FormattedMask {
    /// Is every editable position filled?
    pub fn is_complete(&self, mask: &InputMask) -> bool {
        let last_editable = mask
            .positions()
            .enumerate()
            .filter(|(_, p)| p.is_editable())
            .map(|(i, _)| i)
            .last();
        match (self.last_filled_index, last_editable) {
            (Some(filled), Some(target)) => filled == target,
            (None, None) => true,
            _ => false,
        }
    }
}

/// Mask-parse errors. Currently only the trailing-backslash case;
/// any other character becomes a fixed literal (Qt's permissive
/// behaviour) so most accidental "weird" masks just produce odd
/// templates rather than parse failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaskError {
    /// `\` at the end of the mask string with no character to escape.
    TrailingBackslash,
}

impl std::fmt::Display for MaskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TrailingBackslash => f.write_str("trailing `\\` in mask string"),
        }
    }
}

impl std::error::Error for MaskError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> InputMask {
        InputMask::parse(s).unwrap()
    }

    #[test]
    fn parse_date_mask() {
        let m = parse("99/99/9999");
        assert_eq!(m.len(), 10);
        // Editable positions at 0,1,3,4,6,7,8,9 — separator at 2,5
        assert!(matches!(m.get(0), Some(MaskPosition::Editable { class: MaskClass::Digit, required: true, .. })));
        assert!(matches!(m.get(2), Some(MaskPosition::Fixed('/'))));
        assert!(matches!(m.get(5), Some(MaskPosition::Fixed('/'))));
    }

    #[test]
    fn parse_phone_mask() {
        let m = parse("(999) 999-9999");
        // Fixed '(' ')' ' ' '-', plus 10 editable digits.
        let editable_count = m.positions().filter(|p| p.is_editable()).count();
        assert_eq!(editable_count, 10);
    }

    #[test]
    fn parse_uppercase_letters() {
        let m = parse(">AA");
        match m.get(0) {
            Some(MaskPosition::Editable { class: MaskClass::Letter, case: CaseLock::Upper, .. }) => {}
            other => panic!("expected uppercase letter at 0, got {other:?}"),
        }
    }

    #[test]
    fn parse_escape() {
        let m = parse(r"\99");
        // `\9` is a literal '9'; the second `9` is a digit class.
        assert!(matches!(m.get(0), Some(MaskPosition::Fixed('9'))));
        assert!(matches!(m.get(1), Some(MaskPosition::Editable { class: MaskClass::Digit, .. })));
    }

    #[test]
    fn parse_trailing_backslash_errors() {
        assert_eq!(InputMask::parse(r"99\"), Err(MaskError::TrailingBackslash));
    }

    #[test]
    fn empty_template_is_underscore_for_editable() {
        let m = parse("99/99/9999");
        assert_eq!(m.empty_template('_'), "__/__/____");
        assert_eq!(m.empty_template('·'), "··/··/····");
    }

    #[test]
    fn format_partial_fills_then_placeholders() {
        let m = parse("99/99/9999");
        let f = m.format("1", '_');
        assert_eq!(f.full, "1_/__/____");
        assert_eq!(f.last_filled_index, Some(0));

        let f = m.format("12", '_');
        assert_eq!(f.full, "12/__/____");
        assert_eq!(f.last_filled_index, Some(1));

        let f = m.format("123", '_');
        // `1` → pos0, `2` → pos1, fixed `/` at pos2, `3` → pos3.
        assert_eq!(f.full, "12/3_/____");
        assert_eq!(f.last_filled_index, Some(3));
    }

    #[test]
    fn format_consumes_user_typed_separators() {
        // User types `12/30/2026` (with separators); the mask should
        // not double-eat the separators.
        let m = parse("99/99/9999");
        let f = m.format("12/30/2026", '_');
        assert_eq!(f.full, "12/30/2026");
        assert!(f.is_complete(&m));
    }

    #[test]
    fn format_drops_chars_that_dont_fit_class() {
        let m = parse("99/99/9999");
        let f = m.format("abc12def30ghi2026", '_');
        assert_eq!(f.full, "12/30/2026");
    }

    #[test]
    fn format_uppercase_lock_applies() {
        let m = parse(">AA");
        let f = m.format("us", '_');
        assert_eq!(f.full, "US");
    }

    #[test]
    fn format_complete_for_full_input() {
        let m = parse("99/99/9999");
        let f = m.format("12302026", '_');
        assert_eq!(f.full, "12/30/2026");
        assert!(f.is_complete(&m));
    }

    #[test]
    fn format_empty_input_all_placeholders() {
        let m = parse("99/99/9999");
        let f = m.format("", '_');
        assert_eq!(f.full, "__/__/____");
        assert_eq!(f.last_filled_index, None);
        assert!(!f.is_complete(&m));
    }

    #[test]
    fn strip_trailing_placeholders_truncates_template() {
        let m = parse("99/99/9999");
        let f = m.format("12", '_');
        assert_eq!(m.strip_trailing_placeholders(&f), "12");
        let f = m.format("123", '_');
        assert_eq!(m.strip_trailing_placeholders(&f), "12/3");
        let f = m.format("12302026", '_');
        assert_eq!(m.strip_trailing_placeholders(&f), "12/30/2026");
    }

    #[test]
    fn accepts_at_position() {
        let m = parse("99/99/9999");
        assert!(m.accepts_at(0, '5'));     // editable digit
        assert!(!m.accepts_at(0, 'a'));    // letter at digit pos
        assert!(!m.accepts_at(2, '/'));    // fixed: never accepts
        assert!(!m.accepts_at(99, '5'));   // out of range
    }
}
