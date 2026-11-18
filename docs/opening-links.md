<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Opening links and files

A link the user activates opens in the application the desktop has for its
scheme: `https:` in the browser, `mailto:` in the mail client, `xmpp:` in the
chat client. A file opens in its default application, a folder in the file
manager. [`teksilo_platform::open_uri`](../crates/teksilo-platform/src/open_uri.rs)
does both; no feature turns it on, and `teksilo::prelude::*` brings it into
scope.

```rust
use teksilo::prelude::*;

let link = Link::new(lit!("Teksilo's documentation"))
    .url("https://docs.teksilo.rs/")
    .on_activate_fn(|ctx| {
        if let Err(error) = ctx.open_uri("https://docs.teksilo.rs/") {
            eprintln!("could not open the link: {error}");
        }
    });

let editor = RichTextEditor::read_only(document).on_link_activated(|href, ctx| {
    let _ = ctx.open_uri(href);
});
```

Neither `Link` nor `RichTextEditor` opens anything by itself: the application
decides, in the handler of the user's gesture.

| Call | Takes | Use |
| --- | --- | --- |
| `ctx.open_uri(uri)` | a URI | a link, in a handler |
| `ctx.open_path(path)` | an absolute path that exists | a file or folder, in a handler |
| `open_uri::open_uri(uri)`, `open_uri::open_path(path)` | the same | code with no `EventContext` |

## What is refused

Before anything starts, a URI must be absolute (`scheme:rest`), at most 8 KiB
long, and hold no control character and no whitespace. The schemes
`javascript:`, `vbscript:`, `data:` and `file:` are refused in any letter case,
and so is a one-letter scheme (on Windows `C:\…` is a drive letter). A local
file is opened with `open_path`, which takes a path and never a `file:` string
a message may have carried. Every other scheme is opened: an application that
shows links from elsewhere (chat messages, documents) chooses which schemes it
turns into links.

The error is an `OpenUriError`, which says which rule refused the URI, that
the desktop has no handler (`NoHandler`), or that the handler could not be
started (`Launch`).

## How each platform opens it

Never through a shell, always as one argument.

| Platform | How |
| --- | --- |
| Linux and the BSDs | `xdg-open`, which inside Flatpak goes through the `OpenURI` portal |
| macOS | `/usr/bin/open` |
| Windows | `ShellExecuteW` with the `open` verb |

On Linux and macOS the helper starts detached and a short-lived thread reaps
it; the call returns at once and reports only a failure to start it. What the
helper meets afterwards (no handler for the scheme, an unreadable file) it
writes on the application's standard error. On Windows the call returns once
the handler has started, and reports its failures.

## In tests, and to route it elsewhere

Register an `OpenUriHandle` in app-state and the context methods give it every
target they accept instead of starting the desktop's handler. A refused URI
never reaches it.

```rust
use std::{cell::RefCell, rc::Rc};
use teksilo::platform::open_uri::{OpenTarget, OpenUriHandle};

let opened = Rc::new(RefCell::new(Vec::<OpenTarget>::new()));
let sink = opened.clone();
let handle = OpenUriHandle::new(move |target| {
    sink.borrow_mut().push(target.clone());
    Ok(())
});
// TeksiloAppBuilder::new().app_state(handle) …
```

The free functions have no context and always use the desktop's handler.
