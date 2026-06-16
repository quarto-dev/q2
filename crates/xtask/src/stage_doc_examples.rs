//! `cargo xtask stage-doc-examples` — render the example projects listed
//! in `examples/manifest.yml` and stage their static output into the docs
//! site so `.embed-example-iframe` placeholders resolve (bd-z1smhvuo).
//!
//! ## What it does
//!
//! For each entry in `examples/manifest.yml` (an explicit allow-list — no
//! globs, so a stray directory is never rendered or published by accident):
//!
//! 1. render `examples/<entry>` with `q2` (via `cargo run --bin q2`), and
//! 2. copy the project's **static output** — every top-level `*.html` file
//!    and `*_files/` directory — into `docs/examples/<entry>/`.
//!
//! The docs pages embed the staged copy with, e.g.,
//! `file="/examples/presentations/03-fragments/slides.html"`. The docs
//! `project.resources: [examples]` declaration then copies the staged tree
//! into `_site/` on `q2 render docs/`, and `q2 preview docs/` serves it
//! straight from the VFS.
//!
//! ## Why this is a separate step (not part of `q2 render docs/`)
//!
//! The example projects are standalone Quarto projects at the *repo* root,
//! outside the `docs/` tree, so they cannot be docs inputs or docs
//! resources directly (resources must live inside the project root). This
//! command bridges that gap. Its output under `docs/examples/` is generated
//! and gitignored — regenerate, never commit.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::Path;

use crate::create_worktree::repo_root;

/// Parsed `examples/manifest.yml`.
#[derive(Debug, Deserialize)]
struct Manifest {
    /// Project paths relative to `examples/`, e.g. `presentations/03-fragments`.
    projects: Vec<String>,
}

pub fn run() -> Result<()> {
    let root = repo_root()?;
    let examples_dir = root.join("examples");
    let manifest_path = examples_dir.join("manifest.yml");

    let manifest_text = fs::read_to_string(&manifest_path)
        .with_context(|| format!("reading manifest {}", manifest_path.display()))?;
    let manifest: Manifest = serde_yaml::from_str(&manifest_text)
        .with_context(|| format!("parsing manifest {}", manifest_path.display()))?;

    if manifest.projects.is_empty() {
        println!("stage-doc-examples: manifest lists no projects; nothing to do.");
        return Ok(());
    }

    let docs_examples_dir = root.join("docs").join("examples");

    println!(
        "stage-doc-examples: staging {} example project(s) into {}",
        manifest.projects.len(),
        docs_examples_dir.display()
    );

    for entry in &manifest.projects {
        stage_one(&root, &examples_dir, &docs_examples_dir, entry)?;
    }

    println!("stage-doc-examples: done.");
    Ok(())
}

/// Render `examples/<entry>` and copy its static output to
/// `docs/examples/<entry>/`.
fn stage_one(
    root: &Path,
    examples_dir: &Path,
    docs_examples_dir: &Path,
    entry: &str,
) -> Result<()> {
    let src_project = examples_dir.join(entry);
    if !src_project.is_dir() {
        anyhow::bail!(
            "manifest entry `{entry}` does not name a directory under examples/ ({})",
            src_project.display()
        );
    }

    println!("  • rendering examples/{entry}");
    render_project(root, &src_project)
        .with_context(|| format!("rendering example project `{entry}`"))?;

    let dest = docs_examples_dir.join(entry);
    // Start from a clean destination so a removed/renamed output file in the
    // source never lingers in the staged copy.
    if dest.exists() {
        fs::remove_dir_all(&dest)
            .with_context(|| format!("clearing stale staged output {}", dest.display()))?;
    }
    fs::create_dir_all(&dest)
        .with_context(|| format!("creating staged output dir {}", dest.display()))?;

    let copied = copy_static_output(&src_project, &dest)
        .with_context(|| format!("copying static output for `{entry}`"))?;
    if copied == 0 {
        anyhow::bail!(
            "no static output (`*.html` / `*_files/`) found in {} after rendering — \
             did the render succeed?",
            src_project.display()
        );
    }
    println!("    ↳ staged {copied} item(s) to docs/examples/{entry}");
    Ok(())
}

/// Render a single example project in place with `q2`.
fn render_project(root: &Path, project: &Path) -> Result<()> {
    // We are a nested `cargo` invocation; strip the outer `cargo xtask`'s
    // package env vars so the child cargo fingerprints q2 as a fresh shell
    // would. Without this, an inherited `CARGO_MANIFEST_DIR` forces a full
    // rebuild of q2's TLS-stack dependency closure on every run. See
    // `util::strip_inherited_cargo_env` (bd-awchm8w7).
    let mut cmd = crate::util::nested_command("cargo");
    cmd.current_dir(root)
        .args(["run", "--bin", "q2", "--", "render"])
        .arg(project);
    let status = cmd
        .status()
        .context("spawning `cargo run --bin q2 -- render`")?;
    if !status.success() {
        anyhow::bail!(
            "`q2 render {}` failed (exit {:?})",
            project.display(),
            status.code()
        );
    }
    Ok(())
}

/// Image extensions staged alongside the rendered HTML. A deck that references
/// an image (`logo:`, a slide `![](pic.svg)`, …) keeps it as a **top-level**
/// asset beside `slides.html` (a `type: default` project renders in place), so
/// the embedded iframe needs it copied too — `*_files/` alone is not enough.
const STAGED_IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "svg", "webp", "avif"];

/// Copy the static render output from `src_project` into `dest`: every
/// top-level `*.html` file, every top-level image asset (see
/// [`STAGED_IMAGE_EXTENSIONS`]), and every top-level `*_files/` directory.
/// Source files (`*.qmd`, `_quarto.yml`, `README.md`), caches (`.quarto/`), and
/// dotfiles are skipped — only published artifacts are staged. Returns the
/// number of top-level items copied.
fn copy_static_output(src_project: &Path, dest: &Path) -> Result<usize> {
    let mut copied = 0;
    for item in
        fs::read_dir(src_project).with_context(|| format!("reading {}", src_project.display()))?
    {
        let item = item?;
        let name = item.file_name();
        let name = name.to_string_lossy();
        let path = item.path();
        let file_type = item.file_type()?;

        if file_type.is_dir() && name.ends_with("_files") {
            copy_dir_recursive(&path, &dest.join(name.as_ref()))?;
            copied += 1;
        } else if file_type.is_file() && (name.ends_with(".html") || is_staged_image(&name)) {
            fs::copy(&path, dest.join(name.as_ref()))
                .with_context(|| format!("copying {}", path.display()))?;
            copied += 1;
        }
    }
    Ok(copied)
}

/// Whether a top-level file is an image asset that should be staged.
fn is_staged_image(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| {
        STAGED_IMAGE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
    })
}

/// Recursively copy a directory tree.
fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest).with_context(|| format!("creating {}", dest.display()))?;
    for item in fs::read_dir(src).with_context(|| format!("reading {}", src.display()))? {
        let item = item?;
        let path = item.path();
        let target = dest.join(item.file_name());
        if item.file_type()?.is_dir() {
            copy_dir_recursive(&path, &target)?;
        } else {
            fs::copy(&path, &target).with_context(|| format!("copying {}", path.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::is_staged_image;

    #[test]
    fn recognizes_image_assets_case_insensitively() {
        for name in ["logo.svg", "pic.png", "photo.JPG", "anim.gif", "img.WEBP"] {
            assert!(is_staged_image(name), "{name} should be staged");
        }
    }

    #[test]
    fn rejects_non_images_and_extensionless() {
        for name in [
            "slides.html",
            "slides.qmd",
            "_quarto.yml",
            "README.md",
            "noext",
        ] {
            assert!(!is_staged_image(name), "{name} should not be staged");
        }
    }
}
