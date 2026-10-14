/*
 * extension/build.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Build a TypeScript engine extension bundle (rollup, run under Node).
 */

//! Build logic for `q2 call build-ts-extension` — build a TypeScript engine
//! extension bundle with rollup.
//!
//! Extension authors run this after editing TS source; `q2` never runs it
//! during render. It resolves an alias config by the precedence rules in
//! [`resolve_build_config`] and runs the embedded bundler driver
//! (`ts-packages/quarto-ts-bundle`) under Node — the same Node 24 that runs
//! TS engine extensions, so building adds no requirement. No Deno, no npx and
//! no network.
//!
//! Native-only (mirrors `engine::ts_process`): spawns Node via
//! `std::process`, unavailable on `wasm32-unknown-unknown`.
//!
//! The bundle is ESM for `node >= 24`, with `node:*` imports left external.
//! The same file runs under q2's Node host and Quarto 1's Deno 2.x. Imports
//! that only exist under Deno (`jsr:`, `@std/*`) are not resolved: they fail
//! with a message naming the `node:` replacement.
//!
//! # Config precedence
//!
//! The config is a JSON file `{"alias": {"<import>": "<path>"}}` (named
//! `esbuild.json` for historical reasons; the format is bundler-neutral);
//! relative paths resolve against the config file's directory.
//!
//! 1. `--config <path>` (explicit override) — always wins.
//! 2. `esbuild.json` committed in the extension directory.
//! 3. Workspace — when `workspace_root` is `Some` (auto-detected or
//!    `--workspace`): `@quarto/api` and `@quarto/types` alias to the
//!    workspace's `ts-packages/*/src`, so extensions that import values from
//!    them (`@quarto/api/claims`) build from source.
//! 4. Shipped template — embedded at compile time (`SHIPPED_ESBUILD_JSON`).
//!    Installed binaries land here when tiers 1-3 are absent. It declares no
//!    aliases: `@quarto/*` is not yet published, so a *value* import of it
//!    fails with an actionable message (type-only imports are erased and
//!    need nothing).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// Options for building an extension's TypeScript engine bundle.
#[derive(Debug)]
pub struct BuildOptions {
    /// Path to the extension directory or `_extension.yml`. Defaults to cwd.
    pub ext_dir: Option<PathBuf>,
    /// Explicit `--config` override; wins over all other config sources.
    pub config: Option<PathBuf>,
    /// Force use of the workspace tier (in-repo / pre-publish build).
    pub workspace: bool,
    /// Use this q2 workspace root for the workspace tier instead of walking
    /// up from the extension (for extensions built outside the workspace,
    /// e.g. in a temp dir).
    pub workspace_root: Option<PathBuf>,
}

// ============================================================================
// Config resolution
// ============================================================================

/// Which config tier [`resolve_build_config`] selected. Pure data: no I/O
/// happens until [`load_aliases`] reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    /// Tier 1/2: a user-written `esbuild.json` (explicit or extension-local).
    File(PathBuf),
    /// Tier 3: aliases derived from this q2 workspace root.
    Workspace(PathBuf),
    /// Tier 4: the embedded shipped template.
    Shipped,
}

/// Resolves which alias config applies. Pure: callers probe the filesystem
/// for tiers 1-3 and pass the results.
///
/// 1. `explicit` wins unconditionally.
/// 2. `ext_esbuild_json` — `Some` when an `esbuild.json` exists in the
///    extension directory.
/// 3. `workspace_root` — `Some` when a q2 workspace was detected (or
///    `--workspace` forced).
/// 4. Otherwise the shipped template.
pub fn resolve_build_config(
    explicit: Option<&Path>,
    ext_esbuild_json: Option<&Path>,
    workspace_root: Option<&Path>,
) -> ConfigSource {
    if let Some(p) = explicit {
        return ConfigSource::File(p.to_path_buf());
    }
    if let Some(p) = ext_esbuild_json {
        return ConfigSource::File(p.to_path_buf());
    }
    if let Some(root) = workspace_root {
        return ConfigSource::Workspace(root.to_path_buf());
    }
    ConfigSource::Shipped
}

/// The shipped `resources/extension-build/esbuild.json` template (repo root),
/// embedded at compile time. The release tarball is single-member (just the
/// `q2` binary), so there is no adjacent `resources/` directory to probe.
const SHIPPED_ESBUILD_JSON: &str =
    include_str!("../../../../resources/extension-build/esbuild.json");

/// The on-disk shape of an `esbuild.json`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EsbuildConfigFile {
    #[serde(default)]
    alias: BTreeMap<String, String>,
}

/// Parse an `esbuild.json`; relative alias targets resolve against `base`.
fn parse_config(text: &str, base: &Path, what: &str) -> Result<BTreeMap<String, PathBuf>> {
    let file: EsbuildConfigFile =
        serde_json::from_str(text).with_context(|| format!("Invalid {what}"))?;
    Ok(file
        .alias
        .into_iter()
        .map(|(name, target)| {
            let target = PathBuf::from(target);
            let target = if target.is_absolute() {
                target
            } else {
                base.join(target)
            };
            (name, target)
        })
        .collect())
}

/// Read the import aliases a [`ConfigSource`] stands for.
pub fn load_aliases(source: &ConfigSource) -> Result<BTreeMap<String, PathBuf>> {
    match source {
        ConfigSource::File(path) => {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            let base = path.parent().unwrap_or_else(|| Path::new("."));
            parse_config(&text, base, &path.display().to_string())
        }
        ConfigSource::Workspace(root) => {
            let src = |pkg: &str| root.join("ts-packages").join(pkg).join("src");
            Ok(BTreeMap::from([
                ("@quarto/api".to_string(), src("quarto-api")),
                ("@quarto/types".to_string(), src("quarto-types")),
            ]))
        }
        ConfigSource::Shipped => {
            parse_config(SHIPPED_ESBUILD_JSON, Path::new("."), "shipped esbuild.json")
        }
    }
}

// ============================================================================
// Workspace detection
// ============================================================================

/// Walk up from `start_dir` looking for a directory that contains
/// `ts-packages/quarto-api` (the marker for a q2 source clone).
///
/// Returns `None` when no such directory is found (i.e. we are in an
/// installed-binary context where the packages are not present).
pub fn find_workspace_root(start_dir: &Path) -> Option<PathBuf> {
    let mut dir = start_dir.to_path_buf();
    loop {
        if dir.join("ts-packages").join("quarto-api").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

// ============================================================================
// Command handler
// ============================================================================

/// Builds the extension's `dist/*.js` bundle; returns the resolved output path.
pub fn build_ts_extension(opts: BuildOptions) -> Result<PathBuf> {
    // Locate the extension directory (containing _extension.yml).
    let ext_dir = resolve_extension_dir(opts.ext_dir.as_deref())?;

    // Auto-detect workspace root (walk up from ext_dir).
    let workspace_root = if let Some(root) = &opts.workspace_root {
        Some(root.clone())
    } else if opts.workspace {
        // --workspace: walk up unconditionally.
        Some(find_workspace_root(&ext_dir).with_context(|| {
            format!(
                "--workspace was passed but no q2 workspace root (dir containing \
                         ts-packages/quarto-api) was found walking up from {}",
                ext_dir.display()
            )
        })?)
    } else {
        find_workspace_root(&ext_dir)
    };

    // Probe for an extension-local esbuild.json. A leftover deno.json is no
    // longer read; say so rather than silently ignoring it.
    let ext_esbuild = Some(ext_dir.join("esbuild.json")).filter(|p| p.exists());
    if ext_esbuild.is_none() && opts.config.is_none() && ext_dir.join("deno.json").exists() {
        eprintln!(
            "warning: {} is no longer read; q2 now bundles with rollup. \
             Add an esbuild.json ({{\"alias\": {{...}}}}) if the extension needs import aliases.",
            ext_dir.join("deno.json").display()
        );
    }

    let source = resolve_build_config(
        opts.config.as_deref(),
        ext_esbuild.as_deref(),
        workspace_root.as_deref(),
    );
    let aliases = load_aliases(&source)?;

    let entry_ts = find_entry_ts(&ext_dir)?;
    let output_js = find_output_path(&ext_dir)?;

    run_bundler(&ext_dir, &aliases, &source, &entry_ts, &output_js)?;
    Ok(output_js)
}

// ============================================================================
// Extension directory / entry point helpers
// ============================================================================

/// Resolves `path` to the extension directory.
///
/// - If `path` points directly to `_extension.yml`, use its parent.
/// - If `path` points to a directory, use it directly.
/// - If `path` is `None`, scan the current working directory.
fn resolve_extension_dir(path: Option<&Path>) -> Result<PathBuf> {
    let dir = match path {
        Some(p) if p.file_name().is_some_and(|n| n == "_extension.yml") => p
            .parent()
            .map_or_else(|| PathBuf::from("."), |d| d.to_path_buf()),
        Some(p) => p.to_path_buf(),
        None => std::env::current_dir().context("Failed to get current working directory")?,
    };

    // The bundler runs with `current_dir = dir`, so every path derived from it
    // (entry, output, aliases) must not depend on the caller's cwd.
    let dir = std::path::absolute(&dir)
        .with_context(|| format!("Failed to resolve {}", dir.display()))?;

    // Verify _extension.yml exists in the resolved directory.
    let yml = dir.join("_extension.yml");
    if !yml.exists() {
        bail!(
            "No _extension.yml found in {}. \
             Pass the extension directory or _extension.yml path explicitly.",
            dir.display()
        );
    }

    Ok(dir)
}

/// Finds the TypeScript entry point for the extension.
///
/// Convention: `src/<name>.ts` where `<name>` is the extension directory's
/// last component (the extension name).
fn find_entry_ts(ext_dir: &Path) -> Result<PathBuf> {
    let name = ext_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("extension");
    let candidate = ext_dir.join("src").join(format!("{}.ts", name));
    if candidate.exists() {
        return Ok(candidate);
    }
    // Fall back: any .ts in src/.
    let src_dir = ext_dir.join("src");
    if src_dir.is_dir() {
        for entry in std::fs::read_dir(&src_dir)
            .with_context(|| format!("Failed to read {}", src_dir.display()))?
        {
            let entry = entry?;
            if entry.path().extension().is_some_and(|e| e == "ts") {
                return Ok(entry.path());
            }
        }
    }
    bail!(
        "No TypeScript entry point found. Expected src/{}.ts inside {}. \
         Create the file or use --config to override the build config.",
        name,
        ext_dir.display()
    )
}

/// Reads the output JS path from the first engine contribution in
/// `_extension.yml`.
fn find_output_path(ext_dir: &Path) -> Result<PathBuf> {
    let yml_path = ext_dir.join("_extension.yml");
    let content = std::fs::read_to_string(&yml_path)
        .with_context(|| format!("Failed to read {}", yml_path.display()))?;

    // Parse with serde_yaml; we only need the `contributes.engines[0].path` field.
    let value: serde_yaml::Value = serde_yaml::from_str(&content)
        .with_context(|| format!("Invalid YAML in {}", yml_path.display()))?;

    let path_str = value
        .get("contributes")
        .and_then(|c| c.get("engines"))
        .and_then(|e| e.as_sequence())
        .and_then(|seq| seq.first())
        .and_then(|entry| entry.get("path"))
        .and_then(|p| p.as_str())
        .with_context(|| {
            format!(
                "Could not find contributes.engines[0].path in {}. \
                 The extension must declare a pre-built .js path.",
                yml_path.display()
            )
        })?;

    Ok(ext_dir.join(path_str))
}

// ============================================================================
// Bundler runner
// ============================================================================

/// Strip the Windows verbatim prefix (`\\?\C:\...`) that
/// `Path::canonicalize` adds. Bundlers cannot resolve alias targets that carry
/// it (esbuild: "Could not resolve ... using the alias feature"). UNC verbatim paths
/// (`\\?\UNC\...`) and every non-verbatim path are returned unchanged.
pub fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

/// The bundler driver, built from `ts-packages/quarto-ts-bundle` by
/// `cargo xtask build-ts-bundle` (committed; CI checks it is fresh). One
/// self-contained ESM file, piped to `node -`: nothing to extract or cache.
static BUNDLER_DRIVER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ts-packages/quarto-ts-bundle/dist/ts-bundle.mjs"
));

/// What `BUNDLER_DRIVER` needs to know, passed as JSON in this variable
/// (an environment variable has no quoting or length trouble that argv has
/// for a driver read from stdin).
const JOB_ENV: &str = "Q2_TS_BUNDLE_JOB";

/// The `BundleJob` of `ts-packages/quarto-ts-bundle/src/bundle.ts`. The driver
/// runs with `cwd = ext_dir` and a relative `entry`.
fn bundle_job(entry: &Path, output: &Path, aliases: &BTreeMap<String, PathBuf>) -> Result<String> {
    let text = |p: &Path| {
        p.to_str()
            .map(str::to_owned)
            .with_context(|| format!("Path is not valid UTF-8: {}", p.display()))
    };
    let aliases = aliases
        .iter()
        .map(|(name, target)| Ok((name.clone(), text(&strip_verbatim_prefix(target))?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    Ok(serde_json::json!({
        "entry": text(entry)?,
        "output": text(output)?,
        "aliases": aliases,
    })
    .to_string())
}

/// Turn the bundler's stderr into guidance when it names a known migration problem.
fn explain_failure(stderr: &str, source: &ConfigSource) -> Option<String> {
    if stderr.contains("\"jsr:") || stderr.contains("\"@std/") || stderr.contains("\"npm:") {
        return Some(
            "`jsr:`, `npm:` and `@std/*` imports are Deno-only and are not resolved. \
             Use the `node:` equivalent (node:path, node:fs, node:process, \
             node:child_process, ...) or install the package with npm."
                .to_string(),
        );
    }
    if matches!(source, ConfigSource::Shipped) && stderr.contains("\"@quarto/") {
        return Some(
            "`@quarto/api` and `@quarto/types` are not published yet, so an installed q2 \
             cannot resolve value imports from them. Build inside the q2 workspace \
             (or pass --workspace), or import only types (`import type`), which need no resolution."
                .to_string(),
        );
    }
    if stderr.contains("is not exported by") {
        return Some(
            "Types are erased without type-checking, so an import that names only a type \
             must say so: `import type { T } from \"...\"` or `import { type T } from \"...\"`."
                .to_string(),
        );
    }
    None
}

fn run_bundler(
    ext_dir: &Path,
    aliases: &BTreeMap<String, PathBuf>,
    source: &ConfigSource,
    entry: &Path,
    output: &Path,
) -> Result<()> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    // Ensure the output parent directory exists.
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output directory {}", parent.display()))?;
    }

    let node = quarto_node_discovery::find_node(
        &quarto_node_discovery::Discovery::from_env(),
        "`q2 call build-ts-extension`",
    )?;
    let relative_entry = entry.strip_prefix(ext_dir).unwrap_or(entry);
    let job = bundle_job(relative_entry, output, aliases)?;

    // `--disable-warning`: the driver strips types with Node's built-in
    // `module.stripTypeScriptTypes`, which prints an ExperimentalWarning.
    let mut child = Command::new(&node.path)
        .args([
            "--disable-warning=ExperimentalWarning",
            "--input-type=module",
            "-",
        ])
        .current_dir(ext_dir)
        .env(JOB_ENV, job)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to run Node at {}", node.path.display()))?;
    // Node reads the whole script before running it, so it writes nothing until
    // this is done. A write error means Node exited early; its status and
    // stderr, collected below, say why.
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let _ = stdin.write_all(BUNDLER_DRIVER.as_bytes());
    drop(stdin);
    let result = child
        .wait_with_output()
        .context("Failed to wait for the bundler")?;

    // The driver reports problems on stderr; pass them through either way so
    // warnings are not lost.
    let stderr = String::from_utf8_lossy(&result.stderr);
    if !stderr.is_empty() {
        eprint!("{stderr}");
    }
    if !result.status.success() {
        let hint = explain_failure(&stderr, source)
            .map(|h| format!("\n{h}"))
            .unwrap_or_default();
        bail!(
            "The bundler exited with status {}. Check the TypeScript source for errors.{hint}",
            result.status
        );
    }

    println!("Built: {} → {}", entry.display(), output.display());
    Ok(())
}

// ============================================================================
// Unit tests
// ============================================================================

#[cfg(test)]
mod tests {
    #[test]
    fn strip_verbatim_prefix_handles_windows_forms() {
        let strip = |p: &str| strip_verbatim_prefix(Path::new(p));
        assert_eq!(
            strip(r"\\?\C:\Users\me\q2\src"),
            PathBuf::from(r"C:\Users\me\q2\src")
        );
        assert_eq!(
            strip(r"\\?\UNC\server\share\x"),
            PathBuf::from(r"\\?\UNC\server\share\x")
        );
        assert_eq!(strip("/home/me/q2"), PathBuf::from("/home/me/q2"));
        assert_eq!(strip(r"C:\plain"), PathBuf::from(r"C:\plain"));
    }

    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ---- helpers --------------------------------------------------------

    fn make_dir() -> TempDir {
        tempfile::tempdir().expect("create tempdir")
    }

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, "").unwrap();
    }

    // ---- resolve_build_config tests ------------------------------------

    /// Explicit --config wins over everything else.
    #[test]
    fn explicit_config_wins() {
        let explicit = PathBuf::from("/x/custom.json");
        let source = resolve_build_config(
            Some(&explicit),
            Some(Path::new("/x/ext/esbuild.json")),
            Some(Path::new("/x/workspace")),
        );
        assert_eq!(source, ConfigSource::File(explicit));
    }

    /// Extension-dir esbuild.json wins over workspace and shipped.
    #[test]
    fn ext_dir_esbuild_json_wins_over_workspace() {
        let ext = PathBuf::from("/x/ext/esbuild.json");
        let source = resolve_build_config(None, Some(&ext), Some(Path::new("/x/workspace")));
        assert_eq!(source, ConfigSource::File(ext));
    }

    /// The workspace tier is selected when no explicit/ext config exists.
    #[test]
    fn workspace_selected_when_no_ext_config() {
        let root = PathBuf::from("/x/workspace");
        assert_eq!(
            resolve_build_config(None, None, Some(&root)),
            ConfigSource::Workspace(root)
        );
    }

    /// The shipped template is the final fallback.
    #[test]
    fn shipped_template_is_final_fallback() {
        assert_eq!(
            resolve_build_config(None, None, None),
            ConfigSource::Shipped
        );
    }

    /// `--config` wins with no workspace root (the installed-binary case).
    #[test]
    fn config_wins_without_workspace() {
        let explicit = PathBuf::from("/x/custom.json");
        assert_eq!(
            resolve_build_config(Some(&explicit), None, None),
            ConfigSource::File(explicit)
        );
    }

    // ---- load_aliases tests --------------------------------------------

    #[test]
    fn workspace_aliases_point_at_package_sources() {
        let root = PathBuf::from("/ws");
        let aliases = load_aliases(&ConfigSource::Workspace(root.clone())).unwrap();
        assert_eq!(
            aliases["@quarto/api"],
            root.join("ts-packages/quarto-api/src")
        );
        assert_eq!(
            aliases["@quarto/types"],
            root.join("ts-packages/quarto-types/src")
        );
    }

    #[test]
    fn shipped_template_parses_and_declares_no_aliases() {
        assert!(load_aliases(&ConfigSource::Shipped).unwrap().is_empty());
    }

    #[test]
    fn file_aliases_resolve_relative_to_the_config_dir() {
        let dir = make_dir();
        let config = dir.path().join("esbuild.json");
        fs::write(
            &config,
            r#"{"alias": {"lib": "./vendor/lib.ts", "abs": "/already/absolute"}}"#,
        )
        .unwrap();
        let aliases = load_aliases(&ConfigSource::File(config)).unwrap();
        assert_eq!(aliases["lib"], dir.path().join("./vendor/lib.ts"));
        assert_eq!(aliases["abs"], PathBuf::from("/already/absolute"));
    }

    #[test]
    fn unknown_config_keys_are_rejected() {
        let dir = make_dir();
        let config = dir.path().join("esbuild.json");
        fs::write(&config, r#"{"alais": {}}"#).unwrap();
        let err = load_aliases(&ConfigSource::File(config)).unwrap_err();
        assert!(format!("{err:#}").contains("alais"), "got: {err:#}");
    }

    #[test]
    fn missing_config_file_is_an_error() {
        let dir = make_dir();
        assert!(load_aliases(&ConfigSource::File(dir.path().join("nope.json"))).is_err());
    }

    // ---- bundler job / failure hints -----------------------------------

    #[test]
    fn bundle_job_carries_entry_output_and_aliases() {
        let aliases = BTreeMap::from([("@quarto/api".to_string(), PathBuf::from("/ws/api"))]);
        let job = bundle_job(Path::new("src/x.ts"), Path::new("/o/x.js"), &aliases).unwrap();
        let job: serde_json::Value = serde_json::from_str(&job).unwrap();
        assert_eq!(
            job,
            serde_json::json!({
                "entry": "src/x.ts",
                "output": "/o/x.js",
                "aliases": {"@quarto/api": "/ws/api"},
            })
        );
    }

    #[test]
    fn deno_only_imports_get_a_node_hint() {
        let hint = explain_failure(
            r#"error: Could not resolve "jsr:@std/path" (imported by src/x.ts)"#,
            &ConfigSource::Shipped,
        )
        .unwrap();
        assert!(hint.contains("node:"), "got: {hint}");
    }

    #[test]
    fn quarto_value_imports_in_the_shipped_tier_name_the_workspace() {
        let stderr = r#"error: Could not resolve "@quarto/api/claims" (imported by src/x.ts)"#;
        let hint = explain_failure(stderr, &ConfigSource::Shipped).unwrap();
        assert!(hint.contains("--workspace"), "got: {hint}");
        assert!(explain_failure(stderr, &ConfigSource::Workspace(PathBuf::from("/ws"))).is_none());
    }

    #[test]
    fn a_type_imported_as_a_value_gets_an_import_type_hint() {
        let stderr = r#"error: "Shape" is not exported by "src/types.ts", imported by "src/x.ts"."#;
        let hint = explain_failure(stderr, &ConfigSource::Shipped).unwrap();
        assert!(hint.contains("import type"), "got: {hint}");
    }

    // ---- find_workspace_root tests ------------------------------------

    /// Detects workspace root from a nested directory.
    #[test]
    fn find_workspace_root_detects_ts_packages() {
        let dir = make_dir();
        let api_dir = dir.path().join("ts-packages").join("quarto-api");
        fs::create_dir_all(&api_dir).unwrap();

        let nested = dir.path().join("some").join("nested").join("dir");
        fs::create_dir_all(&nested).unwrap();

        let found = find_workspace_root(&nested);
        assert_eq!(found, Some(dir.path().to_path_buf()));
    }

    /// Returns None when ts-packages/quarto-api is absent.
    #[test]
    fn find_workspace_root_returns_none_when_absent() {
        let dir = make_dir();
        let nested = dir.path().join("deep").join("dir");
        fs::create_dir_all(&nested).unwrap();

        assert!(find_workspace_root(&nested).is_none());
    }

    // ---- resolve_extension_dir tests ----------------------------------

    #[test]
    fn resolve_extension_dir_from_directory() {
        let dir = make_dir();
        touch(&dir.path().join("_extension.yml"));

        let resolved = resolve_extension_dir(Some(dir.path())).unwrap();
        assert_eq!(resolved, std::path::absolute(dir.path()).unwrap());
    }

    #[test]
    fn resolve_extension_dir_from_yml_path() {
        let dir = make_dir();
        let yml = dir.path().join("_extension.yml");
        touch(&yml);

        let resolved = resolve_extension_dir(Some(&yml)).unwrap();
        assert_eq!(resolved, std::path::absolute(dir.path()).unwrap());

        // A relative path resolves to an absolute directory.
        let relative = resolve_extension_dir(Some(Path::new("."))).unwrap_err();
        assert!(
            relative.to_string().contains("No _extension.yml"),
            "got {relative}"
        );
    }

    #[test]
    fn resolve_extension_dir_fails_without_yml() {
        let dir = make_dir();
        // No _extension.yml created.
        let result = resolve_extension_dir(Some(dir.path()));
        assert!(result.is_err(), "should fail when _extension.yml is absent");
    }
}
