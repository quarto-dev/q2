/*
 * tests/integration/book_refactor_guard.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Native refactor guard for the whole-book request phase (pandoc-request
 * R9, task 2). R9 splits `render_book_single_file` into a shared core and a
 * native tail; native `q2 render` of a book must not change. This test
 * records, for each book fixture rendered through the real
 * `run_with_book_support()` to typst and EPUB, everything the refactor could
 * move without changing a byte of `.typ`: the kept `.typ` text, the EPUB's
 * XHTML/OPF entries, the diagnostics (kind and title, in order), the failure
 * messages for a missing `.bib` / `.csl`, the `RenderToFileResult` fields,
 * the files that land in the output directory, and the
 * `.quarto/render-manifest.json` tail.
 *
 * The recordings are golden files under `book_refactor_guard/` (`eol=lf`).
 * Re-record deliberately (and review the diff) with
 * `BOOK_GUARD_RECORD=1 cargo nextest run -p quarto-core -E 'test(book_refactor_guard::)'`.
 * They are re-recorded on purpose after R9 task 1 (citeproc error text) and
 * task 5 (figure-caption images, part pages), which change native output.
 *
 * Like the other book tests this needs real `pandoc` and `typst` (and the
 * network once for `@preview/orange-book`). Execution is switched off
 * (`ExecutionPolicy::None`) so the recordings do not depend on engines.
 */

use std::fmt::Write as _;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;
use zip::ZipArchive;

use quarto_core::engine::ExecutionPolicy;
use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, ProjectRenderSummary, project_type_for};
use quarto_core::render_to_file::{RenderToFileOptions, RenderToFileResult};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// A minimal valid 1x1 PNG.
const ONE_PIXEL_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];

const NUMERIC_CSL: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" version="1.0" class="in-text" default-locale="en-US">
  <info>
    <title>Test Numeric</title>
    <id>http://www.zotero.org/styles/test-numeric</id>
    <link href="http://www.zotero.org/styles/test-numeric" rel="self"/>
    <author><name>Test</name></author>
    <category citation-format="numeric"/>
    <updated>2026-01-01T00:00:00+00:00</updated>
    <rights license="http://creativecommons.org/licenses/by-sa/3.0/">CC BY-SA</rights>
  </info>
  <citation>
    <sort><key variable="citation-number"/></sort>
    <layout delimiter=", "><text variable="citation-number" prefix="[" suffix="]"/></layout>
  </citation>
  <bibliography>
    <sort><key variable="citation-number"/></sort>
    <layout><text variable="citation-number" prefix="[" suffix="] "/><group delimiter=" "><names variable="author"><name/></names><date variable="issued" form="numeric" date-parts="year"/><text variable="title"/></group></layout>
  </bibliography>
</style>
"#;

const REFS_BIB: &str = r#"@book{knuth1984,
  author = {Knuth, Donald E.},
  title = {The TeXbook},
  year = {1984},
  publisher = {Addison-Wesley}
}
@article{doe2020,
  author = {Doe, Jane},
  title = {A Paper},
  year = {2020}
}
"#;

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn write(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn write_str(path: &Path, contents: &str) {
    write(path, contents.as_bytes());
}

// ---------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------

/// Index plus two chapters, nothing else.
fn fixture_basic(dir: &Path) {
    write_str(
        &dir.join("_quarto.yml"),
        "project:\n  type: book\n\nkeep-typ: true\n\nbook:\n  title: \"Guard Basic\"\n  author: \"A. Author\"\n  chapters:\n    - index.qmd\n    - ch1.qmd\n    - ch2.qmd\n",
    );
    write_str(
        &dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nWelcome to the book.\n",
    );
    write_str(&dir.join("ch1.qmd"), "# Chapter One\n\nBody one.\n");
    write_str(&dir.join("ch2.qmd"), "# Chapter Two\n\nBody two.\n");
}

/// The rich fixture: parts (one with an `href:` page that holds an image),
/// an appendix, a subdirectory chapter with local, `../` and site-root
/// images, a figure whose caption holds an image, cross-chapter references
/// and links, a title-only chapter, a chapter with both a title and its own
/// heading, and an EPUB cover image.
fn fixture_rich(dir: &Path) {
    write_str(
        &dir.join("_quarto.yml"),
        "project:\n  type: book\n\nkeep-typ: true\n\nbook:\n  title: \"Guard Rich\"\n  author: \"A. Author\"\n  cover-image: cover.png\n  chapters:\n    - index.qmd\n    - part: \"Part One\"\n      chapters:\n        - one.qmd\n        - sub/two.qmd\n    - part: \"Part Two\"\n      href: partpage.qmd\n      chapters:\n        - titleonly.qmd\n        - withh1.qmd\n  appendices:\n    - app.qmd\n",
    );
    write_str(
        &dir.join("index.qmd"),
        "---\ntitle: Preface\n---\n\nWelcome. See @sec-one and [chapter two](sub/two.qmd).\n",
    );
    write_str(
        &dir.join("one.qmd"),
        "# One {#sec-one}\n\n![root local](rootlocal.png)\n\n::: {#fig-cap}\n![](rootlocal.png){#fig-inner}\n\nFigure caption with ![icon](icon.png).\n:::\n\n```{=html}\n<img src=\"rawimg.png\">\n```\n",
    );
    write_str(
        &dir.join("sub/two.qmd"),
        "# Two {#sec-two}\n\n![local](local.png)\n\n![site root](/img/a.png)\n\n![up](../top.png)\n\nBack to @sec-one and [chapter one](../one.qmd).\n",
    );
    write_str(
        &dir.join("partpage.qmd"),
        "---\ntitle: Part Two Page\n---\n\nIntro to part two.\n",
    );
    write_str(
        &dir.join("titleonly.qmd"),
        "---\ntitle: Title Only Chapter\n---\n\nText with no heading.\n",
    );
    write_str(
        &dir.join("withh1.qmd"),
        "---\ntitle: Meta Title\n---\n\n# Heading Y\n\nBody y.\n",
    );
    write_str(&dir.join("app.qmd"), "# Appendix Alpha\n\nAppendix body.\n");
    for f in [
        "rootlocal.png",
        "icon.png",
        "rawimg.png",
        "top.png",
        "cover.png",
        "img/a.png",
        "sub/local.png",
    ] {
        write(&dir.join(f), ONE_PIXEL_PNG);
    }
}

/// A part page (`href:`) that holds an image, after a subdirectory chapter.
/// Native currently rebases the part page's image against the previous
/// item's directory (`sub/part.png`), so the typst compile fails; recorded
/// as-is and re-recorded when R9 task 5 fixes it.
fn fixture_partpage_image(dir: &Path) {
    write_str(
        &dir.join("_quarto.yml"),
        "project:\n  type: book\n\nkeep-typ: true\n\nbook:\n  title: \"Guard Part Page\"\n  author: \"A. Author\"\n  chapters:\n    - index.qmd\n    - sub/two.qmd\n    - part: \"Part Two\"\n      href: partpage.qmd\n      chapters:\n        - three.qmd\n",
    );
    write_str(
        &dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nWelcome.\n",
    );
    write_str(&dir.join("sub/two.qmd"), "# Two\n\nBody two.\n");
    write_str(
        &dir.join("partpage.qmd"),
        "---\ntitle: Part Two Page\n---\n\n![part image](part.png)\n",
    );
    write_str(&dir.join("three.qmd"), "# Three\n\nBody three.\n");
    write(&dir.join("part.png"), ONE_PIXEL_PNG);
}

/// A figure whose caption holds an image, in a root chapter and in a
/// subdirectory chapter (R9 task 5 changes how native handles these).
fn fixture_caption_image(dir: &Path) {
    write_str(
        &dir.join("_quarto.yml"),
        "project:\n  type: book\n\nkeep-typ: true\n\nbook:\n  title: \"Guard Caption\"\n  author: \"A. Author\"\n  chapters:\n    - index.qmd\n    - one.qmd\n    - sub/two.qmd\n",
    );
    write_str(
        &dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nWelcome.\n",
    );
    write_str(
        &dir.join("one.qmd"),
        "# One\n\n![A caption with ![icon](icon.png) inside.](rootlocal.png){#fig-one}\n",
    );
    write_str(
        &dir.join("sub/two.qmd"),
        "# Two\n\n![Sub caption with ![icon](sub-icon.png).](local.png){#fig-two}\n",
    );
    for f in [
        "icon.png",
        "rootlocal.png",
        "sub/sub-icon.png",
        "sub/local.png",
    ] {
        write(&dir.join(f), ONE_PIXEL_PNG);
    }
}

fn citations_yml(extra: &str) -> String {
    citations_yml_with(extra, true)
}

fn citations_yml_with(extra: &str, citeproc: bool) -> String {
    let citeproc_line = if citeproc { "citeproc: true\n" } else { "" };
    format!(
        "project:\n  type: book\n\nkeep-typ: true\n\nbook:\n  title: \"Guard Cites\"\n  author: \"A. Author\"\n  chapters:\n    - index.qmd\n    - ch1.qmd\n    - ch2.qmd\n\nbibliography: refs.bib\n{citeproc_line}{extra}"
    )
}

fn write_citation_chapters(dir: &Path) {
    write_str(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nIntro.\n");
    write_str(
        &dir.join("ch1.qmd"),
        "# Chapter One\n\nKnuth wrote it [@knuth1984].\n",
    );
    write_str(
        &dir.join("ch2.qmd"),
        "# Chapter Two\n\nDoe and Knuth [@doe2020; @knuth1984].\n",
    );
}

fn fixture_citations(dir: &Path) {
    write_str(&dir.join("_quarto.yml"), &citations_yml(""));
    write_citation_chapters(dir);
    write_str(&dir.join("refs.bib"), REFS_BIB);
}

fn fixture_citations_csl(dir: &Path) {
    write_str(
        &dir.join("_quarto.yml"),
        &citations_yml("csl: numeric.csl\n"),
    );
    write_citation_chapters(dir);
    write_str(&dir.join("refs.bib"), REFS_BIB);
    write_str(&dir.join("numeric.csl"), NUMERIC_CSL);
}

fn fixture_citations_margin(dir: &Path) {
    write_str(
        &dir.join("_quarto.yml"),
        &citations_yml_with(
            "reference-location: margin\ncitation-location: margin\nsuppress-bibliography: true\n",
            // Pandoc's own citeproc would read the root-relative bibliography
            // path the margin mode writes; margin books leave citeproc off.
            false,
        ),
    );
    write_citation_chapters(dir);
    write_str(&dir.join("refs.bib"), REFS_BIB);
}

/// The bibliography file the config names does not exist.
fn fixture_missing_bib(dir: &Path) {
    write_str(&dir.join("_quarto.yml"), &citations_yml(""));
    write_citation_chapters(dir);
}

/// The CSL file the config names does not exist.
fn fixture_missing_csl(dir: &Path) {
    write_str(
        &dir.join("_quarto.yml"),
        &citations_yml("csl: absent.csl\n"),
    );
    write_citation_chapters(dir);
    write_str(&dir.join("refs.bib"), REFS_BIB);
}

/// A copy of a smoke-all orange-book fixture, minus its stale outputs.
fn copy_smoke_fixture(name: &str) -> impl Fn(&Path) + '_ {
    move |dir: &Path| {
        let src = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../quarto/tests/smoke-all/typst")
            .join(name);
        copy_tree(&src, dir);
    }
}

fn copy_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str == "_book" || name_str == ".quarto" || name_str == ".git" {
            continue;
        }
        let from = entry.path();
        let to = dst.join(&name);
        if from.is_dir() {
            copy_tree(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

// ---------------------------------------------------------------------
// Recording
// ---------------------------------------------------------------------

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/integration/book_refactor_guard")
}

/// Compare `actual` with the recorded golden, or (re)record it when
/// `BOOK_GUARD_RECORD` is set.
fn check_golden(name: &str, actual: &str) {
    let path = golden_dir().join(format!("{name}.txt"));
    if std::env::var_os("BOOK_GUARD_RECORD").is_some() {
        write_str(&path, actual);
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "no golden at {} ({e}); record with BOOK_GUARD_RECORD=1",
            path.display()
        )
    });
    // Goldens are `eol=lf`; tolerate a CRLF checkout anyway.
    let expected = expected.replace("\r\n", "\n");
    if expected != actual {
        let got = golden_dir().join(format!("{name}.actual.txt"));
        let _ = std::fs::write(&got, actual);
        panic!(
            "native book output changed for `{name}`: diff {} against {}",
            path.display(),
            got.display()
        );
    }
}

/// Replace machine- and run-specific text with stable placeholders.
fn normalize(text: &str, root: &Path, temp: &Path) -> String {
    let mut out = text.to_string();
    for p in [root, temp] {
        let s = p.to_string_lossy().replace('\\', "/");
        out = out.replace(&s, "<ROOT>");
    }
    out = out.replace('\\', "/");
    let uuid = regex::Regex::new(r"urn:uuid:[0-9a-fA-F-]{36}").unwrap();
    out = uuid.replace_all(&out, "urn:uuid:<UUID>").into_owned();
    let uuid_bare = regex::Regex::new(
        r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}",
    )
    .unwrap();
    out = uuid_bare.replace_all(&out, "<UUID>").into_owned();
    let pipeline_tmp = regex::Regex::new(r#"[^\s'"]*quarto-pipeline_[A-Za-z0-9]+"#).unwrap();
    out = pipeline_tmp
        .replace_all(&out, "<PIPELINE_TMP>")
        .into_owned();
    let stamp = regex::Regex::new(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z").unwrap();
    out = stamp.replace_all(&out, "<TIMESTAMP>").into_owned();
    out
}

struct Rendered {
    _temp: TempDir,
    root: PathBuf,
    temp_path: PathBuf,
    outcome: Result<ProjectRenderSummary<RenderToFileResult>, String>,
}

fn render(fixture: impl FnOnce(&Path), fmt: &str) -> Rendered {
    let temp = TempDir::new().unwrap();
    let root = canonical(temp.path());
    fixture(&root);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&root, runtime.as_ref()).unwrap();
    let project_type = project_type_for(&project);
    let format = Format::from_format_string(fmt).unwrap();
    let options = RenderToFileOptions {
        execution_policy: ExecutionPolicy::None,
        ..Default::default()
    };
    let mut pipeline =
        ProjectPipeline::new(&mut project, project_type, format, fmt, &options, runtime);
    let outcome = pollster::block_on(pipeline.run_with_book_support()).map_err(|e| e.to_string());
    let temp_path = temp.path().to_path_buf();
    Rendered {
        _temp: temp,
        root,
        temp_path,
        outcome,
    }
}

/// Keep the deterministic head of a failure message: tracebacks, temp-dir
/// fragments and source-excerpt gutters vary by machine and run.
fn trim_message(msg: &str) -> String {
    msg.lines()
        .take_while(|l| !l.contains("stack traceback") && !l.contains('\u{250c}'))
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}

fn diag_lines(out: &mut String, label: &str, diags: &[quarto_error_reporting::DiagnosticMessage]) {
    writeln!(out, "{label}: {}", diags.len()).unwrap();
    for d in diags {
        writeln!(
            out,
            "  - [{:?}] {:?} {}",
            d.kind,
            d.code,
            trim_message(&d.title)
        )
        .unwrap();
    }
}

/// Everything about the render except the format-specific text.
fn describe(r: &Rendered) -> String {
    let mut out = String::new();
    match &r.outcome {
        Err(e) => {
            writeln!(out, "result: Err").unwrap();
            writeln!(out, "error: {}", trim_message(e)).unwrap();
        }
        Ok(summary) => {
            writeln!(out, "result: Ok").unwrap();
            writeln!(out, "outputs: {}", summary.outputs.len()).unwrap();
            for o in &summary.outputs {
                writeln!(
                    out,
                    "output: {}",
                    o.output_path
                        .strip_prefix(&r.root)
                        .unwrap_or(&o.output_path)
                        .display()
                )
                .unwrap();
                writeln!(
                    out,
                    "input: {}",
                    o.input_path
                        .strip_prefix(&r.root)
                        .unwrap_or(&o.input_path)
                        .display()
                )
                .unwrap();
                writeln!(
                    out,
                    "resources_dir: {}",
                    o.resources_dir
                        .strip_prefix(&r.root)
                        .unwrap_or(&o.resources_dir)
                        .display()
                )
                .unwrap();
                writeln!(
                    out,
                    "execution_skipped: {}",
                    o.render_output.execution_skipped
                )
                .unwrap();
                writeln!(out, "html_len_is_zero: {}", o.render_output.html.is_empty()).unwrap();
                diag_lines(&mut out, "diagnostics", &o.render_output.diagnostics);
                writeln!(out, "resource_report: {:?}", o.resource_report).unwrap();
            }
            writeln!(out, "pass1_failures: {}", summary.pass1_failures.len()).unwrap();
            for f in &summary.pass1_failures {
                writeln!(
                    out,
                    "  - {} :: {}",
                    f.input.display(),
                    trim_message(&f.error)
                )
                .unwrap();
            }
            writeln!(out, "pass2_failures: {}", summary.pass2_failures.len()).unwrap();
            for f in &summary.pass2_failures {
                writeln!(
                    out,
                    "  - {} :: {}",
                    f.input.display(),
                    trim_message(&f.error)
                )
                .unwrap();
                diag_lines(&mut out, "    diagnostics", &f.diagnostics);
            }
            diag_lines(
                &mut out,
                "project_diagnostics",
                &summary.project_diagnostics,
            );
            writeln!(out, "stopped_early: {}", summary.stopped_early).unwrap();

            // The output directory listing (names only: the PDF/EPUB bytes
            // are compared through the text recordings below).
            writeln!(out, "output_dir_files:").unwrap();
            let out_dir = r.root.join("_book");
            let mut names: Vec<String> = walkdir::WalkDir::new(&out_dir)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
                .map(|e| {
                    e.path()
                        .strip_prefix(&out_dir)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/")
                })
                .collect();
            names.sort();
            for n in names {
                writeln!(out, "  {n}").unwrap();
            }

            let manifest = r.root.join(".quarto/render-manifest.json");
            writeln!(out, "render_manifest:").unwrap();
            match std::fs::read_to_string(&manifest) {
                Ok(m) => writeln!(out, "{m}").unwrap(),
                Err(e) => writeln!(out, "  (none: {e})").unwrap(),
            }
        }
    }
    normalize(&out, &r.root, &r.temp_path)
}

/// `describe` plus every kept `.typ` in the output directory (present even
/// when the compile fails, so a failing render still records its source).
fn typst_snapshot(r: &Rendered) -> String {
    let mut out = describe(r);
    let out_dir = r.root.join("_book");
    let mut typs: Vec<PathBuf> = walkdir::WalkDir::new(&out_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .map(|e| e.into_path())
        .filter(|p| p.extension().is_some_and(|e| e == "typ"))
        .collect();
    typs.sort();
    for typ in typs {
        let rel = typ
            .strip_prefix(&out_dir)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        writeln!(out, "\n===== {rel} =====").unwrap();
        match std::fs::read_to_string(&typ) {
            Ok(t) => out.push_str(&normalize(&t, &r.root, &r.temp_path)),
            Err(e) => writeln!(out, "(unreadable: {e})").unwrap(),
        }
    }
    out
}

/// `describe` plus every XHTML/OPF/NCX/nav entry of the EPUB, sorted by name.
fn epub_snapshot(r: &Rendered) -> String {
    let mut out = describe(r);
    if let Ok(summary) = &r.outcome {
        for o in &summary.outputs {
            let bytes = std::fs::read(&o.output_path).unwrap();
            let mut zip = ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
            let mut names: Vec<String> = zip.file_names().map(String::from).collect();
            names.sort();
            writeln!(out, "\nepub_entries:").unwrap();
            for n in &names {
                writeln!(out, "  {n}").unwrap();
            }
            for n in &names {
                let textual = [".xhtml", ".opf", ".ncx", ".css", ".xml"]
                    .iter()
                    .any(|ext| n.ends_with(ext))
                    && !n.starts_with("EPUB/styles/");
                if !textual {
                    continue;
                }
                let mut s = String::new();
                zip.by_name(n).unwrap().read_to_string(&mut s).unwrap();
                writeln!(out, "\n===== {n} =====").unwrap();
                out.push_str(&normalize(&s, &r.root, &r.temp_path));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[test]
fn typst_basic() {
    check_golden(
        "typst-basic",
        &typst_snapshot(&render(fixture_basic, "typst")),
    );
}

#[test]
fn typst_rich() {
    check_golden(
        "typst-rich",
        &typst_snapshot(&render(fixture_rich, "typst")),
    );
}

#[test]
fn typst_partpage_image() {
    check_golden(
        "typst-partpage-image",
        &typst_snapshot(&render(fixture_partpage_image, "typst")),
    );
}

#[test]
fn epub_partpage_image() {
    check_golden(
        "epub-partpage-image",
        &epub_snapshot(&render(fixture_partpage_image, "epub")),
    );
}

#[test]
fn typst_caption_image() {
    check_golden(
        "typst-caption-image",
        &typst_snapshot(&render(fixture_caption_image, "typst")),
    );
}

#[test]
fn epub_rich() {
    check_golden("epub-rich", &epub_snapshot(&render(fixture_rich, "epub")));
}

#[test]
fn epub_basic() {
    check_golden("epub-basic", &epub_snapshot(&render(fixture_basic, "epub")));
}

#[test]
fn typst_citations() {
    check_golden(
        "typst-citations",
        &typst_snapshot(&render(fixture_citations, "typst")),
    );
}

#[test]
fn typst_citations_csl() {
    check_golden(
        "typst-citations-csl",
        &typst_snapshot(&render(fixture_citations_csl, "typst")),
    );
}

#[test]
fn typst_citations_margin() {
    check_golden(
        "typst-citations-margin",
        &typst_snapshot(&render(fixture_citations_margin, "typst")),
    );
}

#[test]
fn epub_citations() {
    check_golden(
        "epub-citations",
        &epub_snapshot(&render(fixture_citations, "epub")),
    );
}

#[test]
fn typst_missing_bibliography_message() {
    check_golden(
        "typst-missing-bib",
        &typst_snapshot(&render(fixture_missing_bib, "typst")),
    );
}

#[test]
fn typst_missing_csl_message() {
    check_golden(
        "typst-missing-csl",
        &typst_snapshot(&render(fixture_missing_csl, "typst")),
    );
}

#[test]
fn typst_smoke_orange_book() {
    check_golden(
        "typst-smoke-orange-book",
        &typst_snapshot(&render(copy_smoke_fixture("orange-book"), "typst")),
    );
}

#[test]
fn typst_smoke_orange_book_lang() {
    check_golden(
        "typst-smoke-orange-book-lang",
        &typst_snapshot(&render(copy_smoke_fixture("orange-book-lang"), "typst")),
    );
}

#[test]
fn typst_smoke_orange_book_margin() {
    check_golden(
        "typst-smoke-orange-book-margin",
        &typst_snapshot(&render(copy_smoke_fixture("orange-book-margin"), "typst")),
    );
}

#[test]
fn typst_smoke_override_orange_book() {
    check_golden(
        "typst-smoke-override-orange-book",
        &typst_snapshot(&render(copy_smoke_fixture("override-orange-book"), "typst")),
    );
}
