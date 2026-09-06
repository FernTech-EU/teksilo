// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The one place Teksilo turns laid-out text into AccessKit text runs.
//!
//! A `Role::TextRun` child is what makes a node reviewable by character,
//! word and line: `accesskit_consumer::Node::supports_text_ranges` is
//! `(is_text_input || role ∈ {Label, Document, Terminal}) && has runs`,
//! and every platform's text API — `AXBoundsForRange`, UIA's
//! `TextPattern`, AT-SPI's `Text` interface — reads through it. Getting
//! the runs *right* is a long list of small contracts, several of which
//! panic the consumer when broken, and before this module every text
//! widget had its own partial copy of that list.
//!
//! What the emitter guarantees, so no caller has to:
//!
//! - `value` is never `None`, and the sum of `character_lengths` is
//!   exactly its byte length. AccessKit panics otherwise.
//! - No run exceeds [`MAX_RUN_CHARS`] characters. The consumer probes
//!   `character_index as u8`, so a 256-character run aliases its end
//!   onto index 0 and word navigation walks backwards.
//! - A hard break is **one** character at the end of its line's last
//!   run — a `\r\n` being one character whose `character_lengths` entry
//!   is 2 — and no run is break-only.
//! - Every run carries `bounds`, `text_direction`, `character_positions`
//!   and `character_widths`, real when measured and degenerate when not.
//!   `Range::bounding_boxes()` discards the boxes it has already
//!   collected the moment one run lacks any of the four, so a single
//!   geometry-less run empties the geometry of every range that touches
//!   it — a magnifier stops tracking the whole label, not part of it.
//! - Runs sharing a visual line are linked through `next_on_line` /
//!   `previous_on_line`, symmetrically, and never dangle.
//! - Ids are derived from (byte offset, byte length, caller seed) and
//!   deduplicated, because two runs sharing a `NodeId` panic
//!   `accesskit_consumer`'s tree builder.
//!
//! Word boundaries come from UAX #29 rather than from whitespace: a
//! reader stepping by word through "l'anticonstitutionnellement, dit-il"
//! should stop where the language stops, and the apostrophe and hyphen
//! rules are not something a `split_whitespace` can approximate.

use std::borrow::Cow;
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use accesskit::{NodeId, TextDirection};
use teksilo_canvas::{CharGeom, LineEnd, LineTruncation, Point, Rect, TextGeometry};
use unicode_segmentation::UnicodeSegmentation;

use super::{AccessNodeBuilder, TextRunAttributes, TextRunSpec, fnv_mix_u64};

/// The most characters one `Role::TextRun` may carry.
///
/// `accesskit_consumer` stores a position's character index in a `u8`
/// when it probes `word_starts` (`text.rs:396`, `:443`), and
/// `word_starts` is itself a `Vec<u8>`. 255 is therefore the largest
/// index a run can express — and, because a position may sit at the very
/// end of a run, the largest *length* it can have.
pub const MAX_RUN_CHARS: usize = 255;

/// One direction-uniform stretch of a source line, with the geometry the
/// layout measured for it.
#[derive(Debug, Clone)]
pub struct SourceSegment {
    /// Byte range within [`TextRunSource::text`].
    pub byte_range: Range<usize>,
    /// Bounding box of the whole segment.
    pub rect: Rect,
    pub direction: TextDirection,
    /// One entry per character of `byte_range`, in logical order, with
    /// `position` measured from the segment's leading edge.
    pub characters: Vec<CharGeom>,
}

/// One visual line of a source.
#[derive(Debug, Clone)]
pub struct SourceLine {
    /// Byte range within [`TextRunSource::text`], including any trailing
    /// hard break.
    pub byte_range: Range<usize>,
    /// The line box. Used for the y and height of every run on the line,
    /// and as the anchor for text the layout did not measure.
    pub rect: Rect,
    /// Where a caret sits on a line with no characters of its own.
    pub caret_x: f32,
    pub end: LineEnd,
    /// Measured stretches, in logical order. May cover only part of the
    /// line — an ellipsized tail is not measured — and the emitter fills
    /// what is missing with degenerate geometry rather than leaving it
    /// out.
    pub segments: Vec<SourceSegment>,
    /// Where the layout drew the ellipsis, when it cut the line short.
    pub truncation: Option<LineTruncation>,
}

/// A laid-out text, ready to be emitted as AccessKit runs.
#[derive(Debug, Clone)]
pub struct TextRunSource<'a> {
    /// The text every byte range indexes. On a markup layout this is the
    /// *rendered* text — the markup with its syntax stripped — because
    /// that is what the reader hears and what the ranges refer to.
    pub text: Cow<'a, str>,
    pub lines: Vec<SourceLine>,
    /// Formatting announced on every emitted run.
    pub attrs: TextRunAttributes,
    /// Distinguishes two sources with identical text inside one builder —
    /// an editor's block id, a terminal's row index, a scene item's id.
    /// A single-source widget passes `0`.
    pub id_seed: u64,
    /// `true` when the rects are already in window space and must not be
    /// translated by the owner's origin (a scene item under a view
    /// transform, a composite emitting geometry from a child it placed).
    pub rects_are_absolute: bool,
    /// The base direction, announced on the owning node. Falls back to
    /// the first line's first segment when the caller does not set it.
    pub base_direction: TextDirection,
}

/// A retained layout one widget can lend to another widget's
/// accessibility pass.
///
/// A control that owns its accessible name but paints its text through a
/// hidden child — `Badge`, `GroupHeader` — has to emit runs for text it
/// did not lay out. The child publishes what it measured through a
/// handle; the parent reads it during its own `accessibility()`.
#[derive(Debug, Clone)]
pub struct RetainedText {
    /// The text as painted, after markup rendering.
    pub text: String,
    /// What the backend measured, when it measured anything.
    pub geometry: Option<Rc<TextGeometry>>,
    /// Window-space box of the text region. Its origin is the origin of
    /// the coordinate space `geometry` reports in.
    pub bounds: Rect,
    /// Base reading direction resolved at layout time.
    pub base_direction: TextDirection,
}

/// Shared handle onto a widget's retained layout. Empty until the widget
/// has been placed.
pub type TextGeometryHandle = Rc<RefCell<Option<RetainedText>>>;

/// One run the emitter produced.
#[derive(Debug, Clone)]
pub struct EmittedRun {
    pub id: NodeId,
    /// Byte range of the run's own text within the source, **excluding**
    /// any hard break it carries.
    ///
    /// Slicing the source with it can therefore never yield the line
    /// separator, which is what a caret clamped to a run's text needs.
    pub byte_range: Range<usize>,
    /// Character range within the source, **including** the hard break —
    /// which AccessKit counts as one character however many bytes it takes.
    ///
    /// Deliberately not the same basis as
    /// [`byte_range`](Self::byte_range), and the two must not be read
    /// through one another. This is the range [`position_of`] indexes, and
    /// it has to count the break so a consumer's character space matches
    /// the document's across a line boundary.
    ///
    /// [`position_of`]: TextRunEmission::position_of
    pub char_range: Range<usize>,
}

/// What one `push_text_runs` call produced.
#[derive(Debug, Clone, Default)]
pub struct TextRunEmission {
    pub runs: Vec<EmittedRun>,
    /// The literal concatenation of every run's text.
    ///
    /// The owning node's `value` must be byte-identical to this:
    /// `accesskit_consumer` derives a node's document text from its runs
    /// and every divergence is a place where what a reader reviews and
    /// what it announces disagree.
    pub value: String,
}

impl TextRunEmission {
    /// Locate a character of the source within the emitted runs, as the
    /// `(node, character index)` pair AccessKit's `TextPosition` wants.
    ///
    /// A caret sitting exactly on a chunk boundary belongs to the
    /// *earlier* run, at its end — the position a reader arrived at by
    /// moving forward through that run.
    pub fn position_of(&self, char_index: usize) -> Option<(NodeId, usize)> {
        let run = self
            .runs
            .iter()
            .find(|r| char_index <= r.char_range.end && char_index >= r.char_range.start)?;
        Some((run.id, char_index - run.char_range.start))
    }
}

impl<'a> TextRunSource<'a> {
    /// A source with no measured geometry: one line covering the whole
    /// text, reported with degenerate boxes at the owner's leading edge.
    ///
    /// Not a fallback to be avoided — it is the correct answer for text
    /// that was painted without a measuring backend, and it keeps
    /// `bounding_boxes()` non-empty, which is what a magnifier needs in
    /// order to keep tracking rather than to stop.
    pub fn flat(text: &'a str, id_seed: u64) -> Self {
        Self {
            lines: vec![SourceLine {
                byte_range: 0..text.len(),
                rect: Rect::new(0.0, 0.0, 0.0, 0.0),
                caret_x: 0.0,
                end: LineEnd::EndOfText,
                segments: Vec::new(),
                truncation: None,
            }],
            text: Cow::Borrowed(text),
            attrs: TextRunAttributes::default(),
            id_seed,
            rects_are_absolute: false,
            base_direction: TextDirection::LeftToRight,
        }
    }

    /// A source built from what the text backend measured.
    ///
    /// `origin` offsets every rect into the space the caller will report
    /// — the text region's offset inside the widget for local rects, the
    /// widget's window-space origin for absolute ones.
    pub fn from_geometry(
        text: &'a str,
        geometry: &TextGeometry,
        origin: Point,
        id_seed: u64,
    ) -> Self {
        let text: Cow<'a, str> = match &geometry.rendered_text {
            // A markup layout indexes the rendered text, not the source
            // the caller holds.
            Some(rendered) => Cow::Owned(rendered.clone()),
            None => Cow::Borrowed(text),
        };
        let mut lines: Vec<SourceLine> = geometry
            .lines
            .iter()
            .map(|line| SourceLine {
                byte_range: line.byte_range.clone(),
                rect: offset_rect(line.rect, origin),
                caret_x: line.caret_x + origin.x,
                end: line.end,
                segments: line
                    .segments
                    .iter()
                    .map(|segment| SourceSegment {
                        byte_range: segment.byte_range.clone(),
                        rect: offset_rect(segment.rect, origin),
                        direction: to_accesskit_direction(segment.direction),
                        characters: segment.characters.clone(),
                    })
                    .collect(),
                truncation: line.truncation.map(|t| LineTruncation {
                    ellipsis_x: t.ellipsis_x + origin.x,
                    ellipsis_width: t.ellipsis_width,
                }),
            })
            .collect();
        // A `max_lines` cap stops the layout short, and the reader must
        // still hear the whole label. Anything the emitted lines do not
        // reach becomes one unmeasured line below the last, so it is
        // announced and reviewable even though it was never drawn.
        let covered = lines.last().map(|l| l.byte_range.end).unwrap_or(0);
        if covered < text.len() {
            let anchor = lines
                .last()
                .map(|l| Rect::new(l.rect.x, l.rect.y + l.rect.height, 0.0, l.rect.height))
                .unwrap_or_else(|| Rect::new(origin.x, origin.y, 0.0, 0.0));
            lines.push(SourceLine {
                byte_range: covered..text.len(),
                rect: anchor,
                caret_x: anchor.x,
                end: LineEnd::EndOfText,
                segments: Vec::new(),
                truncation: None,
            });
        }
        let base_direction = geometry
            .lines
            .first()
            .and_then(|l| l.segments.first())
            .map(|s| to_accesskit_direction(s.direction))
            .unwrap_or(TextDirection::LeftToRight);
        Self {
            text,
            lines,
            attrs: TextRunAttributes::default(),
            id_seed,
            rects_are_absolute: false,
            base_direction,
        }
    }

    /// A source built from another widget's retained layout, with
    /// **absolute** rects: the donor measured in its own place, and its
    /// origin is already baked in.
    pub fn from_handle(
        handle: &TextGeometryHandle,
        id_seed: u64,
    ) -> Option<TextRunSource<'static>> {
        let borrowed = handle.borrow();
        let retained = borrowed.as_ref()?;
        let mut source = match retained.geometry.as_deref() {
            Some(geometry) => {
                let owned = TextRunSource::from_geometry(
                    &retained.text,
                    geometry,
                    retained.bounds.origin(),
                    id_seed,
                );
                TextRunSource {
                    text: Cow::Owned(owned.text.into_owned()),
                    lines: owned.lines,
                    attrs: owned.attrs,
                    id_seed,
                    rects_are_absolute: true,
                    base_direction: owned.base_direction,
                }
            }
            None => {
                let mut flat = TextRunSource::flat("", id_seed);
                flat.text = Cow::Owned(retained.text.clone());
                flat.lines[0].byte_range = 0..retained.text.len();
                flat.lines[0].rect = retained.bounds;
                flat.lines[0].caret_x = retained.bounds.x;
                flat.rects_are_absolute = true;
                flat
            }
        };
        source.base_direction = retained.base_direction;
        Some(source)
    }

    /// Announce this formatting on every emitted run.
    pub fn with_attrs(mut self, attrs: TextRunAttributes) -> Self {
        self.attrs = attrs;
        self
    }

    /// Treat the rects as already being in window space.
    pub fn with_absolute_rects(mut self) -> Self {
        self.rects_are_absolute = true;
        self
    }

    /// Anchor an unmeasured source at a known box, so its degenerate
    /// runs sit at the owner's leading edge rather than at its origin.
    pub fn with_fallback_rect(mut self, rect: Rect) -> Self {
        for line in &mut self.lines {
            if line.segments.is_empty() {
                line.rect = rect;
                line.caret_x = rect.x;
            }
        }
        self
    }

    /// Declare the base reading direction explicitly, overriding the one
    /// inferred from the first measured segment.
    pub fn with_base_direction(mut self, direction: TextDirection) -> Self {
        self.base_direction = direction;
        self
    }
}

fn offset_rect(rect: [f32; 4], origin: Point) -> Rect {
    Rect::new(rect[0] + origin.x, rect[1] + origin.y, rect[2], rect[3])
}

fn to_accesskit_direction(direction: teksilo_canvas::TextDirection) -> TextDirection {
    match direction {
        teksilo_canvas::TextDirection::LeftToRight => TextDirection::LeftToRight,
        teksilo_canvas::TextDirection::RightToLeft => TextDirection::RightToLeft,
    }
}

/// The character index of every word start in `text`, per UAX #29.
///
/// Index 0 is always a start when the text is non-empty: a line whose
/// first character is a space begins with a run of whitespace, and a
/// reader moving backwards to the line's first word must have somewhere
/// to land. Every other start is the beginning of a non-whitespace
/// segment, which puts a word's *trailing* whitespace inside that word —
/// the convention every platform's word navigation uses.
pub fn line_word_starts(text: &str) -> Vec<usize> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut starts = vec![0usize];
    let mut chars_before = 0usize;
    for (_, segment) in text.split_word_bound_indices() {
        if chars_before > 0 && !segment.chars().all(char::is_whitespace) {
            starts.push(chars_before);
        }
        chars_before += segment.chars().count();
    }
    starts.dedup();
    starts
}

/// Narrow whole-line word starts onto one chunk.
///
/// The line is segmented once and sliced here, never re-segmented per
/// chunk: a chunk boundary falls at 255 characters, which is a place the
/// text has no opinion about, and re-running UAX #29 on the fragment
/// would invent a word start there and lose the one that spans it.
pub fn rebase_word_starts(starts: &[usize], chunk: Range<usize>) -> Vec<u8> {
    starts
        .iter()
        .filter(|&&s| s >= chunk.start && s < chunk.end)
        .map(|&s| (s - chunk.start) as u8)
        .collect()
}

/// Geometry for one piece of a line: what the layout measured, or where
/// to anchor what it did not.
#[derive(Debug, Clone)]
enum PieceGeometry {
    Measured {
        rect: Rect,
        direction: TextDirection,
        characters: Vec<CharGeom>,
    },
    /// Text that was painted but not measured — an ellipsized tail, a
    /// label drawn without a backend. A zero-width box at `x` keeps the
    /// run's geometry present, which is what stops one such run from
    /// emptying the boxes of every range around it.
    Unmeasured { x: f32, direction: TextDirection },
}

#[derive(Debug, Clone)]
struct Piece {
    byte_range: Range<usize>,
    geometry: PieceGeometry,
}

/// Split a line into measured segments and the gaps between them.
///
/// The gaps are what an ellipsis leaves behind: the line still covers its
/// whole source, but only the drawn part was measured. Each gap is
/// anchored where the reader would look for it — at the ellipsis when the
/// layout reported one, otherwise at the edge of the neighbouring text.
fn line_pieces(line: &SourceLine, base_direction: TextDirection) -> Vec<Piece> {
    let mut segments: Vec<&SourceSegment> = line.segments.iter().collect();
    segments.sort_by_key(|s| s.byte_range.start);

    let ellipsis_x = line.truncation.map(|t| t.ellipsis_x);
    let mut pieces = Vec::with_capacity(segments.len() * 2 + 1);
    let mut cursor = line.byte_range.start;

    for segment in &segments {
        if segment.byte_range.start > cursor {
            let x = match pieces.last() {
                // Between two measured stretches: a middle ellipsis.
                Some(Piece {
                    geometry: PieceGeometry::Measured { rect, .. },
                    ..
                }) => ellipsis_x.unwrap_or(rect.x + rect.width),
                _ => ellipsis_x.unwrap_or(line.rect.x),
            };
            pieces.push(Piece {
                byte_range: cursor..segment.byte_range.start,
                geometry: PieceGeometry::Unmeasured {
                    x,
                    direction: base_direction,
                },
            });
        }
        pieces.push(Piece {
            byte_range: segment.byte_range.clone(),
            geometry: PieceGeometry::Measured {
                rect: segment.rect,
                direction: segment.direction,
                characters: segment.characters.clone(),
            },
        });
        cursor = segment.byte_range.end.max(cursor);
    }

    if cursor < line.byte_range.end {
        let x = match pieces.last() {
            Some(Piece {
                geometry: PieceGeometry::Measured { rect, .. },
                ..
            }) => ellipsis_x.unwrap_or(rect.x + rect.width),
            _ => ellipsis_x.unwrap_or(line.rect.x),
        };
        pieces.push(Piece {
            byte_range: cursor..line.byte_range.end,
            geometry: PieceGeometry::Unmeasured {
                x,
                direction: base_direction,
            },
        });
    }

    pieces
}

/// Byte offsets of every character boundary in `slice`, plus its length.
fn char_offsets(slice: &str) -> Vec<usize> {
    let mut offsets: Vec<usize> = slice.char_indices().map(|(i, _)| i).collect();
    offsets.push(slice.len());
    offsets
}

/// Emit the AccessKit text runs for one laid-out source.
///
/// Runs are attached under `parent` when given, and directly under the
/// builder's own node otherwise — the shape a label and a single-line
/// input both want, and the one that keeps text-change events alive
/// (`accesskit_consumer` routes a run's update to its *filtered* parent,
/// and a `Role::Paragraph` in between supports no text ranges, so the
/// event is dropped on all three platforms).
///
/// Emits nothing but the value when the builder cannot own children —
/// see [`AccessNodeBuilder::emits_no_children`]. The value is still
/// computed, because that is what a name probe came for.
pub fn push_text_runs(
    builder: &mut AccessNodeBuilder,
    parent: Option<NodeId>,
    source: &TextRunSource<'_>,
) -> TextRunEmission {
    let text = source.text.as_ref();
    let mut emission = TextRunEmission::default();
    let emit = !builder.emits_no_children();
    let mut char_cursor = 0usize;

    if emit {
        builder.set_text_direction(source.base_direction);
    }

    for line in &source.lines {
        let line_text = text.get(line.byte_range.clone()).unwrap_or("");
        let word_starts = line_word_starts(line_text);
        let line_char_start = char_cursor;

        // The break is one character at the very end of the line, and it
        // rides the last run rather than becoming a run of its own.
        let break_bytes = match line.end {
            LineEnd::HardBreak { bytes, .. } => (bytes as usize).min(line_text.len()),
            _ => 0,
        };
        let body_end = line.byte_range.end - break_bytes;

        let mut pieces = line_pieces(line, source.base_direction);
        for piece in &mut pieces {
            piece.byte_range.end = piece.byte_range.end.min(body_end);
            piece.byte_range.start = piece.byte_range.start.min(piece.byte_range.end);
        }
        pieces.retain(|p| !p.byte_range.is_empty());

        // Chunk every piece. A chunk names the piece it came from rather
        // than borrowing its geometry, so the break-reservation pass below
        // can rewrite the list without fighting the borrow checker.
        let mut chunks: Vec<(Range<usize>, usize, usize)> = Vec::new();
        for (piece_index, piece) in pieces.iter().enumerate() {
            let slice = &text[piece.byte_range.clone()];
            let offsets = char_offsets(slice);
            let count = offsets.len() - 1;
            let mut start = 0usize;
            while start < count {
                let end = (start + MAX_RUN_CHARS).min(count);
                chunks.push((
                    piece.byte_range.start + offsets[start]..piece.byte_range.start + offsets[end],
                    piece_index,
                    start,
                ));
                start = end;
            }
        }

        if break_bytes > 0 {
            // Reserve the break's slot. A last chunk already at the cap
            // must give up one character rather than overflow, and no run
            // may consist of the break alone.
            let needs_split = chunks
                .last()
                .map(|(range, _, _)| text[range.clone()].chars().count() >= MAX_RUN_CHARS)
                .unwrap_or(false);
            if needs_split && let Some((range, piece_index, offset_in_piece)) = chunks.pop() {
                let offsets = char_offsets(&text[range.clone()]);
                let last = offsets.len() - 2;
                let split = range.start + offsets[last];
                chunks.push((range.start..split, piece_index, offset_in_piece));
                chunks.push((split..range.end, piece_index, offset_in_piece + last));
            }
            if chunks.is_empty() {
                // A line with no body that still ends in a break: one run
                // carrying nothing but that break, anchored at the caret.
                pieces.push(Piece {
                    byte_range: body_end..body_end,
                    geometry: PieceGeometry::Unmeasured {
                        x: line.caret_x,
                        direction: source.base_direction,
                    },
                });
                chunks.push((body_end..body_end, pieces.len() - 1, 0));
            }
        }

        let mut line_run_ids: Vec<NodeId> = Vec::new();
        let last_chunk = chunks.len().saturating_sub(1);

        if chunks.is_empty() {
            // A line with no characters at all — an empty label, a blank
            // paragraph. It still gets exactly one run: a node with no
            // runs supports no text ranges, and a change event needs the
            // *old* node to support them too.
            let id = emit_run(
                builder,
                parent,
                source,
                line,
                EmptyRun {
                    byte_start: line.byte_range.start,
                    char_start: line_char_start,
                    value: String::new(),
                    direction: source.base_direction,
                    x: line.caret_x,
                },
                emit,
                &mut emission,
            );
            if let Some(id) = id {
                line_run_ids.push(id);
            }
            continue;
        }

        for (index, (range, piece_index, offset_in_piece)) in chunks.iter().enumerate() {
            let geometry = &pieces[*piece_index].geometry;
            let slice = &text[range.clone()];
            let char_count = slice.chars().count();
            let carries_break = break_bytes > 0 && index == last_chunk;

            let mut value = String::with_capacity(slice.len() + break_bytes);
            value.push_str(slice);
            let mut character_lengths: Vec<u8> =
                slice.chars().map(|c| c.len_utf8() as u8).collect();
            if carries_break {
                let break_slice = &text[body_end..body_end + break_bytes];
                value.push_str(break_slice);
                // AccessKit counts a `\r\n` as one character of length 2.
                character_lengths.push(break_bytes as u8);
            }

            let chunk_char_start = char_cursor - line_char_start;
            let chunk_chars = character_lengths.len();
            let word_starts = rebase_word_starts(
                &word_starts,
                chunk_char_start..chunk_char_start + chunk_chars,
            );

            let (positions, widths, rect, direction) =
                chunk_geometry(geometry, *offset_in_piece, char_count, carries_break, line);

            let element_id = fnv_mix_u64(range.start as u64, range.len() as u64, source.id_seed);
            emission.value.push_str(&value);
            let char_range = char_cursor..char_cursor + chunk_chars;
            char_cursor += chunk_chars;

            if !emit {
                continue;
            }
            let id = builder.push_text_run(
                parent,
                TextRunSpec {
                    element_id,
                    value,
                    character_lengths,
                    word_starts,
                    character_positions: positions,
                    character_widths: widths,
                    bounds: rect,
                    bounds_are_absolute: source.rects_are_absolute,
                    text_direction: direction,
                    attrs: source.attrs,
                },
            );
            if let Some(id) = id {
                emission.runs.push(EmittedRun {
                    id,
                    byte_range: range.clone(),
                    char_range,
                });
                line_run_ids.push(id);
            }
        }

        if emit {
            builder.link_runs_on_line(&line_run_ids);
        }
    }

    emission
}

/// The pieces of a run that carries no characters.
struct EmptyRun {
    byte_start: usize,
    char_start: usize,
    value: String,
    direction: TextDirection,
    x: f32,
}

fn emit_run(
    builder: &mut AccessNodeBuilder,
    parent: Option<NodeId>,
    source: &TextRunSource<'_>,
    line: &SourceLine,
    run: EmptyRun,
    emit: bool,
    emission: &mut TextRunEmission,
) -> Option<NodeId> {
    emission.value.push_str(&run.value);
    if !emit {
        return None;
    }
    let element_id = fnv_mix_u64(run.byte_start as u64, 0, source.id_seed);
    let id = builder.push_text_run(
        parent,
        TextRunSpec {
            element_id,
            value: run.value,
            character_lengths: Vec::new(),
            word_starts: Vec::new(),
            // An empty run with bounds, a direction and an empty
            // positions vector is how `accesskit_consumer` reports a
            // caret rect for a blank line (`text.rs:774`).
            character_positions: Vec::new(),
            character_widths: Vec::new(),
            bounds: Rect::new(run.x, line.rect.y, 0.0, line.rect.height),
            bounds_are_absolute: source.rects_are_absolute,
            text_direction: run.direction,
            attrs: source.attrs,
        },
    )?;
    emission.runs.push(EmittedRun {
        id,
        byte_range: run.byte_start..run.byte_start,
        char_range: run.char_start..run.char_start,
    });
    Some(id)
}

/// Per-character geometry and bounding box for one chunk.
///
/// Positions are rebased to zero at the chunk's first character, because
/// AccessKit measures them from the run's own leading edge. The box is
/// derived from the first and last character rather than from the
/// segment, so a chunk is as wide as its own text.
fn chunk_geometry(
    geometry: &PieceGeometry,
    offset_in_piece: usize,
    char_count: usize,
    carries_break: bool,
    line: &SourceLine,
) -> (Vec<f32>, Vec<f32>, Rect, TextDirection) {
    match geometry {
        PieceGeometry::Measured {
            rect,
            direction,
            characters,
        } => {
            let slice: Vec<CharGeom> = characters
                .iter()
                .skip(offset_in_piece)
                .take(char_count)
                .copied()
                .collect();
            if slice.len() != char_count {
                // The layout measured fewer characters than the text
                // holds. Report the whole chunk as unmeasured rather
                // than a partial vector: a positions array shorter than
                // `character_lengths` panics the consumer's indexing.
                return degenerate(rect.x, char_count, carries_break, line, *direction);
            }
            let base = slice.first().map(|c| c.position).unwrap_or(0.0);
            let mut positions: Vec<f32> = slice.iter().map(|c| c.position - base).collect();
            let mut widths: Vec<f32> = slice.iter().map(|c| c.width).collect();
            let extent = positions
                .last()
                .zip(widths.last())
                .map(|(p, w)| p + w)
                .unwrap_or(0.0);
            if carries_break {
                // The break takes no space; it sits where the text ended.
                positions.push(extent);
                widths.push(0.0);
            }
            let leading = base;
            let bounds = match direction {
                TextDirection::RightToLeft => {
                    // Positions run from the segment's right edge.
                    let right = rect.x + rect.width - leading;
                    Rect::new(right - extent, rect.y, extent, rect.height)
                }
                _ => Rect::new(rect.x + leading, rect.y, extent, rect.height),
            };
            (positions, widths, bounds, *direction)
        }
        PieceGeometry::Unmeasured { x, direction } => {
            degenerate(*x, char_count, carries_break, line, *direction)
        }
    }
}

fn degenerate(
    x: f32,
    char_count: usize,
    carries_break: bool,
    line: &SourceLine,
    direction: TextDirection,
) -> (Vec<f32>, Vec<f32>, Rect, TextDirection) {
    let entries = char_count + usize::from(carries_break);
    (
        vec![0.0; entries],
        vec![0.0; entries],
        Rect::new(x, line.rect.y, 0.0, line.rect.height),
        direction,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accessibility::AccessNodeBuilder;
    use accesskit::Role;
    use slotmap::KeyData;
    use teksilo_canvas::{TextDirection as CanvasDirection, TextLine, TextLineSegment};

    fn owner() -> crate::widget_id::WidgetId {
        crate::widget_id::WidgetId::from(KeyData::from_ffi(1 << 32 | 7))
    }

    /// A builder in the state the tree walker hands to a label: a role
    /// that supports text ranges, so `build` keeps the runs.
    fn label_builder() -> AccessNodeBuilder {
        let mut builder = AccessNodeBuilder::for_widget(owner());
        builder.set_role(Role::Label);
        builder
    }

    /// One measured stretch, with a `CharGeom` per `(position, width)`.
    fn geom_segment(
        bytes: Range<usize>,
        chars: Range<usize>,
        rect: [f32; 4],
        direction: CanvasDirection,
        glyphs: &[(f32, f32)],
    ) -> TextLineSegment {
        TextLineSegment {
            byte_range: bytes,
            char_range: chars,
            direction,
            rect,
            characters: glyphs
                .iter()
                .map(|&(position, width)| CharGeom { position, width })
                .collect(),
        }
    }

    fn geom_line(
        index: usize,
        bytes: Range<usize>,
        chars: Range<usize>,
        rect: [f32; 4],
        end: LineEnd,
        segments: Vec<TextLineSegment>,
    ) -> TextLine {
        TextLine {
            index,
            byte_range: bytes,
            char_range: chars,
            rect,
            baseline: rect[1] + rect[3] * 0.75,
            caret_x: rect[0],
            segments,
            end,
            truncation: None,
        }
    }

    fn geometry_of(text: &str, lines: Vec<TextLine>) -> TextGeometry {
        TextGeometry {
            lines,
            dropped_lines: 0,
            source_len: text.len(),
            rendered_text: None,
            links: Vec::new(),
        }
    }

    /// A source whose line breaking is known but whose glyphs were never
    /// measured — what a backend without per-character geometry, or a
    /// widget painting through `Canvas::draw_text`, produces.
    fn unmeasured<'a>(
        text: &'a str,
        lines: &[(Range<usize>, LineEnd)],
        seed: u64,
    ) -> TextRunSource<'a> {
        let mut source = TextRunSource::flat(text, seed);
        source.lines = lines
            .iter()
            .enumerate()
            .map(|(index, (bytes, end))| SourceLine {
                byte_range: bytes.clone(),
                rect: Rect::new(0.0, index as f32 * 16.0, 0.0, 16.0),
                caret_x: 0.0,
                end: *end,
                segments: Vec::new(),
                truncation: None,
            })
            .collect();
        source
    }

    /// The ids of the runs one source produces, in emission order.
    fn run_ids(source: &TextRunSource<'_>) -> Vec<NodeId> {
        let mut builder = label_builder();
        push_text_runs(&mut builder, None, source)
            .runs
            .iter()
            .map(|run| run.id)
            .collect()
    }

    #[test]
    fn a_plain_label_emits_one_run_whose_value_is_the_text() {
        let mut b = label_builder();
        let source = TextRunSource::flat("Hello", 0);
        let emission = push_text_runs(&mut b, None, &source);
        assert_eq!(emission.value, "Hello");
        assert_eq!(emission.runs.len(), 1);
        let (_, _, children, _) = b.build(owner());
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].1.value(), Some("Hello"));
        assert_eq!(children[0].1.character_lengths(), &[1, 1, 1, 1, 1]);
    }

    #[test]
    fn empty_text_emits_one_run_with_a_caret_rect() {
        // A node with no runs supports no text ranges at all, so an empty
        // label that emitted nothing would stop answering
        // `AXBoundsForRange` — and a text-change event needs the *old*
        // node to support ranges too, or the consumer drops it.
        let mut b = label_builder();
        let source = TextRunSource::flat("", 0).with_fallback_rect(Rect::new(4.0, 2.0, 60.0, 16.0));
        let emission = push_text_runs(&mut b, None, &source);
        assert_eq!(emission.value, "");
        assert_eq!(emission.runs.len(), 1);

        let (_, _, children, local) = b.build(owner());
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].1.value(), Some(""));
        assert!(children[0].1.character_lengths().is_empty());
        // `accesskit_consumer` reports a blank line's caret from an empty
        // run's bounds, so the box must be the caret: zero-width, full
        // line height, at the leading edge.
        assert_eq!(local[0].1, Rect::new(4.0, 2.0, 0.0, 16.0));
    }

    #[test]
    fn single_line_run_carries_positions_widths_bounds_and_direction() {
        // `Range::bounding_boxes()` discards everything it has collected
        // on the first run missing any of the four, so a partially
        // populated run empties the geometry of every range around it.
        let text = "Hi";
        let geometry = geometry_of(
            text,
            vec![geom_line(
                0,
                0..2,
                0..2,
                [0.0, 0.0, 16.0, 16.0],
                LineEnd::EndOfText,
                vec![geom_segment(
                    0..2,
                    0..2,
                    [0.0, 0.0, 16.0, 16.0],
                    CanvasDirection::LeftToRight,
                    &[(0.0, 8.0), (8.0, 8.0)],
                )],
            )],
        );
        let source = TextRunSource::from_geometry(text, &geometry, Point::new(3.0, 5.0), 0);

        let mut b = label_builder();
        let emission = push_text_runs(&mut b, None, &source);
        assert_eq!(emission.runs.len(), 1);

        let (_, root, children, local) = b.build(owner());
        let run = &children[0].1;
        assert_eq!(run.character_positions(), Some(&[0.0f32, 8.0][..]));
        assert_eq!(run.character_widths(), Some(&[8.0f32, 8.0][..]));
        assert_eq!(run.text_direction(), Some(TextDirection::LeftToRight));
        assert_eq!(run.word_starts(), &[0]);
        assert_eq!(local[0].1, Rect::new(3.0, 5.0, 16.0, 16.0));
        // The owning node answers the direction too, for a consumer that
        // asks the container rather than a run.
        assert_eq!(root.text_direction(), Some(TextDirection::LeftToRight));
    }

    #[test]
    fn a_256_char_line_splits_into_two_linked_runs() {
        // `accesskit_consumer` probes a position's character index as a
        // `u8` (`text.rs:396`, `:443`), so index 256 aliases onto 0 and
        // word navigation walks backwards through the run.
        let text = "a".repeat(256);
        let mut b = label_builder();
        let emission = push_text_runs(&mut b, None, &TextRunSource::flat(&text, 0));
        assert_eq!(emission.runs.len(), 2);

        let (_, _, children, _) = b.build(owner());
        assert_eq!(children[0].1.character_lengths().len(), MAX_RUN_CHARS);
        assert_eq!(children[1].1.character_lengths().len(), 1);
        // A split line is still one visual line to a reader navigating by
        // line, so the chain has to be symmetric and terminate.
        assert_eq!(children[0].1.next_on_line(), Some(children[1].0));
        assert_eq!(children[1].1.previous_on_line(), Some(children[0].0));
        assert!(children[0].1.previous_on_line().is_none());
        assert!(children[1].1.next_on_line().is_none());
    }

    #[test]
    fn a_254_char_line_with_crlf_is_one_run() {
        // The break is one character, so 254 characters plus it is 255 —
        // exactly the cap, and no split is warranted.
        let text = format!("{}\r\n", "a".repeat(254));
        let source = unmeasured(
            &text,
            &[(0..text.len(), LineEnd::HardBreak { chars: 1, bytes: 2 })],
            0,
        );

        let mut b = label_builder();
        let emission = push_text_runs(&mut b, None, &source);
        assert_eq!(emission.runs.len(), 1);

        let (_, _, children, _) = b.build(owner());
        let lengths = children[0].1.character_lengths();
        assert_eq!(lengths.len(), MAX_RUN_CHARS);
        assert_eq!(*lengths.last().unwrap(), 2);
        let value = children[0].1.value().unwrap();
        assert!(value.ends_with("\r\n"));
        // AccessKit panics when the lengths do not sum to the value's
        // byte length.
        assert_eq!(
            lengths.iter().map(|n| *n as usize).sum::<usize>(),
            value.len()
        );
    }

    #[test]
    fn a_255_char_line_with_lf_is_two_runs_and_no_run_is_break_only() {
        // The break needs a slot inside a run that is already at the cap,
        // so the last chunk gives up a character rather than overflowing
        // — and the break must not end up alone, which would make a run
        // whose only character is a newline.
        let text = format!("{}\n", "a".repeat(255));
        let source = unmeasured(
            &text,
            &[(0..text.len(), LineEnd::HardBreak { chars: 1, bytes: 1 })],
            0,
        );

        let mut b = label_builder();
        let emission = push_text_runs(&mut b, None, &source);
        assert_eq!(emission.runs.len(), 2);

        let (_, _, children, _) = b.build(owner());
        assert_eq!(children[0].1.character_lengths().len(), 254);
        assert_eq!(children[1].1.value(), Some("a\n"));
        assert_eq!(children[1].1.character_lengths(), &[1, 1]);
        for (_, run) in &children {
            assert_ne!(run.value(), Some("\n"), "a run carries only the break");
        }
        assert_eq!(emission.value, text);
    }

    #[test]
    fn a_300_char_line_splits_255_and_45() {
        let text = "a".repeat(300);
        let mut b = label_builder();
        let emission = push_text_runs(&mut b, None, &TextRunSource::flat(&text, 0));
        assert_eq!(emission.runs.len(), 2);
        assert_eq!(emission.runs[0].char_range, 0..255);
        assert_eq!(emission.runs[1].char_range, 255..300);
        assert_eq!(emission.value, text);
    }

    #[test]
    fn word_starts_are_rebased_per_chunk_and_never_resegmented() {
        // The cap falls at 255 characters, a place the text has no
        // opinion about. Re-running UAX #29 on the fragment would invent
        // a word start there — a reader moving back by word would stop
        // mid-word — and lose the one that spans the boundary.
        let text = format!("{} tail", "a".repeat(260));
        let mut b = label_builder();
        let emission = push_text_runs(&mut b, None, &TextRunSource::flat(&text, 0));
        assert_eq!(emission.runs.len(), 2);

        let (_, _, children, _) = b.build(owner());
        assert_eq!(children[0].1.word_starts(), &[0]);
        // "tail" starts at character 261 of the line, hence 6 of the
        // second chunk. A per-chunk segmentation would report [0, 6].
        assert_eq!(children[1].1.word_starts(), &[6]);
    }

    #[test]
    fn a_continuation_line_starting_with_space_lists_index_zero() {
        // A reader moving backwards to the start of a wrapped line has to
        // land somewhere, so index 0 is a word start even when the line
        // opens on whitespace.
        let text = "abc def";
        let source = unmeasured(
            text,
            &[(0..3, LineEnd::SoftWrap), (3..7, LineEnd::EndOfText)],
            0,
        );

        let mut b = label_builder();
        push_text_runs(&mut b, None, &source);
        let (_, _, children, _) = b.build(owner());
        assert_eq!(children[1].1.value(), Some(" def"));
        assert_eq!(children[1].1.word_starts(), &[0, 1]);
    }

    #[test]
    fn hard_break_lf_is_one_character() {
        let text = "ab\ncd";
        let source = unmeasured(
            text,
            &[
                (0..3, LineEnd::HardBreak { chars: 1, bytes: 1 }),
                (3..5, LineEnd::EndOfText),
            ],
            0,
        );

        let mut b = label_builder();
        let emission = push_text_runs(&mut b, None, &source);
        assert_eq!(emission.value, text);

        let (_, _, children, _) = b.build(owner());
        assert_eq!(children[0].1.value(), Some("ab\n"));
        assert_eq!(children[0].1.character_lengths(), &[1, 1, 1]);
    }

    #[test]
    fn hard_break_crlf_is_one_character_of_two_bytes() {
        // AccessKit counts a `\r\n` as one character whose
        // `character_lengths` entry is 2; two entries would put a caret
        // position between the carriage return and the line feed.
        let text = "ab\r\ncd";
        let source = unmeasured(
            text,
            &[
                (0..4, LineEnd::HardBreak { chars: 1, bytes: 2 }),
                (4..6, LineEnd::EndOfText),
            ],
            0,
        );

        let mut b = label_builder();
        let emission = push_text_runs(&mut b, None, &source);
        assert_eq!(emission.value, text);

        let (_, _, children, _) = b.build(owner());
        let value = children[0].1.value().unwrap();
        assert_eq!(value, "ab\r\n");
        assert_eq!(children[0].1.character_lengths(), &[1, 1, 2]);
        assert_eq!(
            children[0]
                .1
                .character_lengths()
                .iter()
                .map(|n| *n as usize)
                .sum::<usize>(),
            value.len()
        );
    }

    #[test]
    fn lone_cr_is_an_ordinary_character() {
        // Only a line terminator the layout reported as a break gets the
        // collapsed treatment; a stray carriage return inside a line is
        // text, and a reader stepping by character must be able to sit
        // either side of it.
        let text = "a\rb";
        let source = unmeasured(text, &[(0..3, LineEnd::EndOfText)], 0);

        let mut b = label_builder();
        let emission = push_text_runs(&mut b, None, &source);
        assert_eq!(emission.runs.len(), 1);
        assert_eq!(emission.value, text);

        let (_, _, children, _) = b.build(owner());
        assert_eq!(children[0].1.character_lengths(), &[1, 1, 1]);
    }

    #[test]
    fn soft_wrap_adds_no_newline() {
        // Nothing in the source separates the two lines, so inventing a
        // character would desynchronise every offset the widget hands
        // back from a hit test or a caret move.
        let text = "abcdef";
        let source = unmeasured(
            text,
            &[(0..3, LineEnd::SoftWrap), (3..6, LineEnd::EndOfText)],
            0,
        );

        let mut b = label_builder();
        let emission = push_text_runs(&mut b, None, &source);
        assert_eq!(emission.value, "abcdef");
        assert_eq!(emission.runs.len(), 2);

        let (_, _, children, _) = b.build(owner());
        let total: usize = children
            .iter()
            .map(|(_, node)| node.character_lengths().len())
            .sum();
        assert_eq!(total, 6);
    }

    #[test]
    fn unmeasured_tail_gets_a_zero_width_box_at_the_anchor() {
        // The ellipsized tail was painted but never measured. Leaving its
        // geometry out would empty `bounding_boxes()` for every range
        // that touches it, so a magnifier would stop tracking the whole
        // label rather than just its hidden end.
        let text = "Hello world";
        let mut line = geom_line(
            0,
            0..11,
            0..11,
            [0.0, 0.0, 40.0, 16.0],
            LineEnd::EndOfText,
            vec![geom_segment(
                0..5,
                0..5,
                [0.0, 0.0, 40.0, 16.0],
                CanvasDirection::LeftToRight,
                &[
                    (0.0, 8.0),
                    (8.0, 8.0),
                    (16.0, 8.0),
                    (24.0, 8.0),
                    (32.0, 8.0),
                ],
            )],
        );
        line.truncation = Some(LineTruncation {
            ellipsis_x: 40.0,
            ellipsis_width: 8.0,
        });
        let geometry = geometry_of(text, vec![line]);
        let source = TextRunSource::from_geometry(text, &geometry, Point::new(0.0, 0.0), 0);

        let mut b = label_builder();
        let emission = push_text_runs(&mut b, None, &source);
        assert_eq!(emission.runs.len(), 2);
        assert_eq!(emission.value, text);

        let (_, _, children, local) = b.build(owner());
        let tail = &children[1].1;
        assert_eq!(tail.value(), Some(" world"));
        assert_eq!(tail.character_positions(), Some(&[0.0f32; 6][..]));
        assert_eq!(tail.character_widths(), Some(&[0.0f32; 6][..]));
        assert_eq!(tail.text_direction(), Some(TextDirection::LeftToRight));
        // Anchored where the ellipsis was drawn, which is where a reader
        // looking for the hidden text would look.
        assert_eq!(local[1].1, Rect::new(40.0, 0.0, 0.0, 16.0));
    }

    #[test]
    fn rtl_segment_bounds_are_derived_from_the_right_edge() {
        // Character positions are measured from the segment's *leading*
        // edge, which in RTL is its right edge. Reading them as offsets
        // from the left would put every Arabic or Hebrew range's box on
        // the wrong side of the line.
        let text = "ab";
        let geometry = geometry_of(
            text,
            vec![geom_line(
                0,
                0..2,
                0..2,
                [10.0, 0.0, 20.0, 16.0],
                LineEnd::EndOfText,
                vec![geom_segment(
                    0..2,
                    0..2,
                    // The box is wider than what it measured, so the two
                    // conventions disagree: from the left the run would
                    // start at 14, from the right it starts at 10.
                    [10.0, 0.0, 20.0, 16.0],
                    CanvasDirection::RightToLeft,
                    &[(4.0, 8.0), (12.0, 8.0)],
                )],
            )],
        );
        let source = TextRunSource::from_geometry(text, &geometry, Point::new(0.0, 0.0), 0);

        let mut b = label_builder();
        push_text_runs(&mut b, None, &source);
        let (_, root, children, local) = b.build(owner());
        assert_eq!(
            children[0].1.text_direction(),
            Some(TextDirection::RightToLeft)
        );
        assert_eq!(local[0].1, Rect::new(10.0, 0.0, 16.0, 16.0));
        assert_eq!(root.text_direction(), Some(TextDirection::RightToLeft));
    }

    #[test]
    fn bidi_line_emits_one_run_per_direction_all_linked() {
        // One visual line, two reading directions. Each run announces its
        // own direction, and the chain still has to tell a line-navigating
        // reader that the two belong together.
        let text = "abcdef";
        let geometry = geometry_of(
            text,
            vec![geom_line(
                0,
                0..6,
                0..6,
                [0.0, 0.0, 48.0, 16.0],
                LineEnd::EndOfText,
                vec![
                    geom_segment(
                        0..3,
                        0..3,
                        [0.0, 0.0, 24.0, 16.0],
                        CanvasDirection::LeftToRight,
                        &[(0.0, 8.0), (8.0, 8.0), (16.0, 8.0)],
                    ),
                    geom_segment(
                        3..6,
                        3..6,
                        [24.0, 0.0, 24.0, 16.0],
                        CanvasDirection::RightToLeft,
                        &[(0.0, 8.0), (8.0, 8.0), (16.0, 8.0)],
                    ),
                ],
            )],
        );
        let source = TextRunSource::from_geometry(text, &geometry, Point::new(0.0, 0.0), 0);

        let mut b = label_builder();
        let emission = push_text_runs(&mut b, None, &source);
        assert_eq!(emission.runs.len(), 2);

        let (_, _, children, local) = b.build(owner());
        assert_eq!(
            children[0].1.text_direction(),
            Some(TextDirection::LeftToRight)
        );
        assert_eq!(
            children[1].1.text_direction(),
            Some(TextDirection::RightToLeft)
        );
        assert_eq!(local[0].1, Rect::new(0.0, 0.0, 24.0, 16.0));
        assert_eq!(local[1].1, Rect::new(24.0, 0.0, 24.0, 16.0));
        assert_eq!(children[0].1.next_on_line(), Some(children[1].0));
        assert_eq!(children[1].1.previous_on_line(), Some(children[0].0));
    }

    #[test]
    fn run_ids_are_stable_when_a_later_line_reflows() {
        // A run whose id changes looks to the consumer like a different
        // element: the caret and the review cursor lose their place, and
        // every platform re-announces the node. Rewrapping the second
        // line must not disturb the first.
        let text = "abc\ndefghi";
        let before = unmeasured(
            text,
            &[
                (0..4, LineEnd::HardBreak { chars: 1, bytes: 1 }),
                (4..10, LineEnd::EndOfText),
            ],
            0,
        );
        assert_eq!(
            run_ids(&before),
            run_ids(&before),
            "ids are not deterministic"
        );

        let after = unmeasured(
            text,
            &[
                (0..4, LineEnd::HardBreak { chars: 1, bytes: 1 }),
                (4..7, LineEnd::SoftWrap),
                (7..10, LineEnd::EndOfText),
            ],
            0,
        );
        let (before, after) = (run_ids(&before), run_ids(&after));
        assert_eq!(before.len(), 2);
        assert_eq!(after.len(), 3);
        assert_eq!(before[0], after[0]);
    }

    #[test]
    fn run_ids_shift_only_after_an_insertion_point() {
        // Ids come from (byte offset, byte length), so typing into the
        // middle of a document has to leave everything above the caret
        // alone — otherwise a single keystroke invalidates every run in
        // the node.
        let before = "one\ntwo\nthree";
        let after = "one\ntwoX\nthree";
        let before = run_ids(&unmeasured(
            before,
            &[
                (0..4, LineEnd::HardBreak { chars: 1, bytes: 1 }),
                (4..8, LineEnd::HardBreak { chars: 1, bytes: 1 }),
                (8..13, LineEnd::EndOfText),
            ],
            0,
        ));
        let after = run_ids(&unmeasured(
            after,
            &[
                (0..4, LineEnd::HardBreak { chars: 1, bytes: 1 }),
                (4..9, LineEnd::HardBreak { chars: 1, bytes: 1 }),
                (9..14, LineEnd::EndOfText),
            ],
            0,
        ));
        assert_eq!(before[0], after[0]);
        assert_ne!(before[1], after[1]);
        assert_ne!(before[2], after[2]);
    }

    #[test]
    fn two_sources_with_equal_text_get_distinct_ids_from_their_seeds() {
        // Two runs sharing a NodeId panic `accesskit_consumer`'s tree
        // builder, and a widget emitting two identical strings — a
        // terminal's repeated rows, an editor's two empty blocks — is
        // ordinary.
        let mut b = label_builder();
        let first = push_text_runs(&mut b, None, &TextRunSource::flat("Hello", 0));
        let second = push_text_runs(&mut b, None, &TextRunSource::flat("Hello", 1));
        assert_eq!(first.runs.len(), 1);
        assert_eq!(second.runs.len(), 1);
        assert_ne!(first.runs[0].id, second.runs[0].id);

        let (_, _, children, _) = b.build(owner());
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn position_of_maps_a_char_past_255_into_the_second_chunk() {
        // The chunking is invisible to the widget, which still speaks in
        // source character offsets when it reports a caret.
        let text = "a".repeat(300);
        let mut b = label_builder();
        let emission = push_text_runs(&mut b, None, &TextRunSource::flat(&text, 0));
        assert_eq!(emission.position_of(260), Some((emission.runs[1].id, 5)));
    }

    #[test]
    fn position_of_at_a_chunk_boundary_belongs_to_the_earlier_run() {
        // A caret on the boundary is the position a reader arrived at by
        // moving forward through the earlier run; reporting the start of
        // the next one makes a right-arrow appear to do nothing.
        let text = "a".repeat(300);
        let mut b = label_builder();
        let emission = push_text_runs(&mut b, None, &TextRunSource::flat(&text, 0));
        assert_eq!(emission.position_of(255), Some((emission.runs[0].id, 255)));
    }

    #[test]
    fn local_bounds_are_translated_by_the_owner_bounds_in_build() {
        // A widget measures its text in its own space; the owner's
        // window-space rect is only known once the walker has written it,
        // which is the moment `build` runs.
        let text = "Hi";
        let geometry = geometry_of(
            text,
            vec![geom_line(
                0,
                0..2,
                0..2,
                [0.0, 0.0, 16.0, 16.0],
                LineEnd::EndOfText,
                vec![geom_segment(
                    0..2,
                    0..2,
                    [0.0, 0.0, 16.0, 16.0],
                    CanvasDirection::LeftToRight,
                    &[(0.0, 8.0), (8.0, 8.0)],
                )],
            )],
        );
        let source = TextRunSource::from_geometry(text, &geometry, Point::new(4.0, 2.0), 0);

        let mut b = label_builder();
        push_text_runs(&mut b, None, &source);
        b.inner_mut().set_bounds(accesskit::Rect {
            x0: 100.0,
            y0: 50.0,
            x1: 300.0,
            y1: 70.0,
        });

        let (_, _, children, local) = b.build(owner());
        assert_eq!(local[0].1, Rect::new(4.0, 2.0, 16.0, 16.0));
        assert_eq!(
            children[0].1.bounds(),
            Some(accesskit::Rect {
                x0: 104.0,
                y0: 52.0,
                x1: 120.0,
                y1: 68.0,
            })
        );
    }

    #[test]
    fn absolute_rects_are_stored_untranslated() {
        // A scene item under a view transform, or a composite reporting
        // geometry from a child it placed, already holds window-space
        // rects; translating them again would double-count the origin.
        let text = "Hi";
        let geometry = geometry_of(
            text,
            vec![geom_line(
                0,
                0..2,
                0..2,
                [0.0, 0.0, 16.0, 16.0],
                LineEnd::EndOfText,
                vec![geom_segment(
                    0..2,
                    0..2,
                    [0.0, 0.0, 16.0, 16.0],
                    CanvasDirection::LeftToRight,
                    &[(0.0, 8.0), (8.0, 8.0)],
                )],
            )],
        );
        let source = TextRunSource::from_geometry(text, &geometry, Point::new(4.0, 2.0), 0)
            .with_absolute_rects();

        let mut b = label_builder();
        push_text_runs(&mut b, None, &source);
        b.inner_mut().set_bounds(accesskit::Rect {
            x0: 100.0,
            y0: 50.0,
            x1: 300.0,
            y1: 70.0,
        });

        let (_, _, children, local) = b.build(owner());
        assert!(local.is_empty());
        assert_eq!(
            children[0].1.bounds(),
            Some(accesskit::Rect {
                x0: 4.0,
                y0: 2.0,
                x1: 20.0,
                y1: 18.0,
            })
        );
    }

    #[test]
    fn a_role_override_away_from_label_drops_the_runs() {
        // `supports_text_ranges` is false for every other role, so the
        // runs would be inert — invisible to every platform, yet still a
        // node in each update and a child stop the walker reconciles.
        let mut b = label_builder();
        let emission = push_text_runs(&mut b, None, &TextRunSource::flat("Hello", 0));
        assert_eq!(emission.runs.len(), 1);
        b.set_role(Role::Button);

        let (_, root, children, local) = b.build(owner());
        assert!(
            children
                .iter()
                .all(|(_, node)| node.role() != Role::TextRun)
        );
        assert!(children.is_empty());
        assert!(local.is_empty());
        assert!(root.children().is_empty());
    }

    #[test]
    fn a_name_probe_emits_no_runs_but_still_contributes_its_text_to_the_name() {
        // The merge pass and the tooltip-description probe run
        // `accessibility()` on a throwaway builder and read only the
        // name. Runs pushed there would allocate ids that never reach the
        // tree while the widget's real node keeps its own copies.
        let mut b = AccessNodeBuilder::for_name_probe(owner());
        b.set_role(Role::Label);
        let emission = push_text_runs(&mut b, None, &TextRunSource::flat("Hello", 0));
        assert_eq!(emission.value, "Hello");
        assert!(emission.runs.is_empty());

        let (_, _, children, local) = b.build(owner());
        assert!(children.is_empty());
        assert!(local.is_empty());
    }

    #[test]
    fn an_ownerless_builder_emits_no_runs_and_does_not_assert() {
        // The overlay measurement path and the debug inspector both run
        // `accessibility()` through `AccessNodeBuilder::new`, where no
        // owner exists to derive ids from.
        let mut b = AccessNodeBuilder::new();
        b.set_role(Role::Label);
        let emission = push_text_runs(&mut b, None, &TextRunSource::flat("Hello", 0));
        assert_eq!(emission.value, "Hello");
        assert!(emission.runs.is_empty());

        let (_, _, children, local) = b.build(owner());
        assert!(children.is_empty());
        assert!(local.is_empty());
    }
}
