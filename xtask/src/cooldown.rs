//! The crate cooldown (`cargo xtask crate-cooldown`, ADR 0028): no locked crate version may
//! be younger than the policy's `cooldown-days`, so a compromised release has time to be
//! noticed before it is adopted.
//!
//! Publication times come from the crates.io sparse index, which records a `pubtime` on every
//! version. The index files are fetched with `curl`.

use std::collections::{BTreeMap, BTreeSet};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result, bail};
use serde::Deserialize;

use crate::workspace_root;

/// The policy, compiled in like `.config/cargo-tools.toml`.
const POLICY_TOML: &str = include_str!("../../.config/crate-cooldown.toml");

/// Lockfiles to check, relative to the workspace root. `fuzz/` is its own workspace.
const LOCKFILES: [&str; 2] = ["Cargo.lock", "fuzz/Cargo.lock"];

const INDEX_URL: &str = "https://index.crates.io";

/// `Cargo.lock` names the registry in either of these forms.
const REGISTRY_SOURCES: [&str; 2] = [
    "registry+https://github.com/rust-lang/crates.io-index",
    "sparse+https://index.crates.io/",
];

/// How many index files are fetched at once.
const BATCH: usize = 16;

const SECONDS_PER_DAY: i64 = 86_400;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct Policy {
    cooldown_days: u32,
    exclusions: Vec<Exclusion>,
}

/// A version let through for a security fix.
#[derive(Debug, Deserialize)]
struct Exclusion {
    name: String,
    version: String,
    /// Required so every exclusion is explained; nothing reads it.
    #[serde(rename = "reason")]
    _reason: String,
}

#[derive(Debug, Deserialize)]
struct Lockfile {
    #[serde(default)]
    package: Vec<LockedPackage>,
}

#[derive(Debug, Deserialize)]
struct LockedPackage {
    name: String,
    version: String,
    source: Option<String>,
}

/// One line of an index file: a published version.
#[derive(Debug, Deserialize)]
struct IndexLine {
    vers: String,
    pubtime: Option<String>,
}

/// Fails if any version locked from the registry is younger than the policy allows.
pub(crate) fn check() -> Result<()> {
    let policy: Policy =
        toml::from_str(POLICY_TOML).context("could not parse .config/crate-cooldown.toml")?;
    let root = workspace_root()?;

    let mut locked = BTreeSet::new();
    for file in LOCKFILES {
        let text = fs_err::read_to_string(root.join(file))?;
        locked.extend(registry_packages(&text).with_context(|| format!("could not read {file}"))?);
    }

    let names: BTreeSet<&str> = locked.iter().map(|(name, _)| name.as_str()).collect();
    let indexes = fetch_indexes(&names)?;
    let now = unix_now()?;

    let problems = young_releases(&locked, &indexes, &policy, now)?;
    if problems.is_empty() {
        return Ok(());
    }
    bail!(
        "locked crates younger than {} days:\n  {}\nhold one back with `cargo update --precise \
         <older version> <crate>`; for a security fix, add an exclusion to \
         .config/crate-cooldown.toml",
        policy.cooldown_days,
        problems.join("\n  ")
    );
}

/// The `(name, version)` of every package a lockfile takes from crates.io.
fn registry_packages(lock_text: &str) -> Result<BTreeSet<(String, String)>> {
    let lock: Lockfile = toml::from_str(lock_text)?;
    Ok(lock
        .package
        .into_iter()
        .filter(|package| {
            package
                .source
                .as_deref()
                .is_some_and(|source| REGISTRY_SOURCES.contains(&source))
        })
        .map(|package| (package.name, package.version))
        .collect())
}

/// Where the sparse index keeps a crate: sharded by the first characters of its name.
fn index_path(name: &str) -> String {
    let name = name.to_ascii_lowercase();
    match name.len() {
        0..=2 => format!("{}/{name}", name.len()),
        3 => format!("3/{}/{name}", name.get(..1).unwrap_or_default()),
        _ => format!(
            "{}/{}/{name}",
            name.get(..2).unwrap_or_default(),
            name.get(2..4).unwrap_or_default()
        ),
    }
}

/// Fetches each crate's index file with `curl`, sixteen at a time.
fn fetch_indexes(names: &BTreeSet<&str>) -> Result<BTreeMap<String, String>> {
    let names: Vec<&str> = names.iter().copied().collect();
    let mut indexes = BTreeMap::new();
    for batch in names.chunks(BATCH) {
        let children = batch
            .iter()
            .map(|name| {
                Command::new("curl")
                    .args(["--fail", "--silent", "--show-error", "--location"])
                    .args(["--retry", "3"])
                    .arg(format!("{INDEX_URL}/{}", index_path(name)))
                    .stdout(Stdio::piped())
                    .spawn()
                    .context("failed to start `curl`; the cooldown check needs it")
            })
            .collect::<Result<Vec<_>>>()?;
        for (name, child) in batch.iter().zip(children) {
            let output = child.wait_with_output()?;
            if !output.status.success() {
                bail!("could not fetch the index entry of `{name}`");
            }
            indexes.insert((*name).to_owned(), String::from_utf8(output.stdout)?);
        }
    }
    Ok(indexes)
}

/// The publication time, in seconds since the epoch, of each version in an index file.
/// A version with no `pubtime` maps to `None`: the field is newer than most crates.
fn publication_times(index_text: &str) -> Result<BTreeMap<String, Option<i64>>> {
    let mut times = BTreeMap::new();
    for line in index_text.lines().filter(|line| !line.trim().is_empty()) {
        let entry: IndexLine = serde_json::from_str(line).context("malformed index line")?;
        let published = match entry.pubtime {
            Some(stamp) => Some(
                parse_utc(&stamp)
                    .with_context(|| format!("unreadable publication time `{stamp}`"))?,
            ),
            None => None,
        };
        times.insert(entry.vers, published);
    }
    Ok(times)
}

/// One message per locked version published less than the policy's days before `now`.
fn young_releases(
    locked: &BTreeSet<(String, String)>,
    indexes: &BTreeMap<String, String>,
    policy: &Policy,
    now: i64,
) -> Result<Vec<String>> {
    let minimum = i64::from(policy.cooldown_days) * SECONDS_PER_DAY;
    let mut problems = Vec::new();
    for (name, version) in locked {
        let excluded = policy
            .exclusions
            .iter()
            .any(|e| e.name == *name && e.version == *version);
        if excluded {
            continue;
        }
        let index = indexes
            .get(name)
            .with_context(|| format!("no index entry was fetched for `{name}`"))?;
        let times = publication_times(index).with_context(|| format!("index of `{name}`"))?;
        match times.get(version) {
            None => problems.push(format!("{name} {version} is not in its index entry")),
            Some(None) => {}
            Some(Some(published)) => {
                let age = now - published;
                if age < minimum {
                    problems.push(format!(
                        "{name} {version} was published {} hours ago",
                        age.max(0) / 3600
                    ));
                }
            }
        }
    }
    Ok(problems)
}

/// Seconds since the epoch of a UTC timestamp like `2026-07-19T12:40:53Z`; `None` if it is
/// not in that form.
fn parse_utc(stamp: &str) -> Option<i64> {
    let (date, time) = stamp.strip_suffix('Z')?.split_once('T')?;
    let mut date = date.splitn(3, '-').map(|part| part.parse::<i64>().ok());
    let (year, month, day) = (date.next()??, date.next()??, date.next()??);
    let mut time = time
        .split('.')
        .next()?
        .splitn(3, ':')
        .map(|part| part.parse::<i64>().ok());
    let (hour, minute, second) = (time.next()??, time.next()??, time.next()??);

    let valid = (1..=12).contains(&month)
        && (1..=31).contains(&day)
        && (0..24).contains(&hour)
        && (0..60).contains(&minute)
        && (0..=60).contains(&second);
    valid.then(|| {
        days_from_civil(year, month, day) * SECONDS_PER_DAY + hour * 3600 + minute * 60 + second
    })
}

/// Days from 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn unix_now() -> Result<i64> {
    let elapsed = SystemTime::now().duration_since(UNIX_EPOCH)?;
    Ok(i64::try_from(elapsed.as_secs())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = SECONDS_PER_DAY;

    fn policy(days: u32, exclusions: &str) -> Policy {
        toml::from_str(&format!(
            "cooldown-days = {days}\nexclusions = [{exclusions}]"
        ))
        .unwrap()
    }

    fn line(version: &str, pubtime: Option<&str>) -> String {
        let pubtime = pubtime.map_or(String::new(), |p| format!(r#","pubtime":"{p}""#));
        format!(r#"{{"name":"a","vers":"{version}","deps":[],"yanked":false{pubtime}}}"#)
    }

    fn no_problems() -> Vec<String> {
        Vec::new()
    }

    fn locked(version: &str) -> BTreeSet<(String, String)> {
        BTreeSet::from([("a".to_owned(), version.to_owned())])
    }

    fn index(lines: &[String]) -> BTreeMap<String, String> {
        BTreeMap::from([("a".to_owned(), lines.join("\n"))])
    }

    #[test]
    fn the_repository_policy_parses() {
        let policy: Policy = toml::from_str(POLICY_TOML).unwrap();
        assert!(policy.cooldown_days >= 7);
    }

    #[test]
    fn timestamps_convert_to_epoch_seconds() {
        assert_eq!(parse_utc("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_utc("2000-03-01T00:00:00Z"), Some(951_868_800));
        assert_eq!(parse_utc("2001-09-09T01:46:40Z"), Some(1_000_000_000));
        assert_eq!(parse_utc("2024-02-29T00:00:00Z"), Some(1_709_164_800));
        assert_eq!(parse_utc("2001-09-09T01:46:40.123Z"), Some(1_000_000_000));
    }

    #[test]
    fn malformed_timestamps_are_refused() {
        for bad in [
            "",
            "2001-09-09",
            "2001-09-09T01:46:40",
            "2001-13-09T01:46:40Z",
            "2001-09-32T01:46:40Z",
            "2001-09-09T24:00:00Z",
            "2001-09-09T01:60:00Z",
            "yesterday",
        ] {
            assert_eq!(parse_utc(bad), None, "{bad}");
        }
    }

    #[test]
    fn index_paths_are_sharded_by_name_length() {
        assert_eq!(index_path("a"), "1/a");
        assert_eq!(index_path("cc"), "2/cc");
        assert_eq!(index_path("cfg"), "3/c/cfg");
        assert_eq!(index_path("fastrand"), "fa/st/fastrand");
        assert_eq!(index_path("Serde"), "se/rd/serde");
    }

    #[test]
    fn only_registry_packages_are_checked() {
        let lock = r#"
            [[package]]
            name = "aeri-core"
            version = "0.1.0"

            [[package]]
            name = "serde"
            version = "1.0.0"
            source = "registry+https://github.com/rust-lang/crates.io-index"

            [[package]]
            name = "forked"
            version = "0.2.0"
            source = "git+https://example.com/forked#abc"
        "#;
        assert_eq!(
            registry_packages(lock).unwrap(),
            BTreeSet::from([("serde".to_owned(), "1.0.0".to_owned())])
        );
    }

    #[test]
    fn a_version_exactly_at_the_cooldown_passes_and_one_second_short_fails() {
        let published = "2026-01-01T00:00:00Z";
        let at = parse_utc(published).unwrap();
        let index = index(&[line("1.0.0", Some(published))]);
        let policy = policy(7, "");

        let old_enough = young_releases(&locked("1.0.0"), &index, &policy, at + 7 * DAY);
        assert_eq!(old_enough.unwrap(), no_problems());

        let too_young = young_releases(&locked("1.0.0"), &index, &policy, at + 7 * DAY - 1);
        assert_eq!(too_young.unwrap(), ["a 1.0.0 was published 167 hours ago"]);
    }

    #[test]
    fn only_the_locked_version_is_judged() {
        let index = index(&[
            line("1.0.0", Some("2026-01-01T00:00:00Z")),
            line("2.0.0", Some("2026-01-30T00:00:00Z")),
        ]);
        let now = parse_utc("2026-02-01T00:00:00Z").unwrap();
        let found = young_releases(&locked("1.0.0"), &index, &policy(7, ""), now);
        assert_eq!(found.unwrap(), no_problems());
    }

    #[test]
    fn an_exclusion_lets_exactly_its_version_through() {
        let index = index(&[line("1.0.0", Some("2026-01-01T00:00:00Z"))]);
        let now = parse_utc("2026-01-02T00:00:00Z").unwrap();
        let excluded = policy(
            7,
            r#"{ name = "a", version = "1.0.0", reason = "fixes a vulnerability" }"#,
        );
        assert_eq!(
            young_releases(&locked("1.0.0"), &index, &excluded, now).unwrap(),
            no_problems()
        );

        let other_version = policy(
            7,
            r#"{ name = "a", version = "1.0.1", reason = "fixes a vulnerability" }"#,
        );
        assert_eq!(
            young_releases(&locked("1.0.0"), &index, &other_version, now)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn a_version_without_a_publication_time_is_old() {
        let index = index(&[line("1.0.0", None)]);
        let found = young_releases(&locked("1.0.0"), &index, &policy(7, ""), 0);
        assert_eq!(found.unwrap(), no_problems());
    }

    #[test]
    fn a_locked_version_missing_from_its_index_is_reported() {
        let index = index(&[line("1.0.0", None)]);
        let found = young_releases(&locked("9.9.9"), &index, &policy(7, ""), 0).unwrap();
        assert_eq!(found, ["a 9.9.9 is not in its index entry"]);
    }

    #[test]
    fn an_unreadable_publication_time_is_an_error_not_a_pass() {
        let index = index(&[line("1.0.0", Some("last week"))]);
        assert!(young_releases(&locked("1.0.0"), &index, &policy(7, ""), 0).is_err());
    }

    #[test]
    fn a_crate_with_no_fetched_index_is_an_error() {
        let found = young_releases(&locked("1.0.0"), &BTreeMap::new(), &policy(7, ""), 0);
        assert!(found.is_err());
    }

    #[test]
    fn a_release_from_the_future_counts_as_young() {
        let index = index(&[line("1.0.0", Some("2026-01-02T00:00:00Z"))]);
        let before = parse_utc("2026-01-01T00:00:00Z").unwrap();
        let found = young_releases(&locked("1.0.0"), &index, &policy(7, ""), before).unwrap();
        assert_eq!(found, ["a 1.0.0 was published 0 hours ago"]);
    }
}
