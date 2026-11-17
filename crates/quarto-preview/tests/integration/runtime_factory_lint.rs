//! F8-D6 lint: every native site of the live preview server obtains its
//! runtime from the factory (`render_flags::native_runtime[_for]`), so
//! `--profile` cannot be skipped by one of them. A direct
//! `NativeRuntime::new()` in non-test code of `quarto-preview`, or in the
//! `quarto` preview commands, fails this test.
//!
//! Out of scope on purpose: `import_route.rs` builds a runtime to resolve
//! pandoc only (no project is read), and `commands/render.rs` takes the
//! profile through `RenderArgs.profile`.

use std::path::{Path, PathBuf};

/// Files whose non-test code may call `NativeRuntime::new()`.
const ALLOWED: &[&str] = &["import_route.rs"];

/// The non-test, non-comment lines of `text` that construct a runtime.
fn offending_lines(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        // Everything from the first `#[cfg(test)]` module on is test code.
        if line.trim() == "#[cfg(test)]" {
            let next = text.lines().nth(i + 1).unwrap_or("").trim_start();
            if next.starts_with("mod ") {
                break;
            }
            continue;
        }
        let code = line.trim_start();
        if code.starts_with("//") {
            continue;
        }
        if code.contains("NativeRuntime::new()") {
            out.push((i + 1, line.trim().to_string()));
        }
    }
    out
}

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .collect();
    files.sort();
    files
}

#[test]
fn preview_sites_use_the_runtime_factory() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = rust_files(&manifest.join("src"));
    let commands = manifest.join("../quarto/src/commands");
    files.extend(
        ["preview.rs", "preview_static.rs"]
            .iter()
            .map(|name| commands.join(name)),
    );
    let mut failures = Vec::new();
    for file in &files {
        let name = file.file_name().unwrap().to_string_lossy();
        if ALLOWED.contains(&name.as_ref()) {
            continue;
        }
        let text = std::fs::read_to_string(file).unwrap();
        for (line, code) in offending_lines(&text) {
            failures.push(format!("{}:{line}: {code}", file.display()));
        }
    }
    assert!(
        failures.is_empty(),
        "build the runtime with quarto_preview::render_flags::native_runtime[_for] so \
         `--profile` reaches it:\n{}",
        failures.join("\n")
    );
}

#[test]
fn the_lint_sees_a_direct_construction() {
    let src = "fn a() {\n    let r = NativeRuntime::new();\n}\n// NativeRuntime::new()\n#[cfg(test)]\nmod tests {\n    fn t() { NativeRuntime::new(); }\n}\n";
    assert_eq!(
        offending_lines(src),
        vec![(2, "let r = NativeRuntime::new();".to_string())]
    );
}

// ── `-M` reaches every engine run ────────────────────────────────────────────────────────
//
// The session's `-M` overlay is a runtime layer, so an engine run reads it only through a
// runtime built by `render_flags::runtime_with_overlay()`. A site that hands the engine a
// plain `native_runtime()` silently drops `-M` (and `execute: {record: true}` with it).
// Source-side reads (project, inputs, staleness) stay on the plain runtime on purpose.

/// Non-test, non-comment lines that call `record_capture_run(` (the engine run) or build a
/// `PreviewLookup {` (whose `runtime` field the engine step uses), with the `runtime_with_overlay`
/// check each must pass: a `record_capture_run(` call must have `engine_runtime` or
/// `self.runtime` among its arguments, and a `PreviewLookup {` must set `runtime:` from
/// `runtime_with_overlay()`.
fn engine_run_violations(text: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if line.trim() == "#[cfg(test)]"
            && lines
                .get(i + 1)
                .is_some_and(|n| n.trim_start().starts_with("mod "))
        {
            break;
        }
        let code = line.trim_start();
        if code.starts_with("//") || code.starts_with("use ") || code.contains("fn ") {
            continue;
        }
        let window = lines[i..lines.len().min(i + 8)].join("\n");
        if code.contains("record_capture_run(")
            && !window.contains("engine_runtime")
            && !window.contains("self.runtime")
        {
            out.push((i + 1, line.trim().to_string()));
        }
        if code.contains("PreviewLookup {") && !window.contains("runtime_with_overlay()") {
            out.push((i + 1, line.trim().to_string()));
        }
    }
    out
}

#[test]
fn engine_runs_take_the_overlay_runtime() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut failures = Vec::new();
    for file in rust_files(&manifest.join("src")) {
        // `cache.rs` forwards the runtime its caller chose.
        if file.file_name().is_some_and(|n| n == "cache.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        for (line, code) in engine_run_violations(&text) {
            failures.push(format!("{}:{line}: {code}", file.display()));
        }
    }
    assert!(
        failures.is_empty(),
        "an engine run must get quarto_preview::render_flags::runtime_with_overlay() so `-M` \
         reaches it:\n{}",
        failures.join("\n")
    );
}

#[test]
fn the_engine_run_lint_sees_a_plain_runtime() {
    let bad = "fn a() {\n    let p = PreviewLookup {\n        runtime: runtime.clone(),\n    };\n    record_capture_run(&path, &project, runtime, None).await;\n}\n";
    assert_eq!(engine_run_violations(bad).len(), 2);
    let good = "fn a() {\n    let p = PreviewLookup {\n        runtime: crate::render_flags::runtime_with_overlay(),\n    };\n    record_capture_run(&path, &project, engine_runtime, None).await;\n}\n";
    assert!(engine_run_violations(good).is_empty());
}
