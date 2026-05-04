//! Compile-time-validating proc macros for FernUI's i18n runtime.
//!
//! This crate exports `tr!` and `tr_widget!`, the two proc macros that
//! application code and framework code use to produce `LocalizedString`
//! values. Both macros validate every invocation against a Fluent source
//! `.ftl` file at compile time: they read the file, parse it via
//! `fluent-syntax`, and check that the referenced key exists with
//! matching argument names. Missing keys, missing arguments, and unknown
//! arguments are rejected with a `compile_error!` pointing at the call
//! site.
//!
//! The runtime side of the i18n stack lives in `fern-i18n`. The macros
//! emit code that calls `::fern_ui::i18n::localized(...)` and
//! `::fern_ui::i18n::resolve_message[_widget](...)`, so any crate using
//! these macros must depend on `fern-ui` (with the `i18n` feature enabled).
//!
//! # Source file resolution
//!
//! `tr!` reads `$CARGO_MANIFEST_DIR/locales/en-US.ftl` by default. For
//! tests that need a different fixture, set the `FERN_I18N_SOURCE_PATH`
//! environment variable at compile time to a path relative to
//! `CARGO_MANIFEST_DIR` (or an absolute path).
//!
//! `tr_widget!` reads the same path. It is used inside fern-widgets
//! where the crate's own manifest dir points at the framework's own
//! `.ftl` file.
//!
//! # Cache and rebuild tracking
//!
//! The parsed key→args map is cached per path for the duration of a
//! single proc-macro process, so a crate with hundreds of `tr!` calls
//! parses each `.ftl` file once.
//!
//! To ensure cargo rebuilds the downstream crate when the `.ftl` file
//! changes, every expansion also emits an `include_bytes!(...)` token
//! for the source file. `include_bytes!` is a compiler builtin that
//! registers the path as a build dependency — the same mechanism cargo
//! uses to track `include_str!` files.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use fluent_syntax::ast;
use fluent_syntax::parser::parse;
use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, quote_spanned};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Expr, Ident, Result, Token, parse_macro_input, spanned::Spanned};

/// Compile-time-validating translation macro for application strings.
///
/// Usage: `tr!(key_name())` or `tr!(key_name(arg = expr, ...))`.
///
/// Expands to a `LocalizedString` that, at runtime, resolves the message
/// through the active `I18nManager`'s application bundle.
#[proc_macro]
pub fn tr(input: TokenStream) -> TokenStream {
    tr_impl(input, SourceKind::App, /* signal */ false)
}

/// Compile-time-validating translation macro for framework-internal
/// strings. Same surface as `tr!`, but routes to the framework bundle at
/// runtime. Only used inside `fern-widgets` (architecture §12.13).
#[proc_macro]
pub fn tr_widget(input: TokenStream) -> TokenStream {
    tr_impl(input, SourceKind::Widget, /* signal */ false)
}

/// Reactive variant of `tr!` for the `Signal<T>`-inside-translated-sentence
/// case. Every named argument expression must evaluate to a `Signal<T>`
/// where `T: Clone + 'static + Into<FluentValue<'static>>`. Returns a
/// `Signal<String>` that re-renders on (any arg change ∪ locale change ∪
/// `.ftl` hot reload).
///
/// Use `tr!` for fully-static-arg call sites; this macro is for the
/// reactive case.
#[proc_macro]
pub fn tr_signal(input: TokenStream) -> TokenStream {
    tr_impl(input, SourceKind::App, /* signal */ true)
}

/// Reactive variant of `tr_widget!`. Same arg shape as `tr_signal!`.
#[proc_macro]
pub fn tr_signal_widget(input: TokenStream) -> TokenStream {
    tr_impl(input, SourceKind::Widget, /* signal */ true)
}

#[derive(Clone, Copy)]
enum SourceKind {
    App,
    Widget,
}

impl SourceKind {
    /// The i18n crate root path.
    ///
    /// External apps route through `::fern_ui::i18n` so they only
    /// need `fern-ui` in deps (the serde pattern). Internal fern
    /// crates (`fern-widgets`, `fern-i18n` tests) route through
    /// `::fern_i18n` because they can't depend on `fern-ui` (circular).
    fn i18n_root(self) -> TokenStream2 {
        match self {
            SourceKind::Widget => quote!(::fern_i18n),
            SourceKind::App => {
                // Internal crates (fern-i18n's own tests, trybuild
                // fixtures) don't have fern-ui in their dep tree.
                // Detect by checking CARGO_PKG_NAME — any crate
                // starting with "fern-" is internal to the workspace.
                let pkg = std::env::var("CARGO_PKG_NAME").unwrap_or_default();
                if pkg.starts_with("fern-") {
                    quote!(::fern_i18n)
                } else {
                    quote!(::fern_ui::i18n)
                }
            }
        }
    }

    fn resolver_path(self) -> TokenStream2 {
        let root = self.i18n_root();
        match self {
            SourceKind::App => quote!(#root::resolve_message),
            SourceKind::Widget => quote!(#root::resolve_message_widget),
        }
    }
}

// ---------------------------------------------------------------------------
// Parsed macro input
// ---------------------------------------------------------------------------

struct TrCall {
    /// The path segments of the key, e.g. `auth::login_title` parses
    /// as `[auth, login_title]`. A single-segment path remains the
    /// common case; multi-segment paths organize related keys
    /// hierarchically (architecture §12.2.3).
    key_path: Vec<Ident>,
    args: Vec<TrArg>,
}

impl TrCall {
    /// The span of the entire key path — used for "missing key" and
    /// "wrong argument" error reporting.
    fn path_span(&self) -> proc_macro2::Span {
        self.key_path
            .first()
            .map(|i| i.span())
            .unwrap_or_else(proc_macro2::Span::call_site)
    }
}

struct TrArg {
    name: Ident,
    value: Expr,
}

impl Parse for TrCall {
    fn parse(input: ParseStream) -> Result<Self> {
        // Parse `ident (:: ident)*` as the key path.
        let mut key_path = vec![input.parse::<Ident>()?];
        while input.peek(Token![::]) {
            let _colons: Token![::] = input.parse()?;
            key_path.push(input.parse::<Ident>()?);
        }

        let content;
        syn::parenthesized!(content in input);

        let args_punct: Punctuated<TrArg, Token![,]> =
            Punctuated::parse_terminated(&content)?;

        Ok(TrCall {
            key_path,
            args: args_punct.into_iter().collect(),
        })
    }
}

impl Parse for TrArg {
    fn parse(input: ParseStream) -> Result<Self> {
        let name: Ident = input.parse()?;
        let _eq: Token![=] = input.parse()?;
        let value: Expr = input.parse()?;
        Ok(TrArg { name, value })
    }
}

// ---------------------------------------------------------------------------
// Source-file location and parse cache
// ---------------------------------------------------------------------------

/// Where the macro reads its source-language messages from at expansion
/// time. The two variants correspond to the two layouts §12.2 calls out:
/// a single flat `locales/en-US.ftl` file, or a `locales/en-US/`
/// directory containing one or more `.ftl` files organized by feature.
///
/// Directory mode is enabled automatically if the default path
/// `locales/en-US/` exists as a directory, or explicitly via the
/// `FERN_I18N_SOURCE_DIR` environment variable. Applications that
/// already use the flat file layout are unaffected.
#[derive(Clone, Debug)]
enum SourceInfo {
    File(PathBuf),
    Dir(PathBuf),
}

impl SourceInfo {
    fn display(&self) -> std::path::Display<'_> {
        match self {
            Self::File(p) | Self::Dir(p) => p.display(),
        }
    }
}

/// Resolve the `.ftl` source location for the crate currently being
/// compiled. Precedence: `FERN_I18N_SOURCE_DIR` > `FERN_I18N_SOURCE_PATH`
/// > auto-detect `locales/en-US/` directory > fallback to the single
/// `locales/en-US.ftl` file. The env vars let tests point the macro at
/// a fixture without touching the consuming crate's layout (used by
/// `tests/trybuild.rs`).
fn resolve_source() -> std::result::Result<SourceInfo, String> {
    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .map_err(|_| "CARGO_MANIFEST_DIR is not set".to_string())?;
    let manifest_path = PathBuf::from(&manifest);

    if let Ok(dir_override) = std::env::var("FERN_I18N_SOURCE_DIR") {
        let p = PathBuf::from(&dir_override);
        let resolved = if p.is_absolute() {
            p
        } else {
            manifest_path.join(dir_override)
        };
        return Ok(SourceInfo::Dir(resolved));
    }

    if let Ok(file_override) = std::env::var("FERN_I18N_SOURCE_PATH") {
        let p = PathBuf::from(&file_override);
        let resolved = if p.is_absolute() {
            p
        } else {
            manifest_path.join(file_override)
        };
        return Ok(SourceInfo::File(resolved));
    }

    let dir_default = manifest_path.join("locales/en-US");
    if dir_default.is_dir() {
        return Ok(SourceInfo::Dir(dir_default));
    }

    Ok(SourceInfo::File(manifest_path.join("locales/en-US.ftl")))
}

/// Metadata about a single Fluent message.
#[derive(Clone, Debug)]
struct MessageInfo {
    /// Variable names referenced by the message's pattern.
    vars: Vec<String>,
    /// Reconstructable source-language template for the macro's
    /// compile-time fallback path — used when no `I18nManager` is
    /// installed at runtime. `Some(parts)` for patterns composed
    /// entirely of literal text and simple `{ $var }` substitutions;
    /// `None` for patterns that use selectors, plural rules, function
    /// calls, or message references (those need the real Fluent
    /// formatter to produce meaningful output, so the macro leaves
    /// the runtime fallback as the literal key).
    fallback: Option<Vec<FallbackPart>>,
}

/// One piece of a reconstructable source-language fallback template.
#[derive(Clone, Debug)]
enum FallbackPart {
    /// Verbatim literal text from a `TextElement` in the pattern.
    Text(String),
    /// A `{ $var }` substitution. The variable name matches one of
    /// the arg names declared in `vars`, so the macro expansion
    /// converts it to the captured Rust binding via `ToString`.
    Var(String),
}

/// Parsed key map for one source (file or directory). Carries both the
/// key → `MessageInfo` map and the list of files that contributed to
/// it — the expansion emits one `include_bytes!` per file so cargo
/// rebuilds the consuming crate when any of them change.
#[derive(Clone, Debug)]
struct KeyMap {
    messages: HashMap<String, MessageInfo>,
    /// Absolute paths of every `.ftl` file that contributed messages.
    /// For file mode, a single entry. For directory mode, one entry
    /// per file discovered during the walk.
    watched_files: Vec<PathBuf>,
}

fn cache() -> &'static Mutex<HashMap<PathBuf, KeyMap>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, KeyMap>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Dispatch loader used by the macro entry point.
fn load_key_map_for_source(info: &SourceInfo) -> std::result::Result<KeyMap, String> {
    match info {
        SourceInfo::File(p) => load_key_map(p),
        SourceInfo::Dir(p) => load_key_map_from_dir(p),
    }
}

/// Parse one `.ftl` file's entries into `messages` and `watched_files`
/// buffers. Shared by the single-file and directory-walk loaders.
fn parse_ftl_file(
    path: &std::path::Path,
    messages: &mut HashMap<String, MessageInfo>,
    watched_files: &mut Vec<PathBuf>,
) -> std::result::Result<(), String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read `{}`: {e}", path.display()))?;

    let resource = parse(contents.as_str()).map_err(|(_, errs)| {
        format!("Fluent parse errors in `{}`: {:?}", path.display(), errs)
    })?;

    for entry in &resource.body {
        if let ast::Entry::Message(msg) = entry {
            let id = msg.id.name.to_string();
            // Single walk of the pattern: `build_fallback` already
            // descends every element and branch, so we piggy-back a
            // var-collection side effect onto it rather than running
            // a second full walk via `collect_variables`. For
            // patterns that bail out of fallback (selectors, plural
            // rules, term/message refs) we do still need to enumerate
            // `$var`s, so the walker continues through those paths
            // and only reports `fallback = None` at the end.
            let mut vars_buf: Vec<String> = Vec::new();
            let fallback = if let Some(pattern) = &msg.value {
                build_fallback_and_collect_vars(pattern, &mut vars_buf)
            } else {
                None
            };
            // Dedupe while preserving first-seen order.
            let mut seen = std::collections::HashSet::new();
            vars_buf.retain(|v| seen.insert(v.clone()));
            if messages.contains_key(&id) {
                return Err(format!(
                    "duplicate message key `{id}` (second definition in `{}`)",
                    path.display()
                ));
            }
            messages.insert(
                id,
                MessageInfo {
                    vars: vars_buf,
                    fallback,
                },
            );
        }
    }

    watched_files.push(path.to_path_buf());
    Ok(())
}

/// Load-and-parse a single `.ftl` file into a `KeyMap`, or read the
/// cached copy.
fn load_key_map(path: &std::path::Path) -> std::result::Result<KeyMap, String> {
    if let Ok(guard) = cache().lock()
        && let Some(existing) = guard.get(path)
    {
        return Ok(existing.clone());
    }

    let mut messages: HashMap<String, MessageInfo> = HashMap::new();
    let mut watched_files: Vec<PathBuf> = Vec::new();
    parse_ftl_file(path, &mut messages, &mut watched_files)?;

    let map = KeyMap {
        messages,
        watched_files,
    };
    if let Ok(mut guard) = cache().lock() {
        guard.insert(path.to_path_buf(), map.clone());
    }
    Ok(map)
}

/// Walk a `locales/en-US/` directory recursively, parse every `.ftl`
/// file, and merge their messages into a single flat `KeyMap`. The
/// order of files is deterministic (sorted by relative path) so that
/// the error reported on duplicate keys is stable. Architecture
/// §12.2.3: the directory layout is an organizational convention;
/// every key across every file is visible through a single flat
/// lookup. Applications prevent collisions by prefixing keys (e.g.,
/// `auth-login-title = Log in` in `auth.ftl`), and the macro's
/// `tr!(auth::login_title())` syntax targets the same flat key via
/// `path_to_fluent_key`.
fn load_key_map_from_dir(root: &std::path::Path) -> std::result::Result<KeyMap, String> {
    if let Ok(guard) = cache().lock()
        && let Some(existing) = guard.get(root)
    {
        return Ok(existing.clone());
    }

    if !root.is_dir() {
        return Err(format!(
            "`{}` is not a directory (directory mode was requested)",
            root.display()
        ));
    }

    let mut ftl_files: Vec<PathBuf> = Vec::new();
    collect_ftl_files(root, &mut ftl_files).map_err(|e| {
        format!("failed to walk `{}`: {e}", root.display())
    })?;
    ftl_files.sort();

    if ftl_files.is_empty() {
        return Err(format!(
            "no `.ftl` files found under `{}`",
            root.display()
        ));
    }

    let mut messages: HashMap<String, MessageInfo> = HashMap::new();
    let mut watched_files: Vec<PathBuf> = Vec::new();
    for file in &ftl_files {
        parse_ftl_file(file, &mut messages, &mut watched_files)?;
    }

    let map = KeyMap {
        messages,
        watched_files,
    };
    if let Ok(mut guard) = cache().lock() {
        guard.insert(root.to_path_buf(), map.clone());
    }
    Ok(map)
}

/// Recursively collect every `.ftl` file under `dir`.
fn collect_ftl_files(
    dir: &std::path::Path,
    out: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_ftl_files(&path, out)?;
        } else if file_type.is_file()
            && path.extension().and_then(|e| e.to_str()) == Some("ftl")
        {
            out.push(path);
        }
    }
    Ok(())
}

/// Walk a Fluent pattern once, building the fallback template and
/// collecting every `$var` reference in a single pass. Returns
/// `Some(parts)` iff the pattern is composed entirely of literal text
/// and simple `{ $var }` references (safe to reassemble without a
/// Fluent formatter); otherwise returns `None` to disable the
/// fallback, but still pushes any discovered variable references to
/// `vars_out` so that argument validation has complete information.
///
/// Patterns that disable fallback include: selectors, plural rules,
/// function calls, message/term references, and nested placeables.
/// Argument-validation only cares about the set of `$var`s, which
/// this walker collects from every branch of every select.
fn build_fallback_and_collect_vars(
    pattern: &ast::Pattern<&str>,
    vars_out: &mut Vec<String>,
) -> Option<Vec<FallbackPart>> {
    let mut parts: Vec<FallbackPart> = Vec::new();
    let mut fallback_ok = true;
    for element in &pattern.elements {
        match element {
            ast::PatternElement::TextElement { value } => {
                if fallback_ok {
                    parts.push(FallbackPart::Text((*value).to_string()));
                }
            }
            ast::PatternElement::Placeable { expression } => {
                let simple_var = match expression {
                    ast::Expression::Inline(
                        ast::InlineExpression::VariableReference { id },
                    ) => Some(id.name.to_string()),
                    _ => None,
                };
                if let Some(var) = simple_var {
                    vars_out.push(var.clone());
                    if fallback_ok {
                        parts.push(FallbackPart::Var(var));
                    }
                } else {
                    // Non-trivial placeable: disable fallback, but
                    // continue walking to collect any `$var`s inside
                    // selectors/function calls/etc. so the arg
                    // validator has the full set.
                    fallback_ok = false;
                    walk_expr_for_vars(expression, vars_out);
                }
            }
        }
    }
    if fallback_ok { Some(parts) } else { None }
}

fn walk_expr_for_vars(expr: &ast::Expression<&str>, out: &mut Vec<String>) {
    match expr {
        ast::Expression::Inline(inline) => walk_inline_for_vars(inline, out),
        ast::Expression::Select { selector, variants } => {
            walk_inline_for_vars(selector, out);
            for variant in variants {
                // Recurse into each variant's pattern; we discard the
                // returned `Option<Vec<FallbackPart>>` because the
                // outer walk has already marked fallback disabled.
                let _ = build_fallback_and_collect_vars(&variant.value, out);
            }
        }
    }
}

fn walk_inline_for_vars(inline: &ast::InlineExpression<&str>, out: &mut Vec<String>) {
    match inline {
        ast::InlineExpression::VariableReference { id } => {
            out.push(id.name.to_string());
        }
        ast::InlineExpression::Placeable { expression } => {
            walk_expr_for_vars(expression, out);
        }
        ast::InlineExpression::FunctionReference { arguments, .. } => {
            for positional in &arguments.positional {
                walk_inline_for_vars(positional, out);
            }
            for named in &arguments.named {
                walk_inline_for_vars(&named.value, out);
            }
        }
        ast::InlineExpression::MessageReference { .. }
        | ast::InlineExpression::TermReference { .. }
        | ast::InlineExpression::StringLiteral { .. }
        | ast::InlineExpression::NumberLiteral { .. } => {}
    }
}

// ---------------------------------------------------------------------------
// Name conversion + suggestion helpers
// ---------------------------------------------------------------------------

/// Convert a Rust path like `auth::login_title` into the Fluent key
/// `auth__login-title`.
///
/// Two separators at play:
/// - `_` (single underscore) **inside** a segment is converted to
///   `-` — matching the long-standing flat-mode rule
///   (`foo_bar` → `foo-bar`).
/// - `__` (double underscore) is reserved as the **nested module**
///   separator, emitted at every `::` boundary between segments.
///
/// Because `_` → `-` and `__` never occurs inside a normalized
/// segment, the mapping is injective: distinct Rust paths always
/// produce distinct Fluent keys. Callers that *write* `__` directly
/// inside a segment (e.g., `tr!(foo__bar())`) get a compile-time
/// error pointing at the offending segment — the rule is
/// "single underscores inside, `::` between."
///
/// Examples:
/// | Rust path                              | Fluent key                             |
/// |----------------------------------------|----------------------------------------|
/// | `greeting`                             | `greeting`                             |
/// | `foo_bar`                              | `foo-bar`                              |
/// | `auth::login`                          | `auth__login`                          |
/// | `auth::login_title`                    | `auth__login-title`                    |
/// | `auth_login::title`                    | `auth-login__title`                    |
/// | `auth::login::title`                   | `auth__login__title`                   |
/// | `settings::display::resolution_label`  | `settings__display__resolution-label`  |
fn path_to_fluent_key(
    segments: &[Ident],
) -> std::result::Result<String, syn::Error> {
    let mut normalized: Vec<String> = Vec::with_capacity(segments.len());
    for seg in segments {
        let s = seg.to_string();
        if s.contains("__") {
            return Err(syn::Error::new(
                seg.span(),
                format!(
                    "fern-i18n: path segment `{s}` contains `__`, \
                     which is reserved as the nested-module separator. \
                     Use `::` for nesting, or single `_` within a segment."
                ),
            ));
        }
        // Fluent message-id grammar is `[a-zA-Z][a-zA-Z0-9_-]*`. Rust
        // allows Unicode identifiers (e.g., `tr!(héllo())`), which
        // would silently produce a non-Fluent-id string that would
        // fail lookup with a confusing "key not found" error instead
        // of pointing at the real cause. Reject non-ASCII segments
        // upfront with a clearer message.
        let mut chars = s.chars();
        let first = chars.next().expect("syn::Ident is non-empty");
        let first_ok = first.is_ascii_alphabetic();
        let rest_ok = chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !first_ok || !rest_ok {
            return Err(syn::Error::new(
                seg.span(),
                format!(
                    "fern-i18n: path segment `{s}` is not a valid Fluent \
                     message id — must match `[a-zA-Z][a-zA-Z0-9_]*` (ASCII \
                     letters, digits, and underscores only)."
                ),
            ));
        }
        normalized.push(s.replace('_', "-"));
    }
    Ok(normalized.join("__"))
}

/// Find the closest matching key in `candidates` for a user-supplied
/// query, using a cheap Levenshtein-style score. Returns `None` if no
/// candidate is within a small edit budget.
fn suggest_key<'a>(query: &str, candidates: impl Iterator<Item = &'a String>) -> Option<String> {
    let mut best: Option<(usize, &String)> = None;
    for cand in candidates {
        let d = levenshtein(query, cand);
        if d < best.map(|(bd, _)| bd).unwrap_or(usize::MAX) {
            best = Some((d, cand));
        }
    }
    best.and_then(|(d, s)| if d <= 3 { Some(s.clone()) } else { None })
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());
    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![0usize; n + 1];
    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (curr[j - 1] + 1)
                .min(prev[j] + 1)
                .min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

// ---------------------------------------------------------------------------
// Core expansion
// ---------------------------------------------------------------------------

fn tr_impl(input: TokenStream, kind: SourceKind, signal: bool) -> TokenStream {
    let call = parse_macro_input!(input as TrCall);

    let source_info = match resolve_source() {
        Ok(info) => info,
        Err(msg) => {
            return syn::Error::new(call.path_span(), msg)
                .to_compile_error()
                .into();
        }
    };

    let key_map = match load_key_map_for_source(&source_info) {
        Ok(m) => m,
        Err(msg) => {
            return syn::Error::new(
                call.path_span(),
                format!(
                    "fern-i18n: failed to load source `{}`: {msg}",
                    source_info.display()
                ),
            )
            .to_compile_error()
            .into();
        }
    };

    let fluent_key = match path_to_fluent_key(&call.key_path) {
        Ok(k) => k,
        Err(e) => return e.to_compile_error().into(),
    };

    // 1. Validate key existence.
    let (expected_args, fallback_parts): (Vec<String>, Option<Vec<FallbackPart>>) =
        match key_map.messages.get(&fluent_key) {
            Some(info) => (info.vars.clone(), info.fallback.clone()),
            None => {
                let mut msg = format!(
                    "fern-i18n: translation key `{}` not found in `{}`",
                    fluent_key,
                    source_info.display()
                );
                if let Some(suggestion) = suggest_key(&fluent_key, key_map.messages.keys()) {
                    msg.push_str(&format!(" (did you mean `{suggestion}`?)"));
                }
                return syn::Error::new(call.path_span(), msg)
                    .to_compile_error()
                    .into();
            }
        };

    // 2. Validate argument names.
    let provided_names: Vec<String> =
        call.args.iter().map(|a| a.name.to_string()).collect();

    for expected in &expected_args {
        if !provided_names.iter().any(|p| p == expected) {
            return syn::Error::new(
                call.path_span(),
                format!(
                    "fern-i18n: missing argument `{expected}` for key `{fluent_key}`"
                ),
            )
            .to_compile_error()
            .into();
        }
    }
    for arg in &call.args {
        let name = arg.name.to_string();
        if !expected_args.iter().any(|e| e == &name) {
            return syn::Error::new(
                arg.name.span(),
                format!(
                    "fern-i18n: unknown argument `{name}` for key `{fluent_key}` (expected: {})",
                    if expected_args.is_empty() {
                        "no arguments".to_string()
                    } else {
                        expected_args.join(", ")
                    }
                ),
            )
            .to_compile_error()
            .into();
        }
    }

    // 3. Emit the expansion.
    let i18n_root = kind.i18n_root();
    let resolver = kind.resolver_path();
    let key_lit = proc_macro2::Literal::string(&fluent_key);
    // One tracked `include_bytes!` per `.ftl` file that contributed to
    // the key map. In file mode there's a single entry; in directory
    // mode the list covers every file walked. `const _: &[u8] = ...`
    // with anonymous names means multiple tracking consts coexist in
    // the same expansion block without name conflicts.
    let watch_stmts: Vec<TokenStream2> = key_map
        .watched_files
        .iter()
        .map(|p| {
            let lit =
                proc_macro2::Literal::string(&p.to_string_lossy());
            quote! {
                const _: &[u8] = ::core::include_bytes!(#lit);
            }
        })
        .collect();

    // Per-arg let bindings. In `tr!` mode the let stores the expression
    // by-value (consistent with the historical behaviour). In
    // `tr_signal!` mode we wrap with `.clone()` so the caller's `Signal`
    // handle survives — `(expr).clone()` is method-call syntax that
    // auto-refs, so a bare ident remains borrowed instead of moved.
    let mut arg_let_bindings: Vec<TokenStream2> = Vec::new();
    let mut arg_slice_entries_static: Vec<TokenStream2> = Vec::new();
    let mut arg_slice_entries_signal: Vec<TokenStream2> = Vec::new();
    let mut arg_idents: Vec<Ident> = Vec::new();
    for arg in &call.args {
        let binding_ident = &arg.name;
        let value_expr = &arg.value;
        let name_lit = proc_macro2::Literal::string(&arg.name.to_string());
        let binding = if signal {
            quote_spanned! { value_expr.span() =>
                let #binding_ident = (#value_expr).clone();
            }
        } else {
            quote_spanned! { value_expr.span() =>
                let #binding_ident = { #value_expr };
            }
        };
        arg_let_bindings.push(binding);
        arg_slice_entries_static.push(quote! {
            (#name_lit, #i18n_root::FluentValue::from(#binding_ident.clone()))
        });
        arg_slice_entries_signal.push(quote! {
            (#name_lit, #i18n_root::FluentValue::from(#binding_ident.get()))
        });
        arg_idents.push(binding_ident.clone());
    }

    // Fallback handling: when the macro expansion runs without an
    // installed i18n manager (or the active locale's bundle lacks the
    // key), the runtime resolver returns the literal key as a
    // placeholder. For simple patterns — literal text with optional
    // `{ $var }` substitutions — the macro reconstructs the source
    // language text from the pre-parsed `FallbackPart` list, pulling
    // each variable's value from the closure's captured bindings via
    // `ToString`. In `tr_signal!` mode the captured bindings are
    // `Signal<T>` so we read `.get()` first; in `tr!` mode we use the
    // value directly. Patterns that use selectors, plural rules,
    // function calls, or message references bail out at parse time
    // and fall back to returning the key as a placeholder.
    let arg_slice_entries: &[TokenStream2] = if signal {
        &arg_slice_entries_signal
    } else {
        &arg_slice_entries_static
    };

    let fallback_body = match fallback_parts {
        Some(parts) => {
            let mut fallback_stmts: Vec<TokenStream2> = Vec::new();
            for part in parts {
                match part {
                    FallbackPart::Text(text) => {
                        let lit = proc_macro2::Literal::string(&text);
                        fallback_stmts.push(quote! {
                            __fern_fallback.push_str(#lit);
                        });
                    }
                    FallbackPart::Var(var_name) => {
                        let ident = proc_macro2::Ident::new(
                            &var_name,
                            proc_macro2::Span::call_site(),
                        );
                        let value_expr = if signal {
                            quote! { &#ident.get() }
                        } else {
                            quote! { &#ident }
                        };
                        fallback_stmts.push(quote! {
                            __fern_fallback.push_str(
                                &::std::string::ToString::to_string(#value_expr),
                            );
                        });
                    }
                }
            }
            quote! {
                let __fern_result =
                    #resolver(#key_lit, &[#(#arg_slice_entries),*]);
                if __fern_result == #key_lit {
                    let mut __fern_fallback = ::std::string::String::new();
                    #(#fallback_stmts)*
                    __fern_fallback
                } else {
                    __fern_result
                }
            }
        }
        None => quote! {
            #resolver(#key_lit, &[#(#arg_slice_entries),*])
        },
    };

    let expanded = if signal {
        // Reactive lowering: `Signal<String>` subscribed to every arg
        // signal plus the version signal. Re-runs the resolver via a
        // shared `Rc<dyn Fn>` whenever any dependency fires.
        let arg_subscribes: Vec<TokenStream2> = arg_idents
            .iter()
            .map(|ident| {
                quote! {
                    {
                        let __fern_resolver_arg = ::std::rc::Rc::clone(&__fern_resolver);
                        let __fern_weak_arg = __fern_weak.clone();
                        let __fern_h = #ident.observe(move |_| {
                            if let Some(__t) = __fern_weak_arg.upgrade() {
                                __t.set((__fern_resolver_arg)());
                            }
                        });
                        __fern_out.attach_keepalive(__fern_h);
                    }
                }
            })
            .collect();

        // Each arg signal is captured by clone into the resolver
        // closure so the closure can be cloned into N+1 observers.
        let arg_clones_for_resolver: Vec<TokenStream2> = arg_idents
            .iter()
            .map(|ident| quote! { let #ident = #ident.clone(); })
            .collect();

        quote! {
            {
                #(#watch_stmts)*

                // Bring `observe`, `attach_keepalive`, `downgrade` into
                // scope as inherent methods on `Signal`.
                #(#arg_let_bindings)*

                let __fern_resolver: ::std::rc::Rc<dyn ::std::ops::Fn() -> ::std::string::String> = {
                    #(#arg_clones_for_resolver)*
                    ::std::rc::Rc::new(move || {
                        #fallback_body
                    })
                };

                let __fern_out = #i18n_root::Signal::new((__fern_resolver)());
                let __fern_weak = __fern_out
                    .downgrade()
                    .expect("Signal::new returns mutable; downgrade cannot fail");

                if let Some(__fern_ver) = #i18n_root::current_version_signal() {
                    let __fern_resolver_ver = ::std::rc::Rc::clone(&__fern_resolver);
                    let __fern_weak_ver = __fern_weak.clone();
                    let __fern_handle_ver = __fern_ver.observe(move |_| {
                        if let Some(__t) = __fern_weak_ver.upgrade() {
                            __t.set((__fern_resolver_ver)());
                        }
                    });
                    __fern_out.attach_keepalive(__fern_handle_ver);
                }

                #(#arg_subscribes)*

                __fern_out
            }
        }
    } else {
        quote! {
            {
                // Force cargo to track every source `.ftl` file that
                // contributed to the key map. `include_bytes!` is a
                // compiler builtin that registers the path as a build
                // dependency — when any file changes, cargo rebuilds the
                // containing crate. The constants are discarded.
                #(#watch_stmts)*

                #i18n_root::localized({
                    #(#arg_let_bindings)*
                    move || {
                        #fallback_body
                    }
                })
            }
        }
    };

    expanded.into()
}
