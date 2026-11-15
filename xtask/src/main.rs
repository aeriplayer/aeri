//! Repository automation, run as `cargo xtask <task>`.

mod deps;

use std::collections::BTreeMap;
use std::process::Command;

use anyhow::{Context as _, Result, bail, ensure};

const USAGE: &str = "\
tasks:
  install-hooks  install the Git hooks in .pre-commit-config.yaml (requires prek)
  install-tools  install the cargo subcommands the hooks and CI call
                 (all of them, or only the names given: `install-tools cargo-nextest`)
  check-deps     dependency policy cargo-deny cannot express: explicit features,
                 no stale build-script permissions
  release        not implemented yet; use `dist build`";

/// The cargo subcommands the hooks and CI invoke, as `crate = "version"`.
///
/// The only place their versions are written down: developers and CI install from this
/// file, so a hook never meets a different version in CI. They are not prek-managed
/// because prek would build them with its own, newer toolchain rather than the pinned one.
const TOOLS_TOML: &str = include_str!("../../.config/cargo-tools.toml");

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("install-hooks") => install_hooks(),
        Some("install-tools") => install_tools(&args.collect::<Vec<_>>()),
        Some("check-deps") => deps::check(),
        Some("release") => bail!("`release` is not implemented yet; run `dist build`"),
        Some(other) => bail!("unknown task `{other}`\n\n{USAGE}"),
        None => bail!("no task given\n\n{USAGE}"),
    }
}

/// Installs the prek shims for every hook type listed in `.pre-commit-config.yaml`.
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

/// Installs the named tools from [`TOOLS_TOML`] at their pinned versions, or all of them
/// when `requested` is empty.
///
/// Prefers `cargo binstall` (prebuilt binaries); falls back to `cargo install`.
fn install_tools(requested: &[String]) -> Result<()> {
    let tools = pinned_tools()?;
    let selected = select_tools(&tools, requested)?;

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let have_binstall = Command::new(&cargo)
        .args(["binstall", "-V"])
        .output()
        .is_ok_and(|output| output.status.success());

    for (name, version) in selected {
        let spec = format!("{name}@{version}");
        let mut command = Command::new(&cargo);
        if have_binstall {
            command.args(["binstall", "--no-confirm", "--locked", &spec]);
        } else {
            command.args(["install", "--locked", &spec]);
        }
        run_with_retries(&mut command, INSTALL_ATTEMPTS)
            .with_context(|| format!("could not install {spec}"))?;
    }
    Ok(())
}

/// Release lookups on shared CI runners time out now and then; a second try usually works.
const INSTALL_ATTEMPTS: u32 = 3;

/// Runs `command` up to `attempts` times, pausing between tries, and returns the last error.
fn run_with_retries(command: &mut Command, attempts: u32) -> Result<()> {
    let mut result = run(command);
    for _ in 1..attempts {
        if result.is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(5));
        result = run(command);
    }
    result
}

/// The crate-to-version table in [`TOOLS_TOML`].
fn pinned_tools() -> Result<BTreeMap<String, String>> {
    toml::from_str(TOOLS_TOML).context("could not parse .config/cargo-tools.toml")
}

/// Picks the entries of `tools` named in `requested`, or all of them when it is empty.
fn select_tools<'a>(
    tools: &'a BTreeMap<String, String>,
    requested: &[String],
) -> Result<Vec<(&'a str, &'a str)>> {
    if let Some(unknown) = requested.iter().find(|name| !tools.contains_key(*name)) {
        bail!("unknown tool `{unknown}`; known tools are in .config/cargo-tools.toml");
    }
    Ok(tools
        .iter()
        .filter(|(name, _)| requested.is_empty() || requested.contains(name))
        .map(|(name, version)| (name.as_str(), version.as_str()))
        .collect())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pinned_tools_file_parses() {
        assert!(pinned_tools().unwrap().contains_key("cargo-nextest"));
    }

    #[test]
    fn no_names_selects_every_tool() {
        let tools = pinned_tools().unwrap();
        assert_eq!(select_tools(&tools, &[]).unwrap().len(), tools.len());
    }

    #[test]
    fn names_select_only_those_tools() {
        let tools = pinned_tools().unwrap();
        let picked = select_tools(&tools, &["cargo-hack".into()]).unwrap();
        assert_eq!(
            picked.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
            ["cargo-hack"]
        );
    }

    #[test]
    fn unknown_name_is_an_error() {
        let tools = pinned_tools().unwrap();
        assert!(select_tools(&tools, &["cargo-nope".into()]).is_err());
    }
}
