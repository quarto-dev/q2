/*
 * tests/integration/lua_host_context_cli.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * What a Lua filter or shortcode learns about where it is running
 * (GH #939, #940): `quarto.project`, `quarto.doc.input_file` /
 * `output_file`, `PANDOC_STATE`, and `os.getenv` over the host's
 * environment snapshot (`--profile` visible as `QUARTO_PROFILE`,
 * `_environment` values visible, real environment winning).
 *
 * End to end through the `q2` binary because the values under test are
 * assembled from the project context, the document, the resolved output
 * path, and the CLI flags — exactly the plumbing a unit test would stub.
 */

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

/// A filter that appends a code block of `key=value` lines describing
/// where it runs. `tostring` so nils print as `nil`.
const PROBE_LUA: &str = r#"
function Pandoc(doc)
  local function line(k, v) return k .. "=" .. tostring(v) end
  local lines = {
    line("directory", quarto.project.directory),
    line("offset", quarto.project.offset),
    line("output_directory", quarto.project.output_directory),
    line("profile", table.concat(quarto.project.profile, ",")),
    line("profile_type", pandoc.utils.type(quarto.project.profile)),
    line("QUARTO_PROFILE", os.getenv("QUARTO_PROFILE")),
    line("Q2_TEST_REAL", os.getenv("Q2_TEST_REAL")),
    line("Q2_TEST_FILE_ONLY", os.getenv("Q2_TEST_FILE_ONLY")),
    line("Q2_TEST_UNSET", os.getenv("Q2_TEST_UNSET")),
    line("input_file", quarto.doc.input_file),
    line("output_file", quarto.doc.output_file),
    line("state_input", PANDOC_STATE.input_files[1]),
    line("state_output", PANDOC_STATE.output_file),
  }
  doc.blocks:insert(pandoc.CodeBlock(table.concat(lines, "\n"), pandoc.Attr("probe")))
  return doc
end
"#;

/// A shortcode handler reporting the same things from the shortcode
/// environment.
const WHEREAMI_LUA: &str = r#"
return {
  ["whereami"] = function(args, kwargs, meta)
    return pandoc.Str("SC-OFFSET=" .. tostring(quarto.project.offset)
      .. ";SC-PROFILE=" .. tostring(os.getenv("QUARTO_PROFILE")))
  end
}
"#;

/// The `key=value` lines of the probe's code block in a rendered page.
fn probe_values(html: &str) -> HashMap<String, String> {
    let start = html
        .find("id=\"probe\"")
        .unwrap_or_else(|| panic!("no probe block in:\n{html}"));
    let body = &html[start..];
    let open = body.find('>').unwrap() + 1;
    let close = body.find("</code>").unwrap();
    body[open..close]
        .lines()
        .filter_map(|l| {
            let text = strip_tags(l);
            let (k, v) = text.split_once('=')?;
            Some((k.to_string(), v.to_string()))
        })
        .collect()
}

/// The syntax highlighter may wrap lines in spans; drop any tags.
fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    html_escape_undo(&out)
}

fn html_escape_undo(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn q2(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(Q2_BIN)
        .current_dir(root)
        .args(args)
        .env_remove("QUARTO_PROFILE")
        .env_remove("Q2_TEST_FILE_ONLY")
        .env_remove("Q2_TEST_UNSET")
        .env("Q2_TEST_REAL", "real")
        .output()
        .expect("q2 runs")
}

#[test]
fn website_pages_see_project_paths_profiles_and_environment() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    // The filter is named by absolute path: a relative `probe.lua` resolves
    // against each page's own directory today (GH #945), which is not what
    // this test is about.
    let probe = root.join("probe.lua");
    write(root, "probe.lua", PROBE_LUA);
    write(root, "whereami.lua", WHEREAMI_LUA);
    write(
        root,
        "_quarto.yml",
        &format!(
            "project:\n  type: website\nprofile:\n  group:\n    - [drafts, release]\nfilters:\n  - {}\n",
            probe.display()
        ),
    );
    write(
        root,
        "_environment",
        "Q2_TEST_REAL=file\nQ2_TEST_FILE_ONLY=file\n",
    );
    write(
        root,
        "index.qmd",
        "---\ntitle: Index\nshortcodes: [whereami.lua]\n---\n\n{{< whereami >}}\n",
    );
    write(root, "sub/page.qmd", "---\ntitle: Sub\n---\n\nhello\n");

    let out = q2(root, &["render", "--profile", "drafts"]);
    assert!(
        out.status.success(),
        "render failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let index_html = std::fs::read_to_string(root.join("_site").join("index.html")).unwrap();
    let index = probe_values(&index_html);
    let sub = probe_values(
        &std::fs::read_to_string(root.join("_site").join("sub").join("page.html")).unwrap(),
    );

    // quarto.project: the same project from both pages, offsets differ.
    assert_eq!(index["offset"], ".");
    assert_eq!(sub["offset"], "..");
    assert_eq!(index["directory"], sub["directory"]);
    assert!(
        Path::new(&index["directory"]).is_absolute(),
        "{}",
        index["directory"]
    );
    assert_eq!(
        Path::new(&index["output_directory"]),
        Path::new(&index["directory"]).join("_site")
    );
    assert_eq!(index["profile"], "drafts");
    assert_eq!(index["profile_type"], "List");

    // os.getenv over the snapshot: --profile is QUARTO_PROFILE, the real
    // environment beats `_environment`, `_environment` fills gaps, and an
    // unset variable is nil.
    assert_eq!(index["QUARTO_PROFILE"], "drafts");
    assert_eq!(index["Q2_TEST_REAL"], "real");
    assert_eq!(index["Q2_TEST_FILE_ONLY"], "file");
    assert_eq!(index["Q2_TEST_UNSET"], "nil");

    // quarto.doc.input_file / output_file and the minimal PANDOC_STATE:
    // absolute, per page, consistent with quarto.project. Built with
    // Path::join, never literal separators (Windows).
    let dir = Path::new(&index["directory"]);
    assert_eq!(Path::new(&index["input_file"]), dir.join("index.qmd"));
    assert_eq!(
        Path::new(&sub["input_file"]),
        dir.join("sub").join("page.qmd")
    );
    assert_eq!(
        Path::new(&index["output_file"]),
        dir.join("_site").join("index.html")
    );
    assert_eq!(
        Path::new(&sub["output_file"]),
        dir.join("_site").join("sub").join("page.html")
    );
    assert_eq!(sub["state_input"], sub["input_file"]);
    assert_eq!(sub["state_output"], sub["output_file"]);

    // The shortcode environment sees the same things.
    assert!(
        index_html.contains("SC-OFFSET=.;SC-PROFILE=drafts"),
        "{index_html}"
    );
}

/// GH #1013: outside a project, Q1 treats the document's directory as the
/// project (its single-file context), so `quarto.project`'s paths are the
/// document's directory and the offset is `"."`, never nil. The document
/// sits in a subdirectory of the working directory so the project is the
/// document's directory, not the cwd.
#[test]
fn single_file_render_uses_document_dir_as_project_and_no_profiles() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write(root, "sub/probe.lua", PROBE_LUA);
    write(
        root,
        "sub/doc.qmd",
        "---\ntitle: Doc\nfilters: [probe.lua]\n---\n\nhello\n",
    );

    let out = q2(
        root,
        &[
            "render",
            &Path::new("sub").join("doc.qmd").to_string_lossy(),
        ],
    );
    assert!(
        out.status.success(),
        "render failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let doc = probe_values(&std::fs::read_to_string(root.join("sub").join("doc.html")).unwrap());
    // Paths are known outside a project.
    let input = Path::new(&doc["input_file"]);
    assert!(
        input.is_absolute() && input.ends_with("doc.qmd"),
        "{input:?}"
    );
    assert_eq!(
        Path::new(&doc["output_file"]),
        input.with_file_name("doc.html")
    );
    assert_eq!(doc["state_input"], doc["input_file"]);
    // quarto.project: the document's directory, as in Q1.
    let doc_dir = input.parent().unwrap();
    assert_eq!(Path::new(&doc["directory"]), doc_dir);
    assert_eq!(Path::new(&doc["output_directory"]), doc_dir);
    assert_eq!(doc["offset"], ".");
    assert_eq!(doc["profile"], "");
    assert_eq!(doc["profile_type"], "List");
    assert_eq!(doc["QUARTO_PROFILE"], "nil");
    assert_eq!(doc["Q2_TEST_REAL"], "real");
    // No project, no `_environment`: file-only variables do not exist.
    assert_eq!(doc["Q2_TEST_FILE_ONLY"], "nil");
}
