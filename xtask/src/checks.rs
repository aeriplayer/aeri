//! The multi-command checks the manual hooks and CI run (ADR 0028): coverage, mutation
//! testing, Miri, the loom model and fuzzing.

use std::process::Command;

use anyhow::{Context as _, Result, bail, ensure};

use crate::{cargo, run, workspace_root};

/// Coverage floors, as `cargo llvm-cov` arguments: one test run, then a report per floor.
const COVERAGE_STEPS: [&[&str]; 4] = [
    &[
        "llvm-cov",
        "nextest",
        "--workspace",
        "--all-features",
        "--locked",
        "--no-report",
    ],
    &[
        "llvm-cov",
        "report",
        "--ignore-filename-regex",
        r"(^|/)(xtask/|build\.rs$)",
        "--fail-under-lines",
        "85",
    ],
    &[
        "llvm-cov",
        "report",
        "--package",
        "aeri-core",
        "--fail-under-lines",
        "100",
    ],
    &[
        "llvm-cov",
        "report",
        "--package",
        "aeri-dsp",
        "--fail-under-lines",
        "100",
    ],
];

/// The packages Miri leaves out (ADR 0029): `xtask` has no `unsafe`, and the engine, the player and
/// the CLI contain neither `unsafe` nor floating-point arithmetic, so Miri has nothing to find
/// there, and it would cost about a minute per test that plays audio.
const MIRI_EXCLUDED: [&str; 4] = ["xtask", "aeri-engine", "aeri-player", "aeri-cli"];

/// Fuzz targets with the directory of seed inputs each starts from, relative to `fuzz/`.
const FUZZ_TARGETS: [(&str, &str); 2] = [
    ("decode", "../crates/aeri-decode/tests/fixtures"),
    ("m3u", "../crates/aeri-player/tests/fixtures"),
];

const DEFAULT_MUTANTS_BASE: &str = "origin/main";
const DEFAULT_FUZZ_SECONDS: u32 = 60;

/// Runs the tests once, then fails if any coverage floor is missed.
pub(crate) fn coverage() -> Result<()> {
    let root = workspace_root()?;
    for step in COVERAGE_STEPS {
        run(cargo().current_dir(root).args(step))?;
    }
    Ok(())
}

/// Mutation-tests the lines changed since `MUTANTS_BASE` (default `origin/main`).
pub(crate) fn mutants() -> Result<()> {
    let root = workspace_root()?;
    let base = std::env::var("MUTANTS_BASE").unwrap_or_else(|_| DEFAULT_MUTANTS_BASE.into());

    let diff = Command::new("git")
        .current_dir(root)
        .args(["diff", &format!("{base}...HEAD")])
        .output()
        .context("failed to start `git`")?;
    ensure!(
        diff.status.success(),
        "`git diff {base}...HEAD` failed: {}",
        String::from_utf8_lossy(&diff.stderr).trim()
    );
    fs_err::create_dir_all(root.join("target"))?;
    fs_err::write(root.join("target/mutants.diff"), &diff.stdout)?;

    run(cargo().current_dir(root).args([
        "mutants",
        "--in-diff",
        "target/mutants.diff",
        "--no-shuffle",
    ]))
}

/// Runs the tests under Miri. Isolation is off so tests can read the committed fixtures, proptest's
/// regression files are off, and it runs few cases, so property tests run rather than being skipped.
pub(crate) fn miri() -> Result<()> {
    run(cargo()
        .current_dir(workspace_root()?)
        .env("MIRIFLAGS", "-Zmiri-disable-isolation")
        .env("PROPTEST_DISABLE_FAILURE_PERSISTENCE", "true")
        .env("PROPTEST_CASES", "8")
        .args(["+nightly", "miri", "test", "--workspace", "--all-features"])
        .args(MIRI_EXCLUDED.iter().flat_map(|name| ["--exclude", name])))
}

/// Runs the loom model of the real-time bridge, in its own target directory because
/// `--cfg loom` rebuilds every dependency.
pub(crate) fn loom() -> Result<()> {
    run(cargo()
        .current_dir(workspace_root()?)
        .env("RUSTFLAGS", "--cfg loom")
        .env("CARGO_TARGET_DIR", "target/loom")
        .args([
            "test",
            "--package",
            "aeri-audio",
            "--lib",
            "--release",
            "--locked",
            "loom_model",
        ]))
}

/// Fuzzes each target for `FUZZ_SECONDS` (default 60). Runs from `fuzz/`, whose own
/// `rust-toolchain.toml` selects the pinned nightly (ADR 0015).
pub(crate) fn fuzz() -> Result<()> {
    let dir = workspace_root()?.join("fuzz");
    let seconds = fuzz_seconds(std::env::var("FUZZ_SECONDS").ok().as_deref())?;
    let host = host_triple(&dir)?;

    for (target, seeds) in FUZZ_TARGETS {
        let corpus = format!("corpus/{target}");
        fs_err::create_dir_all(dir.join(&corpus))?;
        run(cargo()
            .current_dir(&dir)
            .args(fuzz_args(&host, target, &corpus, seeds, seconds)))?;
    }
    Ok(())
}

/// The arguments of one `cargo fuzz run`: libFuzzer's own flags follow the `--`.
fn fuzz_args(host: &str, target: &str, corpus: &str, seeds: &str, seconds: u32) -> Vec<String> {
    [
        "fuzz",
        "run",
        "--target",
        host,
        target,
        corpus,
        seeds,
        "--",
        &format!("-max_total_time={seconds}"),
        "-timeout=10",
    ]
    .map(String::from)
    .to_vec()
}

/// The seconds each target runs: `value` is `FUZZ_SECONDS`, if it is set.
fn fuzz_seconds(value: Option<&str>) -> Result<u32> {
    value.map_or(Ok(DEFAULT_FUZZ_SECONDS), |text| {
        text.parse()
            .with_context(|| format!("FUZZ_SECONDS must be a whole number, not `{text}`"))
    })
}

/// The compiler's host triple, as the compiler in `dir` reports it.
fn host_triple(dir: &std::path::Path) -> Result<String> {
    let output = Command::new("rustc")
        .current_dir(dir)
        .arg("-vV")
        .output()
        .context("failed to start `rustc`")?;
    ensure!(
        output.status.success(),
        "`rustc -vV` failed in {}",
        dir.display()
    );
    let text = String::from_utf8_lossy(&output.stdout);
    match parse_host(&text) {
        Some(host) => Ok(host.to_owned()),
        None => bail!("`rustc -vV` printed no `host:` line"),
    }
}

/// The value of the `host:` line of `rustc -vV`.
fn parse_host(version_output: &str) -> Option<&str> {
    version_output
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_is_read_from_rustc_version_output() {
        let text = "rustc 1.100.0\nbinary: rustc\nhost: aarch64-apple-darwin\nrelease: 1.100.0\n";
        assert_eq!(parse_host(text), Some("aarch64-apple-darwin"));
        assert_eq!(parse_host("rustc 1.100.0\n"), None);
    }

    #[test]
    fn fuzz_seconds_defaults_and_parses() {
        assert_eq!(fuzz_seconds(None).unwrap(), 60);
        assert_eq!(fuzz_seconds(Some("600")).unwrap(), 600);
        assert!(fuzz_seconds(Some("a minute")).is_err());
    }

    #[test]
    fn libfuzzer_flags_come_after_the_separator() {
        let args = fuzz_args("host", "decode", "corpus/decode", "../seeds", 5);
        let separator = args.iter().position(|a| a == "--").unwrap();
        assert_eq!(args[separator + 1], "-max_total_time=5");
        assert_eq!(&args[..3], ["fuzz", "run", "--target"]);
        assert!(args[..separator].contains(&"corpus/decode".to_owned()));
    }

    #[test]
    fn every_coverage_step_is_a_llvm_cov_command() {
        assert!(COVERAGE_STEPS.iter().all(|step| step[0] == "llvm-cov"));
        assert_eq!(COVERAGE_STEPS.len(), 4);
    }

    #[test]
    fn miri_leaves_out_only_crates_that_exist() {
        let manifest =
            fs_err::read_to_string(workspace_root().unwrap().join("Cargo.toml")).unwrap();
        for name in MIRI_EXCLUDED {
            let member = if name == "xtask" {
                "\"xtask\"".to_owned()
            } else {
                format!("crates/{name}\"")
            };
            assert!(
                manifest.contains(&member),
                "{name} is not a workspace member"
            );
        }
    }

    #[test]
    fn every_fuzz_target_has_seeds_in_the_repository() {
        let fuzz_dir = workspace_root().unwrap().join("fuzz");
        for (_, seeds) in FUZZ_TARGETS {
            assert!(fuzz_dir.join(seeds).is_dir(), "{seeds} is missing");
        }
    }
}
