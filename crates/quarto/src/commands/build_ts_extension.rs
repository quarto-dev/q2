//! `q2 build-ts-extension` command — build a TypeScript engine extension bundle.
//!
//! Extension authors run this after editing TS source; `q2` never runs it
//! during render.  It resolves a deno.json config by the precedence rules in
//! [`resolve_build_config`], then shells out to `deno bundle`.
//!
//! # Config precedence
//!
//! 1. `--config <path>` (explicit override) — always wins.
//! 2. `deno.json` committed in the extension directory.
//! 3. Workspace override — when `workspace_root` is `Some` (auto-detected
//!    or `--workspace`): `<root>/resources/extension-build/deno.workspace.json`.
//! 4. Shipped published template (`shipped_config` arg) — the fallback
//!    used by installed binaries where no workspace is present.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// Arguments for the `build-ts-extension` command.
#[derive(Debug)]
pub struct BuildTsExtensionArgs {
    /// Path to the extension directory or `_extension.yml`. Defaults to cwd.
    pub path: Option<PathBuf>,
    /// Explicit `--config` override; wins over all other config sources.
    pub config: Option<PathBuf>,
    /// Force use of workspace `deno.workspace.json` (in-repo / pre-publish build).
    pub workspace: bool,
}

// ============================================================================
// Pure config resolver — unit-testable, no I/O
// ============================================================================

/// Resolves which deno.json config to pass to `deno bundle`.
///
/// This function is **pure**: it performs no filesystem I/O.  Callers are
/// responsible for probing the filesystem (does `ext_dir/deno.json` exist?
/// is there a workspace root?) and passing the results as arguments.
///
/// # Precedence
///
/// 1. `explicit` wins unconditionally.
/// 2. `ext_deno_json` — `Some` when a `deno.json` exists in the extension dir.
/// 3. `workspace_root` — `Some` when a q2 workspace was detected (or
///    `--workspace` forced): returns `<root>/resources/extension-build/deno.workspace.json`.
/// 4. `shipped_config` — absolute path to the shipped published template;
///    the final fallback.
pub fn resolve_build_config(
    explicit: Option<&Path>,
    ext_deno_json: Option<&Path>,
    workspace_root: Option<&Path>,
    shipped_config: &Path,
) -> PathBuf {
    // 1. Explicit --config wins.
    if let Some(p) = explicit {
        return p.to_path_buf();
    }
    // 2. Extension-committed deno.json.
    if let Some(p) = ext_deno_json {
        return p.to_path_buf();
    }
    // 3. In-repo workspace override.
    if let Some(root) = workspace_root {
        return root
            .join("resources")
            .join("extension-build")
            .join("deno.workspace.json");
    }
    // 4. Shipped published template.
    shipped_config.to_path_buf()
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
// Shipped-config path resolution
// ============================================================================

/// Returns the path to the shipped `resources/extension-build/deno.json`
/// template.
///
/// At runtime the shipped configs live adjacent to the binary in the q2 layout
/// (`<install-prefix>/resources/extension-build/`).  During development and
/// tests we fall back to the workspace root detected from the current executable
/// path, then from `CARGO_MANIFEST_DIR` (available only at compile time, exposed
/// here via a compile-time constant).
///
/// The caller passes the workspace root so the function stays pure and
/// unit-testable; in production the caller uses [`find_workspace_root`] or the
/// executable parent.
fn shipped_config_path(workspace_root: Option<&Path>) -> Option<PathBuf> {
    if let Some(root) = workspace_root {
        let p = root
            .join("resources")
            .join("extension-build")
            .join("deno.json");
        if p.exists() {
            return Some(p);
        }
    }
    None
}

// ============================================================================
// Command handler
// ============================================================================

pub fn execute(args: BuildTsExtensionArgs) -> Result<()> {
    // Locate the extension directory (containing _extension.yml).
    let ext_dir = resolve_extension_dir(args.path.as_deref())?;

    // Auto-detect workspace root (walk up from ext_dir).
    let auto_workspace = if args.workspace {
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

    // Probe for an extension-local deno.json.
    let ext_deno = {
        let p = ext_dir.join("deno.json");
        if p.exists() { Some(p) } else { None }
    };

    // Shipped config (fallback for installed-binary users).
    let shipped = shipped_config_path(auto_workspace.as_deref()).with_context(|| {
        format!(
            "Could not locate shipped resources/extension-build/deno.json. \
                 If you are running from a q2 source clone, ensure the workspace root \
                 contains resources/extension-build/. Current ext_dir: {}",
            ext_dir.display()
        )
    })?;

    let config = resolve_build_config(
        args.config.as_deref(),
        ext_deno.as_deref(),
        auto_workspace.as_deref(),
        &shipped,
    );

    // Locate the TS entry point.
    let entry_ts = find_entry_ts(&ext_dir)?;

    // Locate the output path from _extension.yml.
    let output_js = find_output_path(&ext_dir)?;

    // Run deno bundle.
    run_deno_bundle(&config, &entry_ts, &output_js)
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
// deno bundle runner
// ============================================================================

fn run_deno_bundle(config: &Path, entry: &Path, output: &Path) -> Result<()> {
    use std::process::Command;

    // Ensure the output parent directory exists.
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output directory {}", parent.display()))?;
    }

    let status = Command::new("deno")
        .arg("bundle")
        .arg(format!("--config={}", config.display()))
        .arg(format!("--output={}", output.display()))
        .arg(entry)
        .status()
        .context("Failed to spawn `deno`. Ensure deno is on PATH.")?;

    if !status.success() {
        bail!(
            "`deno bundle` exited with status {}. \
             Check the TypeScript source for errors.",
            status
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

    /// P3-config-1: explicit --config wins over everything else.
    #[test]
    fn explicit_config_wins() {
        let dir = make_dir();
        let explicit = dir.path().join("custom.json");
        let ext_deno = dir.path().join("ext/deno.json");
        let workspace = dir.path().join("workspace");
        let shipped = dir.path().join("shipped/deno.json");

        let result =
            resolve_build_config(Some(&explicit), Some(&ext_deno), Some(&workspace), &shipped);
        assert_eq!(result, explicit, "explicit --config must win");
    }

    /// P3-config-2: extension-dir deno.json wins over workspace and shipped.
    #[test]
    fn ext_dir_deno_json_wins_over_workspace() {
        let dir = make_dir();
        let ext_deno = dir.path().join("ext/deno.json");
        let workspace = dir.path().join("workspace");
        let shipped = dir.path().join("shipped/deno.json");

        let result = resolve_build_config(
            None, // no explicit
            Some(&ext_deno),
            Some(&workspace),
            &shipped,
        );
        assert_eq!(
            result, ext_deno,
            "extension deno.json must win over workspace"
        );
    }

    /// P3-config-3: workspace override is selected when no explicit/ext-deno.
    #[test]
    fn workspace_override_selected_when_no_ext_deno() {
        let dir = make_dir();
        let workspace = dir.path().join("workspace");
        let shipped = dir.path().join("shipped/deno.json");

        let result = resolve_build_config(
            None, // no explicit
            None, // no ext deno.json
            Some(&workspace),
            &shipped,
        );
        let expected = workspace
            .join("resources")
            .join("extension-build")
            .join("deno.workspace.json");
        assert_eq!(result, expected, "workspace override must be selected");
    }

    /// P3-config-4: shipped template is the final fallback.
    #[test]
    fn shipped_template_is_final_fallback() {
        let dir = make_dir();
        let shipped = dir.path().join("shipped/deno.json");

        let result = resolve_build_config(
            None, // no explicit
            None, // no ext deno.json
            None, // no workspace
            &shipped,
        );
        assert_eq!(result, shipped, "shipped template must be final fallback");
    }

    /// RED test: removing the explicit-wins arm would return ext_deno instead.
    #[test]
    fn explicit_beats_ext_deno_json() {
        let dir = make_dir();
        let explicit = dir.path().join("explicit.json");
        let ext_deno = dir.path().join("deno.json");
        let shipped = dir.path().join("shipped/deno.json");

        let result = resolve_build_config(Some(&explicit), Some(&ext_deno), None, &shipped);
        assert_ne!(
            result, ext_deno,
            "explicit must beat extension deno.json (not be equal to ext_deno)"
        );
        assert_eq!(result, explicit);
    }

    /// RED test: removing the workspace arm would fall through to shipped.
    #[test]
    fn workspace_beats_shipped() {
        let dir = make_dir();
        let workspace = dir.path().join("workspace");
        let shipped = dir.path().join("shipped/deno.json");

        let result = resolve_build_config(None, None, Some(&workspace), &shipped);
        assert_ne!(
            result, shipped,
            "workspace override must beat the shipped template"
        );
        assert!(
            result.ends_with("deno.workspace.json"),
            "workspace result must end with deno.workspace.json, got: {}",
            result.display()
        );
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
        assert_eq!(resolved, dir.path());
    }

    #[test]
    fn resolve_extension_dir_from_yml_path() {
        let dir = make_dir();
        let yml = dir.path().join("_extension.yml");
        touch(&yml);

        let resolved = resolve_extension_dir(Some(&yml)).unwrap();
        assert_eq!(resolved, dir.path());
    }

    #[test]
    fn resolve_extension_dir_fails_without_yml() {
        let dir = make_dir();
        // No _extension.yml created.
        let result = resolve_extension_dir(Some(dir.path()));
        assert!(result.is_err(), "should fail when _extension.yml is absent");
    }
}
