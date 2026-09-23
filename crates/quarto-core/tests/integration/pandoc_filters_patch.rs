//! Task 6: carry P3's crossref-numbering patch (upstream PR
//! quarto-dev/quarto-cli#14913) into q2's vendored Q1 filter tree, with
//! markers and a re-vendor tripwire.
//!
//! T6.1a/T6.1b/T6.1c per the P3 Task 6 test seam spec
//! (`.superpowers/sdd/2026-09-18-pandoc-hybrid-P3-implementation/task-6-brief.md`).

use std::path::{Path, PathBuf};

use walkdir::WalkDir;

/// Every site the patch touches carries a comment starting with this
/// prefix, followed by the real upstream PR number as digits.
const MARKER_PREFIX: &str = "QUARTO-PATCH(upstream PR quarto-dev/quarto-cli#";

fn filters_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate dir has a parent")
        .parent()
        .expect("crates/ has a parent")
        .join("resources/pandoc-filters/filters")
}

fn read_filter(rel: &str) -> String {
    let path = filters_root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {path:?}: {e}"))
}

/// The exact set of files that carry a `QUARTO-PATCH(upstream PR
/// quarto-dev/quarto-cli#14913)` marker for this patch. Kept as a
/// hard-coded list (not derived from the walk) so that a stray marker
/// showing up in an unexpected file reddens this test.
const EXPECTED_MARKER_FILES: &[&str] = &[
    "main.lua",
    "customnodes/floatreftarget.lua",
    "modules/callouts.lua",
    "modules/crossref_numbering.lua",
    "modules/import_all.lua",
    "crossref/format.lua",
];

/// T6.1a: the vendored tree's marker inventory.
///
/// Revert hunk: delete the `QUARTO-PATCH(...)` comment from
/// `modules/callouts.lua` -> the "every file in the expected list carries
/// a marker" assertion below goes RED.
///
/// Revert hunk: leave a marker as the literal `QUARTO-PATCH(upstream PR
/// #<N>)` placeholder -> the digits-after-`#` assertion below goes RED.
#[test]
fn test_marker_inventory() {
    let root = filters_root();
    assert!(root.is_dir(), "{root:?} should be a directory");

    let mut files_with_marker: Vec<String> = Vec::new();
    for entry in WalkDir::new(&root) {
        let entry = entry.expect("walking the vendored filters tree should not fail");
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("lua") {
            continue;
        }
        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("failed to read {path:?}: {e}"));
        if content.contains(MARKER_PREFIX) {
            let rel = path
                .strip_prefix(&root)
                .expect("walked path is under root")
                .to_string_lossy()
                .replace('\\', "/");
            files_with_marker.push(rel);
        }
    }
    files_with_marker.sort();

    let mut expected: Vec<String> = EXPECTED_MARKER_FILES
        .iter()
        .map(|s| s.to_string())
        .collect();
    expected.sort();

    assert_eq!(
        files_with_marker, expected,
        "the set of files carrying a QUARTO-PATCH(upstream PR quarto-dev/quarto-cli#...) \
         marker no longer matches the expected list for this patch"
    );

    // Every marker instance must be followed by real digits, not the
    // literal `#<N>` placeholder.
    for rel in EXPECTED_MARKER_FILES {
        let content = read_filter(rel);
        let mut any_marker = false;
        for (idx, _) in content.match_indices(MARKER_PREFIX) {
            any_marker = true;
            let after = &content[idx + MARKER_PREFIX.len()..];
            let digit_count = after.chars().take_while(|c| c.is_ascii_digit()).count();
            assert!(
                digit_count > 0,
                "{rel}: marker at byte {idx} is not followed by a real PR number \
                 (looks like the `#<N>` placeholder was left in place)"
            );
        }
        assert!(
            any_marker,
            "{rel} is in the expected list but has no marker"
        );
    }
}

/// T6.1b: the vendored `main.lua`'s assignment gate, structurally.
///
/// Revert hunk: re-vendor `main.lua` from the pinned tag (overwriting the
/// patch) -> the file again contains `if enableCrossRef then` -> the
/// negative assertion below goes RED.
#[test]
fn test_main_lua_assignment_gate_converted() {
    let content = read_filter("main.lua");

    // Positive existence assertion first: prove we're looking at the real
    // gate site, not a typo'd path that happens to read as "doesn't
    // contain the bad string" for the wrong reason.
    assert!(
        content.contains("tappend(quarto_filter_list, quarto_crossref_filters)"),
        "main.lua does not contain the crossref filter splice; \
         is this the right vendored path?"
    );

    assert!(
        content.contains("_quarto.modules.crossref_numbering.assign_crossref_numbers()"),
        "main.lua's gate should call assign_crossref_numbers()"
    );
    assert!(
        !content.contains("if enableCrossRef then"),
        "main.lua still contains the old bare `if enableCrossRef then` gate \
         -- did a re-vendor overwrite the patch?"
    );
}

/// T6.1c: the vendored tree's whole gate surface, exhaustively.
///
/// Revert hunk: re-vendor `floatreftarget.lua` -> three extra
/// `param("enable-crossref"` occurrences appear -> the occurrence-set
/// assertion below goes RED.
///
/// Revert hunk: upstream adds a fifth `param("enable-crossref"` gate,
/// picked up by a re-pin -> same assertion goes RED, with the new site
/// named in the failure output (it's simply not in `ALLOWED_OCCURRENCES`).
#[test]
fn test_enable_crossref_param_reads_are_exhaustively_allow_listed() {
    // Positive existence assertions: prove crossref_present() actually
    // landed in the two files whose call sites were converted.
    for rel in ["customnodes/floatreftarget.lua", "modules/callouts.lua"] {
        let content = read_filter(rel);
        assert!(
            content.contains("crossref_present()"),
            "{rel} should call _quarto.modules.crossref_numbering.crossref_present()"
        );
    }

    // The allow-list of *permitted* remaining direct reads of the raw
    // param. `layout/ipynb.lua` is deliberately left unconverted (the two
    // predicates there are a verified no-op regardless of which flag they
    // read); `modules/crossref_numbering.lua` is the module that must read
    // the raw param to implement the predicates in the first place.
    const ALLOWED_OCCURRENCES: &[(&str, usize)] = &[
        ("layout/ipynb.lua", 2),
        ("modules/crossref_numbering.lua", 2),
    ];

    let needle = "param(\"enable-crossref\"";
    let root = filters_root();
    let mut actual: Vec<(String, usize)> = Vec::new();
    for entry in WalkDir::new(&root) {
        let entry = entry.expect("walking the vendored filters tree should not fail");
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("lua") {
            continue;
        }
        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("failed to read {path:?}: {e}"));
        let count = content.matches(needle).count();
        if count > 0 {
            let rel = path
                .strip_prefix(&root)
                .expect("walked path is under root")
                .to_string_lossy()
                .replace('\\', "/");
            actual.push((rel, count));
        }
    }
    actual.sort();

    let mut expected: Vec<(String, usize)> = ALLOWED_OCCURRENCES
        .iter()
        .map(|(f, n)| (f.to_string(), *n))
        .collect();
    expected.sort();

    assert_eq!(
        actual, expected,
        "the set of remaining direct `param(\"enable-crossref\"` reads in the \
         vendored tree no longer matches the allow-list -- a new site was \
         either added (needs converting or allow-listing) or removed \
         (shrink the allow-list)"
    );
}
