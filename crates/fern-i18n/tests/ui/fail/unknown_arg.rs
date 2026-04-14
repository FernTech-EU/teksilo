//! Compile-fail: `welcome` only declares `$name`, not `$extra`.

use fern_i18n::tr;

fn main() {
    let name = String::from("A");
    let extra = String::from("B");
    let _ = tr!(welcome(name = name, extra = extra));
}
