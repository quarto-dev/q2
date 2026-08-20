/*
 * tests/integration/website_aliases.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Integration tests for `aliases:` redirect stubs in website
 * projects (bd-aliases-redirects-missing-sch7cd1g).
 */

//! End-to-end tests for the `aliases:` front-matter key.
//!
//! Each test writes a small fixture to a temp dir, drives it through
//! `ProjectPipeline`, and inspects the redirect stubs written into
//! the output directory.
//!
//! Two behaviours here deliberately **diverge from Quarto 1**, so the
//! tests assert the divergence rather than parity:
//!
//! - Alias collisions are hard errors. Q1 warns-and-skips the
//!   stub-vs-page case and is entirely silent when two pages claim the
//!   same alias. A silent last-write-wins produces a redirect pointing
//!   at the wrong page with no signal, which is the failure mode this
//!   feature exists to prevent.
//! - Paths that differ only by case collide **on every platform**, not
//!   just on case-insensitive filesystems, so a Linux CI build fails
//!   the same way a macOS build does instead of shipping a site that
//!   breaks when checked out on macOS or Windows.
//!
//! See `claude-notes/plans/2026-08-12-aliases-redirect-stubs.md`
//! §"Design decisions".
//!
//! The helpers below mirror those in `website_post_render.rs` — this
//! crate's integration tests keep their fixtures per-file rather than
//! sharing a harness module. The one addition is
//! [`try_render_project`], which returns the pipeline's `Result` so
//! the error tests can inspect diagnostics instead of panicking.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::error::QuartoError;
use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, ProjectRenderSummary, project_type_for};
use quarto_core::render_to_file::RenderToFileOptions;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

// ═══════════════════════════════════════════════════════════════════
// Harness
// ═══════════════════════════════════════════════════════════════════

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// Drive a fixture through `ProjectPipeline`, returning the project
/// directory and the pipeline's `Result`.
///
/// Unlike [`render_project`] this does not assert success — the
/// collision tests need the error.
fn try_render_project(
    fixture: impl FnOnce(&Path),
) -> (PathBuf, Result<ProjectRenderSummary, QuartoError>) {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    fixture(&project_dir);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();

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
    let result = pollster::block_on(pipeline.run());

    // Leak the temp dir so the test can inspect files afterwards
    // (cleanup happens at process exit).
    std::mem::forget(temp);
    (project_dir, result)
}

/// [`try_render_project`] plus the assertion that every page rendered.
fn render_project(fixture: impl FnOnce(&Path)) -> (PathBuf, ProjectRenderSummary) {
    let (project_dir, result) = try_render_project(fixture);
    let summary = result.expect("pipeline should succeed");
    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "unexpected failures: pass1={:?} pass2={:?}",
        summary.pass1_failures,
        summary.pass2_failures
    );
    (project_dir, summary)
}

/// A minimal website project config.
const WEBSITE_YML: &str = "project:\n  type: website\n  output-dir: _site\n";

/// Strip ANSI escapes from a rendered diagnostic.
///
/// Ariadne colours the span snippet one character at a time, so
/// `"index.html"` appears there as ten separately-wrapped characters
/// and a naive `contains` would miss it. Assertions run against the
/// plain text.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            out.push(ch);
            continue;
        }
        // CSI (`ESC [ … final`) and OSC (`ESC ] … BEL`/`ST`) are the
        // two forms ariadne emits; both end at an alphabetic byte or
        // the string terminator.
        for esc in chars.by_ref() {
            if esc.is_ascii_alphabetic() || esc == '\u{7}' || esc == '\\' {
                break;
            }
        }
    }
    out
}

/// Assert the pipeline failed with a diagnostic carrying `code`, and
/// return the rendered diagnostics (ANSI stripped) for inspection.
fn expect_error_code(result: Result<ProjectRenderSummary, QuartoError>, code: &str) -> String {
    let err = match result {
        Ok(_) => panic!("expected the render to fail with {code}, but it succeeded"),
        Err(e) => e,
    };
    let QuartoError::Parse(parse) = &err else {
        panic!("expected QuartoError::Parse carrying diagnostics, got: {err:?}");
    };
    let codes: Vec<String> = parse
        .diagnostics
        .iter()
        .map(|d| d.code.clone().unwrap_or_default())
        .collect();
    assert!(
        codes.iter().any(|c| c == code),
        "expected a diagnostic with code {code}; got codes {codes:?} \
         (titles: {:?})",
        parse
            .diagnostics
            .iter()
            .map(|d| d.title.clone())
            .collect::<Vec<_>>()
    );
    strip_ansi(&parse.render())
}

/// Every HTML file under the output dir, as output-dir-relative
/// forward-slash paths, sorted.
fn output_html_files(project_dir: &Path) -> Vec<String> {
    let out = project_dir.join("_site");
    let mut found = Vec::new();
    collect_html(&out, &out, &mut found);
    found.sort();
    found
}

fn collect_html(root: &Path, dir: &Path, found: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // `site_libs` holds copied assets, not rendered pages.
            if path.file_name().and_then(|n| n.to_str()) == Some("site_libs") {
                continue;
            }
            collect_html(root, &path, found);
        } else if path.extension().and_then(|e| e.to_str()) == Some("html") {
            let rel = path.strip_prefix(root).unwrap();
            found.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// Extract the JSON object literal assigned to `var redirects` in a
/// stub, so tests can assert the exact fragment→target mapping.
fn redirect_map(stub_html: &str) -> String {
    let marker = "var redirects = ";
    let start = stub_html
        .find(marker)
        .unwrap_or_else(|| panic!("no `var redirects` in stub:\n{stub_html}"))
        + marker.len();
    let rest = &stub_html[start..];
    let end = rest
        .find(';')
        .unwrap_or_else(|| panic!("unterminated `var redirects` in stub:\n{stub_html}"));
    rest[..end].to_string()
}

// ═══════════════════════════════════════════════════════════════════
// Resolution — where the stub lands, and where it points
// ═══════════════════════════════════════════════════════════════════

/// The repro from the strand: a site-root-relative alias and a
/// page-relative one, on the same page.
#[test]
fn aliases_absolute_and_page_relative_write_stubs() {
    let (project_dir, _summary) = render_project(|dir| {
        write(&dir.join("_quarto.yml"), WEBSITE_YML);
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nH.\n");
        write(
            &dir.join("current/index.qmd"),
            "---\ntitle: Current\naliases:\n  - /old-name.html\n  - ../previous/index.html\n---\n\nC.\n",
        );
    });

    assert_eq!(
        output_html_files(&project_dir),
        vec![
            "current/index.html".to_string(),
            "index.html".to_string(),
            "old-name.html".to_string(),
            "previous/index.html".to_string(),
        ],
        "expected two pages plus two redirect stubs"
    );

    // `/old-name.html` sits at the output root, so the href back to
    // the page is `current/index.html`.
    let root_stub = read(&project_dir.join("_site/old-name.html"));
    assert_eq!(
        redirect_map(&root_stub),
        r#"{"":"current/index.html"}"#,
        "root-level stub should point at the page relative to itself"
    );

    // `../previous/index.html` resolves against the *page's* output
    // location (`current/`), landing at `previous/index.html`. From
    // there the page is one directory up.
    let nested_stub = read(&project_dir.join("_site/previous/index.html"));
    assert_eq!(
        redirect_map(&nested_stub),
        r#"{"":"../current/index.html"}"#,
        "nested stub should reach back up to the page"
    );
}

/// An alias ending in `/` names a directory; the stub is its
/// `index.html` (Q1 `fixupHref`).
#[test]
fn aliases_trailing_slash_gets_index_html() {
    let (project_dir, _summary) = render_project(|dir| {
        write(&dir.join("_quarto.yml"), WEBSITE_YML);
        write(
            &dir.join("index.qmd"),
            "---\ntitle: Home\naliases:\n  - /moved/\n---\n\nH.\n",
        );
    });

    assert_eq!(
        output_html_files(&project_dir),
        vec!["index.html".to_string(), "moved/index.html".to_string()],
    );
    assert_eq!(
        redirect_map(&read(&project_dir.join("_site/moved/index.html"))),
        r#"{"":"../index.html"}"#,
    );
}

/// An extensionless alias is also a directory (Q1 `fixupHref`):
/// `/moved` → `/moved/index.html`, *not* a file named `moved`.
#[test]
fn aliases_extensionless_gets_index_html() {
    let (project_dir, _summary) = render_project(|dir| {
        write(&dir.join("_quarto.yml"), WEBSITE_YML);
        write(
            &dir.join("index.qmd"),
            "---\ntitle: Home\naliases:\n  - /moved\n---\n\nH.\n",
        );
    });

    assert_eq!(
        output_html_files(&project_dir),
        vec!["index.html".to_string(), "moved/index.html".to_string()],
    );
}

/// An alias that already names a file is used verbatim.
#[test]
fn aliases_html_suffix_used_verbatim() {
    let (project_dir, _summary) = render_project(|dir| {
        write(&dir.join("_quarto.yml"), WEBSITE_YML);
        write(
            &dir.join("index.qmd"),
            "---\ntitle: Home\naliases:\n  - /moved.html\n---\n\nH.\n",
        );
    });

    assert_eq!(
        output_html_files(&project_dir),
        vec!["index.html".to_string(), "moved.html".to_string()],
    );
}

/// A project with no `aliases:` anywhere writes no stubs — the
/// negative control for every test above.
#[test]
fn aliases_absent_writes_no_stubs() {
    let (project_dir, summary) = render_project(|dir| {
        write(&dir.join("_quarto.yml"), WEBSITE_YML);
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nH.\n");
    });

    assert_eq!(output_html_files(&project_dir), vec!["index.html"]);
    assert!(
        summary.project_diagnostics.is_empty(),
        "no aliases should mean no diagnostics; got {:?}",
        summary
            .project_diagnostics
            .iter()
            .map(|d| d.title.clone())
            .collect::<Vec<_>>()
    );
}

// ═══════════════════════════════════════════════════════════════════
// Fragments
// ═══════════════════════════════════════════════════════════════════

/// The shape measured in the Connect docs: two *different* pages
/// contribute fragments to one stub, and a third entry (from one of
/// them) supplies the fragment-less default. The single stub must
/// route each fragment to its own page.
///
/// This is why fragments could not be deferred to a follow-up: a
/// fragment-less implementation would send `#deploy` to whichever
/// page won the last write, silently.
#[test]
fn aliases_fragments_from_two_pages_merge_into_one_stub() {
    let (project_dir, _summary) = render_project(|dir| {
        write(&dir.join("_quarto.yml"), WEBSITE_YML);
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nH.\n");
        write(
            &dir.join("build/index.qmd"),
            "---\ntitle: Build\naliases:\n  - /hub\n  - /hub/#image\n---\n\nB.\n",
        );
        write(
            &dir.join("deploy/index.qmd"),
            "---\ntitle: Deploy\naliases:\n  - /hub/#deploy\n---\n\nD.\n",
        );
    });

    assert_eq!(
        output_html_files(&project_dir),
        vec![
            "build/index.html".to_string(),
            "deploy/index.html".to_string(),
            "hub/index.html".to_string(),
            "index.html".to_string(),
        ],
        "the two fragment aliases and the bare one share a single stub"
    );

    // Keys sorted, fragment-less first: three entries, two targets.
    assert_eq!(
        redirect_map(&read(&project_dir.join("_site/hub/index.html"))),
        r#"{"":"../build/index.html","deploy":"../deploy/index.html","image":"../build/index.html"}"#,
        "each fragment must route to its own declaring page"
    );
}

/// A fragment alias on its own still produces a `""` default, so a
/// visitor arriving without a fragment is not stranded.
#[test]
fn aliases_lone_fragment_still_has_default_target() {
    let (project_dir, _summary) = render_project(|dir| {
        write(&dir.join("_quarto.yml"), WEBSITE_YML);
        write(
            &dir.join("page/index.qmd"),
            "---\ntitle: Page\naliases:\n  - /old.html#sec\n---\n\nP.\n",
        );
    });

    assert_eq!(
        redirect_map(&read(&project_dir.join("_site/old.html"))),
        r#"{"":"page/index.html","sec":"page/index.html"}"#,
    );
}

// ═══════════════════════════════════════════════════════════════════
// Stub contents (candidate B — see the plan's §"Design decisions" 4)
// ═══════════════════════════════════════════════════════════════════

/// The stub must work for a reader whose browser runs no JavaScript,
/// and must tell crawlers which page is canonical. Q1's stub does
/// neither; this is the deliberate divergence.
#[test]
fn alias_stub_has_noscript_canonical_and_charset() {
    let (project_dir, _summary) = render_project(|dir| {
        write(&dir.join("_quarto.yml"), WEBSITE_YML);
        write(
            &dir.join("page/index.qmd"),
            "---\ntitle: Page\naliases:\n  - /old.html\n---\n\nP.\n",
        );
    });

    let stub = read(&project_dir.join("_site/old.html"));

    assert!(
        stub.starts_with("<!DOCTYPE html>"),
        "stub should be a standards-mode document; got:\n{stub}"
    );
    assert!(
        stub.contains(r#"<meta charset="utf-8">"#),
        "stub needs a charset so non-ASCII hrefs are not guessed at; got:\n{stub}"
    );
    assert!(
        stub.contains(r#"<link rel="canonical" href="page/index.html">"#),
        "stub should name the canonical page for crawlers; got:\n{stub}"
    );
    // The meta refresh must be *inside* `<noscript>`: bare, it races
    // the script and can win, sending a fragment-carrying URL to the
    // default target instead of that fragment's own page.
    assert!(
        stub.contains(
            r#"<noscript><meta http-equiv="refresh" content="0; url=page/index.html"></noscript>"#
        ),
        "meta refresh must be inside <noscript>; got:\n{stub}"
    );
    assert!(
        stub.contains(r#"<a href="page/index.html">"#),
        "stub should carry a visible link for clients that follow neither; got:\n{stub}"
    );
}

/// Candidate B puts the href into HTML attribute contexts, which Q1's
/// JS-only stub never had. Characters that are special in HTML must be
/// escaped there while staying literal inside the JSON.
#[test]
fn alias_stub_escapes_html_metacharacters_in_href() {
    let (project_dir, _summary) = render_project(|dir| {
        write(&dir.join("_quarto.yml"), WEBSITE_YML);
        write(
            &dir.join("a&b/index.qmd"),
            "---\ntitle: Amp\naliases:\n  - /old.html\n---\n\nA.\n",
        );
    });

    let stub = read(&project_dir.join("_site/old.html"));
    assert!(
        stub.contains(r#"<link rel="canonical" href="a&amp;b/index.html">"#),
        "`&` must be escaped in attribute context; got:\n{stub}"
    );
    assert_eq!(
        redirect_map(&stub),
        r#"{"":"a&b/index.html"}"#,
        "the JSON literal keeps the raw character"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Drafts
// ═══════════════════════════════════════════════════════════════════

/// A draft page emits no stub. Leaking a draft's existence through a
/// live redirect URL is worse than over-eagerly hiding it.
#[test]
fn aliases_draft_page_emits_no_stub() {
    let (project_dir, _summary) = render_project(|dir| {
        write(&dir.join("_quarto.yml"), WEBSITE_YML);
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nH.\n");
        write(
            &dir.join("wip/index.qmd"),
            "---\ntitle: WIP\ndraft: true\naliases:\n  - /old.html\n---\n\nW.\n",
        );
    });

    assert!(
        !project_dir.join("_site/old.html").exists(),
        "a draft page's alias must not become a live redirect"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Non-website projects
// ═══════════════════════════════════════════════════════════════════

/// `aliases:` only means something in a website project. Elsewhere it
/// is inert — and saying so is the whole point of the strand, whose
/// complaint was the silence, not the missing file.
#[test]
fn aliases_in_default_project_warns() {
    let (project_dir, summary) = render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "project:\n  type: default\n  output-dir: _out\n",
        );
        write(
            &dir.join("index.qmd"),
            "---\ntitle: Home\naliases:\n  - /old.html\n---\n\nH.\n",
        );
    });

    assert!(
        !project_dir.join("_out/old.html").exists(),
        "default project should not write stubs"
    );
    assert!(
        summary
            .project_diagnostics
            .iter()
            .any(|d| d.title.to_lowercase().contains("aliases")),
        "expected a warning naming `aliases`; got: {:?}",
        summary
            .project_diagnostics
            .iter()
            .map(|d| d.title.clone())
            .collect::<Vec<_>>()
    );
}

// ═══════════════════════════════════════════════════════════════════
// Collisions — hard errors, diverging from Q1
// ═══════════════════════════════════════════════════════════════════

/// An alias that would land on a real rendered page. Q1 warns and
/// skips; we refuse to render, because the alternative is a site whose
/// author believes a redirect exists when it does not.
#[test]
fn aliases_stub_over_rendered_page_is_error() {
    let (_dir, result) = try_render_project(|dir| {
        write(&dir.join("_quarto.yml"), WEBSITE_YML);
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nH.\n");
        write(
            &dir.join("page/index.qmd"),
            "---\ntitle: Page\naliases:\n  - /index.html\n---\n\nP.\n",
        );
    });

    let rendered = expect_error_code(result, "Q-5-23");
    assert!(
        rendered.contains("index.html"),
        "diagnostic should name the contested path; got:\n{rendered}"
    );
}

/// Two pages claiming the same alias under the same fragment key.
/// Q1 is silent here and the last writer wins, so one of the two
/// authors gets a redirect pointing at the other's page.
#[test]
fn aliases_duplicate_fragment_key_is_error() {
    let (_dir, result) = try_render_project(|dir| {
        write(&dir.join("_quarto.yml"), WEBSITE_YML);
        write(
            &dir.join("one/index.qmd"),
            "---\ntitle: One\naliases:\n  - /shared.html\n---\n\n1.\n",
        );
        write(
            &dir.join("two/index.qmd"),
            "---\ntitle: Two\naliases:\n  - /shared.html\n---\n\n2.\n",
        );
    });

    let rendered = expect_error_code(result, "Q-5-24");
    assert!(
        rendered.contains("shared.html"),
        "diagnostic should name the contested alias; got:\n{rendered}"
    );
}

/// Two pages whose aliases differ only by case. This must fail on
/// every platform — a Linux CI build that let it through would ship a
/// site that breaks the moment it is served from a case-insensitive
/// filesystem.
#[test]
fn aliases_case_only_collision_is_error() {
    let (_dir, result) = try_render_project(|dir| {
        write(&dir.join("_quarto.yml"), WEBSITE_YML);
        write(
            &dir.join("one/index.qmd"),
            "---\ntitle: One\naliases:\n  - /Shared.html\n---\n\n1.\n",
        );
        write(
            &dir.join("two/index.qmd"),
            "---\ntitle: Two\naliases:\n  - /shared.html\n---\n\n2.\n",
        );
    });

    let rendered = expect_error_code(result, "Q-5-25");
    assert!(
        rendered.contains("Shared.html") && rendered.contains("shared.html"),
        "diagnostic should show both spellings; got:\n{rendered}"
    );
}

/// An alias that climbs above the output directory would write outside
/// the site. Q1 has no guard here at all.
#[test]
fn aliases_escaping_output_dir_is_error() {
    let (_dir, result) = try_render_project(|dir| {
        write(&dir.join("_quarto.yml"), WEBSITE_YML);
        write(
            &dir.join("index.qmd"),
            "---\ntitle: Home\naliases:\n  - ../escaped.html\n---\n\nH.\n",
        );
    });

    let rendered = expect_error_code(result, "Q-5-26");
    assert!(
        rendered.contains("escaped.html"),
        "diagnostic should name the offending alias; got:\n{rendered}"
    );
}

/// Several bad aliases in one project must all be reported from a
/// single render. A 69-file project should not learn about its
/// mistakes one render at a time.
#[test]
fn aliases_all_collisions_reported_together() {
    let (_dir, result) = try_render_project(|dir| {
        write(&dir.join("_quarto.yml"), WEBSITE_YML);
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nH.\n");
        write(
            &dir.join("one/index.qmd"),
            "---\ntitle: One\naliases:\n  - /shared.html\n  - /index.html\n---\n\n1.\n",
        );
        write(
            &dir.join("two/index.qmd"),
            "---\ntitle: Two\naliases:\n  - /shared.html\n---\n\n2.\n",
        );
    });

    let err = result.expect_err("expected failure");
    let QuartoError::Parse(parse) = &err else {
        panic!("expected QuartoError::Parse, got {err:?}");
    };
    let codes: Vec<String> = parse
        .diagnostics
        .iter()
        .filter_map(|d| d.code.clone())
        .collect();
    assert!(
        codes.iter().any(|c| c == "Q-5-23") && codes.iter().any(|c| c == "Q-5-24"),
        "both the page collision and the duplicate alias should be reported \
         in one pass; got {codes:?}"
    );
}
