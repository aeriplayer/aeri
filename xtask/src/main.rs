//! Repository automation, run as `cargo xtask <task>`.

use std::process::Command;

use anyhow::{Context as _, Result, bail, ensure};

const USAGE: &str = "\
tasks:
  gen-proto      regenerate IPC code from proto/ (requires protoc)
  install-hooks  install the Git hooks defined in prek.toml (requires prek)
  install-tools  install the cargo subcommands the hooks and CI call
  release        not implemented yet; use `cargo dist build`";

/// Cargo subcommands the hooks in `prek.toml` invoke, as `(crate, version)`.
///
/// The only place their versions are written down: developers and CI install from this
/// list, so a hook never meets a different version in CI. They are not prek-managed
/// because prek would build them with its own, newer toolchain rather than the pinned one.
const TOOLS: &[(&str, &str)] = &[
    ("cargo-nextest", "0.9.152"),
    ("cargo-hack", "0.6.46"),
    ("cargo-llvm-cov", "0.9.1"),
    ("cargo-mutants", "27.1.0"),
];

fn main() -> Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("gen-proto") => gen_proto(),
        Some("install-hooks") => install_hooks(),
        Some("install-tools") => install_tools(),
        Some("release") => bail!("`release` is not implemented yet; run `cargo dist build`"),
        Some(other) => bail!("unknown task `{other}`\n\n{USAGE}"),
        None => bail!("no task given\n\n{USAGE}"),
    }
}

/// Installs the prek shims for every hook type listed in `prek.toml`.
fn install_hooks() -> Result<()> {
    // Older clones set `core.hooksPath`, which hides prek's shims. `--unset` exits
    // non-zero when the key is absent, so the status is deliberately not checked.
    Command::new("git")
        .args(["config", "--local", "--unset", "core.hooksPath"])
        .status()
        .context("failed to start `git`")?;

    run(Command::new("prek").args(["install", "--force", "--prepare-hooks"]))
        .context("could not install hooks; is prek installed? (`brew install prek`)")
}

/// Installs every tool in [`TOOLS`] at its pinned version.
///
/// Prefers `cargo binstall` (prebuilt binaries); falls back to `cargo install`.
fn install_tools() -> Result<()> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let have_binstall = Command::new(&cargo)
        .args(["binstall", "-V"])
        .output()
        .is_ok_and(|output| output.status.success());

    for (name, version) in TOOLS {
        let spec = format!("{name}@{version}");
        let mut command = Command::new(&cargo);
        if have_binstall {
            command.args(["binstall", "--no-confirm", "--locked", &spec]);
        } else {
            command.args(["install", "--locked", &spec]);
        }
        run(&mut command).with_context(|| format!("could not install {spec}"))?;
    }
    Ok(())
}

/// Rebuilds `aeri-ipc`, whose build script regenerates the IPC code when a `.proto`
/// file has changed.
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
