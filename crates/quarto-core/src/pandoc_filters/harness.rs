//! Test harness for shelling out to a real `pandoc` binary against the
//! materialized Q1 filter tree (see [`super::bundle`]).
//!
//! `run_main_lua` is the shared entry point later tasks in the pandoc-hybrid
//! epic build their own tests on: it fixes the exact `pandoc` invocation
//! shape and the runtime-environment contract Q1's `init.lua` requires.
//!
//! Tests: `crates/quarto-core/tests/integration/pandoc_transport.rs`
//! (T2.2, T2.3, T2.4).

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use super::bundle::extract_share_tree;
use super::params_codec::encode_params_blob;

/// The outcome of a `run_main_lua` invocation.
pub struct PandocRunOutcome {
    pub status: ExitStatus,
    pub stderr: String,
    pub out_path: PathBuf,
}

/// Runs `pandoc` with Q1's `main.lua` filter chain against a caller-supplied
/// Pandoc-JSON AST, writing the result to `out`.
///
/// `params_blob_json` is a caller-supplied JSON string (not yet
/// base64-encoded — this function does that internally) that becomes
/// `QUARTO_FILTER_PARAMS` after decoding by `init.lua`.
///
/// Invocation shape:
/// ```text
/// pandoc -f json -t <to_format> --data-dir <share>/pandoc/datadir \
///   -L <share>/filters/main.lua -o <out>
/// ```
/// with env: `QUARTO_SHARE_PATH`, `QUARTO_FILTER_PARAMS`,
/// `QUARTO_FILTER_DEPENDENCY_FILE`.
pub fn run_main_lua(
    ast_json: &str,
    to_format: &str,
    params_blob_json: &str,
    out: &Path,
) -> PandocRunOutcome {
    run_pandoc(ast_json, to_format, params_blob_json, out, true)
}

/// Same as [`run_main_lua`], but never sets `QUARTO_SHARE_PATH` on the child
/// process, regardless of whether `run_main_lua` itself does. Documents the
/// failure shape (T2.3) when the runtime-environment contract is not
/// honoured — see the vacuity-check note in the Task 2 test seam spec.
pub fn run_main_lua_without_share_path(
    ast_json: &str,
    to_format: &str,
    params_blob_json: &str,
    out: &Path,
) -> PandocRunOutcome {
    run_pandoc(ast_json, to_format, params_blob_json, out, false)
}

fn run_pandoc(
    ast_json: &str,
    to_format: &str,
    params_blob_json: &str,
    out: &Path,
    set_share_path: bool,
) -> PandocRunOutcome {
    let share_dir = tempfile::Builder::new()
        .prefix("quarto-pandoc-share-")
        .tempdir()
        .expect("failed to create temp share dir");
    extract_share_tree(share_dir.path()).expect("failed to extract Q1 filter tree");
    let share = share_dir.path();

    let mut ast_file = tempfile::Builder::new()
        .prefix("quarto-pandoc-ast-")
        .suffix(".json")
        .tempfile()
        .expect("failed to create temp file for AST input");
    ast_file
        .write_all(ast_json.as_bytes())
        .expect("failed to write AST input");

    // init.lua's dependenciesFile() only needs the path to exist and be
    // readable/writable — it doesn't need pre-existing content.
    let deps_file = tempfile::Builder::new()
        .prefix("quarto-pandoc-deps-")
        .suffix(".txt")
        .tempfile()
        .expect("failed to create temp file for dependency file");

    let params_b64 = encode_params_blob(params_blob_json);

    let mut cmd = Command::new("pandoc");
    cmd.arg("-f")
        .arg("json")
        .arg("-t")
        .arg(to_format)
        .arg("--data-dir")
        .arg(share.join("pandoc").join("datadir"))
        .arg("-L")
        .arg(share.join("filters").join("main.lua"))
        .arg("-o")
        .arg(out)
        .arg(ast_file.path())
        .env("QUARTO_FILTER_PARAMS", params_b64)
        .env("QUARTO_FILTER_DEPENDENCY_FILE", deps_file.path());

    if set_share_path {
        cmd.env("QUARTO_SHARE_PATH", share);
    }

    let output = cmd.output().expect("failed to execute pandoc");

    PandocRunOutcome {
        status: output.status,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        out_path: out.to_path_buf(),
    }
}

/// Runs `pandoc` with a caller-supplied standalone `-L` filter — **not**
/// Q1's `main.lua` chain — against a caller-supplied Pandoc-JSON AST,
/// writing the result to `out`.
///
/// This is the "standalone probe filter" pattern: `init.lua` (auto-loaded
/// via `--data-dir`) defines the global `param()` function for *every*
/// filter pandoc runs in the chain, regardless of which `-L` file follows
/// it — so a minimal probe filter can read `QUARTO_FILTER_PARAMS` via
/// `param()` without needing any of `main.lua`'s state. `QUARTO_SHARE_PATH`
/// is still required so `init.lua`'s own `require '_format'` etc. resolves,
/// even though `main.lua` itself never runs.
///
/// Kept intentionally minimal (one probe file, no filter chain) per the
/// Task 3 brief; this exact pattern is reused by Task 11's transport smoke
/// test.
pub fn run_probe_filter(
    ast_json: &str,
    to_format: &str,
    params_blob_json: &str,
    probe_lua_source: &str,
    out: &Path,
) -> PandocRunOutcome {
    let share_dir = tempfile::Builder::new()
        .prefix("quarto-pandoc-share-")
        .tempdir()
        .expect("failed to create temp share dir");
    extract_share_tree(share_dir.path()).expect("failed to extract Q1 filter tree");
    let share = share_dir.path();

    let mut ast_file = tempfile::Builder::new()
        .prefix("quarto-pandoc-ast-")
        .suffix(".json")
        .tempfile()
        .expect("failed to create temp file for AST input");
    ast_file
        .write_all(ast_json.as_bytes())
        .expect("failed to write AST input");

    let mut probe_file = tempfile::Builder::new()
        .prefix("quarto-pandoc-probe-")
        .suffix(".lua")
        .tempfile()
        .expect("failed to create temp file for probe filter");
    probe_file
        .write_all(probe_lua_source.as_bytes())
        .expect("failed to write probe filter source");

    // init.lua's dependenciesFile() only needs the path to exist and be
    // readable/writable — it doesn't need pre-existing content.
    let deps_file = tempfile::Builder::new()
        .prefix("quarto-pandoc-deps-")
        .suffix(".txt")
        .tempfile()
        .expect("failed to create temp file for dependency file");

    let params_b64 = encode_params_blob(params_blob_json);

    let output = Command::new("pandoc")
        .arg("-f")
        .arg("json")
        .arg("-t")
        .arg(to_format)
        .arg("--data-dir")
        .arg(share.join("pandoc").join("datadir"))
        .arg("-L")
        .arg(probe_file.path())
        .arg("-o")
        .arg(out)
        .arg(ast_file.path())
        .env("QUARTO_SHARE_PATH", share)
        .env("QUARTO_FILTER_PARAMS", params_b64)
        .env("QUARTO_FILTER_DEPENDENCY_FILE", deps_file.path())
        .output()
        .expect("failed to execute pandoc");

    PandocRunOutcome {
        status: output.status,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        out_path: out.to_path_buf(),
    }
}

/// The outcome of a `run_shim_lua_script` invocation -- a standalone
/// `pandoc lua <script>` run, not a `-L` filter chain (there is no output
/// document, so no [`PandocRunOutcome::out_path`]).
pub struct PandocLuaOutcome {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

/// Escapes `path` as a Lua double-quoted string literal.
fn lua_string_literal(path: &Path) -> String {
    let escaped = path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// Runs `pandoc lua <script>` against `script_body`, a caller-supplied Lua
/// script that can assume the wire-format shim (`quarto2-shim.lua`) is
/// already loaded as the global `quarto2_shim`, with `quarto.json`
/// bootstrapped from the real vendored `_json.lua` module (reachable via
/// `package.path`) -- but with **none** of `main.lua`'s other runtime state
/// (no `crossref`/`quarto_global_state`, no other `quarto.*` members). This
/// is the `L`(lua) tier: a standalone `pandoc lua` script exercising the
/// shim's pure helpers directly, as opposed to `run_main_lua`/
/// `run_main_lua_capturing_ast`'s `L`(chain) tier, which runs the real
/// `-L main.lua` filter chain.
///
/// `script_body` is expected to `assert(...)` its own expectations and
/// raise a Lua error (nonzero exit, message on stderr) on failure.
pub fn run_shim_lua_script(script_body: &str) -> PandocLuaOutcome {
    let share_dir = tempfile::Builder::new()
        .prefix("quarto-pandoc-share-")
        .tempdir()
        .expect("failed to create temp share dir");
    extract_share_tree(share_dir.path()).expect("failed to extract Q1 filter tree");
    let share = share_dir.path();

    let datadir = share.join("pandoc").join("datadir");
    let shim_path = share.join("filters").join("quarto2-shim.lua");

    let script = format!(
        "package.path = {} .. \"/?.lua;\" .. package.path\n\
         local json = require('_json')\n\
         quarto = {{ json = json }}\n\
         dofile({})\n\
         {}\n",
        lua_string_literal(&datadir),
        lua_string_literal(&shim_path),
        script_body,
    );

    let mut script_file = tempfile::Builder::new()
        .prefix("quarto-shim-script-")
        .suffix(".lua")
        .tempfile()
        .expect("failed to create temp file for shim script");
    script_file
        .write_all(script.as_bytes())
        .expect("failed to write shim script");

    let output = Command::new("pandoc")
        .arg("lua")
        .arg(script_file.path())
        .output()
        .expect("failed to execute pandoc lua");

    PandocLuaOutcome {
        status: output.status,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// Runs `pandoc` with Q1's `main.lua` filter chain, followed by a second
/// observer `-L` probe (`quarto2-shim-probe.lua`) that captures the
/// post-`main.lua`-filter AST to a temp file and writes it verbatim, so
/// `FORMAT == "docx"` (or whichever `to_format`) is preserved for
/// `main.lua`'s own writer while the intermediate AST stays inspectable.
/// Measured against pandoc 3.8.1: a second `-L` sees the first filter's
/// output AST and does not otherwise perturb the render.
///
/// Returns the render outcome alongside the captured AST (`Value::Null` if
/// the probe never wrote anything, e.g. because the render failed before
/// reaching the probe).
pub fn run_main_lua_capturing_ast(
    ast_json: &str,
    to_format: &str,
    params_blob_json: &str,
    out: &Path,
) -> (PandocRunOutcome, serde_json::Value) {
    let share_dir = tempfile::Builder::new()
        .prefix("quarto-pandoc-share-")
        .tempdir()
        .expect("failed to create temp share dir");
    extract_share_tree(share_dir.path()).expect("failed to extract Q1 filter tree");
    let share = share_dir.path();

    let mut ast_file = tempfile::Builder::new()
        .prefix("quarto-pandoc-ast-")
        .suffix(".json")
        .tempfile()
        .expect("failed to create temp file for AST input");
    ast_file
        .write_all(ast_json.as_bytes())
        .expect("failed to write AST input");

    // init.lua's dependenciesFile() only needs the path to exist and be
    // readable/writable -- it doesn't need pre-existing content.
    let deps_file = tempfile::Builder::new()
        .prefix("quarto-pandoc-deps-")
        .suffix(".txt")
        .tempfile()
        .expect("failed to create temp file for dependency file");

    let capture_file = tempfile::Builder::new()
        .prefix("quarto-pandoc-capture-")
        .suffix(".json")
        .tempfile()
        .expect("failed to create temp file for captured AST");

    let params_b64 = encode_params_blob(params_blob_json);

    let output = Command::new("pandoc")
        .arg("-f")
        .arg("json")
        .arg("-t")
        .arg(to_format)
        .arg("--data-dir")
        .arg(share.join("pandoc").join("datadir"))
        .arg("-L")
        .arg(share.join("filters").join("main.lua"))
        .arg("-L")
        .arg(share.join("filters").join("quarto2-shim-probe.lua"))
        .arg("-o")
        .arg(out)
        .arg(ast_file.path())
        .env("QUARTO_SHARE_PATH", share)
        .env("QUARTO_FILTER_PARAMS", params_b64)
        .env("QUARTO_FILTER_DEPENDENCY_FILE", deps_file.path())
        .env("QUARTO2_SHIM_PROBE_OUT", capture_file.path())
        .output()
        .expect("failed to execute pandoc");

    let outcome = PandocRunOutcome {
        status: output.status,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        out_path: out.to_path_buf(),
    };

    let captured_ast = std::fs::read_to_string(capture_file.path())
        .ok()
        .filter(|s| !s.is_empty())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null);

    (outcome, captured_ast)
}

/// Runs `main.lua` with `QUARTO2_LAYER1_DUMP` set to a fresh temp path and
/// returns the parsed Layer-1 registry census H7 (`quarto2-shim.lua`'s
/// `quarto2-layer1-census` filter, P5 Task 7) writes there: Q1's live
/// `by_ast_name` handler registry plus the shim's own route table, in one
/// JSON object.
///
/// **Panics if the census file was never written** (H7 absent from the
/// shipped shim, or its `io.open` write failed) rather than falling back to
/// an empty/default census — "file missing" must be a hard error here, not
/// a silently-empty census, which is the same vacuity trap in a different
/// coat (see the Task 7 vacuity-check note: an empty census would satisfy
/// any "for each expected type, if present then …" assertion).
pub fn capture_layer1_introspection(ast_json: &str, params_blob_json: &str) -> serde_json::Value {
    let share_dir = tempfile::Builder::new()
        .prefix("quarto-pandoc-share-")
        .tempdir()
        .expect("failed to create temp share dir");
    extract_share_tree(share_dir.path()).expect("failed to extract Q1 filter tree");
    let share = share_dir.path();

    let mut ast_file = tempfile::Builder::new()
        .prefix("quarto-pandoc-ast-")
        .suffix(".json")
        .tempfile()
        .expect("failed to create temp file for AST input");
    ast_file
        .write_all(ast_json.as_bytes())
        .expect("failed to write AST input");

    let deps_file = tempfile::Builder::new()
        .prefix("quarto-pandoc-deps-")
        .suffix(".txt")
        .tempfile()
        .expect("failed to create temp file for dependency file");

    // A path that does not exist yet: H7's `io.open(out, "w")` creates it.
    let census_dir = tempfile::Builder::new()
        .prefix("quarto-pandoc-layer1-")
        .tempdir()
        .expect("failed to create temp dir for the Layer-1 census");
    let census_path = census_dir.path().join("census.json");

    let params_b64 = encode_params_blob(params_blob_json);
    let out_path = share.join("layer1-probe-out.docx");

    let output = Command::new("pandoc")
        .arg("-f")
        .arg("json")
        .arg("-t")
        .arg("docx")
        .arg("--data-dir")
        .arg(share.join("pandoc").join("datadir"))
        .arg("-L")
        .arg(share.join("filters").join("main.lua"))
        .arg("-o")
        .arg(&out_path)
        .arg(ast_file.path())
        .env("QUARTO_SHARE_PATH", share)
        .env("QUARTO_FILTER_PARAMS", params_b64)
        .env("QUARTO_FILTER_DEPENDENCY_FILE", deps_file.path())
        .env("QUARTO2_LAYER1_DUMP", &census_path)
        .output()
        .expect("failed to execute pandoc");

    assert!(
        output.status.success(),
        "expected the Layer-1 census render to succeed, stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let contents = std::fs::read_to_string(&census_path).unwrap_or_else(|e| {
        panic!(
            "expected the Layer-1 census file at {census_path:?} to exist \
             (H7 missing from quarto2-shim.lua, or its write failed): {e}"
        )
    });
    serde_json::from_str(&contents).expect("Layer-1 census file should contain valid JSON")
}

/// `L`-tier hard-gate: panics (never skips) when `pandoc` is not on `PATH`,
/// or is present but below [`super::version::pandoc_floor`]. A missing or
/// too-old pandoc must never read as a silently-passed test — see the L-tier
/// gate policy in `claude-notes/plans/2026-09-18-pandoc-hybrid-P4-implementation.md`
/// (this is exactly how `preview-renderer` reddened unnoticed on `main` once,
/// GH #250, via a soft compare-and-skip).
///
/// Modelled on `assert_good_pandoc_version` in
/// `crates/pampa/tests/integration/test.rs`, which `.expect()`-panics when
/// pandoc is absent.
pub fn assert_pandoc_available() {
    let output = Command::new("pandoc").arg("--version").output().expect(
        "pandoc must be installed and on PATH to run the `L`-tier pandoc_transport tests \
             (see crates/quarto-core/src/pandoc_filters/harness.rs)",
    );
    let version_str = String::from_utf8_lossy(&output.stdout);
    let floor = super::version::pandoc_floor();
    if !super::version::at_least(&version_str, floor) {
        let first_line = version_str.lines().next().unwrap_or("<no output>");
        panic!(
            "pandoc found ({first_line}) is below the {}.{} floor the `L`-tier \
             pandoc_transport tests require (see crates/quarto-core/src/pandoc_filters/version.rs)",
            floor.0, floor.1
        );
    }
}
