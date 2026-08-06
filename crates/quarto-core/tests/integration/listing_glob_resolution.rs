/*
 * tests/integration/listing_glob_resolution.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Integration tests for listing `contents:` glob base-directory
 * resolution (GH #456, bd-v7ixzsp5). See
 * `claude-notes/plans/2026-08-06-listing-glob-provenance.md`.
 */

//! End-to-end tests pinning the provenance-based glob semantics:
//! a `contents:` glob resolves relative to the directory of the file
//! where it was written — the host document's directory for
//! front-matter globs, the `_metadata.yml`'s directory for directory
//! metadata, and the project root for `_quarto.yml` metadata.
//!
//! Each test writes a fixture project to a temp dir, drives it
//! through the full `ProjectPipeline`, then inspects the rendered
//! HTML and the per-page diagnostics. Same shape as
//! `listing_pipeline.rs`, but outputs are keyed by project-relative
//! output path (fixtures here have several `index.qmd` files) and
//! the full `RenderToFileResult` is kept so diagnostics can be
//! asserted on.

use std::path::PathBuf;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::{RenderToFileOptions, RenderToFileResult};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn canonical(path: &std::path::Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn write(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn read(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// Drive a fresh project through the full Pass-1/Pass-2 pipeline.
/// Returns the project dir and the per-file results (kept whole so
/// tests can inspect both the rendered HTML and the diagnostics).
fn render_project(fixture: impl FnOnce(&std::path::Path)) -> (PathBuf, Vec<RenderToFileResult>) {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    fixture(&project_dir);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();
    assert!(
        !project.is_single_file,
        "test expected a multi-file project"
    );

    let options = RenderToFileOptions::default();
    let project_type = project_type_for(&project);
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        Format::html(),
        "html",
        &options,
        runtime.clone(),
    );
    let summary = pollster::block_on(pipeline.run()).expect("pipeline");
    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "unexpected failures: pass1={:?} pass2={:?}",
        summary.pass1_failures,
        summary.pass2_failures,
    );

    std::mem::forget(temp);
    (project_dir, summary.outputs)
}

/// Rendered HTML for the output whose path ends with
/// `relative_output` (forward-slash, e.g. `"sub/index.html"`).
fn html_for(outputs: &[RenderToFileResult], relative_output: &str) -> String {
    let suffix: PathBuf = relative_output.split('/').collect();
    let out = outputs
        .iter()
        .find(|o| o.output_path.ends_with(&suffix))
        .unwrap_or_else(|| {
            panic!(
                "no output ending in `{}`; have: {:?}",
                relative_output,
                outputs.iter().map(|o| &o.output_path).collect::<Vec<_>>()
            )
        });
    read(&out.output_path)
}

/// Titles of the listing items in a rendered page, in DOM order.
fn listing_titles(html: &str) -> Vec<String> {
    let mut titles = Vec::new();
    let marker = "listing-title\">";
    let mut rest = html;
    while let Some(i) = rest.find(marker) {
        rest = &rest[i + marker.len()..];
        if let Some(end) = rest.find('<') {
            titles.push(rest[..end].to_string());
        }
    }
    titles
}

/// Every diagnostic code attached to any per-page output.
fn all_diag_codes(outputs: &[RenderToFileResult]) -> Vec<String> {
    outputs
        .iter()
        .flat_map(|o| o.render_output.diagnostics.iter())
        .filter_map(|d| d.code.clone())
        .collect()
}

fn assert_no_code(outputs: &[RenderToFileResult], code: &str) {
    let codes = all_diag_codes(outputs);
    assert!(
        !codes.iter().any(|c| c == code),
        "expected no {} diagnostics, got codes: {:?}",
        code,
        codes
    );
}

// ─────────────────────────────────────────────────────────────────
// 1. The reported bug (GH #456): a front-matter glob in a subdir
//    host must resolve against the host's directory only — never
//    against the project root.
// ─────────────────────────────────────────────────────────────────

#[test]
fn frontmatter_glob_resolves_against_host_dir_not_project_root() {
    let (_dir, outputs) = render_project(|p| {
        write(
            &p.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n",
        );
        write(&p.join("index.qmd"), "---\ntitle: Home\n---\n\nHome.\n");
        write(&p.join("about.qmd"), "---\ntitle: About\n---\n\nAbout.\n");
        write(
            &p.join("sub/index.qmd"),
            "---\ntitle: Sub\nlisting:\n  contents:\n    - \"*.qmd\"\n---\n",
        );
        write(
            &p.join("sub/p1.qmd"),
            "---\ntitle: P1\ndate: 2026-01-01\n---\n\nPost one.\n",
        );
    });

    let host = html_for(&outputs, "sub/index.html");
    let titles = listing_titles(&host);
    assert_eq!(
        titles,
        vec!["P1"],
        "glob `*.qmd` from sub/ must match only sub/ siblings"
    );

    // The phantom items previously produced unresolved `.qmd` hrefs
    // and Q-13-4 "missing document" warnings (one per phantom link).
    assert!(
        !host.contains("href=\"about.qmd\""),
        "no unresolved .qmd href may leak into the listing"
    );
    assert_no_code(&outputs, "Q-13-4");
    // Phase 5 (defect #5): glob strings in `contents:` are typed as
    // globs by the key-path annotation — no markdown-parse warning.
    assert_no_code(&outputs, "Q-1-20");
}

// ─────────────────────────────────────────────────────────────────
// 6. Interpretation of `contents:` entries (defects #5/#6): glob
//    strings never take the markdown-parsing path, which used to
//    warn (Q-1-20) on parse failure and silently corrupt the
//    pattern on parse *success* (`p*osts*.qmd` → emphasis →
//    `posts.qmd`).
// ─────────────────────────────────────────────────────────────────

#[test]
fn glob_with_markdown_parseable_asterisks_survives() {
    let (_dir, outputs) = render_project(|p| {
        write(
            &p.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n",
        );
        write(
            &p.join("index.qmd"),
            "---\ntitle: Home\nlisting:\n  contents:\n    - \"p*osts*.qmd\"\n---\n",
        );
        write(
            &p.join("pXosts_extra.qmd"),
            "---\ntitle: Should Match\ndate: 2026-01-01\n---\n\nx.\n",
        );
        write(
            &p.join("posts/ignore.qmd"),
            "---\ntitle: In Subdir\n---\n\nx.\n",
        );
    });

    let host = html_for(&outputs, "index.html");
    assert_eq!(
        listing_titles(&host),
        vec!["Should Match"],
        "the asterisks in `p*osts*.qmd` must survive the front-matter \
         parse verbatim (previously the markdown emphasis parse \
         silently flattened the pattern to `posts.qmd`)"
    );
    assert_no_code(&outputs, "Q-1-20");
}

#[test]
fn subdir_host_project_relative_glob_no_longer_matches() {
    // Intentional behavior change (plan decision 1, ships silently):
    // a subdir host writing `posts/*.qmd` no longer reaches the
    // project-root `posts/` directory.
    let (_dir, outputs) = render_project(|p| {
        write(
            &p.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n",
        );
        write(&p.join("index.qmd"), "---\ntitle: Home\n---\n\nHome.\n");
        write(
            &p.join("posts/a.qmd"),
            "---\ntitle: Post A\ndate: 2026-01-01\n---\n\nA.\n",
        );
        write(
            &p.join("sub/index.qmd"),
            "---\ntitle: Sub\nlisting:\n  contents:\n    - \"posts/*.qmd\"\n---\n",
        );
    });

    let host = html_for(&outputs, "sub/index.html");
    assert_eq!(
        listing_titles(&host),
        Vec::<String>::new(),
        "sub/posts/*.qmd matches nothing; the project-relative fallback is gone"
    );
    assert_no_code(&outputs, "Q-13-4");
    // Inside the project root — no escape warning either.
    assert_no_code(&outputs, "Q-12-17");
}

// ─────────────────────────────────────────────────────────────────
// 2. Globs declared in `_metadata.yml` resolve against the
//    directory holding that `_metadata.yml`.
// ─────────────────────────────────────────────────────────────────

#[test]
fn dirmeta_glob_resolves_against_metadata_dir() {
    let (_dir, outputs) = render_project(|p| {
        write(
            &p.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n",
        );
        write(
            &p.join("rootpost.qmd"),
            "---\ntitle: Root Post\ndate: 2026-01-05\n---\n\nRoot.\n",
        );
        write(
            &p.join("blog/_metadata.yml"),
            "listing:\n  contents:\n    - \"deep/*.qmd\"\n",
        );
        write(
            &p.join("blog/deep/index.qmd"),
            "---\ntitle: Deep Index\n---\n",
        );
        write(
            &p.join("blog/deep/p1.qmd"),
            "---\ntitle: Deep P1\ndate: 2026-01-02\n---\n\nDeep post.\n",
        );
    });

    let host = html_for(&outputs, "blog/deep/index.html");
    let titles = listing_titles(&host);
    assert!(
        titles.contains(&"Deep P1".to_string()),
        "`deep/*.qmd` written in blog/_metadata.yml must resolve against blog/ \
         and match blog/deep/p1.qmd; got titles: {:?}",
        titles
    );
    assert!(
        !titles.contains(&"Root Post".to_string()),
        "project-root files must not leak into the blog listing"
    );
    assert_no_code(&outputs, "Q-13-4");
}

// ─────────────────────────────────────────────────────────────────
// 3. Globs declared in `_quarto.yml` (project metadata layer)
//    resolve against the project root, regardless of the host's
//    directory.
// ─────────────────────────────────────────────────────────────────

#[test]
fn projmeta_glob_resolves_against_project_root() {
    let (_dir, outputs) = render_project(|p| {
        write(
            &p.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\nlisting:\n  contents:\n    - \"posts/*.qmd\"\n",
        );
        write(
            &p.join("sub/viewer.qmd"),
            "---\ntitle: Viewer\n---\n\nViewer.\n",
        );
        write(
            &p.join("posts/a.qmd"),
            "---\ntitle: Post A\ndate: 2026-01-01\n---\n\nA.\n",
        );
    });

    // The listing config applies to every page; on the subdir host
    // the glob must still resolve against the *project root* (where
    // `_quarto.yml` lives), not against sub/.
    let host = html_for(&outputs, "sub/viewer.html");
    let titles = listing_titles(&host);
    assert!(
        titles.contains(&"Post A".to_string()),
        "`posts/*.qmd` written in _quarto.yml must resolve against the \
         project root even for a sub/ host; got titles: {:?}",
        titles
    );
    assert!(
        host.contains("href=\"../posts/a.html\""),
        "item href must be page-relative from sub/"
    );
    assert_no_code(&outputs, "Q-13-4");
}

// ─────────────────────────────────────────────────────────────────
// 4. `../` traversal inside the project works; escaping the
//    project root warns (Q-12-17) and matches nothing.
// ─────────────────────────────────────────────────────────────────

#[test]
fn parent_traversal_glob_matches_root_file() {
    let (_dir, outputs) = render_project(|p| {
        write(
            &p.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n",
        );
        write(&p.join("index.qmd"), "---\ntitle: Home\n---\n\nHome.\n");
        write(
            &p.join("rootpost.qmd"),
            "---\ntitle: Root Post\ndate: 2026-01-04\n---\n\nRoot.\n",
        );
        write(
            &p.join("sub/index.qmd"),
            "---\ntitle: Sub\nlisting:\n  contents:\n    - \"../rootpost.qmd\"\n---\n",
        );
    });

    let host = html_for(&outputs, "sub/index.html");
    assert_eq!(
        listing_titles(&host),
        vec!["Root Post"],
        "`../rootpost.qmd` from sub/ must reach the project root"
    );
    assert!(
        host.contains("href=\"../rootpost.html\""),
        "item href must be page-relative from sub/"
    );
    assert_no_code(&outputs, "Q-13-4");
    assert_no_code(&outputs, "Q-12-17");
}

#[test]
fn glob_escaping_project_root_warns() {
    let (_dir, outputs) = render_project(|p| {
        write(
            &p.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n",
        );
        write(
            &p.join("index.qmd"),
            "---\ntitle: Home\nlisting:\n  contents:\n    - \"../../*.qmd\"\n---\n",
        );
        write(
            &p.join("other.qmd"),
            "---\ntitle: Other\ndate: 2026-01-01\n---\n\nOther.\n",
        );
    });

    let host = html_for(&outputs, "index.html");
    assert_eq!(
        listing_titles(&host),
        Vec::<String>::new(),
        "a glob escaping the project root matches nothing"
    );
    let codes = all_diag_codes(&outputs);
    assert_eq!(
        codes.iter().filter(|c| c.as_str() == "Q-12-17").count(),
        1,
        "exactly one Q-12-17 for the escaping glob; got codes: {:?}",
        codes
    );
}

// ─────────────────────────────────────────────────────────────────
// 5. Negation patterns.
// ─────────────────────────────────────────────────────────────────

#[test]
fn negation_glob_excludes_matched_items() {
    let (_dir, outputs) = render_project(|p| {
        write(
            &p.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n",
        );
        write(&p.join("index.qmd"), "---\ntitle: Home\n---\n\nHome.\n");
        write(
            &p.join("sub/index.qmd"),
            "---\ntitle: Sub\nlisting:\n  contents:\n    - \"*.qmd\"\n    - \"!p2.qmd\"\n---\n",
        );
        write(
            &p.join("sub/p1.qmd"),
            "---\ntitle: P1\ndate: 2026-01-01\n---\n\nOne.\n",
        );
        write(
            &p.join("sub/p2.qmd"),
            "---\ntitle: P2\ndate: 2026-01-02\n---\n\nTwo.\n",
        );
    });

    let host = html_for(&outputs, "sub/index.html");
    assert_eq!(
        listing_titles(&host),
        vec!["P1"],
        "`!p2.qmd` must exclude p2 from the `*.qmd` matches"
    );
}

#[test]
fn negation_only_contents_defaults_to_sibling_qmd() {
    let (_dir, outputs) = render_project(|p| {
        write(
            &p.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n",
        );
        write(&p.join("about.qmd"), "---\ntitle: About\n---\n\nAbout.\n");
        write(
            &p.join("sub/index.qmd"),
            "---\ntitle: Sub\nlisting:\n  contents:\n    - \"!p2.qmd\"\n---\n",
        );
        write(
            &p.join("sub/p1.qmd"),
            "---\ntitle: P1\ndate: 2026-01-01\n---\n\nOne.\n",
        );
        write(
            &p.join("sub/p2.qmd"),
            "---\ntitle: P2\ndate: 2026-01-02\n---\n\nTwo.\n",
        );
    });

    let host = html_for(&outputs, "sub/index.html");
    assert_eq!(
        listing_titles(&host),
        vec!["P1"],
        "negation-only contents defaults the positive set to `*.qmd` \
         (host-dir siblings), then excludes p2"
    );
}
