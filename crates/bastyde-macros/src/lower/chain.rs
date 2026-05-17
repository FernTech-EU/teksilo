//! Constructor-call lowering for chain-form elements.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote_spanned;

use bastyde_parse::BatiElement;

/// Emit the constructor call: either `TypePath::ctor(args)` when the
/// user named an explicit ctor, or `TypePath::new(args)` otherwise
/// (the spec §3.2 default). The emitted call's span is anchored on the
/// type path's head so "cannot find type X" lands on the user's token.
pub(crate) fn lower_ctor_call(e: &BatiElement) -> TokenStream2 {
    let path = &e.type_path;
    let head_span = e.head_span;
    let args = &e.args;

    if e.has_explicit_ctor {
        quote_spanned! { head_span =>
            #path(#(#args),*)
        }
    } else {
        quote_spanned! { head_span =>
            #path::new(#(#args),*)
        }
    }
}
