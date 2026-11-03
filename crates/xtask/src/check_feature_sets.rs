//! `cargo xtask check-feature-sets` — check pampa under every feature set a
//! consumer builds it with after turning its defaults off.
//!
//! The wasm consumers (`wasm-qmd-parser`, `wasm-quarto-hub-client`) are
//! excluded from the workspace and build pampa with `default-features =
//! false`, so a workspace build never compiles those feature sets. A
//! feature-gating mistake then passes PR CI and `verify`, and first fails in
//! the nightly/release wasm build — v0.34.0 broke exactly this way
//! (bd-yt3x1gwk). A native `cargo check -p pampa --no-default-features
//! [--features …]` per set catches that class in seconds, without a wasm
//! toolchain. It does not see `#[cfg(target_arch = "wasm32")]` code; the
//! quarto-hub-mcp parser tier (`QUARTO_QMD_PARSER_REQUIRED=1` in CI) covers
//! that.
//!
//! The sets are derived, not hardcoded: every `Cargo.toml` under `crates/`
//! that mentions pampa is read with `cargo metadata --no-deps`, which
//! resolves workspace inheritance and matches the dependency by package name
//! (so a renamed dependency is still found). A new consumer is picked up
//! automatically. Edges that keep pampa's defaults are skipped: the
//! workspace build already covers them.

use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::verify::find_project_root;

/// The crate whose consumer feature sets are checked.
const CHECKED_CRATE: &str = "pampa";

/// A pampa feature list (sorted, deduplicated) mapped to the consumers that
/// build pampa with exactly that list and no defaults.
pub type FeatureSets = BTreeMap<BTreeSet<String>, BTreeSet<String>>;

pub fn run(profile: Option<&str>) -> Result<()> {
    let root = find_project_root()?;
    // RUSTFLAGS is inherited from the caller's environment (CI sets
    // `-D warnings` on the step, like Clippy).
    check(&root, profile, None)
}

/// Discover the feature sets and `cargo check` pampa under each one.
/// `profile` selects the cargo profile (CI passes Clippy's `ci`, so the
/// checks reuse its dependency artifacts). `rustflags`, when given,
/// overrides `RUSTFLAGS` for the checks (verify passes its own so the checks
/// share its build cache).
pub fn check(root: &Path, profile: Option<&str>, rustflags: Option<&str>) -> Result<()> {
    let sets = discover_feature_sets(root)?;
    if sets.is_empty() {
        bail!(
            "found no consumer that builds {CHECKED_CRATE} with `default-features = false`; \
             the wasm crates under crates/ should be — has discovery broken?"
        );
    }
    for (features, consumers) in &sets {
        let args = check_args(features, profile);
        let consumers = consumers.iter().cloned().collect::<Vec<_>>().join(", ");
        println!("  ↳ cargo {} (for {consumers})", args.join(" "));
        let mut cmd = crate::util::nested_command("cargo");
        cmd.args(&args).current_dir(root);
        if let Some(flags) = rustflags {
            cmd.env("RUSTFLAGS", flags);
        }
        let status = cmd
            .status()
            .with_context(|| format!("Failed to run cargo {args:?}"))?;
        if !status.success() {
            bail!(
                "`cargo {}` failed: {CHECKED_CRATE} does not compile with the feature set \
                 used by {consumers}; gate the offending code on the feature it needs",
                args.join(" ")
            );
        }
    }
    println!("  ✓ {} {CHECKED_CRATE} feature set(s) clean", sets.len());
    Ok(())
}

/// The `cargo check` arguments for one feature set.
pub fn check_args(features: &BTreeSet<String>, profile: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "check".to_string(),
        "-p".to_string(),
        CHECKED_CRATE.to_string(),
        "--no-default-features".to_string(),
    ];
    if !features.is_empty() {
        args.push("--features".to_string());
        args.push(features.iter().cloned().collect::<Vec<_>>().join(","));
    }
    if let Some(profile) = profile {
        args.push("--profile".to_string());
        args.push(profile.to_string());
    }
    args
}

/// Read every candidate manifest under `root/crates` with `cargo metadata`
/// and collect the feature sets of no-default pampa dependencies.
pub fn discover_feature_sets(root: &Path) -> Result<FeatureSets> {
    let mut sets = FeatureSets::new();
    // `cargo metadata` on a workspace member reports the whole workspace, so
    // remember which manifests have been seen and skip them.
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    for manifest in candidate_manifests(root)? {
        if seen.contains(&manifest) {
            continue;
        }
        let metadata = cargo_metadata(&manifest)?;
        for path in package_manifest_paths(&metadata) {
            seen.insert(canonical(&path));
        }
        for (consumer, features) in no_default_edges(&metadata, CHECKED_CRATE) {
            sets.entry(features).or_default().insert(consumer);
        }
    }
    Ok(sets)
}

/// Manifests under `root/crates` whose text mentions the checked crate,
/// excluding the checked crate's own manifest. Hidden, `target` and
/// `node_modules` directories are skipped.
fn candidate_manifests(root: &Path) -> Result<Vec<PathBuf>> {
    let own = canonical(&root.join("crates").join(CHECKED_CRATE).join("Cargo.toml"));
    let mut manifests = Vec::new();
    for entry in walkdir::WalkDir::new(root.join("crates"))
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| {
            e.file_name()
                .to_str()
                .is_none_or(|s| !s.starts_with('.') && s != "target" && s != "node_modules")
        })
    {
        let entry = entry.context("Failed to read directory entry")?;
        if entry.file_name() != "Cargo.toml" {
            continue;
        }
        let path = canonical(entry.path());
        if path == own {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        if text.contains(CHECKED_CRATE) {
            manifests.push(path);
        }
    }
    Ok(manifests)
}

fn cargo_metadata(manifest: &Path) -> Result<serde_json::Value> {
    let output = crate::util::nested_command("cargo")
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(manifest)
        .output()
        .context("Failed to run cargo metadata")?;
    if !output.status.success() {
        bail!(
            "cargo metadata failed for {}:\n{}",
            manifest.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    serde_json::from_slice(&output.stdout)
        .with_context(|| format!("parsing cargo metadata for {}", manifest.display()))
}

fn package_manifest_paths(metadata: &serde_json::Value) -> Vec<PathBuf> {
    packages(metadata)
        .filter_map(|p| p["manifest_path"].as_str().map(PathBuf::from))
        .collect()
}

/// `(consumer package name, feature set)` for each dependency on `krate`
/// (matched by package name, any kind) that turns default features off.
fn no_default_edges(metadata: &serde_json::Value, krate: &str) -> Vec<(String, BTreeSet<String>)> {
    let mut edges = Vec::new();
    for package in packages(metadata) {
        let consumer = package["name"].as_str().unwrap_or("<unnamed>");
        let deps = package["dependencies"].as_array().into_iter().flatten();
        for dep in deps {
            if dep["name"].as_str() != Some(krate) || dep["uses_default_features"] != false {
                continue;
            }
            let features = dep["features"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|f| f.as_str().map(str::to_string))
                .collect();
            edges.push((consumer.to_string(), features));
        }
    }
    edges
}

fn packages(metadata: &serde_json::Value) -> impl Iterator<Item = &serde_json::Value> {
    metadata["packages"].as_array().into_iter().flatten()
}

/// Canonicalize for comparing walkdir paths with `cargo metadata`'s, which
/// may differ by symlinks (e.g. macOS `/private/tmp`). Falls back to the
/// path as given.
fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn set(features: &[&str]) -> BTreeSet<String> {
        features.iter().map(|f| f.to_string()).collect()
    }

    fn metadata(packages: serde_json::Value) -> serde_json::Value {
        json!({ "packages": packages })
    }

    fn dep(name: &str, uses_default_features: bool, features: &[&str]) -> serde_json::Value {
        json!({
            "name": name,
            "uses_default_features": uses_default_features,
            "features": features,
            "rename": null,
            "kind": null,
        })
    }

    #[test]
    fn no_default_edges_keeps_only_no_default_deps_on_the_crate() {
        let m = metadata(json!([
            { "name": "parser", "dependencies": [dep("pampa", false, &[])] },
            { "name": "hub", "dependencies": [dep("pampa", false, &["lua-filter"])] },
            { "name": "cli", "dependencies": [dep("pampa", true, &[])] },
            { "name": "other", "dependencies": [dep("serde", false, &[])] },
        ]));
        assert_eq!(
            no_default_edges(&m, "pampa"),
            vec![
                ("parser".to_string(), set(&[])),
                ("hub".to_string(), set(&["lua-filter"])),
            ]
        );
    }

    #[test]
    fn no_default_edges_matches_renamed_and_dev_dependencies() {
        let mut renamed = dep("pampa", false, &["json-filter"]);
        renamed["rename"] = json!("pp");
        let mut dev = dep("pampa", false, &[]);
        dev["kind"] = json!("dev");
        let m = metadata(json!([
            { "name": "renamer", "dependencies": [renamed] },
            { "name": "tester", "dependencies": [dev] },
        ]));
        assert_eq!(
            no_default_edges(&m, "pampa"),
            vec![
                ("renamer".to_string(), set(&["json-filter"])),
                ("tester".to_string(), set(&[])),
            ]
        );
    }

    #[test]
    fn feature_order_does_not_split_a_set() {
        let a = metadata(json!([
            { "name": "a", "dependencies": [dep("pampa", false, &["lua-filter", "json-filter"])] },
        ]));
        let b = metadata(json!([
            { "name": "b", "dependencies": [dep("pampa", false, &["json-filter", "lua-filter"])] },
        ]));
        let mut sets = FeatureSets::new();
        for m in [&a, &b] {
            for (consumer, features) in no_default_edges(m, "pampa") {
                sets.entry(features).or_default().insert(consumer);
            }
        }
        assert_eq!(sets.len(), 1);
        assert_eq!(sets[&set(&["json-filter", "lua-filter"])], set(&["a", "b"]));
    }

    #[test]
    fn check_args_without_and_with_features() {
        assert_eq!(
            check_args(&set(&[]), None),
            ["check", "-p", "pampa", "--no-default-features"]
        );
        assert_eq!(
            check_args(&set(&["lua-filter", "json-filter"]), Some("ci")),
            [
                "check",
                "-p",
                "pampa",
                "--no-default-features",
                "--features",
                "json-filter,lua-filter",
                "--profile",
                "ci"
            ]
        );
    }

    /// Discovery against the real repo must find the two wasm consumers'
    /// sets. Guards against discovery silently finding nothing (e.g. a
    /// walk or metadata regression), which would turn the check into a
    /// no-op.
    #[test]
    fn discovers_the_wasm_consumers_in_this_repo() {
        let root = Path::new(&*std::env::var("CARGO_MANIFEST_DIR").unwrap().leak()).join("../..");
        let sets = discover_feature_sets(&root).unwrap();
        assert!(
            sets.get(&set(&[]))
                .is_some_and(|c| c.contains("wasm-qmd-parser")),
            "{sets:?}"
        );
        assert!(
            sets.get(&set(&["lua-filter"]))
                .is_some_and(|c| c.contains("wasm-quarto-hub-client")),
            "{sets:?}"
        );
    }
}
