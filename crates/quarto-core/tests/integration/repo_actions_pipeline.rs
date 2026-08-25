/*
 * tests/repo_actions_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Integration tests for website repo-actions
 * (bd-repo-actions-missing-99ezd2fe): the "Edit this page" / "View
 * source" / "Report an issue" links, end-to-end through
 * `ProjectPipeline`.
 */

//! Every test writes a small website fixture to a temp dir, drives it
//! through the real `ProjectPipeline`, then inspects the rendered
//! HTML — same harness shape as `sidebar_pipeline.rs`.

use std::path::PathBuf;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::RenderToFileOptions;
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

/// Build and render a website project fixture. Returns the map of
/// `project-relative output href → rendered HTML` (href, not stem —
/// breadcrumb fixtures have several `index.html` at different depths).
fn render_project(fixture: impl FnOnce(&std::path::Path)) -> Vec<(String, String)> {
    render_project_to_site(fixture).1
}

/// As `render_project`, but also hands back the `_site` root so a test
/// can inspect the non-HTML artifacts (`site_libs/**`, notably the
/// compiled theme CSS).
fn render_project_to_site(
    fixture: impl FnOnce(&std::path::Path),
) -> (PathBuf, Vec<(String, String)>) {
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
        summary.pass2_failures
    );
    std::mem::forget(temp); // keep files alive for inspection

    let site_root = project_dir.join("_site");
    let outputs = summary
        .outputs
        .iter()
        .map(|out| {
            let href = out
                .output_path
                .strip_prefix(&site_root)
                .unwrap_or(&out.output_path)
                .to_string_lossy()
                .replace('\\', "/");
            (href, read(&out.output_path))
        })
        .collect();
    (site_root, outputs)
}

fn find_html<'a>(outputs: &'a [(String, String)], href: &str) -> &'a str {
    &outputs
        .iter()
        .find(|(h, _)| h == href)
        .unwrap_or_else(|| {
            panic!(
                "no output for href '{}'; got: {:?}",
                href,
                outputs.iter().map(|(h, _)| h).collect::<Vec<_>>()
            )
        })
        .1
}

/// The standard fixture: a website with repo-actions and a TOC.
/// `website_extra` is spliced inside the `website:` block and must
/// therefore arrive already indented by two spaces.
fn fixture(project_dir: &std::path::Path, website_extra: &str, front_matter: &str) {
    write(
        &project_dir.join("_quarto.yml"),
        &format!(
            concat!(
                "project:\n",
                "  type: website\n",
                "website:\n",
                "  title: \"Site\"\n",
                "  repo-url: https://github.com/example/docs\n",
                "  repo-branch: main\n",
                "{}",
                "format:\n",
                "  html:\n",
                "    toc: true\n",
            ),
            website_extra
        ),
    );
    write(
        &project_dir.join("index.qmd"),
        &format!("---\ntitle: Home\n{front_matter}---\n\n## One\n\nText.\n"),
    );
}

#[test]
fn repo_actions_render_in_both_placements() {
    let outputs = render_project(|dir| {
        fixture(dir, "  repo-actions: [edit, source, issue]\n", "");
    });
    let html = find_html(&outputs, "index.html");

    assert_eq!(
        html.matches("class=\"toc-actions").count(),
        2,
        "one TOC copy, one footer copy"
    );
    assert_eq!(html.matches("Edit this page").count(), 2);
    assert_eq!(html.matches("View source").count(), 2);
    assert_eq!(html.matches("Report an issue").count(), 2);

    assert!(html.contains("https://github.com/example/docs/edit/main/index.qmd"));
    assert!(html.contains("https://github.com/example/docs/blob/main/index.qmd"));
    assert!(html.contains("https://github.com/example/docs/issues/new"));

    // The TOC copy sits inside nav#TOC; the footer copy carries the
    // responsive classes and sits in .nav-footer-center.
    let nav = html.split("<nav id=\"TOC\"").nth(1).expect("nav#TOC");
    let nav = &nav[..nav.find("</nav>").expect("nav close")];
    assert!(
        nav.contains("class=\"toc-actions\"><ul>"),
        "plain classes in the TOC copy"
    );

    let center = html
        .split("nav-footer-center")
        .nth(1)
        .expect("footer center");
    assert!(center.contains("toc-actions d-sm-block d-md-none"));
}

#[test]
fn footer_is_synthesized_when_no_page_footer_is_configured() {
    let outputs = render_project(|dir| {
        fixture(dir, "  repo-actions: [edit]\n", "");
    });
    let html = find_html(&outputs, "index.html");
    assert!(html.contains("<footer class=\"footer\""));
    assert!(html.contains("nav-footer-center"));
}

#[test]
fn actions_follow_existing_footer_content() {
    let outputs = render_project(|dir| {
        fixture(
            dir,
            "  repo-actions: [edit]\n  page-footer:\n    center: \"Version 1.0\"\n",
            "",
        );
    });
    let html = find_html(&outputs, "index.html");
    let center = html
        .split("<div class=\"nav-footer-center\">")
        .nth(1)
        .expect("center");
    let center = &center[..center.find("</div>\n").unwrap_or(center.len())];
    assert!(
        center.find("Version 1.0").unwrap() < center.find("toc-actions").unwrap(),
        "configured text must precede the appended actions"
    );
}

#[test]
fn page_level_false_suppresses_actions_on_that_page_only() {
    let outputs = render_project(|dir| {
        fixture(dir, "  repo-actions: [edit]\n", "repo-actions: false\n");
        write(
            &dir.join("other.qmd"),
            "---\ntitle: Other\n---\n\n## One\n\nText.\n",
        );
    });
    assert_eq!(
        find_html(&outputs, "index.html")
            .matches("toc-actions")
            .count(),
        0
    );
    assert!(
        find_html(&outputs, "other.html")
            .matches("toc-actions")
            .count()
            > 0
    );
}

/// D-3, at the website scope — the one the transform's top-level gate
/// cannot see. If the synthesis branch forgets its website-aware
/// re-check, this test is what catches it.
#[test]
fn page_footer_false_suppresses_only_the_footer_copy() {
    let outputs = render_project(|dir| {
        fixture(dir, "  repo-actions: [edit]\n  page-footer: false\n", "");
    });
    let html = find_html(&outputs, "index.html");
    assert!(!html.contains("nav-footer-center"), "no footer at all");
    let nav = html.split("<nav id=\"TOC\"").nth(1).expect("nav#TOC");
    assert!(nav.contains("toc-actions"), "the TOC copy is unaffected");
}

#[test]
fn no_toc_yields_a_single_always_visible_footer_copy() {
    let outputs = render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            concat!(
                "project:\n",
                "  type: website\n",
                "website:\n",
                "  title: \"Site\"\n",
                "  repo-url: https://github.com/example/docs\n",
                "  repo-actions: [edit]\n",
                "format:\n",
                "  html:\n",
                "    toc: false\n",
            ),
        );
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nText.\n");
    });
    let html = find_html(&outputs, "index.html");
    assert_eq!(html.matches("class=\"toc-actions").count(), 1);
    assert!(
        !html.contains("d-sm-block"),
        "no TOC copy to fall back from"
    );
}

/// The website-left placement builds its `<nav>` in Rust
/// (`toc_block_html`, called from `SidebarRenderTransform`) rather than
/// through the `toc-block` template partial. It is the one placement a
/// wrong pipeline slot would silently break, and the only one this
/// suite covers besides the default `right`.
#[test]
fn website_left_toc_placement_gets_the_actions_copy() {
    let outputs = render_project(|dir| {
        fixture(dir, "  repo-actions: [edit, source]\n", "");
        // Re-write _quarto.yml with toc-location: left; the sidebar
        // gives SidebarRenderTransform something to merge the TOC into.
        write(
            &dir.join("_quarto.yml"),
            concat!(
                "project:\n",
                "  type: website\n",
                "website:\n",
                "  title: \"Site\"\n",
                "  repo-url: https://github.com/example/docs\n",
                "  repo-branch: main\n",
                "  repo-actions: [edit, source]\n",
                "  sidebar:\n",
                "    contents:\n",
                "      - index.qmd\n",
                "format:\n",
                "  html:\n",
                "    toc: true\n",
                "    toc-location: left\n",
            ),
        );
    });
    let html = find_html(&outputs, "index.html");
    let nav = html.split("<nav id=\"TOC\"").nth(1).expect("nav#TOC");
    let nav = &nav[..nav.find("</nav>").expect("nav close")];
    assert!(
        nav.contains("toc-actions"),
        "the Rust-built TOC nav must carry the actions; if this fails, \
         repo-actions-render is running after sidebar-render"
    );
}

#[test]
fn nested_pages_get_paths_relative_to_the_project_root() {
    let outputs = render_project(|dir| {
        fixture(dir, "  repo-actions: [source]\n", "");
        write(
            &dir.join("guide/intro.qmd"),
            "---\ntitle: Intro\n---\n\n## One\n\nText.\n",
        );
    });
    let html = find_html(&outputs, "guide/intro.html");
    assert!(html.contains("https://github.com/example/docs/blob/main/guide/intro.qmd"));
}

#[test]
fn repo_subdir_is_prepended() {
    let outputs = render_project(|dir| {
        fixture(
            dir,
            "  repo-subdir: website\n  repo-actions: [source]\n",
            "",
        );
    });
    let html = find_html(&outputs, "index.html");
    assert!(html.contains("https://github.com/example/docs/blob/main/website/index.qmd"));
}

#[test]
fn issue_url_overrides_and_forces_an_issue_link() {
    let outputs = render_project(|dir| {
        fixture(
            dir,
            "  issue-url: https://example.com/bugs\n  repo-actions: [edit]\n",
            "",
        );
    });
    let html = find_html(&outputs, "index.html");
    assert!(html.contains("https://example.com/bugs"));
    assert!(html.contains("Report an issue"));
}

#[test]
fn link_target_and_rel_are_applied() {
    let outputs = render_project(|dir| {
        fixture(
            dir,
            "  repo-link-target: _blank\n  repo-link-rel: noopener\n  repo-actions: [edit]\n",
            "",
        );
    });
    let html = find_html(&outputs, "index.html");
    assert!(html.contains("target=\"_blank\" rel=\"noopener\" class=\"toc-action\""));
}

#[test]
fn no_repo_actions_configured_changes_nothing() {
    let outputs = render_project(|dir| {
        fixture(dir, "", "");
    });
    let html = find_html(&outputs, "index.html");
    assert!(!html.contains("toc-actions"));
    assert!(!html.contains("<footer class=\"footer\""));
}

// ---------------------------------------------------------------------------
// Footer styling (bd-repo-actions-footer-unstyled-80xtt35y)
//
// The markup half of repo-actions shipped complete, but the page-footer
// copy inherited no CSS: q2 scoped every `.toc-actions` rule under
// `.sidebar`, so the footer links rendered as a browser-default bulleted
// list instead of Q1's centred flex row. The rules live in Q1 at
// `src/resources/projects/website/navigation/quarto-nav.scss:770-798`.
// ---------------------------------------------------------------------------

/// Concatenate every `.css` file the render dropped under `_site`.
fn site_css(site_root: &std::path::Path) -> String {
    fn walk(dir: &std::path::Path, out: &mut String) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "css") {
                out.push_str(&read(&path));
                out.push('\n');
            }
        }
    }
    let mut out = String::new();
    walk(site_root, &mut out);
    assert!(!out.is_empty(), "render emitted no CSS at all");
    out
}

/// Declaration blocks of every rule whose selector list mentions
/// `.nav-footer` *and* `toc-action`. The emitted CSS is minified, so we
/// match on structure (`selector{decls}`) rather than on formatting.
fn footer_action_rules(css: &str) -> Vec<(String, String)> {
    let mut rules = Vec::new();
    let mut rest = css;
    while let Some(open) = rest.find('{') {
        let selector = rest[..open].rsplit('}').next().unwrap_or("").trim();
        let Some(close) = rest[open..].find('}') else {
            break;
        };
        let decls = &rest[open + 1..open + close];
        if selector.contains(".nav-footer") && selector.contains("toc-action") {
            rules.push((selector.to_string(), decls.to_string()));
        }
        rest = &rest[open + close..];
    }
    rules
}

#[test]
fn footer_repo_actions_ship_their_css() {
    let (site_root, outputs) = render_project_to_site(|dir| {
        fixture(dir, "  repo-actions: [edit, source, issue]\n", "");
    });

    // Precondition: the markup is there. If this fires, the test is
    // measuring the wrong thing.
    let html = find_html(&outputs, "index.html");
    assert!(
        html.contains("toc-actions d-sm-block d-md-none"),
        "fixture should emit the footer copy of the actions"
    );

    let css = site_css(&site_root);
    let rules = footer_action_rules(&css);
    assert!(
        !rules.is_empty(),
        "no `.nav-footer … toc-action…` rule reached the rendered CSS — the \
         footer links fall back to a bulleted list (bd-repo-actions-footer-unstyled)"
    );

    let find = |needle: &str| -> Option<&(String, String)> {
        rules.iter().find(|(sel, _)| sel.contains(needle))
    };

    // The two declarations that produce the visible symptom: without
    // them the `<ul>` is a bulleted block, not a horizontal row.
    let ul = find("toc-actions ul").expect("a rule scoped to `.nav-footer .toc-actions ul`");
    assert!(
        ul.1.contains("display:flex"),
        "`{}` must set display:flex, got `{{{}}}`",
        ul.0,
        ul.1
    );
    assert!(
        ul.1.contains("list-style:none"),
        "`{}` must set list-style:none, got `{{{}}}`",
        ul.0,
        ul.1
    );

    // The `auto` margin pair is what centres the row.
    let first = find(":first-child").expect("a `:first-child` rule centring the row");
    assert!(
        first.1.contains("margin-left:auto"),
        "`{}` must set margin-left:auto, got `{{{}}}`",
        first.0,
        first.1
    );
    let last = find(":last-child").expect("a `:last-child` rule centring the row");
    assert!(
        last.1.contains("margin-right:auto"),
        "`{}` must set margin-right:auto, got `{{{}}}`",
        last.0,
        last.1
    );

    // Inter-item spacing, the icon gap, and the trailing-item reset.
    let li = find("toc-actions ul li").expect("a rule spacing the `li`s");
    assert!(
        li.1.contains("padding-right:1.5em"),
        "`{}` must set padding-right:1.5em, got `{{{}}}`",
        li.0,
        li.1
    );
    assert!(
        find("i.bi").is_some_and(|(_, d)| d.contains("padding-right:.4em")),
        "the footer `i.bi` icon gap is missing; rules: {:?}",
        rules.iter().map(|(s, _)| s).collect::<Vec<_>>()
    );
    assert!(
        find("li:last-of-type").is_some_and(|(_, d)| d.contains("padding-right:0")),
        "the last item's padding reset is missing; rules: {:?}",
        rules.iter().map(|(s, _)| s).collect::<Vec<_>>()
    );

    // Link underlines off, and the block's own vertical padding.
    assert!(
        find("toc-actions a").is_some_and(|(_, d)| d.contains("text-decoration:none")),
        "footer action links must not be underlined; rules: {:?}",
        rules.iter().map(|(s, _)| s).collect::<Vec<_>>()
    );
    let block = rules
        .iter()
        .find(|(sel, _)| sel.trim_end().ends_with(".toc-actions"))
        .expect("a rule on `.nav-footer .toc-actions` itself");
    assert!(
        block.1.contains("padding-top:.5em") && block.1.contains("padding-bottom:.5em"),
        "`{}` must carry the 0.5em vertical padding, got `{{{}}}`",
        block.0,
        block.1
    );
}

/// The sidebar and footer `.toc-actions` treatments are deliberately
/// different in kind — a vertical themed list vs. a centred flex row.
/// Adding the footer rules must not disturb the sidebar ones.
#[test]
fn footer_css_does_not_disturb_the_sidebar_rules() {
    let (site_root, _) = render_project_to_site(|dir| {
        fixture(dir, "  repo-actions: [edit, source, issue]\n", "");
    });
    let css = site_css(&site_root);

    assert!(
        css.contains(".sidebar .toc-actions"),
        "the sidebar `.toc-actions` rules must still be present"
    );
    for (selector, decls) in footer_action_rules(&css) {
        assert!(
            !selector.contains(".sidebar"),
            "footer rule `{}` must not also target the sidebar (`{{{}}}`)",
            selector,
            decls
        );
    }
}
