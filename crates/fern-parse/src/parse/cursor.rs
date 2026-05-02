//! Shared lookahead helpers for the body and property parsers.

use syn::parse::ParseStream;
use syn::{Ident, Path, Token, token};

/// Peek whether the current cursor is at an element-start prefix per
/// spec §3.1 "commit on distinctive prefix":
///
/// An UpperCamel `Ident` followed by `(` (positional args), `{` (body),
/// or `::` (path continuation) commits to element parsing. A bare
/// UpperCamel ident at end-of-stream also counts (e.g. `Spacer`).
///
/// Lowercase idents are not element starts — they are properties or
/// structural keywords.
///
/// One explicit exclusion: paths ending in `UpperCamel::UpperCamel`
/// (enum-variant shape — `Cmd::Save`, `ImageFit::Contain`,
/// `TextOverflow::Ellipsis(EllipsisMode::Trailing)`). These are only
/// treated as element starts if followed by a `{` body block. Without
/// the body they're Rust expressions: a variant value or a tuple-variant
/// construction. The macro can't distinguish variants from types at
/// expansion time, so the UpperCamel::UpperCamel shape is used as a
/// reliable syntactic signal.
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

    // When the path has `::`, inspect the whole path to detect the
    // enum-variant shape.
    if fork.peek(Token![::]) {
        let path_fork = input.fork();
        if let Ok(path) = path_fork.parse::<Path>() {
            if ends_in_variant_shape(&path) {
                // `Type::Variant { ... }` is still element syntax
                // (fern body block); bare `Type::Variant` or
                // `Type::Variant(args)` is an expression.
                return path_fork.peek(token::Brace);
            }
        }
        return true;
    }

    // Single-ident UpperCamel.
    fork.is_empty() || fork.peek(token::Paren) || fork.peek(token::Brace)
}

/// Returns true when the path's last two segments are both UpperCamel
/// — the Rust enum-variant shape (`Cmd::Save`, `ImageFit::Contain`).
fn ends_in_variant_shape(path: &Path) -> bool {
    if path.segments.len() < 2 {
        return false;
    }
    let last = &path.segments[path.segments.len() - 1].ident;
    let second_last = &path.segments[path.segments.len() - 2].ident;
    ident_starts_upper(last) && ident_starts_upper(second_last)
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
