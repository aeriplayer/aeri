use std::process::Command;

fn main() {
    let task = std::env::args().nth(1).unwrap_or_default();
    match task.as_str() {
        "gen-proto" => gen_proto(),
        "install-hooks" => install_hooks(),
        "release" => {
            unimplemented!("release: wired up in Phase 1 via cargo-dist; run `cargo dist build`")
        }
        _ => {
            eprintln!(
                "unknown task: {task:?}\n\nknown tasks:\n  gen-proto\n  install-hooks\n  release"
            );
            std::process::exit(1);
        }
    }
}

fn install_hooks() {
    let status = Command::new("git")
        .args(["config", "core.hooksPath", ".githooks"])
        .status()
        .expect("failed to invoke git");
    if status.success() {
        println!("pre-commit hook installed (.githooks/)");
    } else {
        std::process::exit(status.code().unwrap_or(1));
    }
}

fn gen_proto() {
    // Build aeri-ipc, which triggers build.rs → buffa-build + connectrpc-build.
    // build.rs emits rerun-if-changed on the proto file, so editing player.proto
    // and re-running this command is the standard regeneration workflow.
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(cargo)
        .args(["build", "--package", "aeri-ipc"])
        .status()
        .expect("failed to invoke cargo");
    std::process::exit(status.code().unwrap_or(1));
}
