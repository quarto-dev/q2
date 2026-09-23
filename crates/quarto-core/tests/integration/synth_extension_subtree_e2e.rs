/*
 * tests/integration/synth_extension_subtree_e2e.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Extension-subtree infrastructure plan
 * (claude-notes/plans/2026-09-23-extension-subtree-infrastructure.md),
 * Phase 3 — end-to-end proof that a vendored extension subtree is
 * discoverable and executable with **zero** `_extensions/` install, using
 * the `synth-echo` fake fixture at
 * `tests/fixtures/extension-subtrees/synth-echo/` (no real engine is
 * vendored by this plan).
 *
 * Mirrors `synth_engines_e2e.rs`'s shape, but drives discovery through
 * `QUARTO_EXTENSION_SUBTREES_DIR` (the dev/test seam `extension::mod.rs`'s
 * `builtin_extension_subtree_roots` honors) instead of installing the
 * fixture under a project's `_extensions/`.
 */

// Native-only: TsEngine / TsEngineHost are behind cfg(not(target_arch = "wasm32")).
#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// Return `true` when `deno` is on PATH (the E2E's subprocess runtime).
fn deno_available() -> bool {
    Command::new("deno")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Absolute path to the committed `synth-echo` subtree fixture, shaped like
/// a whole vendored repo: `_extensions/synth-echo/{_extension.yml,src/}`.
fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/extension-subtrees/synth-echo")
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

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// Copy the committed fixture into a fresh tempdir (so committed source is
/// never mutated with build artifacts) and return the tempdir alongside the
/// `_extensions`-shaped subtree root within it
/// (`<tmp>/_extensions/synth-echo/`) — the shape
/// `builtin_extension_subtree_roots` expects a per-subtree root to have,
/// mirroring production's `resources/extension-subtrees/<name>/_extensions/`.
fn setup_subtree_root() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    copy_dir(&fixture_root(), tmp.path());
    let subtree_root = tmp.path().join("_extensions");
    (tmp, subtree_root)
}

/// Render `input` through the real per-document render path
/// (`render_to_file` -> `render_document_to_file`, the same entry `quarto
/// render` uses) and return the rendered HTML.
fn render_html(input: &Path) -> String {
    let options = RenderToFileOptions::default();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_to_file(input, "html", &options, runtime).expect("render_to_file");
    std::fs::read_to_string(&result.output_path).expect("read rendered HTML")
}

#[test]
fn discovers_synth_echo_via_subtree_root_env_var_with_zero_extensions_install() {
    let (_tmp, subtree_root) = setup_subtree_root();

    let project_tmp = TempDir::new().unwrap();
    let input = project_tmp.path().join("doc.qmd");
    write_file(&input, "# hi\n");

    // Each nextest test runs in its own process, so mutating the process
    // environment here is safe (no cross-test race).
    unsafe {
        std::env::set_var("QUARTO_EXTENSION_SUBTREES_DIR", &subtree_root);
    }
    let runtime = NativeRuntime::new();
    let builtin_roots = quarto_core::extension::all_builtin_extension_roots(&runtime);
    let builtin_root_refs: Vec<&Path> = builtin_roots.iter().map(|p| p.as_path()).collect();
    let (extensions, _diags) =
        quarto_core::extension::discover_extensions(&input, None, &builtin_root_refs, &runtime);
    unsafe {
        std::env::remove_var("QUARTO_EXTENSION_SUBTREES_DIR");
    }

    assert_eq!(
        extensions
            .iter()
            .filter(|e| e.id.name == "synth-echo")
            .count(),
        1,
        "expected exactly one synth-echo extension via the subtree root; found: {:?}",
        extensions.iter().map(|e| &e.id.name).collect::<Vec<_>>()
    );
    assert!(
        !project_tmp.path().join("_extensions").exists(),
        "discovery must not require a project-level _extensions/ install"
    );
}

#[test]
fn e2e_single_file_render_executes_synthsub_via_subtree_root() {
    if !deno_available() {
        eprintln!("SKIP: no deno");
        return;
    }

    let (_tmp, subtree_root) = setup_subtree_root();
    let ext_dir = subtree_root.join("synth-echo");
    crate::engine_fixture_build::ensure_bundle(&ext_dir, "synth-echo");

    // Single-file render (no `_quarto.yml` ancestor) so this exercises
    // `discover_extensions_and_build_registry` -- the OTHER caller of
    // `discover_extensions_only`, distinct from the project-mode path
    // Phase 2's unit tests already covered -- closing the loop on the
    // WASM-branch fix from Phase 2 (both callers now go through
    // `all_builtin_extension_roots`).
    let project_tmp = TempDir::new().unwrap();
    let input = project_tmp.path().join("doc.qmd");
    write_file(
        &input,
        "---\ntitle: Synth Subtree E2E\n---\n\n```{synthsub}\nhello from synthsub\n```\n",
    );

    unsafe {
        std::env::set_var("QUARTO_EXTENSION_SUBTREES_DIR", &subtree_root);
    }
    let html = render_html(&input);
    unsafe {
        std::env::remove_var("QUARTO_EXTENSION_SUBTREES_DIR");
    }

    assert!(
        html.contains("SYNTHSUB_EXECUTED:hello from synthsub"),
        "expected the real Deno engine host to have executed the {{synthsub}} cell; got:\n{html}"
    );

    // D4: q2's extension-contributed engines are registry-only
    // (`Contributes.engines` -> `build_engine_registry`) -- they never enter
    // the merged metadata handed to the writer, unlike Q1 (which resolves
    // engine objects into pandoc metadata and needs `filterBundledSubtreeEngines`
    // to strip them back out before templating). Nothing in `synth-echo`'s
    // `execute()` ever echoes its own engine name (only the executed cell's
    // body), so if the engine's identifier leaked into the rendered page
    // anyway, that would mean q2 grew the Q1 behavior this plan's D1-D4
    // survey found absent.
    assert!(
        !html.contains("synth-echo"),
        "the extension-contributed engine's own name must not leak into rendered\
         template metadata (no Q1-style filterBundledSubtreeEngines needed); got:\n{html}"
    );
}
