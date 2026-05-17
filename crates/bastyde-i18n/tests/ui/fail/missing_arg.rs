//! Compile-fail: `welcome` requires a `$name` argument.

use bastyde_i18n::tr;

fn main() {
    let _ = tr!(welcome());
}
