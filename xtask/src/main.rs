//! Repository automation, run as `cargo xtask <task>`.

use std::process::Command;

use anyhow::{Context, Result, bail, ensure};

const USAGE: &str = "\
tasks:
  gen-proto      regenerate IPC code from proto/ (requires protoc)
  install-hooks  install the Git hooks defined in prek.toml (requires prek)
  release        not implemented yet; use `cargo dist build`";

fn main() -> Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("gen-proto") => gen_proto(),
        Some("install-hooks") => install_hooks(),
        Some("release") => bail!("`release` is not implemented yet; run `cargo dist build`"),
        Some(other) => bail!("unknown task `{other}`\n\n{USAGE}"),
        None => bail!("no task given\n\n{USAGE}"),
    }
}

/// Installs the prek shims for every hook type listed in `prek.toml`.
fn install_hooks() -> Result<()> {
    // Earlier clones pointed `core.hooksPath` at `.githooks/`. Git would keep using
    // that directory and never reach the shims, so clear it first. `--unset` exits
    // non-zero when the key is already absent, which is the state we want.
    Command::new("git")
        .args(["config", "--local", "--unset", "core.hooksPath"])
        .status()
        .context("failed to start `git`")?;

    run(Command::new("prek").args(["install", "--force", "--prepare-hooks"]))
        .context("could not install hooks; is prek installed? (`brew install prek`)")
}

/// Rebuilds `aeri-ipc`, whose build script generates the IPC code.
///
/// The build script declares `rerun-if-changed` on the `.proto` files, so editing
/// one and running this task is the whole regeneration workflow.
fn gen_proto() -> Result<()> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    run(Command::new(cargo).args(["build", "--package", "aeri-ipc"]))
}

/// Runs `command` to completion, turning a non-zero exit into an error.
fn run(command: &mut Command) -> Result<()> {
    let program = command.get_program().to_string_lossy().into_owned();
    let status = command
        .status()
        .with_context(|| format!("failed to start `{program}`"))?;
    ensure!(status.success(), "`{program}` exited with {status}");
    Ok(())
}
