//! Shared lookahead helpers for the body and property parsers.

use syn::parse::ParseStream;
use syn::{Ident, Token, token};

/// Peek whether the current cursor is at an element-start prefix per
/// spec §3.1 "commit on distinctive prefix":
///
/// An UpperCamel `Ident` followed by `(` (positional args), `{` (body),
/// or `::` (path continuation) commits to element parsing. A bare
/// UpperCamel ident at end-of-stream also counts (e.g. `Spacer`).
///
/// Lowercase idents are not element starts — they are properties or
/// structural keywords.
pub(crate) fn peek_element_start(input: ParseStream) -> bool {
    if !input.peek(Ident) {
        return false;
    }
    let fork = input.fork();
    let Ok(ident) = fork.parse::<Ident>() else {
        return false;
    };
    if !ident_starts_upper(&ident) {
        return false;
    }
    // At end of stream with only the UpperCamel ident consumed, it's
    // still an element (equivalent to empty-parens form, spec §3.2).
    fork.is_empty()
        || fork.peek(token::Paren)
        || fork.peek(token::Brace)
        || fork.peek(Token![::])
}

pub(crate) fn ident_starts_upper(ident: &Ident) -> bool {
    ident
        .to_string()
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase())
}

/// Peek whether the cursor is at `#{ ... }` — the escape form.
pub(crate) fn peek_escape(input: ParseStream) -> bool {
    input.peek(Token![#]) && input.peek2(token::Brace)
}

/// Peek whether the cursor is at `Ident = <element-start>` — a
/// binding. We check that the following `=` is a single-equals (not
/// `==`) and that after it lies an element-start prefix.
pub(crate) fn peek_binding(input: ParseStream) -> bool {
    if !input.peek(Ident) || !input.peek2(Token![=]) || input.peek2(Token![==]) {
        return false;
    }
    let fork = input.fork();
    let Ok(_ident) = fork.parse::<Ident>() else {
        return false;
    };
    let Ok(_eq) = fork.parse::<Token![=]>() else {
        return false;
    };
    peek_element_start(&fork)
}
