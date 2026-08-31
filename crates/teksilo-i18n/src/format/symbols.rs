// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Locale number symbols, and the parse direction that mirrors
//! [`NumberFormatter`](super::NumberFormatter).
//!
//! # Why the symbols are *probed* rather than read
//!
//! ICU4X does expose `icu_decimal::provider::DecimalSymbols` with public
//! `decimal_separator` / `grouping_separator` fields, but that struct
//! carries ICU's "unstable Rust representation, may change in SemVer minor
//! releases" banner. More importantly, reading the provider gives symbols
//! that are only *believed* to match what the formatter emits.
//!
//! So instead we format a probe value through the very
//! [`DecimalFormatter`] the display path uses and read the separators back
//! out of its [`icu_decimal::parts`] annotations. The symbols are
//! then the ones the formatter actually produced, by construction — the
//! same property that makes `DateEdit`'s single `ParsedPattern` drive
//! format, parse, and input mask without drifting.
//!
//! # What "de-localizing" means
//!
//! [`NumberSymbols::delocalize`] does **not** parse to a number. It
//! rewrites a locale-formatted string into a C-locale one:
//!
//! ```text
//!   "1 234,56"  (fr-FR, U+202F group separator)  →  "1234.56"
//!   "1.234,56"  (de-DE)                          →  "1234.56"
//!   "١٢٣٤٫٥٦"   (ar-EG, arab numbering system)    →  "1234.56"
//! ```
//!
//! The caller then applies its own numeric parse. That keeps exactness
//! decisions where the type lives: routing `SpinBox<i64>` through an `f64`
//! here would silently lose integers past 2^53, and this layer has no way
//! to know that mattered.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use icu_decimal::DecimalFormatter;
use icu_decimal::options::{DecimalFormatterOptions, GroupingStrategy};
use icu_decimal::parts;
use unic_langid::LanguageIdentifier;
use writeable::{Part, PartsWrite, Writeable};

use super::lang_to_icu_locale;
use crate::thread_local::with_active;

/// The symbols a locale uses to write a decimal number, recovered from
/// ICU's own formatted output.
///
/// Obtain one with [`NumberSymbols::for_locale`] or
/// [`NumberSymbols::current`]; both are cached per locale, so repeated
/// calls do not re-run the probe.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NumberSymbols {
    decimal_separator: String,
    group_separator: String,
    minus_sign: String,
    plus_sign: String,
    zero_digit: char,
    /// Digits in the rightmost group (3 nearly everywhere).
    primary_group_size: u8,
    /// Digits in every group above the first — 3 in most locales, but 2 in
    /// the Indic lakh/crore system (`12,34,567`).
    secondary_group_size: u8,
}

impl Default for NumberSymbols {
    /// The C-locale / ISO fallback, used when no `I18nManager` is
    /// installed on this thread or when ICU has no data for the locale.
    fn default() -> Self {
        Self {
            decimal_separator: ".".to_string(),
            group_separator: ",".to_string(),
            minus_sign: "-".to_string(),
            plus_sign: "+".to_string(),
            zero_digit: '0',
            primary_group_size: 3,
            secondary_group_size: 3,
        }
    }
}

impl NumberSymbols {
    /// Character(s) separating the integer and fraction parts — `.` in
    /// en-US, `,` in fr-FR and de-DE, `٫` (U+066B) in ar-EG.
    pub fn decimal_separator(&self) -> &str {
        &self.decimal_separator
    }

    /// Character(s) separating thousands groups — `,` in en-US, `.` in
    /// de-DE, U+202F (narrow no-break space) in fr-FR.
    pub fn group_separator(&self) -> &str {
        &self.group_separator
    }

    /// The locale's minus sign. Not always ASCII `-`: several locales use
    /// U+2212 MINUS SIGN, and RTL locales may add directional marks.
    pub fn minus_sign(&self) -> &str {
        &self.minus_sign
    }

    /// The locale's explicit plus sign.
    pub fn plus_sign(&self) -> &str {
        &self.plus_sign
    }

    /// Digit zero of the locale's default numbering system. Digits are
    /// contiguous from here: every CLDR numbering system Teksilo can
    /// select is decimal and encoded as ten consecutive code points.
    pub fn zero_digit(&self) -> char {
        self.zero_digit
    }

    /// Whether this locale writes digits in something other than ASCII
    /// (`arab`, `deva`, …).
    pub fn has_non_ascii_digits(&self) -> bool {
        self.zero_digit != '0'
    }

    /// Symbols for `lang`, cached per locale for the life of the thread.
    pub fn for_locale(lang: &LanguageIdentifier) -> Rc<Self> {
        SYMBOL_CACHE.with(|c| {
            let mut c = c.borrow_mut();
            if let Some(existing) = c.get(lang) {
                return existing.clone();
            }
            let rc = Rc::new(probe(lang).unwrap_or_default());
            c.insert(lang.clone(), rc.clone());
            rc
        })
    }

    /// Symbols for the locale of the active `I18nManager`, or the C-locale
    /// fallback when none is installed.
    pub fn current() -> Rc<Self> {
        let lang = with_active(|m| m.locale_signal().get()).unwrap_or_default();
        Self::for_locale(&lang)
    }

    /// Rewrite a locale-formatted number into a C-locale one that
    /// `str::parse` accepts: group separators dropped, decimal separator
    /// normalized to `.`, signs normalized to ASCII, and the locale's
    /// digits mapped to ASCII.
    ///
    /// Returns `None` when the input holds a character this locale cannot
    /// account for, when it carries no digit at all, or when the rewritten
    /// result is not syntactically a number (`"1.234,56"` typed into a
    /// fr-FR field would yield two decimal points, and is rejected).
    ///
    /// Parsing is **lenient**, matching ICU's own default: whitespace is
    /// ignored anywhere, and ASCII digits are accepted even in a locale
    /// whose numbering system is not `latn` (people type on the keyboard
    /// they have).
    ///
    /// One ambiguity is inherent and resolved in CLDR's favour: in a
    /// locale whose *group* separator is `.` (de-DE), `"1.5"` de-localizes
    /// to `"15"`, not `"1.5"` — the `.` is read as grouping, because that
    /// is what it means when this locale writes a number. In locales where
    /// `.` is neither separator it is accepted as a decimal point, so a
    /// numeric-keypad `.` still works in fr-FR.
    pub fn delocalize(&self, input: &str) -> Option<String> {
        let mut out = String::with_capacity(input.len());
        let mut rest = input.trim();
        let mut saw_digit = false;

        while !rest.is_empty() {
            // Multi-character symbols first: a separator may be longer
            // than one char, and `group` must beat the bare-`.` branch
            // below so de-DE reads `.` as grouping.
            if !self.group_separator.is_empty()
                && let Some(r) = rest.strip_prefix(&self.group_separator)
            {
                rest = r;
                continue;
            }
            if !self.decimal_separator.is_empty()
                && let Some(r) = rest.strip_prefix(&self.decimal_separator)
            {
                out.push('.');
                rest = r;
                continue;
            }
            if !self.minus_sign.is_empty()
                && let Some(r) = rest.strip_prefix(&self.minus_sign)
            {
                out.push('-');
                rest = r;
                continue;
            }
            if !self.plus_sign.is_empty()
                && let Some(r) = rest.strip_prefix(&self.plus_sign)
            {
                out.push('+');
                rest = r;
                continue;
            }

            let c = rest.chars().next()?;
            let advance = c.len_utf8();

            if let Some(d) = self.ascii_digit(c) {
                out.push(d);
                saw_digit = true;
            } else if c.is_whitespace() {
                // Includes U+00A0 / U+202F: a pasted or hand-typed space
                // stands in for whichever space this locale groups with.
            } else if c == '.' {
                // Reachable only when `.` is neither separator for this
                // locale — accept it as the numpad decimal point.
                out.push('.');
            } else if matches!(c, '-' | '+' | 'e' | 'E') {
                out.push(c);
            } else {
                return None;
            }
            rest = &rest[advance..];
        }

        if !saw_digit {
            return None;
        }
        // Structural check. The rewrite is per-character, so a lenient
        // input can still assemble into nonsense (two decimal points, a
        // stray sign). Parsing as `f64` validates *syntax* only — the
        // string is what we return, so the caller's own numeric type still
        // decides range and exactness.
        out.parse::<f64>().ok().map(|_| out)
    }

    /// Digits in the rightmost thousands group.
    pub fn primary_group_size(&self) -> u8 {
        self.primary_group_size
    }

    /// Digits in each group above the first. Differs from
    /// [`primary_group_size`](Self::primary_group_size) in the Indic
    /// lakh/crore system, where 1234567 is written `12,34,567`.
    pub fn secondary_group_size(&self) -> u8 {
        self.secondary_group_size
    }

    /// The inverse of [`delocalize`](Self::delocalize): rewrite a C-locale
    /// numeric string into this locale's conventions.
    ///
    /// Takes and returns a **string** rather than a number, so a caller
    /// holding an `i64`/`u64` keeps full precision — formatting through
    /// `f64` would silently round anything past 2^53. That is why an
    /// editable numeric widget should render with this rather than with
    /// [`NumberFormatter`](super::NumberFormatter), which is `f64`-based
    /// and aimed at display-only values.
    ///
    /// `grouping` inserts thousands separators; leave it off for a field
    /// the user types into, where separators fight the caret.
    ///
    /// Input is expected to be what `f64`/integer `Display` produces:
    /// optional sign, digits, optional `.` and fraction. Anything else —
    /// notably scientific notation — is returned unchanged rather than
    /// mangled.
    pub fn localize(&self, plain: &str, grouping: bool) -> String {
        if plain.contains(['e', 'E']) {
            return plain.to_string();
        }
        let (sign, rest) = match plain.strip_prefix('-') {
            Some(r) => (self.minus_sign.as_str(), r),
            None => match plain.strip_prefix('+') {
                Some(r) => (self.plus_sign.as_str(), r),
                None => ("", plain),
            },
        };
        let (int_part, frac_part) = match rest.split_once('.') {
            Some((i, f)) => (i, Some(f)),
            None => (rest, None),
        };
        if !int_part.bytes().all(|b| b.is_ascii_digit())
            || frac_part.is_some_and(|f| !f.bytes().all(|b| b.is_ascii_digit()))
        {
            return plain.to_string();
        }

        let mut out = String::with_capacity(plain.len() + 8);
        out.push_str(sign);

        if grouping {
            out.push_str(&self.group_integer(int_part));
        } else {
            self.push_shaped(&mut out, int_part);
        }
        if let Some(frac) = frac_part {
            out.push_str(&self.decimal_separator);
            self.push_shaped(&mut out, frac);
        }
        out
    }

    /// Insert this locale's group separators into a run of ASCII digits,
    /// shaping the digits on the way. Grouping runs right-to-left: the
    /// rightmost group takes `primary_group_size` digits, every group
    /// above it takes `secondary_group_size`.
    fn group_integer(&self, digits: &str) -> String {
        let primary = self.primary_group_size.max(1) as usize;
        let secondary = self.secondary_group_size.max(1) as usize;

        let bytes = digits.as_bytes();
        let mut cuts = Vec::new();
        let mut pos = bytes.len();
        let mut size = primary;
        while pos > size {
            pos -= size;
            cuts.push(pos);
            size = secondary;
        }
        cuts.reverse();

        let mut out = String::with_capacity(digits.len() * 2);
        let mut prev = 0usize;
        for cut in cuts {
            self.push_shaped(&mut out, &digits[prev..cut]);
            out.push_str(&self.group_separator);
            prev = cut;
        }
        self.push_shaped(&mut out, &digits[prev..]);
        out
    }

    /// Append ASCII digits in this locale's numbering system.
    fn push_shaped(&self, out: &mut String, digits: &str) {
        if self.zero_digit == '0' {
            out.push_str(digits);
            return;
        }
        let zero = self.zero_digit as u32;
        for c in digits.chars() {
            match c.to_digit(10) {
                Some(d) => out.push(char::from_u32(zero + d).unwrap_or(c)),
                None => out.push(c),
            }
        }
    }

    /// Map one character to its ASCII digit, accepting both this locale's
    /// numbering system and plain ASCII.
    fn ascii_digit(&self, c: char) -> Option<char> {
        if c.is_ascii_digit() {
            return Some(c);
        }
        if self.zero_digit == '0' {
            return None;
        }
        let zero = self.zero_digit as u32;
        let code = c as u32;
        (code >= zero && code < zero + 10)
            .then(|| char::from_digit(code - zero, 10))
            .flatten()
    }
}

thread_local! {
    static SYMBOL_CACHE: RefCell<HashMap<LanguageIdentifier, Rc<NumberSymbols>>> =
        RefCell::new(HashMap::new());
}

// ------------------------------------------------------------
// The probe
// ------------------------------------------------------------

/// Records the `Part` annotations `FormattedDecimal` emits, alongside the
/// string it wrote. Structurally identical to `writeable`'s own internal
/// test writer — that crate offers no public recording sink.
#[derive(Default)]
struct PartsRecorder {
    string: String,
    parts: Vec<(usize, usize, Part)>,
}

impl fmt::Write for PartsRecorder {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.string.push_str(s);
        Ok(())
    }
}

impl PartsWrite for PartsRecorder {
    type SubPartsWrite = Self;

    fn with_part(
        &mut self,
        part: Part,
        mut f: impl FnMut(&mut Self::SubPartsWrite) -> fmt::Result,
    ) -> fmt::Result {
        let start = self.string.len();
        f(self)?;
        let end = self.string.len();
        if start < end {
            self.parts.push((start, end, part));
        }
        Ok(())
    }
}

impl PartsRecorder {
    fn record(w: &impl Writeable) -> Self {
        let mut rec = Self::default();
        // Writing into a String cannot fail; a sink error here would mean
        // the formatter itself misbehaved, in which case an empty probe
        // result falls back to the C-locale symbols.
        let _ = w.write_to_parts(&mut rec);
        rec
    }

    /// The first span annotated with `part`, as a string.
    fn first(&self, part: Part) -> Option<String> {
        self.parts
            .iter()
            .find(|(_, _, p)| *p == part)
            .map(|(s, e, _)| self.string[*s..*e].to_string())
    }
}

/// Format three probe values through `lang`'s own decimal formatter and
/// read the symbols back out of the part annotations.
fn probe(lang: &LanguageIdentifier) -> Option<NumberSymbols> {
    let icu_locale = lang_to_icu_locale(lang);
    let mut opts = DecimalFormatterOptions::default();
    // Grouping must be on, or the probe emits no `GROUP` part at all.
    opts.grouping_strategy = Some(GroupingStrategy::Auto);
    let formatter = DecimalFormatter::try_new((&icu_locale).into(), opts).ok()?;

    let fallback = NumberSymbols::default();

    // Probe 1 — a negative, grouped, fractional value yields MINUS_SIGN,
    // GROUP and DECIMAL in one pass.
    let signed: fixed_decimal::Decimal = "-1234567.8".parse().ok()?;
    let rec = PartsRecorder::record(&formatter.format(&signed));
    let group_separator = rec
        .first(parts::GROUP)
        .unwrap_or_else(|| fallback.group_separator.clone());
    let decimal_separator = rec
        .first(parts::DECIMAL)
        .unwrap_or_else(|| fallback.decimal_separator.clone());
    let minus_sign = rec
        .first(parts::MINUS_SIGN)
        .unwrap_or_else(|| fallback.minus_sign.clone());

    // Probe 2 — an explicit positive sign is only rendered on request.
    let positive = "1"
        .parse::<fixed_decimal::Decimal>()
        .ok()?
        .with_sign(fixed_decimal::Sign::Positive);
    let plus_sign = PartsRecorder::record(&formatter.format(&positive))
        .first(parts::PLUS_SIGN)
        .unwrap_or_else(|| fallback.plus_sign.clone());

    // Probe 3 — zero renders as exactly one digit, in this locale's
    // numbering system.
    let zero: fixed_decimal::Decimal = "0".parse().ok()?;
    let zero_digit = formatter
        .format(&zero)
        .to_string()
        .chars()
        .next()
        .unwrap_or('0');

    // Probe 4 — group sizes. Splitting a 7-digit grouped integer on the
    // separator distinguishes the usual 3;3 from the Indic 3;2 lakh
    // system (`12,34,567`), which a hardcoded "every three digits" would
    // render wrong.
    let grouped: fixed_decimal::Decimal = "1234567".parse().ok()?;
    let grouped = formatter.format(&grouped).to_string();
    let mut chunks: Vec<usize> = if group_separator.is_empty() {
        Vec::new()
    } else {
        grouped
            .split(group_separator.as_str())
            .map(|c| c.chars().count())
            .collect()
    };
    let primary_group_size = chunks.pop().unwrap_or(3).clamp(1, 255) as u8;
    let secondary_group_size = chunks
        .pop()
        .unwrap_or(primary_group_size as usize)
        .clamp(1, 255) as u8;

    Some(NumberSymbols {
        decimal_separator,
        group_separator,
        minus_sign,
        plus_sign,
        zero_digit,
        primary_group_size,
        secondary_group_size,
    })
}

/// De-localize `input` against the active locale's symbols. Convenience
/// wrapper over [`NumberSymbols::current`] +
/// [`NumberSymbols::delocalize`].
pub fn delocalize_number(input: &str) -> Option<String> {
    NumberSymbols::current().delocalize(input)
}
