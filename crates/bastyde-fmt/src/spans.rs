//! Byte-range utilities shared between the printer and the host-file
//! visitor. Walk a `proc_macro2::TokenStream` and compute the union of
//! its leaf-token byte ranges in the original source.

use std::ops::Range;

use proc_macro2::{Span, TokenStream, TokenTree};

pub fn ts_byte_range(ts: &TokenStream) -> Option<Range<usize>> {
    let mut start = usize::MAX;
    let mut end = 0usize;
    walk_extents(ts, &mut start, &mut end);
    if start == usize::MAX {
        None
    } else {
        Some(start..end)
    }
}

fn walk_extents(ts: &TokenStream, start: &mut usize, end: &mut usize) {
    for tt in ts.clone() {
        match tt {
            TokenTree::Group(g) => {
                merge(g.span_open(), start, end);
                walk_extents(&g.stream(), start, end);
                merge(g.span_close(), start, end);
            }
            TokenTree::Ident(i) => merge(i.span(), start, end),
            TokenTree::Punct(p) => merge(p.span(), start, end),
            TokenTree::Literal(l) => merge(l.span(), start, end),
        }
    }
}

fn merge(span: Span, start: &mut usize, end: &mut usize) {
    let r = span.byte_range();
    if r.start < *start {
        *start = r.start;
    }
    if r.end > *end {
        *end = r.end;
    }
}
