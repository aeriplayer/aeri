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
use std::process::Command;

use anyhow::{Context as _, Result, bail};
use serde_json::Value;

/// Runs both checks and reports every violation at once.
///
/// # Errors
///
/// Lists every violation found, or says why `deny.toml` or `cargo metadata` could not be
/// read.
pub fn check() -> Result<()> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("xtask has no parent directory")?
        .join("deny.toml");
    let deny: toml::Table = fs_err::read_to_string(&path)?
        .parse()
        .with_context(|| format!("could not parse {}", path.display()))?;

    let targets = string_list(&deny, &["graph", "targets"])?;
    let allowed = string_list(&deny, &["bans", "build", "allow-build-scripts"])?;

    let mut problems = BTreeSet::new();
    let mut with_build_scripts = BTreeSet::new();
    for target in &targets {
        let metadata = metadata_for(target)?;
        problems.extend(default_feature_violations(&metadata)?);
        with_build_scripts.extend(build_script_crates(&metadata)?);
    }
    problems.extend(
        stale_entries(&allowed, &with_build_scripts)
            .into_iter()
            .map(|name| {
                format!(
                    "deny.toml: `{name}` is in allow-build-scripts but no crate of that \
                     name has a build script; remove it"
                )
            }),
    );

    if problems.is_empty() {
        return Ok(());
    }
    bail!(
        "dependency policy violated:\n  {}",
        problems.into_iter().collect::<Vec<_>>().join("\n  ")
    );
}

/// The resolved graph for one platform, with every feature on, as cargo-deny sees it.
fn metadata_for(target: &str) -> Result<Value> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let output = Command::new(cargo)
        .args([
            "metadata",
            "--format-version=1",
            "--locked",
            "--all-features",
        ])
        .args(["--filter-platform", target])
        .output()
        .context("failed to start `cargo metadata`")?;
    if !output.status.success() {
        bail!(
            "`cargo metadata` failed for {target}:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    serde_json::from_slice(&output.stdout).context("`cargo metadata` printed invalid JSON")
}

/// Dependencies of workspace members that come from outside the workspace and keep their
/// default features.
fn default_feature_violations(metadata: &Value) -> Result<BTreeSet<String>> {
    let members = workspace_members(metadata)?;
    let mut violations = BTreeSet::new();
    for package in packages(metadata)? {
        if !members.contains(field_str(package, "id")?) {
            continue;
        }
        let name = field_str(package, "name")?;
        let dependencies = package["dependencies"]
            .as_array()
            .context("a package in `cargo metadata` has no `dependencies` array")?;
        for dependency in dependencies {
            let is_external = dependency["path"].is_null();
            if is_external && dependency["uses_default_features"].as_bool() != Some(false) {
                violations.insert(format!(
                    "{name}: `{}` keeps its default features; set `default-features = false` \
                     and list the features the code uses",
                    field_str(dependency, "name")?
                ));
            }
        }
    }
    Ok(violations)
}

/// Names of the crates in the graph that have a build script.
fn build_script_crates(metadata: &Value) -> Result<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    for package in packages(metadata)? {
        let targets = package["targets"]
            .as_array()
            .context("a package in `cargo metadata` has no `targets` array")?;
        let has_build_script = targets.iter().any(|target| {
            target["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "custom-build"))
        });
        if has_build_script {
            names.insert(field_str(package, "name")?.to_owned());
        }
    }
    Ok(names)
}

/// Allowlist entries that name no crate with a build script.
fn stale_entries<'a>(allowed: &'a [String], present: &BTreeSet<String>) -> Vec<&'a str> {
    allowed
        .iter()
        .filter(|name| !present.contains(*name))
        .map(String::as_str)
        .collect()
}

fn workspace_members(metadata: &Value) -> Result<BTreeSet<&str>> {
    metadata["workspace_members"]
        .as_array()
        .context("`cargo metadata` has no `workspace_members` array")?
        .iter()
        .map(|id| id.as_str().context("a workspace member id is not a string"))
        .collect()
}

fn packages(metadata: &Value) -> Result<&Vec<Value>> {
    metadata["packages"]
        .as_array()
        .context("`cargo metadata` has no `packages` array")
}

fn field_str<'a>(object: &'a Value, key: &str) -> Result<&'a str> {
    object[key]
        .as_str()
        .with_context(|| format!("`cargo metadata` entry has no string `{key}`"))
}

/// The array of strings at `path` in `table`; a missing key is an empty list.
fn string_list(table: &toml::Table, path: &[&str]) -> Result<Vec<String>> {
    let Some((last, parents)) = path.split_last() else {
        bail!("empty TOML path");
    };
    let mut current = table;
    for key in parents {
        match current.get(*key) {
            Some(toml::Value::Table(inner)) => current = inner,
            Some(_) => bail!("deny.toml: `{key}` is not a table"),
            None => return Ok(Vec::new()),
        }
    }
    let Some(value) = current.get(*last) else {
        return Ok(Vec::new());
    };
    value
        .as_array()
        .with_context(|| format!("deny.toml: `{}` is not an array", path.join(".")))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .with_context(|| format!("deny.toml: `{}` holds a non-string", path.join(".")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn metadata() -> Value {
        json!({
            "workspace_members": ["member-id"],
            "packages": [
                {
                    "id": "member-id",
                    "name": "member",
                    "targets": [{ "kind": ["lib"] }],
                    "dependencies": [
                        { "name": "lean", "path": null, "uses_default_features": false },
                        { "name": "heavy", "path": null, "uses_default_features": true },
                        { "name": "sibling", "path": "/ws/sibling", "uses_default_features": true }
                    ]
                },
                {
                    "id": "heavy-id",
                    "name": "heavy",
                    "targets": [{ "kind": ["lib"] }, { "kind": ["custom-build"] }],
                    "dependencies": [
                        { "name": "deep", "path": null, "uses_default_features": true }
                    ]
                }
            ]
        })
    }

    #[test]
    fn only_external_dependencies_of_members_with_defaults_are_reported() {
        let violations = default_feature_violations(&metadata()).unwrap();
        assert_eq!(violations.len(), 1);
        assert!(violations.first().unwrap().starts_with("member: `heavy`"));
    }

    #[test]
    fn crates_with_a_custom_build_target_have_build_scripts() {
        assert_eq!(
            build_script_crates(&metadata()).unwrap(),
            BTreeSet::from(["heavy".to_owned()])
        );
    }

    #[test]
    fn allowlist_entries_without_a_build_script_are_stale() {
        let present = BTreeSet::from(["heavy".to_owned()]);
        let allowed = ["heavy".to_owned(), "gone".to_owned()];
        assert_eq!(stale_entries(&allowed, &present), ["gone"]);
    }

    #[test]
    fn missing_tables_read_as_empty_lists() {
        let table: toml::Table = "[graph]\ntargets = [\"a\"]".parse().unwrap();
        assert_eq!(string_list(&table, &["graph", "targets"]).unwrap(), ["a"]);
        assert_eq!(
            string_list(&table, &["bans", "build", "allow-build-scripts"]).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_non_string_entry_is_an_error() {
        let table: toml::Table = "list = [1]".parse().unwrap();
        assert!(string_list(&table, &["list"]).is_err());
    }
}
