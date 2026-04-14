//! `tr!` accepting nested `::` paths, validated against a directory
//! walk of `nested_fixture/`. The test runner sets
//! `FERN_I18N_SOURCE_DIR` to the fixture before invoking trybuild.

use fern_i18n::tr;

fn main() {
    // Root namespace — no `::`, just `greeting`. The macro converts
    // the path to the Fluent key `greeting`, which is defined in
    // `nested_fixture/main.ftl`.
    let ls = tr!(greeting());
    assert_eq!(ls.resolve_now(), "Hello");

    // `auth::login_title` → Fluent key `auth-login-title`, defined
    // in `nested_fixture/auth/auth.ftl`.
    let ls = tr!(auth::login_title());
    assert_eq!(ls.resolve_now(), "Sign in");

    // Deeper nesting works too. `settings::display::resolution_label`
    // → Fluent key `settings-display-resolution-label`, defined in
    // `nested_fixture/settings/display.ftl`.
    let ls = tr!(settings::display::resolution_label());
    assert_eq!(ls.resolve_now(), "Resolution");
}
