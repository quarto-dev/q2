/*
 * tests/integration/toc_component_render.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end pins for the TOC's client side through the real binary
 * (bd-toc-scrollspy-js-41e624d8): the default script on one page, the
 * `toc-component:` binding on another, from the checked-in fixture that
 * also serves the browser hand check.
 */

//! Renders `tests/integration/fixtures/components-toc/` with the real
//! binary. `plain.qmd` ships `quarto-toc.js` and `toc-expand: 2`;
//! `index.qmd` binds the nav to `toc.tsx` and ships no script. The
//! pipeline-level details (manifest contents, the sidebar twin) are in
//! quarto-core's `toc_js_pipeline.rs`; this test keeps the fixture
//! rendering, so `q2 preview --static` on it stays a valid browser check.

use std::path::{Path, PathBuf};
use std::process::Command;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/integration/fixtures/components-toc")
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let target = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

struct Rendered {
    dir: tempfile::TempDir,
    stderr: String,
}

impl Rendered {
    fn site(&self) -> PathBuf {
        self.dir.path().join("_site")
    }
    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.site().join(rel))
            .unwrap_or_else(|e| panic!("{rel}: {e}\nstderr:\n{}", self.stderr))
    }
}

fn render() -> Rendered {
    let dir = tempfile::TempDir::new().unwrap();
    copy_dir(&fixture(), dir.path());
    let output = Command::new(Q2_BIN)
        .arg("render")
        .current_dir(dir.path())
        .env("NO_COLOR", "1")
        .output()
        .expect("q2 runs");
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "render failed: {:?}\nstdout:\n{}\nstderr:\n{stderr}",
        output.status,
        String::from_utf8_lossy(&output.stdout)
    );
    Rendered { dir, stderr }
}

#[test]
fn default_page_ships_the_script_and_the_component_page_does_not() {
    let r = render();
    assert!(
        !r.stderr.contains("Q-25-") && !r.stderr.contains("Q-13-"),
        "no component or navigation diagnostics expected:\n{}",
        r.stderr
    );

    let plain = r.read("plain.html");
    assert!(
        plain.contains("<script src=\"site_libs/quarto-toc/quarto-toc.js\"></script>"),
        "plain.html links the script:\n{plain}"
    );
    assert!(plain.contains("<ul data-toc-expanded=\"2\">"), "{plain}");
    assert!(
        plain.contains("<nav id=\"TOC\" role=\"doc-toc\" class=\"toc-active\">"),
        "plain.html's nav carries no host attribute:\n{plain}"
    );
    assert!(
        !plain.contains("quarto-components-manifest"),
        "plain.html binds no component:\n{plain}"
    );

    let index = r.read("index.html");
    assert!(
        !index.contains("quarto-toc.js"),
        "index.html must not link the default script:\n{index}"
    );
    assert!(
        index.contains(
            "<nav id=\"TOC\" role=\"doc-toc\" class=\"toc-active\" data-quarto-component=\"quarto-toc\">"
        ),
        "index.html's nav is the component host:\n{index}"
    );
    assert!(
        index.contains("\"quarto-toc\":{\"content\":\"dom\",\"export\":\"default\",\"module\":\"site_libs/quarto-components/toc.js\""),
        "manifest names the TOC component:\n{index}"
    );
    assert!(index.contains("\"id\":\"alpha\""), "{index}");

    for rel in [
        "site_libs/quarto-toc/quarto-toc.js",
        "site_libs/quarto-components/toc.js",
        "site_libs/quarto-components/quarto-components.js",
        "site_libs/quarto-react/react.js",
    ] {
        assert!(r.site().join(rel).exists(), "missing {rel}");
    }
    // The transpiled component kept its JSX-free shape and reads `props`.
    let module = r.read("site_libs/quarto-components/toc.js");
    assert!(module.contains("props.entries"), "{module}");
    assert!(!module.contains("<ol>"), "JSX must be compiled: {module}");
}
