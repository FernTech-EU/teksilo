// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The KDE configuration text format and its cascade.
//!
//! [`KConfig::parse`] reads one file (one *layer*) and never fails: every
//! malformed line is skipped and reported as a [`ParseDiagnostic`].
//! [`KConfig::cascade`] merges layers, lowest priority first, the way KDE
//! reads `kdeglobals` from the system directories and then the user's own
//! file.
//!
//! # Grammar
//!
//! - The file is split into lines at byte `0x0A` only, and each line is
//!   trimmed of ASCII whitespace, so a CR before the LF goes away.
//! - An empty line, or one whose first byte is `#`, is ignored. `;` is not a
//!   comment.
//! - A line starting with `[` is a group header: `[a][b]` names the group
//!   `b` nested inside `a`. A nested group is a separate group; it inherits
//!   nothing from its parent. A header with an unclosed segment is skipped
//!   and the current group does not change.
//! - A trailing `[$i]` segment makes the group immutable; a `[$i]` line on
//!   its own makes the rest of the file immutable, and no higher layer is
//!   read at all.
//! - Any other line is `key=value`, both trimmed. The key may carry
//!   bracketed suffixes: `[$i]` (immutable entry), `[$e]` (environment
//!   expansion on string reads), `[$d]` (deleted entry, which hides the key
//!   from lower layers), and a locale tag such as `[de]`. Locale-tagged
//!   entries are ignored.
//! - `\s`, `\t`, `\n`, `\r`, `\\` and `\xHH` are decoded in keys, values and
//!   group names.
//! - A UTF-8 byte order mark is not recognised: it stays part of the first
//!   line, as KDE does, and a diagnostic names it.
//! - Text is decoded as UTF-8 with invalid sequences replaced by U+FFFD.
//! - Within one file the last occurrence of a key wins, unless an earlier
//!   occurrence was immutable.
//!
//! `[$e]` expansion never runs a command: `$(cmd)` reads back as `(cmd)`.

use std::collections::BTreeMap;
use std::fmt;

use crate::color::KdeColor;

/// A line longer than this many bytes is skipped.
const MAX_LINE_BYTES: usize = 1536 * 1024;

/// After this many counted errors the rest of the file is not read.
const MAX_ERRORS: usize = 100;

/// The UTF-8 byte order mark.
const BOM: &[u8] = b"\xEF\xBB\xBF";

/// Where a layer sits in a cascade, which changes how immutable groups
/// treat it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LayerKind {
    /// A system file, or the user's own `system.kdeglobals`.
    #[default]
    Default,
    /// The user's own file, the one a KDE program writes (`kdeglobals`,
    /// `breezerc`). A group a lower layer made immutable ignores every
    /// entry of this layer.
    User,
}

/// What a [`ParseDiagnostic`] reports.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DiagnosticKind {
    /// The file starts with a UTF-8 byte order mark, which is kept as part
    /// of the first line, so a group header there is not recognised.
    #[error("the file starts with a UTF-8 byte order mark, which is part of the first line")]
    ByteOrderMark,
    /// A line longer than 1.5 MiB, skipped.
    #[error("the line is longer than 1.5 MiB and was skipped")]
    LineTooLong,
    /// A group header segment has no closing `]`; the line was skipped and
    /// the current group kept.
    #[error("a group header segment has no closing `]`")]
    UnclosedGroupHeader,
    /// A key has a `[` with no `]` after it; the line was skipped.
    #[error("a key has a `[` with no closing `]`")]
    UnclosedKeyBracket,
    /// A key has two locale tags; the line was skipped.
    #[error("a key has more than one locale tag")]
    SecondLocale,
    /// An entry line has no `=` and is not a deletion; it was skipped.
    #[error("the line has no `=`")]
    MissingEquals,
    /// An escape sequence that is not one of the recognised ones; the
    /// backslash and the character after it were kept.
    #[error("unknown escape sequence `\\{0}`")]
    InvalidEscape(char),
    /// The file reached the error limit; the rest of it was not read.
    #[error("too many errors, the rest of the file was not read")]
    TooManyErrors,
}

impl DiagnosticKind {
    /// Whether the diagnostic counts toward the error limit.
    fn is_counted(&self) -> bool {
        matches!(
            self,
            DiagnosticKind::UnclosedGroupHeader
                | DiagnosticKind::UnclosedKeyBracket
                | DiagnosticKind::SecondLocale
                | DiagnosticKind::MissingEquals
                | DiagnosticKind::InvalidEscape(_)
        )
    }
}

/// One problem found while parsing, with the 1-based line it is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseDiagnostic {
    /// The 1-based line number.
    pub line: usize,
    /// What was wrong.
    pub kind: DiagnosticKind,
}

impl fmt::Display for ParseDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.kind)
    }
}

/// One stored entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    value: Option<String>,
    immutable: bool,
    expand: bool,
}

impl Entry {
    /// The decoded value, or `None` for a deleted entry.
    pub fn value(&self) -> Option<&str> {
        self.value.as_deref()
    }

    /// Whether the entry was deleted with `[$d]`.
    pub fn is_deleted(&self) -> bool {
        self.value.is_none()
    }

    /// Whether the entry is immutable: no later occurrence and no higher
    /// layer can change it.
    pub fn is_immutable(&self) -> bool {
        self.immutable
    }

    /// Whether the entry carries `[$e]`, so a string read expands its
    /// environment variables.
    pub fn expands(&self) -> bool {
        self.expand
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Group {
    entries: BTreeMap<String, Entry>,
    immutable: bool,
}

/// One parsed configuration file, or the merge of several.
///
/// Groups are addressed by their path: `&[]` is the root group (entries
/// before any header), `&["General"]` is `[General]`, and
/// `&["Colors:Header", "Inactive"]` is `[Colors:Header][Inactive]`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KConfig {
    groups: BTreeMap<Vec<String>, Group>,
    file_immutable: bool,
    kind: LayerKind,
}

/// The variables a `[$e]` string read may expand.
///
/// [`KdeConfigLocations::expansion_env`](crate::KdeConfigLocations::expansion_env)
/// builds one from the same environment the locations were read from.
pub struct ExpansionEnv<'a> {
    var: &'a dyn Fn(&str) -> Option<String>,
    data_home: String,
    config_home: String,
    cache_home: String,
}

impl<'a> ExpansionEnv<'a> {
    /// An expansion environment that reads variables through `var` and
    /// replaces an unset `QT_DATA_HOME`, `QT_CONFIG_HOME` or
    /// `QT_CACHE_HOME` with the given directories.
    pub fn new(
        var: &'a dyn Fn(&str) -> Option<String>,
        data_home: impl Into<String>,
        config_home: impl Into<String>,
        cache_home: impl Into<String>,
    ) -> Self {
        ExpansionEnv {
            var,
            data_home: data_home.into(),
            config_home: config_home.into(),
            cache_home: cache_home.into(),
        }
    }

    fn lookup(&self, name: &str) -> String {
        match (self.var)(name) {
            Some(value) => value,
            None => match name {
                "QT_DATA_HOME" => self.data_home.clone(),
                "QT_CONFIG_HOME" => self.config_home.clone(),
                "QT_CACHE_HOME" => self.cache_home.clone(),
                _ => String::new(),
            },
        }
    }

    /// Expands the environment references in `value`.
    ///
    /// `$$` is a literal `$`; `${NAME}` and `$NAME` (the longest run of
    /// letters, digits and `_`) are replaced by the variable's value, or by
    /// nothing when it is unset or empty; a `$` followed by anything else is
    /// removed, so `$(cmd)` becomes `(cmd)` and nothing is executed; a
    /// trailing `$` is kept.
    pub fn expand(&self, value: &str) -> String {
        let mut out = String::with_capacity(value.len());
        let mut rest = value;
        while let Some(at) = rest.find('$') {
            out.push_str(&rest[..at]);
            let after = &rest[at + 1..];
            let mut chars = after.chars();
            match chars.next() {
                None => {
                    out.push('$');
                    rest = after;
                }
                Some('$') => {
                    out.push('$');
                    rest = &after[1..];
                }
                Some('{') => {
                    let inner = &after[1..];
                    let (name, next) = match inner.find('}') {
                        Some(end) => (&inner[..end], &inner[end + 1..]),
                        None => (inner, ""),
                    };
                    if !name.is_empty() {
                        out.push_str(&self.lookup(name));
                    }
                    rest = next;
                }
                Some(c) if is_name_char(c) => {
                    let end = after
                        .char_indices()
                        .find(|&(_, c)| !is_name_char(c))
                        .map_or(after.len(), |(i, _)| i);
                    out.push_str(&self.lookup(&after[..end]));
                    rest = &after[end..];
                }
                Some(_) => {
                    rest = after;
                }
            }
        }
        out.push_str(rest);
        out
    }
}

impl fmt::Debug for ExpansionEnv<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExpansionEnv")
            .field("data_home", &self.data_home)
            .field("config_home", &self.config_home)
            .field("cache_home", &self.cache_home)
            .finish_non_exhaustive()
    }
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl KConfig {
    /// Parses one file's text. Never fails; every problem is reported in
    /// the returned diagnostics and the offending line is skipped.
    pub fn parse(text: &str) -> (KConfig, Vec<ParseDiagnostic>) {
        KConfig::parse_bytes(text.as_bytes())
    }

    /// Parses one file's bytes, decoding text as UTF-8 with invalid
    /// sequences replaced by U+FFFD.
    pub fn parse_bytes(bytes: &[u8]) -> (KConfig, Vec<ParseDiagnostic>) {
        Parser::default().run(bytes)
    }

    /// The same layer, marked as the given [`LayerKind`].
    pub fn with_kind(mut self, kind: LayerKind) -> KConfig {
        self.kind = kind;
        self
    }

    /// This layer's kind ([`LayerKind::Default`] unless set).
    pub fn kind(&self) -> LayerKind {
        self.kind
    }

    /// Whether the layer contains a bare `[$i]` line, so no higher layer is
    /// read on top of it.
    pub fn is_file_immutable(&self) -> bool {
        self.file_immutable
    }

    /// The same merge with its whole-file lock lifted, so a file cascaded on
    /// it is read again.
    ///
    /// The entries the lock made immutable stay immutable, and so do the
    /// groups a `[$i]` header locked, so a file cascaded on the result still
    /// cannot change them; it can only add keys they do not set and replace
    /// entries that were never locked. Used for the colour-scheme file of
    /// the no-colours fallback, which is not a layer of the `kdeglobals`
    /// cascade that a whole-file lock suppresses.
    pub(crate) fn without_file_lock(mut self) -> KConfig {
        self.file_immutable = false;
        self
    }

    /// Merges `layers`, lowest priority first.
    ///
    /// For each entry of each layer, in order:
    ///
    /// 1. an entry the merge already holds as immutable is kept;
    /// 2. a key the merge does not hold yet, in a group a lower layer made
    ///    immutable, is ignored;
    /// 3. in a [`LayerKind::User`] layer, every entry of a group a lower
    ///    layer made immutable is ignored;
    /// 4. otherwise the entry replaces the one held (a deleted entry hides
    ///    the key).
    ///
    /// A layer that is file-immutable stops the merge: no higher layer is
    /// read. The result is a [`LayerKind::Default`] layer that can itself be
    /// cascaded under another file.
    pub fn cascade(layers: &[KConfig]) -> KConfig {
        let mut merged = KConfig::default();
        for layer in layers {
            if merged.file_immutable {
                break;
            }
            for (path, group) in &layer.groups {
                let target = merged.groups.entry(path.clone()).or_default();
                let locked_by_lower = target.immutable;
                if locked_by_lower && layer.kind == LayerKind::User {
                    continue;
                }
                for (key, entry) in &group.entries {
                    match target.entries.get(key) {
                        Some(held) if held.immutable => {}
                        None if locked_by_lower => {}
                        _ => {
                            target.entries.insert(key.clone(), entry.clone());
                        }
                    }
                }
            }
            for (path, group) in &layer.groups {
                if group.immutable {
                    merged.groups.entry(path.clone()).or_default().immutable = true;
                }
            }
            if layer.file_immutable {
                merged.file_immutable = true;
            }
        }
        merged
            .groups
            .retain(|_, g| !g.entries.is_empty() || g.immutable);
        merged
    }

    /// The stored entry for `key` in `group`, deleted entries included.
    pub fn entry(&self, group: &[&str], key: &str) -> Option<&Entry> {
        self.groups.get(&owned_path(group))?.entries.get(key)
    }

    /// The decoded value of `key` in `group`, or `None` when it is absent
    /// or deleted. Not expanded, even when the entry carries `[$e]`.
    pub fn get(&self, group: &[&str], key: &str) -> Option<&str> {
        self.entry(group, key)?.value()
    }

    /// Whether `key` is present (and not deleted) in `group`.
    pub fn has_key(&self, group: &[&str], key: &str) -> bool {
        self.get(group, key).is_some()
    }

    /// Whether `group` was made immutable by a `[$i]` header.
    pub fn is_group_immutable(&self, group: &[&str]) -> bool {
        self.groups
            .get(&owned_path(group))
            .is_some_and(|g| g.immutable)
    }

    /// Whether `group` exists: it, or a group nested inside it, holds at
    /// least one entry that is not deleted.
    pub fn group_exists(&self, group: &[&str]) -> bool {
        self.groups.iter().any(|(path, g)| {
            path.len() >= group.len()
                && path.iter().zip(group).all(|(a, b)| a == b)
                && g.entries.values().any(|e| !e.is_deleted())
        })
    }

    /// Every group path that holds an entry, in sorted order.
    pub fn groups(&self) -> impl Iterator<Item = Vec<&str>> {
        self.groups
            .iter()
            .filter(|(_, g)| !g.entries.is_empty())
            .map(|(path, _)| path.iter().map(String::as_str).collect())
    }

    /// The present (not deleted) keys of `group`, in sorted order.
    pub fn keys(&self, group: &[&str]) -> Vec<&str> {
        self.groups
            .get(&owned_path(group))
            .map(|g| {
                g.entries
                    .iter()
                    .filter(|(_, e)| !e.is_deleted())
                    .map(|(k, _)| k.as_str())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The value of `key` in `group` read as a string: expanded through
    /// `env` when the entry carries `[$e]`, as it is otherwise.
    pub fn read_string(&self, group: &[&str], key: &str, env: &ExpansionEnv<'_>) -> Option<String> {
        let entry = self.entry(group, key)?;
        let value = entry.value()?;
        Some(if entry.expand {
            env.expand(value)
        } else {
            value.to_owned()
        })
    }

    /// The value read as a boolean: `false`, `no`, `off` and `0` (ignoring
    /// ASCII case) are false and every other value, an empty one included,
    /// is true. `None` when the key is absent.
    pub fn read_bool(&self, group: &[&str], key: &str) -> Option<bool> {
        self.get(group, key).map(parse_bool)
    }

    /// The value read as a base-10 integer, or `default` when the key is
    /// absent or the value does not parse.
    pub fn read_int(&self, group: &[&str], key: &str, default: i64) -> i64 {
        self.get(group, key)
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    }

    /// The value read as a finite real number in the C locale, or `default`
    /// when the key is absent or the value does not parse (a non-finite
    /// value does not parse).
    pub fn read_real(&self, group: &[&str], key: &str, default: f64) -> f64 {
        self.get(group, key).and_then(parse_real).unwrap_or(default)
    }

    /// The value read as a colour, or `None` when the key is absent or the
    /// value is not a colour (see [`KdeColor::parse`]).
    pub fn read_color(&self, group: &[&str], key: &str) -> Option<KdeColor> {
        self.get(group, key).and_then(KdeColor::parse)
    }

    /// Stores `value` for `key` in `group` as a plain entry (neither
    /// immutable nor expanding), replacing whatever the group held for the
    /// key. The group is created when it does not exist.
    pub(crate) fn set(&mut self, group: &[&str], key: &str, value: impl Into<String>) {
        self.groups
            .entry(owned_path(group))
            .or_default()
            .entries
            .insert(
                key.to_owned(),
                Entry {
                    value: Some(value.into()),
                    immutable: false,
                    expand: false,
                },
            );
    }
}

pub(crate) fn parse_bool(value: &str) -> bool {
    !["false", "no", "off", "0"]
        .iter()
        .any(|f| value.eq_ignore_ascii_case(f))
}

pub(crate) fn parse_real(value: &str) -> Option<f64> {
    value.parse::<f64>().ok().filter(|v| v.is_finite())
}

fn owned_path(group: &[&str]) -> Vec<String> {
    group.iter().map(|s| (*s).to_owned()).collect()
}

fn is_trim_byte(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c)
}

fn trim_bytes(mut s: &[u8]) -> &[u8] {
    while let [first, rest @ ..] = s {
        if is_trim_byte(*first) {
            s = rest;
        } else {
            break;
        }
    }
    while let [rest @ .., last] = s {
        if is_trim_byte(*last) {
            s = rest;
        } else {
            break;
        }
    }
    s
}

/// The state of one file's parse.
#[derive(Default)]
struct Parser {
    config: KConfig,
    diagnostics: Vec<ParseDiagnostic>,
    errors: usize,
    line: usize,
    group: Vec<String>,
    /// Entries of the current group are immutable: its header carried
    /// `[$i]`, or the file became immutable before it.
    group_locked: bool,
}

/// What one line asked for, so the parser can decide whether to stop.
enum Flow {
    Continue,
    Stop,
}

impl Parser {
    fn run(mut self, bytes: &[u8]) -> (KConfig, Vec<ParseDiagnostic>) {
        for (index, raw) in bytes.split(|&b| b == b'\n').enumerate() {
            self.line = index + 1;
            if index == 0 && raw.starts_with(BOM) {
                self.note(DiagnosticKind::ByteOrderMark);
            }
            if raw.len() > MAX_LINE_BYTES {
                self.note(DiagnosticKind::LineTooLong);
                continue;
            }
            if let Flow::Stop = self.line_bytes(trim_bytes(raw)) {
                break;
            }
        }
        (self.config, self.diagnostics)
    }

    /// Records a diagnostic; returns `Stop` once the error limit is hit.
    fn note(&mut self, kind: DiagnosticKind) -> Flow {
        let counted = kind.is_counted();
        self.diagnostics.push(ParseDiagnostic {
            line: self.line,
            kind,
        });
        if counted {
            self.errors += 1;
            if self.errors >= MAX_ERRORS {
                self.diagnostics.push(ParseDiagnostic {
                    line: self.line,
                    kind: DiagnosticKind::TooManyErrors,
                });
                return Flow::Stop;
            }
        }
        Flow::Continue
    }

    fn line_bytes(&mut self, line: &[u8]) -> Flow {
        match line.first() {
            None | Some(b'#') => Flow::Continue,
            Some(b'[') => self.header(line),
            Some(_) => self.entry(line),
        }
    }

    fn header(&mut self, line: &[u8]) -> Flow {
        // Split into segments `[..]`: each starts after a `[`, ends at the
        // next `]`, and another follows only when the byte after `]` is `[`.
        let mut segments: Vec<&[u8]> = Vec::new();
        let mut pos = 0;
        let mut ends_at_line_end = false;
        loop {
            debug_assert_eq!(line[pos], b'[');
            let start = pos + 1;
            let Some(len) = line[start..].iter().position(|&b| b == b']') else {
                return self.note(DiagnosticKind::UnclosedGroupHeader);
            };
            segments.push(&line[start..start + len]);
            let next = start + len + 1;
            if next == line.len() {
                ends_at_line_end = true;
                break;
            }
            if line[next] != b'[' {
                break;
            }
            pos = next;
        }

        let mut lock_group = false;
        if ends_at_line_end && segments.last() == Some(&&b"$i"[..]) {
            segments.pop();
            if segments.is_empty() {
                // A bare `[$i]`: the rest of the file is immutable, and the
                // entries before the next header go to the empty-named group.
                self.config.file_immutable = true;
                self.group = vec![String::new()];
                self.group_locked = true;
                return Flow::Continue;
            }
            lock_group = true;
        }

        let mut path = Vec::with_capacity(segments.len());
        for segment in segments {
            let (name, flow) = self.decode(segment);
            if let Flow::Stop = flow {
                return Flow::Stop;
            }
            path.push(name);
        }
        if lock_group {
            self.config
                .groups
                .entry(path.clone())
                .or_default()
                .immutable = true;
        }
        self.group = path;
        self.group_locked = lock_group || self.config.file_immutable;
        Flow::Continue
    }

    fn entry(&mut self, line: &[u8]) -> Flow {
        let (key_part, value_part) = match line.iter().position(|&b| b == b'=') {
            Some(eq) => (trim_bytes(&line[..eq]), Some(trim_bytes(&line[eq + 1..]))),
            None => (line, None),
        };
        if key_part.is_empty() {
            return Flow::Continue;
        }

        // Peel the bracketed suffixes off the key, from the right.
        let mut key = key_part;
        let mut immutable = false;
        let mut expand = false;
        let mut delete = false;
        let mut locale: Option<&[u8]> = None;
        while key.last() == Some(&b']') {
            let Some(open) = key.iter().rposition(|&b| b == b'[') else {
                break;
            };
            let inside = &key[open + 1..key.len() - 1];
            if let Some(options) = inside.strip_prefix(b"$") {
                for &letter in options {
                    match letter {
                        b'i' => immutable = true,
                        b'e' => expand = true,
                        b'd' => delete = true,
                        _ => {}
                    }
                }
            } else if locale.is_some() {
                return self.note(DiagnosticKind::SecondLocale);
            } else {
                locale = Some(inside);
            }
            key = trim_bytes(&key[..open]);
        }
        if let Some(open) = key.iter().rposition(|&b| b == b'[')
            && !key[open..].contains(&b']')
        {
            return self.note(DiagnosticKind::UnclosedKeyBracket);
        }
        if value_part.is_none() && !delete {
            return self.note(DiagnosticKind::MissingEquals);
        }

        let (key, flow) = self.decode(key);
        if let Flow::Stop = flow {
            return Flow::Stop;
        }
        let value = if delete {
            None
        } else {
            let (value, flow) = self.decode(value_part.unwrap_or_default());
            if let Flow::Stop = flow {
                return Flow::Stop;
            }
            Some(value)
        };
        if locale.is_some() || key.is_empty() {
            return Flow::Continue;
        }

        let entry = Entry {
            value,
            immutable: immutable || self.group_locked,
            expand,
        };
        let group = self.config.groups.entry(self.group.clone()).or_default();
        match group.entries.get(&key) {
            Some(held) if held.immutable => {}
            _ => {
                group.entries.insert(key, entry);
            }
        }
        Flow::Continue
    }

    /// Decodes escape sequences in a key, value or group-name segment and
    /// turns the bytes into text.
    fn decode(&mut self, text: &[u8]) -> (String, Flow) {
        if !text.contains(&b'\\') {
            return (String::from_utf8_lossy(text).into_owned(), Flow::Continue);
        }
        let text = trim_bytes(text);
        let mut out = Vec::with_capacity(text.len());
        let mut flow = Flow::Continue;
        let mut i = 0;
        while i < text.len() {
            let b = text[i];
            if b != b'\\' {
                out.push(b);
                i += 1;
                continue;
            }
            let Some(&next) = text.get(i + 1) else {
                out.push(b'\\');
                break;
            };
            i += 2;
            match next {
                b's' => out.push(b' '),
                b't' => out.push(b'\t'),
                b'n' => out.push(b'\n'),
                b'r' => out.push(b'\r'),
                b'\\' => out.push(b'\\'),
                b';' | b',' => out.extend_from_slice(&[b'\\', next]),
                b'x' => {
                    if text.len() - i < 2 {
                        out.push(b'x');
                        break;
                    }
                    let pair = &text[i..i + 2];
                    i += 2;
                    match std::str::from_utf8(pair)
                        .ok()
                        .and_then(|p| u8::from_str_radix(p, 16).ok())
                    {
                        Some(byte) if pair.iter().all(u8::is_ascii_hexdigit) => out.push(byte),
                        _ => out.push(b'x'),
                    }
                }
                other => {
                    out.extend_from_slice(&[b'\\', other]);
                    let shown = char::from_u32(u32::from(other)).unwrap_or('\u{FFFD}');
                    if let Flow::Stop = self.note(DiagnosticKind::InvalidEscape(shown)) {
                        flow = Flow::Stop;
                    }
                }
            }
        }
        (String::from_utf8_lossy(&out).into_owned(), flow)
    }
}

/// Escapes `value` so that [`KConfig::parse`] reads it back unchanged:
/// backslash, line feed, carriage return and tab are escaped, as are a
/// leading or trailing space and every other ASCII control character.
pub(crate) fn escape_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let last = value.chars().count().saturating_sub(1);
    for (i, c) in value.chars().enumerate() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ' ' if i == 0 || i == last => out.push_str("\\s"),
            c if c.is_ascii_control() => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
