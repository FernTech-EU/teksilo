//! Compile-fail: the key `nonexistent` is not defined in the fixture.

use fern_i18n::tr;

fn main() {
    let _ = tr!(nonexistent());
}
