/*
 * tests/integration/engine_fixture_build.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Plan 1c3, Task 4 — a shared in-process build helper for the hermetic
 * fixture-regeneration seams (Task 5/6, `synth_engines_e2e`,
 * `echo_engine_e2e`, `behave_engine_e2e`, `capture_splice_seam`, etc.).
 *
 * `build_bundle`/`ensure_bundle` call `quarto_core::extension::build::build_ts_extension`
 * in-process (no subprocess to `q2`, no `CARGO_BIN_EXE`) against the workspace
 * `deno.workspace.json` import map, so the 10 `HERMETIC_FIXTURES` (pure/
 * type-only imports) can regenerate their `dist` bundle at test time
 * instead of being committed to git.
 *
 * `build_helper_produces_bundle` (seam T7) is the linchpin: it proves the
 * in-process call + `--config` tempdir build + the `../../resources/...`
 * path depth all actually work, before any consumer wires the helper in.
 */

// Native-only: extension::build is behind cfg(not(target_arch = "wasm32")).
#![cfg(not(target_arch = "wasm32"))]

use std::path::Path;

/// Fixtures whose bundles are hermetically regenerable (pure/type-only imports).
pub const HERMETIC_FIXTURES: &[&str] = &[
    "alpha",
    "beta",
    "behave",
    "mismatch",
    "content-claim",
    "fallback-univ",
    "interop-r",
    "whenclass-marimo",
    "echo-engine",
    "echo-legacy",
];

pub fn deno_available() -> bool {
    std::process::Command::new("deno")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Build `<ext_dir>/dist/<name>.js` in place via the workspace import map.
/// Hermetic (no network/lock). Caller gates on `deno_available()`.
pub fn build_bundle(ext_dir: &Path) {
    let cfg = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../resources/extension-build/deno.workspace.json");
    quarto_core::extension::build::build_ts_extension(
        quarto_core::extension::build::BuildOptions {
            ext_dir: Some(ext_dir.to_path_buf()),
            config: Some(cfg),
            workspace: false,
        },
    )
    .expect("build fixture bundle");
}

/// Build only if `name` is hermetic; else no-op (committed bundle used as-is).
pub fn ensure_bundle(ext_dir: &Path, name: &str) {
    if HERMETIC_FIXTURES.contains(&name) {
        build_bundle(ext_dir);
    }
}

/// Absolute path to a committed fixture extension directory
/// (`crates/quarto-core/tests/fixtures/extensions/<name>`).
fn fixture_ext_dir(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/extensions")
        .join(name)
}

/// Recursively copy `src` into `dst` (dst is created).
fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

/// Seam T7 (Task 4 linchpin): recursively copy the committed `alpha` fixture's
/// `src/` + `_extension.yml` (deliberately NOT its committed `dist/`, so the
/// build actually has to produce the bundle) into a tempdir, then prove
/// `build_bundle` regenerates `dist/alpha.js` from source.
#[test]
fn build_helper_produces_bundle() {
    if !deno_available() {
        eprintln!("SKIP: no deno");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let dst = tmp.path().join("alpha");
    let src = fixture_ext_dir("alpha");

    std::fs::create_dir_all(&dst).unwrap();
    std::fs::copy(src.join("_extension.yml"), dst.join("_extension.yml")).unwrap();
    copy_dir(&src.join("src"), &dst.join("src"));

    build_bundle(&dst);
    assert!(dst.join("dist/alpha.js").exists());
}
