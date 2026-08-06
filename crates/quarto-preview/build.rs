//! Build script that locates the SPA bundles embedded at compile time.
//!
//! Mirrors `crates/quarto-trace-server/build.rs` for the viewer SPA.
//! The `include_dir!` macro needs concrete compile-time paths; this
//! script resolves two of them:
//!
//! 1. **Viewer** (`QUARTO_PREVIEW_EMBED_DIR`): the q2-preview SPA. If
//!    `q2-preview-spa/dist/index.html` exists, embed that directory;
//!    otherwise embed a placeholder `index.html` pointing at
//!    `cargo xtask build-q2-preview-spa` (A.4 / bd-501n).
//! 2. **Editor** (`QUARTO_HUB_CLIENT_EMBED_DIR`): the full hub-client
//!    editor served by `q2 preview --ui editor` (live-share plan
//!    Phase 4, bd-jt1etjbn). If `hub-client/dist-preview-embed/`
//!    exists, embed a *filtered copy*: files byte-identical to the
//!    real viewer dist at the same relative path are stripped, because
//!    the runtime lookup serves those paths from the viewer embed —
//!    that is how the ~38 MB `wasm_quarto_hub_client_bg-*.wasm` (plus
//!    the automerge/tree-sitter wasm and shared fonts, all with
//!    content-hashed names identical across the two Vite builds) is
//!    embedded once instead of twice. Otherwise embed a placeholder
//!    pointing at `cargo xtask build-hub-client-embed`. No cargo
//!    warning for the missing editor dist — unlike the viewer it is
//!    opt-in (`--ui editor`), and the placeholder page names the fix.
//!
//! Both paths are exposed via `cargo:rustc-env` and consumed by
//! `src/lib.rs` through `include_dir!("$VAR")`.

use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root = manifest_dir.join("..").join("..");
    let real_dist = workspace_root.join("q2-preview-spa").join("dist");

    let viewer_is_real = real_dist.join("index.html").is_file();
    let embed_dir = if viewer_is_real {
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

    // Editor embed (Phase 4). The viewer dist is only a dedupe target
    // when it is what the viewer embed actually serves — stripping
    // against a dist that isn't embedded would 404 the shared assets.
    let editor_dist = workspace_root.join("hub-client").join("dist-preview-embed");
    let editor_embed_dir = if editor_dist.join("index.html").is_file() {
        let dedupe_against = viewer_is_real.then_some(real_dist.as_path());
        make_editor_embed(&editor_dist, dedupe_against)
    } else {
        make_editor_placeholder()
    };
    println!(
        "cargo:rustc-env=QUARTO_HUB_CLIENT_EMBED_DIR={}",
        editor_embed_dir.display()
    );
    println!("cargo:rerun-if-changed={}", editor_dist.display());
    if editor_dist.is_dir() {
        watch_recursive(&editor_dist);
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
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        if is_dir {
            watch_recursive(&path);
        }
    }
}

/// Produce the editor embed directory in `OUT_DIR`: a copy of
/// `editor_dist` minus every file that is byte-identical to
/// `dedupe_against` at the same relative path (those are served through
/// the viewer embed at runtime). Rebuilt from scratch on every rerun so
/// deleted/renamed dist files can never linger in the embed.
fn make_editor_embed(editor_dist: &Path, dedupe_against: Option<&Path>) -> PathBuf {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let embed = out_dir.join("editor-embed");
    if embed.exists() {
        std::fs::remove_dir_all(&embed).expect("clear stale editor embed");
    }
    std::fs::create_dir_all(&embed).expect("create editor embed dir");
    copy_filtered(editor_dist, editor_dist, &embed, dedupe_against);
    embed
}

fn copy_filtered(root: &Path, dir: &Path, embed: &Path, dedupe_against: Option<&Path>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(it) => it,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let rel = path.strip_prefix(root).expect("entry under walk root");
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            copy_filtered(root, &path, embed, dedupe_against);
            continue;
        }
        let bytes = std::fs::read(&path).expect("read editor dist file");
        if let Some(viewer) = dedupe_against
            && std::fs::read(viewer.join(rel)).is_ok_and(|v| v == bytes)
        {
            continue; // shared with the viewer embed; served from there
        }
        let dest = embed.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).expect("create embed subdir");
        }
        std::fs::write(&dest, bytes).expect("write embed file");
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

fn make_editor_placeholder() -> PathBuf {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let dist = out_dir.join("editor-placeholder-dist");
    std::fs::create_dir_all(&dist).expect("create editor placeholder dist dir");

    let index = dist.join("index.html");
    // Same `<div id="root">` contract as the viewer placeholder: even
    // on an unbuilt tree, `--ui editor` boots to a page with the React
    // mount point and instructions.
    let html = r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8"/>
    <title>q2 preview — editor UI not built</title>
    <style>
      body { font-family: -apple-system, Segoe UI, sans-serif; max-width: 640px; margin: 40px auto; color: #222; }
      code, pre { background: #f4f4f7; padding: 2px 6px; border-radius: 4px; }
      pre { padding: 10px; overflow: auto; }
      h1 { font-size: 18px; }
    </style>
  </head>
  <body>
    <div id="root">
      <h1>The q2 preview editor UI is not built</h1>
      <p>
        The embedded hub-client editor bundle is a placeholder. Build
        the editor and rebuild the <code>quarto</code> binary:
      </p>
      <pre>cargo xtask build-hub-client-embed
cargo build -p quarto</pre>
      <p>
        Or run without <code>--ui editor</code> to use the default
        read-only preview UI.
      </p>
    </div>
  </body>
</html>
"#;
    write_if_changed(&index, html);

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
