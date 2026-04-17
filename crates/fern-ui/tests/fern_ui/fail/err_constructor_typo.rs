//! Spec §9.1: a type-path typo lands on the user's ident via the
//! macro's span-preserving emission; the compiler then surfaces its
//! "cannot find type `Buton`" error under that token, not the macro's
//! synthetic span.

use fern_ui::prelude::*;

fn main() {
    let _ = fern!(Buton("oops"));
}
