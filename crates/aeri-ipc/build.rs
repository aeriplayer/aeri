use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Require protoc at build time. Install it (e.g. `brew install protobuf` /
    // `apt install protobuf-compiler`) before adding RPC methods to player.proto.
    let protoc_available = Command::new("protoc")
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

    let protos = &["../../proto/aeri/v1/player.proto"];
    let includes = &["../../proto/"];

    buffa_build::Config::new()
        .files(protos)
        .includes(includes)
        .compile()?;

    connectrpc_build::Config::new()
        .files(protos)
        .includes(includes)
        .compile()?;

    Ok(())
}
