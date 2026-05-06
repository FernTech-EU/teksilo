// Compile the .proto file into Rust modules at build time. Uses
// tonic-build (which wraps prost-build) so both client and server
// stubs are produced. Output lands in OUT_DIR and is included via
// `tonic::include_proto!` in lib.rs.
//
// Touching the .proto file triggers a rebuild via `cargo:rerun-if-
// changed=` directives that tonic-build emits.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Use the bundled protoc so the build doesn't depend on a
    // system-wide protobuf-compiler install. `protobuf-src` puts
    // the binary somewhere under target/ and gives us its path.
    // Honor a pre-set PROTOC so callers can opt into a system
    // protoc. On Windows the vendored abseil hits CRT-mismatch
    // link errors, so `protobuf-src` is excluded as a build-dep
    // there (see Cargo.toml) and PROTOC must be set.
    #[cfg(not(target_os = "windows"))]
    if std::env::var_os("PROTOC").is_none() {
        unsafe {
            std::env::set_var("PROTOC", protobuf_src::protoc());
        }
    }

    let proto_dir = std::path::Path::new("proto");
    let proto_file = proto_dir.join("telemetry/v1.proto");

    println!("cargo:rerun-if-changed={}", proto_file.display());

    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&[proto_file], &[proto_dir])?;

    Ok(())
}
