//! Generates the IPC message types and service stubs from `proto/`.

use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Re-run when the proto file or protoc installation changes.
    println!("cargo:rerun-if-changed=../../proto/aeri/v1/player.proto");
    println!("cargo:rerun-if-env-changed=PROTOC");

    // Require protoc at build time. Install it (e.g. `brew install protobuf` /
    // `apt install protobuf-compiler`) before adding RPC methods to player.proto.
    let protoc = std::env::var("PROTOC").unwrap_or_else(|_| "protoc".into());
    let protoc_available = Command::new(&protoc)
        .arg("--version")
        .status()
        .is_ok_and(|s| s.success());

    if !protoc_available {
        println!(
            "cargo:warning=protoc not found — skipping IPC code generation. \
             Install protobuf-compiler and re-run `cargo xtask gen-proto`."
        );
        return Ok(());
    }

    let proto_files = &["../../proto/aeri/v1/player.proto"];
    let includes = &["../../proto/"];

    buffa_build::Config::new()
        .files(proto_files)
        .includes(includes)
        .compile()?;

    connectrpc_build::Config::new()
        .files(proto_files)
        .includes(includes)
        .compile()?;

    Ok(())
}
