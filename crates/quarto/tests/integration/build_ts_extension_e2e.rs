/*
 * build_ts_extension_e2e.rs
 *
 * P2-18 — end-to-end test for `q2 build-ts-extension`.
 *
 * Invokes `q2 build-ts-extension` on a temp COPY of the echo-engine fixture,
 * then verifies that the output `.js` bundle is produced, non-empty, and
 * contains a recognisable token from the source.
 *
 * Gated on `deno` availability: the test early-returns (skip) when `deno`
 * is not on PATH.  On this machine deno 2.9.0 is present, so the test
 * should RUN and pass.
 *
 * P2-18 non-vacuity: the fixture is copied to a tempdir and its `dist/`
 * output is DELETED before the build, so the committed `dist/echo-engine.js`
 * is never read or mutated. The assertion therefore proves the build ACTUALLY
 * produced the `.js` this run — if `build-ts-extension` skipped the `deno
 * bundle` step, no `.js` exists in the tempdir and the test goes RED. (The
 * earlier form asserted the COMMITTED `dist/echo-engine.js` existed, which is
 * always true regardless of whether the build ran — a vacuous pass.)
 */

use std::path::Path;
use std::process::Command;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

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

/// Return `true` when `deno` is on PATH.
fn deno_available() -> bool {
    Command::new("deno")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// P2-18: `q2 build-ts-extension` produces a loadable `.js` bundle.
///
/// Runs the command on the committed `echo-engine` fixture under
/// `crates/quarto-core/tests/fixtures/extensions/echo-engine/`, copied to a
/// tempdir. The fixture's `_extension.yml` declares the output path as
/// `dist/echo-engine.js`, which — after deleting any pre-existing copy in the
/// tempdir — we assert exists, is non-empty, and was produced by this build.
#[test]
fn build_ts_extension_produces_bundle() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — skipping build_ts_extension_e2e");
        return;
    }

    // The fixture is committed relative to the quarto crate root.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    // quarto crate is at crates/quarto/, so workspace root is ../..
    let workspace_root = Path::new(manifest_dir)
        .parent() // crates/
        .and_then(|p| p.parent()) // repo root
        .expect("Failed to find workspace root from CARGO_MANIFEST_DIR");

    let committed_ext =
        workspace_root.join("crates/quarto-core/tests/fixtures/extensions/echo-engine");
    assert!(
        committed_ext.exists(),
        "fixture directory missing: {}",
        committed_ext.display()
    );

    // P2-18 non-vacuity: copy the fixture to a temp dir and delete its committed
    // `dist/` so the build MUST produce the `.js` fresh (and the committed
    // `dist/echo-engine.js` in the working tree is never touched). The temp copy
    // lives under the workspace's gitignored `target/` so that build-ts-extension's
    // workspace auto-detection (walking up for `ts-packages/quarto-api`) still
    // resolves `resources/extension-build/` — a copy fully outside the workspace
    // can't locate the shipped deno config.
    let tmp = tempfile::TempDir::new_in(workspace_root.join("target")).expect("tempdir");
    let ext_dir = tmp.path().join("echo-engine");
    copy_dir(&committed_ext, &ext_dir);
    let js_path = ext_dir.join("dist/echo-engine.js");
    if js_path.exists() {
        std::fs::remove_file(&js_path).expect("remove pre-existing dist bundle in temp copy");
    }
    assert!(
        !js_path.exists(),
        "precondition: dist bundle must not exist before the build"
    );

    // Run q2 build-ts-extension on the temp copy.
    let output = Command::new(Q2_BIN)
        .arg("build-ts-extension")
        .arg(&ext_dir)
        .current_dir(workspace_root)
        .output()
        .expect("Failed to spawn q2");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "q2 build-ts-extension failed.\nstdout: {stdout}\nstderr: {stderr}"
    );

    // Assert the declared output path was produced BY THIS BUILD (it did not
    // exist a moment ago), proving the `deno bundle` step actually ran.
    assert!(
        js_path.exists(),
        "Expected bundle at {} but it was not created.\nstdout: {stdout}\nstderr: {stderr}",
        js_path.display()
    );

    let js_content = std::fs::read_to_string(&js_path).expect("Failed to read bundle output");

    // Non-empty.
    assert!(
        !js_content.is_empty(),
        "Bundle at {} is empty",
        js_path.display()
    );

    // Sanity-check: the bundle must contain the engine name token.
    // Named revert: if the bundler output is empty or doesn't bundle the source,
    // this check goes RED.
    assert!(
        js_content.contains("echo"),
        "Bundle does not contain 'echo' token — bundling may have failed:\n{}",
        &js_content[..js_content.len().min(500)]
    );

    // The bundle should be a valid ES module (has 'export').
    assert!(
        js_content.contains("export"),
        "Bundle does not contain 'export' — may not be a valid ES module:\n{}",
        &js_content[..js_content.len().min(500)]
    );
}
