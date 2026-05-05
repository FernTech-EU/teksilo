# fern-ui

## Build

```bash
cargo build           # all crates
cargo test --workspace
```

See [`.claude/CLAUDE.md`](.claude/CLAUDE.md) for the full crate layout,
example demos, and tooling.

### Windows: PROTOC required for the gRPC analytics adapter

`fern-analytics-fern` pulls in `fern-collector-proto` (path-dep on the
sibling [`fern-collector`](../fern-collector) repo), which compiles the
`.proto` schema with `tonic-build`. On non-Windows hosts a vendored
`protoc` is built from source via the `protobuf-src` crate; on Windows
MSVC that vendored build hits a CRT-mismatch link failure, so
`protobuf-src` is excluded as a build-dep there and you must supply
your own `protoc`:

```powershell
winget install protobuf      # one-time, then refresh shell or use full path
$env:PROTOC = "C:\Path\To\protoc.exe"
cargo test --workspace
```

Linux and macOS need no setup — the vendored protoc kicks in. The
gating lives in
[`fern-collector/crates/fern-collector-proto/Cargo.toml`](../fern-collector/crates/fern-collector-proto/Cargo.toml).
