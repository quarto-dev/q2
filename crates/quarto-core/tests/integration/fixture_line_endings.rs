/*
 * fixture_line_endings.rs
 *
 * Guard for the LF pin on fixtures the hub-client harness specs compare
 * byte-for-byte (GH #1347).
 *
 * `hub-client/e2e/pandoc-import-emf.harness.spec.ts` compares imports with
 * the `expected.qmd` files in `import-recordings`, and `pandoc-parity.harness.spec.ts`
 * renders the `pandoc-goldens` .qmd sources. A CRLF checkout (Windows with
 * `core.autocrlf`) breaks both, so `.gitattributes` pins their text files to
 * `eol=lf`. These tests assert the pin exists and holds in the working tree.
 *
 * Copyright (c) 2026 Posit, PBC
 */

use std::path::{Path, PathBuf};
use std::process::Command;

/// Fixture directories (relative to the repo root) whose text files must be LF.
const LF_DIRS: &[&str] = &[
    "crates/quarto-core/tests/fixtures/import-recordings",
    "crates/quarto-core/tests/fixtures/pandoc-goldens",
];

/// Extensions treated as text in those directories.
const TEXT_EXTENSIONS: &[&str] = &["qmd", "md", "txt", "json", "rtf"];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/quarto-core should be two levels below the repo root")
        .to_path_buf()
}

fn collect_text_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("fixture directory should be readable") {
        let path = entry.expect("readable dir entry").path();
        if path.is_dir() {
            collect_text_files(&path, out);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| TEXT_EXTENSIONS.contains(&e))
        {
            out.push(path);
        }
    }
}

/// Text fixtures, as `/`-separated paths relative to the repo root, sorted.
fn text_fixtures(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    for dir in LF_DIRS {
        collect_text_files(&root.join(dir), &mut files);
    }
    let mut rel: Vec<String> = files
        .iter()
        .map(|p| {
            p.strip_prefix(root)
                .expect("fixture under repo root")
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        })
        .collect();
    rel.sort();
    assert!(!rel.is_empty(), "found no text fixtures under {LF_DIRS:?}");
    rel
}

#[test]
fn harness_fixtures_are_pinned_to_lf_in_gitattributes() {
    let root = repo_root();
    let in_git = Command::new("git")
        .current_dir(&root)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .is_ok_and(|o| o.status.success());
    if !in_git {
        eprintln!("skipping: {} is not a git work tree", root.display());
        return;
    }

    let files = text_fixtures(&root);
    let output = Command::new("git")
        .current_dir(&root)
        .args(["check-attr", "eol", "--"])
        .args(&files)
        .output()
        .expect("failed to run git check-attr");
    assert!(
        output.status.success(),
        "git check-attr failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Each line is `<path>: eol: <value>`.
    let unpinned: Vec<&str> = std::str::from_utf8(&output.stdout)
        .expect("utf-8 git output")
        .lines()
        .filter(|line| !line.ends_with(": eol: lf"))
        .collect();
    assert!(
        unpinned.is_empty(),
        "these harness fixtures are not pinned to `eol=lf` in .gitattributes, \
         so a Windows checkout can convert them to CRLF (#1347):\n{}",
        unpinned.join("\n"),
    );
}

#[test]
fn harness_fixtures_use_lf_line_endings() {
    let root = repo_root();
    let offenders: Vec<String> = text_fixtures(&root)
        .into_iter()
        .filter(|rel| {
            std::fs::read(root.join(rel))
                .expect("fixture should be readable")
                .windows(2)
                .any(|w| w == b"\r\n")
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "these harness fixtures contain CRLF line endings: {}\n\
         They must check out with LF; see the import-recordings / pandoc-goldens \
         `eol=lf` rules in .gitattributes.",
        offenders.join(", "),
    );
}
