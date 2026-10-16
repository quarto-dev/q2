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
