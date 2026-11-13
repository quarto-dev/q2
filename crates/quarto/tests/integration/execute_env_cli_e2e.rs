//! GH #1131 (bd-8fmuyh4d): code cells see the execute environment Quarto 1
//! gives them — `QUARTO_PROJECT_ROOT`, `QUARTO_DOCUMENT_PATH`,
//! `QUARTO_DOCUMENT_FILE`, `QUARTO_EXECUTE_INFO` and `QUARTO_FIG_*`.
//!
//! These drive the real `q2` binary. The execute-info file lives in the
//! render's temp dir and is gone when the render ends, so each cell copies
//! what it saw (the variables, and the file) next to its document, and the
//! test reads those copies.
//!
//! Gated on a real toolchain like the other engine e2e tests: knitr needs
//! `Rscript` with the `knitr` package; Jupyter needs a `python3` kernel. A
//! skip is an environment signal, not a pass.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::jupyter_kernel_cleanup_e2e::{jupyter_available, python3_kernel_available};
use quarto_core::engine::ts_process::node_available_for_tests;

use crate::render_cli_e2e::{knitr_r_package_available, rscript_available};

const BRAND: &str = "color:\n  background:\n    light: \"#ffffff\"\n    dark: \"#1b1b1b\"\n  foreground:\n    light: \"#1b1b1b\"\n    dark: \"#ffffff\"\n";

fn render(dir: &Path) -> std::process::Output {
    let output = Command::new(env!("CARGO_BIN_EXE_q2"))
        .arg("render")
        .arg(dir)
        .output()
        .expect("q2 render runs");
    assert!(
        output.status.success(),
        "q2 render failed.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    output
}

fn read_json(path: &Path) -> Value {
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

/// The real directory of `dir` (macOS reports `/private/var/...` for a
/// `/var/...` temp dir); q2 resolves paths through the same filesystem.
fn canonical(dir: &Path) -> std::path::PathBuf {
    std::fs::canonicalize(dir).unwrap()
}

/// Assert the copied variables describe `doc` in `project`.
fn assert_paths(env: &Value, project: &Path, doc_dir: &Path, doc_file: &str) {
    let path_of = |key: &str| {
        let raw = env[key]
            .as_str()
            .unwrap_or_else(|| panic!("{key} missing from {env}"));
        std::fs::canonicalize(raw).unwrap_or_else(|e| panic!("{key}={raw}: {e}"))
    };
    assert_eq!(path_of("QUARTO_PROJECT_ROOT"), canonical(project));
    assert_eq!(path_of("QUARTO_DOCUMENT_PATH"), canonical(doc_dir));
    assert_eq!(env["QUARTO_DOCUMENT_FILE"], Value::from(doc_file));
}

#[test]
fn knitr_cells_see_the_execute_environment() {
    if !rscript_available() || !knitr_r_package_available() {
        eprintln!("skipping: R toolchain with knitr not available");
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("proj");
    let sub = project.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(
        project.join("_quarto.yml"),
        // The brand is declared in front matter: a project-level `brand:` with
        // a document in a subdirectory hits bd-08hfh3ut once code runs.
        "project:\n  type: default\n",
    )
    .unwrap();
    std::fs::write(project.join("_brand.yml"), BRAND).unwrap();
    // R writes the variables as a JSON object by hand (no jsonlite needed)
    // and copies the info file.
    std::fs::write(
        sub.join("doc.qmd"),
        r#"---
title: knitr
engine: knitr
brand: ../_brand.yml
execute:
  fig-width: 5.5
---

```{r}
keys <- c("QUARTO_PROJECT_ROOT", "QUARTO_DOCUMENT_PATH", "QUARTO_DOCUMENT_FILE",
          "QUARTO_EXECUTE_INFO", "QUARTO_FIG_WIDTH", "QUARTO_FIG_HEIGHT",
          "QUARTO_FIG_DPI", "QUARTO_FIG_FORMAT")
vals <- Sys.getenv(keys)
esc <- function(s) gsub('"', '\\"', gsub("\\\\", "\\\\\\\\", s))
json <- paste0("{", paste0('"', keys, '": "', esc(vals), '"', collapse = ", "), "}")
out <- Sys.getenv("QUARTO_DOCUMENT_PATH")
writeLines(json, file.path(out, "env.json"))
file.copy(Sys.getenv("QUARTO_EXECUTE_INFO"), file.path(out, "info.json"), overwrite = TRUE)
```
"#,
    )
    .unwrap();

    render(&project);

    let env = read_json(&sub.join("env.json"));
    assert_paths(&env, &project, &sub, "doc.qmd");
    assert_eq!(env["QUARTO_FIG_WIDTH"], Value::from("5.5"));
    assert_eq!(env["QUARTO_FIG_HEIGHT"], Value::from("5"));
    assert_eq!(env["QUARTO_FIG_DPI"], Value::from("96"));
    assert_eq!(env["QUARTO_FIG_FORMAT"], Value::from("png"));

    let info = read_json(&sub.join("info.json"));
    assert_eq!(
        std::fs::canonicalize(info["document-path"].as_str().unwrap()).unwrap(),
        canonical(&sub.join("doc.qmd"))
    );
    let format = &info["format"];
    assert_eq!(format["identifier"]["target-format"], Value::from("html"));
    assert_eq!(format["identifier"]["base-format"], Value::from("html"));
    assert_eq!(format["execute"]["fig-width"], Value::from(5.5));
    assert_eq!(format["metadata"]["title"], Value::from("knitr"));
    assert!(
        format["language"]["callout-tip-title"].is_string(),
        "language bin carries resolved terms: {}",
        format["language"]
    );
    let brand = &format["render"]["brand"];
    assert_eq!(
        brand["light"]["data"]["color"]["background"],
        Value::from("#ffffff")
    );
    assert_eq!(
        brand["dark"]["data"]["color"]["background"],
        Value::from("#1b1b1b")
    );
}

#[test]
fn jupyter_cells_see_their_own_document() {
    if !jupyter_available() {
        eprintln!("skipping: jupyter not available");
        return;
    }
    if !python3_kernel_available() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("proj");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("_quarto.yml"), "project:\n  type: default\n").unwrap();
    // Two documents in one directory, rendered in parallel: each must see
    // its own values (each runs on its own kernel since bd-i7yz08uh).
    let cell = r#"```{python}
import json, os, shutil
keys = ["QUARTO_PROJECT_ROOT", "QUARTO_DOCUMENT_PATH", "QUARTO_DOCUMENT_FILE",
        "QUARTO_EXECUTE_INFO", "QUARTO_FIG_WIDTH", "QUARTO_FIG_HEIGHT",
        "QUARTO_FIG_DPI", "QUARTO_FIG_FORMAT"]
out = os.environ["QUARTO_DOCUMENT_PATH"]
stem = os.path.splitext(os.environ["QUARTO_DOCUMENT_FILE"])[0]
with open(os.path.join(out, stem + ".env.json"), "w") as f:
    json.dump({k: os.environ.get(k) for k in keys}, f)
shutil.copy(os.environ["QUARTO_EXECUTE_INFO"], os.path.join(out, stem + ".info.json"))
```
"#;
    for doc in ["a", "b"] {
        std::fs::write(
            project.join(format!("{doc}.qmd")),
            format!("---\ntitle: {doc}\nengine: jupyter\n---\n\n{cell}"),
        )
        .unwrap();
    }

    render(&project);

    for doc in ["a", "b"] {
        let env = read_json(&project.join(format!("{doc}.env.json")));
        assert_paths(&env, &project, &project, &format!("{doc}.qmd"));
        assert_eq!(env["QUARTO_FIG_WIDTH"], Value::from("7"));
        let info = read_json(&project.join(format!("{doc}.info.json")));
        assert_eq!(info["format"]["metadata"]["title"], Value::from(doc));
    }
}

// ── bd-x3h4zjhl: TS engines (marimo, julia) get the same environment ──────────
//
// Both tests build a project with a default profile (`prod`) and an
// `_environment` / `_environment-prod` value, and never set `QUARTO_PROFILE`
// on the test process: the engine must see it because q2 passed it.

const PROJECT_ENV_KEYS: &str = "QUARTO_PROJECT_ROOT QUARTO_DOCUMENT_PATH QUARTO_DOCUMENT_FILE \
QUARTO_EXECUTE_INFO QUARTO_FIG_WIDTH QUARTO_FIG_HEIGHT QUARTO_FIG_DPI QUARTO_FIG_FORMAT \
QUARTO_PROFILE X3H_FROM_ENV X3H_FROM_PROFILE";

fn profile_project(project: &Path) {
    std::fs::create_dir_all(project.join("sub")).unwrap();
    std::fs::write(
        project.join("_quarto.yml"),
        "project:\n  type: default\nprofile:\n  default: prod\n",
    )
    .unwrap();
    std::fs::write(project.join("_environment"), "X3H_FROM_ENV=base value\n").unwrap();
    std::fs::write(
        project.join("_environment-prod"),
        "X3H_FROM_PROFILE=prod value\n",
    )
    .unwrap();
}

/// Assert on every variable the cell copied, plus `document-path` in the info file.
fn assert_ts_engine_env(project: &Path, sub: &Path) {
    let env = read_json(&sub.join("env.json"));
    eprintln!("env.json seen by the cell: {env}");
    assert_paths(&env, project, sub, "doc.qmd");
    assert_eq!(env["QUARTO_FIG_WIDTH"], Value::from("5.5"));
    assert_eq!(env["QUARTO_FIG_HEIGHT"], Value::from("5"));
    assert_eq!(env["QUARTO_FIG_DPI"], Value::from("96"));
    assert_eq!(env["QUARTO_FIG_FORMAT"], Value::from("png"));
    assert_eq!(env["QUARTO_PROFILE"], Value::from("prod"));
    assert_eq!(env["X3H_FROM_ENV"], Value::from("base value"));
    assert_eq!(env["X3H_FROM_PROFILE"], Value::from("prod value"));
    let info = read_json(&sub.join("info.json"));
    assert_eq!(
        std::fs::canonicalize(info["document-path"].as_str().unwrap()).unwrap(),
        canonical(&sub.join("doc.qmd"))
    );
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let to = dst.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &to);
        } else {
            std::fs::copy(entry.path(), &to).unwrap();
        }
    }
}

fn uv_available() -> bool {
    Command::new("uv")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn marimo_cells_see_the_execute_environment() {
    if !node_available_for_tests() || !uv_available() {
        eprintln!("skipping: node + uv not both available");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("proj");
    profile_project(&project);
    let sub = project.join("sub");
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../quarto-core/tests/fixtures/extensions/marimo/_extensions/marimo"),
        &project.join("_extensions/marimo"),
    );
    std::fs::write(
        sub.join("doc.qmd"),
        format!(
            r#"---
title: marimo
engine: marimo
execute:
  fig-width: 5.5
---

```{{python .marimo}}
import json, os, shutil
keys = "{PROJECT_ENV_KEYS}".split()
out = os.environ["QUARTO_DOCUMENT_PATH"]
with open(os.path.join(out, "env.json"), "w") as f:
    json.dump({{k: os.environ.get(k) for k in keys}}, f)
shutil.copy(os.environ["QUARTO_EXECUTE_INFO"], os.path.join(out, "info.json"))
```
"#
        ),
    )
    .unwrap();

    render(&project);
    assert_ts_engine_env(&project, &sub);
}

/// `julia` and `node` on `PATH`, and a julia environment with a `Manifest.toml`
/// (`QUARTO_JULIA_PROJECT`, else the engine's runtime directory). A copy of the
/// helper in `julia_targets_cli.rs`.
fn julia_available() -> bool {
    let ok = |program: &str| {
        Command::new(program)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    };
    if !ok("julia") || !ok("node") {
        return false;
    }
    std::env::var_os("QUARTO_JULIA_PROJECT")
        .map(PathBuf::from)
        .or_else(|| {
            quarto_util::quarto_runtime_dir()
                .ok()
                .map(|d| d.join("julia"))
        })
        .is_some_and(|dir| dir.join("Manifest.toml").exists())
}

#[test]
#[ignore = "bd-x3h4zjhl: needs julia fork sync (Session 2)"]
fn julia_cells_see_the_execute_environment() {
    if !julia_available() {
        eprintln!("skipping: julia, node and a julia environment not all available");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("proj");
    profile_project(&project);
    let sub = project.join("sub");
    // Julia has no JSON package in the cell environment: write the object by hand.
    std::fs::write(
        sub.join("doc.qmd"),
        format!(
            r#"---
title: julia
engine: julia
execute:
  fig-width: 5.5
  daemon: false
---

```{{julia}}
keys = split("{PROJECT_ENV_KEYS}")
out = ENV["QUARTO_DOCUMENT_PATH"]
esc(s) = replace(replace(s, "\\" => "\\\\"), "\"" => "\\\"")
pairs = ["\"$k\": \"$(esc(get(ENV, k, "")))\"" for k in keys]
write(joinpath(out, "env.json"), "{{" * join(pairs, ", ") * "}}")
cp(ENV["QUARTO_EXECUTE_INFO"], joinpath(out, "info.json"); force = true)
```
"#
        ),
    )
    .unwrap();

    render(&project);
    assert_ts_engine_env(&project, &sub);
}
