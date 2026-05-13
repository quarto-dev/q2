//! Build script that locates the q2-preview-spa bundle at compile time.
//!
//! Mirrors `crates/quarto-trace-server/build.rs` exactly. The
//! `include_dir!` macro needs a concrete compile-time path; this script
//! resolves it:
//!
//! 1. If `q2-preview-spa/dist/index.html` exists, embed that directory.
//! 2. Otherwise, write a placeholder `index.html` into the crate's
//!    `OUT_DIR` and embed that, so the build still succeeds. The
//!    placeholder tells the user to run `cargo xtask build-q2-preview-spa`
//!    (added in A.4 / bd-501n).
//!
//! The chosen path is exposed to the crate as
//! `QUARTO_PREVIEW_EMBED_DIR` via `cargo:rustc-env`, and `src/lib.rs`
//! consumes it via `include_dir!("$QUARTO_PREVIEW_EMBED_DIR")`.

use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root = manifest_dir.join("..").join("..");
    let real_dist = workspace_root.join("q2-preview-spa").join("dist");

    let embed_dir = if real_dist.join("index.html").is_file() {
        real_dist.clone()
    } else {
        make_placeholder_dist()
    };

    println!(
        "cargo:rustc-env=QUARTO_PREVIEW_EMBED_DIR={}",
        embed_dir.display()
    );

    // Re-run if the real dist/ tree changes. Directory-mtime watches
    // miss in-place file rewrites (vite emits hashed filenames so this
    // matters); emit per-file rerun-if-changed entries to catch
    // content edits, plus the directory entry for additions.
    println!("cargo:rerun-if-changed={}", real_dist.display());
    if real_dist.is_dir() {
        watch_recursive(&real_dist);
    }
}

fn watch_recursive(root: &Path) {
    let entries = match std::fs::read_dir(root) {
        Ok(it) => it,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        println!("cargo:rerun-if-changed={}", path.display());
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            watch_recursive(&path);
        }
    }
}

fn make_placeholder_dist() -> PathBuf {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let dist = out_dir.join("placeholder-dist");
    std::fs::create_dir_all(&dist).expect("create placeholder dist dir");

    let index = dist.join("index.html");
    // `<div id="root">` is load-bearing for the A.2 smoke test
    // (`crates/quarto-preview/tests/smoke.rs`) — even the placeholder
    // must include it so the test passes when running against a
    // freshly-checked-out tree where the SPA hasn't been built.
    let html = r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8"/>
    <title>q2 preview — SPA not built</title>
    <style>
      body { font-family: -apple-system, Segoe UI, sans-serif; max-width: 640px; margin: 40px auto; color: #222; }
      code, pre { background: #f4f4f7; padding: 2px 6px; border-radius: 4px; }
      pre { padding: 10px; overflow: auto; }
      h1 { font-size: 18px; }
    </style>
  </head>
  <body>
    <div id="root">
      <h1>q2 preview SPA is not built</h1>
      <p>
        The embedded SPA bundle is a placeholder. Build the real UI
        and rebuild the <code>quarto</code> binary:
      </p>
      <pre>cargo xtask build-q2-preview-spa
cargo build -p quarto</pre>
      <p>
        For iterative UI work you can also run the Vite dev server:
      </p>
      <pre>cd q2-preview-spa && npm run dev</pre>
    </div>
  </body>
</html>
"#;
    write_if_changed(&index, html);

    emit_warning(
        "q2-preview-spa/dist/index.html not found; embedding placeholder. \
         Run `cargo xtask build-q2-preview-spa` and rebuild to embed the real SPA.",
    );

    dist
}

fn write_if_changed(path: &Path, contents: &str) {
    let existing = std::fs::read_to_string(path).ok();
    if existing.as_deref() != Some(contents) {
        std::fs::write(path, contents).expect("write placeholder html");
    }
}

fn emit_warning(msg: &str) {
    println!("cargo:warning={}", msg);
}
