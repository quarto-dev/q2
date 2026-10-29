/*
 * tests/integration/linked_resources.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Auto-discovery of linked local files (bd-twfep08m).
 *
 * A Markdown link to an existing, in-project, non-document file
 * publishes that file next to the page, as Quarto 1 does. The walk
 * feeds `meta.resources`, so post filters can prune what it found, and
 * `resource-discovery: false` (merged metadata) turns it off.
 *
 * Plan: claude-notes/plans/2026-10-29-linked-resource-discovery.md
 */

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, ProjectRenderSummary, project_type_for};
use quarto_core::render_to_file::RenderToFileOptions;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn canonical(path: &Path) -> PathBuf {
    quarto_system_runtime::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

const WEBSITE: &str = "project:\n  type: website\nwebsite:\n  title: t\n";

/// A fresh website project; returns the temp dir guard and its
/// canonical root.
fn website(extra_config: &str) -> (TempDir, PathBuf) {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write(
        &dir.join("_quarto.yml"),
        &format!("{WEBSITE}{extra_config}"),
    );
    write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nHome.\n");
    (temp, dir)
}

/// Run the project pipeline and return the summary plus the output dir.
fn run(project_dir: &Path) -> (ProjectRenderSummary, PathBuf) {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(project_dir, runtime.as_ref()).expect("discover");
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
    let summary = pollster::block_on(pipeline.run()).expect("run");
    (summary, project.output_dir.clone())
}

/// Render and assert that every document succeeded.
fn render(project_dir: &Path) -> PathBuf {
    let (summary, output_dir) = run(project_dir);
    let failures: Vec<_> = summary
        .pass1_failures
        .iter()
        .chain(&summary.pass2_failures)
        .map(|f| (&f.input, &f.error))
        .collect();
    assert!(failures.is_empty(), "render failures: {failures:?}");
    output_dir
}

/// Every file under `dir`, project-relative and `/`-separated, sorted,
/// excluding `site_libs/`.
fn files_under(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path.strip_prefix(root).unwrap();
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.retain(|p| !p.starts_with("site_libs/"));
    out.sort();
    out
}

fn assert_copied(output_dir: &Path, rel: &str) {
    assert!(
        output_dir.join(rel).is_file(),
        "{rel} should be copied; output has {:?}",
        files_under(output_dir)
    );
}

fn assert_not_copied(output_dir: &Path, rel: &str) {
    assert!(
        !output_dir.join(rel).exists(),
        "{rel} should NOT be copied; output has {:?}",
        files_under(output_dir)
    );
}

// ─────────────────────────────────────────────────────────────────────
// What gets discovered
// ─────────────────────────────────────────────────────────────────────

/// The forwarded report's repro: two linked files next to the page,
/// one via a `../` path back into the same directory.
#[test]
fn linked_files_next_to_the_page_are_copied() {
    let (_t, dir) = website("");
    write(&dir.join("guide/dashboard.json"), "{}\n");
    write(&dir.join("guide/data.csv"), "a,b\n");
    write(
        &dir.join("guide/index.qmd"),
        "---\ntitle: Guide\n---\n\n\
         - [Dashboard](dashboard.json){download=\"my-dashboard.json\"}\n\
         - [Data](data.csv)\n\
         - [From home](../guide/data.csv)\n",
    );

    let out = render(&dir);

    assert_copied(&out, "guide/dashboard.json");
    assert_copied(&out, "guide/data.csv");
    assert_eq!(
        std::fs::read_to_string(out.join("guide/data.csv")).unwrap(),
        "a,b\n"
    );
    let html = std::fs::read_to_string(out.join("guide/index.html")).unwrap();
    assert!(
        html.contains(r#"href="dashboard.json" download="my-dashboard.json""#),
        "the link itself is unchanged: {html}"
    );
}

/// The lookup ignores `?query` / `#fragment` and percent-decodes.
#[test]
fn query_fragment_and_percent_encoding_are_resolved() {
    let (_t, dir) = website("");
    write(&dir.join("frag.csv"), "x\n");
    write(&dir.join("query.csv"), "x\n");
    write(&dir.join("my file.csv"), "x\n");
    write(
        &dir.join("page.qmd"),
        "---\ntitle: P\n---\n\n\
         [f](frag.csv#row=3) [q](query.csv?x=1) [s](my%20file.csv)\n",
    );

    let out = render(&dir);

    assert_copied(&out, "frag.csv");
    assert_copied(&out, "query.csv");
    assert_copied(&out, "my file.csv");
}

/// Root-relative targets, subdirectories, and links nested in
/// footnotes, tables, callouts and spans are all found.
#[test]
fn nested_and_root_relative_links_are_found() {
    let (_t, dir) = website("");
    for f in [
        "guide/root.csv",
        "guide/sub/inner.csv",
        "guide/footnote.csv",
        "guide/table.csv",
        "guide/callout.csv",
        "guide/_underscore.csv",
    ] {
        write(&dir.join(f), "x\n");
    }
    write(
        &dir.join("guide/index.qmd"),
        "---\ntitle: G\n---\n\n\
         [root](/guide/root.csv) and [inner](sub/inner.csv) and [u](_underscore.csv).\n\n\
         Note.[^1]\n\n\
         [^1]: [fn](footnote.csv)\n\n\
         | a |\n|---|\n| [t](table.csv) |\n\n\
         ::: {.callout-note}\n[c](callout.csv)\n:::\n",
    );

    let out = render(&dir);

    for f in [
        "guide/root.csv",
        "guide/sub/inner.csv",
        "guide/footnote.csv",
        "guide/table.csv",
        "guide/callout.csv",
        "guide/_underscore.csv",
    ] {
        assert_copied(&out, f);
    }
}

/// Links produced by a shortcode or by a pre filter are discovered:
/// the walk runs after both.
#[test]
fn shortcode_and_pre_filter_links_are_found() {
    let (_t, dir) = website("");
    write(&dir.join("sc.csv"), "x\n");
    write(&dir.join("pre.csv"), "x\n");
    write(
        &dir.join("make-link.lua"),
        "function Para(p)\n\
         \x20 if pandoc.utils.stringify(p) == 'MARK' then\n\
         \x20   return pandoc.Para({pandoc.Link('pre', 'pre.csv')})\n\
         \x20 end\n\
         end\n",
    );
    write(
        &dir.join("page.qmd"),
        "---\ntitle: P\nlink: sc.csv\nfilters:\n  - make-link.lua\n---\n\n\
         MARK\n\n[sc]({{< meta link >}})\n",
    );

    let out = render(&dir);

    assert_copied(&out, "sc.csv");
    assert_copied(&out, "pre.csv");
}

/// Everything that is not a Markdown link to an existing, in-project,
/// non-document file is left alone — and none of it fails the render.
#[test]
fn non_candidates_are_not_copied() {
    let temp = TempDir::new().unwrap();
    let root = canonical(temp.path());
    let dir = root.join("proj");
    write(&dir.join("_quarto.yml"), WEBSITE);
    write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nHome.\n");
    write(&root.join("outside.csv"), "x\n");
    write(&dir.join(".hidden/secret.csv"), "x\n");
    write(&dir.join(".dotfile.csv"), "x\n");
    write(&dir.join("other.qmd"), "---\ntitle: O\n---\n");
    write(&dir.join("_draft.qmd"), "---\ntitle: D\n---\n");
    write(&dir.join("dir/inner.csv"), "x\n");
    for f in ["rawhtml.csv", "code.csv", "attr.csv"] {
        write(&dir.join(f), "x\n");
    }
    write(
        &dir.join("page.qmd"),
        "---\ntitle: P\n---\n\n\
         [o](other.qmd) [d](_draft.qmd) [a](#top) [m](mailto:a@b.c) [h](https://example.com/x.csv)\n\n\
         [out](../outside.csv) [hidden](.hidden/secret.csv) [dot](.dotfile.csv)\n\n\
         [missing](missing.csv) [dir](dir/)\n\n\
         `[code](code.csv)` [attr]{data-href=\"attr.csv\"}\n\n\
         ```{=html}\n<a href=\"rawhtml.csv\">raw</a>\n```\n",
    );

    let out = render(&dir);

    for f in [
        "other.qmd",
        "_draft.qmd",
        "outside.csv",
        ".hidden/secret.csv",
        ".dotfile.csv",
        "missing.csv",
        "dir/inner.csv",
        "rawhtml.csv",
        "code.csv",
        "attr.csv",
    ] {
        assert_not_copied(&out, f);
    }
}

/// A file inside the output directory is never a candidate (it would be
/// copied onto itself, or a previous render's output republished).
#[test]
fn files_under_the_output_dir_are_not_candidates() {
    let (_t, dir) = website("");
    write(&dir.join("_site/stale.csv"), "old\n");
    write(
        &dir.join("page.qmd"),
        "---\ntitle: P\n---\n\n[s](_site/stale.csv)\n",
    );

    let out = render(&dir);

    assert_not_copied(&out, "_site/stale.csv");
}

/// A file that looks like another format's render output of a project
/// document (`report.pdf` next to `report.qmd`) is not copied: a stale
/// in-place render must not overwrite the fresh output.
#[test]
fn sibling_render_outputs_of_project_documents_are_not_copied() {
    let (_t, dir) = website("");
    write(&dir.join("report.qmd"), "---\ntitle: R\n---\n");
    write(&dir.join("report.pdf"), "stale\n");
    write(&dir.join("report.csv"), "data\n");
    write(
        &dir.join("page.qmd"),
        "---\ntitle: P\n---\n\n[pdf](report.pdf) [csv](report.csv)\n",
    );

    let out = render(&dir);

    assert_not_copied(&out, "report.pdf");
    // Only render-output extensions are treated this way.
    assert_copied(&out, "report.csv");
}

/// An un-rendered non-`.qmd` document (a notebook offered as a
/// download) is copied, as Quarto 1 does.
#[test]
fn unrendered_notebook_is_copied() {
    let (_t, dir) = website("");
    write(&dir.join("notebooks/analysis.ipynb"), "{}\n");
    write(
        &dir.join("page.qmd"),
        "---\ntitle: P\n---\n\n[nb](notebooks/analysis.ipynb)\n",
    );

    let out = render(&dir);

    assert_copied(&out, "notebooks/analysis.ipynb");
}

// ─────────────────────────────────────────────────────────────────────
// `resource-discovery` (merged metadata)
// ─────────────────────────────────────────────────────────────────────

fn linking_page(dir: &Path, rel: &str, frontmatter_extra: &str) {
    let page = dir.join(rel);
    write(&page.with_file_name("data.csv"), "x\n");
    write(
        &page,
        &format!("---\ntitle: P\n{frontmatter_extra}---\n\n[d](data.csv)\n"),
    );
}

#[test]
fn front_matter_false_disables_discovery() {
    let (_t, dir) = website("");
    linking_page(&dir, "page.qmd", "resource-discovery: false\n");

    let out = render(&dir);

    assert_not_copied(&out, "data.csv");
}

#[test]
fn metadata_yml_false_disables_discovery_for_its_directory() {
    let (_t, dir) = website("");
    write(
        &dir.join("off/_metadata.yml"),
        "resource-discovery: false\n",
    );
    linking_page(&dir, "off/page.qmd", "");
    linking_page(&dir, "on/page.qmd", "");

    let out = render(&dir);

    assert_not_copied(&out, "off/data.csv");
    assert_copied(&out, "on/data.csv");
}

#[test]
fn project_false_disables_and_document_true_overrides() {
    let (_t, dir) = website("resource-discovery: false\n");
    linking_page(&dir, "a/page.qmd", "");
    linking_page(&dir, "b/page.qmd", "resource-discovery: true\n");

    let out = render(&dir);

    assert_not_copied(&out, "a/data.csv");
    assert_copied(&out, "b/data.csv");
}

#[test]
fn format_level_false_disables_discovery() {
    let (_t, dir) = website("format:\n  html:\n    resource-discovery: false\n");
    linking_page(&dir, "page.qmd", "");

    let out = render(&dir);

    assert_not_copied(&out, "data.csv");
}

/// A non-boolean value is an error (`Q-5-52`) that fails the document:
/// a typo such as `flase` must not publish files the author meant to
/// keep out.
#[test]
fn non_boolean_value_is_an_error_and_copies_nothing() {
    let (_t, dir) = website("");
    linking_page(&dir, "page.qmd", "resource-discovery: flase\n");

    let (summary, out) = run(&dir);

    let failure = summary
        .pass2_failures
        .iter()
        .find(|f| f.input.ends_with("page.qmd"))
        .unwrap_or_else(|| {
            panic!(
                "page.qmd should fail; summary: {:?}",
                summary.pass2_failures
            )
        });
    assert!(
        failure
            .diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("Q-5-52")),
        "expected Q-5-52, got {:?} / {}",
        failure.diagnostics,
        failure.error
    );
    assert_not_copied(&out, "data.csv");
}

// ─────────────────────────────────────────────────────────────────────
// Lua control through `meta.resources`
// ─────────────────────────────────────────────────────────────────────

/// A post filter that removes a discovered entry from `meta.resources`
/// stops the copy; an author-declared entry it removes is still
/// published (the bd-o8pr additivity defense).
#[test]
fn post_filter_can_prune_discovered_but_not_declared_entries() {
    let (_t, dir) = website("");
    for f in ["keep.csv", "prune.csv", "declared.csv"] {
        write(&dir.join(f), "x\n");
    }
    write(
        &dir.join("prune.lua"),
        "function Meta(meta)\n\
         \x20 local out = pandoc.List()\n\
         \x20 for _, r in ipairs(meta.resources or {}) do\n\
         \x20   local s = pandoc.utils.stringify(r)\n\
         \x20   if s ~= 'prune.csv' and s ~= 'declared.csv' then out:insert(r) end\n\
         \x20 end\n\
         \x20 meta.resources = out\n\
         \x20 return meta\n\
         end\n",
    );
    write(
        &dir.join("page.qmd"),
        "---\ntitle: P\nresources:\n  - declared.csv\nfilters:\n  - at: post-quarto\n    path: prune.lua\n---\n\n\
         [k](keep.csv) [p](prune.csv)\n",
    );

    let out = render(&dir);

    assert_copied(&out, "keep.csv");
    assert_not_copied(&out, "prune.csv");
    assert_copied(&out, "declared.csv");
}

/// A post filter sees the discovered entries in `meta.resources`.
/// A pre filter can turn discovery off for the document.
#[test]
fn pre_filter_can_disable_discovery() {
    let (_t, dir) = website("");
    write(&dir.join("data.csv"), "x\n");
    write(
        &dir.join("off.lua"),
        "function Meta(meta)\n\
         \x20 meta['resource-discovery'] = false\n\
         \x20 return meta\n\
         end\n",
    );
    write(
        &dir.join("page.qmd"),
        "---\ntitle: P\nfilters:\n  - off.lua\n---\n\n[d](data.csv)\n",
    );

    let out = render(&dir);

    assert_not_copied(&out, "data.csv");
}

/// The render manifest attributes discovered files to auto-discovery,
/// and filter additions to the filter.
#[test]
fn manifest_records_auto_discovery_origin() {
    let (_t, dir) = website("");
    write(&dir.join("linked.csv"), "x\n");
    write(&dir.join("from-filter.csv"), "x\n");
    write(
        &dir.join("add.lua"),
        "function Meta(meta)\n\
         \x20 local r = meta.resources or pandoc.List()\n\
         \x20 r:insert('from-filter.csv')\n\
         \x20 meta.resources = r\n\
         \x20 return meta\n\
         end\n",
    );
    write(
        &dir.join("page.qmd"),
        "---\ntitle: P\nfilters:\n  - at: post-quarto\n    path: add.lua\n---\n\n[l](linked.csv)\n",
    );

    let _out = render(&dir);

    let body = std::fs::read_to_string(dir.join(".quarto/render-manifest.json")).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&body).unwrap();
    let origin_of = |source: &str| -> serde_json::Value {
        manifest["resources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["source"] == source)
            .unwrap_or_else(|| panic!("{source} missing from manifest: {body}"))["origin"]
            .clone()
    };
    let linked = origin_of("linked.csv");
    assert_eq!(linked["kind"], "AutoDiscovery", "{linked}");
    assert_eq!(linked["via"], "link", "{linked}");
    assert_eq!(origin_of("from-filter.csv")["kind"], "LuaFilter");
}

// ─────────────────────────────────────────────────────────────────────
// Other project types
// ─────────────────────────────────────────────────────────────────────

/// A default project with a separate `output-dir` copies linked files
/// too; nothing about discovery is website-specific.
#[test]
fn default_project_with_output_dir_copies_linked_files() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write(&dir.join("_quarto.yml"), "project:\n  output-dir: _out\n");
    write(&dir.join("data/table.csv"), "x\n");
    write(
        &dir.join("doc.qmd"),
        "---\ntitle: D\n---\n\n[t](data/table.csv)\n",
    );

    let out = render(&dir);

    assert_copied(&out, "data/table.csv");
}
