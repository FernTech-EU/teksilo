# Using bastyde-fmt in VS Code

This guide walks through wiring [`bastyde-fmt-lsp`](../crates/bastyde-fmt-lsp/)
into VS Code so `bati!` blocks reformat on save alongside `rust-analyzer`.

There are two practical paths:

- **Path A — no LSP, run-on-save:** invoke `cargo bastyde-fmt` from a
  shell hook on every save. Simplest setup, no extension authoring.
- **Path B — LSP via a tiny custom extension:** install `bastyde-fmt-lsp`
  and a ~30-line VS Code extension that registers it for Rust files.
  More moving parts, but you get the LSP capability surface (so it
  composes with rust-analyzer cleanly and obeys VS Code's standard
  format-on-save flow).

If you just want it to work today, do Path A. If you're already
authoring VS Code extensions or want format-on-save to participate in
the editor's normal formatter chain, do Path B.

---

## Path A — run-on-save

### 1. Install the CLI

```sh
cargo install --path crates/cargo-bastyde-fmt
```

Verify:

```sh
cargo bastyde-fmt --version
```

### 2. Install the *Run on Save* extension

Open the Extensions view (`Ctrl+Shift+X`), search for `emeraldwalk.runonsave`
("Run on Save" by emeraldwalk), and install it.

### 3. Configure the hook

Add this to your **workspace** `settings.json`
(`.vscode/settings.json` at the repo root):

```json
{
  "emeraldwalk.runonsave": {
    "commands": [
      {
        "match": "\\.rs$",
        "isAsync": true,
        "cmd": "cargo bastyde-fmt --quiet \"${file}\""
      }
    ]
  }
}
```

That's it. Save any `.rs` file and the hook runs `cargo bastyde-fmt` on
just that file. Files without `bati!` are skipped before parsing
(cheap string scan), so the cost on non-bati files is negligible.

### Notes

- The hook runs *after* VS Code's own formatter (rust-analyzer /
  rustfmt). That's the right ordering — rustfmt formats Rust, then
  bastyde-fmt formats bati! bodies, then the buffer is saved-then-
  reloaded by VS Code if the on-disk file changed.
- The async write is safe with `cargo bastyde-fmt`'s atomic-rename
  strategy — VS Code reloads the buffer when it sees the inode change.
- Use `${workspaceFolder}` instead of `${file}` if you want every
  save to format the entire workspace (slower; usually overkill).

---

## Path B — LSP via a custom extension

Use this if you want the formatter to participate in VS Code's normal
**Format Document** / **Format Document With…** UI, or if you're
already authoring a VS Code extension and want to bundle this in.

### 1. Install the LSP server binary

```sh
cargo install --path crates/bastyde-fmt-lsp
```

Verify the binary is on `PATH`:

```sh
which bastyde-fmt-lsp
bastyde-fmt-lsp --help 2>&1 | head -3 || true
```

(`bastyde-fmt-lsp` doesn't print help — it just speaks JSON-RPC on
stdio. The `which` check is enough.)

### 2. Scaffold the extension

Create a directory anywhere outside this repo (e.g.
`~/.vscode/bastyde-fmt-extension/`) with the following files.

#### `package.json`

```json
{
  "name": "bastyde-fmt-vscode",
  "displayName": "bastyde-fmt",
  "description": "Format bati! DSL blocks via bastyde-fmt-lsp",
  "version": "0.1.0",
  "publisher": "local",
  "engines": { "vscode": "^1.75.0" },
  "categories": ["Formatters"],
  "activationEvents": ["onLanguage:rust"],
  "main": "./out/extension.js",
  "contributes": {},
  "scripts": {
    "compile": "tsc -p ./",
    "watch": "tsc -watch -p ./"
  },
  "dependencies": {
    "vscode-languageclient": "^9.0.1"
  },
  "devDependencies": {
    "@types/node": "^20",
    "@types/vscode": "^1.75.0",
    "typescript": "^5.3.0"
  }
}
```

#### `tsconfig.json`

```json
{
  "compilerOptions": {
    "module": "commonjs",
    "target": "es2022",
    "outDir": "out",
    "lib": ["es2022"],
    "sourceMap": true,
    "strict": true
  },
  "include": ["src/**/*"]
}
```

#### `src/extension.ts`

```typescript
import * as vscode from 'vscode';
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from 'vscode-languageclient/node';

let client: LanguageClient | undefined;

export function activate(context: vscode.ExtensionContext) {
  const config = vscode.workspace.getConfiguration('bastydeFmt');
  const command = config.get<string>('serverPath') ?? 'bastyde-fmt-lsp';

  const serverOptions: ServerOptions = {
    run:   { command, transport: TransportKind.stdio },
    debug: { command, transport: TransportKind.stdio },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: 'file', language: 'rust' }],
  };

  client = new LanguageClient(
    'bastydeFmt',
    'bastyde-fmt LSP',
    serverOptions,
    clientOptions,
  );

  client.start();
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}
```

### 3. Build the extension

```sh
cd ~/.vscode/bastyde-fmt-extension
npm install
npm run compile
```

### 4. Sideload it

The simplest way is the **Run Extension** command:

1. Open the extension folder in VS Code.
2. Press `F5` to launch a development host with the extension loaded.

For a permanent install without packaging:

1. Symlink the folder into `~/.vscode/extensions/`:
   ```sh
   ln -s ~/.vscode/bastyde-fmt-extension ~/.vscode/extensions/local.bastyde-fmt-vscode-0.1.0
   ```
2. Restart VS Code.

For team-wide distribution, package with `vsce package` and share the
`.vsix` file (each developer runs `code --install-extension bastyde-fmt-vscode-0.1.0.vsix`).

### 5. Configure format-on-save

VS Code can run multiple formatters in sequence. Add this to your
workspace `settings.json` so rust-analyzer formats Rust first, then
bastyde-fmt formats `bati!` bodies:

```json
{
  "[rust]": {
    "editor.formatOnSave": true,
    "editor.defaultFormatter": "rust-lang.rust-analyzer",
    "editor.codeActionsOnSave": {
      "source.formatDocument": "explicit"
    }
  }
}
```

To explicitly invoke bastyde-fmt on demand, use the **Format Document
With…** command (`Ctrl+Shift+P` → "Format Document With…") and pick
*bastyde-fmt LSP*. To make bastyde-fmt the default formatter (instead of
rust-analyzer), change `editor.defaultFormatter` to `"local.bastyde-fmt-vscode"`.
Most users will want rust-analyzer as default and bastyde-fmt as a
secondary action.

### 6. (Optional) Custom server path

If `bastyde-fmt-lsp` isn't on `PATH`, point the extension at it:

```json
{
  "bastydeFmt.serverPath": "/home/you/.cargo/bin/bastyde-fmt-lsp"
}
```

The extension reads this on activation; reload the window after
changing it.

---

## Troubleshooting

**Nothing happens on save.**
First check that the binary works at all:

```sh
echo '' | bastyde-fmt-lsp
```

(It should sit waiting for input. Press `Ctrl+C` to exit.)

If that works, open VS Code's *Output* panel and select the
*bastyde-fmt LSP* channel — initialization errors and JSON-RPC traffic
land there.

**Format on save reformats things I didn't expect.**
The formatter has documented normalizations (empty bodies collapse,
`ctx =>` joins to root). Run `cargo bastyde-fmt --check src/your-file.rs`
from the terminal to see the exact diff before letting the editor
apply it.

**Format-on-save fights with rust-analyzer.**
With both formatters wired and `formatOnSave` true, VS Code runs only
the *default formatter*. To run both, either:

- Use Path A (run-on-save shell command) instead of the LSP — that
  runs *after* VS Code's own format pass.
- Add a code action that invokes both (`source.formatDocument` for
  rust-analyzer, then a custom command for bastyde-fmt). See VS Code's
  [code-actions-on-save](https://code.visualstudio.com/docs/editor/codebasics#_code-actions-on-save)
  docs.

**The extension says "bastyde-fmt-lsp not found".**
Set `bastydeFmt.serverPath` explicitly in settings (step 6) or add
`~/.cargo/bin` to your shell's `PATH` and restart VS Code.

---

## Why two paths?

Path A is a five-minute setup with no maintenance burden — `cargo
bastyde-fmt` is a self-contained tool, the *Run on Save* extension is
maintained by someone else, and there's nothing for you to keep
working as VS Code or LSP versions drift.

Path B integrates with VS Code's first-class formatter chain, but you
own a TypeScript extension. For most users — including a single
developer working on a personal project — Path A is the right
trade-off. Path B is worth it when you have multiple developers and
want the formatter discoverable through VS Code's standard UI rather
than needing every dev to install a third-party extension and edit
their settings.

Both paths use the same underlying formatter, produce the same output,
and can coexist (set up Path A as a fallback when the extension isn't
loaded, for example).

---

## Related

- [bastyde-fmt.md](bastyde-fmt.md) — full reference for the formatter,
  including library API, normalization rules, and architecture.
- [crates/bastyde-fmt-lsp/](../crates/bastyde-fmt-lsp/) — server source.
- [crates/cargo-bastyde-fmt/](../crates/cargo-bastyde-fmt/) — CLI source.
