/*
 * extension/build.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Build a TypeScript engine extension bundle (`deno bundle`).
 */

//! Build logic for `q2 call build-ts-extension` — build a TypeScript engine
//! extension bundle.
//!
//! Extension authors run this after editing TS source; `q2` never runs it
//! during render.  It resolves a deno.json config by the precedence rules in
//! [`resolve_build_config`], then shells out to `deno bundle`.
//!
//! Native-only (mirrors `engine::ts_process`): spawns `deno` via
//! `std::process`, unavailable on `wasm32-unknown-unknown`.
//!
//! # Config precedence
//!
//! 1. `--config <path>` (explicit override) — always wins.
//! 2. `deno.json` committed in the extension directory.
//! 3. Workspace override — when `workspace_root` is `Some` (auto-detected
//!    or `--workspace`): `<root>/resources/extension-build/deno.workspace.json`.
//! 4. Shipped published template — embedded at compile time
//!    (`SHIPPED_DENO_JSON`, via `include_str!`) and materialized to a temp
//!    file *only when this tier is actually reached* (see
//!    [`materialize_shipped_config`]). This is the fallback used by
//!    installed binaries where no workspace is present; resolution is
//!    lazy so `--config` / an ext-dir `deno.json` / `--workspace` short-
//!    circuit before it, and an installed binary never touches it unless
//!    tiers 1-3 are all absent.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// Options for building an extension's TypeScript engine bundle.
#[derive(Debug)]
pub struct BuildOptions {
    /// Path to the extension directory or `_extension.yml`. Defaults to cwd.
    pub ext_dir: Option<PathBuf>,
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
/// Tiers 1-3 are **pure**: no filesystem I/O, no fallibility. Tier 4
/// (`shipped_config`) is a **lazy provider closure**, invoked only when
/// tiers 1-3 are all absent — this function owns the eager/lazy boundary.
/// That laziness is the whole point: an installed binary with `--config`
/// set (or an ext-dir `deno.json`, or `--workspace`) must never touch tier
/// 4, even if materializing it would fail. Callers are responsible for
/// probing the filesystem for tiers 1-3 (does `ext_dir/deno.json` exist?
/// is there a workspace root?) and passing the results as arguments.
///
/// # Precedence
///
/// 1. `explicit` wins unconditionally.
/// 2. `ext_deno_json` — `Some` when a `deno.json` exists in the extension dir.
/// 3. `workspace_root` — `Some` when a q2 workspace was detected (or
///    `--workspace` forced): returns `<root>/resources/extension-build/deno.workspace.json`.
/// 4. `shipped_config` — lazy provider for the embedded shipped template;
///    the final fallback, invoked only when 1-3 are all absent.
pub fn resolve_build_config<F>(
    explicit: Option<&Path>,
    ext_deno_json: Option<&Path>,
    workspace_root: Option<&Path>,
    shipped_config: F,
) -> Result<PathBuf>
where
    F: FnOnce() -> Result<PathBuf>,
{
    // 1. Explicit --config wins.
    if let Some(p) = explicit {
        return Ok(p.to_path_buf());
    }
    // 2. Extension-committed deno.json.
    if let Some(p) = ext_deno_json {
        return Ok(p.to_path_buf());
    }
    // 3. In-repo workspace override.
    if let Some(root) = workspace_root {
        return Ok(root
            .join("resources")
            .join("extension-build")
            .join("deno.workspace.json"));
    }
    // 4. Shipped published template — lazy: only materialized when reached.
    shipped_config()
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
// Shipped-config embed (tier 4 — installed-binary fallback)
// ============================================================================

/// The shipped `resources/extension-build/deno.json` template (repo root),
/// embedded at compile time.
///
/// Fully self-contained: its imports are `jsr:@quarto/api` /
/// `jsr:@quarto/types` / `jsr:/@std/` specifiers only — no relative paths —
/// so it works unmodified from a materialized temp file regardless of
/// working directory. This is what makes embedding (rather than an
/// exe-adjacent resource probe) the right shape: the release tarball is
/// single-member (just the `q2` binary), so there is no adjacent
/// `resources/` directory to probe at runtime. Contrast the workspace
/// variant (`deno.workspace.json`, tier 3), which has `../../ts-packages/…`
/// relative imports and therefore stays in-repo-only, resolved via
/// `workspace_root` rather than embedded.
const SHIPPED_DENO_JSON: &str = include_str!("../../../../resources/extension-build/deno.json");

/// Materializes the embedded shipped `deno.json` to a temp file and returns
/// a drop guard for it alongside its path, for use as
/// `deno bundle --config=<path>`.
///
/// Called only when tier 4 is actually selected — see the laziness
/// contract on [`resolve_build_config`].
///
/// Returns a [`tempfile::TempPath`] guard rather than persisting the file
/// via `NamedTempFile::keep()`. `keep()` detaches the file from RAII
/// cleanup entirely — the file handle it returns is not itself a live
/// guard, so a caller that discards it (as this function's previous form
/// did) leaks one `q2-shipped-deno-*.json` into the OS temp dir on every
/// installed-binary invocation that reaches tier 4, unboundedly. The
/// caller (`build_ts_extension()`) instead holds the `TempPath` for exactly
/// as long as the file needs to exist — through the `deno bundle` subprocess
/// run — and lets it drop afterward, on every exit path (success or `deno
/// bundle` failure alike), which deletes the file.
fn materialize_shipped_config() -> Result<(tempfile::TempPath, PathBuf)> {
    let mut tmp = tempfile::Builder::new()
        .prefix("q2-shipped-deno-")
        .suffix(".json")
        .tempfile()
        .context("Failed to create a temp file for the shipped deno.json config")?;
    tmp.write_all(SHIPPED_DENO_JSON.as_bytes())
        .context("Failed to write the shipped deno.json config to a temp file")?;
    let path = tmp.into_temp_path();
    let path_buf = path.to_path_buf();
    Ok((path, path_buf))
}

// ============================================================================
// Command handler
// ============================================================================

/// Builds the extension's `dist/*.js` bundle; returns the resolved output path.
pub fn build_ts_extension(opts: BuildOptions) -> Result<PathBuf> {
    // Locate the extension directory (containing _extension.yml).
    let ext_dir = resolve_extension_dir(opts.ext_dir.as_deref())?;

    // Auto-detect workspace root (walk up from ext_dir).
    let auto_workspace = if opts.workspace {
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

    // Tier 4 (the shipped template) is resolved lazily: `resolve_build_config`
    // only calls `materialize_shipped_config` when tiers 1-3 are all absent,
    // so an installed binary with `--config` set (or an ext-dir deno.json,
    // or `--workspace`) never touches it. No eager resolution happens ahead
    // of this call — that's the P1.2 fix.
    //
    // `shipped_guard` holds tier 4's temp-file drop guard for the rest of
    // this function, if tier 4 was selected (`None` otherwise). It is
    // declared here, before `config`, so it stays alive through the
    // `run_deno_bundle` call below and is dropped — deleting the temp
    // file — when `build_ts_extension()` returns, on every exit path
    // (success or `deno bundle` failure alike). This is what stops the
    // materialized shipped `deno.json` from leaking into the OS temp dir.
    let mut shipped_guard: Option<tempfile::TempPath> = None;
    let config = resolve_build_config(
        opts.config.as_deref(),
        ext_deno.as_deref(),
        auto_workspace.as_deref(),
        || {
            let (guard, path) = materialize_shipped_config()?;
            shipped_guard = Some(guard);
            Ok(path)
        },
    )?;

    // Locate the TS entry point.
    let entry_ts = find_entry_ts(&ext_dir)?;

    // Locate the output path from _extension.yml.
    let output_js = find_output_path(&ext_dir)?;

    // Run deno bundle. `shipped_guard` (if `Some`) remains in scope through
    // this call and is dropped when `build_ts_extension()` returns, cleaning
    // up the tier-4 temp file regardless of whether the bundle succeeds or
    // fails.
    run_deno_bundle(&config, &entry_ts, &output_js)?;

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

    /// Panic-if-called stub for the tier-4 provider, for tests where tiers
    /// 1-3 must short-circuit before it. Also doubles as an assertion that
    /// `resolve_build_config` really is lazy: a mechanical signature update
    /// that dropped the laziness would call this and fail the test.
    fn unreachable_shipped_provider() -> Result<PathBuf> {
        panic!("shipped_config provider must not be called when an earlier tier is selected")
    }

    /// P3-config-1: explicit --config wins over everything else.
    #[test]
    fn explicit_config_wins() {
        let dir = make_dir();
        let explicit = dir.path().join("custom.json");
        let ext_deno = dir.path().join("ext/deno.json");
        let workspace = dir.path().join("workspace");

        let result = resolve_build_config(
            Some(&explicit),
            Some(&ext_deno),
            Some(&workspace),
            unreachable_shipped_provider,
        )
        .unwrap();
        assert_eq!(result, explicit, "explicit --config must win");
    }

    /// P3-config-2: extension-dir deno.json wins over workspace and shipped.
    #[test]
    fn ext_dir_deno_json_wins_over_workspace() {
        let dir = make_dir();
        let ext_deno = dir.path().join("ext/deno.json");
        let workspace = dir.path().join("workspace");

        let result = resolve_build_config(
            None, // no explicit
            Some(&ext_deno),
            Some(&workspace),
            unreachable_shipped_provider,
        )
        .unwrap();
        assert_eq!(
            result, ext_deno,
            "extension deno.json must win over workspace"
        );
    }

    /// P3-config-3: workspace override is selected when no explicit/ext-deno.
    ///
    /// This also covers the `--workspace` leg's needed behavior described in
    /// the P1.2 plan item: `workspace_root = Some(root)` selects tier 3, and
    /// the shipped provider is never touched.
    #[test]
    fn workspace_override_selected_when_no_ext_deno() {
        let dir = make_dir();
        let workspace = dir.path().join("workspace");

        let result = resolve_build_config(
            None, // no explicit
            None, // no ext deno.json
            Some(&workspace),
            unreachable_shipped_provider,
        )
        .unwrap();
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
            || Ok(shipped.clone()),
        )
        .unwrap();
        assert_eq!(result, shipped, "shipped template must be final fallback");
    }

    /// RED test: removing the explicit-wins arm would return ext_deno instead.
    #[test]
    fn explicit_beats_ext_deno_json() {
        let dir = make_dir();
        let explicit = dir.path().join("explicit.json");
        let ext_deno = dir.path().join("deno.json");

        let result = resolve_build_config(
            Some(&explicit),
            Some(&ext_deno),
            None,
            unreachable_shipped_provider,
        )
        .unwrap();
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

        let result =
            resolve_build_config(None, None, Some(&workspace), unreachable_shipped_provider)
                .unwrap();
        assert!(
            result.ends_with("deno.workspace.json"),
            "workspace result must end with deno.workspace.json, got: {}",
            result.display()
        );
    }

    // ---- T4/T5 (P1.2, frozen 2026-07-02) --------------------------------

    /// T4: `--config X` wins with `workspace_root = None`, and the lazy
    /// shipped-config provider is never invoked.
    ///
    /// **Why this binds the real fix (vacuity note from the P1.2 seam
    /// spec):** before the fix, `execute()` resolved the shipped config
    /// *eagerly* via `shipped_config_path(...).with_context()?` **before**
    /// calling `resolve_build_config` at all — so an installed binary (no
    /// workspace root, thus no on-disk shipped config to find) hard-errored
    /// even when `--config` was passed, regardless of what
    /// `resolve_build_config`'s own precedence logic said. Unit-testing the
    /// old `resolve_build_config` (which took `shipped_config: &Path`,
    /// already resolved) could never catch that bug — it only tested logic
    /// that was already correct. The fix moves the eager/lazy boundary
    /// *into* `resolve_build_config` itself (the `shipped_config: F`
    /// closure parameter) and removes the separate eager call from
    /// `execute()`, so this function is now literally the seam `execute()`
    /// routes through with nothing eager ahead of it — testing it directly
    /// binds the fix.
    #[test]
    fn config_wins_without_workspace_and_never_touches_shipped_provider() {
        let dir = make_dir();
        let explicit = dir.path().join("custom.json");

        let result = resolve_build_config(
            Some(&explicit),
            None, // no ext deno.json
            None, // no workspace root — the installed-binary case
            unreachable_shipped_provider,
        )
        .unwrap();

        assert_eq!(result, explicit);
    }

    /// T5: when tiers 1-3 are all absent, tier 4 is selected and the lazy
    /// provider materializes the embedded `resources/extension-build/deno.json`
    /// to a temp file. Assert the materialized file's content matches the
    /// embedded source exactly and parses as JSON — proving the embed
    /// round-trips through `include_str!` + the temp-file write, not just
    /// that *some* file got created.
    ///
    /// Plumbing note: `materialize_shipped_config` now returns
    /// `(TempPath, PathBuf)` instead of a bare `PathBuf` (see the temp-file
    /// leak fix below), so this test wraps it in a closure that stashes the
    /// guard in a local `Option` — mechanical adaptation only. The asserted
    /// behavior (content equality + JSON parse) is unchanged, and the guard
    /// is kept alive across those assertions so the file still exists when
    /// they run.
    #[test]
    fn tier4_materializes_embedded_shipped_config() {
        let mut guard: Option<tempfile::TempPath> = None;
        let result = resolve_build_config(None, None, None, || {
            let (g, path) = materialize_shipped_config()?;
            guard = Some(g);
            Ok(path)
        })
        .unwrap();

        let content = std::fs::read_to_string(&result).expect("materialized temp file must exist");
        assert_eq!(
            content, SHIPPED_DENO_JSON,
            "materialized content must match the embedded deno.json source"
        );

        let _: serde_json::Value =
            serde_json::from_str(&content).expect("materialized shipped config must parse as JSON");

        // Guard stays alive through the assertions above; drop it explicitly
        // here so the cleanup-on-drop behavior is exercised within the test.
        drop(guard);
    }

    /// P1.2-followup: the materialized shipped-config temp file must be
    /// removed once its drop guard goes out of scope — it must not leak
    /// into the OS temp dir on every installed-binary invocation that
    /// reaches tier 4 (the exact path the P1.2 fix exists to unblock).
    ///
    /// RED against the pre-fix `.keep()`-based implementation: `.keep()`
    /// detaches the file from RAII cleanup entirely, so nothing removes it
    /// when the returned path falls out of scope — this assertion failed
    /// before the fix. GREEN once `materialize_shipped_config` returns a
    /// `TempPath` guard instead of persisting via `.keep()`.
    #[test]
    fn shipped_config_temp_file_removed_after_guard_drops() {
        let path_buf = {
            let (_guard, path_buf) = materialize_shipped_config().unwrap();
            assert!(
                path_buf.exists(),
                "materialized file must exist while its guard is alive"
            );
            path_buf
            // `_guard` (TempPath) drops here, deleting the file.
        };
        assert!(
            !path_buf.exists(),
            "materialized shipped-config temp file must be removed once its guard drops"
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
