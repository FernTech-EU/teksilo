// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Injected, language-agnostic editing configuration.
//!
//! Everything a code editor does that *looks* language-specific is a mechanism
//! here plus a value the application supplies. The editor knows how to toggle a
//! line comment; it does not know that Rust uses `//`. It knows how to close a
//! bracket; it does not know that Rust has `<>` in generics and Python does not.
//!
//! This is the difference between a widget and an IDE, and it is why there is
//! no `Language` enum anywhere in this module: adding one would mean every new
//! language is a change to Bastyde rather than a value in the caller's code.

/// How a line's leading indentation is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentStyle {
    /// `width` spaces per indent level.
    Spaces(u8),
    /// One tab character per level, rendered `width` columns wide.
    Tabs { width: u8 },
}

impl IndentStyle {
    /// The text one indent level inserts.
    pub fn unit(&self) -> String {
        match self {
            Self::Spaces(n) => " ".repeat(*n as usize),
            Self::Tabs { .. } => "\t".to_string(),
        }
    }

    /// How many columns one level occupies on screen. Both styles need this:
    /// spaces to know how many to strip on dedent, tabs to render the stop.
    pub fn width(&self) -> u8 {
        match self {
            Self::Spaces(n) => *n,
            Self::Tabs { width } => *width,
        }
    }
}

impl Default for IndentStyle {
    /// Four spaces. Chosen because it is the majority default across editors
    /// and is unambiguous on every renderer; a project that disagrees says so.
    fn default() -> Self {
        Self::Spaces(4)
    }
}

/// A pair of characters the editor treats as opening and closing delimiters.
///
/// Used for auto-closing and for match highlighting. The application declares
/// the set, because the *same* character means different things per language:
/// `<` is a bracket in a generic parameter list and a less-than sign in
/// arithmetic, and only the caller knows which document this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BracketPair {
    pub open: char,
    pub close: char,
}

impl BracketPair {
    pub const fn new(open: char, close: char) -> Self {
        Self { open, close }
    }
}

/// The three pairs that are structural in essentially every bracketed
/// language. A convenience starting point, not a default — an editor with no
/// configured pairs simply does no bracket handling, which is correct for
/// prose or a log.
pub const COMMON_BRACKETS: &[BracketPair] = &[
    BracketPair::new('(', ')'),
    BracketPair::new('[', ']'),
    BracketPair::new('{', '}'),
];

/// Editing behaviour the code editor applies, all supplied by the application.
#[derive(Debug, Clone)]
pub struct CodeConfig {
    /// How indentation is written and how wide a level is.
    pub indent: IndentStyle,
    /// Whether Enter carries the current line's leading whitespace onto the
    /// new line.
    pub auto_indent: bool,
    /// Token that starts a line comment (`"//"`, `"#"`, `"--"`, `";"`).
    /// `None` disables `CodeCommand::ToggleLineComment` entirely rather than
    /// guessing.
    pub line_comment: Option<String>,
    /// Delimiter pairs for auto-closing and match highlighting. Empty disables
    /// both.
    pub brackets: Vec<BracketPair>,
    /// Whether typing an opening delimiter inserts its closing partner.
    pub auto_close_brackets: bool,
    /// Whether the delimiter matching the caret's is highlighted.
    pub match_brackets: bool,
}

impl Default for CodeConfig {
    /// The language-neutral default: indent and auto-indent work (they need no
    /// language knowledge), while comment toggling and bracket handling stay
    /// **off** because they cannot be right without the application saying what
    /// the tokens are. A wrong guess here is worse than nothing — inserting `//`
    /// into a Python file corrupts it silently.
    fn default() -> Self {
        Self {
            indent: IndentStyle::default(),
            auto_indent: true,
            line_comment: None,
            brackets: Vec::new(),
            auto_close_brackets: false,
            match_brackets: false,
        }
    }
}

impl CodeConfig {
    /// The closing partner for `open`, if it is a configured opening delimiter.
    pub fn closing_for(&self, open: char) -> Option<char> {
        self.brackets
            .iter()
            .find(|p| p.open == open)
            .map(|p| p.close)
    }

    /// The opening partner for `close`, if it is a configured closing delimiter.
    pub fn opening_for(&self, close: char) -> Option<char> {
        self.brackets
            .iter()
            .find(|p| p.close == close)
            .map(|p| p.open)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spaces_indent_unit_is_its_width() {
        assert_eq!(IndentStyle::Spaces(4).unit(), "    ");
        assert_eq!(IndentStyle::Spaces(2).unit(), "  ");
    }

    /// A tab is one character however wide it renders — conflating the two is
    /// the classic indent bug (deleting 4 columns eats 4 tabs).
    #[test]
    fn tab_indent_unit_is_one_character_regardless_of_width() {
        assert_eq!(IndentStyle::Tabs { width: 8 }.unit(), "\t");
        assert_eq!(IndentStyle::Tabs { width: 8 }.width(), 8);
    }

    /// The default must not pretend to know the language. Guessing `//` would
    /// silently corrupt a Python file the first time someone hit Ctrl+/.
    #[test]
    fn the_default_config_makes_no_language_assumptions() {
        let c = CodeConfig::default();
        assert!(c.line_comment.is_none(), "must not guess a comment token");
        assert!(c.brackets.is_empty(), "must not guess bracket pairs");
        assert!(!c.auto_close_brackets);
        assert!(!c.match_brackets);
        // These two need no language knowledge, so they are on.
        assert!(c.auto_indent);
        assert_eq!(c.indent, IndentStyle::Spaces(4));
    }

    #[test]
    fn bracket_lookup_resolves_both_directions() {
        let c = CodeConfig {
            brackets: COMMON_BRACKETS.to_vec(),
            ..CodeConfig::default()
        };
        assert_eq!(c.closing_for('('), Some(')'));
        assert_eq!(c.opening_for('}'), Some('{'));
        assert_eq!(c.closing_for('<'), None, "unconfigured pairs stay unknown");
    }
}
