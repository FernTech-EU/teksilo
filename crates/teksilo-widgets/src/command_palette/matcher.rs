// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The palette's fuzzy matcher: does a query match a command, how well, and
//! which characters did it match, so the row can show them.
//!
//! Matching is by subsequence, not substring, so `ndw` finds "New Window".
//! The weights encode three preferences, strongest first: typed letters that
//! land consecutively beat the same letters scattered, a letter landing at the
//! start of a word beats one landing mid-word, and an early match beats a late
//! one.
//!
//! The alignment is chosen, not taken greedily. A query has many ways to land
//! in a haystack (`exp` in "File Export": the `e` of "File", or the `E` of
//! "Export"), and taking the first occurrence of each letter scores and
//! highlights the worst of them. The matcher scores every alignment by
//! dynamic programming and keeps the best, so the highlight shows the reading
//! a person meant.

/// Bonus for a matched character that follows the previous match directly.
/// Compounds along a run, so a whole word typed out beats the same letters
/// collected from the starts of several words.
const CONSECUTIVE_BONUS: i32 = 15;
/// Bonus for a matched character that starts a word.
const WORD_START_BONUS: i32 = 20;
/// Penalty per skipped character between two matches, and before the first.
const GAP_PENALTY: i32 = 1;
/// Extra penalty for opening a gap between two matches. Without it, two word
/// starts a few letters apart outscore the same letters typed as a run: `zo`
/// read as the Z of "Zoom" and the O of "Out", not as the start of "Zoom".
const GAP_OPEN_PENALTY: i32 = 5;
/// Ceiling on any one gap's penalty, so a long category name does not bury
/// every command filed under it.
const MAX_GAP_PENALTY: i32 = 25;

/// A successful match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FuzzyMatch {
    /// Higher is better.
    pub score: i32,
    /// The matched characters, as char indices into the haystack, ascending.
    pub positions: Vec<usize>,
}

/// Fold one character's case. One character in, one out, so an index into the
/// folded text is an index into the original: a character whose lowercase
/// form is longer (`İ`) keeps only its first character, which is the one a
/// person types.
fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Whether the character at `i` starts a word: the first character, one after
/// a non-alphanumeric, or an uppercase letter after a lowercase one
/// (`camelCase`).
fn is_word_start(raw: &[char], i: usize) -> bool {
    if i == 0 {
        return true;
    }
    let prev = raw[i - 1];
    let cur = raw[i];
    !prev.is_alphanumeric() || (prev.is_lowercase() && cur.is_uppercase())
}

/// Match `needle` against `haystack`, or `None` when the needle is not a
/// subsequence of it. Case is folded on both sides. An empty needle matches
/// everything with score 0 and no positions.
pub(super) fn fuzzy_match(needle: &str, haystack: &str) -> Option<FuzzyMatch> {
    let needle: Vec<char> = needle.chars().map(fold).collect();
    if needle.is_empty() {
        return Some(FuzzyMatch {
            score: 0,
            positions: Vec::new(),
        });
    }
    let raw: Vec<char> = haystack.chars().collect();
    let hay: Vec<char> = raw.iter().copied().map(fold).collect();
    let (n, m) = (needle.len(), hay.len());
    if n > m {
        return None;
    }

    // `best[i][j]`: the best alignment of `needle[..=i]` with `needle[i]` at
    // `hay[j]`, as (score, run length ending here, previous position).
    #[derive(Clone, Copy)]
    struct Cell {
        score: i32,
        run: i32,
        prev: usize,
    }
    let mut best: Vec<Vec<Option<Cell>>> = vec![vec![None; m]; n];

    for (j, &c) in hay.iter().enumerate() {
        if c == needle[0] {
            let bonus = if is_word_start(&raw, j) {
                WORD_START_BONUS
            } else {
                0
            };
            best[0][j] = Some(Cell {
                score: bonus - (j as i32 * GAP_PENALTY).min(MAX_GAP_PENALTY),
                run: 0,
                prev: usize::MAX,
            });
        }
    }
    for i in 1..n {
        for j in i..m {
            if hay[j] != needle[i] {
                continue;
            }
            let bonus = if is_word_start(&raw, j) {
                WORD_START_BONUS
            } else {
                0
            };
            let mut here: Option<Cell> = None;
            for k in (i - 1)..j {
                let Some(from) = best[i - 1][k] else {
                    continue;
                };
                let (score, run) = if k + 1 == j {
                    let run = from.run + 1;
                    (from.score + CONSECUTIVE_BONUS * run, run)
                } else {
                    let gap = (j - k - 1) as i32;
                    let penalty = (GAP_OPEN_PENALTY + gap * GAP_PENALTY).min(MAX_GAP_PENALTY);
                    (from.score - penalty, 0)
                };
                let score = score + bonus;
                // Ties go to the longer run: the score is the same now, and
                // the run is what the next letter's bonus compounds on.
                let better = match here {
                    None => true,
                    Some(h) => score > h.score || (score == h.score && run > h.run),
                };
                if better {
                    here = Some(Cell {
                        score,
                        run,
                        prev: k,
                    });
                }
            }
            best[i][j] = here;
        }
    }

    let (mut j, last) = best[n - 1]
        .iter()
        .enumerate()
        .filter_map(|(j, c)| c.map(|c| (j, c)))
        .max_by(|(ja, a), (jb, b)| a.score.cmp(&b.score).then(jb.cmp(ja)))?;
    let score = last.score;
    let mut positions = vec![0; n];
    for i in (0..n).rev() {
        positions[i] = j;
        if i > 0 {
            j = best[i][j].expect("a backtracked cell exists").prev;
        }
    }
    Some(FuzzyMatch { score, positions })
}

/// The score alone, for ranking.
#[cfg(test)]
pub(super) fn fuzzy_score(needle: &str, haystack: &str) -> Option<i32> {
    fuzzy_match(needle, haystack).map(|m| m.score)
}

/// Inline markup for `text` with the characters at `positions` (char
/// indices into `text`) in bold, for a `TextWidget` with markup on.
///
/// Every character the markup grammar reads is escaped. A `*` or `\` is never
/// put inside a bold run, because an escaped `*` next to the closing `**`
/// would be read as part of the marker.
pub(super) fn highlighted_markup(text: &str, positions: &[usize]) -> String {
    let mut out = String::with_capacity(text.len() + positions.len() * 4);
    let mut bold = false;
    let mut next = positions.iter().copied().peekable();
    for (i, c) in text.chars().enumerate() {
        let matched = next.peek() == Some(&i);
        if matched {
            next.next();
        }
        let want_bold = matched && !matches!(c, '*' | '\\');
        if want_bold != bold {
            out.push_str("**");
            bold = want_bold;
        }
        if matches!(c, '[' | ']' | '(' | ')' | '*' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    if bold {
        out.push_str("**");
    }
    out
}
