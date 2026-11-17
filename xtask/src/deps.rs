//! Dependency policy that cargo-deny cannot express (`cargo xtask check-deps`).
//!
//! - Every external dependency a workspace crate declares turns default features off and
//!   names the features it uses, so nothing is compiled that the code did not ask for.
//! - Every entry in `deny.toml`'s `bans.build.allow-build-scripts` still names a crate with
//!   a build script, so the allowlist cannot accumulate stale permissions.
//!
//! Both are read from `cargo metadata` for each platform in `deny.toml`'s `[graph]`, the
//! same dependency graph cargo-deny checks.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context as _, Result, bail};
use cargo_metadata::{CargoOpt, Metadata, MetadataCommand};
use serde::Deserialize;

use crate::workspace_root;

/// The parts of `deny.toml` this check reads; cargo-deny owns the rest.
#[derive(Debug, Deserialize)]
struct DenyConfig {
    graph: Graph,
    #[serde(default)]
    bans: Bans,
}

#[derive(Debug, Deserialize)]
struct Graph {
    targets: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct Bans {
    #[serde(default)]
    build: Build,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Build {
    #[serde(default)]
    allow_build_scripts: Vec<String>,
}

/// A dependency as a workspace crate declares it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Declared {
    dependant: String,
    dependency: String,
    external: bool,
    default_features: bool,
}

/// Runs both checks and reports every violation at once.
pub(crate) fn check() -> Result<()> {
    let root = workspace_root()?;
    let deny_path = root.join("deny.toml");
    let deny: DenyConfig = toml::from_str(&fs_err::read_to_string(&deny_path)?)
        .with_context(|| format!("could not parse {}", deny_path.display()))?;

    let mut declared = BTreeSet::new();
    let mut with_build_scripts = BTreeSet::new();
    for target in &deny.graph.targets {
        let metadata = metadata_for(root, target)?;
        declared.extend(declared_dependencies(&metadata));
        with_build_scripts.extend(build_script_crates(&metadata));
    }

    let stale =
        stale_entries(&deny.bans.build.allow_build_scripts, &with_build_scripts).map(|name| {
            format!(
                "deny.toml: `{name}` is in allow-build-scripts but no crate of that name has \
                 a build script; remove it"
            )
        });
    let problems: Vec<String> = default_feature_violations(&declared).chain(stale).collect();
    if problems.is_empty() {
        return Ok(());
    }
    bail!("dependency policy violated:\n  {}", problems.join("\n  "));
}

/// The resolved graph for one platform, with every feature on, as cargo-deny sees it.
fn metadata_for(root: &Path, target: &str) -> Result<Metadata> {
    MetadataCommand::new()
        .current_dir(root)
        .features(CargoOpt::AllFeatures)
        .other_options(["--locked", "--filter-platform", target].map(String::from))
        .exec()
        .with_context(|| format!("`cargo metadata` failed for {target}"))
}

fn declared_dependencies(metadata: &Metadata) -> impl Iterator<Item = Declared> {
    metadata
        .workspace_packages()
        .into_iter()
        .flat_map(|package| {
            package.dependencies.iter().map(|dependency| Declared {
                dependant: package.name.to_string(),
                dependency: dependency.name.clone(),
                external: dependency.path.is_none(),
                default_features: dependency.uses_default_features,
            })
        })
}

fn build_script_crates(metadata: &Metadata) -> impl Iterator<Item = String> {
    metadata
        .packages
        .iter()
        .filter(|package| {
            package
                .targets
                .iter()
                .any(cargo_metadata::Target::is_custom_build)
        })
        .map(|package| package.name.to_string())
}

/// External dependencies declared with their default features left on.
fn default_feature_violations(declared: &BTreeSet<Declared>) -> impl Iterator<Item = String> {
    declared
        .iter()
        .filter(|d| d.external && d.default_features)
        .map(|d| {
            format!(
                "{}: `{}` keeps its default features; set `default-features = false` and \
                 list the features the code uses",
                d.dependant, d.dependency
            )
        })
}

/// Allowlist entries that name no crate with a build script.
fn stale_entries<'a>(
    allowed: &'a [String],
    with_build_scripts: &'a BTreeSet<String>,
) -> impl Iterator<Item = &'a str> {
    allowed
        .iter()
        .filter(|name| !with_build_scripts.contains(*name))
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared(dependency: &str, external: bool, default_features: bool) -> Declared {
        Declared {
            dependant: "member".into(),
            dependency: dependency.into(),
            external,
            default_features,
        }
    }

    #[test]
    fn only_external_dependencies_with_defaults_are_reported() {
        let all = BTreeSet::from([
            declared("lean", true, false),
            declared("heavy", true, true),
            declared("sibling", false, true),
        ]);
        let violations: Vec<_> = default_feature_violations(&all).collect();
        assert_eq!(violations.len(), 1);
        assert!(violations[0].starts_with("member: `heavy`"));
    }

    #[test]
    fn allowlist_entries_without_a_build_script_are_stale() {
        let present = BTreeSet::from(["heavy".to_owned()]);
        let allowed = ["heavy".to_owned(), "gone".to_owned()];
        assert_eq!(
            stale_entries(&allowed, &present).collect::<Vec<_>>(),
            ["gone"]
        );
    }

    #[test]
    fn a_deny_config_without_a_build_section_allows_no_build_scripts() {
        let config: DenyConfig =
            toml::from_str("[graph]\ntargets = [\"a\"]\n[bans]\nwildcards = \"deny\"").unwrap();
        assert_eq!(config.graph.targets, ["a"]);
        assert_eq!(config.bans.build.allow_build_scripts, Vec::<String>::new());
    }

    #[test]
    fn the_repository_deny_config_parses() {
        let config: DenyConfig = toml::from_str(include_str!("../../deny.toml")).unwrap();
        assert!(
            config
                .graph
                .targets
                .iter()
                .any(|t| t == "aarch64-apple-darwin")
        );
    }
}
